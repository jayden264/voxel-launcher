//! Project detail pages, searching across everything, and missing prerequisites.

use launcher_core::config::Dirs;
use launcher_core::download::sha1_bytes;
use launcher_core::loader::LoaderKind;
use launcher_core::mods::{scan_content, target_for, ContentKind, ModrinthClient};
use serde_json::json;
use std::collections::HashSet;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn project() -> serde_json::Value {
    json!({
        "id": "SOD", "slug": "sodium", "title": "Sodium", "description": "Fast renderer",
        "body": "# Sodium\nMakes it fast.", "icon_url": "https://cdn.modrinth.com/s.png",
        "gallery": [{ "url": "https://cdn.modrinth.com/g1.png", "title": "Shot", "featured": true }],
        "categories": ["optimization"], "downloads": 1000, "followers": 50,
        "license": { "id": "LGPL-3.0", "name": "GNU LGPL 3", "url": null },
        "source_url": "https://example.com/src", "issues_url": null, "wiki_url": null, "discord_url": null,
        "loaders": ["fabric", "quilt"], "game_versions": ["1.20.1", "1.21.1"]
    })
}

fn versions() -> serde_json::Value {
    json!([{
        "id": "v2", "project_id": "SOD", "version_number": "0.6", "name": "Sodium 0.6", "version_type": "release",
        "game_versions": ["1.21.1"], "loaders": ["fabric"], "date_published": "2024-05-01T00:00:00Z", "downloads": 10,
        "files": [], "dependencies": [
            { "project_id": "API", "dependency_type": "required" },
            { "project_id": "OPT", "dependency_type": "optional" },
            { "project_id": "EMB", "dependency_type": "embedded" }
        ]
    }, {
        "id": "v1", "project_id": "SOD", "version_number": "0.5", "version_type": "release",
        "game_versions": ["1.20.1"], "loaders": ["fabric"], "date_published": "2024-01-01T00:00:00Z", "downloads": 5,
        "files": [], "dependencies": []
    }])
}

async fn serve_project(server: &MockServer) {
    Mock::given(method("GET")).and(path("/project/SOD")).respond_with(ResponseTemplate::new(200).set_body_json(project())).mount(server).await;
    Mock::given(method("GET")).and(path("/project/SOD/version")).respond_with(ResponseTemplate::new(200).set_body_json(versions())).mount(server).await;
    Mock::given(method("GET"))
        .and(path("/projects"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "id": "API", "title": "Fabric API", "icon_url": null },
            { "id": "OPT", "title": "Optional Thing", "icon_url": null }
        ])))
        .mount(server)
        .await;
}

#[tokio::test]
async fn detail_lists_prerequisites_and_whether_they_are_installed() {
    let server = MockServer::start().await;
    serve_project(&server).await;
    let client = ModrinthClient::new(server.uri());
    let t = target_for("1.21.1", LoaderKind::Fabric);
    let installed: HashSet<String> = ["API".to_string()].into();

    let d = client.project_detail("SOD", Some(&t), &installed).await.unwrap();

    assert_eq!((d.title.as_str(), d.body.as_str()), ("Sodium", "# Sodium\nMakes it fast."));
    assert_eq!(d.license.as_deref(), Some("GNU LGPL 3"));
    assert_eq!(d.gallery.len(), 1);
    assert_eq!(d.versions.len(), 2);
    assert_eq!(d.versions[0].version_number, "0.6");
    // Required and optional, never embedded; each says whether it is already there.
    let deps: Vec<_> = d.dependencies.iter().map(|x| (x.project_id.as_str(), x.dependency_type.as_str(), x.installed)).collect();
    assert_eq!(deps, vec![("API", "required", true), ("OPT", "optional", false)]);
    assert_eq!(d.dependencies[0].title, "Fabric API");
    assert_eq!(d.compatible, Some(true));
}

#[tokio::test]
async fn detail_for_a_target_with_no_matching_version_says_incompatible() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/project/SOD")).respond_with(ResponseTemplate::new(200).set_body_json(project())).mount(&server).await;
    Mock::given(method("GET")).and(path("/project/SOD/version")).and(query_param("game_versions", "[\"1.8.9\"]")).respond_with(ResponseTemplate::new(200).set_body_json(json!([]))).mount(&server).await;
    let client = ModrinthClient::new(server.uri());
    let t = target_for("1.8.9", LoaderKind::Fabric);

    let d = client.project_detail("SOD", Some(&t), &HashSet::new()).await.unwrap();

    assert_eq!(d.compatible, Some(false));
    assert!(d.versions.is_empty() && d.dependencies.is_empty());
}

#[tokio::test]
async fn detail_without_a_target_shows_every_version() {
    let server = MockServer::start().await;
    serve_project(&server).await;
    let d = ModrinthClient::new(server.uri()).project_detail("SOD", None, &HashSet::new()).await.unwrap();
    assert_eq!(d.compatible, None);
    assert_eq!(d.versions.len(), 2);
}

#[tokio::test]
async fn search_any_asks_for_the_project_type_only() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .and(query_param("facets", "[[\"project_type:mod\"]]"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "hits": [], "total_hits": 0 })))
        .expect(1)
        .mount(&server)
        .await;
    ModrinthClient::new(server.uri()).search_any(ContentKind::Mod, "x", 0).await.unwrap();
}

#[tokio::test]
async fn scan_reports_required_prerequisites_that_are_not_installed() {
    let server = MockServer::start().await;
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    let mods = dirs.mods_dir("inst").unwrap();
    std::fs::create_dir_all(&mods).unwrap();
    let data = b"needs-api".to_vec();
    std::fs::write(mods.join("needy.jar"), &data).unwrap();
    let h = sha1_bytes(&data);
    let ver = json!({
        "id": "v1", "project_id": "NEEDY", "version_number": "1", "version_type": "release",
        "files": [{ "hashes": { "sha1": h }, "url": "https://cdn.modrinth.com/x.jar", "filename": "needy.jar", "primary": true, "size": 1 }],
        "dependencies": [{ "project_id": "API", "dependency_type": "required" }, { "project_id": "NEEDY", "dependency_type": "required" }, { "project_id": "OPT", "dependency_type": "optional" }]
    });
    Mock::given(method("POST")).and(path("/version_files")).respond_with(ResponseTemplate::new(200).set_body_json(json!({ h.clone(): ver.clone() }))).mount(&server).await;
    Mock::given(method("POST")).and(path("/version_files/update")).respond_with(ResponseTemplate::new(200).set_body_json(json!({ h.clone(): ver }))).mount(&server).await;
    Mock::given(method("GET"))
        .and(path("/projects"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "id": "NEEDY", "title": "Needy" }, { "id": "API", "title": "Fabric API" }
        ])))
        .mount(&server)
        .await;

    let report = scan_content(&ModrinthClient::new(server.uri()), &dirs, "inst", ContentKind::Mod, &target_for("1.21.1", LoaderKind::Fabric)).await.unwrap();

    assert_eq!(report.missing.len(), 1, "{:?}", report.missing);
    let m = &report.missing[0];
    assert_eq!((m.project_id.as_str(), m.title.as_str()), ("API", "Fabric API"));
    assert_eq!(m.needed_by, vec!["Needy".to_string()]);
}
