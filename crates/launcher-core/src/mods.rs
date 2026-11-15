//! Mods: a small Modrinth client, dependency planning and installation into an
//! instance's `mods` folder.

use crate::config::{load_or_default, save_atomic, Dirs};
use crate::download::{DownloadTask, Downloader, ProgressFn};
use crate::loader::LoaderKind;
use crate::{Error, Result};
use reqwest::{header, StatusCode, Url};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::time::Duration;

/// Modrinth asks for a unique User-Agent; this one carries no personal data.
const USER_AGENT: &str = "voxel-launcher/0.1";
const MODRINTH_API: &str = "https://api.modrinth.com/v2";
const PAGE_SIZE: u32 = 20;
const MAX_ATTEMPTS: u32 = 3;

/// What kind of Modrinth project goes into which folder of an instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentKind {
    #[default]
    Mod,
    Shader,
    ResourcePack,
}

impl ContentKind {
    /// Folder inside the instance directory.
    pub fn dir_name(&self) -> &'static str {
        match self {
            ContentKind::Mod => "mods",
            ContentKind::Shader => "shaderpacks",
            ContentKind::ResourcePack => "resourcepacks",
        }
    }
    pub fn extension(&self) -> &'static str {
        match self {
            ContentKind::Mod => ".jar",
            ContentKind::Shader | ContentKind::ResourcePack => ".zip",
        }
    }
    /// Modrinth `project_type`.
    pub fn project_type(&self) -> &'static str {
        match self {
            ContentKind::Mod => "mod",
            ContentKind::Shader => "shader",
            ContentKind::ResourcePack => "resourcepack",
        }
    }
    /// Name of the per-instance record file (`.voxel/<name>.json`).
    pub fn record_name(&self) -> &'static str {
        match self {
            ContentKind::Mod => "mods",
            ContentKind::Shader => "shaders",
            ContentKind::ResourcePack => "resourcepacks",
        }
    }
}

/// What an instance needs from a project: its Minecraft version and the
/// loaders whose files it can use (Quilt runs Fabric mods too).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub mc_version: String,
    pub loaders: Vec<String>,
    pub content: ContentKind,
}

pub fn target_for(mc: &str, kind: LoaderKind) -> Target {
    let loaders = match kind {
        LoaderKind::Fabric => vec!["fabric".to_string()],
        LoaderKind::Quilt => vec!["quilt".to_string(), "fabric".to_string()],
        LoaderKind::Forge => vec!["forge".to_string()],
        LoaderKind::NeoForge => vec!["neoforge".to_string()],
    };
    Target { mc_version: mc.to_string(), loaders, content: ContentKind::Mod }
}

