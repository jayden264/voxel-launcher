//! Instance backups: a zip of the worlds and settings (or, for a full backup,
//! the mods too) that can be put back, whole or one world at a time.
//!
//! * Automatic backups skip themselves when nothing changed since the last one
//!   and are pruned to the newest [`KEEP_AUTO`]; manual ones are never pruned.
//! * Every restore first backs up what is there now (a `Safety` backup), so a
//!   rollback can itself be undone.
//! * A restore extracts to a scratch folder before touching the instance, so a
//!   damaged backup changes nothing.

use crate::config::Dirs;
use crate::{paths, Error, Result};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const KEEP_AUTO: usize = 10;
pub const KEEP_SAFETY: usize = 5;
/// An automatic backup of more than this is skipped (with an error the UI shows) rather than stalling every launch.
const AUTO_LIMIT_BYTES: u64 = 6 * 1024 * 1024 * 1024;
const MANIFEST: &str = "voxel-backup.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// Saves, `config`, `options.txt`, `servers.dat`.
    Worlds,
    /// Everything in `Worlds` plus mods, shader packs and resource packs.
    Full,
}

impl Scope {
    fn tops(self) -> &'static [&'static str] {
        match self {
            Scope::Worlds => &["saves", "config", "options.txt", "servers.dat"],
            Scope::Full => &["saves", "config", "options.txt", "servers.dat", "mods", "shaderpacks", "resourcepacks", ".voxel"],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Made before every launch.
    Auto,
    Manual,
    /// The state that was replaced by a restore (or a world delete).
    Safety,
}

impl Kind {
    fn slug(self) -> &'static str {
        match self {
            Kind::Auto => "auto",
            Kind::Manual => "manual",
            Kind::Safety => "safety",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Manifest {
    created: u64,
    kind: Kind,
    scope: Scope,
    reason: String,
    sig: String,
    worlds: Vec<String>,
    tops: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupInfo {
    pub id: String,
    /// Milliseconds since the epoch.
    pub created: u64,
    pub kind: Kind,
    pub scope: Scope,
    pub reason: String,
    pub size: u64,
    pub worlds: Vec<String>,
    #[serde(skip)]
    sig: String,
}

struct Entry {
    abs: PathBuf,
    rel: String,
    size: u64,
    mtime: u64,
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn check_name(name: &str) -> Result<()> {
    let bad = name.is_empty() || name == "." || name.contains(['/', '\\', ':', '\0']) || name.contains("..");
    if bad {
        Err(Error::UnsafePath(name.to_string()))
    } else {
        Ok(())
    }
}

fn collect(game: &Path, scope: Scope) -> Vec<Entry> {
    fn walk(dir: &Path, rel: &str, out: &mut Vec<Entry>) {
        let Ok(read) = std::fs::read_dir(dir) else { return };
        for e in read.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let rel = format!("{rel}/{name}");
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                walk(&e.path(), &rel, out);
            } else if ft.is_file() && name != "session.lock" {
                let Ok(meta) = e.metadata() else { continue };
                let mtime = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
                out.push(Entry { abs: e.path(), rel, size: meta.len(), mtime });
            }
        }
    }
    let mut out = Vec::new();
    for top in scope.tops() {
        let p = game.join(top);
        match std::fs::symlink_metadata(&p) {
            Ok(m) if m.is_dir() => walk(&p, top, &mut out),
            Ok(m) if m.is_file() => out.push(Entry {
                abs: p,
                rel: top.to_string(),
                size: m.len(),
                mtime: m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0),
            }),
            _ => {}
        }
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

fn signature(files: &[Entry]) -> String {
    let mut h = Sha1::new();
    for f in files {
        h.update(format!("{}|{}|{}\n", f.rel, f.size, f.mtime));
    }
    hex::encode(h.finalize())
}

fn worlds_of<'a>(rels: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut v: Vec<String> = rels.filter_map(|r| r.strip_prefix("saves/")).filter_map(|r| r.split('/').next()).filter(|w| !w.is_empty()).map(str::to_string).collect();
    v.sort();
    v.dedup();
    v
}

fn read_manifest(path: &Path) -> Result<Manifest> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path)?)?;
    let mut f = zip.by_name(MANIFEST)?;
    let mut s = String::new();
    f.read_to_string(&mut s)?;
    Ok(serde_json::from_str(&s)?)
}

