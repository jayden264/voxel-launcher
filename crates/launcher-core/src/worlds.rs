//! The worlds (`saves/*`) of an instance: what `level.dat` says about them,
//! how big they are and how long they have been played.

use crate::backup::{self, Kind, Scope};
use crate::config::Dirs;
use crate::{Error, Result};
use serde::Serialize;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

const MAX_LEVEL_BYTES: u64 = 32 * 1024 * 1024;
const MAX_ICON_BYTES: u64 = 256 * 1024;
const TICKS_PER_SECOND: u64 = 20;

#[derive(Debug, Clone, Serialize)]
pub struct World {
    /// Folder name inside `saves`.
    pub dir: String,
    /// The name shown in the game; the folder name if `level.dat` cannot be read.
    pub name: String,
    /// Milliseconds since the epoch.
    pub last_played: Option<i64>,
    /// `survival`, `creative`, `adventure` or `spectator`.
    pub game_mode: Option<String>,
    pub hardcore: bool,
    pub version: Option<String>,
    pub size: u64,
    /// Seconds played, as counted by the game's own statistics.
    pub play_secs: u64,
    /// `data:image/png;base64,…` of the world's `icon.png`.
    pub icon: Option<String>,
}

// ------------------------------------------------------------------ NBT

enum Nbt {
    Byte(i8),
    Int(i32),
    Long(i64),
    Str(String),
    Compound(HashMap<String, Nbt>),
    Other,
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let s = self.b.get(self.at..end)?;
        self.at = end;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn i16(&mut self) -> Option<i16> {
        Some(i16::from_be_bytes(self.take(2)?.try_into().ok()?))
    }
    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }
    fn i64(&mut self) -> Option<i64> {
        Some(i64::from_be_bytes(self.take(8)?.try_into().ok()?))
    }
    fn string(&mut self) -> Option<String> {
        let n = self.i16()? as u16 as usize;
        Some(String::from_utf8_lossy(self.take(n)?).into_owned())
    }

    fn payload(&mut self, ty: u8, depth: u32) -> Option<Nbt> {
        if depth > 32 {
            return None;
        }
        Some(match ty {
            1 => Nbt::Byte(self.u8()? as i8),
            2 => {
                self.take(2)?;
                Nbt::Other
            }
            3 => Nbt::Int(self.i32()?),
            4 => Nbt::Long(self.i64()?),
            5 => {
                self.take(4)?;
                Nbt::Other
            }
            6 => {
                self.take(8)?;
                Nbt::Other
            }
            7 => {
                let n = self.i32()?.max(0) as usize;
                self.take(n)?;
                Nbt::Other
            }
            8 => Nbt::Str(self.string()?),
            9 => {
                let inner = self.u8()?;
                let n = self.i32()?.max(0);
                for _ in 0..n {
                    self.payload(inner, depth + 1)?;
                }
                Nbt::Other
            }
            10 => {
                let mut map = HashMap::new();
                loop {
                    let t = self.u8()?;
                    if t == 0 {
                        break;
                    }
                    let name = self.string()?;
                    map.insert(name, self.payload(t, depth + 1)?);
                }
                Nbt::Compound(map)
            }
            11 => {
                let n = self.i32()?.max(0) as usize;
                self.take(n.checked_mul(4)?)?;
                Nbt::Other
            }
            12 => {
                let n = self.i32()?.max(0) as usize;
                self.take(n.checked_mul(8)?)?;
                Nbt::Other
            }
            _ => return None,
        })
    }
}

struct Level {
    name: Option<String>,
    last_played: Option<i64>,
    game_type: Option<i32>,
    hardcore: bool,
    version: Option<String>,
}

fn read_level(path: &Path) -> Option<Level> {
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(file).take(MAX_LEVEL_BYTES).read_to_end(&mut bytes).ok()?;
    let mut r = Reader { b: &bytes, at: 0 };
    if r.u8()? != 10 {
        return None;
    }
    r.string()?;
    let Nbt::Compound(root) = r.payload(10, 0)? else { return None };
    let Some(Nbt::Compound(data)) = root.get("Data") else { return None };
    let version = match data.get("Version") {
        Some(Nbt::Compound(v)) => match v.get("Name") {
            Some(Nbt::Str(s)) => Some(s.clone()),
            _ => None,
        },
        _ => None,
    };
    Some(Level {
        name: match data.get("LevelName") {
            Some(Nbt::Str(s)) => Some(s.clone()),
            _ => None,
        },
        last_played: match data.get("LastPlayed") {
            Some(Nbt::Long(v)) => Some(*v),
            _ => None,
        },
        game_type: match data.get("GameType") {
            Some(Nbt::Int(v)) => Some(*v),
            _ => None,
        },
        hardcore: matches!(data.get("hardcore"), Some(Nbt::Byte(v)) if *v != 0),
        version,
    })
}

