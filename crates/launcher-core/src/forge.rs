//! Forge and NeoForge. Both ship an installer jar holding an `install_profile.json`
//! (libraries + processor steps) and a `version.json`; we read those and run the
//! steps ourselves instead of calling the installer.

use crate::config::Dirs;
use crate::download::{sha1_file, DownloadTask, Downloader, Progress, ProgressFn};
use crate::loader::{check_segment, cmp_versions, profile_id, LoaderKind, LoaderVersion};
use crate::paths::{self, maven_path};
use crate::{install, version, Error, Result};
use futures::future::BoxFuture;
use regex::Regex;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

const FORGE_METADATA: &str = "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml";
const FORGE_PROMOTIONS: &str = "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";
const NEOFORGE_METADATA: &str = "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml";

// ------------------------------------------------------------ version lists

fn maven_versions(xml: &str) -> Vec<String> {
    let re = Regex::new(r"<version>([^<]+)</version>").expect("static regex");
    re.captures_iter(xml).map(|c| c[1].to_string()).collect()
}

fn newest_first(mut v: Vec<String>) -> Vec<String> {
    v.sort_by(|a, b| cmp_versions(b, a));
    v.dedup();
    v
}

/// Forge builds for Minecraft `mc`, newest first, with the promoted
/// `recommended` (else `latest`) build moved to the front. A Forge maven
/// version is `{mc}-{forge}` (older ones `{mc}-{forge}-{mc}`); we keep `{forge}`.
pub fn parse_forge_versions(metadata_xml: &str, promotions_json: &str, mc: &str) -> Vec<LoaderVersion> {
    let prefix = format!("{mc}-");
    let suffix = format!("-{mc}");
    let found: Vec<String> = maven_versions(metadata_xml)
        .into_iter()
        .filter_map(|v| {
            let rest = v.strip_prefix(&prefix)?;
            Some(rest.strip_suffix(&suffix).unwrap_or(rest).to_string())
        })
        .collect();
    let mut list = newest_first(found);

    let promos: serde_json::Value = serde_json::from_str(promotions_json).unwrap_or_default();
    let promoted = ["recommended", "latest"]
        .iter()
        .find_map(|kind| promos["promos"][format!("{mc}-{kind}")].as_str().map(str::to_string));
    if let Some(p) = promoted {
        if let Some(i) = list.iter().position(|v| *v == p) {
            let v = list.remove(i);
            list.insert(0, v);
        }
    }
    list.into_iter().map(|version| LoaderVersion { version, stable: true }).collect()
}

/// The NeoForge version prefix that belongs to Minecraft `mc`:
/// `1.21.1` → `21.1.`, `1.21` → `21.0.`, `26.3` → `26.3.0.`, `26.1.2` → `26.1.2.`.
/// `None` for versions NeoForge does not cover (it starts at 1.20.2) or odd ids.
pub fn neoforge_prefix(mc: &str) -> Option<String> {
    let nums: Vec<u32> = mc.split('.').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    match nums.as_slice() {
        [1, a, rest @ ..] if rest.len() <= 1 => {
            let b = rest.first().copied().unwrap_or(0);
            ((*a, b) >= (20, 2)).then(|| format!("{a}.{b}."))
        }
        [yy, n, rest @ ..] if *yy >= 26 && rest.len() <= 1 => {
            Some(format!("{yy}.{n}.{}.", rest.first().copied().unwrap_or(0)))
        }
        _ => None,
    }
}

/// NeoForge builds for `mc`, newest first; `-beta`/`-alpha` builds are unstable.
pub fn parse_neoforge_versions(metadata_xml: &str, mc: &str) -> Vec<LoaderVersion> {
    let Some(prefix) = neoforge_prefix(mc) else { return Vec::new() };
    let found: Vec<String> = maven_versions(metadata_xml).into_iter().filter(|v| v.starts_with(&prefix)).collect();
    newest_first(found)
        .into_iter()
        .map(|version| LoaderVersion { stable: !version.contains('-'), version })
        .collect()
}

