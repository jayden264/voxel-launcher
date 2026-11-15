//! Looking for a newer release on GitHub.

use launcher_core::updater::{check, is_newer, parse_repo};
use serde_json::json;
use std::time::Duration;
use wiremock::matchers::{header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn release(tag: &str) -> serde_json::Value {
    json!({
        "tag_name": tag, "name": format!("Voxel {tag}"), "draft": false, "prerelease": false,
        "html_url": format!("https://github.com/acme/voxel/releases/tag/{tag}"),
        "body": "## 更新\n- 修复了一些问题",
        "assets": [
            { "name": "voxel-source.zip", "browser_download_url": "https://github.com/acme/voxel/releases/download/x/source.zip" },
            { "name": "Voxel_0.2.0_x64-setup.exe", "browser_download_url": "https://github.com/acme/voxel/releases/download/x/Voxel_0.2.0_x64-setup.exe" }
        ]
    })
}

async fn serve(server: &MockServer, body: ResponseTemplate) {
    Mock::given(method("GET")).and(path("/repos/acme/voxel/releases/latest")).and(header_exists("user-agent")).respond_with(body).mount(server).await;
}

fn t() -> Duration {
    Duration::from_secs(3)
}

#[test]
fn versions_compare_by_their_numbers_not_their_text() {
    for (current, latest, newer) in [
        ("0.1.0", "0.2.0", true),
        ("0.1.0", "v0.1.1", true),
        ("0.9.0", "0.10.0", true),
        ("0.1.0", "0.1.0", false),
        ("0.2.0", "0.1.9", false),
        ("1.0", "1.0.1", true),
        ("0.1.0", "1.0.0", true),
        ("0.1.0", "garbage", false),
        ("0.1.0", "", false),
    ] {
        assert_eq!(is_newer(current, latest), newer, "{current} vs {latest}");
    }
}

#[test]
fn the_repository_comes_from_a_github_address_and_placeholders_are_not_repositories() {
    assert_eq!(parse_repo("https://github.com/acme/voxel"), Some(("acme".into(), "voxel".into())));
    assert_eq!(parse_repo("https://github.com/acme/voxel.git"), Some(("acme".into(), "voxel".into())));
    assert_eq!(parse_repo("https://github.com/acme/voxel/"), Some(("acme".into(), "voxel".into())));
    for bad in ["", "https://example.com/acme/voxel", "https://github.com/acme", "https://github.com/OWNER/REPO", "https://github.com/a b/c"] {
        assert_eq!(parse_repo(bad), None, "{bad:?}");
    }
}

#[tokio::test]
async fn a_newer_release_is_reported_with_notes_and_the_installer() {
    let server = MockServer::start().await;
    serve(&server, ResponseTemplate::new(200).set_body_json(release("v0.2.0"))).await;

    let u = check(&server.uri(), ("acme", "voxel"), "0.1.0", t()).await.unwrap().expect("an update");

    assert_eq!(u.version, "0.2.0");
    assert!(u.notes.contains("修复了一些问题"));
    assert_eq!(u.url, "https://github.com/acme/voxel/releases/tag/v0.2.0");
    assert!(u.installer_url.as_deref().is_some_and(|x| x.ends_with("x64-setup.exe")), "the installer, not the source zip");
}

#[tokio::test]
async fn the_same_or_an_older_release_is_not_an_update() {
    let server = MockServer::start().await;
    serve(&server, ResponseTemplate::new(200).set_body_json(release("v0.1.0"))).await;
    assert!(check(&server.uri(), ("acme", "voxel"), "0.1.0", t()).await.unwrap().is_none());
    assert!(check(&server.uri(), ("acme", "voxel"), "0.3.0", t()).await.unwrap().is_none());
}

#[tokio::test]
async fn no_releases_yet_and_drafts_and_prereleases_are_quietly_ignored() {
    let server = MockServer::start().await;
    serve(&server, ResponseTemplate::new(404)).await;
    assert!(check(&server.uri(), ("acme", "voxel"), "0.1.0", t()).await.unwrap().is_none());

    for (draft, pre) in [(true, false), (false, true)] {
        let server = MockServer::start().await;
        let mut r = release("v9.0.0");
        r["draft"] = json!(draft);
        r["prerelease"] = json!(pre);
        serve(&server, ResponseTemplate::new(200).set_body_json(r)).await;
        assert!(check(&server.uri(), ("acme", "voxel"), "0.1.0", t()).await.unwrap().is_none());
    }
}

#[tokio::test]
async fn links_must_stay_on_github_and_bad_replies_are_errors() {
    let server = MockServer::start().await;
    let mut r = release("v0.2.0");
    r["html_url"] = json!("https://evil.example.com/phish");
    r["assets"][1]["browser_download_url"] = json!("https://evil.example.com/setup.exe");
    serve(&server, ResponseTemplate::new(200).set_body_json(r)).await;
    let u = check(&server.uri(), ("acme", "voxel"), "0.1.0", t()).await.unwrap().unwrap();
    assert_eq!(u.url, "https://github.com/acme/voxel/releases/tag/v0.2.0", "falls back to the repository's own release page");
    assert_eq!(u.installer_url, None);

    let server = MockServer::start().await;
    serve(&server, ResponseTemplate::new(200).set_body_string("<html>not json</html>")).await;
    assert!(check(&server.uri(), ("acme", "voxel"), "0.1.0", t()).await.is_err());
    let server = MockServer::start().await;
    serve(&server, ResponseTemplate::new(500)).await;
    assert!(check(&server.uri(), ("acme", "voxel"), "0.1.0", t()).await.is_err());
}

// ------------------------------------------------------- download + install

use launcher_core::updater::{download_installer, installer_args, DownloadPolicy};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};

fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn local_policy() -> DownloadPolicy<'static> {
    DownloadPolicy { hosts: &["127.0.0.1"], https_only: false, require_digest: true }
}

async fn serve_file(server: &MockServer, route: &str, body: &[u8]) {
    Mock::given(method("GET")).and(path(route)).respond_with(ResponseTemplate::new(200).set_body_bytes(body.to_vec())).mount(server).await;
}

#[tokio::test]
async fn the_installer_is_downloaded_checked_and_saved_with_progress() {
    let server = MockServer::start().await;
    let data = vec![7u8; 300_000];
    serve_file(&server, "/setup.exe", &data).await;
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("setup.exe");
    let last = AtomicU64::new(0);
    let total_seen = AtomicU64::new(0);

    download_installer(&format!("{}/setup.exe", server.uri()), &dest, Some(&sha256(&data)), Some(data.len() as u64), &local_policy(), &|done, total| {
        last.store(done, Ordering::Relaxed);
        total_seen.store(total, Ordering::Relaxed);
    })
    .await
    .unwrap();

    assert_eq!(std::fs::read(&dest).unwrap(), data);
    assert_eq!((last.load(Ordering::Relaxed), total_seen.load(Ordering::Relaxed)), (300_000, 300_000));
    assert!(!tmp.path().join("setup.exe.part").exists());
}

#[tokio::test]
async fn a_download_that_does_not_match_its_checksum_is_thrown_away() {
    let server = MockServer::start().await;
    serve_file(&server, "/setup.exe", b"tampered installer").await;
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("setup.exe");

    let err = download_installer(&format!("{}/setup.exe", server.uri()), &dest, Some(&sha256(b"the real installer")), None, &local_policy(), &|_, _| {}).await.unwrap_err().to_string();

    assert!(err.contains("校验"), "{err}");
    assert!(!dest.exists() && !tmp.path().join("setup.exe.part").exists(), "nothing is left behind to be run");
}

#[tokio::test]
async fn a_wrong_size_or_a_missing_checksum_is_refused() {
    let server = MockServer::start().await;
    serve_file(&server, "/setup.exe", b"abc").await;
    let tmp = tempfile::tempdir().unwrap();
    let url = format!("{}/setup.exe", server.uri());

    assert!(download_installer(&url, &tmp.path().join("a.exe"), Some(&sha256(b"abc")), Some(999), &local_policy(), &|_, _| {}).await.is_err(), "size differs from what GitHub said");
    let before = server.received_requests().await.unwrap().len();
    assert!(download_installer(&url, &tmp.path().join("b.exe"), None, None, &local_policy(), &|_, _| {}).await.is_err(), "no checksum, no automatic install");
    assert_eq!(server.received_requests().await.unwrap().len(), before, "refused before anything was downloaded");
}

#[tokio::test]
async fn only_allowed_hosts_may_serve_the_installer_even_through_redirects() {
    let tmp = tempfile::tempdir().unwrap();
    let data = b"payload".to_vec();

    // Not an allowed host from the start: no request is made.
    let server = MockServer::start().await;
    serve_file(&server, "/setup.exe", &data).await;
    let strict = DownloadPolicy { hosts: &["github.com"], https_only: false, require_digest: true };
    assert!(download_installer(&format!("{}/setup.exe", server.uri()), &tmp.path().join("a.exe"), Some(&sha256(&data)), None, &strict, &|_, _| {}).await.is_err());
    assert!(server.received_requests().await.unwrap().is_empty());

    // An allowed host that redirects somewhere else is followed only while the hops stay allowed.
    let elsewhere = MockServer::start().await;
    serve_file(&elsewhere, "/setup.exe", &data).await;
    let elsewhere_url = format!("http://localhost:{}/setup.exe", elsewhere.address().port());
    let hop = MockServer::start().await;
    Mock::given(method("GET")).and(path("/go")).respond_with(ResponseTemplate::new(302).insert_header("location", elsewhere_url.as_str())).mount(&hop).await;
    let err = download_installer(&format!("{}/go", hop.uri()), &tmp.path().join("b.exe"), Some(&sha256(&data)), None, &local_policy(), &|_, _| {}).await;
    assert!(err.is_err(), "the redirect leaves the allowed hosts");
    assert!(elsewhere.received_requests().await.unwrap().is_empty());

    // The real policy: https and GitHub's own hosts only.
    let real = DownloadPolicy::github();
    assert!(download_installer("http://github.com/x.exe", &tmp.path().join("c.exe"), Some("00"), None, &real, &|_, _| {}).await.is_err(), "plain http is refused");
    assert!(download_installer("https://evil.example.com/x.exe", &tmp.path().join("d.exe"), Some("00"), None, &real, &|_, _| {}).await.is_err());
    assert!(download_installer("https://github.com.evil.example.com/x.exe", &tmp.path().join("e.exe"), Some("00"), None, &real, &|_, _| {}).await.is_err(), "a look-alike host is not github.com");
}

#[test]
fn the_installer_runs_with_a_progress_window_in_update_mode_and_reopens_the_launcher() {
    assert_eq!(installer_args(), ["/P", "/UPDATE", "/R"]);
}

#[tokio::test]
async fn release_assets_carry_their_checksum_and_size() {
    let server = MockServer::start().await;
    let mut r = release("v0.3.0");
    r["assets"][1]["digest"] = json!("sha256:ABCDEF0123");
    r["assets"][1]["size"] = json!(4565507);
    serve(&server, ResponseTemplate::new(200).set_body_json(r)).await;

    let u = check(&server.uri(), ("acme", "voxel"), "0.1.0", Duration::from_secs(3)).await.unwrap().unwrap();

    assert_eq!((u.installer_sha256.as_deref(), u.installer_size), (Some("abcdef0123"), Some(4565507)));
}
