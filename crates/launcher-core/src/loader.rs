//! Mod loaders: a loader is a version JSON that inherits from a vanilla
//! version. Fabric/Quilt get it ready-made from their meta service; Forge and
//! NeoForge generate it by running their installer's steps (see `forge`).

use crate::config::Dirs;
use crate::download::{Downloader, ProgressFn};
use crate::version::{self, ResolvedVersion, VersionJson};
use crate::{install, Error, Result};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LoaderKind {
    Fabric,
    Quilt,
    Forge,
    NeoForge,
}

impl LoaderKind {
    pub fn label(&self) -> &'static str {
        match self {
            LoaderKind::Fabric => "Fabric",
            LoaderKind::Quilt => "Quilt",
            LoaderKind::Forge => "Forge",
            LoaderKind::NeoForge => "NeoForge",
        }
    }

    /// Forge and NeoForge are installed by running installer steps.
    pub fn runs_installer(&self) -> bool {
        matches!(self, LoaderKind::Forge | LoaderKind::NeoForge)
    }
}

/// Id of the version JSON we launch, e.g. `fabric-loader-0.16.9-1.21.1`,
/// `1.21.1-forge-52.1.0`, `neoforge-21.1.172`. For Fabric/Quilt it matches the
/// `id` the meta service puts in its profile; for Forge/NeoForge we rewrite the
/// installer's id to this (legacy installers use other ids).
pub fn profile_id(kind: LoaderKind, loader_version: &str, mc: &str) -> String {
    match kind {
        LoaderKind::Fabric => format!("fabric-loader-{loader_version}-{mc}"),
        LoaderKind::Quilt => format!("quilt-loader-{loader_version}-{mc}"),
        LoaderKind::Forge => format!("{mc}-forge-{loader_version}"),
        LoaderKind::NeoForge => format!("neoforge-{loader_version}"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LoaderVersion {
    pub version: String,
    /// No pre-release suffix (`-beta.1`). The meta services' own `stable`
    /// flag is not used: Fabric flags only the single newest loader.
    pub stable: bool,
}

impl LoaderKind {
    fn meta_url(&self) -> &'static str {
        match self {
            LoaderKind::Fabric => "https://meta.fabricmc.net/v2/versions/loader",
            LoaderKind::Quilt => "https://meta.quiltmc.org/v3/versions/loader",
            // Forge/NeoForge have no meta service of this shape (see `forge`).
            LoaderKind::Forge | LoaderKind::NeoForge => "",
        }
    }
}

/// Version strings go into URLs and file names: allow only plain characters.
pub(crate) fn check_segment(s: &str) -> Result<()> {
    let ok = !s.is_empty()
        && s != ".."
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-'))
        && !s.contains("..");
    if ok {
        Ok(())
    } else {
        Err(Error::UnsafePath(s.to_string()))
    }
}

fn split_pre(v: &str) -> (&str, Option<&str>) {
    match v.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (v, None),
    }
}

fn cmp_core(a: &str, b: &str) -> Ordering {
    let nums = |s: &str| s.split('.').map(|p| p.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    let (x, y) = (nums(a), nums(b));
    for i in 0..x.len().max(y.len()) {
        let o = x.get(i).unwrap_or(&0).cmp(y.get(i).unwrap_or(&0));
        if o != Ordering::Equal {
            return o;
        }
    }
    Ordering::Equal
}

fn cmp_pre(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.split('.'), b.split('.'));
    loop {
        match (x.next(), y.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(p), Some(q)) => {
                let o = match (p.parse::<u64>(), q.parse::<u64>()) {
                    (Ok(m), Ok(n)) => m.cmp(&n),
                    _ => p.cmp(q),
                };
                if o != Ordering::Equal {
                    return o;
                }
            }
        }
    }
}

/// Semver-style ordering: numeric core, then a release beats its pre-releases.
pub(crate) fn cmp_versions(a: &str, b: &str) -> Ordering {
    let ((ac, ap), (bc, bp)) = (split_pre(a), split_pre(b));
    cmp_core(ac, bc).then_with(|| match (ap, bp) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => cmp_pre(x, y),
    })
}