/// Installer URLs to try for a Forge build (newer builds are `{mc}-{v}`,
/// some older ones `{mc}-{v}-{mc}`).
pub fn forge_installer_candidates(mc: &str, loader_version: &str) -> Vec<String> {
    [format!("{mc}-{loader_version}"), format!("{mc}-{loader_version}-{mc}")]
        .into_iter()
        .map(|full| format!("https://maven.minecraftforge.net/net/minecraftforge/forge/{full}/forge-{full}-installer.jar"))
        .collect()
}

pub fn neoforge_installer_url(loader_version: &str) -> String {
    format!("https://maven.neoforged.net/releases/net/neoforged/neoforge/{loader_version}/neoforge-{loader_version}-installer.jar")
}

/// Loader versions for `mc`; an empty list means unsupported.
pub async fn list_versions(dl: &Downloader, kind: LoaderKind, mc: &str) -> Result<Vec<LoaderVersion>> {
    let text = |b: Vec<u8>| String::from_utf8_lossy(&b).into_owned();
    match kind {
        LoaderKind::Forge => {
            let xml = text(dl.fetch(FORGE_METADATA).await?);
            // Promotions only decide the preselected build; the list works without them.
            let promos = match dl.fetch(FORGE_PROMOTIONS).await {
                Ok(b) => text(b),
                Err(_) => String::new(),
            };
            Ok(parse_forge_versions(&xml, &promos, mc))
        }
        LoaderKind::NeoForge => {
            if neoforge_prefix(mc).is_none() {
                return Ok(Vec::new());
            }
            Ok(parse_neoforge_versions(&text(dl.fetch(NEOFORGE_METADATA).await?), mc))
        }
        other => Err(Error::Other(format!("{} 不使用安装器", other.label()))),
    }
}

// ------------------------------------------------------- installer profile

/// One value of the profile's `data` map; we always install the `client` side.
#[derive(Debug, Clone, Deserialize)]
pub struct DataEntry {
    pub client: String,
    pub server: String,
}

/// One installer step: a Java tool run with `args`, optionally checked by `outputs`.
#[derive(Debug, Clone, Deserialize)]
pub struct Processor {
    /// Sides the step runs on; absent means both.
    #[serde(default)]
    pub sides: Option<Vec<String>>,
    /// Maven coordinate of the tool's jar (its `Main-Class` is the entry point).
    pub jar: String,
    #[serde(default)]
    pub classpath: Vec<String>,
    #[serde(default)]
    pub args: Vec<String>,
    /// Expected sha1 per output file; both sides may contain `{VARIABLES}`.
    #[serde(default)]
    pub outputs: BTreeMap<String, String>,
}

pub fn runs_on_client(p: &Processor) -> bool {
    p.sides.as_ref().is_none_or(|s| s.iter().any(|x| x == "client"))
}

/// A library the installer needs. An empty `url` means the jar is shipped
/// inside the installer under `maven/<path>`.
#[derive(Debug, Clone)]
pub struct ProfileLibrary {
    pub name: String,
    /// Path relative to the libraries directory.
    pub path: String,
    pub url: String,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Clone)]
pub enum InstallProfile {
    /// `spec` installers: libraries, `data` variables and processor steps (possibly none).
    Modern {
        minecraft: String,
        libraries: Vec<ProfileLibrary>,
        data: BTreeMap<String, DataEntry>,
        processors: Vec<Processor>,
        /// Installer entry holding the version JSON (no leading slash).
        version_json_entry: String,
    },
    /// Old installers: a ready `versionInfo` and a universal jar inside the installer.
    Legacy {
        version_info: serde_json::Value,
        universal_entry: String,
        universal_coord: String,
        minecraft: String,
    },
}

