//! Recommended mods: per-loader lists, availability filtering and merged plans.

use launcher_core::loader::LoaderKind;
use launcher_core::mods::{target_for, ModrinthClient};
use launcher_core::recommended::{available, plan_many, recommended_for};
use serde_json::{json, Value};
use std::collections::HashSet;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn version(id: &str, project: &str, size: u64, deps: &[(&str, &str)]) -> Value {
    json!({
        "id": id, "project_id": project, "version_number": "1.0", "version_type": "release",
        "files": [{"hashes": {"sha1": format!("{:0>40}", id)}, "url": format!("https://cdn.modrinth.com/{id}.jar"),
                   "filename": format!("{id}.jar"), "primary": true, "size": size}],
        "dependencies": deps.iter().map(|(p, t)| json!({"project_id": p, "version_id": null, "dependency_type": t})).collect::<Vec<_>>()
    })
}

async fn mount(server: &MockServer, project: &str, versions: Vec<Value>) {
    Mock::given(method("GET"))
        .and(path(format!("/project/{project}/version")))
        .respond_with(ResponseTemplate::new(200).set_body_json(Value::Array(versions)))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/project/{project}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": project, "title": project.to_uppercase(), "icon_url": null})))
        .mount(server)
        .await;
}

fn none() -> HashSet<String> {
    HashSet::new()
}

#[test]
fn every_loader_has_its_own_list() {
    assert!(recommended_for(LoaderKind::Forge).is_empty());
    let fabric = recommended_for(LoaderKind::Fabric);
    assert_eq!(fabric[0].slug, "fabric-api");
    assert_eq!(fabric.iter().filter(|r| r.default_on).map(|r| r.slug).collect::<Vec<_>>(), vec!["fabric-api"]);
    assert!(fabric.iter().any(|r| r.slug == "iris") && fabric.iter().any(|r| r.slug == "sodium"));
    assert_eq!(recommended_for(LoaderKind::Quilt).len(), fabric.len());
    let neo: Vec<_> = recommended_for(LoaderKind::NeoForge).iter().map(|r| r.slug).collect();
    assert_eq!(neo, vec!["sodium", "lithium", "iris"]);
    assert!(recommended_for(LoaderKind::NeoForge).iter().all(|r| !r.default_on));
}

#[tokio::test]
async fn available_drops_entries_without_a_compatible_version() {
    let server = MockServer::start().await;
    mount(&server, "fabric-api", vec![version("fa", "fabric-api", 10, &[])]).await;
    mount(&server, "sodium", vec![version("so", "sodium", 10, &[])]).await;
    mount(&server, "lithium", vec![]).await; // no build for this version
    Mock::given(method("GET")).and(path("/project/modmenu/version")).respond_with(ResponseTemplate::new(500)).mount(&server).await;
    mount(&server, "iris", vec![version("ir", "iris", 10, &[("sodium", "required")])]).await;

    let list = available(&ModrinthClient::new(server.uri()), LoaderKind::Fabric, "1.21.1").await;
    let slugs: Vec<_> = list.iter().map(|r| r.slug).collect();
    assert_eq!(slugs, vec!["fabric-api", "sodium", "iris"], "lithium has no build, modmenu errored");
}

#[tokio::test]
async fn plan_many_installs_a_shared_dependency_once_and_marks_chosen_mods() {
    let server = MockServer::start().await;
    mount(&server, "iris", vec![version("ir", "iris", 100, &[("sodium", "required")])]).await;
    mount(&server, "sodium", vec![version("so", "sodium", 50, &[])]).await;
    mount(&server, "fabric-api", vec![version("fa", "fabric-api", 10, &[])]).await;
    let t = target_for("1.21.1", LoaderKind::Fabric);
    let slugs: Vec<String> = ["fabric-api", "iris", "sodium"].map(String::from).to_vec();

    let plan = plan_many(&ModrinthClient::new(server.uri()), &t, &slugs, &none()).await.unwrap();
    let ids: Vec<_> = plan.files.iter().map(|f| f.project_id.as_str()).collect();
    assert_eq!(ids, vec!["fabric-api", "iris", "sodium"], "each project once");
    assert_eq!(plan.total_bytes, 160);
    let sodium = plan.files.iter().find(|f| f.project_id == "sodium").unwrap();
    assert!(sodium.user_selected, "sodium was chosen explicitly even though iris needs it too");
    assert!(plan.files.iter().all(|f| f.user_selected));
}

#[tokio::test]
async fn plan_many_marks_pure_dependencies_as_such() {
    let server = MockServer::start().await;
    mount(&server, "iris", vec![version("ir", "iris", 100, &[("sodium", "required")])]).await;
    mount(&server, "sodium", vec![version("so", "sodium", 50, &[])]).await;
    let t = target_for("1.21.1", LoaderKind::Fabric);
    let plan = plan_many(&ModrinthClient::new(server.uri()), &t, &["iris".to_string()], &none()).await.unwrap();
    assert_eq!(plan.files.len(), 2);
    let sodium = plan.files.iter().find(|f| f.project_id == "sodium").unwrap();
    assert!(!sodium.user_selected && sodium.required_by.as_deref() == Some("IRIS"));
}

#[tokio::test]
async fn plan_many_skips_what_is_already_installed() {
    let server = MockServer::start().await;
    mount(&server, "fabric-api", vec![version("fa", "fabric-api", 10, &[])]).await;
    mount(&server, "sodium", vec![version("so", "sodium", 50, &[])]).await;
    let t = target_for("1.21.1", LoaderKind::Fabric);
    let installed: HashSet<String> = ["fabric-api".to_string()].into();
    let slugs: Vec<String> = ["fabric-api", "sodium"].map(String::from).to_vec();
    let plan = plan_many(&ModrinthClient::new(server.uri()), &t, &slugs, &installed).await.unwrap();
    let ids: Vec<_> = plan.files.iter().map(|f| f.project_id.as_str()).collect();
    assert_eq!(ids, vec!["sodium"]);
}

#[tokio::test]
async fn plan_many_with_an_empty_choice_is_an_empty_plan() {
    let server = MockServer::start().await;
    let t = target_for("1.21.1", LoaderKind::Fabric);
    let plan = plan_many(&ModrinthClient::new(server.uri()), &t, &[], &none()).await.unwrap();
    assert!(plan.files.is_empty() && plan.total_bytes == 0);
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn plan_many_fails_naming_an_unavailable_choice() {
    let server = MockServer::start().await;
    mount(&server, "lithium", vec![]).await;
    let t = target_for("1.21.1", LoaderKind::Fabric);
    let err = plan_many(&ModrinthClient::new(server.uri()), &t, &["lithium".to_string()], &none()).await.unwrap_err();
    assert!(err.to_string().contains("没有适配"), "{err}");
}
