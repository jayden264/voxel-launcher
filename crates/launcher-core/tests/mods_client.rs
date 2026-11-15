//! Modrinth client behaviour against a local mock of api.modrinth.com.

use launcher_core::loader::LoaderKind;
use launcher_core::mods::{target_for, ModrinthClient};
use std::time::{Duration, Instant};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SEARCH_BODY: &str = r#"{"hits":[
  {"project_id":"AANobbMI","project_type":"mod","slug":"sodium","author":"jellysquid3","title":"Sodium",
   "description":"A high-performance rendering engine","icon_url":"https://cdn.modrinth.com/data/AANobbMI/icon.png",
   "downloads":252637937,"categories":["fabric"]},
  {"project_id":"P7dR8mSH","project_type":"mod","slug":"fabric-api","author":"modmuss50","title":"Fabric API",
   "description":"Core API","icon_url":null,"downloads":1000}
 ],"offset":0,"limit":20,"total_hits":2}"#;

const VERSIONS_BODY: &str = r#"[
 {"id":"SMxNOGZ6","project_id":"AANobbMI","version_number":"mc1.21.1-0.8.13-fabric","version_type":"release",
  "loaders":["fabric"],"game_versions":["1.21.1"],
  "files":[
    {"hashes":{"sha512":"aa","sha1":"003c114c85ca88ef3362e018deb6aca0c682d6a1"},
     "url":"https://cdn.modrinth.com/data/AANobbMI/versions/SMxNOGZ6/sodium-fabric-0.8.13%2Bmc1.21.1.jar",
     "filename":"sodium-fabric-0.8.13+mc1.21.1.jar","primary":true,"size":1574609},
    {"hashes":{"sha512":"bb","sha1":"1111111111111111111111111111111111111111"},
     "url":"https://cdn.modrinth.com/x-sources.jar","filename":"sodium-sources.jar","primary":false,"size":10}],
  "dependencies":[
    {"version_id":null,"project_id":"P7dR8mSH","file_name":null,"dependency_type":"required"},
    {"version_id":"abc","project_id":null,"file_name":null,"dependency_type":"incompatible"}]}
]"#;

fn client(server: &MockServer) -> ModrinthClient {
    ModrinthClient::new(server.uri())
}

#[tokio::test]
async fn search_sends_facets_and_user_agent() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .and(header("user-agent", "voxel-launcher/0.1"))
        .and(query_param(
            "facets",
            r#"[["project_type:mod"],["versions:1.21.1"],["categories:fabric"]]"#,
        ))
        .and(query_param("query", "sodium"))
        .and(query_param("limit", "20"))
        .and(query_param("offset", "40"))
        .respond_with(ResponseTemplate::new(200).set_body_string(SEARCH_BODY))
        .expect(1)
        .mount(&server)
        .await;

    let t = target_for("1.21.1", LoaderKind::Fabric);
    let r = client(&server).search(&t, "sodium", 40).await.unwrap();
    assert_eq!(r.total, 2);
    assert_eq!(r.hits[0].title, "Sodium");
    assert_eq!(r.hits[0].project_id, "AANobbMI");
    assert_eq!(r.hits[0].downloads, 252637937);
    assert_eq!(r.hits[0].author, "jellysquid3");
    assert!(r.hits[1].icon_url.is_none());
    server.verify().await;
}

#[tokio::test]
async fn quilt_target_searches_both_loaders_in_one_facet_group() {
    let t = target_for("1.21.1", LoaderKind::Quilt);
    assert_eq!(t.loaders, vec!["quilt", "fabric"]);
    assert_eq!(target_for("1.21.1", LoaderKind::Fabric).loaders, vec!["fabric"]);

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .and(query_param(
            "facets",
            r#"[["project_type:mod"],["versions:1.21.1"],["categories:quilt","categories:fabric"]]"#,
        ))
        .and(query_param("query", ""))
        .respond_with(ResponseTemplate::new(200).set_body_string(SEARCH_BODY))
        .expect(1)
        .mount(&server)
        .await;
    client(&server).search(&t, "", 0).await.unwrap();
    server.verify().await;
}

