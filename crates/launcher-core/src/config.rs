//! Data directories and robust JSON persistence (atomic write, corrupt-file backup).

use crate::account::OfflineAccount;
use crate::loader::{self, LoaderKind};
use crate::mirror::Source;
use crate::mods::ContentKind;
use crate::{paths, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct Dirs {
    root: PathBuf,
}

impl Dirs {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn default_root() -> PathBuf {
        dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("MinecraftLauncher")
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn shared(&self) -> PathBuf {
        self.root.join("shared")
    }
    pub fn libraries(&self) -> PathBuf {
        self.shared().join("libraries")
    }
    pub fn assets(&self) -> PathBuf {
        self.shared().join("assets")
    }
    pub fn versions(&self) -> PathBuf {
        self.shared().join("versions")
    }
    pub fn runtime(&self) -> PathBuf {
        self.shared().join("runtime")
    }
    pub fn instances(&self) -> PathBuf {
        self.root.join("instances")
    }
    /// Per-instance game directory (`--gameDir`). `id` is validated.
    pub fn instance(&self, id: &str) -> Result<PathBuf> {
        paths::safe_join(&self.instances(), id)
    }
    /// `<instance>/mods`, where loader mods live.
    pub fn mods_dir(&self, id: &str) -> Result<PathBuf> {
        self.content_dir(id, ContentKind::Mod)
    }
    /// `<instance>/.voxel/mods.json`: which installed jar came from which project.
    pub fn mods_record(&self, id: &str) -> Result<PathBuf> {
        self.content_record(id, ContentKind::Mod)
    }
    /// The folder of `kind` inside the instance (`mods`, `shaderpacks`, `resourcepacks`).
    pub fn content_dir(&self, id: &str, kind: ContentKind) -> Result<PathBuf> {
        paths::safe_join(&self.instance(id)?, kind.dir_name())
    }
    /// `<instance>/.voxel/<kind>.json`: which installed file came from which project.
    pub fn content_record(&self, id: &str, kind: ContentKind) -> Result<PathBuf> {
        Ok(self.instance(id)?.join(".voxel").join(format!("{}.json", kind.record_name())))
    }
    /// Where an instance's backups live: `<root>/backups/<id>`.
    pub fn backups(&self, id: &str) -> Result<PathBuf> {
        paths::safe_join(&self.root.join("backups"), id)
    }
    /// Play-time sessions.
    pub fn stats_file(&self) -> PathBuf {
        self.root.join("stats.json")
    }
    pub fn settings_file(&self) -> PathBuf {
        self.root.join("config.json")
    }
    pub fn instances_file(&self) -> PathBuf {
        self.root.join("instances.json")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub schema_version: u32,
    pub source: Source,
    pub java_path: Option<String>,
    pub accounts: Vec<OfflineAccount>,
    pub selected_account: Option<String>,
    pub selected_instance: Option<String>,
    pub max_downloads: usize,
    pub default_memory_mb: Option<u32>,
    /// Back the worlds up before every launch.
    pub auto_backup: bool,
    /// Servers the user saved, for one-click joining.
    pub servers: Vec<crate::servers::ServerEntry>,
    /// Ask GitHub for a newer launcher version each time the launcher starts.
    pub check_updates: bool,
    /// A version the user chose to skip.
    pub ignored_update: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            source: Source::Mirror,
            java_path: None,
            accounts: vec![],
            selected_account: None,
            selected_instance: None,
            max_downloads: 24,
            default_memory_mb: None,
            auto_backup: true,
            servers: vec![],
            check_updates: true,
            ignored_update: None,
        }
    }
}

/// A launched game process we may want to re-attach to after a launcher restart.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunningGame {
    pub pid: u32,
    /// Process start time (seconds since epoch) from `sysinfo`, guards against pid reuse.
    pub start_time: u64,
}

/// Which loader (and version) an instance was created with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoaderRef {
    pub kind: LoaderKind,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Instance {
    pub id: String,
    pub name: String,
    /// What actually gets launched: a vanilla id, or a loader profile id.
    pub version_id: String,
    /// Vanilla Minecraft version; `None` on instances created before loaders existed.
    pub mc_version: Option<String>,
    pub loader: Option<LoaderRef>,
    pub memory_mb: Option<u32>,
    /// Extra JVM arguments (whitespace separated). Applied after `-Xmx`, so they win.
    pub jvm_args: String,
    pub java_path: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub running: Option<RunningGame>,
}

impl Default for Instance {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            version_id: String::new(),
            mc_version: None,
            loader: None,
            memory_mb: None,
            jvm_args: String::new(),
            java_path: None,
            width: None,
            height: None,
            running: None,
        }
    }
}