/// The newest backup first. Files that are not valid backups are ignored.
pub fn list(dirs: &Dirs, instance_id: &str) -> Result<Vec<BackupInfo>> {
    let dir = dirs.backups(instance_id)?;
    let Ok(read) = std::fs::read_dir(&dir) else { return Ok(vec![]) };
    let mut out = Vec::new();
    for e in read.flatten() {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) != Some("zip") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|s| s.to_str()).map(str::to_string) else { continue };
        let Ok(m) = read_manifest(&path) else { continue };
        out.push(BackupInfo { id, created: m.created, kind: m.kind, scope: m.scope, reason: m.reason, size: e.metadata().map(|x| x.len()).unwrap_or(0), worlds: m.worlds, sig: m.sig });
    }
    out.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| b.id.cmp(&a.id)));
    Ok(out)
}

/// Back the instance up. `Ok(None)` when there is nothing to back up, or (for
/// `Kind::Auto`) nothing changed since the last backup of the same scope.
pub fn create(
    dirs: &Dirs,
    instance_id: &str,
    scope: Scope,
    kind: Kind,
    reason: &str,
    progress: Option<&(dyn Fn(u64, u64) + Sync)>,
) -> Result<Option<BackupInfo>> {
    let game = dirs.instance(instance_id)?;
    let files = collect(&game, scope);
    if files.is_empty() {
        return Ok(None);
    }
    let total: u64 = files.iter().map(|f| f.size).sum();
    if kind == Kind::Auto && total > AUTO_LIMIT_BYTES {
        return Err(Error::Other(format!("存档和设置共 {} GB，超过自动备份上限，已跳过（可以手动备份）", total / (1024 * 1024 * 1024))));
    }
    let sig = signature(&files);
    if kind == Kind::Auto && list(dirs, instance_id)?.iter().find(|b| b.scope == scope).is_some_and(|b| b.sig == sig) {
        return Ok(None);
    }

    let dir = dirs.backups(instance_id)?;
    std::fs::create_dir_all(&dir)?;
    let created = now_ms();
    let mut id = format!("{created}-{}", kind.slug());
    let mut n = 1;
    while dir.join(format!("{id}.zip")).exists() {
        n += 1;
        id = format!("{created}-{}-{n}", kind.slug());
    }
    let manifest = Manifest {
        created,
        kind,
        scope,
        reason: reason.to_string(),
        sig,
        worlds: worlds_of(files.iter().map(|f| f.rel.as_str())),
        tops: scope.tops().iter().map(|t| t.to_string()).collect(),
    };

    let part = dir.join(format!("{id}.zip.part"));
    let written = (|| -> Result<()> {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&part)?);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).compression_level(Some(1));
        zip.start_file(MANIFEST, opts)?;
        zip.write_all(&serde_json::to_vec(&manifest)?)?;
        let mut done = 0u64;
        let mut buf = vec![0u8; 256 * 1024];
        for f in &files {
            // A file that vanished or is locked is left out rather than failing the whole backup.
            let Ok(mut src) = std::fs::File::open(&f.abs) else { continue };
            zip.start_file(&f.rel, opts.large_file(f.size > 3_900_000_000))?;
            loop {
                let n = match src.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                zip.write_all(&buf[..n])?;
                done += n as u64;
                if let Some(cb) = progress {
                    cb(done.min(total), total);
                }
            }
        }
        zip.finish()?;
        if let Some(cb) = progress {
            cb(total, total);
        }
        Ok(())
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    std::fs::rename(&part, dir.join(format!("{id}.zip")))?;
    prune(dirs, instance_id)?;
    let size = std::fs::metadata(dir.join(format!("{id}.zip"))).map(|m| m.len()).unwrap_or(0);
    Ok(Some(BackupInfo { id, created, kind, scope, reason: manifest.reason, size, worlds: manifest.worlds, sig: manifest.sig }))
}

fn prune(dirs: &Dirs, instance_id: &str) -> Result<()> {
    let dir = dirs.backups(instance_id)?;
    for (kind, keep) in [(Kind::Auto, KEEP_AUTO), (Kind::Safety, KEEP_SAFETY)] {
        for old in list(dirs, instance_id)?.into_iter().filter(|b| b.kind == kind).skip(keep) {
            let _ = std::fs::remove_file(dir.join(format!("{}.zip", old.id)));
        }
    }
    Ok(())
}

