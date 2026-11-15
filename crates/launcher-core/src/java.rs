//! Java runtime handling: download Mojang's official JREs, probe a given `java`.

use crate::config::Dirs;
use crate::download::{DownloadTask, Downloader, ProgressFn};
use crate::version::Artifact;
use crate::{paths, Error, Result};
use regex::Regex;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const RUNTIME_ALL_URL: &str = "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

/// Component used when a version JSON has no `javaVersion` (everything before 1.17).
pub const LEGACY_COMPONENT: &str = "jre-legacy";
pub const LEGACY_MAJOR: u32 = 8;

#[derive(Debug, Deserialize)]
struct RuntimeEntry {
    manifest: Artifact,
}

type AllRuntimes = HashMap<String, HashMap<String, Vec<RuntimeEntry>>>;

#[derive(Debug, Deserialize)]
struct RuntimeManifest {
    files: HashMap<String, RuntimeFile>,
}

#[derive(Debug, Deserialize)]
struct RuntimeFile {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    downloads: Option<RuntimeDownloads>,
    #[serde(default)]
    executable: bool,
}

#[derive(Debug, Deserialize)]
struct RuntimeDownloads {
    raw: Option<Artifact>,
}

/// Key used by Mojang's runtime index for this machine.
pub fn platform_key() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "windows-x64",
        ("windows", "x86") => "windows-x86",
        ("windows", "aarch64") => "windows-arm64",
        ("macos", "aarch64") => "mac-os-arm64",
        ("macos", _) => "mac-os",
        (_, "x86") => "linux-i386",
        _ => "linux",
    }
}

pub fn java_exe_name() -> &'static str {
    if cfg!(windows) {
        "java.exe"
    } else {
        "java"
    }
}

/// Path of the `java` executable for an installed component.
pub fn runtime_java_path(dirs: &Dirs, component: &str) -> Result<PathBuf> {
    let base = paths::safe_join(&dirs.runtime(), component)?;
    Ok(base.join("bin").join(java_exe_name()))
}

/// Make sure Mojang's JRE `component` (e.g. `java-runtime-delta`) is installed
/// under `shared/runtime/<component>` and return its `java` executable.
pub async fn ensure_runtime(
    dl: &Downloader,
    dirs: &Dirs,
    component: &str,
    progress: Option<ProgressFn>,
) -> Result<PathBuf> {
    let base = paths::safe_join(&dirs.runtime(), component)?;
    let all: AllRuntimes = dl.fetch_json(RUNTIME_ALL_URL).await?;
    let entry = all
        .get(platform_key())
        .and_then(|p| p.get(component))
        .and_then(|v| v.first())
        .ok_or_else(|| {
            Error::Java(format!("Mojang 没有为 {} 提供 {component} 运行时", platform_key()))
        })?;

    let manifest: RuntimeManifest = {
        let bytes = dl.fetch_checked(&entry.manifest.url, entry.manifest.sha1.as_deref()).await?;
        serde_json::from_slice(&bytes)?
    };

    let mut tasks = Vec::new();
    let mut executables = Vec::new();
    for (rel, file) in &manifest.files {
        match file.kind.as_str() {
            "directory" => {
                std::fs::create_dir_all(paths::safe_join(&base, rel)?)?;
            }
            "file" => {
                let Some(raw) = file.downloads.as_ref().and_then(|d| d.raw.as_ref()) else {
                    continue;
                };
                let task = DownloadTask::rel(&base, rel, raw.url.clone())?
                    .sha1(raw.sha1.clone())
                    .size(raw.size);
                if file.executable {
                    executables.push(task.dest.clone());
                }
                tasks.push(task);
            }
            _ => {} // symlinks are not used by the Windows runtimes
        }
    }
    dl.download_all(tasks, "Java 运行时", progress).await?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for p in &executables {
            let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755));
        }
    }
    #[cfg(not(unix))]
    let _ = executables;

    let java = base.join("bin").join(java_exe_name());
    if !java.is_file() {
        return Err(Error::Java(format!("运行时安装后找不到 {}", java.display())));
    }
    Ok(java)
}

/// Which Java a version needs and whether it is already on disk.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct JavaNeed {
    /// Mojang's runtime name, e.g. `java-runtime-delta`.
    pub component: String,
    pub major: u32,
    /// The runtime's `java` is already under `shared/runtime`.
    pub installed: bool,
}

pub fn need_for(dirs: &Dirs, resolved: &crate::version::ResolvedVersion) -> Result<JavaNeed> {
    let (component, major) = match &resolved.json.java_version {
        Some(j) => (j.component.clone(), j.major_version),
        None => (LEGACY_COMPONENT.to_string(), LEGACY_MAJOR),
    };
    let installed = runtime_java_path(dirs, &component)?.is_file();
    Ok(JavaNeed { component, major, installed })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaInfo {
    pub major: u32,
    pub version: String,
}

/// Extract the major version from `java -version` output
/// (`"1.8.0_202"` => 8, `"21.0.4"` => 21).
pub fn parse_java_version(output: &str) -> Option<JavaInfo> {
    let re = Regex::new(r#"version "(\d+)(?:\.(\d+))?([^"]*)""#).ok()?;
    let caps = re.captures(output)?;
    let first: u32 = caps[1].parse().ok()?;
    let major = if first == 1 {
        caps.get(2)?.as_str().parse().ok()?
    } else {
        first
    };
    let version = caps.get(0)?.as_str().trim_start_matches("version ").trim_matches('"').to_string();
    Some(JavaInfo { major, version })
}

/// Run `java -version` to check that a user-provided Java works.
pub async fn probe_java(java: &Path) -> Result<JavaInfo> {
    let mut cmd = tokio::process::Command::new(java);
    cmd.arg("-version").stdin(std::process::Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let out = tokio::time::timeout(std::time::Duration::from_secs(15), cmd.output())
        .await
        .map_err(|_| Error::Java("java -version 超时".into()))?
        .map_err(|e| Error::Java(format!("无法运行 {}: {e}", java.display())))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    parse_java_version(&text)
        .ok_or_else(|| Error::Java(format!("无法识别 Java 版本输出: {}", text.trim())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_java_versions() {
        let j8 = r#"java version "1.8.0_202"
Java(TM) SE Runtime Environment (build 1.8.0_202-b08)"#;
        assert_eq!(parse_java_version(j8).unwrap().major, 8);
        let j21 = r#"openjdk version "21.0.4" 2024-07-16 LTS"#;
        assert_eq!(parse_java_version(j21).unwrap().major, 21);
        let j17 = r#"openjdk version "17" 2021-09-14"#;
        assert_eq!(parse_java_version(j17).unwrap().major, 17);
        assert!(parse_java_version("garbage").is_none());
    }

    #[test]
    fn platform_key_is_known() {
        assert!(!platform_key().is_empty());
    }
}
