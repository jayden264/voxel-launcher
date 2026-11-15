//! Reading the worlds of an instance, and the play-time statistics.

use flate2::write::GzEncoder;
use flate2::Compression;
use launcher_core::backup::{self, Kind, Scope};
use launcher_core::config::Dirs;
use launcher_core::stats::{self, Session};
use launcher_core::worlds::{delete_world, list_worlds, world_times};
use std::io::Write;
use std::path::Path;

const INST: &str = "inst";

// ---- a tiny NBT writer for fixtures
fn named(ty: u8, name: &str, payload: &[u8]) -> Vec<u8> {
    let mut v = vec![ty];
    v.extend((name.len() as u16).to_be_bytes());
    v.extend(name.as_bytes());
    v.extend(payload);
    v
}
fn s(name: &str, val: &str) -> Vec<u8> {
    let mut p = (val.len() as u16).to_be_bytes().to_vec();
    p.extend(val.as_bytes());
    named(8, name, &p)
}
fn long(name: &str, v: i64) -> Vec<u8> {
    named(4, name, &v.to_be_bytes())
}
fn int(name: &str, v: i32) -> Vec<u8> {
    named(3, name, &v.to_be_bytes())
}
fn byte(name: &str, v: i8) -> Vec<u8> {
    named(1, name, &[v as u8])
}
fn compound(name: &str, children: &[Vec<u8>]) -> Vec<u8> {
    let mut p: Vec<u8> = children.concat();
    p.push(0);
    named(10, name, &p)
}
fn list_of_strings(name: &str, items: &[&str]) -> Vec<u8> {
    let mut p = vec![8u8];
    p.extend((items.len() as i32).to_be_bytes());
    for i in items {
        p.extend((i.len() as u16).to_be_bytes());
        p.extend(i.as_bytes());
    }
    named(9, name, &p)
}
fn byte_array(name: &str, data: &[u8]) -> Vec<u8> {
    let mut p = (data.len() as i32).to_be_bytes().to_vec();
    p.extend(data);
    named(7, name, &p)
}

fn level_dat(name: &str, last_played: i64, mode: i32, hardcore: i8, version: &str) -> Vec<u8> {
    let data = compound(
        "Data",
        &[
            list_of_strings("ServerBrands", &["vanilla", "fabric"]),
            byte_array("junk", &[1, 2, 3, 4, 5]),
            s("LevelName", name),
            long("LastPlayed", last_played),
            int("GameType", mode),
            byte("hardcore", hardcore),
            compound("Version", &[int("Id", 4000), s("Name", version)]),
        ],
    );
    let root = compound("", &[data]);
    let mut enc = GzEncoder::new(Vec::new(), Compression::fast());
    enc.write_all(&root).unwrap();
    enc.finish().unwrap()
}

