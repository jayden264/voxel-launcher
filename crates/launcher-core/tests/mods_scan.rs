//! Naming files that were not installed by us, and finding newer versions.

use launcher_core::config::Dirs;
use launcher_core::download::sha1_bytes;
use launcher_core::mods::{list_installed, scan_content, target_for, ContentKind, ModrinthClient};
use launcher_core::loader::LoaderKind;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const INST: &str = "inst";

fn version(id: &str, project: &str, number: &str, kind: &str, filename: &str, sha1: &str) -> serde_json::Value {
    json!({
        "id": id, "project_id": project, "version_number": number, "version_type": kind,
        "files": [{ "hashes": { "sha1": sha1 }, "url": format!("https://cdn.modrinth.com/data/{project}/{filename}"),
                    "filename": filename, "primary": true, "size": 10 }],
        "dependencies": []
    })
}

async fn mount(server: &MockServer, route: &str, body: serde_json::Value) {
    Mock::given(method("POST")).and(path(route)).respond_with(ResponseTemplate::new(200).set_body_json(body)).mount(server).await;
}

#[tokio::test]
async fn names_unknown_jars_and_offers_newer_versions() {
    let server = MockServer::start().await;
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    let mods = dirs.mods_dir(INST).unwrap();
    std::fs::create_dir_all(&mods).unwrap();

    // A hand-dropped jar, a disabled one (never offered an update) and one nobody knows.
    let (old, off, unknown) = (b"old-sodium".to_vec(), b"off-lithium".to_vec(), b"mystery".to_vec());
    std::fs::write(mods.join("sodium-0.5.jar"), &old).unwrap();
    std::fs::write(mods.join("lithium-0.1.jar.disabled"), &off).unwrap();
    std::fs::write(mods.join("mystery.jar"), &unknown).unwrap();
    let (h_old, h_off) = (sha1_bytes(&old), sha1_bytes(&off));

    mount(
        &server,
        "/version_files",
        json!({
            h_old.clone(): version("v-old", "SOD", "0.5", "release", "sodium-0.5.jar", &h_old),
            h_off.clone(): version("v-off", "LIT", "0.1", "release", "lithium-0.1.jar", &h_off),
        }),
    )
    .await;
    mount(
        &server,
        "/version_files/update",
        json!({ h_old.clone(): version("v-new", "SOD", "0.6", "release", "sodium-0.6.jar", &"a".repeat(40)) }),
    )
    .await;
    Mock::given(method("GET"))
        .and(path("/projects"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "id": "SOD", "title": "Sodium", "icon_url": "https://cdn.modrinth.com/s.png" },
            { "id": "LIT", "title": "Lithium", "icon_url": null }
        ])))
        .mount(&server)
        .await;

    let client = ModrinthClient::new(server.uri());
    let t = target_for("1.21.1", LoaderKind::Fabric);
    let report = scan_content(&client, &dirs, INST, ContentKind::Mod, &t).await.unwrap();

    assert_eq!(report.identified, 2);
    assert_eq!(report.updates.len(), 1, "the disabled mod gets no update: {:?}", report.updates);
    let u = &report.updates[0];
    assert_eq!((u.filename.as_str(), u.title.as_str()), ("sodium-0.5.jar", "Sodium"));
    assert_eq!((u.current_version.as_str(), u.new_version.as_str()), ("0.5", "0.6"));
    assert_eq!(u.file.filename, "sodium-0.6.jar");

    let list = list_installed(&dirs, INST).unwrap();
    let sodium = list.iter().find(|m| m.filename == "sodium-0.5.jar").unwrap();
    assert_eq!(sodium.title.as_deref(), Some("Sodium"));
    assert_eq!(sodium.project_id.as_deref(), Some("SOD"));
    assert_eq!(list.iter().find(|m| m.filename == "lithium-0.1.jar").unwrap().title.as_deref(), Some("Lithium"));
    assert_eq!(list.iter().find(|m| m.filename == "mystery.jar").unwrap().title, None);
}

#[tokio::test]
async fn a_beta_is_not_offered_over_a_release() {
    let server = MockServer::start().await;
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    let mods = dirs.mods_dir(INST).unwrap();
    std::fs::create_dir_all(&mods).unwrap();
    let data = b"stable".to_vec();
    std::fs::write(mods.join("x-1.jar"), &data).unwrap();
    let h = sha1_bytes(&data);

    mount(&server, "/version_files", json!({ h.clone(): version("v1", "X", "1.0", "release", "x-1.jar", &h) })).await;
    mount(&server, "/version_files/update", json!({ h.clone(): version("v2", "X", "2.0-beta", "beta", "x-2.jar", &"b".repeat(40)) })).await;
    Mock::given(method("GET")).and(path("/projects")).respond_with(ResponseTemplate::new(200).set_body_json(json!([{ "id": "X", "title": "X" }]))).mount(&server).await;

    let client = ModrinthClient::new(server.uri());
    let report = scan_content(&client, &dirs, INST, ContentKind::Mod, &target_for("1.21.1", LoaderKind::Fabric)).await.unwrap();
    assert!(report.updates.is_empty());
}

#[tokio::test]
async fn an_up_to_date_mod_has_no_update_and_an_empty_folder_is_fine() {
    let server = MockServer::start().await;
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    let client = ModrinthClient::new(server.uri());
    let t = target_for("1.21.1", LoaderKind::Fabric);

    let empty = scan_content(&client, &dirs, INST, ContentKind::Mod, &t).await.unwrap();
    assert_eq!((empty.identified, empty.updates.len()), (0, 0));
    assert!(server.received_requests().await.unwrap().is_empty(), "nothing to ask about");

    let mods = dirs.mods_dir(INST).unwrap();
    std::fs::create_dir_all(&mods).unwrap();
    let data = b"latest".to_vec();
    std::fs::write(mods.join("y.jar"), &data).unwrap();
    let h = sha1_bytes(&data);
    let v = version("same", "Y", "3.0", "release", "y.jar", &h);
    mount(&server, "/version_files", json!({ h.clone(): v.clone() })).await;
    mount(&server, "/version_files/update", json!({ h.clone(): v })).await;
    Mock::given(method("GET")).and(path("/projects")).respond_with(ResponseTemplate::new(200).set_body_json(json!([{ "id": "Y", "title": "Y" }]))).mount(&server).await;
    let report = scan_content(&client, &dirs, INST, ContentKind::Mod, &t).await.unwrap();
    assert!(report.updates.is_empty());
}
