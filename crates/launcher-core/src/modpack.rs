//! Modrinth modpacks (`.mrpack`): a zip with `modrinth.index.json` (the files to
//! download, and which game and loader versions to use) plus `overrides/`
//! folders that are copied into the instance.

use crate::config::Dirs;
use crate::download::{DownloadTask, Downloader, ProgressFn};
use crate::loader::{check_segment, LoaderKind};
use crate::mods::{check_id, ModrinthClient, SearchResult, VersionFile};
use crate::{paths, Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

/// Hosts a modpack may download from (the Modrinth modpack format's allow-list).
/// Upper bound for `modrinth.index.json` (real packs are well under 1 MiB).
const MAX_INDEX_BYTES: u64 = 16 << 20;

const ALLOWED_HOSTS: [&str; 4] = ["cdn.modrinth.com", "github.com", "raw.githubusercontent.com", "gitlab.com"];

#[derive(Debug, Clone)]
pub struct MrFile {
    /// Path relative to the instance directory, e.g. `mods/x.jar`.
    pub path: String,
    pub sha1: String,
    pub size: u64,
    pub downloads: Vec<String>,
    /// False when the pack marks the file as unsupported on the client.
    pub client_required: bool,
}

#[derive(Debug, Clone)]
pub struct MrIndex {
    pub name: String,
    pub version_id: String,
    pub dependencies: BTreeMap<String, String>,
    pub files: Vec<MrFile>,
}

/// What the pack needs installed: a Minecraft version and (maybe) a loader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackTarget {
    pub mc: String,
    pub loader: Option<(LoaderKind, String)>,
}

fn bad(msg: impl Into<String>) -> Error {
    Error::Other(msg.into())
}

fn check_download(url: &str) -> Result<()> {
    let u = reqwest::Url::parse(url).map_err(|_| bad(format!("整合包里的下载地址无效: {url}")))?;
    let ok = u.scheme() == "https"
        && u.username().is_empty()
        && u.password().is_none()
        && u.host_str().is_some_and(|h| ALLOWED_HOSTS.contains(&h));
    if ok {
        Ok(())
    } else {
        Err(bad(format!("整合包要从不受信任的地址下载文件: {url}")))
    }
}

/// A path inside the instance directory; our own bookkeeping folder is off limits.
fn check_pack_path(path: &str) -> Result<()> {
    paths::safe_join(Path::new("instance"), path)?;
    let first = path.replace('\\', "/");
    if first.trim_start_matches("./").split('/').next().is_some_and(|c| c.eq_ignore_ascii_case(".voxel")) {
        return Err(Error::UnsafePath(path.to_string()));
    }
    Ok(())
}

pub fn parse_index(bytes: &[u8]) -> Result<MrIndex> {
    let v: Value = serde_json::from_slice(bytes)?;
    if v["formatVersion"].as_u64() != Some(1) {
        return Err(bad("不支持的整合包格式版本"));
    }
    if v["game"].as_str() != Some("minecraft") {
        return Err(bad("这个整合包不是 Minecraft 整合包"));
    }
    let name = v["name"].as_str().ok_or_else(|| bad("整合包缺少名称"))?.to_string();
    let dependencies = v["dependencies"]
        .as_object()
        .map(|o| o.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect())
        .unwrap_or_default();

    let mut files = Vec::new();
    for f in v["files"].as_array().into_iter().flatten() {
        let path = f["path"].as_str().ok_or_else(|| bad("整合包文件缺少路径"))?.to_string();
        check_pack_path(&path)?;
        let sha1 = f["hashes"]["sha1"].as_str().unwrap_or("").to_ascii_lowercase();
        if sha1.len() != 40 || !sha1.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(bad(format!("{path} 缺少有效的 sha1 校验值")));
        }
        let downloads: Vec<String> = f["downloads"].as_array().into_iter().flatten().filter_map(|d| d.as_str().map(str::to_string)).collect();
        if downloads.is_empty() {
            return Err(bad(format!("{path} 没有下载地址")));
        }
        for d in &downloads {
            check_download(d)?;
        }
        files.push(MrFile {
            path,
            sha1,
            size: f["fileSize"].as_u64().unwrap_or(0),
            downloads,
            client_required: f["env"]["client"].as_str() != Some("unsupported"),
        });
    }
    Ok(MrIndex { name, version_id: v["versionId"].as_str().unwrap_or("").to_string(), dependencies, files })
}

