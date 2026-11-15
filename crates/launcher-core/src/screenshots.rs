//! An instance's screenshots: listing, small previews for the grid, deleting.
//!
//! Previews are made here (decoded, scaled, re-encoded as JPEG) and cached on
//! disk, so a page of a hundred 4K screenshots costs the UI a few KB each
//! instead of megabytes.

use crate::config::Dirs;
use crate::{Error, Result};
use serde::Serialize;
use sha1::{Digest, Sha1};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const EXTENSIONS: [&str; 3] = ["png", "jpg", "jpeg"];

#[derive(Debug, Clone, Serialize)]
pub struct Screenshot {
    /// File name inside the instance's `screenshots` folder.
    pub file: String,
    /// Milliseconds since the epoch.
    pub taken: u64,
    pub size: u64,
}

pub fn thumb_cache_dir(dirs: &Dirs) -> PathBuf {
    dirs.root().join("cache").join("thumbs")
}

fn is_image_name(name: &str) -> bool {
    Path::new(name).extension().and_then(|e| e.to_str()).is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Names come from the UI: a plain file name with an image extension.
fn checked(dirs: &Dirs, instance_id: &str, file: &str) -> Result<PathBuf> {
    let bad = file.is_empty() || file.contains(['/', '\\', ':', '\0']) || file.contains("..") || !is_image_name(file);
    if bad {
        return Err(Error::UnsafePath(file.to_string()));
    }
    Ok(dirs.instance(instance_id)?.join("screenshots").join(file))
}

/// Newest first.
pub fn list(dirs: &Dirs, instance_id: &str) -> Result<Vec<Screenshot>> {
    let dir = dirs.instance(instance_id)?.join("screenshots");
    let Ok(read) = std::fs::read_dir(&dir) else { return Ok(vec![]) };
    let mut out: Vec<Screenshot> = read
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter_map(|e| {
            let file = e.file_name().to_string_lossy().into_owned();
            if !is_image_name(&file) {
                return None;
            }
            let meta = e.metadata().ok()?;
            let taken = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_millis() as u64;
            Some(Screenshot { file, taken, size: meta.len() })
        })
        .collect();
    out.sort_by(|a, b| b.taken.cmp(&a.taken).then_with(|| b.file.cmp(&a.file)));
    Ok(out)
}

fn cache_key(instance_id: &str, file: &str) -> String {
    let mut h = Sha1::new();
    h.update(format!("{instance_id}/{file}"));
    hex::encode(h.finalize())[..16].to_string()
}

/// `data:image/jpeg;base64,…` of the screenshot scaled to at most `max_width`
/// pixels wide (never enlarged).
pub fn image_data_url(dirs: &Dirs, instance_id: &str, file: &str, max_width: u32) -> Result<String> {
    let path = checked(dirs, instance_id, file)?;
    let meta = std::fs::metadata(&path).map_err(|_| Error::Missing(path.clone()))?;
    let mtime = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    // The key includes the modification time, so an edited file gets a fresh preview.
    let cache = thumb_cache_dir(dirs).join(format!("{}-{}-{}-{}.jpg", cache_key(instance_id, file), mtime, meta.len(), max_width));
    let bytes = match std::fs::read(&cache) {
        Ok(b) => b,
        Err(_) => {
            let img = image::open(&path).map_err(|e| Error::Other(format!("无法读取截图 {file}: {e}")))?;
            let img = if img.width() > max_width { img.resize(max_width, u32::MAX, image::imageops::FilterType::Triangle) } else { img };
            let mut out = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 82).encode_image(&img.to_rgb8()).map_err(|e| Error::Other(format!("无法生成截图预览: {e}")))?;
            if std::fs::create_dir_all(thumb_cache_dir(dirs)).is_ok() {
                let _ = std::fs::write(&cache, &out);
            }
            out
        }
    };
    Ok(format!("data:image/jpeg;base64,{}", crate::worlds::base64(&bytes)))
}

/// Delete the screenshot and the previews made from it.
pub fn delete(dirs: &Dirs, instance_id: &str, file: &str) -> Result<()> {
    let path = checked(dirs, instance_id, file)?;
    if !path.is_file() {
        return Err(Error::Missing(path));
    }
    std::fs::remove_file(&path)?;
    let prefix = format!("{}-", cache_key(instance_id, file));
    if let Ok(read) = std::fs::read_dir(thumb_cache_dir(dirs)) {
        for e in read.flatten() {
            if e.file_name().to_string_lossy().starts_with(&prefix) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    Ok(())
}
