//! Problems that can be seen before the game starts: duplicates and missing prerequisites.

use launcher_core::config::Dirs;
use launcher_core::precheck::precheck;
use std::io::Write;
use std::path::Path;

const INST: &str = "inst";

fn zip_bytes(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut buf);
    for (name, body) in entries {
        zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(body).unwrap();
    }
    zip.finish().unwrap();
    buf.into_inner()
}

fn fabric_json(id: &str, extra: &str) -> Vec<u8> {
    format!(r#"{{"schemaVersion":1,"id":"{id}","name":"Name of {id}","version":"1"{extra}}}"#).into_bytes()
}

fn put(dir: &Path, file: &str, entries: &[(&str, Vec<u8>)]) {
    std::fs::write(dir.join(file), zip_bytes(entries)).unwrap();
}

fn setup() -> (tempfile::TempDir, Dirs, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    let mods = dirs.mods_dir(INST).unwrap();
    std::fs::create_dir_all(&mods).unwrap();
    (tmp, dirs, mods)
}

#[test]
fn a_missing_required_prerequisite_is_reported_with_who_needs_it() {
    let (_t, dirs, mods) = setup();
    put(&mods, "a.jar", &[("fabric.mod.json", fabric_json("a", r#","depends":{"fabric-api":"*","minecraft":"1.21","fabricloader":">=0.15","java":">=21","mixinextras":">=0.5"}"#))]);
    put(&mods, "b.jar", &[("fabric.mod.json", fabric_json("b", r#","depends":{"fabric-api":"*"}"#))]);

    let problems = precheck(&dirs, INST).unwrap();

    assert_eq!(problems.len(), 1, "{problems:?}");
    let p = &problems[0];
    assert_eq!(p.code, "missing-dependency");
    assert!(p.title.contains("fabric-api"), "{}", p.title);
    assert!(p.detail.contains("Name of a") && p.detail.contains("Name of b"), "{}", p.detail);
}

#[test]
fn prerequisites_can_be_provided_by_id_alias_nested_jar_or_another_mod() {
    let (_t, dirs, mods) = setup();
    let nested = zip_bytes(&[("fabric.mod.json", fabric_json("fabric-api-base", ""))]);
    put(
        &mods,
        "fabric-api.jar",
        &[
            ("fabric.mod.json", fabric_json("fabric-api", r#","provides":["fabric"],"jars":[{"file":"META-INF/jars/base.jar"}]"#)),
            ("META-INF/jars/base.jar", nested),
        ],
    );
    put(&mods, "lib.jar", &[("fabric.mod.json", fabric_json("lib", ""))]);
    put(
        &mods,
        "user.jar",
        &[("fabric.mod.json", fabric_json("user", r#","depends":{"fabric":"*","fabric-api-base":"*","lib":"*","fabric-api":"*"}"#))],
    );

    assert!(precheck(&dirs, INST).unwrap().is_empty());
}

#[test]
fn a_disabled_mod_does_not_satisfy_a_prerequisite() {
    let (_t, dirs, mods) = setup();
    put(&mods, "lib.jar.disabled", &[("fabric.mod.json", fabric_json("lib", ""))]);
    put(&mods, "user.jar", &[("fabric.mod.json", fabric_json("user", r#","depends":{"lib":"*"}"#))]);
    put(&mods, "off.jar.disabled", &[("fabric.mod.json", fabric_json("off", r#","depends":{"nothing-here":"*"}"#))]);

    let problems = precheck(&dirs, INST).unwrap();

    assert_eq!(problems.len(), 1, "a disabled mod's own needs do not matter: {problems:?}");
    assert!(problems[0].title.contains("lib"));
}

#[test]
fn quilt_mods_and_unreadable_files_are_handled() {
    let (_t, dirs, mods) = setup();
    let quilt = br#"{"quilt_loader":{"id":"q","version":"1","provides":["qalias"],"depends":["quilted_fabric_api",{"id":"optional-thing","optional":true},{"id":"minecraft","versions":">=1.21"}]}}"#.to_vec();
    put(&mods, "q.jar", &[("quilt.mod.json", quilt)]);
    put(&mods, "other.jar", &[("fabric.mod.json", fabric_json("other", r#","depends":{"qalias":"*"}"#))]);
    std::fs::write(mods.join("broken.jar"), b"not a zip").unwrap();
    put(&mods, "forge.jar", &[("META-INF/mods.toml", b"[[mods]]\nmodId=\"forgey\"\n".to_vec())]);

    let problems = precheck(&dirs, INST).unwrap();

    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].title.contains("quilted_fabric_api"));
}

#[test]
fn duplicates_come_first_and_offer_a_fix() {
    let (_t, dirs, mods) = setup();
    put(&mods, "x-1.jar", &[("fabric.mod.json", fabric_json("x", ""))]);
    put(&mods, "x-2.jar", &[("fabric.mod.json", fabric_json("x", r#","depends":{"gone":"*"}"#))]);

    let problems = precheck(&dirs, INST).unwrap();

    assert_eq!(problems[0].code, "duplicate-mods");
    assert_eq!(problems[0].fix, Some("disable-duplicates"));
    assert!(problems.iter().any(|p| p.code == "missing-dependency"));
}

#[test]
fn an_instance_without_a_mods_folder_has_no_problems() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(precheck(&Dirs::new(tmp.path()), "nothing").unwrap().is_empty());
}