impl Instance {
    /// The vanilla Minecraft version this instance plays.
    pub fn game_version(&self) -> &str {
        self.mc_version.as_deref().unwrap_or(&self.version_id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct InstancesFile {
    pub schema_version: u32,
    pub instances: Vec<Instance>,
}

impl Default for InstancesFile {
    fn default() -> Self {
        Self { schema_version: SCHEMA_VERSION, instances: vec![] }
    }
}

impl InstancesFile {
    pub fn find(&self, id: &str) -> Option<&Instance> {
        self.instances.iter().find(|i| i.id == id)
    }

    /// Create an instance with a unique filesystem-safe id derived from its name.
    pub fn add(&mut self, name: &str, version_id: &str) -> &Instance {
        self.add_with_loader(name, version_id, None)
    }

    /// Like [`add`](Self::add) for a vanilla `mc_version`, optionally played
    /// through a loader (then `version_id` is the loader's profile id).
    pub fn add_with_loader(&mut self, name: &str, mc_version: &str, loader: Option<LoaderRef>) -> &Instance {
        let version_id = match &loader {
            Some(l) => loader::profile_id(l.kind, &l.version, mc_version),
            None => mc_version.to_string(),
        };
        let mc_version = loader.is_some().then(|| mc_version.to_string());
        let base = slugify(name);
        let mut id = base.clone();
        let mut n = 2;
        while self.find(&id).is_some() {
            id = format!("{base}-{n}");
            n += 1;
        }
        self.instances.push(Instance {
            id,
            name: name.to_string(),
            version_id,
            mc_version,
            loader,
            ..Default::default()
        });
        self.instances.last().unwrap()
    }
}

fn slugify(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let s = s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    if s.is_empty() {
        "instance".into()
    } else {
        s
    }
}

/// Load JSON; on a missing file return the default, on a corrupt file back it
/// up (`<name>.bak-<unix-time>`) and return the default. Never panics.
pub fn load_or_default<T: DeserializeOwned + Default>(path: &Path) -> T {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(_) => return T::default(),
    };
    match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("{} 已损坏 ({e})，已备份并使用默认值", path.display());
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
            name.push(format!(".bak-{ts}"));
            let _ = std::fs::rename(path, path.with_file_name(name));
            T::default()
        }
    }
}

/// Write JSON via a temp file + rename so a crash never leaves a half-written file.
pub fn save_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp_name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);
    std::fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Default heap size from physical memory: a quarter, clamped to 2-4 GiB,
/// never above `max_memory_mb()`.
pub fn recommended_memory_mb() -> u32 {
    let total = total_memory_mb();
    (total / 4).clamp(2048, 4096).min(max_memory_mb())
}

/// Upper bound for the memory slider: ~75% of physical RAM.
pub fn max_memory_mb() -> u32 {
    ((total_memory_mb() as f64) * 0.75) as u32
}

/// Windows build number (e.g. 22631); 0 when unknown / not Windows.
/// Mica needs build >= 22000 (Windows 11).
pub fn os_build() -> u32 {
    if !cfg!(windows) {
        return 0;
    }
    sysinfo::System::kernel_version()
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0)
}

fn total_memory_mb() -> u32 {
    let mb = match sysinfo::System::new() {
        Ok(mut sys) => {
            sys.refresh_memory();
            sys.total_memory() / 1024 / 1024
        }
        Err(_) => 0,
    };
    if mb == 0 {
        8192
    } else {
        mb.min(u32::MAX as u64) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_save_and_corrupt_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.json");
        let mut s = Settings::default();
        s.java_path = Some("C:/java".into());
        save_atomic(&p, &s).unwrap();
        let back: Settings = load_or_default(&p);
        assert_eq!(back.java_path.as_deref(), Some("C:/java"));
        assert!(!dir.path().join("config.json.tmp").exists());

        std::fs::write(&p, b"{ not json").unwrap();
        let recovered: Settings = load_or_default(&p);
        assert_eq!(recovered.schema_version, SCHEMA_VERSION);
        let backups = std::fs::read_dir(dir.path())
            .unwrap()
            .filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().contains(".bak-"))
            .count();
        assert_eq!(backups, 1);
    }

    #[test]
    fn instance_ids_are_unique_and_safe() {
        let mut f = InstancesFile::default();
        assert_eq!(f.add("My World!", "1.21.1").id, "my-world");
        assert_eq!(f.add("My World!", "1.21.1").id, "my-world-2");
        assert_eq!(f.add("中文", "1.21.1").id, "instance");
        assert!(Dirs::new("C:/d").instance("my-world").is_ok());
        assert!(Dirs::new("C:/d").instance("../x").is_err());
    }

    #[test]
    fn old_instances_json_without_loader_fields_loads() {
        let f: InstancesFile = serde_json::from_str(
            r#"{"schema_version":1,"instances":[{"id":"a","name":"A","version_id":"1.21.1"}]}"#,
        )
        .unwrap();
        let i = &f.instances[0];
        assert!(i.loader.is_none() && i.mc_version.is_none());
        assert_eq!(i.game_version(), "1.21.1");
    }

    #[test]
    fn add_with_loader_sets_profile_version_id() {
        use crate::loader::LoaderKind;
        let mut f = InstancesFile::default();
        let i = f.add_with_loader(
            "mods",
            "1.21.1",
            Some(LoaderRef { kind: LoaderKind::Fabric, version: "0.16.9".into() }),
        );
        assert_eq!(i.version_id, "fabric-loader-0.16.9-1.21.1");
        assert_eq!(i.mc_version.as_deref(), Some("1.21.1"));
        assert_eq!(i.game_version(), "1.21.1");
        let q = f.add_with_loader(
            "q",
            "1.21.1",
            Some(LoaderRef { kind: LoaderKind::Quilt, version: "0.27.1".into() }),
        );
        assert_eq!(q.version_id, "quilt-loader-0.27.1-1.21.1");
        let plain = f.add_with_loader("plain", "1.20.4", None);
        assert_eq!(plain.version_id, "1.20.4");
        assert_eq!(plain.game_version(), "1.20.4");
    }

    #[test]
    fn loader_ref_roundtrips_json() {
        use crate::loader::LoaderKind;
        let r = LoaderRef { kind: LoaderKind::Fabric, version: "0.16.9".into() };
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains(r#""kind":"fabric""#), "{s}");
        assert_eq!(serde_json::from_str::<LoaderRef>(&s).unwrap(), r);
    }
}
