//! Installing, listing, disabling and removing mods in an instance folder.

use launcher_core::config::Dirs;
use launcher_core::download::{sha1_bytes, Downloader};
use launcher_core::mirror::Source;
use launcher_core::mods::{
    install_plan, installed_project_ids, list_installed, remove_mod, set_enabled, InstallPlan, PlanFile,
};
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const INST: &str = "inst";

fn dl() -> Downloader {
    Downloader::new(Source::Official).with_retry(0, Duration::from_millis(1))
}

fn bytes(tag: &str) -> Vec<u8> {
    format!("jar-bytes-{tag}").into_bytes().repeat(50)
}

/// A plan entry whose download URL is `<server>/<url_path>`.
fn file(server: &MockServer, project: &str, filename: &str, url_path: &str, data: &[u8], selected: bool) -> PlanFile {
    PlanFile {
        project_id: project.into(),
        version_id: format!("v-{filename}"),
        title: format!("Title {project}"),
        version_number: "1.0".into(),
        icon_url: Some("https://cdn.modrinth.com/i.png".into()),
        filename: filename.into(),
        url: format!("{}/{url_path}", server.uri()),
        sha1: sha1_bytes(data),
        size: data.len() as u64,
        required_by: (!selected).then(|| "Title A".to_string()),
        user_selected: selected,
        dest: launcher_core::mods::ContentKind::Mod,
    }
}

fn plan(files: Vec<PlanFile>) -> InstallPlan {
    let total_bytes = files.iter().map(|f| f.size).sum();
    InstallPlan { files, total_bytes, warnings: vec![] }
}

async fn serve(server: &MockServer, url_path: &str, data: &[u8]) {
    Mock::given(method("GET"))
        .and(path(format!("/{url_path}")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(data.to_vec()))
        .mount(server)
        .await;
}

fn mods_dir(dirs: &Dirs) -> std::path::PathBuf {
    dirs.mods_dir(INST).unwrap()
}

fn entries(dir: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .map(|r| r.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

#[tokio::test]
async fn installs_files_and_records_them() {
    let server = MockServer::start().await;
    let (a, b) = (bytes("a"), bytes("b"));
    serve(&server, "a", &a).await;
    serve(&server, "b", &b).await;
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    let p = plan(vec![file(&server, "A", "mod-a.jar", "a", &a, true), file(&server, "B", "lib-b.jar", "b", &b, false)]);

    install_plan(&dl(), &dirs, INST, &p, None).await.unwrap();

    assert_eq!(std::fs::read(mods_dir(&dirs).join("mod-a.jar")).unwrap(), a);
    assert_eq!(std::fs::read(mods_dir(&dirs).join("lib-b.jar")).unwrap(), b);
    let list = list_installed(&dirs, INST).unwrap();
    assert_eq!(list.len(), 2);
    let ma = list.iter().find(|m| m.filename == "mod-a.jar").unwrap();
    assert!(ma.enabled && ma.user_installed);
    assert_eq!(ma.project_id.as_deref(), Some("A"));
    assert_eq!(ma.title.as_deref(), Some("Title A"));
    assert_eq!(ma.size, a.len() as u64);
    assert!(!list.iter().find(|m| m.filename == "lib-b.jar").unwrap().user_installed);
    assert!(dirs.mods_record(INST).unwrap().is_file());
}

#[tokio::test]
async fn rejects_malicious_filenames_before_writing() {
    let server = MockServer::start().await;
    let good = bytes("good");
    serve(&server, "good", &good).await;
    serve(&server, "evil", &good).await;
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    for bad in ["../evil.jar", "a/b.jar", "a\\b.jar", "x.exe", "C:evil.jar", "", "evil.jar.disabled", "..jar"] {
        let p = plan(vec![
            file(&server, "G", "good.jar", "good", &good, true),
            file(&server, "E", bad, "evil", &good, false),
        ]);
        assert!(install_plan(&dl(), &dirs, INST, &p, None).await.is_err(), "{bad:?} must be rejected");
        assert!(entries(&mods_dir(&dirs)).is_empty(), "{bad:?}: nothing may be written, got {:?}", entries(&mods_dir(&dirs)));
    }
    assert!(!tmp.path().join("evil.jar").exists());
    assert!(server.received_requests().await.unwrap().is_empty(), "no request before validation");
}

#[tokio::test]
async fn bad_checksum_leaves_no_file_and_no_record() {
    let server = MockServer::start().await;
    serve(&server, "a", &bytes("served")).await;
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    // The plan promises different bytes than the server delivers.
    let p = plan(vec![file(&server, "A", "mod-a.jar", "a", &bytes("promised"), true)]);

    assert!(install_plan(&dl(), &dirs, INST, &p, None).await.is_err());
    assert!(entries(&mods_dir(&dirs)).is_empty(), "{:?}", entries(&mods_dir(&dirs)));
    assert!(list_installed(&dirs, INST).unwrap().is_empty());
}

#[tokio::test]
async fn partial_failure_keeps_successful_files() {
    let server = MockServer::start().await;
    let a = bytes("a");
    serve(&server, "a", &a).await;
    Mock::given(method("GET")).and(path("/gone")).respond_with(ResponseTemplate::new(404)).mount(&server).await;
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    let p = plan(vec![
        file(&server, "A", "mod-a.jar", "a", &a, true),
        file(&server, "B", "lib-b.jar", "gone", &bytes("b"), false),
    ]);

    assert!(install_plan(&dl(), &dirs, INST, &p, None).await.is_err());
    assert_eq!(entries(&mods_dir(&dirs)), vec!["mod-a.jar"]);
    let list = list_installed(&dirs, INST).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].project_id.as_deref(), Some("A"));
}

#[tokio::test]
async fn reinstall_same_project_replaces_old_file() {
    let server = MockServer::start().await;
    let (v1, v2) = (bytes("v1"), bytes("v2"));
    serve(&server, "v1", &v1).await;
    serve(&server, "v2", &v2).await;
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    install_plan(&dl(), &dirs, INST, &plan(vec![file(&server, "P", "a-1.0.jar", "v1", &v1, true)]), None).await.unwrap();
    install_plan(&dl(), &dirs, INST, &plan(vec![file(&server, "P", "a-2.0.jar", "v2", &v2, true)]), None).await.unwrap();

    assert_eq!(entries(&mods_dir(&dirs)), vec!["a-2.0.jar"]);
    let list = list_installed(&dirs, INST).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].filename, "a-2.0.jar");
}

