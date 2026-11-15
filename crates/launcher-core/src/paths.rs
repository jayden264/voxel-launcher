//! Path safety and maven coordinate helpers.
//!
//! Every path that originates from remote JSON (library paths, asset names,
//! runtime manifests, zip entries, ...) must go through [`safe_join`].

use crate::{Error, Result};
use std::path::{Component, Path, PathBuf};

const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

fn bad(rel: &str) -> Error {
    Error::UnsafePath(rel.to_string())
}

/// Join an untrusted relative path onto `base`, rejecting anything that could
/// escape it. Uses lexical normalisation only (the target does not exist yet).
pub fn safe_join(base: &Path, rel: &str) -> Result<PathBuf> {
    // ':' anywhere: drive letters and NTFS alternate data streams.
    if rel.is_empty() || rel.contains('\0') || rel.contains(':') {
        return Err(bad(rel));
    }
    let unified = rel.replace('\\', "/");
    if unified.starts_with('/') {
        return Err(bad(rel)); // absolute / UNC
    }
    let mut out = base.to_path_buf();
    let mut depth = 0usize;
    for part in unified.split('/') {
        match part {
            "" | "." => continue,
            ".." => return Err(bad(rel)),
            p => {
                if p.ends_with('.') || p.ends_with(' ') {
                    return Err(bad(rel));
                }
                let stem = p.split('.').next().unwrap_or("").to_ascii_uppercase();
                if RESERVED.contains(&stem.as_str()) {
                    return Err(bad(rel));
                }
                out.push(p);
                depth += 1;
            }
        }
    }
    if depth == 0 {
        return Err(bad(rel));
    }
    // Defence in depth: the lexical result must still start with base and
    // contain only normal components after it.
    let tail = out.strip_prefix(base).map_err(|_| bad(rel))?;
    if tail.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(bad(rel));
    }
    Ok(out)
}

/// Parsed maven coordinate `group:artifact:version[:classifier][@ext]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Maven {
    pub group: String,
    pub artifact: String,
    pub version: String,
    pub classifier: Option<String>,
    pub ext: String,
}

impl Maven {
    pub fn parse(coord: &str) -> Option<Maven> {
        let (body, ext) = match coord.split_once('@') {
            Some((b, e)) => (b, e.to_string()),
            None => (coord, "jar".to_string()),
        };
        let parts: Vec<&str> = body.split(':').collect();
        if parts.len() < 3 || parts.len() > 4 || parts.iter().any(|p| p.is_empty()) {
            return None;
        }
        Some(Maven {
            group: parts[0].to_string(),
            artifact: parts[1].to_string(),
            version: parts[2].to_string(),
            classifier: parts.get(3).map(|s| s.to_string()),
            ext,
        })
    }

    /// Repository-relative path, e.g. `org/ow2/asm/asm/9.6/asm-9.6.jar`.
    pub fn path(&self) -> String {
        let mut file = format!("{}-{}", self.artifact, self.version);
        if let Some(c) = &self.classifier {
            file.push('-');
            file.push_str(c);
        }
        file.push('.');
        file.push_str(&self.ext);
        format!(
            "{}/{}/{}/{}",
            self.group.replace('.', "/"),
            self.artifact,
            self.version,
            file
        )
    }

    /// Dedup key used when merging libraries: `group:artifact[:classifier]`.
    pub fn key(&self) -> String {
        match &self.classifier {
            Some(c) => format!("{}:{}:{}", self.group, self.artifact, c),
            None => format!("{}:{}", self.group, self.artifact),
        }
    }
}

pub fn maven_path(coord: &str) -> Option<String> {
    Maven::parse(coord).map(|m| m.path())
}

/// The path the OS itself resolves `p` to (junctions, symlinks and the
/// per-app folder redirection of packaged Windows apps followed). Fabric/Quilt
/// compare the `-cp` strings with the paths the JVM reports for loaded jars; if
/// the two spellings differ they load their own classes twice and every mixin
/// plugin fails with a ClassCastException. Falls back to `p` if it can't be
/// resolved (e.g. it does not exist yet).
pub fn real_path(p: &Path) -> PathBuf {
    match std::fs::canonicalize(p) {
        Ok(c) => {
            let s = c.to_string_lossy();
            // `\\?\C:\x` -> `C:\x`, but keep `\\?\UNC\...` as it is.
            match s.strip_prefix(r"\\?\") {
                Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
                _ => c,
            }
        }
        Err(_) => p.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn real_path_follows_a_junction_and_drops_the_verbatim_prefix() {
        let t = tempfile::tempdir().unwrap();
        let target = t.path().join("real");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("a.jar"), b"x").unwrap();
        let link = t.path().join("link");
        let st = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .output()
            .unwrap();
        assert!(st.status.success(), "mklink failed");

        let got = real_path(&link.join("a.jar"));
        assert!(got.ends_with("real\\a.jar"), "{got:?}");
        assert!(!got.to_string_lossy().starts_with(r"\\?\"), "{got:?}");
        // A path that does not exist is returned untouched.
        let missing = t.path().join("nope.jar");
        assert_eq!(real_path(&missing), missing);
    }

    #[test]
    fn rejects_malicious_paths() {
        let base = Path::new("C:/data");
        for p in [
            "../evil.dll",
            "a/../../evil",
            "/etc/passwd",
            "\\\\server\\share\\x",
            "C:/Windows/x",
            "a.txt:stream",
            "con",
            "dir/NUL.txt",
            "trailing.",
            "trailing /x",
            "",
            "a\0b",
            "./",
        ] {
            assert!(safe_join(base, p).is_err(), "should reject {p:?}");
        }
    }

    #[test]
    fn accepts_normal_paths() {
        let base = Path::new("C:/data");
        assert_eq!(
            safe_join(base, "org/ow2/asm/asm/9.6/asm-9.6.jar").unwrap(),
            Path::new("C:/data/org/ow2/asm/asm/9.6/asm-9.6.jar")
        );
        assert_eq!(
            safe_join(base, "bin\\javaw.exe").unwrap(),
            Path::new("C:/data/bin/javaw.exe")
        );
        assert!(safe_join(base, "assets/中文 名.txt").is_ok());
    }

    #[test]
    fn maven_paths() {
        assert_eq!(
            maven_path("org.ow2.asm:asm:9.6").unwrap(),
            "org/ow2/asm/asm/9.6/asm-9.6.jar"
        );
        assert_eq!(
            maven_path("org.lwjgl:lwjgl:3.3.3:natives-windows").unwrap(),
            "org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3-natives-windows.jar"
        );
        assert_eq!(
            maven_path("net.fabricmc:fabric-loader:0.16.9@zip").unwrap(),
            "net/fabricmc/fabric-loader/0.16.9/fabric-loader-0.16.9.zip"
        );
        assert!(maven_path("bad").is_none());
    }

    #[test]
    fn maven_keys_ignore_version() {
        let a = Maven::parse("org.ow2.asm:asm:9.3").unwrap();
        let b = Maven::parse("org.ow2.asm:asm:9.6").unwrap();
        assert_eq!(a.key(), b.key());
        let n = Maven::parse("org.lwjgl:lwjgl:3.3.3:natives-windows").unwrap();
        assert_ne!(n.key(), Maven::parse("org.lwjgl:lwjgl:3.3.3").unwrap().key());
    }
}