/// Read and validate `modrinth.index.json` from a `.mrpack` on disk.
pub fn read_index(mrpack: &Path) -> Result<MrIndex> {
    use std::io::Read;
    let file = std::fs::File::open(mrpack).map_err(|_| Error::Missing(mrpack.to_path_buf()))?;
    let mut zip = zip::ZipArchive::new(file)?;
    let entry = zip.by_name("modrinth.index.json").map_err(|_| bad("整合包里没有 modrinth.index.json"))?;
    let mut bytes = Vec::new();
    entry.take(MAX_INDEX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_INDEX_BYTES {
        return Err(bad("整合包的索引文件过大"));
    }
    parse_index(&bytes)
}

/// The Minecraft version and loader the pack asks for.
pub fn pack_target(idx: &MrIndex) -> Result<PackTarget> {
    let mc = idx.dependencies.get("minecraft").ok_or_else(|| bad("整合包没有指定 Minecraft 版本"))?;
    check_segment(mc)?;
    let loader = [
        ("fabric-loader", LoaderKind::Fabric),
        ("quilt-loader", LoaderKind::Quilt),
        ("forge", LoaderKind::Forge),
        ("neoforge", LoaderKind::NeoForge),
    ]
    .into_iter()
    .find_map(|(key, kind)| idx.dependencies.get(key).map(|v| (kind, v.clone())));
    if let Some((_, v)) = &loader {
        check_segment(v)?;
    }
    Ok(PackTarget { mc: mc.clone(), loader })
}

// ----------------------------------------------------------- search / versions

/// One downloadable version of a modpack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackVersion {
    pub id: String,
    pub name: String,
    pub version_number: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    /// `release`, `beta` or `alpha`.
    pub version_type: String,
    /// The `.mrpack` file.
    pub file: VersionFile,
}

#[derive(Deserialize)]
struct RawPackVersion {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    version_number: String,
    #[serde(default)]
    version_type: String,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    loaders: Vec<String>,
    #[serde(default)]
    files: Vec<RawPackFile>,
}

#[derive(Deserialize)]
struct RawPackFile {
    #[serde(default)]
    hashes: RawPackHashes,
    url: String,
    filename: String,
    #[serde(default)]
    primary: bool,
    #[serde(default)]
    size: u64,
}

#[derive(Deserialize, Default)]
struct RawPackHashes {
    #[serde(default)]
    sha1: String,
}

fn is_mrpack(f: &RawPackFile) -> bool {
    f.filename.to_ascii_lowercase().ends_with(".mrpack")
}

impl ModrinthClient {
    /// Search modpacks; an empty `query` lists popular ones.
    pub async fn search_modpacks(&self, query: &str, offset: u32) -> Result<SearchResult> {
        self.search_facets(serde_json::json!([["project_type:modpack"]]).to_string(), query, offset).await
    }

    /// Versions of a modpack, newest first, each with its `.mrpack` file
    /// (versions without one are left out).
    pub async fn pack_versions(&self, project: &str) -> Result<Vec<PackVersion>> {
        check_id(project)?;
        let raw: Vec<RawPackVersion> = self.get_json(self.url(&format!("/project/{project}/version"), &[])?).await?;
        Ok(raw
            .into_iter()
            .filter_map(|v| {
                let f = v.files.iter().find(|f| f.primary && is_mrpack(f)).or_else(|| v.files.iter().find(|f| is_mrpack(f)))?;
                let file = VersionFile { filename: f.filename.clone(), url: f.url.clone(), sha1: f.hashes.sha1.clone(), size: f.size, primary: f.primary };
                Some(PackVersion {
                    id: v.id,
                    name: v.name,
                    version_number: v.version_number,
                    game_versions: v.game_versions,
                    loaders: v.loaders,
                    version_type: v.version_type,
                    file,
                })
            })
            .collect())
    }
}

// ---------------------------------------------------------------- installing

/// Per-entry limit for files unpacked from a modpack's overrides.
const MAX_OVERRIDE_BYTES: u64 = 512 << 20;

