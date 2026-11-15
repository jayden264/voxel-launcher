//! The screenshots an instance has taken.

use launcher_core::config::Dirs;
use launcher_core::screenshots::{delete, image_data_url, list, thumb_cache_dir};
use std::time::{Duration, SystemTime};

const INST: &str = "inst";

fn png(path: &std::path::Path, w: u32, h: u32) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::RgbaImage::from_fn(w, h, |x, y| image::Rgba([(x % 255) as u8, (y % 255) as u8, 90, 255])).save(path).unwrap();
}

fn setup() -> (tempfile::TempDir, Dirs, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::new(tmp.path());
    let shots = dirs.instance(INST).unwrap().join("screenshots");
    (tmp, dirs, shots)
}

fn age(path: &std::path::Path, secs: u64) {
    let f = std::fs::File::options().write(true).open(path).unwrap();
    f.set_modified(SystemTime::now() - Duration::from_secs(secs)).unwrap();
}

#[test]
fn lists_images_newest_first_and_ignores_everything_else() {
    let (_t, dirs, shots) = setup();
    png(&shots.join("old.png"), 8, 8);
    png(&shots.join("new.png"), 8, 8);
    std::fs::write(shots.join("notes.txt"), b"x").unwrap();
    std::fs::create_dir_all(shots.join("sub.png")).unwrap();
    age(&shots.join("old.png"), 7200);
    age(&shots.join("new.png"), 60);

    let l = list(&dirs, INST).unwrap();

    assert_eq!(l.iter().map(|s| s.file.as_str()).collect::<Vec<_>>(), ["new.png", "old.png"]);
    assert!(l[0].taken > l[1].taken && l[0].size > 0);
    assert!(list(&dirs, "nothing").unwrap().is_empty());
}

#[test]
fn a_thumbnail_is_a_small_jpeg_data_url_and_is_cached() {
    let (_t, dirs, shots) = setup();
    png(&shots.join("big.png"), 640, 360);

    let url = image_data_url(&dirs, INST, "big.png", 64).unwrap();

    assert!(url.starts_with("data:image/jpeg;base64,"));
    let cached: Vec<_> = std::fs::read_dir(thumb_cache_dir(&dirs)).unwrap().flatten().collect();
    assert_eq!(cached.len(), 1);
    let img = image::open(cached[0].path()).unwrap();
    assert_eq!((img.width(), img.height()), (64, 36), "scaled down, aspect kept");
    // A second call is served from the cache and is identical.
    assert_eq!(image_data_url(&dirs, INST, "big.png", 64).unwrap(), url);
    assert_eq!(std::fs::read_dir(thumb_cache_dir(&dirs)).unwrap().count(), 1);
}

#[test]
fn a_picture_smaller_than_the_limit_is_not_enlarged() {
    let (_t, dirs, shots) = setup();
    png(&shots.join("tiny.png"), 20, 10);
    image_data_url(&dirs, INST, "tiny.png", 400).unwrap();
    let f = std::fs::read_dir(thumb_cache_dir(&dirs)).unwrap().flatten().next().unwrap();
    assert_eq!(image::open(f.path()).unwrap().width(), 20);
}

#[test]
fn odd_names_and_broken_files_are_errors() {
    let (_t, dirs, shots) = setup();
    std::fs::create_dir_all(&shots).unwrap();
    std::fs::write(shots.join("broken.png"), b"not an image").unwrap();
    for bad in ["../x.png", "a/b.png", "a\\b.png", "", "x.txt", "..", "C:evil.png"] {
        assert!(image_data_url(&dirs, INST, bad, 100).is_err(), "{bad:?}");
        assert!(delete(&dirs, INST, bad).is_err(), "{bad:?}");
    }
    assert!(image_data_url(&dirs, INST, "broken.png", 100).is_err());
    assert!(image_data_url(&dirs, INST, "missing.png", 100).is_err());
}

#[test]
fn deleting_removes_the_file_and_its_cached_thumbnails() {
    let (_t, dirs, shots) = setup();
    png(&shots.join("a.png"), 100, 50);
    png(&shots.join("b.png"), 100, 50);
    image_data_url(&dirs, INST, "a.png", 32).unwrap();
    image_data_url(&dirs, INST, "a.png", 64).unwrap();
    image_data_url(&dirs, INST, "b.png", 32).unwrap();

    delete(&dirs, INST, "a.png").unwrap();

    assert!(!shots.join("a.png").exists() && shots.join("b.png").exists());
    assert_eq!(std::fs::read_dir(thumb_cache_dir(&dirs)).unwrap().count(), 1, "only b's thumbnail is left");
    assert!(delete(&dirs, INST, "a.png").is_err());
}
