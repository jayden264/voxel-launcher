//! The one-click diagnostics bundle: what goes in, what is left out.

use launcher_core::config::{Dirs, InstancesFile, Settings};
use launcher_core::diagnostics::export;
use std::io::Read;
use std::path::Path;

const INST: &str = "inst";

fn put(path: &Path, body: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

fn read_zip(path: &Path) -> std::collections::BTreeMap<String, String> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut out = std::collections::BTreeMap::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        let mut s = String::new();
        f.read_to_string(&mut s).unwrap();
        out.insert(f.name().unwrap().to_string(), s);
    }
    out
}

fn setup() -> (tempfile::TempDir, Dirs, Settings, InstancesFile) {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path().join("data"));
    let mut settings = Settings::default();
    settings.java_path = Some(r"C:\Users\alice\java\bin\java.exe".into());
    let mut instances = InstancesFile::default();
    instances.add(INST, "1.21.1");
    // `add` assigns its own id; point the files at it.
    let id = instances.instances[0].id.clone();
    let game = dirs.instance(&id).unwrap();
    put(&game.join("logs/latest.log"), b"[12:00:00] [main/INFO]: loading from C:\\Users\\alice\\AppData\\Roaming\\x\n[12:00:01] boom\n");
    put(&game.join("launcher-stdio.log"), b"stdio line\n");
    put(&game.join("mods/sodium.jar"), b"not really a jar");
    put(&game.join("mods/off.jar.disabled"), b"x");
    (tmp, dirs, settings, instances)
}

#[test]
fn the_bundle_holds_system_settings_instances_logs_and_the_mod_list() {
    let (tmp, dirs, settings, instances) = setup();
    let id = instances.instances[0].id.clone();
    let out = tmp.path().join("out");

    let path = export(&dirs, &settings, &instances, Some(&id), "Voxel 0.1.0\nWindows 11", &[], &out).unwrap();

    assert!(path.is_file() && path.starts_with(&out));
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with("voxel-diagnostics-") && name.ends_with(".zip"), "{name}");
    let files = read_zip(&path);
    assert!(files["README.txt"].contains("没有包含"), "the README says what is left out");
    assert!(files["system.txt"].contains("Windows 11"));
    assert!(files.contains_key("settings.json") && files.contains_key("instances.json"));
    assert!(files["instance/latest.log"].contains("boom"));
    assert_eq!(files["instance/launcher-stdio.log"], "stdio line\n");
    assert!(files["instance/mods.txt"].contains("sodium.jar") && files["instance/mods.txt"].contains("已禁用"));
}

#[test]
fn the_users_name_is_removed_from_every_text_file() {
    let (tmp, dirs, settings, instances) = setup();
    let id = instances.instances[0].id.clone();

    let path = export(&dirs, &settings, &instances, Some(&id), "home: C:\\Users\\alice", &["alice".to_string()], &tmp.path().join("out")).unwrap();

    for (name, text) in read_zip(&path) {
        assert!(!text.to_lowercase().contains("alice"), "{name} still names the user:\n{text}");
    }
    assert!(read_zip(&path)["instance/latest.log"].contains(r"C:\Users\<user>\AppData"));
}

#[test]
fn accounts_servers_and_tokens_never_leave_the_machine() {
    let (tmp, dirs, mut settings, instances) = setup();
    settings.accounts.push(launcher_core::account::OfflineAccount::new("SecretSteve").unwrap());
    settings.servers.push(launcher_core::servers::ServerEntry { id: "s".into(), name: "MyPrivateServer".into(), host: "10.1.2.3".into(), port: 25565 });

    let path = export(&dirs, &settings, &instances, None, "sys", &[], &tmp.path().join("out")).unwrap();

    let all: String = read_zip(&path).values().cloned().collect();
    for secret in ["SecretSteve", "MyPrivateServer", "10.1.2.3"] {
        assert!(!all.contains(secret), "{secret} leaked");
    }
    assert!(read_zip(&path)["settings.json"].contains("\"accounts\": 1"), "only the count is kept");
}

#[test]
fn huge_logs_are_cut_to_their_end_and_only_recent_crash_reports_are_kept() {
    let (tmp, dirs, settings, instances) = setup();
    let id = instances.instances[0].id.clone();
    let game = dirs.instance(&id).unwrap();
    let mut big = vec![b'x'; 5 * 1024 * 1024];
    big.extend_from_slice(b"\nTHE-END-MARKER\n");
    put(&game.join("logs/latest.log"), &big);
    for i in 0..5 {
        let p = game.join(format!("crash-reports/crash-{i}.txt"));
        put(&p, format!("report {i}").as_bytes());
        let f = std::fs::File::options().write(true).open(&p).unwrap();
        f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(1000 - i * 100)).unwrap();
    }

    let path = export(&dirs, &settings, &instances, Some(&id), "sys", &[], &tmp.path().join("out")).unwrap();
    let files = read_zip(&path);

    let log = &files["instance/latest.log"];
    assert!(log.len() < 3 * 1024 * 1024 && log.contains("THE-END-MARKER") && log.contains("已截断"));
    let crashes: Vec<&String> = files.keys().filter(|k| k.starts_with("instance/crash-reports/")).collect();
    assert_eq!(crashes.len(), 3);
    assert!(files.contains_key("instance/crash-reports/crash-4.txt") && !files.contains_key("instance/crash-reports/crash-0.txt"));
}

#[test]
fn an_instance_with_nothing_on_disk_still_gives_a_bundle() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path().join("data"));
    let path = export(&dirs, &Settings::default(), &InstancesFile::default(), Some("ghost"), "sys", &[], &tmp.path().join("out")).unwrap();
    assert!(read_zip(&path).contains_key("system.txt"));
}