fn str_field<'a>(v: &'a serde_json::Value, key: &str) -> Result<&'a str> {
    v.get(key).and_then(|x| x.as_str()).ok_or_else(|| Error::Other(format!("安装配置缺少字段 `{key}`")))
}

/// A path taken from the profile must stay inside whatever directory it is joined to.
fn check_relative(rel: &str) -> Result<()> {
    paths::safe_join(Path::new("base"), rel).map(|_| ())
}

pub fn parse_profile(bytes: &[u8]) -> Result<InstallProfile> {
    let v: serde_json::Value = serde_json::from_slice(bytes)?;
    if v.get("processors").is_some_and(|p| p.is_array()) {
        let mut libraries = Vec::new();
        for l in v["libraries"].as_array().into_iter().flatten() {
            let name = str_field(l, "name")?.to_string();
            let art = &l["downloads"]["artifact"];
            let path = match art["path"].as_str() {
                Some(p) => p.to_string(),
                None => maven_path(&name).ok_or_else(|| Error::Other(format!("无效的库坐标: {name}")))?,
            };
            check_relative(&path)?;
            libraries.push(ProfileLibrary {
                name,
                path,
                url: art["url"].as_str().unwrap_or("").to_string(),
                sha1: art["sha1"].as_str().filter(|s| !s.is_empty()).map(str::to_string),
                size: art["size"].as_u64(),
            });
        }
        let data = match v.get("data") {
            Some(d) if !d.is_null() => serde_json::from_value(d.clone())?,
            _ => BTreeMap::new(),
        };
        let processors = serde_json::from_value(v["processors"].clone())?;
        let entry = v["json"].as_str().unwrap_or("/version.json").trim_start_matches('/').to_string();
        check_relative(&entry)?;
        Ok(InstallProfile::Modern {
            minecraft: str_field(&v, "minecraft")?.to_string(),
            libraries,
            data,
            processors,
            version_json_entry: entry,
        })
    } else {
        let install = &v["install"];
        let universal_entry = str_field(install, "filePath")?.to_string();
        check_relative(&universal_entry)?;
        Ok(InstallProfile::Legacy {
            version_info: v["versionInfo"].clone(),
            universal_entry,
            universal_coord: str_field(install, "path")?.to_string(),
            minecraft: str_field(install, "minecraft")?.to_string(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataValue {
    /// `[group:artifact:version[:classifier][@ext]]` → a file in the libraries directory.
    Path(PathBuf),
    /// `'text'`
    Literal(String),
    /// `/entry` → a file inside the installer, extracted to the extraction directory.
    InstallerFile(String),
}

/// Variables of the client side plus the installer entries they need.
#[derive(Debug, Clone, Default)]
pub struct Resolved {
    pub vars: BTreeMap<String, String>,
    pub installer_entries: Vec<String>,
}

/// Turns the profile's `{PLACEHOLDERS}` and `[coordinates]` into concrete paths.
#[derive(Debug, Clone)]
pub struct Resolver {
    root: PathBuf,
    installer: PathBuf,
    minecraft_jar: PathBuf,
    mc: String,
    libraries_dir: PathBuf,
    extract_dir: PathBuf,
}

impl Resolver {
    pub fn new(root: &Path, installer: &Path, minecraft_jar: &Path, mc: &str, libraries_dir: &Path, extract_dir: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            installer: installer.to_path_buf(),
            minecraft_jar: minecraft_jar.to_path_buf(),
            mc: mc.to_string(),
            libraries_dir: libraries_dir.to_path_buf(),
            extract_dir: extract_dir.to_path_buf(),
        }
    }

    /// Library file of a maven coordinate (also used for a step's jar and classpath).
    pub fn coord_path(&self, coord: &str) -> Result<PathBuf> {
        let rel = maven_path(coord).ok_or_else(|| Error::Other(format!("无效的坐标: {coord}")))?;
        paths::safe_join(&self.libraries_dir, &rel)
    }

    pub fn data_value(&self, entry: &DataEntry) -> Result<DataValue> {
        let s = entry.client.trim();
        if let Some(inner) = s.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            Ok(DataValue::Path(self.coord_path(inner)?))
        } else if s.len() >= 2 && s.starts_with('\'') && s.ends_with('\'') {
            Ok(DataValue::Literal(s[1..s.len() - 1].to_string()))
        } else if let Some(entry) = s.strip_prefix('/') {
            paths::safe_join(&self.extract_dir, entry)?;
            Ok(DataValue::InstallerFile(entry.to_string()))
        } else {
            Ok(DataValue::Literal(s.to_string()))
        }
    }

    pub fn variables(&self, data: &BTreeMap<String, DataEntry>) -> Result<Resolved> {
        let mut out = Resolved::default();
        for (name, entry) in data {
            let value = match self.data_value(entry)? {
                DataValue::Path(p) => p.to_string_lossy().into_owned(),
                DataValue::Literal(s) => s,
                DataValue::InstallerFile(e) => {
                    let p = paths::safe_join(&self.extract_dir, &e)?;
                    out.installer_entries.push(e);
                    p.to_string_lossy().into_owned()
                }
            };
            out.vars.insert(name.clone(), value);
        }
        Ok(out)
    }

    fn builtin(&self, name: &str) -> Option<String> {
        let p = |x: &Path| x.to_string_lossy().into_owned();
        match name {
            "ROOT" => Some(p(&self.root)),
            "INSTALLER" => Some(p(&self.installer)),
            "MINECRAFT_JAR" => Some(p(&self.minecraft_jar)),
            "SIDE" => Some("client".to_string()),
            "MINECRAFT_VERSION" => Some(self.mc.clone()),
            "LIBRARY_DIR" => Some(p(&self.libraries_dir)),
            _ => None,
        }
    }

    /// Expand one step argument: a whole-argument `[coordinate]` becomes the
    /// library path, `{NAME}` is replaced from `vars` or the built-ins.
    pub fn expand_arg(&self, arg: &str, vars: &BTreeMap<String, String>) -> Result<String> {
        if let Some(inner) = arg.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            return Ok(self.coord_path(inner)?.to_string_lossy().into_owned());
        }
        let re = Regex::new(r"\{([A-Za-z0-9_]+)\}").expect("static regex");
        let mut out = String::with_capacity(arg.len());
        let mut last = 0;
        for c in re.captures_iter(arg) {
            let whole = c.get(0).expect("match");
            out.push_str(&arg[last..whole.start()]);
            let name = &c[1];
            let value = vars
                .get(name)
                .cloned()
                .or_else(|| self.builtin(name))
                .ok_or_else(|| Error::Other(format!("未知的占位符 {{{name}}}")))?;
            out.push_str(&value);
            last = whole.end();
        }
        out.push_str(&arg[last..]);
        Ok(out)
    }
}

// --------------------------------------------------------------- installing

/// One external process: a Java tool run for an installer step.
#[derive(Debug, Clone)]
pub struct ProcessSpec {
    pub java: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// The tool's stdout and stderr are appended here.
    pub log: PathBuf,
}

#[derive(Debug, Clone, Copy)]
pub struct ProcessOutcome {
    pub code: i32,
}

/// Runs a step. Tests plug in a stub so no Java is needed.
pub type RunFn = Arc<dyn Fn(ProcessSpec, CancellationToken) -> BoxFuture<'static, Result<ProcessOutcome>> + Send + Sync>;

pub fn real_runner() -> RunFn {
    Arc::new(|spec, cancel| Box::pin(run_process(spec, cancel)))
}

async fn run_process(spec: ProcessSpec, cancel: CancellationToken) -> Result<ProcessOutcome> {
    if let Some(dir) = spec.log.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let log = std::fs::OpenOptions::new().create(true).append(true).open(&spec.log)?;
    let log_err = log.try_clone()?;
    let mut cmd = tokio::process::Command::new(&spec.java);
    cmd.args(&spec.args)
        .current_dir(&spec.cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(log))
        .stderr(std::process::Stdio::from(log_err))
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let mut child = cmd
        .spawn()
        .map_err(|e| Error::Other(format!("无法启动 Java（{}）: {e}", spec.java.display())))?;
    tokio::select! {
        status = child.wait() => Ok(ProcessOutcome { code: status?.code().unwrap_or(-1) }),
        _ = cancel.cancelled() => {
            let _ = child.kill().await;
            Err(Error::Cancelled)
        }
    }
}

fn read_entry(zip: &mut zip::ZipArchive<std::fs::File>, name: &str, cap: u64) -> Result<Vec<u8>> {
    use std::io::Read;
    let entry = zip.by_name(name).map_err(|_| Error::Other(format!("安装器中没有 {name}")))?;
    let mut out = Vec::new();
    entry.take(cap + 1).read_to_end(&mut out)?;
    if out.len() as u64 > cap {
        return Err(Error::Other(format!("安装器中的 {name} 过大")));
    }
    Ok(out)
}

fn extract_entry(zip: &mut zip::ZipArchive<std::fs::File>, name: &str, dest: &Path) -> Result<()> {
    let mut entry = zip.by_name(name).map_err(|_| Error::Other(format!("安装器中没有 {name}")))?;
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = dest.with_extension("part");
    std::io::copy(&mut entry, &mut std::fs::File::create(&tmp)?)?;
    std::fs::rename(&tmp, dest)?;
    Ok(())
}

/// `Main-Class` of a jar's manifest.
fn main_class(jar: &Path) -> Result<String> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(jar).map_err(|_| Error::Missing(jar.to_path_buf()))?)?;
    let text = String::from_utf8_lossy(&read_entry(&mut zip, "META-INF/MANIFEST.MF", 1 << 20)?).into_owned();
    // Manifest lines longer than 72 bytes continue on the next line after one space.
    let text = text.replace("\r\n ", "").replace("\n ", "");
    text.lines()
        .find_map(|l| l.strip_prefix("Main-Class:").map(|m| m.trim().to_string()))
        .ok_or_else(|| Error::Other(format!("{} 的清单里没有 Main-Class", jar.display())))
}

