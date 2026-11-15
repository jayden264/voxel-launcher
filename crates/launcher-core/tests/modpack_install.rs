//! Modpack search, versions and file installation (synthetic .mrpack archives).

use launcher_core::config::Dirs;
use launcher_core::download::{sha1_bytes, Downloader};
use launcher_core::mirror::Source;
use launcher_core::modpack::{install_pack_files, MrFile, MrIndex};
use launcher_core::mods::ModrinthClient;
use launcher_core::Error;
use serde_json::json;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const INST: &str = "pack";

fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in entries {
        w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap().into_inner()
}

struct Env {
    server: MockServer,
    tmp: tempfile::TempDir,
    dirs: Dirs,
}

impl Env {
    async fn new() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        Env { server: MockServer::start().await, dirs: Dirs::new(tmp.path()), tmp }
    }

    fn dl(&self) -> Downloader {
        let base = self.server.uri();
        Downloader::new(Source::Official)
            .with_retry(0, Duration::from_millis(1))
            .with_resolver(Arc::new(move |u| vec![u.replace("https://cdn.modrinth.com", &base)]))
    }

    async fn serve(&self, p: &str, data: &[u8]) {
        Mock::given(method("GET"))
            .and(path(p.to_string()))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(data.to_vec()))
            .mount(&self.server)
            .await;
    }

    fn write_pack(&self, entries: &[(&str, &[u8])]) -> PathBuf {
        let p = self.tmp.path().join("pack.mrpack");
        std::fs::write(&p, zip_bytes(entries)).unwrap();
        p
    }

    fn instance(&self) -> PathBuf {
        self.dirs.instance(INST).unwrap()
    }
}

fn file(path: &str, urls: &[&str], data: &[u8], client_required: bool) -> MrFile {
    MrFile {
        path: path.into(),
        sha1: sha1_bytes(data),
        size: data.len() as u64,
        downloads: urls.iter().map(|u| u.to_string()).collect(),
        client_required,
    }
}

fn index(files: Vec<MrFile>) -> MrIndex {
    MrIndex { name: "Pack".into(), version_id: "1".into(), dependencies: BTreeMap::new(), files }
}

async fn install(env: &Env, pack: &Path, idx: &MrIndex) -> launcher_core::Result<()> {
    install_pack_files(&env.dl(), &env.dirs, INST, pack, idx, None).await
}

#[tokio::test]
async fn installs_files_and_overrides_into_the_instance() {
    let env = Env::new().await;
    env.serve("/data/a.jar", b"mod-a").await;
    env.serve("/data/pack.zip", b"resources").await;
    let idx = index(vec![
        file("mods/a.jar", &["https://cdn.modrinth.com/data/a.jar"], b"mod-a", true),
        file("resourcepacks/pack.zip", &["https://cdn.modrinth.com/data/pack.zip"], b"resources", true),
    ]);
    let pack = env.write_pack(&[
        ("modrinth.index.json", b"{}"),
        ("overrides/config/sodium.json", b"{\"x\":1}"),
        ("overrides/options.txt", b"fov:90"),
        ("overrides/", b""),
    ]);
    install(&env, &pack, &idx).await.unwrap();

    assert_eq!(std::fs::read(env.instance().join("mods/a.jar")).unwrap(), b"mod-a");
    assert_eq!(std::fs::read(env.instance().join("resourcepacks/pack.zip")).unwrap(), b"resources");
    assert_eq!(std::fs::read(env.instance().join("config/sodium.json")).unwrap(), b"{\"x\":1}");
    assert_eq!(std::fs::read(env.instance().join("options.txt")).unwrap(), b"fov:90");
    assert!(!env.instance().join("modrinth.index.json").exists(), "the index is not an override");
}

#[tokio::test]
async fn client_overrides_win_over_overrides() {
    let env = Env::new().await;
    let pack = env.write_pack(&[
        ("client-overrides/options.txt", b"client"),
        ("overrides/options.txt", b"common"),
        ("server-overrides/options.txt", b"server"),
    ]);
    install(&env, &pack, &index(vec![])).await.unwrap();
    assert_eq!(std::fs::read(env.instance().join("options.txt")).unwrap(), b"client");
}

#[tokio::test]
async fn files_unsupported_on_the_client_are_not_downloaded() {
    let env = Env::new().await;
    env.serve("/data/a.jar", b"mod-a").await;
    let idx = index(vec![
        file("mods/a.jar", &["https://cdn.modrinth.com/data/a.jar"], b"mod-a", true),
        file("mods/server-only.jar", &["https://cdn.modrinth.com/data/server.jar"], b"srv", false),
    ]);
    install(&env, &env.write_pack(&[("modrinth.index.json", b"{}")]), &idx).await.unwrap();
    assert!(env.instance().join("mods/a.jar").exists());
    assert!(!env.instance().join("mods/server-only.jar").exists());
    let asked: Vec<_> = env.server.received_requests().await.unwrap().iter().map(|r| r.url.path().to_string()).collect();
    assert_eq!(asked, vec!["/data/a.jar"]);
}