// ---------------------------------------------------------------- helpers

fn dir_size(dir: &Path) -> u64 {
    let Ok(read) = std::fs::read_dir(dir) else { return 0 };
    read.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}

/// Ticks of play time in one player's statistics file, for every format the game has used.
fn play_ticks(json: &serde_json::Value) -> u64 {
    if let Some(custom) = json.pointer("/stats/minecraft:custom") {
        // 1.17+ calls it play_time, 1.13–1.16 play_one_minute.
        return ["minecraft:play_time", "minecraft:play_one_minute"].iter().filter_map(|k| custom.get(k)).filter_map(|v| v.as_u64()).next().unwrap_or(0);
    }
    json.get("stat.playOneMinute").and_then(|v| v.as_u64()).unwrap_or(0)
}

fn play_secs(world: &Path) -> u64 {
    let Ok(read) = std::fs::read_dir(world.join("stats")) else { return 0 };
    read.flatten()
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
        .filter_map(|e| std::fs::read(e.path()).ok())
        .filter_map(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .map(|j| play_ticks(&j))
        .sum::<u64>()
        / TICKS_PER_SECOND
}

pub(crate) fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

fn icon_of(world: &Path) -> Option<String> {
    let path = world.join("icon.png");
    if std::fs::metadata(&path).ok()?.len() > MAX_ICON_BYTES {
        return None;
    }
    Some(format!("data:image/png;base64,{}", base64(&std::fs::read(path).ok()?)))
}

fn mode_name(t: i32) -> Option<&'static str> {
    match t {
        0 => Some("survival"),
        1 => Some("creative"),
        2 => Some("adventure"),
        3 => Some("spectator"),
        _ => None,
    }
}

// -------------------------------------------------------------------- API

/// The instance's worlds, most recently played first. A folder counts as a
/// world when it has a `level.dat`; one that cannot be read is still listed.
pub fn list_worlds(dirs: &Dirs, instance_id: &str) -> Result<Vec<World>> {
    let saves = dirs.instance(instance_id)?.join("saves");
    let Ok(read) = std::fs::read_dir(&saves) else { return Ok(vec![]) };
    let mut out = Vec::new();
    for e in read.flatten() {
        let path = e.path();
        if !path.is_dir() || !path.join("level.dat").is_file() {
            continue;
        }
        let dir = e.file_name().to_string_lossy().into_owned();
        let level = read_level(&path.join("level.dat"));
        out.push(World {
            name: level.as_ref().and_then(|l| l.name.clone()).unwrap_or_else(|| dir.clone()),
            last_played: level.as_ref().and_then(|l| l.last_played),
            game_mode: level.as_ref().and_then(|l| l.game_type).and_then(mode_name).map(str::to_string),
            hardcore: level.as_ref().is_some_and(|l| l.hardcore),
            version: level.as_ref().and_then(|l| l.version.clone()),
            size: dir_size(&path),
            play_secs: play_secs(&path),
            icon: icon_of(&path),
            dir,
        });
    }
    out.sort_by(|a, b| b.last_played.cmp(&a.last_played).then_with(|| a.dir.cmp(&b.dir)));
    Ok(out)
}

/// What the playtime ranking needs: names and seconds only, without sizing or reading icons.
#[derive(Debug, Clone, Serialize)]
pub struct WorldTime {
    pub dir: String,
    pub name: String,
    pub play_secs: u64,
}

pub fn world_times(dirs: &Dirs, instance_id: &str) -> Result<Vec<WorldTime>> {
    let saves = dirs.instance(instance_id)?.join("saves");
    let Ok(read) = std::fs::read_dir(&saves) else { return Ok(vec![]) };
    Ok(read
        .flatten()
        .filter(|e| e.path().join("level.dat").is_file())
        .map(|e| {
            let dir = e.file_name().to_string_lossy().into_owned();
            let name = read_level(&e.path().join("level.dat")).and_then(|l| l.name).unwrap_or_else(|| dir.clone());
            WorldTime { play_secs: play_secs(&e.path()), name, dir }
        })
        .collect())
}

/// Delete a world. A backup of it is made first, so the delete can be undone.
pub fn delete_world(dirs: &Dirs, instance_id: &str, world: &str) -> Result<()> {
    if world.is_empty() || world == "." || world.contains(['/', '\\', ':', '\0']) || world.contains("..") {
        return Err(Error::UnsafePath(world.to_string()));
    }
    let path = dirs.instance(instance_id)?.join("saves").join(world);
    if !path.is_dir() {
        return Err(Error::Missing(path));
    }
    backup::create(dirs, instance_id, Scope::Worlds, Kind::Safety, &format!("删除存档「{world}」前的状态"), None)?;
    std::fs::remove_dir_all(path)?;
    Ok(())
}
