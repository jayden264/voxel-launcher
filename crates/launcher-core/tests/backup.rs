//! Backing an instance up, and putting it back.

use launcher_core::backup::{create, delete, list, restore, restore_world, Kind, Scope};
use launcher_core::config::Dirs;
use std::path::Path;
use std::time::Duration;

const INST: &str = "inst";

fn write(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

fn setup() -> (tempfile::TempDir, Dirs, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    let game = dirs.instance(INST).unwrap();
    write(&game.join("saves/Alpha/level.dat"), "alpha-1");
    write(&game.join("saves/Alpha/region/r.0.0.mca"), "chunks-1");
    write(&game.join("saves/Beta/level.dat"), "beta-1");
    write(&game.join("saves/Beta/session.lock"), "held by the game");
    write(&game.join("options.txt"), "fov:70");
    write(&game.join("config/mod.toml"), "a=1");
    write(&game.join("mods/sodium.jar"), "jar-1");
    (tmp, dirs, game)
}

fn pause() {
    std::thread::sleep(Duration::from_millis(5));
}

#[test]
fn a_worlds_backup_holds_saves_and_settings_but_not_mods() {
    let (_t, dirs, _game) = setup();

    let b = create(&dirs, INST, Scope::Worlds, Kind::Manual, "手动备份", None).unwrap().unwrap();

    assert_eq!((b.kind, b.scope), (Kind::Manual, Scope::Worlds));
    assert_eq!(b.reason, "手动备份");
    assert_eq!(b.worlds, vec!["Alpha".to_string(), "Beta".to_string()]);
    assert!(b.size > 0);
    let listed = list(&dirs, INST).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, b.id);

    // Restoring into an emptied instance brings back saves and settings, and never the lock file or mods.
    let game = dirs.instance(INST).unwrap();
    std::fs::remove_dir_all(&game).unwrap();
    restore(&dirs, INST, &b.id).unwrap();
    assert_eq!(read(&game.join("saves/Alpha/region/r.0.0.mca")), "chunks-1");
    assert_eq!(read(&game.join("options.txt")), "fov:70");
    assert!(!game.join("saves/Beta/session.lock").exists());
    assert!(!game.join("mods").exists());
}

#[test]
fn a_full_backup_also_holds_mods() {
    let (_t, dirs, game) = setup();
    let b = create(&dirs, INST, Scope::Full, Kind::Manual, "完整备份", None).unwrap().unwrap();
    std::fs::remove_dir_all(game.join("mods")).unwrap();
    restore(&dirs, INST, &b.id).unwrap();
    assert_eq!(read(&game.join("mods/sodium.jar")), "jar-1");
}

#[test]
fn an_automatic_backup_is_skipped_when_nothing_changed() {
    let (_t, dirs, game) = setup();
    assert!(create(&dirs, INST, Scope::Worlds, Kind::Auto, "启动前自动备份", None).unwrap().is_some());
    pause();
    assert!(create(&dirs, INST, Scope::Worlds, Kind::Auto, "启动前自动备份", None).unwrap().is_none(), "same files, no new backup");

    write(&game.join("saves/Alpha/level.dat"), "alpha-2-longer");
    pause();
    assert!(create(&dirs, INST, Scope::Worlds, Kind::Auto, "启动前自动备份", None).unwrap().is_some());
    assert_eq!(list(&dirs, INST).unwrap().len(), 2);
}

#[test]
fn nothing_to_back_up_is_not_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    assert!(create(&dirs, "empty", Scope::Worlds, Kind::Auto, "x", None).unwrap().is_none());
    assert!(list(&dirs, "empty").unwrap().is_empty());
}

#[test]
fn restoring_first_saves_the_current_state_so_it_can_be_undone() {
    let (_t, dirs, game) = setup();
    let old = create(&dirs, INST, Scope::Worlds, Kind::Auto, "启动前自动备份", None).unwrap().unwrap();
    pause();
    write(&game.join("saves/Alpha/level.dat"), "alpha-ruined");
    write(&game.join("saves/Gamma/level.dat"), "gamma-new");

    restore(&dirs, INST, &old.id).unwrap();

    assert_eq!(read(&game.join("saves/Alpha/level.dat")), "alpha-1");
    assert!(!game.join("saves/Gamma").exists(), "worlds made after the backup go away with the rollback");
    let all = list(&dirs, INST).unwrap();
    let safety = all.iter().find(|b| b.kind == Kind::Safety).expect("a safety backup was made");
    assert!(safety.worlds.contains(&"Gamma".to_string()));

    // And the rollback itself can be rolled back.
    restore(&dirs, INST, &safety.id).unwrap();
    assert_eq!(read(&game.join("saves/Alpha/level.dat")), "alpha-ruined");
    assert_eq!(read(&game.join("saves/Gamma/level.dat")), "gamma-new");
}

