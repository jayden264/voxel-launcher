//! Loader meta handling against a local mock of the Fabric/Quilt meta services.

use launcher_core::config::Dirs;
use launcher_core::download::Downloader;
use launcher_core::loader::{self, LoaderKind};
use launcher_core::mirror::Source;
use std::sync::Arc;
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const FABRIC_PROFILE: &str = include_str!("fixtures/fabric-loader-0.16.9-1.21.1.json");

fn dl(server: &MockServer) -> Downloader {
    let base = server.uri();
    Downloader::new(Source::Official)
        .with_retry(0, Duration::from_millis(1))
        .with_resolver(Arc::new(move |u| {
            vec![u.replace("https://meta.fabricmc.net", &base).replace("https://meta.quiltmc.org", &base)]
        }))
}

fn list_json(versions: &[(&str, Option<bool>)]) -> String {
    let items: Vec<String> = versions
        .iter()
        .map(|(v, stable)| match stable {
            Some(s) => format!(r#"{{"loader":{{"version":"{v}","build":1,"stable":{s}}}}}"#),
            None => format!(r#"{{"loader":{{"version":"{v}","build":1}}}}"#),
        })
        .collect();
    format!("[{}]", items.join(","))
}

#[tokio::test]
async fn lists_versions_newest_first_with_stable_inferred_from_name() {
    let server = MockServer::start().await;
    // Quilt returns them in arbitrary order and has no `stable` flag.
    Mock::given(method("GET"))
        .and(path("/v3/versions/loader/1.21.1"))
        .respond_with(ResponseTemplate::new(200).set_body_string(list_json(&[
            ("0.20.0-beta.9", None),
            ("0.24.0", None),
            ("0.20.0-beta.1", None),
            ("0.27.1", None),
            ("0.20.0-beta.10", None),
        ])))
        .mount(&server)
        .await;
    let list = loader::list_loader_versions(&dl(&server), LoaderKind::Quilt, "1.21.1").await.unwrap();
    let names: Vec<_> = list.iter().map(|v| v.version.as_str()).collect();
    assert_eq!(names, vec!["0.27.1", "0.24.0", "0.20.0-beta.10", "0.20.0-beta.9", "0.20.0-beta.1"]);
    let stable: Vec<_> = list.iter().map(|v| v.stable).collect();
    assert_eq!(stable, vec![true, true, false, false, false]);
    assert_eq!(loader::default_version(&list).unwrap().version, "0.27.1");
}

#[tokio::test]
async fn fabric_stable_flag_is_ignored_because_only_the_newest_is_flagged() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/versions/loader/1.21.1"))
        .respond_with(ResponseTemplate::new(200).set_body_string(list_json(&[
            ("0.19.5", Some(false)),
            ("0.20.1", Some(true)),
            ("0.1.0.49", Some(false)),
        ])))
        .mount(&server)
        .await;
    let list = loader::list_loader_versions(&dl(&server), LoaderKind::Fabric, "1.21.1").await.unwrap();
    let names: Vec<_> = list.iter().map(|v| v.version.as_str()).collect();
    assert_eq!(names, vec!["0.20.1", "0.19.5", "0.1.0.49"]);
    assert!(list.iter().all(|v| v.stable));
}

#[tokio::test]
async fn default_version_falls_back_to_newest_when_nothing_is_stable() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v3/versions/loader/1.14.4"))
        .respond_with(ResponseTemplate::new(200).set_body_string(list_json(&[("0.9.0-beta.1", None), ("0.9.0-beta.2", None)])))
        .mount(&server)
        .await;
    let list = loader::list_loader_versions(&dl(&server), LoaderKind::Quilt, "1.14.4").await.unwrap();
    assert_eq!(loader::default_version(&list).unwrap().version, "0.9.0-beta.2");
    assert!(loader::default_version(&[]).is_none());
}

#[tokio::test]
async fn empty_list_means_unsupported_and_install_writes_nothing() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/versions/loader/1.12.2"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v2/versions/loader/1.12.2/0.16.9/profile/json"))
        .respond_with(ResponseTemplate::new(400).set_body_string("no such version"))
        .mount(&server)
        .await;
    let d = dl(&server);
    assert!(loader::list_loader_versions(&d, LoaderKind::Fabric, "1.12.2").await.unwrap().is_empty());

    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    let err = loader::fetch_profile(&d, &dirs, LoaderKind::Fabric, "1.12.2", "0.16.9").await.unwrap_err();
    assert!(err.to_string().contains("不支持"), "{err}");
    let entries = std::fs::read_dir(dirs.versions()).map(|r| r.count()).unwrap_or(0);
    assert_eq!(entries, 0);
}

#[tokio::test]
async fn http_400_from_meta_means_unsupported_not_an_error() {
    // The live Fabric service answers an unsupported Minecraft version with
    // `400 Bad Request` and a body of `[]`.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/versions/loader/1.12.2"))
        .respond_with(ResponseTemplate::new(400).set_body_string("[]"))
        .mount(&server)
        .await;
    let list = loader::list_loader_versions(&dl(&server), LoaderKind::Fabric, "1.12.2").await.unwrap();
    assert!(list.is_empty());
}

#[tokio::test]
async fn server_errors_from_meta_are_still_errors() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/versions/loader/1.21.1"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    assert!(loader::list_loader_versions(&dl(&server), LoaderKind::Fabric, "1.21.1").await.is_err());
}

