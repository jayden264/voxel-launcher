//! Sharing instances: copy one, or export one as a Modrinth `.mrpack`.
//!
//! An export links every mod Modrinth hosts (so the pack is small and the
//! authors' downloads stay with them) and bundles what it cannot link inside
//! the pack, so the result installs anywhere without missing files.

use crate::config::{Dirs, Instance, InstancesFile};
use crate::loader::LoaderKind;
use crate::mods::ModrinthClient;
use crate::{Error, Result};
use serde::Serialize;
use sha1::Sha1;
use sha2::{Digest, Sha512};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Folders a clone does not take along: they are logs and leftovers, not the instance.
const CLONE_SKIP: [&str; 6] = ["logs", "crash-reports", "screenshots", "quickPlay", "launcher-stdio.log", "jvm-args.txt"];
/// Folders (next to `mods`) whose files a pack carries as overrides.
const PACK_DIRS: [&str; 5] = ["config", "defaultconfigs", "kubejs", "scripts", "datapacks"];
/// A single override file bigger than this is left out (a pack is for sharing, not for archives).
const MAX_OVERRIDE_BYTES: u64 = 32 * 1024 * 1024;
const CDN_PREFIX: &str = "https://cdn.modrinth.com/";

// -------------------------------------------------------------------- clone

fn copy_tree(from: &Path, to: &Path, skip_top: &[&str], top: bool) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if (top && skip_top.contains(&name.as_str())) || name == "session.lock" {
            continue;
        }
        let ft = e.file_type()?;
        if ft.is_dir() {
            copy_tree(&e.path(), &to.join(&name), skip_top, false)?;
        } else if ft.is_file() {
            std::fs::copy(e.path(), to.join(&name))?;
        }
    }
    Ok(())
}

/// A new instance with the same version, loader, settings and files as `from_id`.
/// Logs and crash reports stay behind; worlds come along only when asked.
pub fn clone_instance(dirs: &Dirs, instances: &mut InstancesFile, from_id: &str, new_name: &str, include_saves: bool) -> Result<Instance> {
    let src = instances.find(from_id).cloned().ok_or_else(|| Error::Other("要复制的实例不存在".into()))?;
    let added_id = instances.add_with_loader(new_name, src.game_version(), src.loader.clone()).id.clone();
    let result = (|| -> Result<()> {
        let from = dirs.instance(from_id)?;
        if from.is_dir() {
            let mut skip: Vec<&str> = CLONE_SKIP.to_vec();
            if !include_saves {
                skip.push("saves");
            }
            copy_tree(&from, &dirs.instance(&added_id)?, &skip, true)?;
        }
        Ok(())
    })();
    if let Err(e) = result {
        instances.instances.retain(|i| i.id != added_id);
        if let Ok(dir) = dirs.instance(&added_id) {
            let _ = std::fs::remove_dir_all(dir);
        }
        return Err(e);
    }
    let inst = instances.instances.iter_mut().find(|i| i.id == added_id).expect("just added");
    inst.memory_mb = src.memory_mb;
    inst.jvm_args = src.jvm_args.clone();
    inst.java_path = src.java_path.clone();
    inst.width = src.width;
    inst.height = src.height;
    Ok(inst.clone())
}

// -------------------------------------------------------------------- export

#[derive(Debug, Clone)]
pub struct ExportOptions {
    pub name: String,
    pub version: String,
    pub summary: Option<String>,
    /// Put the worlds in the pack (off by default: they are big and personal).
    pub include_saves: bool,
    /// Put `options.txt` in the pack (off by default: it holds personal settings).
    pub include_options: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExportReport {
    pub path: PathBuf,
    /// Files the pack only links to (downloaded from Modrinth on install).
    pub linked: usize,
    /// Mods, shader packs and resource packs carried inside the pack itself because Modrinth does not host them.
    pub bundled: usize,
    pub size: u64,
}

struct Candidate {
    /// Path inside the instance, with `/`.
    rel: String,
    abs: PathBuf,
    size: u64,
    sha1: String,
    sha512: String,
}

fn hash_file(path: &Path) -> Result<(String, String)> {
    use sha1::Digest as _;
    let mut f = std::fs::File::open(path)?;
    let (mut a, mut b) = (Sha1::new(), Sha512::new());
    let mut buf = vec![0u8; 128 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        a.update(&buf[..n]);
        b.update(&buf[..n]);
    }
    Ok((hex::encode(a.finalize()), hex::encode(b.finalize())))
}

/// Enabled jars in `mods`, zips in `shaderpacks` and `resourcepacks`.
fn content_candidates(game: &Path) -> Result<Vec<Candidate>> {
    let mut out = Vec::new();
    for (dir, ext) in [("mods", "jar"), ("shaderpacks", "zip"), ("resourcepacks", "zip")] {
        let Ok(read) = std::fs::read_dir(game.join(dir)) else { continue };
        for e in read.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !e.file_type().map(|t| t.is_file()).unwrap_or(false) || !name.to_ascii_lowercase().ends_with(&format!(".{ext}")) {
                continue;
            }
            let (sha1, sha512) = hash_file(&e.path())?;
            out.push(Candidate { rel: format!("{dir}/{name}"), size: e.metadata()?.len(), abs: e.path(), sha1, sha512 });
        }
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(out)
}

fn files_under(base: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) {
    let Ok(read) = std::fs::read_dir(base) else { return };
    for e in read.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let rel = format!("{rel}/{name}");
        match e.file_type() {
            Ok(t) if t.is_dir() => files_under(&e.path(), &rel, out),
            Ok(t) if t.is_file() && name != "session.lock" && e.metadata().map(|m| m.len() <= MAX_OVERRIDE_BYTES).unwrap_or(false) => out.push((rel, e.path())),
            _ => {}
        }
    }
}