#[test]
fn one_world_can_be_restored_without_touching_the_others() {
    let (_t, dirs, game) = setup();
    let b = create(&dirs, INST, Scope::Worlds, Kind::Manual, "m", None).unwrap().unwrap();
    pause();
    write(&game.join("saves/Alpha/level.dat"), "alpha-ruined");
    write(&game.join("saves/Beta/level.dat"), "beta-progress");
    write(&game.join("options.txt"), "fov:110");

    restore_world(&dirs, INST, &b.id, "Alpha").unwrap();

    assert_eq!(read(&game.join("saves/Alpha/level.dat")), "alpha-1");
    assert_eq!(read(&game.join("saves/Beta/level.dat")), "beta-progress");
    assert_eq!(read(&game.join("options.txt")), "fov:110");
    assert!(restore_world(&dirs, INST, &b.id, "Missing").is_err());
    for bad in ["../x", "a/b", "", ".."] {
        assert!(restore_world(&dirs, INST, &b.id, bad).is_err(), "{bad:?}");
    }
}

#[test]
fn old_automatic_backups_are_pruned_but_manual_ones_stay() {
    let (_t, dirs, game) = setup();
    create(&dirs, INST, Scope::Worlds, Kind::Manual, "keep me", None).unwrap().unwrap();
    for i in 0..13 {
        pause();
        write(&game.join("saves/Alpha/level.dat"), &format!("change-{i}-{}", "x".repeat(i)));
        create(&dirs, INST, Scope::Worlds, Kind::Auto, "启动前自动备份", None).unwrap().unwrap();
    }
    let all = list(&dirs, INST).unwrap();
    assert_eq!(all.iter().filter(|b| b.kind == Kind::Auto).count(), 10);
    assert_eq!(all.iter().filter(|b| b.kind == Kind::Manual).count(), 1);
    assert!(all.windows(2).all(|w| w[0].created >= w[1].created), "newest first");
}

#[test]
fn deleting_a_backup_removes_it_and_rejects_odd_ids() {
    let (_t, dirs, _game) = setup();
    let b = create(&dirs, INST, Scope::Worlds, Kind::Manual, "m", None).unwrap().unwrap();
    for bad in ["../x", "a/b", ""] {
        assert!(delete(&dirs, INST, bad).is_err(), "{bad:?}");
    }
    delete(&dirs, INST, &b.id).unwrap();
    assert!(list(&dirs, INST).unwrap().is_empty());
    assert!(restore(&dirs, INST, &b.id).is_err());
}

#[test]
fn a_corrupt_backup_file_is_left_alone_and_does_not_break_listing() {
    let (_t, dirs, game) = setup();
    let b = create(&dirs, INST, Scope::Worlds, Kind::Manual, "m", None).unwrap().unwrap();
    let dir = dirs.backups(INST).unwrap();
    std::fs::write(dir.join("999-manual.zip"), b"not a zip").unwrap();
    std::fs::write(dir.join("notes.txt"), b"hi").unwrap();

    assert_eq!(list(&dirs, INST).unwrap().len(), 1);
    assert!(restore(&dirs, INST, "999-manual").is_err());
    assert_eq!(read(&game.join("saves/Alpha/level.dat")), "alpha-1", "a failed restore changes nothing");
    assert!(list(&dirs, INST).unwrap().iter().any(|x| x.id == b.id));
}

#[test]
fn progress_reports_bytes_up_to_the_total() {
    let (_t, dirs, _game) = setup();
    let last = std::sync::Mutex::new((0u64, 0u64));
    let cb = |done: u64, total: u64| *last.lock().unwrap() = (done, total);
    create(&dirs, INST, Scope::Worlds, Kind::Manual, "m", Some(&cb)).unwrap().unwrap();
    let (done, total) = *last.lock().unwrap();
    assert!(total > 0 && done == total, "{done}/{total}");
}
