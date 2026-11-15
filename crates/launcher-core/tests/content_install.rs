//! Shader packs and resource packs: their own folders, extensions and records.

use launcher_core::config::Dirs;
use launcher_core::download::{sha1_bytes, Downloader};
use launcher_core::loader::LoaderKind;
use launcher_core::mirror::Source;
use launcher_core::mods::{
    content_target, install_plan, list_content, list_installed, remove_content, target_for, ContentKind, InstallPlan,
    ModrinthClient, PlanFile,
};
use std::time::Duration;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const INST: &str = "inst";

fn dl() -> Downloader {
    Downloader::new(Source::Official).with_retry(0, Duration::from_millis(1))
}

fn file(server: &MockServer, project: &str, filename: &str, data: &[u8], dest: ContentKind) -> PlanFile {
    PlanFile {
        project_id: project.into(),
        version_id: format!("v-{project}"),
        title: format!("Title {project}"),
        version_number: "1.0".into(),
        icon_url: None,
        filename: filename.into(),
        url: format!("{}/{project}", server.uri()),
        sha1: sha1_bytes(data),
        size: data.len() as u64,
        required_by: None,
        user_selected: true,
        dest,
    }
}

async fn serve(server: &MockServer, project: &str, data: &[u8]) {
    Mock::given(method("GET"))
        .and(path(format!("/{project}")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(data.to_vec()))
        .mount(server)
        .await;
}

fn plan(files: Vec<PlanFile>) -> InstallPlan {
    let total_bytes = files.iter().map(|f| f.size).sum();
    InstallPlan { files, total_bytes, warnings: vec![] }
}

fn names(dir: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .map(|r| r.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

#[test]
fn content_kinds_know_their_folder_extension_and_type() {
    assert_eq!(ContentKind::default(), ContentKind::Mod);
    assert_eq!((ContentKind::Mod.dir_name(), ContentKind::Mod.extension(), ContentKind::Mod.project_type()), ("mods", ".jar", "mod"));
    assert_eq!(
        (ContentKind::Shader.dir_name(), ContentKind::Shader.extension(), ContentKind::Shader.project_type()),
        ("shaderpacks", ".zip", "shader")
    );
    assert_eq!(
        (ContentKind::ResourcePack.dir_name(), ContentKind::ResourcePack.extension(), ContentKind::ResourcePack.project_type()),
        ("resourcepacks", ".zip", "resourcepack")
    );
    assert_eq!(serde_json::to_string(&ContentKind::ResourcePack).unwrap(), r#""resourcepack""#);
}

#[test]
fn plans_without_a_destination_are_mods() {
    let f: PlanFile = serde_json::from_str(
        r#"{"project_id":"p","version_id":"v","title":"t","version_number":"1","icon_url":null,"filename":"a.jar",
            "url":"u","sha1":"s","size":1,"required_by":null,"user_selected":true}"#,
    )
    .unwrap();
    assert_eq!(f.dest, ContentKind::Mod);
}

#[test]
fn content_targets_use_the_right_loaders() {
    assert_eq!(content_target("1.21.1", LoaderKind::Quilt, ContentKind::Mod), target_for("1.21.1", LoaderKind::Quilt));
    assert_eq!(content_target("1.21.1", LoaderKind::Fabric, ContentKind::Shader).loaders, vec!["iris"]);
    assert_eq!(content_target("1.21.1", LoaderKind::Forge, ContentKind::ResourcePack).loaders, vec!["minecraft"]);
    assert_eq!(content_target("1.21.1", LoaderKind::Fabric, ContentKind::Shader).content, ContentKind::Shader);
}

#[tokio::test]
async fn files_go_to_the_folder_of_their_kind() {
    let server = MockServer::start().await;
    for (p, d) in [("m", &b"mod"[..]), ("s", &b"shader"[..]), ("r", &b"pack"[..])] {
        serve(&server, p, d).await;
    }
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    let p = plan(vec![
        file(&server, "m", "a.jar", b"mod", ContentKind::Mod),
        file(&server, "s", "Complementary.zip", b"shader", ContentKind::Shader),
        file(&server, "r", "Faithful.zip", b"pack", ContentKind::ResourcePack),
    ]);
    install_plan(&dl(), &dirs, INST, &p, None).await.unwrap();

    let inst = dirs.instance(INST).unwrap();
    assert_eq!(names(&inst.join("mods")), vec!["a.jar"]);
    assert_eq!(names(&inst.join("shaderpacks")), vec!["Complementary.zip"]);
    assert_eq!(names(&inst.join("resourcepacks")), vec!["Faithful.zip"]);

    // Records are kept per kind.
    let shaders = list_content(&dirs, INST, ContentKind::Shader).unwrap();
    assert_eq!(shaders.len(), 1);
    assert_eq!((shaders[0].filename.as_str(), shaders[0].project_id.as_deref()), ("Complementary.zip", Some("s")));
    assert_eq!(list_content(&dirs, INST, ContentKind::ResourcePack).unwrap().len(), 1);
    let mods = list_installed(&dirs, INST).unwrap();
    assert_eq!(mods.len(), 1);
    assert_eq!(mods[0].filename, "a.jar");
}

#[tokio::test]
async fn the_extension_must_match_the_destination() {
    let server = MockServer::start().await;
    serve(&server, "x", b"data").await;
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    for (name, dest) in [("evil.jar", ContentKind::Shader), ("evil.zip", ContentKind::Mod), ("evil.exe", ContentKind::ResourcePack), ("../x.zip", ContentKind::Shader)] {
        let p = plan(vec![file(&server, "x", name, b"data", dest)]);
        assert!(install_plan(&dl(), &dirs, INST, &p, None).await.is_err(), "{name} as {dest:?}");
    }
    assert!(server.received_requests().await.unwrap().is_empty(), "rejected before any download");
    let inst = dirs.instance(INST).unwrap();
    for d in ["mods", "shaderpacks", "resourcepacks"] {
        assert!(names(&inst.join(d)).is_empty(), "{d}");
    }
}

#[tokio::test]
async fn packs_can_be_listed_and_removed() {
    let server = MockServer::start().await;
    serve(&server, "s", b"shader").await;
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    install_plan(&dl(), &dirs, INST, &plan(vec![file(&server, "s", "S.zip", b"shader", ContentKind::Shader)]), None).await.unwrap();
    // A pack dropped in by hand shows up without a project.
    std::fs::write(dirs.instance(INST).unwrap().join("shaderpacks/hand.zip"), b"1234").unwrap();
    std::fs::write(dirs.instance(INST).unwrap().join("shaderpacks/readme.txt"), b"x").unwrap();
    let list = list_content(&dirs, INST, ContentKind::Shader).unwrap();
    assert_eq!(list.iter().map(|m| m.filename.as_str()).collect::<Vec<_>>(), vec!["hand.zip", "S.zip"], "sorted case-insensitively; readme.txt is ignored");
    assert!(remove_content(&dirs, INST, ContentKind::Shader, "../mods/a.jar").is_err());
    remove_content(&dirs, INST, ContentKind::Shader, "S.zip").unwrap();
    assert_eq!(names(&dirs.instance(INST).unwrap().join("shaderpacks")), vec!["hand.zip", "readme.txt"]);
    assert!(list_content(&dirs, INST, ContentKind::Shader).unwrap().iter().all(|m| m.project_id.is_none()));
    assert!(remove_content(&dirs, INST, ContentKind::Shader, "S.zip").is_err(), "already gone");
}

#[tokio::test]
async fn search_uses_project_type_facets_for_shaders_and_packs() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .and(query_param("facets", r#"[["project_type:shader"],["versions:1.21.1"]]"#))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"hits":[],"total_hits":0}"#))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .and(query_param("facets", r#"[["project_type:resourcepack"],["versions:1.21.1"]]"#))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"hits":[],"total_hits":0}"#))
        .expect(1)
        .mount(&server)
        .await;
    let c = ModrinthClient::new(server.uri());
    c.search(&content_target("1.21.1", LoaderKind::Fabric, ContentKind::Shader), "", 0).await.unwrap();
    c.search(&content_target("1.21.1", LoaderKind::Forge, ContentKind::ResourcePack), "x", 0).await.unwrap();
    server.verify().await;
}
