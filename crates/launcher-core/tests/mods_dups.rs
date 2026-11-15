//! Spotting the same mod installed twice, from the ids inside the jars.

use launcher_core::config::Dirs;
use launcher_core::mods::{disable_duplicates, list_installed, mod_id_of_jar};
use std::io::Write;
use std::path::Path;

const INST: &str = "inst";

fn jar(path: &Path, entries: &[(&str, &str)]) {
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for (name, body) in entries {
        zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
}

fn fabric(path: &Path, id: &str) {
    jar(path, &[("fabric.mod.json", &format!(r#"{{"schemaVersion":1,"id":"{id}","version":"1"}}"#))]);
}

#[test]
fn reads_the_mod_id_for_each_loader_format() {
    let tmp = tempfile::tempdir().unwrap();
    let p = |n: &str| tmp.path().join(n);
    fabric(&p("a.jar"), "sodium");
    jar(&p("q.jar"), &[("quilt.mod.json", r#"{"quilt_loader":{"id":"lithium","version":"1"}}"#)]);
    jar(&p("f.jar"), &[("META-INF/mods.toml", "modLoader=\"javafml\"\n[[mods]]\nmodId = \"jei\"\nversion=\"1\"\n")]);
    jar(&p("n.jar"), &[("META-INF/neoforge.mods.toml", "[[mods]]\nmodId='create'\n")]);
    jar(&p("none.jar"), &[("readme.txt", "hi")]);
    std::fs::write(p("broken.jar"), b"not a zip").unwrap();

    assert_eq!(mod_id_of_jar(&p("a.jar")).as_deref(), Some("sodium"));
    assert_eq!(mod_id_of_jar(&p("q.jar")).as_deref(), Some("lithium"));
    assert_eq!(mod_id_of_jar(&p("f.jar")).as_deref(), Some("jei"));
    assert_eq!(mod_id_of_jar(&p("n.jar")).as_deref(), Some("create"));
    assert_eq!(mod_id_of_jar(&p("none.jar")), None);
    assert_eq!(mod_id_of_jar(&p("broken.jar")), None);
    assert_eq!(mod_id_of_jar(&p("missing.jar")), None);
}

#[test]
fn two_enabled_copies_are_flagged_and_a_disabled_copy_is_not() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    let mods = dirs.mods_dir(INST).unwrap();
    std::fs::create_dir_all(&mods).unwrap();
    fabric(&mods.join("sodium-0.5.jar"), "sodium");
    fabric(&mods.join("sodium-0.6.jar"), "sodium");
    fabric(&mods.join("lithium.jar"), "lithium");
    fabric(&mods.join("iris-1.jar"), "iris");
    fabric(&mods.join("iris-0.jar.disabled"), "iris");

    let list = list_installed(&dirs, INST).unwrap();
    let dup = |n: &str| list.iter().find(|m| m.filename == n).unwrap().duplicate;
    assert!(dup("sodium-0.5.jar") && dup("sodium-0.6.jar"));
    assert!(!dup("lithium.jar"));
    assert!(!dup("iris-1.jar") && !dup("iris-0.jar"), "a disabled copy cannot clash");
    assert_eq!(list.iter().find(|m| m.filename == "lithium.jar").unwrap().mod_id.as_deref(), Some("lithium"));
}

#[test]
fn disabling_duplicates_keeps_the_newest_file_and_is_reversible() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    let mods = dirs.mods_dir(INST).unwrap();
    std::fs::create_dir_all(&mods).unwrap();
    fabric(&mods.join("old.jar"), "sodium");
    fabric(&mods.join("new.jar"), "sodium");
    fabric(&mods.join("other.jar"), "lithium");
    let old = std::fs::File::options().write(true).open(mods.join("old.jar")).unwrap();
    old.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600)).unwrap();
    drop(old);

    let disabled = disable_duplicates(&dirs, INST).unwrap();

    assert_eq!(disabled, vec!["old.jar".to_string()]);
    assert!(mods.join("new.jar").is_file() && mods.join("other.jar").is_file());
    assert!(!mods.join("old.jar").exists() && mods.join("old.jar.disabled").is_file(), "renamed, not deleted");
    assert!(list_installed(&dirs, INST).unwrap().iter().all(|m| !m.duplicate));
    assert!(disable_duplicates(&dirs, INST).unwrap().is_empty());
}