pub fn delete(dirs: &Dirs, instance_id: &str, backup_id: &str) -> Result<()> {
    check_name(backup_id)?;
    let path = dirs.backups(instance_id)?.join(format!("{backup_id}.zip"));
    if !path.is_file() {
        return Err(Error::Missing(path));
    }
    std::fs::remove_file(path)?;
    Ok(())
}

fn open_backup(dirs: &Dirs, instance_id: &str, backup_id: &str) -> Result<(zip::ZipArchive<std::fs::File>, Manifest, PathBuf)> {
    check_name(backup_id)?;
    let dir = dirs.backups(instance_id)?;
    let path = dir.join(format!("{backup_id}.zip"));
    if !path.is_file() {
        return Err(Error::Missing(path));
    }
    let manifest = read_manifest(&path).map_err(|_| Error::Other("这个备份文件已损坏，无法回退".into()))?;
    Ok((zip::ZipArchive::new(std::fs::File::open(path)?)?, manifest, dir))
}

/// Extract the entries `want` accepts into a fresh scratch folder next to the backups.
fn extract(zip: &mut zip::ZipArchive<std::fs::File>, scratch: &Path, want: impl Fn(&str) -> bool) -> Result<usize> {
    let _ = std::fs::remove_dir_all(scratch);
    std::fs::create_dir_all(scratch)?;
    let mut count = 0;
    let run = (|| -> Result<()> {
        for i in 0..zip.len() {
            let mut f = zip.by_index(i)?;
            let name = f.name()?.to_string();
            if name == MANIFEST || f.is_dir() || !want(&name) {
                continue;
            }
            let dest = paths::safe_join(scratch, &name)?;
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::io::copy(&mut f, &mut std::fs::File::create(&dest)?)?;
            count += 1;
        }
        Ok(())
    })();
    if let Err(e) = run {
        let _ = std::fs::remove_dir_all(scratch);
        return Err(e);
    }
    Ok(count)
}

fn replace(target: &Path, source: &Path) -> Result<()> {
    if let Ok(m) = std::fs::symlink_metadata(target) {
        if m.is_dir() {
            std::fs::remove_dir_all(target)?;
        } else {
            std::fs::remove_file(target)?;
        }
    }
    if source.exists() {
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(source, target)?;
    }
    Ok(())
}

/// Put everything the backup covers back as it was. What is there now is saved
/// first as a `Safety` backup.
pub fn restore(dirs: &Dirs, instance_id: &str, backup_id: &str) -> Result<()> {
    let (mut zip, manifest, dir) = open_backup(dirs, instance_id, backup_id)?;
    let scratch = dir.join(".restore-tmp");
    extract(&mut zip, &scratch, |_| true)?;
    let game = dirs.instance(instance_id)?;
    let swapped = (|| -> Result<()> {
        create(dirs, instance_id, manifest.scope, Kind::Safety, "回退前的状态", None)?;
        for top in &manifest.tops {
            replace(&game.join(top), &scratch.join(top))?;
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&scratch);
    swapped
}

/// Put one world back from the backup and leave everything else as it is.
pub fn restore_world(dirs: &Dirs, instance_id: &str, backup_id: &str, world: &str) -> Result<()> {
    check_name(world)?;
    let (mut zip, _, dir) = open_backup(dirs, instance_id, backup_id)?;
    let prefix = format!("saves/{world}/");
    let scratch = dir.join(".restore-tmp");
    if extract(&mut zip, &scratch, |n| n.starts_with(&prefix))? == 0 {
        let _ = std::fs::remove_dir_all(&scratch);
        return Err(Error::Other(format!("这个备份里没有存档「{world}」")));
    }
    let game = dirs.instance(instance_id)?;
    let swapped = (|| -> Result<()> {
        create(dirs, instance_id, Scope::Worlds, Kind::Safety, &format!("回退存档「{world}」前的状态"), None)?;
        replace(&game.join("saves").join(world), &scratch.join("saves").join(world))
    })();
    let _ = std::fs::remove_dir_all(&scratch);
    swapped
}
