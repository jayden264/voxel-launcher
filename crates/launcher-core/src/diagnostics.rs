//! A diagnostics bundle a user can send when something goes wrong: system
//! facts, sanitised settings, and the logs and mod list of one instance.
//!
//! Left out on purpose: account names, saved servers, worlds. The user's own
//! name is replaced everywhere it appears (it is in most file paths).

use crate::config::{Dirs, InstancesFile, Settings};
use crate::mods::list_installed;
use crate::Result;
use regex::RegexBuilder;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const LOG_TAIL_BYTES: u64 = 2 * 1024 * 1024;
const SMALL_TAIL_BYTES: u64 = 512 * 1024;
const MAX_CRASH_REPORTS: usize = 3;

const README: &str = "这是 Voxel 启动器导出的诊断包，用来帮人排查问题。\n\n\
包含：system.txt（系统和启动器信息）、settings.json（设置，账号只保留数量）、instances.json（实例列表）、\n\
instance/（所选实例的最新日志、最近 3 份崩溃报告、模组列表）。\n\n\
没有包含：账号名称、保存的服务器、存档、截图。你的 Windows 用户名已全部替换为 <user>。\n\
发出去之前，可以随意打开里面的文件检查。\n";

/// Where the bundle goes by default: the user's Downloads folder.
pub fn default_output_dir(fallback: &Path) -> PathBuf {
    dirs::download_dir().or_else(dirs::desktop_dir).unwrap_or_else(|| fallback.to_path_buf())
}

/// Words to blank out of the bundle: the user's login name and home folder name.
pub fn user_names() -> Vec<String> {
    let mut names: Vec<String> = ["USERNAME", "USER"].iter().filter_map(|k| std::env::var(k).ok()).collect();
    if let Some(n) = dirs::home_dir().and_then(|h| h.file_name().map(|n| n.to_string_lossy().into_owned())) {
        names.push(n);
    }
    names.sort();
    names.dedup();
    names
}

/// "windows x86_64 (Windows 11 Pro for Workstations 10.0.26300)".
pub fn os_description() -> String {
    format!("{} {} ({})", std::env::consts::OS, std::env::consts::ARCH, sysinfo::System::long_os_version().unwrap_or_default())
}

/// Replace each of `names` (case-insensitively, wherever it appears) with `<user>`.
fn redact(text: &str, names: &[String]) -> String {
    let mut out = text.to_string();
    for n in names.iter().filter(|n| n.chars().count() >= 3) {
        if let Ok(re) = RegexBuilder::new(&regex::escape(n)).case_insensitive(true).build() {
            out = re.replace_all(&out, "<user>").into_owned();
        }
    }
    out
}

/// The last `max` bytes of a text file, with a note when something was cut.
fn tail(path: &Path, max: u64) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let cut = len > max;
    if cut {
        f.seek(SeekFrom::Start(len - max)).ok()?;
    }
    let mut bytes = Vec::new();
    f.take(max).read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Some(if cut { format!("…（文件过大，已截断，只保留最后 {} KB）\n{text}", max / 1024) } else { text })
}

/// `YYYYMMDD-HHMMSS` in UTC from seconds since the epoch.
fn stamp(secs: u64) -> String {
    let (days, rem) = (secs / 86400, secs % 86400);
    // Civil-from-days (Howard Hinnant).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}{m:02}{d:02}-{:02}{:02}{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

fn mods_text(dirs: &Dirs, instance_id: &str) -> String {
    let Ok(mods) = list_installed(dirs, instance_id) else { return "（读不到模组文件夹）\n".into() };
    if mods.is_empty() {
        return "（没有模组）\n".into();
    }
    mods.iter()
        .map(|m| {
            format!(
                "{}\t{} 字节\t{}\t{}{}\n",
                m.filename,
                m.size,
                if m.enabled { "启用" } else { "已禁用" },
                m.title.as_deref().unwrap_or("-"),
                m.version_number.as_deref().map(|v| format!(" {v}")).unwrap_or_default()
            )
        })
        .collect()
}

/// Build the bundle in `out_dir` and return its path. `redact_names` are the
/// words to blank out (the user's Windows name); `system_text` is what the
/// caller knows about the machine.
pub fn export(
    dirs: &Dirs,
    settings: &Settings,
    instances: &InstancesFile,
    instance_id: Option<&str>,
    system_text: &str,
    redact_names: &[String],
    out_dir: &Path,
) -> Result<PathBuf> {
    std::fs::create_dir_all(out_dir)?;
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let base = format!("voxel-diagnostics-{}", stamp(secs));
    let mut path = out_dir.join(format!("{base}.zip"));
    let mut n = 1;
    while path.exists() {
        n += 1;
        path = out_dir.join(format!("{base}-{n}.zip"));
    }

    let settings_json = serde_json::to_string_pretty(&serde_json::json!({
        "source": settings.source,
        "java_path": settings.java_path,
        "max_downloads": settings.max_downloads,
        "default_memory_mb": settings.default_memory_mb,
        "auto_backup": settings.auto_backup,
        "accounts": settings.accounts.len(),
        "servers": settings.servers.len(),
    }))?;
    let instances_json = serde_json::to_string_pretty(&instances.instances)?;

    let mut files: Vec<(String, String)> = vec![
        ("README.txt".into(), README.into()),
        ("system.txt".into(), system_text.to_string()),
        ("settings.json".into(), settings_json),
        ("instances.json".into(), instances_json),
    ];
    if let Some(id) = instance_id {
        if let Ok(game) = dirs.instance(id) {
            if let Some(t) = tail(&game.join("logs").join("latest.log"), LOG_TAIL_BYTES) {
                files.push(("instance/latest.log".into(), t));
            }
            if let Some(t) = tail(&game.join("launcher-stdio.log"), SMALL_TAIL_BYTES) {
                files.push(("instance/launcher-stdio.log".into(), t));
            }
            let mut crashes: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(game.join("crash-reports"))
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("txt"))
                .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
                .collect();
            crashes.sort_by(|a, b| b.0.cmp(&a.0));
            for (_, p) in crashes.into_iter().take(MAX_CRASH_REPORTS) {
                if let (Some(name), Some(t)) = (p.file_name().and_then(|n| n.to_str()), tail(&p, SMALL_TAIL_BYTES)) {
                    files.push((format!("instance/crash-reports/{name}"), t));
                }
            }
            files.push(("instance/mods.txt".into(), mods_text(dirs, id)));
        }
    }

    let part = path.with_extension("zip.part");
    let written = (|| -> Result<()> {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&part)?);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, text) in &files {
            zip.start_file(name, opts)?;
            zip.write_all(redact(text, redact_names).as_bytes())?;
        }
        zip.finish()?;
        Ok(())
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    std::fs::rename(&part, &path)?;
    Ok(path)
}