fn log_tail(log: &Path) -> String {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(20)..].join("\n")
}

/// Every expected output exists and has the expected sha1.
async fn outputs_valid(outs: &[(PathBuf, String)]) -> bool {
    for (path, sha1) in outs {
        if !path.is_file() || !sha1_file(path).await.is_ok_and(|h| h.eq_ignore_ascii_case(sha1)) {
            return false;
        }
    }
    true
}

fn write_version_json(dirs: &Dirs, id: &str, json: &serde_json::Value) -> Result<()> {
    let path = version::version_json_path(&dirs.versions(), id)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(json)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

fn step_progress(progress: &Option<ProgressFn>, done: usize, total: usize) {
    if let Some(cb) = progress {
        cb(&Progress {
            stage: format!("安装步骤 {}/{total}", (done + 1).min(total)),
            files_done: done as u64,
            files_total: total as u64,
            bytes_done: 0,
            bytes_total: 0,
        });
    }
}

/// Install Forge/NeoForge for `mc`: the vanilla version first, then the
/// installer's steps. Returns the profile id; the caller finishes the
/// remaining libraries and assets with `install_version(id)`.
pub async fn install_forge_like(
    dl: &Downloader,
    dirs: &Dirs,
    kind: LoaderKind,
    mc: &str,
    loader_version: &str,
    java: &Path,
    progress: Option<ProgressFn>,
) -> Result<String> {
    install::install_version(dl, dirs, mc, progress.clone()).await?;
    run_installer(dl, dirs, kind, mc, loader_version, java, &real_runner(), progress).await
}

/// The installer part of [`install_forge_like`], with the vanilla version
/// already installed and the process runner injected.
#[allow(clippy::too_many_arguments)]
pub async fn run_installer(
    dl: &Downloader,
    dirs: &Dirs,
    kind: LoaderKind,
    mc: &str,
    loader_version: &str,
    java: &Path,
    run: &RunFn,
    progress: Option<ProgressFn>,
) -> Result<String> {
    if !kind.runs_installer() {
        return Err(Error::Other(format!("{} 不使用安装器", kind.label())));
    }
    check_segment(mc)?;
    check_segment(loader_version)?;
    let id = profile_id(kind, loader_version, mc);
    if version::version_json_path(&dirs.versions(), &id)?.is_file() {
        return Ok(id); // finished earlier; the version JSON is written last
    }

    let extract_dir = dirs.shared().join("tmp").join(format!("installer-{id}"));
    let _ = std::fs::remove_dir_all(&extract_dir);
    let result = install_steps(dl, dirs, kind, mc, loader_version, &id, java, run, progress, &extract_dir).await;
    let _ = std::fs::remove_dir_all(&extract_dir);
    result.map(|()| id)
}

#[allow(clippy::too_many_arguments)]
async fn install_steps(
    dl: &Downloader,
    dirs: &Dirs,
    kind: LoaderKind,
    mc: &str,
    loader_version: &str,
    id: &str,
    java: &Path,
    run: &RunFn,
    progress: Option<ProgressFn>,
    extract_dir: &Path,
) -> Result<()> {
    // 1. The installer jar (its .sha1 sidecar is checked by the downloader).
    let installers = dirs.shared().join("installers");
    let candidates = match kind {
        LoaderKind::NeoForge => vec![neoforge_installer_url(loader_version)],
        _ => forge_installer_candidates(mc, loader_version),
    };
    let mut installer = None;
    let mut last_err = Error::Other("没有可用的安装器地址".into());
    for url in candidates {
        let file = url.rsplit('/').next().unwrap_or("installer.jar").to_string();
        let task = DownloadTask::rel(&installers, &file, url)?.sidecar(true);
        let dest = task.dest.clone();
        match dl.download_all(vec![task], "安装器", progress.clone()).await {
            Ok(()) => {
                installer = Some(dest);
                break;
            }
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(e) => last_err = e,
        }
    }
    let installer = installer.ok_or(last_err)?;
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&installer)?)?;

    // 2. What kind of installer is it?
    let profile = parse_profile(&read_entry(&mut zip, "install_profile.json", 16 << 20)?)?;
    let libraries_dir = dirs.libraries();
    let (version_json, minecraft_ok) = match profile {
        InstallProfile::Legacy { mut version_info, universal_entry, universal_coord, minecraft } => {
            let rel = maven_path(&universal_coord).ok_or_else(|| Error::Other(format!("无效的坐标: {universal_coord}")))?;
            extract_entry(&mut zip, &universal_entry, &paths::safe_join(&libraries_dir, &rel)?)?;
            if version_info.get("inheritsFrom").is_none_or(|v| v.is_null()) {
                version_info["inheritsFrom"] = serde_json::Value::String(mc.to_string());
            }
            (version_info, minecraft == mc)
        }
        InstallProfile::Modern { minecraft, libraries, data, processors, version_json_entry } => {
            if minecraft != mc {
                return Err(Error::Other(format!("安装器是为 Minecraft {minecraft} 准备的，不是 {mc}")));
            }
            // 3. Libraries: downloaded, or shipped inside the installer under maven/.
            let mut tasks = Vec::new();
            for lib in &libraries {
                let dest = paths::safe_join(&libraries_dir, &lib.path)?;
                if lib.url.is_empty() {
                    extract_entry(&mut zip, &format!("maven/{}", lib.path), &dest)?;
                } else {
                    tasks.push(
                        DownloadTask::new(lib.url.clone(), dest)
                            .sha1(lib.sha1.clone())
                            .size(lib.size)
                            .sidecar(lib.sha1.is_none()),
                    );
                }
            }
            dl.download_all(tasks, "安装库", progress.clone()).await?;

            // 4. Variables, installer files they need, then the client steps.
            let minecraft_jar = version::version_json_path(&dirs.versions(), mc)?.with_extension("jar");
            let resolver = Resolver::new(&dirs.shared(), &installer, &minecraft_jar, mc, &libraries_dir, extract_dir);
            let resolved = resolver.variables(&data)?;
            for entry in &resolved.installer_entries {
                extract_entry(&mut zip, entry, &paths::safe_join(extract_dir, entry)?)?;
            }
            let steps: Vec<&Processor> = processors.iter().filter(|p| runs_on_client(p)).collect();
            let log = dirs.shared().join("logs").join(format!("forge-{id}.log"));
            let _ = std::fs::remove_file(&log);
            for (i, p) in steps.iter().enumerate() {
                if dl.cancel_token().is_cancelled() {
                    return Err(Error::Cancelled);
                }
                let outs = p
                    .outputs
                    .iter()
                    .map(|(k, v)| Ok((PathBuf::from(resolver.expand_arg(k, &resolved.vars)?), resolver.expand_arg(v, &resolved.vars)?)))
                    .collect::<Result<Vec<_>>>()?;
                if !outs.is_empty() && outputs_valid(&outs).await {
                    continue;
                }
                step_progress(&progress, i, steps.len());

                let tool = resolver.coord_path(&p.jar)?;
                let mut classpath = vec![tool.clone()];
                for c in &p.classpath {
                    classpath.push(resolver.coord_path(c)?);
                }
                for (jar, coord) in classpath.iter().zip(std::iter::once(&p.jar).chain(p.classpath.iter())) {
                    if !jar.is_file() {
                        return Err(Error::Other(format!("缺少安装步骤所需的库: {coord}")));
                    }
                }
                let sep = if cfg!(windows) { ";" } else { ":" };
                let mut args = vec![
                    "-cp".to_string(),
                    classpath.iter().map(|c| c.to_string_lossy().into_owned()).collect::<Vec<_>>().join(sep),
                    main_class(&tool)?,
                ];
                for a in &p.args {
                    args.push(resolver.expand_arg(a, &resolved.vars)?);
                }
                let spec = ProcessSpec { java: java.to_path_buf(), args, cwd: dirs.shared(), log: log.clone() };
                let remove_outputs = || {
                    for (path, _) in &outs {
                        let _ = std::fs::remove_file(path);
                    }
                };
                match run(spec, dl.cancel_token()).await {
                    Err(e) => {
                        remove_outputs();
                        return Err(e);
                    }
                    Ok(o) if o.code != 0 => {
                        remove_outputs();
                        return Err(Error::Other(format!("安装步骤失败（退出码 {}）\n{}", o.code, log_tail(&log))));
                    }
                    Ok(_) => {}
                }
                if !outs.is_empty() && !outputs_valid(&outs).await {
                    remove_outputs();
                    return Err(Error::Other(format!("安装步骤的输出校验失败\n{}", log_tail(&log))));
                }
            }
            let bytes = read_entry(&mut zip, &version_json_entry, 16 << 20)?;
            let json: serde_json::Value = serde_json::from_slice(&bytes)?;
            let inherits_ok = json["inheritsFrom"].as_str() == Some(mc);
            if !inherits_ok {
                return Err(Error::Other(format!("加载器配置不是基于 Minecraft {mc}")));
            }
            (json, true)
        }
    };
    if !minecraft_ok {
        return Err(Error::Other(format!("安装器不是为 Minecraft {mc} 准备的")));
    }

    // 5. Written last, under our own id, so an unfinished install never looks finished.
    let mut json = version_json;
    json["id"] = serde_json::Value::String(id.to_string());
    write_version_json(dirs, id, &json)
}
