//! Copying an instance, and exporting one as a `.mrpack` others can install.

use launcher_core::config::{Dirs, Instance, InstancesFile, LoaderRef};
use launcher_core::download::sha1_bytes;
use launcher_core::loader::LoaderKind;
use launcher_core::modpack::{pack_target, read_index};
use launcher_core::mods::ModrinthClient;
use launcher_core::share::{clone_instance, export_mrpack, ExportOptions};
use serde_json::json;
use std::io::Read;
use std::path::Path;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn put(path: &Path, body: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

fn fabric_instance(instances: &mut InstancesFile) -> Instance {
    instances.add_with_loader("Original", "1.21.1", Some(LoaderRef { kind: LoaderKind::Fabric, version: "0.16.5".into() }));
    let i = instances.instances.last_mut().unwrap();
    i.memory_mb = Some(6144);
    i.jvm_args = "-Dfoo=bar".into();
    i.java_path = Some("C:/java/bin/java.exe".into());
    i.clone()
}

// -------------------------------------------------------------------- clone

#[test]
fn a_clone_copies_files_and_settings_but_not_logs_or_saves_unless_asked() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    let mut instances = InstancesFile::default();
    let orig = fabric_instance(&mut instances);
    let game = dirs.instance(&orig.id).unwrap();
    put(&game.join("mods/sodium.jar"), b"jar");
    put(&game.join("config/x.toml"), b"a=1");
    put(&game.join("options.txt"), b"fov:70");
    put(&game.join("saves/World/level.dat"), b"w");
    put(&game.join("logs/latest.log"), b"log");
    put(&game.join("crash-reports/c.txt"), b"crash");

    let copy = clone_instance(&dirs, &mut instances, &orig.id, "Original 副本", false).unwrap();

    assert_ne!(copy.id, orig.id);
    assert_eq!(copy.name, "Original 副本");
    assert_eq!((copy.version_id.as_str(), copy.mc_version.as_deref()), (orig.version_id.as_str(), Some("1.21.1")));
    assert_eq!((copy.memory_mb, copy.jvm_args.as_str(), copy.java_path.as_deref()), (Some(6144), "-Dfoo=bar", Some("C:/java/bin/java.exe")));
    assert_eq!(copy.loader.as_ref().map(|l| l.kind), Some(LoaderKind::Fabric));
    assert!(instances.find(&copy.id).is_some());
    let new = dirs.instance(&copy.id).unwrap();
    assert_eq!(std::fs::read(new.join("mods/sodium.jar")).unwrap(), b"jar");
    assert_eq!(std::fs::read(new.join("config/x.toml")).unwrap(), b"a=1");
    assert_eq!(std::fs::read(new.join("options.txt")).unwrap(), b"fov:70");
    assert!(!new.join("saves").exists() && !new.join("logs").exists() && !new.join("crash-reports").exists());
    assert!(game.join("saves/World/level.dat").is_file(), "the original is untouched");
}

#[test]
fn a_clone_can_bring_the_worlds_along() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    let mut instances = InstancesFile::default();
    let orig = fabric_instance(&mut instances);
    put(&dirs.instance(&orig.id).unwrap().join("saves/World/level.dat"), b"w");

    let copy = clone_instance(&dirs, &mut instances, &orig.id, "With worlds", true).unwrap();

    assert_eq!(std::fs::read(dirs.instance(&copy.id).unwrap().join("saves/World/level.dat")).unwrap(), b"w");
}

#[test]
fn cloning_an_unknown_or_unplayed_instance_behaves() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    let mut instances = InstancesFile::default();
    assert!(clone_instance(&dirs, &mut instances, "ghost", "x", false).is_err());
    assert!(instances.instances.is_empty());

    let orig = fabric_instance(&mut instances); // no folder on disk yet
    let copy = clone_instance(&dirs, &mut instances, &orig.id, "Empty copy", false).unwrap();
    assert_eq!(instances.instances.len(), 2);
    assert_ne!(copy.id, orig.id);
}

// -------------------------------------------------------------------- export

const SODIUM: &[u8] = b"sodium-bytes";
const MYSTERY: &[u8] = b"hand-made-mod";

fn version_for(hash: &str, url: &str) -> serde_json::Value {
    json!({
        "id": "v1", "project_id": "SOD", "version_number": "1", "version_type": "release", "dependencies": [],
        "files": [{ "hashes": { "sha1": hash }, "url": url, "filename": "sodium.jar", "primary": true, "size": SODIUM.len() }]
    })
}

async fn modrinth(known: serde_json::Value) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST")).and(path("/version_files")).respond_with(ResponseTemplate::new(200).set_body_json(known)).mount(&server).await;
    server
}

fn read_zip(path: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    (0..zip.len())
        .map(|i| {
            let mut f = zip.by_index(i).unwrap();
            let mut b = Vec::new();
            f.read_to_end(&mut b).unwrap();
            (f.name().unwrap().to_string(), b)
        })
        .collect()
}

fn instance_with_files() -> (tempfile::TempDir, Dirs, Instance) {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    let mut instances = InstancesFile::default();
    let inst = fabric_instance(&mut instances);
    let game = dirs.instance(&inst.id).unwrap();
    put(&game.join("mods/sodium.jar"), SODIUM);
    put(&game.join("mods/mystery.jar"), MYSTERY);
    put(&game.join("mods/off.jar.disabled"), b"disabled");
    put(&game.join("config/sodium.json"), b"{}");
    put(&game.join("options.txt"), b"fov:70");
    put(&game.join("saves/World/level.dat"), b"w");
    put(&game.join("logs/latest.log"), b"log");
    (tmp, dirs, inst)
}