fn safe_file_name(name: &str) -> String {
    let cleaned: String = name.chars().map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '_' } else { c }).collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    let short: String = trimmed.chars().take(60).collect();
    if short.is_empty() {
        "modpack".into()
    } else {
        short
    }
}

fn loader_key(kind: LoaderKind) -> &'static str {
    match kind {
        LoaderKind::Fabric => "fabric-loader",
        LoaderKind::Quilt => "quilt-loader",
        LoaderKind::Forge => "forge",
        LoaderKind::NeoForge => "neoforge",
    }
}

/// Write `<name>-<version>.mrpack` into `out_dir`. Modrinth is asked which mods it hosts; if it
/// cannot be reached, every file is simply bundled and the pack still works.
pub async fn export_mrpack(client: &ModrinthClient, dirs: &Dirs, instance: &Instance, opts: &ExportOptions, out_dir: &Path) -> Result<ExportReport> {
    let game = dirs.instance(&instance.id)?;
    let candidates = {
        let game = game.clone();
        tokio::task::spawn_blocking(move || content_candidates(&game)).await.map_err(|e| Error::Other(e.to_string()))??
    };

    let hashes: Vec<String> = candidates.iter().map(|c| c.sha1.clone()).collect();
    let known = if hashes.is_empty() { HashMap::new() } else { client.versions_by_hashes(&hashes).await.unwrap_or_default() };

    let mut index_files = Vec::new();
    let mut overrides: Vec<(String, PathBuf)> = Vec::new();
    let mut linked = 0;
    for c in &candidates {
        let url = known
            .get(&c.sha1)
            .and_then(|v| v.files.iter().find(|f| f.sha1.eq_ignore_ascii_case(&c.sha1)))
            .map(|f| f.url.clone())
            .filter(|u| u.starts_with(CDN_PREFIX) && !u.contains('@'));
        match url {
            Some(url) => {
                linked += 1;
                index_files.push(serde_json::json!({
                    "path": c.rel,
                    "hashes": { "sha1": c.sha1, "sha512": c.sha512 },
                    "env": { "client": "required", "server": "required" },
                    "downloads": [url],
                    "fileSize": c.size,
                }));
            }
            None => overrides.push((format!("overrides/{}", c.rel), c.abs.clone())),
        }
    }
    for dir in PACK_DIRS {
        let mut found = Vec::new();
        files_under(&game.join(dir), dir, &mut found);
        overrides.extend(found.into_iter().map(|(rel, abs)| (format!("overrides/{rel}"), abs)));
    }
    if opts.include_options && game.join("options.txt").is_file() {
        overrides.push(("overrides/options.txt".into(), game.join("options.txt")));
    }
    if opts.include_saves {
        let mut found = Vec::new();
        files_under(&game.join("saves"), "saves", &mut found);
        overrides.extend(found.into_iter().map(|(rel, abs)| (format!("overrides/{rel}"), abs)));
    }

    let mut deps = serde_json::Map::new();
    deps.insert("minecraft".into(), instance.game_version().into());
    if let Some(l) = &instance.loader {
        deps.insert(loader_key(l.kind).into(), l.version.clone().into());
    }
    let mut index = serde_json::json!({
        "formatVersion": 1,
        "game": "minecraft",
        "versionId": opts.version,
        "name": opts.name,
        "files": index_files,
        "dependencies": deps,
    });
    if let Some(s) = opts.summary.as_ref().filter(|s| !s.trim().is_empty()) {
        index["summary"] = s.clone().into();
    }

    std::fs::create_dir_all(out_dir)?;
    let base = format!("{}-{}", safe_file_name(&opts.name), safe_file_name(&opts.version));
    let mut path = out_dir.join(format!("{base}.mrpack"));
    let mut n = 1;
    while path.exists() {
        n += 1;
        path = out_dir.join(format!("{base} ({n}).mrpack"));
    }

    let bundled = candidates.len() - linked;
    let (target, index_bytes) = (path.clone(), serde_json::to_vec_pretty(&index)?);
    tokio::task::spawn_blocking(move || -> Result<()> {
        let part = target.with_extension("mrpack.part");
        let written = (|| -> Result<()> {
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&part)?);
            let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).compression_level(Some(3));
            zip.start_file("modrinth.index.json", opts)?;
            zip.write_all(&index_bytes)?;
            let mut buf = vec![0u8; 256 * 1024];
            for (name, abs) in &overrides {
                // A file that vanished meanwhile is skipped rather than failing the export.
                let Ok(mut src) = std::fs::File::open(abs) else { continue };
                zip.start_file(name, opts)?;
                loop {
                    let n = match src.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    zip.write_all(&buf[..n])?;
                }
            }
            zip.finish()?;
            Ok(())
        })();
        if let Err(e) = written {
            let _ = std::fs::remove_file(&part);
            return Err(e);
        }
        std::fs::rename(&part, &target)?;
        Ok(())
    })
    .await
    .map_err(|e| Error::Other(e.to_string()))??;

    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    Ok(ExportReport { path, linked, bundled, size })
}