#[tokio::test]
async fn project_versions_parse_files_and_dependencies() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/project/sodium/version"))
        .and(query_param("game_versions", r#"["1.21.1"]"#))
        .and(query_param("loaders", r#"["fabric"]"#))
        .respond_with(ResponseTemplate::new(200).set_body_string(VERSIONS_BODY))
        .expect(1)
        .mount(&server)
        .await;
    let t = target_for("1.21.1", LoaderKind::Fabric);
    let v = client(&server).project_versions("sodium", &t).await.unwrap();
    assert_eq!(v.len(), 1);
    let v = &v[0];
    assert_eq!((v.id.as_str(), v.project_id.as_str()), ("SMxNOGZ6", "AANobbMI"));
    assert_eq!(v.version_type, "release");
    assert_eq!(v.files.len(), 2);
    assert!(v.files[0].primary && !v.files[1].primary);
    assert_eq!(v.files[0].sha1, "003c114c85ca88ef3362e018deb6aca0c682d6a1");
    assert_eq!(v.files[0].size, 1574609);
    assert_eq!(v.files[0].filename, "sodium-fabric-0.8.13+mc1.21.1.jar");
    assert_eq!(v.dependencies.len(), 2);
    assert_eq!(v.dependencies[0].dependency_type, "required");
    assert_eq!(v.dependencies[0].project_id.as_deref(), Some("P7dR8mSH"));
    assert_eq!(v.dependencies[1].version_id.as_deref(), Some("abc"));
    assert!(v.dependencies[1].project_id.is_none());
    server.verify().await;
}

#[tokio::test]
async fn version_by_id_and_project_title() {
    let server = MockServer::start().await;
    let one = VERSIONS_BODY.trim()[1..VERSIONS_BODY.trim().len() - 1].to_string();
    Mock::given(method("GET"))
        .and(path("/version/SMxNOGZ6"))
        .respond_with(ResponseTemplate::new(200).set_body_string(one))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/project/AANobbMI"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(r#"{"id":"AANobbMI","slug":"sodium","title":"Sodium","icon_url":"https://cdn.modrinth.com/i.png"}"#),
        )
        .mount(&server)
        .await;
    let c = client(&server);
    assert_eq!(c.version_by_id("SMxNOGZ6").await.unwrap().project_id, "AANobbMI");
    let (title, icon) = c.project_title("AANobbMI").await.unwrap();
    assert_eq!(title, "Sodium");
    assert_eq!(icon.as_deref(), Some("https://cdn.modrinth.com/i.png"));
}

#[tokio::test]
async fn rejects_project_ids_that_could_escape_the_url_path() {
    let server = MockServer::start().await;
    let t = target_for("1.21.1", LoaderKind::Fabric);
    let c = client(&server);
    assert!(c.project_versions("../users/x", &t).await.is_err());
    assert!(c.project_versions("a/b", &t).await.is_err());
    assert!(c.version_by_id("x?y=1").await.is_err());
    assert!(c.project_title("").await.is_err());
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn retries_after_429_using_retry_after() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/project/sodium/version"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "1"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/project/sodium/version"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .mount(&server)
        .await;
    let t = target_for("1.21.1", LoaderKind::Fabric);
    let start = Instant::now();
    let v = client(&server).project_versions("sodium", &t).await.unwrap();
    assert!(v.is_empty());
    assert!(start.elapsed() >= Duration::from_millis(950), "{:?}", start.elapsed());
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn gives_up_after_three_429s() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
        .mount(&server)
        .await;
    let t = target_for("1.21.1", LoaderKind::Fabric);
    let err = client(&server).project_versions("sodium", &t).await.unwrap_err();
    assert!(err.to_string().contains("429"), "{err}");
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
}

#[tokio::test]
async fn other_http_errors_name_the_status() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).respond_with(ResponseTemplate::new(404)).mount(&server).await;
    let t = target_for("1.21.1", LoaderKind::Fabric);
    let err = client(&server).project_versions("nope", &t).await.unwrap_err();
    assert!(err.to_string().contains("404"), "{err}");
    assert_eq!(server.received_requests().await.unwrap().len(), 1, "4xx must not be retried");
}

/// Hits the real API; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn real_modrinth_search_and_versions() {
    let c = ModrinthClient::official();
    let t = target_for("1.21.1", LoaderKind::Fabric);
    let r = c.search(&t, "sodium", 0).await.unwrap();
    assert!(r.hits.iter().any(|h| h.slug == "sodium"));
    let v = c.project_versions("fabric-api", &t).await.unwrap();
    assert!(!v.is_empty() && v[0].files.iter().any(|f| f.primary && !f.sha1.is_empty()));
}

#[test]
fn forge_and_neoforge_targets_use_their_own_loader_name() {
    assert_eq!(target_for("1.21.1", LoaderKind::Forge).loaders, vec!["forge"]);
    assert_eq!(target_for("1.21.1", LoaderKind::NeoForge).loaders, vec!["neoforge"]);
}
