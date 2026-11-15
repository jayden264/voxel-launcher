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
