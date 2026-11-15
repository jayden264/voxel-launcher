//! Dependency planning against a mock Modrinth.

use launcher_core::loader::LoaderKind;
use launcher_core::mods::{plan_install, target_for, ModrinthClient, Target};
use serde_json::{json, Value};
use std::collections::HashSet;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// One Modrinth version object. `deps`: `(project_id, version_id, type)`; an
/// empty string means "absent".
fn version(id: &str, project: &str, vtype: &str, size: u64, deps: &[(&str, &str, &str)]) -> Value {
    let opt = |s: &str| if s.is_empty() { Value::Null } else { json!(s) };
    json!({
        "id": id, "project_id": project, "version_number": format!("{id}-1.0"), "version_type": vtype,
        "files": [
            {"hashes": {"sha1": format!("{:0>40}", id)}, "url": format!("https://cdn.example/{id}.jar"),
             "filename": format!("{id}.jar"), "primary": true, "size": size},
            {"hashes": {"sha1": "f".repeat(40)}, "url": "https://cdn.example/sources.jar",
             "filename": "sources.jar", "primary": false, "size": 1}
        ],
        "dependencies": deps.iter().map(|(p, v, t)| json!({"project_id": opt(p), "version_id": opt(v), "dependency_type": t})).collect::<Vec<_>>()
    })
}

async fn mount_project(server: &MockServer, project: &str, title: &str, versions: Vec<Value>) {
    Mock::given(method("GET"))
        .and(path(format!("/project/{project}/version")))
        .respond_with(ResponseTemplate::new(200).set_body_json(Value::Array(versions)))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/project/{project}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"title": title, "icon_url": null})))
        .mount(server)
        .await;
}

fn target() -> Target {
    target_for("1.21.1", LoaderKind::Fabric)
}

fn none() -> HashSet<String> {
    HashSet::new()
}

#[tokio::test]
async fn plans_selected_mod_plus_required_dependency() {
    let server = MockServer::start().await;
    mount_project(&server, "A", "Mod A", vec![version("va", "A", "release", 100, &[("B", "", "required"), ("C", "", "optional")])]).await;
    mount_project(&server, "B", "Mod B", vec![version("vb", "B", "release", 50, &[])]).await;
    mount_project(&server, "C", "Mod C", vec![version("vc", "C", "release", 7, &[])]).await;

    let plan = plan_install(&ModrinthClient::new(server.uri()), &target(), "A", &none()).await.unwrap();
    let ids: Vec<_> = plan.files.iter().map(|f| f.project_id.as_str()).collect();
    assert_eq!(ids, vec!["A", "B"], "optional C must not be planned");
    assert!(plan.files[0].user_selected && plan.files[0].required_by.is_none());
    assert!(!plan.files[1].user_selected);
    assert_eq!(plan.files[1].required_by.as_deref(), Some("Mod A"));
    assert_eq!(plan.files[0].filename, "va.jar", "primary file is chosen");
    assert_eq!(plan.files[0].sha1, format!("{:0>40}", "va"));
    assert_eq!(plan.total_bytes, 150);
    assert!(plan.warnings.is_empty());
}

#[tokio::test]
async fn skips_already_installed_dependency() {
    let server = MockServer::start().await;
    mount_project(&server, "A", "Mod A", vec![version("va", "A", "release", 100, &[("B", "", "required")])]).await;
    mount_project(&server, "B", "Mod B", vec![version("vb", "B", "release", 50, &[])]).await;
    let installed: HashSet<String> = ["B".to_string()].into();
    let plan = plan_install(&ModrinthClient::new(server.uri()), &target(), "A", &installed).await.unwrap();
    assert_eq!(plan.files.len(), 1);
    assert_eq!(plan.total_bytes, 100);
}

#[tokio::test]
async fn terminates_on_dependency_cycle() {
    let server = MockServer::start().await;
    mount_project(&server, "A", "Mod A", vec![version("va", "A", "release", 1, &[("B", "", "required")])]).await;
    mount_project(&server, "B", "Mod B", vec![version("vb", "B", "release", 1, &[("A", "", "required")])]).await;
    let plan = plan_install(&ModrinthClient::new(server.uri()), &target(), "A", &none()).await.unwrap();
    let ids: Vec<_> = plan.files.iter().map(|f| f.project_id.as_str()).collect();
    assert_eq!(ids, vec!["A", "B"]);
}