fn put(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn setup() -> (tempfile::TempDir, Dirs, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    let saves = dirs.instance(INST).unwrap().join("saves");
    (tmp, dirs, saves)
}

#[test]
fn reads_name_mode_version_and_last_played_from_level_dat() {
    let (_t, dirs, saves) = setup();
    put(&saves.join("New World/level.dat"), &level_dat("我的世界", 1_700_000_000_000, 1, 0, "1.21.1"));
    put(&saves.join("New World/region/r.0.0.mca"), &[0u8; 1000]);
    put(&saves.join("Old/level.dat"), &level_dat("Old", 1_600_000_000_000, 0, 1, "1.12.2"));
    std::fs::create_dir_all(saves.join("not-a-world")).unwrap();

    let worlds = list_worlds(&dirs, INST).unwrap();

    assert_eq!(worlds.len(), 2, "a folder without level.dat is not a world");
    assert_eq!(worlds[0].dir, "New World", "most recently played first");
    assert_eq!(worlds[0].name, "我的世界");
    assert_eq!(worlds[0].last_played, Some(1_700_000_000_000));
    assert_eq!((worlds[0].game_mode.as_deref(), worlds[0].hardcore), (Some("creative"), false));
    assert_eq!(worlds[0].version.as_deref(), Some("1.21.1"));
    assert!(worlds[0].size >= 1000);
    assert_eq!((worlds[1].game_mode.as_deref(), worlds[1].hardcore), (Some("survival"), true));
}

#[test]
fn a_world_with_an_unreadable_level_dat_still_shows_up_by_folder_name() {
    let (_t, dirs, saves) = setup();
    put(&saves.join("Broken/level.dat"), b"garbage");
    let worlds = list_worlds(&dirs, INST).unwrap();
    assert_eq!(worlds.len(), 1);
    assert_eq!((worlds[0].name.as_str(), worlds[0].last_played, worlds[0].version.clone()), ("Broken", None, None));
}

#[test]
fn the_icon_comes_back_as_a_data_url() {
    let (_t, dirs, saves) = setup();
    put(&saves.join("W/level.dat"), &level_dat("W", 1, 0, 0, "1.21"));
    put(&saves.join("W/icon.png"), &[0x89, b'P', b'N', b'G', 1, 2, 3]);
    let w = &list_worlds(&dirs, INST).unwrap()[0];
    assert_eq!(w.icon.as_deref(), Some("data:image/png;base64,iVBORwECAw=="));
}

#[test]
fn play_time_is_read_from_every_stats_format_and_summed_over_players() {
    let (_t, dirs, saves) = setup();
    put(&saves.join("New/level.dat"), &level_dat("New", 1, 0, 0, "1.21"));
    put(&saves.join("New/stats/a.json"), br#"{"stats":{"minecraft:custom":{"minecraft:play_time":72000}},"DataVersion":1}"#); // 1 hour
    put(&saves.join("New/stats/b.json"), br#"{"stats":{"minecraft:custom":{"minecraft:play_time":36000}}}"#); // +30 min
    put(&saves.join("Mid/level.dat"), &level_dat("Mid", 1, 0, 0, "1.16"));
    put(&saves.join("Mid/stats/a.json"), br#"{"stats":{"minecraft:custom":{"minecraft:play_one_minute":1200}}}"#); // 1 min
    put(&saves.join("Old/level.dat"), &level_dat("Old", 1, 0, 0, "1.12"));
    put(&saves.join("Old/stats/a.json"), br#"{"stat.playOneMinute":2400}"#); // 2 min
    put(&saves.join("None/level.dat"), &level_dat("None", 1, 0, 0, "1.21"));
    put(&saves.join("None/stats/broken.json"), b"{");

    let by = |d: &str| list_worlds(&dirs, INST).unwrap().into_iter().find(|w| w.dir == d).unwrap().play_secs;

    assert_eq!(by("New"), 5400);
    assert_eq!(by("Mid"), 60);
    assert_eq!(by("Old"), 120);
    assert_eq!(by("None"), 0);
}

#[test]
fn deleting_a_world_backs_it_up_first() {
    let (_t, dirs, saves) = setup();
    put(&saves.join("Doomed/level.dat"), &level_dat("Doomed", 1, 0, 0, "1.21"));
    put(&saves.join("Doomed/data.txt"), b"precious");
    put(&saves.join("Keep/level.dat"), &level_dat("Keep", 1, 0, 0, "1.21"));

    delete_world(&dirs, INST, "Doomed").unwrap();

    assert!(!saves.join("Doomed").exists() && saves.join("Keep").exists());
    let all = backup::list(&dirs, INST).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].kind, Kind::Safety);
    backup::restore_world(&dirs, INST, &all[0].id, "Doomed").unwrap();
    assert_eq!(std::fs::read(saves.join("Doomed/data.txt")).unwrap(), b"precious");
    for bad in ["../x", "a/b", "", ".."] {
        assert!(delete_world(&dirs, INST, bad).is_err(), "{bad:?}");
    }
    assert!(delete_world(&dirs, INST, "Nope").is_err());
    let _ = Scope::Worlds;
}

#[test]
fn sessions_are_recorded_ignoring_blips_and_capped() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    assert!(stats::sessions(&dirs).is_empty());

    stats::record(&dirs, Session { instance_id: "a".into(), start: 100, secs: 3600, crashed: false }).unwrap();
    stats::record(&dirs, Session { instance_id: "b".into(), start: 9000, secs: 2, crashed: false }).unwrap();
    stats::record(&dirs, Session { instance_id: "a".into(), start: 20000, secs: 600, crashed: true }).unwrap();

    let got = stats::sessions(&dirs);
    assert_eq!(got.len(), 2, "a two-second blip is not a game");
    assert_eq!((got[0].start, got[0].secs, got[1].crashed), (100, 3600, true));

    let many = (0..(stats::MAX_SESSIONS + 5)).map(|i| Session { instance_id: "a".into(), start: 1_000_000 + i as u64, secs: 60, crashed: false }).collect();
    stats::record_all(&dirs, many).unwrap();
    let got = stats::sessions(&dirs);
    assert_eq!(got.len(), stats::MAX_SESSIONS);
    assert_eq!(got.last().unwrap().start, 1_000_000 + stats::MAX_SESSIONS as u64 + 4, "the newest are kept");
}

#[test]
fn a_corrupt_stats_file_starts_over_instead_of_failing() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    std::fs::write(dirs.stats_file(), b"{not json").unwrap();
    assert!(stats::sessions(&dirs).is_empty());
    stats::record(&dirs, Session { instance_id: "a".into(), start: 1, secs: 60, crashed: false }).unwrap();
    assert_eq!(stats::sessions(&dirs).len(), 1);
}

#[test]
fn world_times_gives_names_and_seconds_only() {
    let (_t, dirs, saves) = setup();
    put(&saves.join("A/level.dat"), &level_dat("Alpha", 1, 0, 0, "1.21"));
    put(&saves.join("A/stats/p.json"), br#"{"stats":{"minecraft:custom":{"minecraft:play_time":1200}}}"#);
    put(&saves.join("junk/readme.txt"), b"x");
    let t = world_times(&dirs, INST).unwrap();
    assert_eq!(t.len(), 1);
    assert_eq!((t[0].dir.as_str(), t[0].name.as_str(), t[0].play_secs), ("A", "Alpha", 60));
    assert!(world_times(&dirs, "none").unwrap().is_empty());
}