/// Target for shaders (Iris) or resource packs (loader-independent); mods as `target_for`.
pub fn content_target(mc: &str, loader: LoaderKind, kind: ContentKind) -> Target {
    match kind {
        ContentKind::Mod => target_for(mc, loader),
        ContentKind::Shader => Target { mc_version: mc.to_string(), loaders: vec!["iris".to_string()], content: kind },
        ContentKind::ResourcePack => Target { mc_version: mc.to_string(), loaders: vec!["minecraft".to_string()], content: kind },
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModHit {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub author: String,
}

/// How to order and narrow a search. Both come from the UI, so both are checked.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SearchOpts {
    /// `relevance`, `downloads`, `follows`, `newest` or `updated`.
    pub sort: Option<String>,
    /// One Modrinth category, such as `optimization`.
    pub category: Option<String>,
}

impl SearchOpts {
    fn check(&self) -> Result<()> {
        if let Some(s) = &self.sort {
            if !["relevance", "downloads", "follows", "newest", "updated"].contains(&s.as_str()) {
                return Err(Error::Other(format!("无效的排序方式: {s}")));
            }
        }
        if let Some(c) = &self.category {
            let ok = !c.is_empty() && c.len() <= 40 && c.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-');
            if !ok {
                return Err(Error::Other(format!("无效的分类: {c}")));
            }
        }
        Ok(())
    }

    fn add_category(&self, facets: &mut serde_json::Value) {
        if let (Some(c), Some(list)) = (&self.category, facets.as_array_mut()) {
            list.push(serde_json::json!([format!("categories:{c}")]));
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub hits: Vec<ModHit>,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionFile {
    pub filename: String,
    pub url: String,
    pub sha1: String,
    pub size: u64,
    pub primary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub version_id: Option<String>,
    /// `required`, `optional`, `incompatible` or `embedded`.
    pub dependency_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModVersion {
    pub id: String,
    pub project_id: String,
    pub version_number: String,
    /// `release`, `beta` or `alpha`.
    pub version_type: String,
    pub files: Vec<VersionFile>,
    pub dependencies: Vec<Dependency>,
}

#[derive(Deserialize)]
struct RawVersion {
    id: String,
    project_id: String,
    #[serde(default)]
    version_number: String,
    #[serde(default)]
    version_type: String,
    #[serde(default)]
    files: Vec<RawFile>,
    #[serde(default)]
    dependencies: Vec<Dependency>,
}

#[derive(Deserialize)]
struct RawFile {
    #[serde(default)]
    hashes: RawHashes,
    url: String,
    filename: String,
    #[serde(default)]
    primary: bool,
    #[serde(default)]
    size: u64,
}

#[derive(Deserialize, Default)]
struct RawHashes {
    #[serde(default)]
    sha1: String,
}

impl From<RawVersion> for ModVersion {
    fn from(r: RawVersion) -> Self {
        ModVersion {
            id: r.id,
            project_id: r.project_id,
            version_number: r.version_number,
            version_type: r.version_type,
            files: r
                .files
                .into_iter()
                .map(|f| VersionFile { filename: f.filename, url: f.url, sha1: f.hashes.sha1, size: f.size, primary: f.primary })
                .collect(),
            dependencies: r.dependencies,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct GalleryImage {
    pub url: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DepInfo {
    pub project_id: String,
    pub title: String,
    pub icon_url: Option<String>,
    /// `required` or `optional`.
    pub dependency_type: String,
    pub installed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DetailVersion {
    pub id: String,
    pub version_number: String,
    pub version_type: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub date_published: String,
    pub downloads: u64,
}

/// A project page: what Modrinth shows for it.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectDetail {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    /// Markdown.
    pub body: String,
    pub icon_url: Option<String>,
    pub gallery: Vec<GalleryImage>,
    pub categories: Vec<String>,
    pub downloads: u64,
    pub followers: u64,
    pub license: Option<String>,
    pub source_url: Option<String>,
    pub issues_url: Option<String>,
    pub wiki_url: Option<String>,
    pub loaders: Vec<String>,
    pub game_versions: Vec<String>,
    /// For a given instance: does any version run on it? `None` when no instance was given.
    pub compatible: Option<bool>,
    pub versions: Vec<DetailVersion>,
    pub dependencies: Vec<DepInfo>,
}

/// Project ids and slugs go into URL paths: allow only plain characters.
pub(crate) fn check_id(id: &str) -> Result<()> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && !id.contains("..")
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if ok {
        Ok(())
    } else {
        Err(Error::Other(format!("无效的项目 ID: {id}")))
    }
}

pub struct ModrinthClient {
    http: reqwest::Client,
    base: String,
}

impl ModrinthClient {
    pub fn new(base: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(6))
            .timeout(Duration::from_secs(20))
            .build()
            .expect("failed to build HTTP client");
        Self { http, base: base.into().trim_end_matches('/').to_string() }
    }

    pub fn official() -> Self {
        Self::new(MODRINTH_API)
    }

    pub(crate) fn url(&self, path: &str, params: &[(&str, String)]) -> Result<Url> {
        Url::parse_with_params(&format!("{}{path}", self.base), params)
            .map_err(|e| Error::Other(format!("无效的请求地址: {e}")))
    }

    /// GET JSON. A `429` waits `Retry-After` seconds (default 2, at most 30)
    /// and tries again, up to three attempts in all; other failures are final.
    pub(crate) async fn get_json<T: DeserializeOwned>(&self, url: Url) -> Result<T> {
        for attempt in 1..=MAX_ATTEMPTS {
            let resp = self.http.get(url.clone()).send().await?;
            let status = resp.status();
            if status == StatusCode::TOO_MANY_REQUESTS {
                if attempt == MAX_ATTEMPTS {
                    return Err(Error::Other("Modrinth 请求过于频繁 (HTTP 429)，请稍后再试".into()));
                }
                let secs = resp
                    .headers()
                    .get(header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.trim().parse::<u64>().ok())
                    .unwrap_or(2)
                    .min(30);
                tokio::time::sleep(Duration::from_secs(secs)).await;
                continue;
            }
            if !status.is_success() {
                return Err(Error::Other(format!("Modrinth 请求失败: HTTP {status}")));
            }
            return Ok(resp.json::<T>().await?);
        }
        unreachable!("loop always returns")
    }

    /// Search mods, shaders or resource packs for `t`; an empty `query` lists popular ones.
    pub async fn search(&self, t: &Target, query: &str, offset: u32) -> Result<SearchResult> {
        self.search_with(t, query, offset, &SearchOpts::default()).await
    }

    /// [`Self::search`] with a sort order and/or one category.
    pub async fn search_with(&self, t: &Target, query: &str, offset: u32, opts: &SearchOpts) -> Result<SearchResult> {
        opts.check()?;
        let mut facets = if t.content == ContentKind::Mod {
            let categories: Vec<String> = t.loaders.iter().map(|l| format!("categories:{l}")).collect();
            serde_json::json!([["project_type:mod"], [format!("versions:{}", t.mc_version)], categories])
        } else {
            serde_json::json!([[format!("project_type:{}", t.content.project_type())], [format!("versions:{}", t.mc_version)]])
        };
        opts.add_category(&mut facets);
        self.search_facets_sorted(facets.to_string(), query, offset, opts.sort.as_deref()).await
    }

    /// Search with ready-made facets (a JSON array of arrays).
    pub(crate) async fn search_facets(&self, facets: String, query: &str, offset: u32) -> Result<SearchResult> {
        self.search_facets_sorted(facets, query, offset, None).await
    }

    pub(crate) async fn search_facets_sorted(&self, facets: String, query: &str, offset: u32, sort: Option<&str>) -> Result<SearchResult> {
        #[derive(Deserialize)]
        struct Raw {
            hits: Vec<ModHit>,
            #[serde(default)]
            total_hits: u64,
        }
        let mut params = vec![
            ("query", query.to_string()),
            ("facets", facets),
            ("limit", PAGE_SIZE.to_string()),
            ("offset", offset.to_string()),
        ];
        if let Some(s) = sort {
            params.push(("index", s.to_string()));
        }
        let url = self.url("/search", &params)?;
        let raw: Raw = self.get_json(url).await?;
        Ok(SearchResult { hits: raw.hits, total: raw.total_hits })
    }

    /// Search every project of `kind`, whatever it runs on.
    pub async fn search_any(&self, kind: ContentKind, query: &str, offset: u32) -> Result<SearchResult> {
        self.search_any_with(kind, query, offset, &SearchOpts::default()).await
    }

    pub async fn search_any_with(&self, kind: ContentKind, query: &str, offset: u32, opts: &SearchOpts) -> Result<SearchResult> {
        opts.check()?;
        let mut facets = serde_json::json!([[format!("project_type:{}", kind.project_type())]]);
        opts.add_category(&mut facets);
        self.search_facets_sorted(facets.to_string(), query, offset, opts.sort.as_deref()).await
    }

    /// Everything a project page shows. With a target, versions and prerequisites
    /// are those that run on it, and `compatible` says whether any does;
    /// `installed` (project ids) marks prerequisites that are already there.
    pub async fn project_detail(&self, project: &str, t: Option<&Target>, installed: &HashSet<String>) -> Result<ProjectDetail> {
        #[derive(Deserialize)]
        struct RawLicense {
            #[serde(default)]
            name: Option<String>,
        }
        #[derive(Deserialize)]
        struct RawGallery {
            url: String,
            #[serde(default)]
            title: Option<String>,
        }
        #[derive(Deserialize)]
        struct RawProject {
            id: String,
            slug: String,
            title: String,
            #[serde(default)]
            description: String,
            #[serde(default)]
            body: String,
            #[serde(default)]
            icon_url: Option<String>,
            #[serde(default)]
            gallery: Vec<RawGallery>,
            #[serde(default)]
            categories: Vec<String>,
            #[serde(default)]
            downloads: u64,
            #[serde(default)]
            followers: u64,
            #[serde(default)]
            license: Option<RawLicense>,
            #[serde(default)]
            source_url: Option<String>,
            #[serde(default)]
            issues_url: Option<String>,
            #[serde(default)]
            wiki_url: Option<String>,
            #[serde(default)]
            loaders: Vec<String>,
            #[serde(default)]
            game_versions: Vec<String>,
        }
        #[derive(Deserialize)]
        struct RawDetailVersion {
            id: String,
            #[serde(default)]
            version_number: String,
            #[serde(default)]
            version_type: String,
            #[serde(default)]
            game_versions: Vec<String>,
            #[serde(default)]
            loaders: Vec<String>,
            #[serde(default)]
            date_published: String,
            #[serde(default)]
            downloads: u64,
            #[serde(default)]
            dependencies: Vec<Dependency>,
        }

        check_id(project)?;
        let p: RawProject = self.get_json(self.url(&format!("/project/{project}"), &[])?).await?;
        let params: Vec<(&str, String)> = match t {
            Some(t) => vec![
                ("game_versions", serde_json::json!([t.mc_version]).to_string()),
                ("loaders", serde_json::json!(t.loaders).to_string()),
            ],
            None => vec![],
        };
        let raw: Vec<RawDetailVersion> = self.get_json(self.url(&format!("/project/{project}/version"), &params)?).await?;

        // Prerequisites come from the newest version that applies.
        let mut wanted: Vec<(String, String)> = Vec::new();
        if let Some(newest) = raw.first() {
            for d in &newest.dependencies {
                if let (Some(id), "required" | "optional") = (&d.project_id, d.dependency_type.as_str()) {
                    if id != &p.id && !wanted.iter().any(|(w, _)| w == id) {
                        wanted.push((id.clone(), d.dependency_type.clone()));
                    }
                }
            }
        }
        let ids: Vec<String> = wanted.iter().map(|(id, _)| id.clone()).collect();
        let info = if ids.is_empty() { HashMap::new() } else { self.projects_info(&ids).await? };
        let dependencies = wanted
            .into_iter()
            .map(|(id, kind)| {
                let (title, icon_url) = info.get(&id).cloned().unwrap_or_else(|| (id.clone(), None));
                DepInfo { installed: installed.contains(&id), project_id: id, title, icon_url, dependency_type: kind }
            })
            .collect();

        Ok(ProjectDetail {
            id: p.id,
            slug: p.slug,
            title: p.title,
            description: p.description,
            body: p.body,
            icon_url: p.icon_url,
            gallery: p.gallery.into_iter().map(|g| GalleryImage { url: g.url, title: g.title }).collect(),
            categories: p.categories,
            downloads: p.downloads,
            followers: p.followers,
            license: p.license.and_then(|l| l.name),
            source_url: p.source_url,
            issues_url: p.issues_url,
            wiki_url: p.wiki_url,
            loaders: p.loaders,
            game_versions: p.game_versions,
            compatible: t.map(|_| !raw.is_empty()),
            versions: raw
                .into_iter()
                .take(30)
                .map(|v| DetailVersion {
                    id: v.id,
                    version_number: v.version_number,
                    version_type: v.version_type,
                    game_versions: v.game_versions,
                    loaders: v.loaders,
                    date_published: v.date_published,
                    downloads: v.downloads,
                })
                .collect(),
            dependencies,
        })
    }

    /// Versions of `project` that run on `t`, newest first.
    pub async fn project_versions(&self, project: &str, t: &Target) -> Result<Vec<ModVersion>> {
        check_id(project)?;
        let url = self.url(
            &format!("/project/{project}/version"),
            &[
                ("game_versions", serde_json::json!([t.mc_version]).to_string()),
                ("loaders", serde_json::json!(t.loaders).to_string()),
            ],
        )?;
        let raw: Vec<RawVersion> = self.get_json(url).await?;
        Ok(raw.into_iter().map(Into::into).collect())
    }

    pub async fn version_by_id(&self, id: &str) -> Result<ModVersion> {
        check_id(id)?;
        let raw: RawVersion = self.get_json(self.url(&format!("/version/{id}"), &[])?).await?;
        Ok(raw.into())
    }

    /// `(title, icon url)` of a project, for showing dependencies.
    pub async fn project_title(&self, id: &str) -> Result<(String, Option<String>)> {
        #[derive(Deserialize)]
        struct Raw {
            title: String,
            #[serde(default)]
            icon_url: Option<String>,
        }
        check_id(id)?;
        let raw: Raw = self.get_json(self.url(&format!("/project/{id}"), &[])?).await?;
        Ok((raw.title, raw.icon_url))
    }

    /// POST JSON; retries a `429` like [`Self::get_json`].
    async fn post_json<T: DeserializeOwned>(&self, url: Url, body: &serde_json::Value) -> Result<T> {
        for attempt in 1..=MAX_ATTEMPTS {
            let resp = self.http.post(url.clone()).json(body).send().await?;
            let status = resp.status();
            if status == StatusCode::TOO_MANY_REQUESTS && attempt < MAX_ATTEMPTS {
                let secs = resp
                    .headers()
                    .get(header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.trim().parse::<u64>().ok())
                    .unwrap_or(2)
                    .min(30);
                tokio::time::sleep(Duration::from_secs(secs)).await;
                continue;
            }
            if !status.is_success() {
                return Err(Error::Other(format!("Modrinth 请求失败: HTTP {status}")));
            }
            return Ok(resp.json::<T>().await?);
        }
        unreachable!("loop always returns")
    }

    /// The version each file (by sha1) belongs to; unknown files are absent.
    pub async fn versions_by_hashes(&self, hashes: &[String]) -> Result<HashMap<String, ModVersion>> {
        let mut out = HashMap::new();
        for chunk in hashes.chunks(100) {
            let body = serde_json::json!({ "hashes": chunk, "algorithm": "sha1" });
            let raw: HashMap<String, RawVersion> = self.post_json(self.url("/version_files", &[])?, &body).await?;
            out.extend(raw.into_iter().map(|(h, v)| (h, v.into())));
        }
        Ok(out)
    }

    /// The newest version for `t` of the project each file (by sha1) belongs to.
    pub async fn latest_by_hashes(&self, hashes: &[String], t: &Target) -> Result<HashMap<String, ModVersion>> {
        let mut out = HashMap::new();
        for chunk in hashes.chunks(100) {
            let body = serde_json::json!({
                "hashes": chunk, "algorithm": "sha1",
                "loaders": t.loaders, "game_versions": [t.mc_version],
            });
            let raw: HashMap<String, RawVersion> = self.post_json(self.url("/version_files/update", &[])?, &body).await?;
            out.extend(raw.into_iter().map(|(h, v)| (h, v.into())));
        }
        Ok(out)
    }

    /// `(title, icon url)` per project id.
    pub async fn projects_info(&self, ids: &[String]) -> Result<HashMap<String, (String, Option<String>)>> {
        #[derive(Deserialize)]
        struct Raw {
            id: String,
            title: String,
            #[serde(default)]
            icon_url: Option<String>,
        }
        let mut out = HashMap::new();
        for chunk in ids.chunks(100) {
            for id in chunk {
                check_id(id)?;
            }
            let url = self.url("/projects", &[("ids", serde_json::json!(chunk).to_string())])?;
            let raw: Vec<Raw> = self.get_json(url).await?;
            out.extend(raw.into_iter().map(|p| (p.id, (p.title, p.icon_url))));
        }
        Ok(out)
    }

    /// The project id behind a slug (installed-mod records and dependencies use ids).
    pub async fn project_id(&self, slug: &str) -> Result<String> {
        #[derive(Deserialize)]
        struct Raw {
            id: String,
        }
        check_id(slug)?;
        let raw: Raw = self.get_json(self.url(&format!("/project/{slug}"), &[])?).await?;
        Ok(raw.id)
    }
}

// ---------------------------------------------------------------- planning

/// One file to download, with why it is part of the plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanFile {
    pub project_id: String,
    pub version_id: String,
    pub title: String,
    pub version_number: String,
    pub icon_url: Option<String>,
    pub filename: String,
    pub url: String,
    pub sha1: String,
    pub size: u64,
    /// Title of the mod that needs this one (dependencies only).
    pub required_by: Option<String>,
    pub user_selected: bool,
    /// Which folder the file goes to.
    #[serde(default)]
    pub dest: ContentKind,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstallPlan {
    pub files: Vec<PlanFile>,
    pub total_bytes: u64,
    pub warnings: Vec<String>,
}

/// Work out what installing `project_id` needs, without downloading anything.
/// `installed` holds the project ids already in the instance (disabled ones
/// included); they are never planned again.
pub async fn plan_install(
    client: &ModrinthClient,
    t: &Target,
    project_id: &str,
    installed: &HashSet<String>,
) -> Result<InstallPlan> {
    let loader = t.loaders.first().map(String::as_str).unwrap_or("fabric");
    let mut plan = InstallPlan::default();
    let mut seen: HashSet<String> = installed.clone();
    // (project id or slug, title of the mod that requires it)
    let mut queue: VecDeque<(String, Option<String>)> = VecDeque::from([(project_id.to_string(), None)]);

    while let Some((pid, required_by)) = queue.pop_front() {
        let user_selected = required_by.is_none();
        let (title, icon_url) = client.project_title(&pid).await?;
        let versions = client.project_versions(&pid, t).await?;
        let none_found = || match &required_by {
            Some(by) => Error::Other(format!("依赖项「{title}」（{by} 需要）没有适配 {} + {loader} 的版本", t.mc_version)),
            None => Error::Other(format!("「{title}」没有适配 {} + {loader} 的版本", t.mc_version)),
        };
        let chosen = match versions.iter().find(|v| v.version_type == "release") {
            Some(v) => v,
            None => {
                let v = versions.first().ok_or_else(none_found)?;
                plan.warnings.push(format!("「{title}」只有 {} 版本，可能不稳定", v.version_type));
                v
            }
        };
        if !seen.insert(chosen.project_id.clone()) {
            if user_selected {
                return Err(Error::Other(format!("「{title}」已安装")));
            }
            continue; // dependency already installed or already planned (also ends cycles)
        }
        let file = chosen
            .files
            .iter()
            .find(|f| f.primary)
            .or_else(|| chosen.files.first())
            .ok_or_else(|| Error::Other(format!("「{title}」的版本 {} 没有可下载的文件", chosen.version_number)))?;
        plan.total_bytes += file.size;
        plan.files.push(PlanFile {
            project_id: chosen.project_id.clone(),
            version_id: chosen.id.clone(),
            title: title.clone(),
            version_number: chosen.version_number.clone(),
            icon_url,
            filename: file.filename.clone(),
            url: file.url.clone(),
            sha1: file.sha1.clone(),
            size: file.size,
            required_by,
            user_selected,
            dest: ContentKind::Mod,
        });

        for dep in &chosen.dependencies {
            let dep_project = match (&dep.project_id, &dep.version_id) {
                (Some(p), _) => p.clone(),
                (None, Some(v)) => client.version_by_id(v).await?.project_id,
                (None, None) => continue,
            };
            match dep.dependency_type.as_str() {
                "required" if !seen.contains(&dep_project) => queue.push_back((dep_project, Some(title.clone()))),
                "incompatible" if installed.contains(&dep_project) => {
                    let other = client.project_title(&dep_project).await.map(|(t, _)| t).unwrap_or(dep_project);
                    plan.warnings.push(format!("「{title}」与已安装的「{other}」不兼容"));
                }
                _ => {}
            }
        }
    }
    Ok(plan)
}

/// Plan one project of any kind. Mods use [`plan_install`]. A shader or resource
/// pack is its newest release for `mc`; a shader also needs Iris (with Sodium)
/// unless the instance has it, and on Forge we only warn because Iris does not exist there.
pub async fn plan_content(
    client: &ModrinthClient,
    loader: LoaderKind,
    mc: &str,
    kind: ContentKind,
    project: &str,
    installed_mods: &HashSet<String>,
    installed_content: &HashSet<String>,
) -> Result<InstallPlan> {
    if kind == ContentKind::Mod {
        return plan_install(client, &target_for(mc, loader), project, installed_mods).await;
    }
    let t = content_target(mc, loader, kind);
    let (title, icon_url) = client.project_title(project).await?;
    let versions = client.project_versions(project, &t).await?;
    let mut plan = InstallPlan::default();
    let chosen = match versions.iter().find(|v| v.version_type == "release") {
        Some(v) => v,
        None => {
            let v = versions.first().ok_or_else(|| Error::Other(format!("「{title}」没有适配 {mc} 的版本")))?;
            plan.warnings.push(format!("「{title}」只有 {} 版本，可能不稳定", v.version_type));
            v
        }
    };
    if installed_content.contains(&chosen.project_id) {
        return Err(Error::Other(format!("「{title}」已安装")));
    }
    let file = chosen
        .files
        .iter()
        .find(|f| f.primary)
        .or_else(|| chosen.files.first())
        .ok_or_else(|| Error::Other(format!("「{title}」的版本 {} 没有可下载的文件", chosen.version_number)))?;
    plan.total_bytes += file.size;
    plan.files.push(PlanFile {
        project_id: chosen.project_id.clone(),
        version_id: chosen.id.clone(),
        title: title.clone(),
        version_number: chosen.version_number.clone(),
        icon_url,
        filename: file.filename.clone(),
        url: file.url.clone(),
        sha1: file.sha1.clone(),
        size: file.size,
        required_by: None,
        user_selected: true,
        dest: kind,
    });

    if kind == ContentKind::Shader {
        if loader == LoaderKind::Forge {
            plan.warnings.push("Forge 上的光影需要 Oculus 或 OptiFine，启动器暂不自动安装".to_string());
        } else {
            let iris_id = client.project_id("iris").await?;
            if !installed_mods.contains(&iris_id) {
                let sub = plan_install(client, &target_for(mc, loader), "iris", installed_mods).await?;
                for mut f in sub.files {
                    f.user_selected = false;
                    if f.project_id == iris_id {
                        f.required_by = Some(title.clone());
                    }
                    plan.total_bytes += f.size;
                    plan.files.push(f);
                }
                plan.warnings.extend(sub.warnings);
            }
        }
    }
    Ok(plan)
}

// ------------------------------------------------------------ installation

#[derive(Debug, Clone, Serialize)]
pub struct InstalledMod {
    /// File name without the `.disabled` suffix.
    pub filename: String,
    pub enabled: bool,
    pub size: u64,
    pub project_id: Option<String>,
    pub title: Option<String>,
    pub version_number: Option<String>,
    pub icon_url: Option<String>,
    /// Chosen by the user (or added by hand) rather than pulled in as a dependency.
    pub user_installed: bool,
    /// The id the loader knows the mod by, read from the jar (mods only).
    pub mod_id: Option<String>,
    /// Another enabled file carries the same `mod_id`: the game would refuse to start.
    pub duplicate: bool,
}

/// The mod id declared inside a jar (Fabric, Quilt, Forge or NeoForge); `None`
/// when the file is unreadable or declares none.
pub fn mod_id_of_jar(path: &std::path::Path) -> Option<String> {
    use std::io::Read;
    fn read(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<String> {
        let mut f = zip.by_name(name).ok()?;
        let mut s = String::new();
        f.by_ref().take(512 * 1024).read_to_string(&mut s).ok()?;
        Some(s)
    }
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).ok()?).ok()?;
    if let Some(v) = read(&mut zip, "fabric.mod.json").and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()) {
        if let Some(id) = v.get("id").and_then(|x| x.as_str()) {
            return Some(id.to_string());
        }
    }
    if let Some(v) = read(&mut zip, "quilt.mod.json").and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()) {
        if let Some(id) = v.pointer("/quilt_loader/id").and_then(|x| x.as_str()) {
            return Some(id.to_string());
        }
    }
    let re = regex::Regex::new(r#"(?m)^\s*modId\s*=\s*["']([^"']+)["']"#).expect("static regex");
    for name in ["META-INF/neoforge.mods.toml", "META-INF/mods.toml"] {
        if let Some(text) = read(&mut zip, name) {
            if let Some(c) = re.captures(&text) {
                return Some(c[1].to_string());
            }
        }
    }
    None
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RecordEntry {
    project_id: String,
    version_id: String,
    version_number: String,
    title: String,
    icon_url: Option<String>,
    sha1: String,
    user_installed: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
struct Record {
    version: u32,
    mods: BTreeMap<String, RecordEntry>,
}

impl Default for Record {
    fn default() -> Self {
        Self { version: 1, mods: BTreeMap::new() }
    }
}

fn load_record(dirs: &Dirs, instance_id: &str, kind: ContentKind) -> Result<Record> {
    Ok(load_or_default(&dirs.content_record(instance_id, kind)?))
}

/// File names come from the network or the UI: plain names with the
/// extension of the folder they go to.
fn check_filename(name: &str, kind: ContentKind) -> Result<()> {
    let bad = name.is_empty()
        || name.len() > 200
        || name.contains(['/', '\\', ':', '\0'])
        || name.contains("..")
        || !name.to_ascii_lowercase().ends_with(kind.extension());
    if bad {
        Err(Error::UnsafePath(name.to_string()))
    } else {
        Ok(())
    }
}

fn disabled_name(filename: &str) -> String {
    format!("{filename}.disabled")
}

/// Download every file of `plan` into the folder of its kind and record where
/// each came from. File names are validated before anything is written. A
/// failed file does not stop the others: files that arrived intact are kept
/// and recorded, the first error is returned, and no `.part` is left behind.
pub async fn install_plan(
    dl: &Downloader,
    dirs: &Dirs,
    instance_id: &str,
    plan: &InstallPlan,
    progress: Option<ProgressFn>,
) -> Result<()> {
    for f in &plan.files {
        check_filename(&f.filename, f.dest)?;
    }
    let mut tasks = Vec::new();
    for f in &plan.files {
        let dir = dirs.content_dir(instance_id, f.dest)?;
        std::fs::create_dir_all(&dir)?;
        tasks.push(DownloadTask::rel(&dir, &f.filename, f.url.clone())?.sha1(Some(f.sha1.clone())).size(Some(f.size)));
    }
    let results = dl.download_all_settled(tasks, "下载", progress).await;

    let mut records: BTreeMap<&'static str, (ContentKind, Record)> = BTreeMap::new();
    let mut first_err = None;
    for (f, result) in plan.files.iter().zip(results) {
        let dir = dirs.content_dir(instance_id, f.dest)?;
        let record = match records.entry(f.dest.dir_name()) {
            std::collections::btree_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::btree_map::Entry::Vacant(e) => e.insert((f.dest, load_record(dirs, instance_id, f.dest)?)),
        };
        match result {
            Ok(()) => {
                // Same project under another file name: this is an update.
                let old: Vec<String> = record
                    .1
                    .mods
                    .iter()
                    .filter(|(name, e)| e.project_id == f.project_id && **name != f.filename)
                    .map(|(name, _)| name.clone())
                    .collect();
                for name in old {
                    let _ = std::fs::remove_file(dir.join(&name));
                    let _ = std::fs::remove_file(dir.join(disabled_name(&name)));
                    record.1.mods.remove(&name);
                }
                record.1.mods.insert(
                    f.filename.clone(),
                    RecordEntry {
                        project_id: f.project_id.clone(),
                        version_id: f.version_id.clone(),
                        version_number: f.version_number.clone(),
                        title: f.title.clone(),
                        icon_url: f.icon_url.clone(),
                        sha1: f.sha1.clone(),
                        user_installed: f.user_selected,
                    },
                );
            }
            Err(e) => {
                let _ = std::fs::remove_file(dir.join(format!("{}.part", f.filename)));
                first_err.get_or_insert(e);
            }
        }
    }
    for (kind, record) in records.values() {
        save_atomic(&dirs.content_record(instance_id, *kind)?, record)?;
    }
    match first_err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Everything in the instance's folder for `kind`: files with the kind's
/// extension (and, for mods, `*.jar.disabled`), joined with what we recorded.
pub fn list_content(dirs: &Dirs, instance_id: &str, kind: ContentKind) -> Result<Vec<InstalledMod>> {
    let dir = dirs.content_dir(instance_id, kind)?;
    let record = load_record(dirs, instance_id, kind)?;
    let ext = kind.extension();
    let mut out = Vec::new();
    let read = match std::fs::read_dir(&dir) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e.into()),
    };
    for entry in read {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let (filename, enabled) = if name.to_ascii_lowercase().ends_with(ext) {
            (name, true)
        } else if let Some(base) = name.strip_suffix(".disabled").filter(|b| b.to_ascii_lowercase().ends_with(ext)) {
            (base.to_string(), false)
        } else {
            continue;
        };
        let rec = record.mods.get(&filename);
        let mod_id = (kind == ContentKind::Mod).then(|| mod_id_of_jar(&entry.path())).flatten();
        out.push(InstalledMod {
            mod_id,
            duplicate: false,
            size: entry.metadata()?.len(),
            project_id: rec.map(|r| r.project_id.clone()),
            title: rec.map(|r| r.title.clone()),
            version_number: rec.map(|r| r.version_number.clone()),
            icon_url: rec.and_then(|r| r.icon_url.clone()),
            user_installed: rec.is_none_or(|r| r.user_installed),
            filename,
            enabled,
        });
    }
    out.sort_by_key(|m| m.filename.to_lowercase());
    let mut seen: HashMap<String, usize> = HashMap::new();
    for m in out.iter().filter(|m| m.enabled) {
        if let Some(id) = &m.mod_id {
            *seen.entry(id.clone()).or_default() += 1;
        }
    }
    for m in out.iter_mut() {
        m.duplicate = m.enabled && m.mod_id.as_ref().is_some_and(|id| seen[id] > 1);
    }
    Ok(out)
}

/// Switch off every copy of a duplicated mod except the most recently modified
/// one (ties: the later file name). Files are renamed to `.disabled`, never
/// deleted, so this can be undone. Returns the file names that were disabled.
pub fn disable_duplicates(dirs: &Dirs, instance_id: &str) -> Result<Vec<String>> {
    let dir = dirs.content_dir(instance_id, ContentKind::Mod)?;
    let mut groups: BTreeMap<String, Vec<(std::time::SystemTime, String)>> = BTreeMap::new();
    for m in list_installed(dirs, instance_id)?.into_iter().filter(|m| m.duplicate) {
        let modified = std::fs::metadata(dir.join(&m.filename)).and_then(|x| x.modified()).unwrap_or(std::time::UNIX_EPOCH);
        groups.entry(m.mod_id.unwrap_or_default()).or_default().push((modified, m.filename));
    }
    let mut disabled = Vec::new();
    for (_, mut files) in groups {
        files.sort();
        files.pop(); // the newest stays
        for (_, name) in files {
            if set_enabled(dirs, instance_id, &name, false).is_ok() {
                disabled.push(name);
            }
        }
    }
    disabled.sort();
    Ok(disabled)
}

/// A newer version of an installed file, ready to hand to [`install_plan`].
#[derive(Debug, Clone, Serialize)]
pub struct UpdateItem {
    pub filename: String,
    pub title: String,
    pub current_version: String,
    pub new_version: String,
    pub file: PlanFile,
}

/// A required prerequisite that no installed file provides.
#[derive(Debug, Clone, Serialize)]
pub struct MissingDep {
    pub project_id: String,
    pub title: String,
    pub icon_url: Option<String>,
    /// Titles of the installed mods that need it.
    pub needed_by: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ScanReport {
    /// Files that had no record and were named from Modrinth this time.
    pub identified: usize,
    pub updates: Vec<UpdateItem>,
    pub missing: Vec<MissingDep>,
}

/// Look every file of `kind` up on Modrinth by hash: name the ones we did not
/// install ourselves (hand-added, from a modpack), and list newer versions for
/// `t`. Disabled files are named but never offered an update (updating would
/// switch them back on), and an alpha/beta never replaces a release.
pub async fn scan_content(
    client: &ModrinthClient,
    dirs: &Dirs,
    instance_id: &str,
    kind: ContentKind,
    t: &Target,
) -> Result<ScanReport> {
    let files = list_content(dirs, instance_id, kind)?;
    if files.is_empty() {
        return Ok(ScanReport::default());
    }
    let dir = dirs.content_dir(instance_id, kind)?;
    let mut by_hash: HashMap<String, (String, bool)> = HashMap::new();
    for f in &files {
        let on_disk = if f.enabled { dir.join(&f.filename) } else { dir.join(disabled_name(&f.filename)) };
        by_hash.insert(crate::download::sha1_file(&on_disk).await?, (f.filename.clone(), f.enabled));
    }
    let hashes: Vec<String> = by_hash.keys().cloned().collect();
    let current = client.versions_by_hashes(&hashes).await?;
    if current.is_empty() {
        return Ok(ScanReport::default());
    }
    // Required prerequisites of enabled mods that no installed file provides.
    let have: HashSet<&str> = current.values().map(|v| v.project_id.as_str()).collect();
    let mut needed: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (hash, ver) in &current {
        if !by_hash[hash].1 {
            continue;
        }
        for d in &ver.dependencies {
            if let (Some(id), "required") = (&d.project_id, d.dependency_type.as_str()) {
                if !have.contains(id.as_str()) {
                    needed.entry(id.clone()).or_default().push(ver.project_id.clone());
                }
            }
        }
    }
    let project_ids: Vec<String> = current
        .values()
        .map(|v| v.project_id.clone())
        .chain(needed.keys().cloned())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let info = client.projects_info(&project_ids).await?;
    let title_of = |id: &str| info.get(id).map(|(t, _)| t.clone()).unwrap_or_else(|| id.to_string());
    let mut report = ScanReport::default();
    for (id, users) in &needed {
        let mut needed_by: Vec<String> = users.iter().map(|u| title_of(u)).collect();
        needed_by.sort();
        needed_by.dedup();
        report.missing.push(MissingDep { project_id: id.clone(), title: title_of(id), icon_url: info.get(id).and_then(|(_, i)| i.clone()), needed_by });
    }

    let mut record = load_record(dirs, instance_id, kind)?;
    for (hash, ver) in &current {
        let Some((filename, _)) = by_hash.get(hash) else { continue };
        let (title, icon_url) = info.get(&ver.project_id).cloned().unwrap_or_else(|| (filename.clone(), None));
        let known = record.mods.get(filename).is_some_and(|r| r.sha1 == *hash);
        if !known {
            report.identified += 1;
            record.mods.insert(
                filename.clone(),
                RecordEntry {
                    project_id: ver.project_id.clone(),
                    version_id: ver.id.clone(),
                    version_number: ver.version_number.clone(),
                    title,
                    icon_url,
                    sha1: hash.clone(),
                    user_installed: record.mods.get(filename).is_none_or(|r| r.user_installed),
                },
            );
        }
    }
    if report.identified > 0 {
        save_atomic(&dirs.content_record(instance_id, kind)?, &record)?;
    }

    let enabled: Vec<String> = hashes.iter().filter(|h| by_hash[*h].1 && current.contains_key(*h)).cloned().collect();
    if enabled.is_empty() {
        return Ok(report);
    }
    let latest = client.latest_by_hashes(&enabled, t).await?;
    for hash in &enabled {
        let (cur, Some(new)) = (&current[hash], latest.get(hash)) else { continue };
        let worse_channel = new.version_type != "release" && cur.version_type == "release";
        if new.id == cur.id || worse_channel {
            continue;
        }
        let Some(file) = new.files.iter().find(|f| f.primary).or_else(|| new.files.first()) else { continue };
        let filename = by_hash[hash].0.clone();
        let rec = &record.mods[&filename];
        report.updates.push(UpdateItem {
            title: rec.title.clone(),
            current_version: cur.version_number.clone(),
            new_version: new.version_number.clone(),
            file: PlanFile {
                project_id: new.project_id.clone(),
                version_id: new.id.clone(),
                title: rec.title.clone(),
                version_number: new.version_number.clone(),
                icon_url: rec.icon_url.clone(),
                filename: file.filename.clone(),
                url: file.url.clone(),
                sha1: file.sha1.clone(),
                size: file.size,
                required_by: None,
                user_selected: rec.user_installed,
                dest: kind,
            },
            filename,
        });
    }
    report.updates.sort_by_key(|u| u.title.to_lowercase());
    Ok(report)
}

/// Mods in the instance's `mods` folder (enabled and disabled).
pub fn list_installed(dirs: &Dirs, instance_id: &str) -> Result<Vec<InstalledMod>> {
    list_content(dirs, instance_id, ContentKind::Mod)
}

/// Project ids of installed mods, disabled ones included.
pub fn installed_project_ids(dirs: &Dirs, instance_id: &str) -> Result<HashSet<String>> {
    Ok(list_installed(dirs, instance_id)?.into_iter().filter_map(|m| m.project_id).collect())
}

/// Enable or disable a mod by renaming `x.jar` <-> `x.jar.disabled`.
pub fn set_enabled(dirs: &Dirs, instance_id: &str, filename: &str, enabled: bool) -> Result<()> {
    check_filename(filename, ContentKind::Mod)?;
    let dir = dirs.content_dir(instance_id, ContentKind::Mod)?;
    let (on, off) = (dir.join(filename), dir.join(disabled_name(filename)));
    let (from, to) = if enabled { (off, on) } else { (on, off) };
    if !from.is_file() {
        return Err(Error::Missing(from));
    }
    if to.exists() {
        return Err(Error::Other(format!("{} 已存在", to.display())));
    }
    std::fs::rename(from, to)?;
    Ok(())
}

/// Delete one file of `kind` (enabled or disabled) and forget its record.
pub fn remove_content(dirs: &Dirs, instance_id: &str, kind: ContentKind, filename: &str) -> Result<()> {
    check_filename(filename, kind)?;
    let dir = dirs.content_dir(instance_id, kind)?;
    let mut removed = false;
    for p in [dir.join(filename), dir.join(disabled_name(filename))] {
        if p.is_file() {
            std::fs::remove_file(&p)?;
            removed = true;
        }
    }
    if !removed {
        return Err(Error::Missing(dir.join(filename)));
    }
    let mut record = load_record(dirs, instance_id, kind)?;
    if record.mods.remove(filename).is_some() {
        save_atomic(&dirs.content_record(instance_id, kind)?, &record)?;
    }
    Ok(())
}

/// Delete one mod file (enabled or disabled) and forget its record.
pub fn remove_mod(dirs: &Dirs, instance_id: &str, filename: &str) -> Result<()> {
    remove_content(dirs, instance_id, ContentKind::Mod, filename)
}
