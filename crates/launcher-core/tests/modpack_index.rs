//! Reading `modrinth.index.json` of a .mrpack safely.

use launcher_core::loader::LoaderKind;
use launcher_core::modpack::{pack_target, parse_index};
use serde_json::{json, Value};

const REAL: &str = include_str!("fixtures/fabulously-optimized-6.5.0.modrinth.index.json");

fn real() -> Value {
    serde_json::from_str(REAL).unwrap()
}

fn bytes(v: &Value) -> Vec<u8> {
    serde_json::to_vec(v).unwrap()
}

/// One minimal file entry.
fn entry(path: &str, url: &str) -> Value {
    json!({"path": path, "hashes": {"sha1": "a".repeat(40), "sha512": "b"}, "downloads": [url], "fileSize": 5,
           "env": {"client": "required", "server": "required"}})
}

fn index(files: Vec<Value>) -> Value {
    json!({"formatVersion": 1, "game": "minecraft", "versionId": "1.0", "name": "Test Pack",
           "dependencies": {"minecraft": "1.21.1", "fabric-loader": "0.16.9"}, "files": files})
}

#[test]
fn parses_the_real_fabulously_optimized_index() {
    let idx = parse_index(&bytes(&real())).unwrap();
    assert_eq!((idx.name.as_str(), idx.version_id.as_str()), ("Fabulously Optimized", "6.5.0"));
    assert_eq!(idx.files.len(), 50);
    assert!(idx.files.iter().all(|f| f.client_required && f.sha1.len() == 40 && !f.downloads.is_empty()));
    let first = &idx.files[0];
    assert_eq!(first.path, "mods/BetterGrassify-1.8.6+fabric.1.21.1.jar");
    assert_eq!(first.sha1, "f006203d16fe3d516dbc822b5616427f834725db");
    assert_eq!(first.size, 87323);

    let t = pack_target(&idx).unwrap();
    assert_eq!(t.mc, "1.21.1");
    assert_eq!(t.loader, Some((LoaderKind::Fabric, "0.19.3".to_string())));
}

#[test]
fn rejects_paths_that_escape_the_instance() {
    for bad in ["../evil.jar", "mods/../../evil.jar", "/abs/evil.jar", "C:/evil.jar", "mods/a:b.jar", ".voxel/mods.json", "", "mods\\..\\..\\x"] {
        let idx = index(vec![entry(bad, "https://cdn.modrinth.com/data/x/versions/y/a.jar")]);
        assert!(parse_index(&bytes(&idx)).is_err(), "{bad:?} must be rejected");
    }
    assert!(parse_index(&bytes(&index(vec![entry("mods/sub/dir/a.jar", "https://cdn.modrinth.com/data/x/a.jar")]))).is_ok());
}

#[test]
fn only_known_hosts_over_https_are_accepted() {
    for ok in [
        "https://cdn.modrinth.com/data/x/a.jar",
        "https://github.com/o/r/releases/download/v1/a.jar",
        "https://raw.githubusercontent.com/o/r/main/a.jar",
        "https://gitlab.com/o/r/-/raw/main/a.jar",
    ] {
        assert!(parse_index(&bytes(&index(vec![entry("mods/a.jar", ok)]))).is_ok(), "{ok}");
    }
    for bad in [
        "http://cdn.modrinth.com/data/x/a.jar",
        "https://evil.example.com/a.jar",
        "https://cdn.modrinth.com.evil.com/a.jar",
        "https://evil.com/cdn.modrinth.com/a.jar",
        "https://cdn.modrinth.com@evil.com/a.jar",
        "file:///C:/a.jar",
        "ftp://github.com/a.jar",
        "not a url",
    ] {
        assert!(parse_index(&bytes(&index(vec![entry("mods/a.jar", bad)]))).is_err(), "{bad} must be rejected");
    }
}

#[test]
fn a_file_without_a_download_or_a_sha1_is_rejected() {
    let mut no_dl = entry("mods/a.jar", "https://cdn.modrinth.com/a.jar");
    no_dl["downloads"] = json!([]);
    assert!(parse_index(&bytes(&index(vec![no_dl]))).is_err());
    let mut no_sha = entry("mods/a.jar", "https://cdn.modrinth.com/a.jar");
    no_sha["hashes"] = json!({"sha512": "b"});
    assert!(parse_index(&bytes(&index(vec![no_sha]))).is_err());
    let mut bad_sha = entry("mods/a.jar", "https://cdn.modrinth.com/a.jar");
    bad_sha["hashes"]["sha1"] = json!("not-hex");
    assert!(parse_index(&bytes(&index(vec![bad_sha]))).is_err());
}

#[test]
fn files_marked_unsupported_on_the_client_are_flagged() {
    let mut server_only = entry("mods/server.jar", "https://cdn.modrinth.com/a.jar");
    server_only["env"] = json!({"client": "unsupported", "server": "required"});
    let mut optional = entry("mods/opt.jar", "https://cdn.modrinth.com/b.jar");
    optional["env"] = json!({"client": "optional", "server": "optional"});
    let mut no_env = entry("mods/plain.jar", "https://cdn.modrinth.com/c.jar");
    no_env.as_object_mut().unwrap().remove("env");
    let idx = parse_index(&bytes(&index(vec![server_only, optional, no_env]))).unwrap();
    let flags: Vec<_> = idx.files.iter().map(|f| f.client_required).collect();
    assert_eq!(flags, vec![false, true, true], "optional files are installed, only `unsupported` is skipped");
}

#[test]
fn unknown_formats_and_other_games_are_rejected() {
    let mut v = index(vec![]);
    v["formatVersion"] = json!(2);
    assert!(parse_index(&bytes(&v)).is_err());
    let mut v = index(vec![]);
    v["game"] = json!("hytale");
    assert!(parse_index(&bytes(&v)).is_err());
    assert!(parse_index(b"{ not json").is_err());
}

#[test]
fn pack_target_maps_every_loader_and_plain_packs() {
    let t = |deps: Value| {
        let mut v = index(vec![]);
        v["dependencies"] = deps;
        pack_target(&parse_index(&bytes(&v)).unwrap())
    };
    assert_eq!(t(json!({"minecraft": "1.21.1", "quilt-loader": "0.27.1"})).unwrap().loader, Some((LoaderKind::Quilt, "0.27.1".into())));
    assert_eq!(t(json!({"minecraft": "1.20.1", "forge": "47.2.0"})).unwrap().loader, Some((LoaderKind::Forge, "47.2.0".into())));
    assert_eq!(t(json!({"minecraft": "1.21.1", "neoforge": "21.1.172"})).unwrap().loader, Some((LoaderKind::NeoForge, "21.1.172".into())));
    let plain = t(json!({"minecraft": "1.21.1"})).unwrap();
    assert_eq!((plain.mc.as_str(), plain.loader), ("1.21.1", None));
    assert!(t(json!({"fabric-loader": "0.16.9"})).is_err(), "no minecraft version");
}

#[test]
fn loader_and_minecraft_versions_must_be_plain_strings() {
    let mut v = index(vec![]);
    v["dependencies"] = json!({"minecraft": "../1.21.1", "fabric-loader": "0.16.9"});
    assert!(pack_target(&parse_index(&bytes(&v)).unwrap()).is_err());
    v["dependencies"] = json!({"minecraft": "1.21.1", "fabric-loader": "0.16.9/../x"});
    assert!(pack_target(&parse_index(&bytes(&v)).unwrap()).is_err());
}
