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
    /// Its SHA-256 as GitHub reports it (lowercase hex); needed to install it automatically.
    pub installer_sha256: Option<String>,
    pub installer_size: Option<u64>,
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
    #[serde(default)]
    size: u64,
    /// `sha256:<hex>`
    #[serde(default)]
    digest: Option<String>,
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
    let installer = rel.assets.into_iter().find(|a| a.name.to_ascii_lowercase().ends_with(".exe") && on_github(&a.browser_download_url));
    Ok(Some(UpdateInfo {
        version,
        notes: rel.body.unwrap_or_default().chars().take(4000).collect(),
        url: if on_github(&rel.html_url) { rel.html_url } else { page },
        installer_sha256: installer.as_ref().and_then(|a| a.digest.as_deref()).and_then(|d| d.strip_prefix("sha256:")).map(|h| h.to_ascii_lowercase()),
        installer_size: installer.as_ref().map(|a| a.size).filter(|s| *s > 0),
        installer_url: installer.map(|a| a.browser_download_url),
    }))
}

// ------------------------------------------------------ download and install

/// Where an installer may come from, and what must be known about it first.
#[derive(Debug, Clone, Copy)]
pub struct DownloadPolicy<'a> {
    /// Host names allowed for the address and for every redirect (subdomains included).
    pub hosts: &'a [&'a str],
    pub https_only: bool,
    /// Refuse to download at all without a checksum to verify it against.
    pub require_digest: bool,
}

impl DownloadPolicy<'static> {
    /// GitHub itself and the hosts it serves release files from, over https, checksum required.
    pub fn github() -> Self {
        Self { hosts: &["github.com", "githubusercontent.com"], https_only: true, require_digest: true }
    }
}

fn host_allowed(url: &reqwest::Url, hosts: &[&str], https_only: bool) -> bool {
    let scheme_ok = if https_only { url.scheme() == "https" } else { matches!(url.scheme(), "http" | "https") };
    scheme_ok && url.host_str().is_some_and(|h| hosts.iter().any(|a| h == *a || h.ends_with(&format!(".{a}"))))
}

/// Upper bound for an installer: far above a real one, far below "fill the disk".
const MAX_INSTALLER_BYTES: u64 = 300 * 1024 * 1024;

/// Download `url` to `dest`, check its SHA-256 (and size, when known) and only then keep it.
/// A file that fails any check is deleted, so nothing unverified is ever left to run.
pub async fn download_installer(
    url: &str,
    dest: &std::path::Path,
    sha256: Option<&str>,
    size: Option<u64>,
    policy: &DownloadPolicy<'_>,
    progress: &(dyn Fn(u64, u64) + Sync),
) -> Result<()> {
    use sha2::{Digest, Sha256};
    let parsed = reqwest::Url::parse(url).map_err(|_| Error::Other("更新包的下载地址无效".into()))?;
    if !host_allowed(&parsed, policy.hosts, policy.https_only) {
        return Err(Error::Other("更新包的下载地址不在允许的范围内，已取消".into()));
    }
    let expected = match sha256.map(str::to_ascii_lowercase) {
        Some(h) if h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()) => Some(h),
        None if !policy.require_digest => None,
        _ => return Err(Error::Other("没有可用的校验值，无法确认更新包没被篡改，已取消自动更新".into())),
    };

    let hosts: Vec<String> = policy.hosts.iter().map(|h| h.to_string()).collect();
    let https_only = policy.https_only;
    let redirects = reqwest::redirect::Policy::custom(move |attempt| {
        let refs: Vec<&str> = hosts.iter().map(String::as_str).collect();
        if attempt.previous().len() >= 5 {
            attempt.error("跳转次数过多")
        } else if host_allowed(attempt.url(), &refs, https_only) {
            attempt.follow()
        } else {
            attempt.error("下载被重定向到了不允许的地址")
        }
    });
    let client = reqwest::Client::builder().user_agent("voxel-launcher").redirect(redirects).connect_timeout(Duration::from_secs(15)).build()?;
    let mut resp = client.get(parsed).send().await.map_err(|e| Error::Other(format!("下载更新失败: {e}")))?;
    if !resp.status().is_success() {
        return Err(Error::Other(format!("下载更新失败: HTTP {}", resp.status())));
    }
    let total = size.or(resp.content_length()).unwrap_or(0);
    if total > MAX_INSTALLER_BYTES {
        return Err(Error::Other("更新包大得不正常，已取消".into()));
    }

    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut part_name = dest.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    part_name.push(".part");
    let part = dest.with_file_name(part_name);
    let result = async {
        let mut file = std::fs::File::create(&part)?;
        let mut hasher = Sha256::new();
        let mut done = 0u64;
        while let Some(chunk) = resp.chunk().await.map_err(|e| Error::Other(format!("下载更新失败: {e}")))? {
            done += chunk.len() as u64;
            if done > MAX_INSTALLER_BYTES {
                return Err(Error::Other("更新包大得不正常，已取消".into()));
            }
            std::io::Write::write_all(&mut file, &chunk)?;
            hasher.update(&chunk);
            progress(done, total.max(done));
        }
        drop(file);
        if size.is_some_and(|s| s != done) {
            return Err(Error::Other(format!("更新包大小不对（应为 {} 字节，实际 {done} 字节），已丢弃", size.unwrap_or(0))));
        }
        if let Some(want) = &expected {
            if hex::encode(hasher.finalize()) != *want {
                return Err(Error::Other("更新包校验失败，内容和发布的不一致，已丢弃".into()));
            }
        }
        Ok(())
    }
    .await;
    match result {
        Ok(()) => {
            std::fs::rename(&part, dest)?;
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            Err(e)
        }
    }
}

/// Arguments for the downloaded NSIS installer: a small progress window without questions
/// (`/P`), update mode (`/UPDATE`: keep the folder, no reinstall page) and reopen the launcher
/// afterwards (`/R`).
pub fn installer_args() -> [&'static str; 3] {
    ["/P", "/UPDATE", "/R"]
}

/// Start the downloaded installer so that it outlives this process. The caller must then exit,
/// so the installer can replace the running program.
pub fn launch_installer(path: &std::path::Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        std::process::Command::new(path).args(installer_args()).creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP).spawn()?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err(Error::Other("自动更新目前只支持 Windows".into()))
    }
}