#[tokio::test]
async fn override_entries_cannot_escape_the_instance() {
    let env = Env::new().await;
    for evil in ["overrides/../../evil.txt", "overrides/../evil.txt", "overrides//abs.txt", "overrides/C:/evil.txt", "overrides/.voxel/mods.json", "client-overrides/../../evil.txt"] {
        let pack = env.write_pack(&[(evil, b"x")]);
        let r = install(&env, &pack, &index(vec![])).await;
        assert!(r.is_err(), "{evil} must be rejected");
    }
    assert!(!env.tmp.path().join("evil.txt").exists());
    assert!(!env.tmp.path().parent().unwrap().join("evil.txt").exists());
    assert!(!env.instance().join(".voxel").exists());
}

#[tokio::test]
async fn a_bad_checksum_fails_and_names_the_file() {
    let env = Env::new().await;
    env.serve("/data/a.jar", b"something else").await;
    let idx = index(vec![file("mods/a.jar", &["https://cdn.modrinth.com/data/a.jar"], b"mod-a", true)]);
    let err = install(&env, &env.write_pack(&[("modrinth.index.json", b"{}")]), &idx).await.unwrap_err().to_string();
    assert!(err.contains("a.jar"), "{err}");
    assert!(!env.instance().join("mods/a.jar").exists());
}

#[tokio::test]
async fn a_second_download_url_is_tried_when_the_first_fails() {
    let env = Env::new().await;
    Mock::given(method("GET")).and(path("/data/gone.jar")).respond_with(ResponseTemplate::new(404)).mount(&env.server).await;
    env.serve("/data/mirror.jar", b"mod-a").await;
    let idx = index(vec![file("mods/a.jar", &["https://cdn.modrinth.com/data/gone.jar", "https://cdn.modrinth.com/data/mirror.jar"], b"mod-a", true)]);
    install(&env, &env.write_pack(&[("modrinth.index.json", b"{}")]), &idx).await.unwrap();
    assert_eq!(std::fs::read(env.instance().join("mods/a.jar")).unwrap(), b"mod-a");
}

#[tokio::test]
async fn cancelling_stops_the_install() {
    let env = Env::new().await;
    env.serve("/data/a.jar", b"mod-a").await;
    let token = CancellationToken::new();
    token.cancel();
    let idx = index(vec![file("mods/a.jar", &["https://cdn.modrinth.com/data/a.jar"], b"mod-a", true)]);
    let pack = env.write_pack(&[("modrinth.index.json", b"{}")]);
    let err = install_pack_files(&env.dl().with_cancel(token), &env.dirs, INST, &pack, &idx, None).await.unwrap_err();
    assert!(matches!(err, Error::Cancelled), "{err}");
}

#[tokio::test]
async fn search_modpacks_uses_modpack_facets() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .and(query_param("facets", r#"[["project_type:modpack"]]"#))
        .and(query_param("query", "fabulously"))
        .and(query_param("offset", "20"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"hits":[{"project_id":"1KVo5zza","slug":"fabulously-optimized","title":"Fabulously Optimized","description":"d","icon_url":null,"downloads":5,"author":"a"}],"total_hits":21}"#,
        ))
        .expect(1)
        .mount(&server)
        .await;
    let r = ModrinthClient::new(server.uri()).search_modpacks("fabulously", 20).await.unwrap();
    assert_eq!((r.total, r.hits[0].slug.as_str()), (21, "fabulously-optimized"));
    server.verify().await;
}

#[tokio::test]
async fn pack_versions_keep_only_versions_with_an_mrpack_file() {
    let server = MockServer::start().await;
    let version = |id: &str, files: serde_json::Value| {
        json!({"id": id, "project_id": "p", "name": format!("Name {id}"), "version_number": format!("v{id}"), "version_type": "release",
               "game_versions": ["1.21.1"], "loaders": ["fabric"], "files": files, "dependencies": []})
    };
    let mrpack = |name: &str, primary: bool| {
        json!({"hashes": {"sha1": "a".repeat(40)}, "url": format!("https://cdn.modrinth.com/{name}"), "filename": name, "primary": primary, "size": 10})
    };
    Mock::given(method("GET"))
        .and(path("/project/pack/version"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            version("1", json!([mrpack("Pack-1.mrpack", true), mrpack("extra.zip", false)])),
            version("2", json!([mrpack("only.zip", true)])),
            version("3", json!([mrpack("other.zip", true), mrpack("Pack-3.MRPACK", false)])),
        ])))
        .mount(&server)
        .await;
    let v = ModrinthClient::new(server.uri()).pack_versions("pack").await.unwrap();
    assert_eq!(v.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), vec!["1", "3"]);
    assert_eq!(v[0].file.filename, "Pack-1.mrpack");
    assert_eq!(v[1].file.filename, "Pack-3.MRPACK", "a non-primary .mrpack is used when the primary file is something else");
    assert_eq!((v[0].version_number.as_str(), v[0].game_versions.clone(), v[0].loaders.clone()), ("v1", vec!["1.21.1".to_string()], vec!["fabric".to_string()]));
}

