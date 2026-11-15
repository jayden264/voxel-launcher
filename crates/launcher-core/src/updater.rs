//! "Is there a newer version?" — asks the GitHub Releases API about the
//! launcher's own repository. It only reports; nothing is downloaded or run.

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const GITHUB_API: &str = "https://api.github.com";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateInfo {
    /// Without a leading `v`.
    pub version: String,
    /// The release notes (Markdown).
    pub notes: String,
    /// The release page on GitHub.
    pub url: String,
    /// The Windows installer attached to the release, when there is one.
    pub installer_url: Option<String>,
}

/// `(owner, repo)` of a GitHub address. Placeholders such as `OWNER/REPO` are not repositories.
pub fn parse_repo(url: &str) -> Option<(String, String)> {
    let rest = url.trim().strip_prefix("https://github.com/")?;
    let mut parts = rest.trim_end_matches('/').split('/');
    let (owner, repo) = (parts.next()?, parts.next()?.trim_end_matches(".git"));
    let plain = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if parts.next().is_some() || !plain(owner) || !plain(repo) || (owner == "OWNER" && repo == "REPO") {
        return None;
    }
    Some((owner.to_string(), repo.to_string()))
}

fn numbers(v: &str) -> Option<Vec<u64>> {
    let core = v.trim().trim_start_matches(['v', 'V']).split(['-', '+']).next()?;
    let parts: Option<Vec<u64>> = core.split('.').map(|p| p.parse().ok()).collect();
    parts.filter(|p| !p.is_empty())
}

/// Is `latest` a higher version than `current`? Compared number by number; anything unreadable is "no".
pub fn is_newer(current: &str, latest: &str) -> bool {
    let (Some(a), Some(b)) = (numbers(current), numbers(latest)) else { return false };
    let n = a.len().max(b.len());
    let get = |v: &[u64], i: usize| v.get(i).copied().unwrap_or(0);
    (0..n).map(|i| (get(&a, i), get(&b, i))).find(|(x, y)| x != y).is_some_and(|(x, y)| y > x)
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

fn on_github(url: &str) -> bool {
    url.starts_with("https://github.com/")
}

/// Look at the latest release of `repo` on the API at `api_base`. `Ok(None)` when there is no
/// newer one, no release yet, or the latest is a draft or pre-release.
pub async fn check(api_base: &str, repo: (&str, &str), current: &str, timeout: Duration) -> Result<Option<UpdateInfo>> {
    let client = reqwest::Client::builder().user_agent("voxel-launcher").timeout(timeout).connect_timeout(timeout).build()?;
    let url = format!("{}/repos/{}/{}/releases/latest", api_base.trim_end_matches('/'), repo.0, repo.1);
    let resp = client.get(url).header("Accept", "application/vnd.github+json").send().await?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !resp.status().is_success() {
        return Err(Error::Other(format!("检查更新失败: HTTP {}", resp.status())));
    }
    let rel: Release = resp.json().await.map_err(|_| Error::Other("检查更新失败: 返回的内容无法识别".into()))?;
    let version = rel.tag_name.trim().trim_start_matches(['v', 'V']).to_string();
    if rel.draft || rel.prerelease || !is_newer(current, &version) {
        return Ok(None);
    }
    let page = format!("https://github.com/{}/{}/releases/tag/{}", repo.0, repo.1, rel.tag_name);
    Ok(Some(UpdateInfo {
        version,
        notes: rel.body.unwrap_or_default().chars().take(4000).collect(),
        url: if on_github(&rel.html_url) { rel.html_url } else { page },
        installer_url: rel
            .assets
            .into_iter()
            .find(|a| a.name.to_ascii_lowercase().ends_with(".exe") && on_github(&a.browser_download_url))
            .map(|a| a.browser_download_url),
    }))
}