/// A meta service saying "no such version": `Downloader` reports 4xx either as
/// `Permanent` or as a download failure naming the status.
fn is_unsupported(e: &Error) -> bool {
    match e {
        Error::Permanent(_) => true,
        Error::Download { reason, .. } => reason.contains("HTTP 400") || reason.contains("HTTP 404"),
        _ => false,
    }
}

/// Loader versions available for Minecraft `mc`, newest first. An empty list
/// means this loader does not support `mc`.
pub async fn list_loader_versions(dl: &Downloader, kind: LoaderKind, mc: &str) -> Result<Vec<LoaderVersion>> {
    #[derive(Deserialize)]
    struct Entry {
        loader: Inner,
    }
    #[derive(Deserialize)]
    struct Inner {
        version: String,
    }
    check_segment(mc)?;
    if kind.runs_installer() {
        return crate::forge::list_versions(dl, kind, mc).await;
    }
    let entries: Vec<Entry> = match dl.fetch_json(&format!("{}/{mc}", kind.meta_url())).await {
        Ok(v) => v,
        // The live services answer an unsupported version with `400` + `[]`.
        Err(e) if is_unsupported(&e) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut list: Vec<LoaderVersion> = entries
        .into_iter()
        .map(|e| LoaderVersion { stable: !e.loader.version.contains('-'), version: e.loader.version })
        .collect();
    list.sort_by(|a, b| cmp_versions(&b.version, &a.version));
    Ok(list)
}

/// The version to preselect: the newest stable one, else the newest of all.
pub fn default_version(list: &[LoaderVersion]) -> Option<&LoaderVersion> {
    list.iter().find(|v| v.stable).or_else(|| list.first())
}

/// Make sure `versions/<profile id>/<profile id>.json` exists and return the
/// profile id. Does nothing (and makes no request) if it is already there.
pub async fn fetch_profile(
    dl: &Downloader,
    dirs: &Dirs,
    kind: LoaderKind,
    mc: &str,
    loader_version: &str,
) -> Result<String> {
    check_segment(mc)?;
    check_segment(loader_version)?;
    let id = profile_id(kind, loader_version, mc);
    let path = version::version_json_path(&dirs.versions(), &id)?;
    if path.is_file() {
        return Ok(id);
    }
    let url = format!("{}/{mc}/{loader_version}/profile/json", kind.meta_url());
    let bytes = match dl.fetch(&url).await {
        Ok(b) => b,
        Err(e) if is_unsupported(&e) => {
            return Err(Error::Other(format!(
                "{} 不支持 Minecraft {mc}（或没有加载器版本 {loader_version}）",
                kind.label()
            )))
        }
        Err(e) => return Err(e),
    };
    let json: VersionJson = serde_json::from_slice(&bytes)?;
    if json.id != id {
        return Err(Error::Other(format!("加载器配置的 id 异常: {}（应为 {id}）", json.id)));
    }
    if json.inherits_from.as_deref() != Some(mc) {
        return Err(Error::Other(format!("加载器配置不是基于 Minecraft {mc}")));
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &bytes)?;
    std::fs::rename(&tmp, &path)?;
    Ok(id)
}

/// Download the loader and everything needed to launch it (vanilla jar,
/// libraries, assets). Safe to call repeatedly. Forge and NeoForge run their
/// installer's steps with `java` (required for them, ignored for Fabric/Quilt).
pub async fn install_loader(
    dl: &Downloader,
    dirs: &Dirs,
    kind: LoaderKind,
    mc: &str,
    loader_version: &str,
    java: Option<&Path>,
    progress: Option<ProgressFn>,
) -> Result<ResolvedVersion> {
    let id = if kind.runs_installer() {
        let java = java.ok_or_else(|| Error::Other(format!("安装 {} 需要 Java", kind.label())))?;
        crate::forge::install_forge_like(dl, dirs, kind, mc, loader_version, java, progress.clone()).await?
    } else {
        fetch_profile(dl, dirs, kind, mc, loader_version).await?
    };
    install::install_version(dl, dirs, &id, progress).await
}