#[tokio::test]
async fn disable_and_enable_roundtrip() {
    let server = MockServer::start().await;
    let a = bytes("a");
    serve(&server, "a", &a).await;
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    install_plan(&dl(), &dirs, INST, &plan(vec![file(&server, "A", "x.jar", "a", &a, true)]), None).await.unwrap();

    set_enabled(&dirs, INST, "x.jar", false).unwrap();
    assert_eq!(entries(&mods_dir(&dirs)), vec!["x.jar.disabled"]);
    let m = &list_installed(&dirs, INST).unwrap()[0];
    assert_eq!((m.filename.as_str(), m.enabled), ("x.jar", false));
    assert_eq!(m.project_id.as_deref(), Some("A"), "record survives disabling");

    set_enabled(&dirs, INST, "x.jar", true).unwrap();
    assert_eq!(entries(&mods_dir(&dirs)), vec!["x.jar"]);
    assert!(list_installed(&dirs, INST).unwrap()[0].enabled);

    assert!(set_enabled(&dirs, INST, "nope.jar", false).is_err());
    assert!(set_enabled(&dirs, INST, "../x.jar", false).is_err());
}

#[tokio::test]
async fn disabled_mod_still_counts_as_installed_project() {
    let server = MockServer::start().await;
    let a = bytes("a");
    serve(&server, "a", &a).await;
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    install_plan(&dl(), &dirs, INST, &plan(vec![file(&server, "A", "x.jar", "a", &a, true)]), None).await.unwrap();
    set_enabled(&dirs, INST, "x.jar", false).unwrap();
    assert!(installed_project_ids(&dirs, INST).unwrap().contains("A"));
}

#[tokio::test]
async fn remove_mod_deletes_file_and_record_and_rejects_traversal() {
    let server = MockServer::start().await;
    let a = bytes("a");
    serve(&server, "a", &a).await;
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    install_plan(&dl(), &dirs, INST, &plan(vec![file(&server, "A", "x.jar", "a", &a, true)]), None).await.unwrap();

    // A file outside the mods folder that traversal would hit.
    let outside = dirs.instance(INST).unwrap().join("keep.jar");
    std::fs::write(&outside, b"keep").unwrap();
    assert!(remove_mod(&dirs, INST, "../keep.jar").is_err());
    assert!(outside.exists());

    remove_mod(&dirs, INST, "x.jar").unwrap();
    assert!(entries(&mods_dir(&dirs)).is_empty());
    assert!(installed_project_ids(&dirs, INST).unwrap().is_empty());
    assert!(remove_mod(&dirs, INST, "x.jar").is_err(), "already gone");

    // Removing a disabled mod works too.
    install_plan(&dl(), &dirs, INST, &plan(vec![file(&server, "A", "y.jar", "a", &a, true)]), None).await.unwrap();
    set_enabled(&dirs, INST, "y.jar", false).unwrap();
    remove_mod(&dirs, INST, "y.jar").unwrap();
    assert!(entries(&mods_dir(&dirs)).is_empty());
}

#[tokio::test]
async fn corrupt_mods_json_is_treated_as_empty() {
    let server = MockServer::start().await;
    let a = bytes("a");
    serve(&server, "a", &a).await;
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    install_plan(&dl(), &dirs, INST, &plan(vec![file(&server, "A", "x.jar", "a", &a, true)]), None).await.unwrap();
    std::fs::write(dirs.mods_record(INST).unwrap(), b"{ not json").unwrap();

    let list = list_installed(&dirs, INST).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].filename, "x.jar");
    assert!(list[0].project_id.is_none());
}

#[tokio::test]
async fn manually_added_jar_is_listed_without_project() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    assert!(list_installed(&dirs, INST).unwrap().is_empty(), "missing mods folder is fine");
    std::fs::create_dir_all(mods_dir(&dirs)).unwrap();
    std::fs::write(mods_dir(&dirs).join("hand.jar"), b"1234").unwrap();
    std::fs::write(mods_dir(&dirs).join("off.jar.disabled"), b"12").unwrap();
    std::fs::write(mods_dir(&dirs).join("readme.txt"), b"x").unwrap();

    let list = list_installed(&dirs, INST).unwrap();
    let names: Vec<_> = list.iter().map(|m| (m.filename.as_str(), m.enabled)).collect();
    assert_eq!(names, vec![("hand.jar", true), ("off.jar", false)]);
    assert!(list.iter().all(|m| m.project_id.is_none() && m.user_installed));
    assert_eq!(list[0].size, 4);
}