fn opts() -> ExportOptions {
    ExportOptions { name: "我的整合包".into(), version: "1.0.0".into(), summary: None, include_saves: false, include_options: false }
}

#[tokio::test]
async fn known_mods_are_linked_unknown_ones_are_bundled_and_the_pack_installs_with_our_own_importer() {
    let (tmp, dirs, inst) = instance_with_files();
    let url = "https://cdn.modrinth.com/data/SOD/versions/v1/sodium.jar";
    let server = modrinth(json!({ sha1_bytes(SODIUM): version_for(&sha1_bytes(SODIUM), url) })).await;
    let out = tmp.path().join("out");

    let r = export_mrpack(&ModrinthClient::new(server.uri()), &dirs, &inst, &opts(), &out).await.unwrap();

    assert_eq!((r.linked, r.bundled), (1, 1));
    assert!(r.path.extension().is_some_and(|e| e == "mrpack") && r.path.starts_with(&out));
    let files = read_zip(&r.path);
    assert!(files.contains_key("overrides/mods/mystery.jar"), "a mod nobody hosts travels inside the pack");
    assert!(!files.contains_key("overrides/mods/sodium.jar"), "a hosted mod is only linked");
    assert!(!files.keys().any(|k| k.contains("off.jar")), "disabled mods are left out");
    assert!(files.contains_key("overrides/config/sodium.json"));
    for private in ["overrides/options.txt", "overrides/saves/World/level.dat", "overrides/logs/latest.log"] {
        assert!(!files.contains_key(private), "{private} must not be in the pack by default");
    }

    // The pack reads back through the same code that installs other people's packs.
    let idx = read_index(&r.path).unwrap();
    assert_eq!(idx.name, "我的整合包");
    let t = pack_target(&idx).unwrap();
    assert_eq!((t.mc.as_str(), t.loader), ("1.21.1", Some((LoaderKind::Fabric, "0.16.5".to_string()))));
    assert_eq!(idx.files.len(), 1);
    assert_eq!((idx.files[0].path.as_str(), idx.files[0].sha1.as_str()), ("mods/sodium.jar", sha1_bytes(SODIUM).as_str()));
    assert_eq!(idx.files[0].downloads, vec![url.to_string()]);
    let raw: serde_json::Value = serde_json::from_slice(&files["modrinth.index.json"]).unwrap();
    assert_eq!(raw["files"][0]["hashes"]["sha512"].as_str().unwrap().len(), 128, "the format requires sha512 too");
    assert_eq!(raw["files"][0]["fileSize"], SODIUM.len());
}

#[tokio::test]
async fn saves_and_settings_are_included_only_when_asked() {
    let (tmp, dirs, inst) = instance_with_files();
    let server = modrinth(json!({})).await;
    let mut o = opts();
    o.include_saves = true;
    o.include_options = true;

    let r = export_mrpack(&ModrinthClient::new(server.uri()), &dirs, &inst, &o, &tmp.path().join("out")).await.unwrap();

    let files = read_zip(&r.path);
    assert!(files.contains_key("overrides/options.txt") && files.contains_key("overrides/saves/World/level.dat"));
    assert!(!files.keys().any(|k| k.contains("session.lock")));
}

#[tokio::test]
async fn when_modrinth_cannot_be_reached_everything_is_bundled_and_the_pack_still_works() {
    let (tmp, dirs, inst) = instance_with_files();
    let server = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(500)).mount(&server).await;

    let r = export_mrpack(&ModrinthClient::new(server.uri()), &dirs, &inst, &opts(), &tmp.path().join("out")).await.unwrap();

    assert_eq!((r.linked, r.bundled), (0, 2));
    let files = read_zip(&r.path);
    assert!(files.contains_key("overrides/mods/sodium.jar") && files.contains_key("overrides/mods/mystery.jar"));
    assert!(read_index(&r.path).unwrap().files.is_empty());
}

#[tokio::test]
async fn a_download_address_outside_modrinths_cdn_is_never_written_into_the_pack() {
    let (tmp, dirs, inst) = instance_with_files();
    let server = modrinth(json!({ sha1_bytes(SODIUM): version_for(&sha1_bytes(SODIUM), "https://evil.example.com/sodium.jar") })).await;

    let r = export_mrpack(&ModrinthClient::new(server.uri()), &dirs, &inst, &opts(), &tmp.path().join("out")).await.unwrap();

    assert_eq!(r.linked, 0);
    assert!(read_zip(&r.path).contains_key("overrides/mods/sodium.jar"));
}

#[tokio::test]
async fn a_vanilla_instance_exports_with_only_the_game_version_and_names_are_made_file_safe() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    let mut instances = InstancesFile::default();
    let inst = instances.add("Vanilla", "1.20.4").clone();
    put(&dirs.instance(&inst.id).unwrap().join("config/a.cfg"), b"x");
    let server = modrinth(json!({})).await;
    let mut o = opts();
    o.name = "a/b:c*?\"<>|名字".into();
    let out = tmp.path().join("out");

    let first = export_mrpack(&ModrinthClient::new(server.uri()), &dirs, &inst, &o, &out).await.unwrap();
    let second = export_mrpack(&ModrinthClient::new(server.uri()), &dirs, &inst, &o, &out).await.unwrap();

    let name = first.path.file_name().unwrap().to_string_lossy().into_owned();
    assert!(!name.contains(['/', ':', '*', '?', '"', '<', '>', '|']) && name.ends_with(".mrpack"), "{name}");
    assert_ne!(first.path, second.path, "an existing file is never overwritten");
    let idx = read_index(&first.path).unwrap();
    assert_eq!(idx.dependencies.len(), 1);
    assert_eq!(pack_target(&idx).unwrap().loader, None);
}
