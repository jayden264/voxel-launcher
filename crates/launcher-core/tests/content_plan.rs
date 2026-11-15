//! Planning shader packs and resource packs, including the Iris prerequisite.

use launcher_core::loader::LoaderKind;
use launcher_core::mods::{plan_content, ContentKind, ModrinthClient};
use serde_json::{json, Value};
use std::collections::HashSet;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn version(id: &str, project: &str, file: &str, size: u64, deps: &[(&str, &str)]) -> Value {
    json!({
        "id": id, "project_id": project, "version_number": "1.0", "version_type": "release",
        "files": [{"hashes": {"sha1": format!("{:0>40}", id)}, "url": format!("https://cdn.modrinth.com/{id}/{file}"),
                   "filename": file, "primary": true, "size": size}],
        "dependencies": deps.iter().map(|(p, t)| json!({"project_id": p, "version_id": null, "dependency_type": t})).collect::<Vec<_>>()
    })
}

async fn mount(server: &MockServer, project: &str, title: &str, versions: Vec<Value>) {
    Mock::given(method("GET"))
        .and(path(format!("/project/{project}/version")))
        .respond_with(ResponseTemplate::new(200).set_body_json(Value::Array(versions)))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/project/{project}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": project, "title": title, "icon_url": null})))
        .mount(server)
        .await;
}

fn set(ids: &[&str]) -> HashSet<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

async fn iris_world(server: &MockServer) {
    mount(server, "iris", "Iris Shaders", vec![version("ir", "iris", "iris.jar", 100, &[("sodium", "required")])]).await;
    mount(server, "sodium", "Sodium", vec![version("so", "sodium", "sodium.jar", 50, &[])]).await;
    mount(server, "complementary", "Complementary", vec![version("co", "complementary", "Complementary.zip", 7, &[])]).await;
}

#[tokio::test]
async fn shader_on_fabric_without_iris_adds_iris_and_sodium() {
    let server = MockServer::start().await;
    iris_world(&server).await;
    let c = ModrinthClient::new(server.uri());
    let plan = plan_content(&c, LoaderKind::Fabric, "1.21.1", ContentKind::Shader, "complementary", &set(&[]), &set(&[])).await.unwrap();

    let got: Vec<_> = plan.files.iter().map(|f| (f.project_id.as_str(), f.dest, f.user_selected)).collect();
    assert_eq!(
        got,
        vec![
            ("complementary", ContentKind::Shader, true),
            ("iris", ContentKind::Mod, false),
            ("sodium", ContentKind::Mod, false)
        ]
    );
    assert_eq!(plan.files[1].required_by.as_deref(), Some("Complementary"), "iris is there for the shader");
    assert_eq!(plan.files[2].required_by.as_deref(), Some("Iris Shaders"));
    assert_eq!(plan.total_bytes, 157);
    assert!(plan.warnings.is_empty());
}

#[tokio::test]
async fn shader_with_iris_already_installed_adds_nothing_else() {
    let server = MockServer::start().await;
    iris_world(&server).await;
    let c = ModrinthClient::new(server.uri());
    let plan = plan_content(&c, LoaderKind::NeoForge, "1.21.1", ContentKind::Shader, "complementary", &set(&["iris", "sodium"]), &set(&[])).await.unwrap();
    assert_eq!(plan.files.len(), 1);
    assert_eq!(plan.files[0].dest, ContentKind::Shader);
}

#[tokio::test]
async fn shader_on_forge_warns_and_adds_no_mods() {
    let server = MockServer::start().await;
    iris_world(&server).await;
    let c = ModrinthClient::new(server.uri());
    let plan = plan_content(&c, LoaderKind::Forge, "1.20.1", ContentKind::Shader, "complementary", &set(&[]), &set(&[])).await.unwrap();
    assert_eq!(plan.files.len(), 1);
    assert_eq!(plan.warnings.len(), 1);
    assert!(plan.warnings[0].contains("Oculus"), "{:?}", plan.warnings);
    // Iris was never even asked about.
    let asked: Vec<_> = server.received_requests().await.unwrap().iter().map(|r| r.url.path().to_string()).collect();
    assert!(asked.iter().all(|p| !p.contains("iris")), "{asked:?}");
}

#[tokio::test]
async fn resource_packs_have_no_prerequisites_on_any_loader() {
    let server = MockServer::start().await;
    mount(&server, "faithful", "Faithful", vec![version("fa", "faithful", "Faithful.zip", 9, &[])]).await;
    let c = ModrinthClient::new(server.uri());
    for loader in [LoaderKind::Fabric, LoaderKind::Forge] {
        let plan = plan_content(&c, loader, "1.21.1", ContentKind::ResourcePack, "faithful", &set(&[]), &set(&[])).await.unwrap();
        assert_eq!(plan.files.len(), 1);
        assert_eq!((plan.files[0].dest, plan.files[0].filename.as_str()), (ContentKind::ResourcePack, "Faithful.zip"));
        assert!(plan.warnings.is_empty());
    }
}

#[tokio::test]
async fn packs_are_looked_up_with_their_own_loader_names() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/project/faithful/version"))
        .and(query_param("loaders", r#"["minecraft"]"#))
        .and(query_param("game_versions", r#"["1.21.1"]"#))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([version("fa", "faithful", "F.zip", 1, &[])])))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/project/faithful"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "faithful", "title": "F", "icon_url": null})))
        .mount(&server)
        .await;
    plan_content(&ModrinthClient::new(server.uri()), LoaderKind::Fabric, "1.21.1", ContentKind::ResourcePack, "faithful", &set(&[]), &set(&[])).await.unwrap();
    server.verify().await;
}

#[tokio::test]
async fn a_pack_without_a_compatible_version_is_a_clear_error() {
    let server = MockServer::start().await;
    mount(&server, "old-pack", "Old Pack", vec![]).await;
    let c = ModrinthClient::new(server.uri());
    let err = plan_content(&c, LoaderKind::Fabric, "1.21.1", ContentKind::Shader, "old-pack", &set(&[]), &set(&[])).await.unwrap_err().to_string();
    assert!(err.contains("Old Pack") && err.contains("1.21.1") && err.contains("没有适配"), "{err}");
}

#[tokio::test]
async fn an_installed_pack_is_refused() {
    let server = MockServer::start().await;
    mount(&server, "faithful", "Faithful", vec![version("fa", "faithful", "F.zip", 1, &[])]).await;
    let c = ModrinthClient::new(server.uri());
    let err = plan_content(&c, LoaderKind::Fabric, "1.21.1", ContentKind::ResourcePack, "faithful", &set(&[]), &set(&["faithful"])).await.unwrap_err();
    assert!(err.to_string().contains("已安装"), "{err}");
}

#[tokio::test]
async fn mods_still_use_the_dependency_planner() {
    let server = MockServer::start().await;
    mount(&server, "iris", "Iris Shaders", vec![version("ir", "iris", "iris.jar", 100, &[("sodium", "required")])]).await;
    mount(&server, "sodium", "Sodium", vec![version("so", "sodium", "sodium.jar", 50, &[])]).await;
    let c = ModrinthClient::new(server.uri());
    let plan = plan_content(&c, LoaderKind::Fabric, "1.21.1", ContentKind::Mod, "iris", &set(&[]), &set(&[])).await.unwrap();
    assert_eq!(plan.files.len(), 2);
    assert!(plan.files.iter().all(|f| f.dest == ContentKind::Mod));
}