/// Put a modpack's files into the instance: download every file the client
/// needs (trying each listed address in turn), then unpack `overrides/` and
/// `client-overrides/` (the latter wins). The `.mrpack` must already be on disk.
pub async fn install_pack_files(
    dl: &Downloader,
    dirs: &Dirs,
    instance_id: &str,
    mrpack: &Path,
    idx: &MrIndex,
    progress: Option<ProgressFn>,
) -> Result<()> {
    let inst = dirs.instance(instance_id)?;
    std::fs::create_dir_all(&inst)?;

    let wanted: Vec<&MrFile> = idx.files.iter().filter(|f| f.client_required).collect();
    let mut pending: Vec<usize> = (0..wanted.len()).collect();
    let mut errors: BTreeMap<usize, String> = BTreeMap::new();
    let rounds = wanted.iter().map(|f| f.downloads.len()).max().unwrap_or(0);
    for round in 0..rounds {
        let mut ids = Vec::new();
        let mut tasks = Vec::new();
        for &i in &pending {
            let f = wanted[i];
            if let Some(url) = f.downloads.get(round) {
                ids.push(i);
                tasks.push(DownloadTask::new(url.clone(), paths::safe_join(&inst, &f.path)?).sha1(Some(f.sha1.clone())).size(Some(f.size).filter(|n| *n > 0)));
            }
        }
        if tasks.is_empty() {
            break;
        }
        let results = dl.download_all_settled(tasks, "整合包文件", progress.clone()).await;
        let mut failed = Vec::new();
        for (i, r) in ids.iter().zip(results) {
            match r {
                Ok(()) => {
                    errors.remove(i);
                }
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                Err(e) => {
                    errors.insert(*i, e.to_string());
                    failed.push(*i);
                }
            }
        }
        // Files that had no address left for this round stay pending as failures.
        pending.retain(|i| failed.contains(i) || wanted[*i].downloads.get(round).is_none());
        if pending.is_empty() {
            break;
        }
    }
    if let Some(i) = pending.first() {
        let why = errors.get(i).cloned().unwrap_or_else(|| "没有可用的下载地址".into());
        return Err(Error::Other(format!("下载 {} 失败: {why}", wanted[*i].path)));
    }

    let mut zip = zip::ZipArchive::new(std::fs::File::open(mrpack).map_err(|_| Error::Missing(mrpack.to_path_buf()))?)?;
    for prefix in ["overrides/", "client-overrides/"] {
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i)?;
            let name = entry.name()?.to_string();
            let Some(rel) = name.strip_prefix(prefix) else { continue };
            if rel.is_empty() || name.ends_with('/') {
                continue; // a directory entry
            }
            check_pack_path(rel)?;
            let dest = paths::safe_join(&inst, rel)?;
            if entry.size() > MAX_OVERRIDE_BYTES {
                return Err(bad(format!("整合包里的 {rel} 过大")));
            }
            if let Some(dir) = dest.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let tmp = dest.with_extension("part");
            std::io::copy(&mut entry, &mut std::fs::File::create(&tmp)?)?;
            std::fs::rename(&tmp, &dest)?;
        }
    }
    Ok(())
}

// ------------------------------------------------------ interrupted installs

/// `<instance>/.voxel/modpack-pending.json`: present from the moment a modpack
/// instance is created until all its files are in place, so a crash or a closed
/// launcher in between is noticed and finished instead of leaving a broken instance.
fn pending_file(dirs: &Dirs, instance_id: &str) -> Result<std::path::PathBuf> {
    Ok(dirs.instance(instance_id)?.join(".voxel").join("modpack-pending.json"))
}

pub fn mark_pending(dirs: &Dirs, instance_id: &str, mrpack: &Path) -> Result<()> {
    let value = serde_json::json!({ "mrpack": mrpack.to_string_lossy() });
    crate::config::save_atomic(&pending_file(dirs, instance_id)?, &value)
}

/// The `.mrpack` an unfinished install was started from, if any.
pub fn pending_pack(dirs: &Dirs, instance_id: &str) -> Result<Option<std::path::PathBuf>> {
    let bytes = match std::fs::read(pending_file(dirs, instance_id)?) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let v: Value = serde_json::from_slice(&bytes)?;
    Ok(v["mrpack"].as_str().map(std::path::PathBuf::from))
}

pub fn clear_pending(dirs: &Dirs, instance_id: &str) -> Result<()> {
    match std::fs::remove_file(pending_file(dirs, instance_id)?) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Finish an interrupted modpack install (already-complete files are skipped,
/// half-downloaded ones continue). Returns whether anything was pending. On
/// failure the marker stays, so the next attempt picks up where this one stopped.
pub async fn resume_pending(dl: &Downloader, dirs: &Dirs, instance_id: &str, progress: Option<ProgressFn>) -> Result<bool> {
    let Some(pack) = pending_pack(dirs, instance_id)? else { return Ok(false) };
    let idx = read_index(&pack)?;
    install_pack_files(dl, dirs, instance_id, &pack, &idx, progress).await?;
    clear_pending(dirs, instance_id)?;
    Ok(true)
}