#[tokio::test]
async fn fetch_profile_writes_json_and_is_idempotent() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/versions/loader/1.21.1/0.16.9/profile/json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(FABRIC_PROFILE))
        .expect(1)
        .mount(&server)
        .await;
    let d = dl(&server);
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());

    let id = loader::fetch_profile(&d, &dirs, LoaderKind::Fabric, "1.21.1", "0.16.9").await.unwrap();
    assert_eq!(id, "fabric-loader-0.16.9-1.21.1");
    let file = dirs.versions().join(&id).join(format!("{id}.json"));
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(v["id"], "fabric-loader-0.16.9-1.21.1");
    assert!(!dirs.versions().join(&id).join(format!("{id}.json.tmp")).exists());

    // Second call: file already present, no further request (mock expects exactly 1).
    assert_eq!(loader::fetch_profile(&d, &dirs, LoaderKind::Fabric, "1.21.1", "0.16.9").await.unwrap(), id);
    server.verify().await;
}

#[tokio::test]
async fn rejects_profile_with_wrong_inherits_from() {
    let server = MockServer::start().await;
    let wrong = FABRIC_PROFILE.replace(r#""inheritsFrom":"1.21.1""#, r#""inheritsFrom":"1.20.4""#);
    assert_ne!(wrong, FABRIC_PROFILE, "fixture shape changed");
    Mock::given(method("GET"))
        .and(path("/v2/versions/loader/1.21.1/0.16.9/profile/json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(wrong))
        .mount(&server)
        .await;
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    assert!(loader::fetch_profile(&dl(&server), &dirs, LoaderKind::Fabric, "1.21.1", "0.16.9").await.is_err());
    assert_eq!(std::fs::read_dir(dirs.versions()).map(|r| r.count()).unwrap_or(0), 0);
}

#[tokio::test]
async fn rejects_profile_with_unexpected_or_unsafe_id() {
    let server = MockServer::start().await;
    let evil = FABRIC_PROFILE.replace(r#""id":"fabric-loader-0.16.9-1.21.1""#, r#""id":"..\\..\\evil""#);
    assert_ne!(evil, FABRIC_PROFILE, "fixture shape changed");
    Mock::given(method("GET"))
        .and(path("/v2/versions/loader/1.21.1/0.16.9/profile/json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(evil))
        .mount(&server)
        .await;
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    assert!(loader::fetch_profile(&dl(&server), &dirs, LoaderKind::Fabric, "1.21.1", "0.16.9").await.is_err());
    assert!(!tmp.path().join("evil").exists());
}

#[tokio::test]
async fn rejects_unsafe_version_arguments_before_any_request() {
    let server = MockServer::start().await;
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    let r = loader::fetch_profile(&dl(&server), &dirs, LoaderKind::Fabric, "1.21.1", "../../x").await;
    assert!(r.is_err());
    assert!(server.received_requests().await.unwrap().is_empty());
}

/// Hits the real meta services; run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore]
async fn real_fabric_versions_for_1_21_1() {
    let d = Downloader::new(Source::Official);
    let list = loader::list_loader_versions(&d, LoaderKind::Fabric, "1.21.1").await.unwrap();
    assert!(!list.is_empty());
    assert!(loader::default_version(&list).is_some());
    assert!(loader::list_loader_versions(&d, LoaderKind::Fabric, "1.12.2").await.unwrap().is_empty());
}

#[test]
fn profile_ids_for_all_kinds() {
    assert_eq!(loader::profile_id(LoaderKind::Fabric, "0.16.9", "1.21.1"), "fabric-loader-0.16.9-1.21.1");
    assert_eq!(loader::profile_id(LoaderKind::Quilt, "0.27.1", "1.21.1"), "quilt-loader-0.27.1-1.21.1");
    assert_eq!(loader::profile_id(LoaderKind::Forge, "52.1.0", "1.21.1"), "1.21.1-forge-52.1.0");
    assert_eq!(loader::profile_id(LoaderKind::NeoForge, "21.1.172", "1.21.1"), "neoforge-21.1.172");
}

#[test]
fn loader_kind_serde_is_lowercase_and_labelled() {
    assert_eq!(serde_json::to_string(&LoaderKind::Forge).unwrap(), r#""forge""#);
    assert_eq!(serde_json::to_string(&LoaderKind::NeoForge).unwrap(), r#""neoforge""#);
    assert_eq!(serde_json::from_str::<LoaderKind>(r#""neoforge""#).unwrap(), LoaderKind::NeoForge);
    assert_eq!((LoaderKind::Forge.label(), LoaderKind::NeoForge.label()), ("Forge", "NeoForge"));
}

#[test]
fn only_forge_and_neoforge_run_installers() {
    assert!(LoaderKind::Forge.runs_installer() && LoaderKind::NeoForge.runs_installer());
    assert!(!LoaderKind::Fabric.runs_installer() && !LoaderKind::Quilt.runs_installer());
}

#[tokio::test]
async fn installing_forge_without_a_java_is_a_clear_error_and_touches_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    let dl = Downloader::new(Source::Official);
    let err = loader::install_loader(&dl, &dirs, LoaderKind::Forge, "1.21.1", "52.1.0", None, None).await.unwrap_err();
    assert!(err.to_string().contains("Java"), "{err}");
    assert!(!dirs.versions().exists());
}