#[tokio::test]
async fn read_index_reads_and_validates_the_index_of_a_pack() {
    use launcher_core::modpack::read_index;
    let env = Env::new().await;
    let index = json!({"formatVersion": 1, "game": "minecraft", "versionId": "7", "name": "Pack", "dependencies": {"minecraft": "1.21.1"},
                       "files": [{"path": "mods/a.jar", "hashes": {"sha1": "a".repeat(40)}, "downloads": ["https://cdn.modrinth.com/a.jar"], "fileSize": 3}]});
    let pack = env.write_pack(&[("modrinth.index.json", index.to_string().as_bytes()), ("overrides/x.txt", b"x")]);
    let idx = read_index(&pack).unwrap();
    assert_eq!((idx.name.as_str(), idx.files.len()), ("Pack", 1));

    let no_index = env.write_pack(&[("overrides/x.txt", b"x")]);
    assert!(read_index(&no_index).is_err());
    assert!(read_index(&env.tmp.path().join("missing.mrpack")).is_err());
    let not_a_zip = env.tmp.path().join("bad.mrpack");
    std::fs::write(&not_a_zip, b"not a zip").unwrap();
    assert!(read_index(&not_a_zip).is_err());
}

// ------------------------------------------------------ interrupted installs

use launcher_core::modpack::{clear_pending, mark_pending, pending_pack, resume_pending};

fn real_pack(env: &Env, files: &[(&str, &[u8])]) -> PathBuf {
    let entries: Vec<serde_json::Value> = files
        .iter()
        .map(|(p, d)| {
            json!({"path": p, "hashes": {"sha1": sha1_bytes(d)}, "downloads": [format!("https://cdn.modrinth.com/data/{}", p.replace('/', "_"))],
                   "fileSize": d.len(), "env": {"client": "required", "server": "required"}})
        })
        .collect();
    let index = json!({"formatVersion": 1, "game": "minecraft", "versionId": "1", "name": "P", "dependencies": {"minecraft": "1.21.1"}, "files": entries});
    env.write_pack(&[("modrinth.index.json", index.to_string().as_bytes()), ("overrides/options.txt", b"fov:90")])
}

#[tokio::test]
async fn the_pending_marker_roundtrips() {
    let env = Env::new().await;
    assert_eq!(pending_pack(&env.dirs, INST).unwrap(), None);
    mark_pending(&env.dirs, INST, Path::new("C:/x/p.mrpack")).unwrap();
    assert_eq!(pending_pack(&env.dirs, INST).unwrap(), Some(PathBuf::from("C:/x/p.mrpack")));
    clear_pending(&env.dirs, INST).unwrap();
    assert_eq!(pending_pack(&env.dirs, INST).unwrap(), None);
    clear_pending(&env.dirs, INST).unwrap(); // clearing twice is fine
}

#[tokio::test]
async fn resuming_finishes_an_interrupted_install_and_clears_the_marker() {
    let env = Env::new().await;
    env.serve("/data/mods_a.jar", b"mod-a").await;
    env.serve("/data/mods_b.jar", b"mod-b").await;
    let pack = real_pack(&env, &[("mods/a.jar", b"mod-a"), ("mods/b.jar", b"mod-b")]);
    // State after a crash: b.jar complete, a.jar half-downloaded, overrides never unpacked.
    std::fs::create_dir_all(env.instance().join("mods")).unwrap();
    std::fs::write(env.instance().join("mods/b.jar"), b"mod-b").unwrap();
    std::fs::write(env.instance().join("mods/a.jar.part"), b"mod").unwrap();
    mark_pending(&env.dirs, INST, &pack).unwrap();

    assert!(resume_pending(&env.dl(), &env.dirs, INST, None).await.unwrap());
    assert_eq!(std::fs::read(env.instance().join("mods/a.jar")).unwrap(), b"mod-a");
    assert_eq!(std::fs::read(env.instance().join("options.txt")).unwrap(), b"fov:90");
    assert_eq!(pending_pack(&env.dirs, INST).unwrap(), None);
    assert!(!resume_pending(&env.dl(), &env.dirs, INST, None).await.unwrap(), "nothing left to do");
}

#[tokio::test]
async fn a_failed_resume_keeps_the_marker_so_the_next_launch_tries_again() {
    let env = Env::new().await;
    Mock::given(method("GET")).respond_with(ResponseTemplate::new(404)).mount(&env.server).await;
    let pack = real_pack(&env, &[("mods/a.jar", b"mod-a")]);
    mark_pending(&env.dirs, INST, &pack).unwrap();
    assert!(resume_pending(&env.dl(), &env.dirs, INST, None).await.is_err());
    assert_eq!(pending_pack(&env.dirs, INST).unwrap(), Some(pack));
}

#[tokio::test]
async fn a_missing_pack_file_is_an_error_and_keeps_the_marker() {
    let env = Env::new().await;
    let gone = env.tmp.path().join("gone.mrpack");
    mark_pending(&env.dirs, INST, &gone).unwrap();
    assert!(resume_pending(&env.dl(), &env.dirs, INST, None).await.is_err());
    assert_eq!(pending_pack(&env.dirs, INST).unwrap(), Some(gone));
}