#[tokio::test]
async fn errors_when_no_compatible_version() {
    let server = MockServer::start().await;
    mount_project(&server, "A", "Mod A", vec![]).await;
    let err = plan_install(&ModrinthClient::new(server.uri()), &target(), "A", &none()).await.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("1.21.1") && msg.contains("fabric") && msg.contains("没有适配"), "{msg}");
}

#[tokio::test]
async fn errors_naming_dependency_without_compatible_version() {
    let server = MockServer::start().await;
    mount_project(&server, "A", "Mod A", vec![version("va", "A", "release", 1, &[("B", "", "required")])]).await;
    mount_project(&server, "B", "Library B", vec![]).await;
    let err = plan_install(&ModrinthClient::new(server.uri()), &target(), "A", &none()).await.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("Library B") && msg.contains("没有适配"), "{msg}");
}

#[tokio::test]
async fn prefers_release_over_beta_and_warns_when_only_beta() {
    let server = MockServer::start().await;
    // API order is newest first: the beta is newer than the release.
    mount_project(&server, "A", "Mod A", vec![version("beta", "A", "beta", 1, &[]), version("rel", "A", "release", 1, &[])]).await;
    let plan = plan_install(&ModrinthClient::new(server.uri()), &target(), "A", &none()).await.unwrap();
    assert_eq!(plan.files[0].version_id, "rel");
    assert!(plan.warnings.is_empty());

    let server = MockServer::start().await;
    mount_project(&server, "A", "Mod A", vec![version("b2", "A", "beta", 1, &[]), version("a1", "A", "alpha", 1, &[])]).await;
    let plan = plan_install(&ModrinthClient::new(server.uri()), &target(), "A", &none()).await.unwrap();
    assert_eq!(plan.files[0].version_id, "b2");
    assert_eq!(plan.warnings.len(), 1);
    assert!(plan.warnings[0].contains("Mod A") && plan.warnings[0].contains("beta"), "{:?}", plan.warnings);
}

#[tokio::test]
async fn resolves_dependency_given_only_a_version_id() {
    let server = MockServer::start().await;
    mount_project(&server, "A", "Mod A", vec![version("va", "A", "release", 1, &[("", "vb-pinned", "required")])]).await;
    Mock::given(method("GET"))
        .and(path("/version/vb-pinned"))
        .respond_with(ResponseTemplate::new(200).set_body_json(version("vb-pinned", "B", "release", 1, &[])))
        .mount(&server)
        .await;
    mount_project(&server, "B", "Mod B", vec![version("vb", "B", "release", 2, &[])]).await;
    let plan = plan_install(&ModrinthClient::new(server.uri()), &target(), "A", &none()).await.unwrap();
    let ids: Vec<_> = plan.files.iter().map(|f| f.project_id.as_str()).collect();
    assert_eq!(ids, vec!["A", "B"]);
}

#[tokio::test]
async fn warns_on_incompatible_installed_mod() {
    let server = MockServer::start().await;
    mount_project(&server, "A", "Mod A", vec![version("va", "A", "release", 1, &[("X", "", "incompatible")])]).await;
    mount_project(&server, "X", "Mod X", vec![]).await;
    let installed: HashSet<String> = ["X".to_string()].into();
    let plan = plan_install(&ModrinthClient::new(server.uri()), &target(), "A", &installed).await.unwrap();
    assert_eq!(plan.files.len(), 1);
    assert_eq!(plan.warnings.len(), 1);
    assert!(plan.warnings[0].contains("Mod A") && plan.warnings[0].contains("Mod X"), "{:?}", plan.warnings);

    // Not installed: no warning.
    let plan = plan_install(&ModrinthClient::new(server.uri()), &target(), "A", &none()).await.unwrap();
    assert!(plan.warnings.is_empty());
}

#[tokio::test]
async fn refuses_to_plan_a_project_that_is_already_installed() {
    let server = MockServer::start().await;
    mount_project(&server, "A", "Mod A", vec![version("va", "A", "release", 1, &[])]).await;
    let installed: HashSet<String> = ["A".to_string()].into();
    let err = plan_install(&ModrinthClient::new(server.uri()), &target(), "A", &installed).await.unwrap_err();
    assert!(err.to_string().contains("已安装"), "{err}");
}
