//! Tauri shell around `launcher-core`. Everything touching files, network or
//! processes happens here in Rust; the web UI only calls these commands.

use launcher_core::account::{Account, OfflineAccount};
use launcher_core::config::{self, Dirs, Instance, InstancesFile, LoaderRef, RunningGame, Settings};
use launcher_core::download::{DownloadTask, Downloader, Progress, ProgressFn};
use launcher_core::loader::{self, LoaderKind, LoaderVersion};
use launcher_core::modpack::{self, PackVersion};
use launcher_core::mods::{self, ContentKind, InstallPlan, InstalledMod, ModrinthClient, ProjectDetail, ScanReport, SearchOpts, SearchResult};
use launcher_core::recommended::{self, Recommended};
use launcher_core::version::ResolvedVersion;
use launcher_core::{backup, diagnostics, doctor, install, java, launch, netcheck, precheck, screenshots, servers, share, stats, updater, worlds, Error as CoreError};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;
use tokio_util::sync::CancellationToken;

// ------------------------------------------------------------------- state

struct Inner {
    dirs: Dirs,
    settings: Mutex<Settings>,
    instances: Mutex<InstancesFile>,
    busy: Mutex<Option<CancellationToken>>,
    mica: bool,
}

struct AppState(Arc<Inner>);

type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

impl Inner {
    fn save_settings(&self) -> CmdResult<()> {
        let s = self.settings.lock().unwrap().clone();
        config::save_atomic(&self.dirs.settings_file(), &s).map_err(err)
    }
    fn save_instances(&self) -> CmdResult<()> {
        let i = self.instances.lock().unwrap().clone();
        config::save_atomic(&self.dirs.instances_file(), &i).map_err(err)
    }

    /// Forget "running" markers whose process is gone (or was reused by another pid owner).
    fn refresh_running(&self) {
        let mut changed = false;
        {
            let mut inst = self.instances.lock().unwrap();
            for i in inst.instances.iter_mut() {
                if let Some(r) = &i.running {
                    if !launch::is_game_running(r) {
                        i.running = None;
                        changed = true;
                    }
                }
            }
        }
        if changed {
            let _ = self.save_instances();
        }
    }

    fn instance(&self, id: &str) -> CmdResult<Instance> {
        self.instances
            .lock()
            .unwrap()
            .find(id)
            .cloned()
            .ok_or_else(|| format!("找不到实例 {id}"))
    }
}

/// Only one install/launch at a time; released on drop.
struct BusyGuard(Arc<Inner>);

impl Drop for BusyGuard {
    fn drop(&mut self) {
        *self.0.busy.lock().unwrap() = None;
    }
}

fn acquire_busy(inner: &Arc<Inner>) -> CmdResult<(BusyGuard, CancellationToken)> {
    let mut slot = inner.busy.lock().unwrap();
    if slot.is_some() {
        return Err("已有任务正在进行，请稍候".into());
    }
    let token = CancellationToken::new();
    *slot = Some(token.clone());
    Ok((BusyGuard(inner.clone()), token))
}

fn make_downloader(inner: &Inner, token: CancellationToken) -> Downloader {
    let s = inner.settings.lock().unwrap();
    Downloader::new(s.source).with_limits(s.max_downloads.clamp(4, 64), 12).with_cancel(token)
}

// -------------------------------------------------------------------- dtos

#[derive(Serialize)]
struct SystemInfo {
    max_mem_mb: u32,
    recommended_mem_mb: u32,
    mica: bool,
    /// The launcher's own version.
    version: String,
    /// Its GitHub address, when one is set.
    repo_url: Option<String>,
}

#[derive(Serialize)]
struct Snapshot {
    settings: Settings,
    instances: Vec<Instance>,
    system: SystemInfo,
    data_dir: String,
}

fn snapshot(inner: &Inner) -> Snapshot {
    inner.refresh_running();
    Snapshot {
        settings: inner.settings.lock().unwrap().clone(),
        instances: inner.instances.lock().unwrap().instances.clone(),
        system: SystemInfo {
            max_mem_mb: config::max_memory_mb(),
            recommended_mem_mb: config::recommended_memory_mb(),
            mica: inner.mica,
            version: env!("CARGO_PKG_VERSION").to_string(),
            repo_url: updater::parse_repo(env!("CARGO_PKG_REPOSITORY")).map(|(o, r)| format!("https://github.com/{o}/{r}")),
        },
        data_dir: inner.dirs.root().to_string_lossy().into_owned(),
    }
}

#[derive(Serialize)]
struct VersionDto {
    id: String,
    kind: String,
    release_time: String,
}

#[derive(Serialize)]
struct VersionList {
    latest_release: String,
    latest_snapshot: String,
    versions: Vec<VersionDto>,
}

#[derive(Serialize)]
struct JavaDto {
    major: u32,
    version: String,
}

#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum LaunchEvent {
    Stage { label: String },
    Progress { stage: String, files_done: u64, files_total: u64, bytes_done: u64, bytes_total: u64 },
    Log { line: String },
    Started { pid: u32 },
    /// The game's own window is on screen (the process starting is not the same thing).
    Ready,
    /// Something the user should know happened on the way to launching (shown as a toast).
    Notice { text: String },
    Exited { code: Option<i32>, crash_report: Option<String>, diagnosis: Option<doctor::Diagnosis> },
}

fn emit(ch: &Channel<LaunchEvent>, ev: LaunchEvent) {
    let _ = ch.send(ev);
}

fn progress_fn(ch: &Channel<LaunchEvent>) -> ProgressFn {
    let ch = ch.clone();
    Arc::new(move |p: &Progress| {
        emit(
            &ch,
            LaunchEvent::Progress {
                stage: p.stage.clone(),
                files_done: p.files_done,
                files_total: p.files_total,
                bytes_done: p.bytes_done,
                bytes_total: p.bytes_total,
            },
        )
    })
}

// ---------------------------------------------------------------- commands

#[tauri::command]
fn get_snapshot(state: State<AppState>) -> Snapshot {
    snapshot(&state.0)
}

#[tauri::command]
async fn list_versions(state: State<'_, AppState>) -> CmdResult<VersionList> {
    let dl = make_downloader(&state.0, CancellationToken::new());
    let m = install::fetch_manifest(&dl).await.map_err(err)?;
    let versions = m
        .supported()
        .into_iter()
        .map(|e| VersionDto { id: e.id.clone(), kind: e.kind.clone(), release_time: e.release_time.clone() })
        .collect();
    Ok(VersionList { latest_release: m.latest.release.clone(), latest_snapshot: m.latest.snapshot.clone(), versions })
}

#[tauri::command]
fn add_account(name: String, state: State<AppState>) -> CmdResult<Snapshot> {
    let name = name.trim().to_string();
    let account = OfflineAccount::new(&name).map_err(err)?;
    {
        let mut s = state.0.settings.lock().unwrap();
        if s.accounts.iter().any(|a| a.name.eq_ignore_ascii_case(&name)) {
            return Err("已存在同名账号".into());
        }
        s.accounts.push(account);
        s.selected_account = Some(name);
    }
    state.0.save_settings()?;
    Ok(snapshot(&state.0))
}

#[tauri::command]
fn remove_account(name: String, state: State<AppState>) -> CmdResult<Snapshot> {
    {
        let mut s = state.0.settings.lock().unwrap();
        s.accounts.retain(|a| a.name != name);
        if s.selected_account.as_deref() == Some(name.as_str()) {
            s.selected_account = s.accounts.first().map(|a| a.name.clone());
        }
    }
    state.0.save_settings()?;
    Ok(snapshot(&state.0))
}

#[tauri::command]
fn select_account(name: String, state: State<AppState>) -> CmdResult<Snapshot> {
    {
        let mut s = state.0.settings.lock().unwrap();
        if !s.accounts.iter().any(|a| a.name == name) {
            return Err("账号不存在".into());
        }
        s.selected_account = Some(name);
    }
    state.0.save_settings()?;
    Ok(snapshot(&state.0))
}

/// Only the user-editable fields are taken from the UI; accounts are managed by their own commands.
#[tauri::command]
fn save_settings(settings: Settings, state: State<AppState>) -> CmdResult<Snapshot> {
    {
        let mut s = state.0.settings.lock().unwrap();
        s.source = settings.source;
        s.java_path = settings.java_path.filter(|p| !p.trim().is_empty());
        s.max_downloads = settings.max_downloads.clamp(4, 64);
        s.default_memory_mb = settings.default_memory_mb;
        s.auto_backup = settings.auto_backup;
        s.check_updates = settings.check_updates;
    }
    state.0.save_settings()?;
    Ok(snapshot(&state.0))
}

#[tauri::command]
fn create_instance(
    name: String,
    version_id: String,
    loader: Option<LoaderRef>,
    state: State<AppState>,
) -> CmdResult<Snapshot> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 40 {
        return Err("实例名称需为 1-40 个字符".into());
    }
    // `version_id` is the Minecraft version; with a loader the instance launches its profile.
    let id = {
        let mut inst = state.0.instances.lock().unwrap();
        inst.add_with_loader(name, &version_id, loader).id.clone()
    };
    state.0.settings.lock().unwrap().selected_instance = Some(id);
    state.0.save_instances()?;
    state.0.save_settings()?;
    Ok(snapshot(&state.0))
}

#[tauri::command]
fn update_instance(instance: Instance, state: State<AppState>) -> CmdResult<Snapshot> {
    {
        let mut inst = state.0.instances.lock().unwrap();
        let cur = inst
            .instances
            .iter_mut()
            .find(|i| i.id == instance.id)
            .ok_or_else(|| "实例不存在".to_string())?;
        let name = instance.name.trim();
        if name.is_empty() {
            return Err("实例名称不能为空".into());
        }
        cur.name = name.to_string();
        cur.memory_mb = instance.memory_mb;
        cur.jvm_args = instance.jvm_args;
        cur.java_path = instance.java_path.filter(|p| !p.trim().is_empty());
        cur.width = instance.width;
        cur.height = instance.height;
    }
    state.0.save_instances()?;
    Ok(snapshot(&state.0))
}

#[tauri::command]
fn delete_instance(id: String, delete_files: bool, state: State<AppState>) -> CmdResult<Snapshot> {
    state.0.refresh_running();
    let inst = state.0.instance(&id)?;
    if inst.running.is_some() {
        return Err("游戏正在运行，无法删除".into());
    }
    if delete_files {
        let dir = state.0.dirs.instance(&id).map_err(err)?;
        if dir.is_dir() {
            std::fs::remove_dir_all(&dir).map_err(err)?;
        }
    }
    {
        state.0.instances.lock().unwrap().instances.retain(|i| i.id != id);
        let mut s = state.0.settings.lock().unwrap();
        if s.selected_instance.as_deref() == Some(id.as_str()) {
            s.selected_instance = None;
        }
    }
    state.0.save_instances()?;
    state.0.save_settings()?;
    Ok(snapshot(&state.0))
}

#[tauri::command]
fn select_instance(id: String, state: State<AppState>) -> CmdResult<Snapshot> {
    state.0.instance(&id)?;
    state.0.settings.lock().unwrap().selected_instance = Some(id);
    state.0.save_settings()?;
    Ok(snapshot(&state.0))
}

#[tauri::command]
async fn probe_java(path: String) -> CmdResult<JavaDto> {
    let info = java::probe_java(std::path::Path::new(path.trim())).await.map_err(err)?;
    Ok(JavaDto { major: info.major, version: info.version })
}

#[tauri::command]
fn open_game_dir(id: String, app: AppHandle, state: State<AppState>) -> CmdResult<()> {
    state.0.instance(&id)?;
    let dir = state.0.dirs.instance(&id).map_err(err)?;
    std::fs::create_dir_all(&dir).map_err(err)?;
    app.opener().open_path(dir.to_string_lossy(), None::<&str>).map_err(err)
}

#[tauri::command]
fn open_data_dir(app: AppHandle, state: State<AppState>) -> CmdResult<()> {
    let dir = state.0.dirs.root().to_path_buf();
    std::fs::create_dir_all(&dir).map_err(err)?;
    app.opener().open_path(dir.to_string_lossy(), None::<&str>).map_err(err)
}

#[tauri::command]
fn cancel_task(state: State<AppState>) {
    if let Some(t) = state.0.busy.lock().unwrap().as_ref() {
        t.cancel();
    }
}

#[tauri::command]
fn kill_game(id: String, state: State<AppState>) -> CmdResult<bool> {
    let inst = state.0.instance(&id)?;
    Ok(inst.running.as_ref().map(launch::kill_game).unwrap_or(false))
}

/// Download a version without launching it.
#[tauri::command]
async fn install_version(version_id: String, on_event: Channel<LaunchEvent>, state: State<'_, AppState>) -> CmdResult<()> {
    let (_guard, token) = acquire_busy(&state.0)?;
    let dl = make_downloader(&state.0, token);
    emit(&on_event, LaunchEvent::Stage { label: format!("下载 {version_id}") });
    install::install_version(&dl, &state.0.dirs, &version_id, Some(progress_fn(&on_event)))
        .await
        .map_err(err)?;
    Ok(())
}

/// Java for `resolved`: the instance's or global path, else Mojang's runtime
/// for the version (downloaded on first use).
async fn resolve_java(
    dl: &Downloader,
    dirs: &Dirs,
    custom: Option<String>,
    resolved: &ResolvedVersion,
    ch: &Channel<LaunchEvent>,
) -> Result<(PathBuf, u32), CoreError> {
    let required = resolved.json.java_version.as_ref().map(|j| j.major_version).unwrap_or(java::LEGACY_MAJOR);
    match custom.filter(|p| !p.trim().is_empty()) {
        Some(p) => {
            let info = java::probe_java(std::path::Path::new(&p)).await?;
            if info.major < required {
                emit(
                    ch,
                    LaunchEvent::Log {
                        line: format!("[启动器] 警告: 该版本需要 Java {required}，当前 Java 为 {}，可能无法启动", info.major),
                    },
                );
            }
            Ok((PathBuf::from(p), info.major))
        }
        None => {
            let component = resolved
                .json
                .java_version
                .as_ref()
                .map(|j| j.component.clone())
                .unwrap_or_else(|| java::LEGACY_COMPONENT.to_string());
            emit(ch, LaunchEvent::Stage { label: "准备 Java 运行时".into() });
            let p = java::ensure_runtime(dl, dirs, &component, Some(progress_fn(ch))).await?;
            Ok((p, required))
        }
    }
}

fn custom_java(inst: &Instance, global: &Option<String>) -> Option<String> {
    inst.java_path.clone().or(global.clone()).filter(|p| !p.trim().is_empty())
}

/// Make sure everything an instance needs is on disk: the vanilla version, or
/// its loader plus the vanilla version underneath. Forge and NeoForge run
/// their installer steps with the Java of the Minecraft version.
async fn ensure_instance_version(
    dl: &Downloader,
    dirs: &Dirs,
    inst: &Instance,
    global_java: &Option<String>,
    ch: &Channel<LaunchEvent>,
) -> Result<ResolvedVersion, CoreError> {
    let progress = Some(progress_fn(ch));
    let resolved = match &inst.loader {
        Some(l) if l.kind.runs_installer() => {
            let vanilla = install::install_version(dl, dirs, inst.game_version(), progress.clone()).await?;
            let (java_path, _) = resolve_java(dl, dirs, custom_java(inst, global_java), &vanilla, ch).await?;
            emit(ch, LaunchEvent::Stage { label: format!("安装 {} {}", l.kind.label(), l.version) });
            loader::install_loader(dl, dirs, l.kind, inst.game_version(), &l.version, Some(&java_path), progress.clone()).await?
        }
        Some(l) => loader::install_loader(dl, dirs, l.kind, inst.game_version(), &l.version, None, progress.clone()).await?,
        None => install::install_version(dl, dirs, &inst.version_id, progress.clone()).await?,
    };
    // A modpack instance whose install was interrupted (crash, launcher closed) is finished here,
    // before anything launches it half-empty.
    if modpack::pending_pack(dirs, &inst.id)?.is_some() {
        emit(ch, LaunchEvent::Stage { label: "继续安装整合包文件".into() });
        modpack::resume_pending(dl, dirs, &inst.id, progress).await?;
    }
    Ok(resolved)
}

/// Download an instance's game files (and loader) without launching it.
#[tauri::command]
async fn install_instance(id: String, on_event: Channel<LaunchEvent>, state: State<'_, AppState>) -> CmdResult<()> {
    let inst = state.0.instance(&id)?;
    let (_guard, token) = acquire_busy(&state.0)?;
    let dl = make_downloader(&state.0, token);
    emit(&on_event, LaunchEvent::Stage { label: format!("下载 {}", inst.version_id) });
    let global_java = state.0.settings.lock().unwrap().java_path.clone();
    ensure_instance_version(&dl, &state.0.dirs, &inst, &global_java, &on_event).await.map_err(err)?;
    Ok(())
}

/// Loader versions for a Minecraft version, newest first; empty = unsupported.
#[tauri::command]
async fn list_loader_versions(kind: LoaderKind, mc_version: String, state: State<'_, AppState>) -> CmdResult<Vec<LoaderVersion>> {
    let dl = make_downloader(&state.0, CancellationToken::new());
    loader::list_loader_versions(&dl, kind, &mc_version).await.map_err(err)
}

// -------------------------------------------------------------------- mods

fn require_loader(inst: &Instance) -> CmdResult<LoaderRef> {
    inst.loader.clone().ok_or_else(|| "当前实例是原版，没有加载器，无法安装模组".to_string())
}

fn require_stopped(inst: &Instance) -> CmdResult<()> {
    if inst.running.is_some() {
        Err("游戏正在运行，请先关闭游戏再修改模组".into())
    } else {
        Ok(())
    }
}

/// The plan comes back from the web UI, so re-check it before downloading:
/// files may only come from the Modrinth CDN over https and need a sha1.
fn validate_plan(plan: &InstallPlan) -> CmdResult<()> {
    for f in &plan.files {
        let from_cdn = f
            .url
            .strip_prefix("https://cdn.modrinth.com/")
            .is_some_and(|rest| !rest.is_empty() && !rest.contains('@'));
        if !from_cdn {
            return Err(format!("拒绝下载来源不明的文件: {}", f.url));
        }
        if f.sha1.len() != 40 || !f.sha1.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!("{} 缺少有效的 sha1 校验值", f.filename));
        }
    }
    Ok(())
}

/// The memory the instance asks for: its own setting, else the default, within what this machine can have.
fn wanted_memory_mb(inst: &Instance, settings: &Settings) -> u32 {
    let max = config::max_memory_mb();
    inst.memory_mb.or(settings.default_memory_mb).unwrap_or_else(config::recommended_memory_mb).clamp(512, max.max(512))
}

/// Run blocking file work off the UI thread.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> launcher_core::Result<T> + Send + 'static) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?.map_err(err)
}

/// The instance, and what mods it can run.
fn mod_context(inner: &Inner, id: &str) -> CmdResult<(Instance, mods::Target)> {
    inner.refresh_running();
    let inst = inner.instance(id)?;
    let l = require_loader(&inst)?;
    let target = mods::target_for(inst.game_version(), l.kind);
    Ok((inst, target))
}

/// Loader used to pick files for `kind`: mods and shaders need the instance's
/// loader; resource packs work on any instance.
fn content_loader(inst: &Instance, kind: ContentKind) -> CmdResult<LoaderKind> {
    match (kind, &inst.loader) {
        (ContentKind::ResourcePack, l) => Ok(l.as_ref().map(|l| l.kind).unwrap_or(LoaderKind::Fabric)),
        _ => Ok(require_loader(inst)?.kind),
    }
}

fn content_context(inner: &Inner, id: &str, kind: ContentKind) -> CmdResult<(Instance, LoaderKind)> {
    inner.refresh_running();
    let inst = inner.instance(id)?;
    let loader = content_loader(&inst, kind)?;
    Ok((inst, loader))
}

#[tauri::command]
async fn content_search(instance_id: String, kind: ContentKind, query: String, offset: u32, opts: Option<SearchOpts>, state: State<'_, AppState>) -> CmdResult<SearchResult> {
    let (inst, loader) = content_context(&state.0, &instance_id, kind)?;
    let target = mods::content_target(inst.game_version(), loader, kind);
    ModrinthClient::official().search_with(&target, query.trim(), offset, &opts.unwrap_or_default()).await.map_err(err)
}

#[tauri::command]
async fn content_plan(instance_id: String, kind: ContentKind, project_id: String, state: State<'_, AppState>) -> CmdResult<InstallPlan> {
    let (inst, loader) = content_context(&state.0, &instance_id, kind)?;
    let installed_mods = mods::installed_project_ids(&state.0.dirs, &instance_id).map_err(err)?;
    let installed_content = mods::list_content(&state.0.dirs, &instance_id, kind)
        .map_err(err)?
        .into_iter()
        .filter_map(|m| m.project_id)
        .collect();
    mods::plan_content(&ModrinthClient::official(), loader, inst.game_version(), kind, &project_id, &installed_mods, &installed_content)
        .await
        .map_err(err)
}

#[tauri::command]
fn content_list(instance_id: String, kind: ContentKind, state: State<AppState>) -> CmdResult<Vec<InstalledMod>> {
    content_context(&state.0, &instance_id, kind)?;
    mods::list_content(&state.0.dirs, &instance_id, kind).map_err(err)
}

/// Search everything of `kind`, not limited to what one instance can run.
#[tauri::command]
async fn content_search_all(kind: ContentKind, query: String, offset: u32, opts: Option<SearchOpts>) -> CmdResult<SearchResult> {
    ModrinthClient::official().search_any_with(kind, query.trim(), offset, &opts.unwrap_or_default()).await.map_err(err)
}

/// A project page. With an instance, versions and prerequisites are those for it.
#[tauri::command]
async fn content_detail(project_id: String, kind: ContentKind, instance_id: Option<String>, state: State<'_, AppState>) -> CmdResult<ProjectDetail> {
    let client = ModrinthClient::official();
    let Some(id) = instance_id else {
        return client.project_detail(&project_id, None, &Default::default()).await.map_err(err);
    };
    let (inst, loader) = content_context(&state.0, &id, kind)?;
    let target = mods::content_target(inst.game_version(), loader, kind);
    let installed = mods::installed_project_ids(&state.0.dirs, &id).map_err(err)?;
    client.project_detail(&project_id, Some(&target), &installed).await.map_err(err)
}

/// Plan the prerequisites a scan found missing, so the user can confirm installing them.
#[tauri::command]
async fn content_plan_missing(instance_id: String, project_ids: Vec<String>, state: State<'_, AppState>) -> CmdResult<InstallPlan> {
    let (_, target) = mod_context(&state.0, &instance_id)?;
    let installed = mods::installed_project_ids(&state.0.dirs, &instance_id).map_err(err)?;
    let plan = recommended::plan_many(&ModrinthClient::official(), &target, &project_ids, &installed).await.map_err(err)?;
    validate_plan(&plan)?;
    Ok(plan)
}

/// Problems visible from the mod files alone, so launching can stop before a doomed start.
#[tauri::command]
async fn precheck_instance(instance_id: String, state: State<'_, AppState>) -> CmdResult<Vec<precheck::Problem>> {
    state.0.instance(&instance_id)?;
    let inner = state.0.clone();
    let inst = inner.instance(&instance_id)?;
    let wanted = wanted_memory_mb(&inst, &inner.settings.lock().unwrap());
    let mut problems = blocking(move || precheck::precheck(&inner.dirs, &instance_id)).await?;
    // The launcher lowers the heap by itself, but when almost nothing is free the game will crawl or die anyway.
    let free = doctor::available_memory_mb();
    if free > 0 && free < 1536 {
        let hogs: Vec<String> = doctor::top_memory_programs(3).into_iter().map(|(n, mb)| format!("{n}（{:.1} GB）", mb as f64 / 1024.0)).collect();
        problems.push(precheck::Problem {
            code: "low-memory",
            title: format!("电脑可用内存只剩 {:.1} GB", free as f64 / 1024.0),
            detail: format!(
                "游戏想用 {:.1} GB，启动器会自动降低，但太少会很卡甚至崩溃。{}",
                wanted as f64 / 1024.0,
                if hogs.is_empty() { String::new() } else { format!("占内存最多的程序：{}。关掉它们再启动效果最好。", hogs.join("、")) }
            ),
            fix: None,
        });
    }
    Ok(problems)
}

/// Disable all but the newest copy of every duplicated mod (reversible).
#[tauri::command]
fn mods_disable_duplicates(instance_id: String, state: State<AppState>) -> CmdResult<Vec<String>> {
    let (inst, _) = mod_context(&state.0, &instance_id)?;
    require_stopped(&inst)?;
    mods::disable_duplicates(&state.0.dirs, &instance_id).map_err(err)
}

/// Name files we did not install ourselves and look for newer versions (needs network).
#[tauri::command]
async fn content_scan(instance_id: String, kind: ContentKind, state: State<'_, AppState>) -> CmdResult<ScanReport> {
    let (inst, loader) = content_context(&state.0, &instance_id, kind)?;
    let target = mods::content_target(inst.game_version(), loader, kind);
    mods::scan_content(&ModrinthClient::official(), &state.0.dirs, &instance_id, kind, &target).await.map_err(err)
}

#[tauri::command]
fn content_remove(instance_id: String, kind: ContentKind, filename: String, state: State<AppState>) -> CmdResult<()> {
    let (inst, _) = content_context(&state.0, &instance_id, kind)?;
    require_stopped(&inst)?;
    mods::remove_content(&state.0.dirs, &instance_id, kind, &filename).map_err(err)
}

#[tauri::command]
async fn mods_install(instance_id: String, plan: InstallPlan, on_event: Channel<LaunchEvent>, state: State<'_, AppState>) -> CmdResult<()> {
    state.0.refresh_running();
    require_stopped(&state.0.instance(&instance_id)?)?;
    validate_plan(&plan)?;
    let (_guard, token) = acquire_busy(&state.0)?;
    let dl = make_downloader(&state.0, token);
    emit(&on_event, LaunchEvent::Stage { label: "下载".into() });
    mods::install_plan(&dl, &state.0.dirs, &instance_id, &plan, Some(progress_fn(&on_event))).await.map_err(err)
}

/// Mods worth offering for a new instance of this loader and Minecraft version.
#[tauri::command]
async fn recommended_mods(kind: LoaderKind, mc_version: String) -> CmdResult<Vec<Recommended>> {
    Ok(recommended::available(&ModrinthClient::official(), kind, &mc_version).await)
}

/// Install the mods the user ticked when creating the instance (dependencies included).
#[tauri::command]
async fn install_extras(instance_id: String, slugs: Vec<String>, on_event: Channel<LaunchEvent>, state: State<'_, AppState>) -> CmdResult<()> {
    if slugs.is_empty() {
        return Ok(());
    }
    let (inst, target) = mod_context(&state.0, &instance_id)?;
    require_stopped(&inst)?;
    let installed = mods::installed_project_ids(&state.0.dirs, &instance_id).map_err(err)?;
    let plan = recommended::plan_many(&ModrinthClient::official(), &target, &slugs, &installed).await.map_err(err)?;
    validate_plan(&plan)?;
    let (_guard, token) = acquire_busy(&state.0)?;
    let dl = make_downloader(&state.0, token);
    emit(&on_event, LaunchEvent::Stage { label: "下载模组".into() });
    mods::install_plan(&dl, &state.0.dirs, &instance_id, &plan, Some(progress_fn(&on_event))).await.map_err(err)
}

#[tauri::command]
fn mods_set_enabled(instance_id: String, filename: String, enabled: bool, state: State<AppState>) -> CmdResult<()> {
    let (inst, _) = mod_context(&state.0, &instance_id)?;
    require_stopped(&inst)?;
    mods::set_enabled(&state.0.dirs, &instance_id, &filename, enabled).map_err(err)
}

// --------------------------------------------------------------- modpacks

/// The instance name for a modpack: what the user typed, else the pack's own name; at most 40 characters.
fn pack_instance_name(requested: &str, pack_name: &str) -> String {
    let t = requested.trim();
    let t = if t.is_empty() { pack_name.trim() } else { t };
    t.chars().take(40).collect::<String>().trim().to_string()
}

/// The version comes back from the web UI: only a `.mrpack` on the Modrinth CDN with a sha1 may be downloaded.
fn validate_pack_version(v: &PackVersion) -> CmdResult<()> {
    let f = &v.file;
    let from_cdn = f.url.strip_prefix("https://cdn.modrinth.com/").is_some_and(|rest| !rest.is_empty() && !rest.contains('@'));
    if !from_cdn {
        return Err(format!("拒绝下载来源不明的整合包: {}", f.url));
    }
    if f.sha1.len() != 40 || !f.sha1.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("整合包缺少有效的 sha1 校验值".into());
    }
    let name_ok = f.filename.to_ascii_lowercase().ends_with(".mrpack") && !f.filename.contains(['/', '\\', ':']) && !f.filename.contains("..");
    if !name_ok {
        return Err(format!("整合包文件名无效: {}", f.filename));
    }
    Ok(())
}

#[tauri::command]
async fn modpack_search(query: String, offset: u32) -> CmdResult<SearchResult> {
    ModrinthClient::official().search_modpacks(query.trim(), offset).await.map_err(err)
}

#[tauri::command]
async fn modpack_versions(project_id: String) -> CmdResult<Vec<PackVersion>> {
    ModrinthClient::official().pack_versions(&project_id).await.map_err(err)
}

/// Install a modpack as a new, fully set-up instance. If anything fails the
/// new instance (record and folder) is removed again.
#[tauri::command]
async fn install_modpack(name: String, version: PackVersion, on_event: Channel<LaunchEvent>, state: State<'_, AppState>) -> CmdResult<Snapshot> {
    validate_pack_version(&version)?;
    let inner = state.0.clone();
    let (_guard, token) = acquire_busy(&inner)?;
    let dl = make_downloader(&inner, token);
    let dirs = &inner.dirs;

    emit(&on_event, LaunchEvent::Stage { label: "下载整合包".into() });
    let file = &version.file;
    let task = DownloadTask::rel(&dirs.shared().join("modpacks"), &format!("{}.mrpack", file.sha1.to_ascii_lowercase()), file.url.clone())
        .map_err(err)?
        .sha1(Some(file.sha1.clone()))
        .size(Some(file.size).filter(|n| *n > 0));
    let pack_path = task.dest.clone();
    dl.download_all(vec![task], "整合包", Some(progress_fn(&on_event))).await.map_err(err)?;
    let idx = modpack::read_index(&pack_path).map_err(err)?;
    let target = modpack::pack_target(&idx).map_err(err)?;
    let base_name = pack_instance_name(&name, &idx.name);
    if base_name.is_empty() {
        return Err("实例名称不能为空".into());
    }

    // A fresh instance: never adopt a folder that is already there (an old instance's files may live in it).
    let loader_ref = target.loader.as_ref().map(|(kind, version)| LoaderRef { kind: *kind, version: version.clone() });
    let mut created = None;
    for n in 1..=50 {
        let display = if n == 1 { base_name.clone() } else { format!("{base_name} {n}") };
        let inst = inner.instances.lock().unwrap().add_with_loader(&display, &target.mc, loader_ref.clone()).clone();
        if dirs.instance(&inst.id).map_err(err)?.exists() {
            inner.instances.lock().unwrap().instances.retain(|i| i.id != inst.id);
            continue;
        }
        created = Some(inst);
        break;
    }
    let inst = created.ok_or_else(|| "找不到可用的实例名称".to_string())?;
    inner.save_instances()?;

    // The marker stays until every file is in place: `ensure_instance_version` installs the game and
    // the loader, then unpacks the pack, and clears it. Closing the launcher half-way leaves it
    // behind, and the next launch of this instance finishes the job.
    let result: CmdResult<()> = async {
        modpack::mark_pending(dirs, &inst.id, &pack_path).map_err(err)?;
        let global_java = inner.settings.lock().unwrap().java_path.clone();
        ensure_instance_version(&dl, dirs, &inst, &global_java, &on_event).await.map_err(err)?;
        Ok(())
    }
    .await;
    if let Err(e) = result {
        inner.instances.lock().unwrap().instances.retain(|i| i.id != inst.id);
        let _ = inner.save_instances();
        if let Ok(dir) = dirs.instance(&inst.id) {
            let _ = std::fs::remove_dir_all(dir);
        }
        return Err(e);
    }
    inner.settings.lock().unwrap().selected_instance = Some(inst.id.clone());
    inner.save_settings()?;
    Ok(snapshot(&inner))
}

/// Where to go when the game is up: a world of the instance, or a server.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum QuickPlayArg {
    World { name: String },
    Server { host: String, port: u16 },
}

/// Install (if needed) and start an instance. Returns once the game process
/// is running; a background task then streams its log and reports the exit.
#[tauri::command]
async fn launch_instance(id: String, quick_play: Option<QuickPlayArg>, on_event: Channel<LaunchEvent>, state: State<'_, AppState>) -> CmdResult<()> {
    let inner = state.0.clone();
    inner.refresh_running();
    let inst = inner.instance(&id)?;
    if inst.running.is_some() {
        return Err("该实例已在运行".into());
    }
    let (_guard, token) = acquire_busy(&inner)?;
    let quick = quick_play.map(|q| match q {
        QuickPlayArg::World { name } => launch::QuickPlay::World(name),
        QuickPlayArg::Server { host, port } => launch::QuickPlay::Server { host, port },
    });
    let launched = prepare_and_spawn(&inner, &inst, &on_event, token, quick).await.map_err(err)?;

    emit(&on_event, LaunchEvent::Started { pid: launched.pid });
    {
        let mut all = inner.instances.lock().unwrap();
        if let Some(i) = all.instances.iter_mut().find(|i| i.id == id) {
            i.running = Some(launched.running.clone());
        }
    }
    inner.save_instances()?;

    let game_dir = inner.dirs.instance(&id).map_err(err)?;
    tauri::async_runtime::spawn(monitor(
        inner.clone(),
        id,
        game_dir,
        Some(launched.child),
        launched.running,
        launched.stdio_log,
        on_event,
        true,
    ));
    Ok(())
}

/// Re-attach to a game that kept running while the launcher was closed.
#[tauri::command]
async fn attach_game(id: String, on_event: Channel<LaunchEvent>, state: State<'_, AppState>) -> CmdResult<bool> {
    let inner = state.0.clone();
    inner.refresh_running();
    let inst = inner.instance(&id)?;
    let Some(running) = inst.running else { return Ok(false) };
    let game_dir = inner.dirs.instance(&id).map_err(err)?;
    let stdio = game_dir.join("launcher-stdio.log");
    emit(&on_event, LaunchEvent::Started { pid: running.pid });
    tauri::async_runtime::spawn(monitor(inner.clone(), id, game_dir, None, running, stdio, on_event, false));
    Ok(true)
}

async fn prepare_and_spawn(
    inner: &Arc<Inner>,
    inst: &Instance,
    ch: &Channel<LaunchEvent>,
    token: CancellationToken,
    quick: Option<launch::QuickPlay>,
) -> Result<launch::Launched, CoreError> {
    let settings = inner.settings.lock().unwrap().clone();
    let account = settings
        .selected_account
        .as_ref()
        .and_then(|n| settings.accounts.iter().find(|a| &a.name == n))
        .or_else(|| settings.accounts.first())
        .cloned()
        .ok_or_else(|| CoreError::Launch("请先在「账号」页添加一个玩家名".into()))?;

    let dl = make_downloader(inner, token);
    let dirs = &inner.dirs;

    emit(ch, LaunchEvent::Stage { label: "检查游戏文件".into() });
    let resolved = ensure_instance_version(&dl, dirs, inst, &settings.java_path, ch).await?;

    // Java: per-instance path, global path, or Mojang's runtime for this version.
    let (java_path, java_major) = resolve_java(&dl, dirs, custom_java(inst, &settings.java_path), &resolved, ch).await?;

    emit(ch, LaunchEvent::Stage { label: "准备启动".into() });
    let game_dir = dirs.instance(&inst.id)?;
    std::fs::create_dir_all(&game_dir)?;
    if quick.is_some() {
        std::fs::create_dir_all(game_dir.join("quickPlay"))?;
    }
    let natives = launch::prepare_natives(dirs, &resolved)?;
    let index_id = resolved.json.asset_index.as_ref().map(|a| a.id.clone()).unwrap_or_default();
    let index = install::load_asset_index(dirs, &index_id)?;
    let game_assets = launch::prepare_assets(dirs, &index_id, &index, &game_dir)?;

    // Disk: a full drive makes the game die half-way through writing worlds/logs.
    if let Some(free) = doctor::free_disk_mb(&game_dir) {
        if free < 300 {
            return Err(CoreError::Launch(format!("磁盘空间不足（仅剩 {free} MB），请清理后再启动")));
        }
        if free < 1500 {
            emit(ch, LaunchEvent::Log { line: format!("[启动器] 磁盘剩余空间较少（{free} MB），建议清理") });
        }
    }

    // Worlds first: a crash or a corrupted world can then be rolled back from the "存档" page.
    if settings.auto_backup {
        emit(ch, LaunchEvent::Stage { label: "备份存档".into() });
        let (inner2, id, ch2) = (inner.clone(), inst.id.clone(), ch.clone());
        let done = tauri::async_runtime::spawn_blocking(move || {
            let tick = std::sync::Mutex::new(std::time::Instant::now() - Duration::from_secs(1));
            let report = |done: u64, total: u64| {
                let mut t = tick.lock().unwrap();
                if done >= total || t.elapsed() >= Duration::from_millis(120) {
                    *t = std::time::Instant::now();
                    emit(&ch2, LaunchEvent::Progress { stage: "备份".into(), files_done: 0, files_total: 0, bytes_done: done, bytes_total: total });
                }
            };
            backup::create(&inner2.dirs, &id, backup::Scope::Worlds, backup::Kind::Auto, "启动前自动备份", Some(&report))
        })
        .await;
        match done {
            Ok(Ok(Some(b))) => emit(ch, LaunchEvent::Log { line: format!("[启动器] 已备份存档（{} 个世界，{:.1} MB），出问题可以在「存档」页回退", b.worlds.len(), b.size as f64 / 1048576.0) }),
            Ok(Ok(None)) => {}
            // A failed backup must never stop the game from starting.
            Ok(Err(e)) => emit(ch, LaunchEvent::Notice { text: format!("存档备份失败，将继续启动：{e}") }),
            Err(_) => {}
        }
    }

    // Memory: honour the setting, but never ask for more than is free right now.
    let max = config::max_memory_mb();
    let wanted = wanted_memory_mb(inst, &settings);
    let free = doctor::available_memory_mb();
    let memory = doctor::safe_memory_mb(wanted, free, max);
    if memory < wanted {
        emit(
            ch,
            LaunchEvent::Log { line: format!("[启动器] 可用内存不足，本次将内存从 {wanted} MB 降为 {memory} MB，避免启动失败") },
        );
        emit(
            ch,
            LaunchEvent::Notice {
                text: format!("电脑当前只剩 {:.1} GB 可用内存，已自动把游戏内存从 {:.1} GB 调到 {:.1} GB（只影响这一次）", free as f64 / 1024.0, wanted as f64 / 1024.0, memory as f64 / 1024.0),
            },
        );
    }

    let plan = launch::build_plan(&launch::LaunchRequest {
        dirs,
        resolved: &resolved,
        account: &account,
        java: java_path,
        java_major,
        game_dir: game_dir.clone(),
        game_assets,
        natives_dir: natives,
        memory_mb: memory,
        custom_jvm_args: &inst.jvm_args,
        width: inst.width,
        height: inst.height,
        quick_play: quick,
    })?;

    // Start from a clean log so the tail does not replay the previous session.
    let _ = std::fs::remove_file(launch::latest_log_path(&game_dir));
    emit(ch, LaunchEvent::Stage { label: format!("启动游戏（{}）", account.player_name()) });
    launch::spawn(&plan)
}

// ------------------------------------------------------------ log + monitor

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// True once `pid` owns a visible, titled top-level window (GLFW's game window).
#[cfg(windows)]
fn game_window_visible(pid: u32) -> bool {
    use std::ffi::c_void;
    type Hwnd = *mut c_void;
    #[link(name = "user32")]
    extern "system" {
        fn EnumWindows(cb: unsafe extern "system" fn(Hwnd, isize) -> i32, lparam: isize) -> i32;
        fn GetWindowThreadProcessId(h: Hwnd, pid: *mut u32) -> u32;
        fn IsWindowVisible(h: Hwnd) -> i32;
        fn GetWindowTextLengthW(h: Hwnd) -> i32;
    }
    struct Ctx {
        pid: u32,
        found: bool,
    }
    unsafe extern "system" fn each(h: Hwnd, lparam: isize) -> i32 {
        let ctx = &mut *(lparam as *mut Ctx);
        let mut owner = 0u32;
        GetWindowThreadProcessId(h, &mut owner);
        if owner == ctx.pid && IsWindowVisible(h) != 0 && GetWindowTextLengthW(h) > 0 {
            ctx.found = true;
            return 0;
        }
        1
    }
    let mut ctx = Ctx { pid, found: false };
    unsafe {
        EnumWindows(each, &mut ctx as *mut Ctx as isize);
    }
    ctx.found
}

#[cfg(not(windows))]
fn game_window_visible(_pid: u32) -> bool {
    true
}

struct LogTail {
    path: PathBuf,
    offset: u64,
    partial: String,
    seen_any: bool,
}

impl LogTail {
    fn new(path: PathBuf, offset: u64) -> Self {
        Self { path, offset, partial: String::new(), seen_any: false }
    }

    /// New complete lines since the last poll. Handles a missing file and truncation/rotation.
    async fn poll(&mut self) -> Vec<String> {
        use tokio::io::{AsyncReadExt, AsyncSeekExt};
        let Ok(mut f) = tokio::fs::File::open(&self.path).await else { return vec![] };
        let Ok(meta) = f.metadata().await else { return vec![] };
        if meta.len() < self.offset {
            self.offset = 0;
            self.partial.clear();
        }
        if meta.len() == self.offset {
            return vec![];
        }
        if f.seek(std::io::SeekFrom::Start(self.offset)).await.is_err() {
            return vec![];
        }
        let mut buf = Vec::new();
        let want = (meta.len() - self.offset).min(256 * 1024);
        if (&mut f).take(want).read_to_end(&mut buf).await.is_err() {
            return vec![];
        }
        self.offset += buf.len() as u64;
        self.partial.push_str(&String::from_utf8_lossy(&buf));
        let mut lines = Vec::new();
        while let Some(pos) = self.partial.find('\n') {
            let line: String = self.partial.drain(..=pos).collect();
            lines.push(line.trim_end_matches(['\r', '\n']).to_string());
        }
        if !lines.is_empty() {
            self.seen_any = true;
        }
        lines
    }
}

/// Stream `logs/latest.log`, watch the process and report how it ended.
/// `launcher-stdio.log` is only shown when the game never produced a log
/// (e.g. the JVM failed to start), so lines are not duplicated.
#[allow(clippy::too_many_arguments)]
async fn monitor(
    inner: Arc<Inner>,
    id: String,
    game_dir: PathBuf,
    mut child: Option<std::process::Child>,
    running: RunningGame,
    stdio_log: PathBuf,
    ch: Channel<LaunchEvent>,
    fresh: bool,
) {
    let latest = launch::latest_log_path(&game_dir);
    let start_offset = if fresh { 0 } else { tokio::fs::metadata(&latest).await.map(|m| m.len()).unwrap_or(0) };
    let mut tail = LogTail::new(latest, start_offset);
    let started = std::time::SystemTime::now();
    let began = std::time::Instant::now();
    let mut ready = false;
    let mut ready_at: Option<u64> = None;
    let mut code: Option<i32> = None;

    loop {
        for line in tail.poll().await {
            emit(&ch, LaunchEvent::Log { line });
        }
        // Tell the UI when the window really exists; give up waiting after 2 minutes.
        if !ready && (game_window_visible(running.pid) || began.elapsed() > Duration::from_secs(120)) {
            ready = true;
            ready_at = Some(now_secs());
            emit(&ch, LaunchEvent::Ready);
        }
        let exited = match child.as_mut() {
            Some(c) => match c.try_wait() {
                Ok(Some(status)) => {
                    code = status.code();
                    true
                }
                Ok(None) => false,
                Err(_) => true,
            },
            None => !launch::is_game_running(&running),
        };
        if exited {
            break;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    // Drain whatever was written right before exit.
    tokio::time::sleep(Duration::from_millis(200)).await;
    for line in tail.poll().await {
        emit(&ch, LaunchEvent::Log { line });
    }
    if !tail.seen_any {
        if let Ok(text) = tokio::fs::read_to_string(&stdio_log).await {
            let lines: Vec<&str> = text.lines().collect();
            for l in lines.iter().skip(lines.len().saturating_sub(200)) {
                emit(&ch, LaunchEvent::Log { line: l.to_string() });
            }
        }
    }

    let crashed = code != Some(0);
    // Play time runs from the window appearing; for a game we re-attached to, from when its process began.
    if let Some(start) = if fresh { ready_at } else { Some(running.start_time) } {
        let secs = now_secs().saturating_sub(start);
        let _ = stats::record(&inner.dirs, stats::Session { instance_id: id.clone(), start, secs, crashed });
    }
    let crash_report = if crashed {
        launch::latest_crash_report(&game_dir)
            .filter(|p| {
                p.metadata().and_then(|m| m.modified()).map(|t| t >= started - Duration::from_secs(5)).unwrap_or(true)
                    || !fresh
            })
            .map(|p| p.to_string_lossy().into_owned())
    } else {
        None
    };

    // Explain the failure: crash report first, then the game log, then raw stdout.
    let diagnosis = if crashed {
        let read = |p: PathBuf| async move { tokio::fs::read_to_string(p).await.unwrap_or_default() };
        let report = match &crash_report {
            Some(p) => read(PathBuf::from(p)).await,
            None => String::new(),
        };
        let log = read(launch::latest_log_path(&game_dir)).await;
        let stdio = read(stdio_log.clone()).await;
        doctor::diagnose_all(&[&report, &log, &stdio])
    } else {
        None
    };

    {
        let mut all = inner.instances.lock().unwrap();
        if let Some(i) = all.instances.iter_mut().find(|i| i.id == id) {
            i.running = None;
        }
    }
    let _ = inner.save_instances();
    emit(&ch, LaunchEvent::Exited { code, crash_report, diagnosis });
}

#[tauri::command]
async fn read_text_file(path: String, state: State<'_, AppState>) -> CmdResult<String> {
    // Only crash reports / logs inside the data directory may be read.
    let p = std::fs::canonicalize(&path).map_err(err)?;
    let root = std::fs::canonicalize(state.0.dirs.root()).map_err(err)?;
    if !p.starts_with(&root) {
        return Err("拒绝读取数据目录之外的文件".into());
    }
    let text = tokio::fs::read_to_string(&p).await.map_err(err)?;
    Ok(text.chars().take(200_000).collect())
}

// ------------------------------------------------ worlds, backups, statistics

fn stopped_for(inner: &Inner, id: &str, what: &str) -> CmdResult<()> {
    inner.refresh_running();
    if inner.instance(id)?.running.is_some() {
        return Err(format!("游戏正在运行，请先关闭游戏再{what}"));
    }
    Ok(())
}

#[tauri::command]
async fn worlds_list(instance_id: String, state: State<'_, AppState>) -> CmdResult<Vec<worlds::World>> {
    state.0.instance(&instance_id)?;
    let inner = state.0.clone();
    blocking(move || worlds::list_worlds(&inner.dirs, &instance_id)).await
}

#[tauri::command]
async fn world_delete(instance_id: String, world: String, state: State<'_, AppState>) -> CmdResult<()> {
    stopped_for(&state.0, &instance_id, "删除存档")?;
    let inner = state.0.clone();
    blocking(move || worlds::delete_world(&inner.dirs, &instance_id, &world)).await
}

#[tauri::command]
fn world_open(instance_id: String, world: String, app: AppHandle, state: State<AppState>) -> CmdResult<()> {
    state.0.instance(&instance_id)?;
    let saves = state.0.dirs.instance(&instance_id).map_err(err)?.join("saves");
    let dir = launcher_core::paths::safe_join(&saves, &world).map_err(err)?;
    if !dir.is_dir() {
        return Err("存档不存在".into());
    }
    app.opener().open_path(dir.to_string_lossy(), None::<&str>).map_err(err)
}

#[tauri::command]
async fn backup_list(instance_id: String, state: State<'_, AppState>) -> CmdResult<Vec<backup::BackupInfo>> {
    state.0.instance(&instance_id)?;
    let inner = state.0.clone();
    blocking(move || backup::list(&inner.dirs, &instance_id)).await
}

#[tauri::command]
async fn backup_create(instance_id: String, scope: backup::Scope, reason: Option<String>, state: State<'_, AppState>) -> CmdResult<Option<backup::BackupInfo>> {
    state.0.instance(&instance_id)?;
    let inner = state.0.clone();
    let reason = reason.filter(|r| !r.trim().is_empty()).unwrap_or_else(|| "手动备份".into());
    blocking(move || backup::create(&inner.dirs, &instance_id, scope, backup::Kind::Manual, &reason.chars().take(60).collect::<String>(), None)).await
}

/// Roll back everything a backup covers, or one world of it. The current state is backed up first.
#[tauri::command]
async fn backup_restore(instance_id: String, backup_id: String, world: Option<String>, state: State<'_, AppState>) -> CmdResult<()> {
    stopped_for(&state.0, &instance_id, "回退")?;
    let inner = state.0.clone();
    blocking(move || match world {
        Some(w) => backup::restore_world(&inner.dirs, &instance_id, &backup_id, &w),
        None => backup::restore(&inner.dirs, &instance_id, &backup_id),
    })
    .await
}

#[tauri::command]
async fn backup_delete(instance_id: String, backup_id: String, state: State<'_, AppState>) -> CmdResult<()> {
    state.0.instance(&instance_id)?;
    let inner = state.0.clone();
    blocking(move || backup::delete(&inner.dirs, &instance_id, &backup_id)).await
}

#[tauri::command]
async fn stats_sessions(state: State<'_, AppState>) -> CmdResult<Vec<stats::Session>> {
    let inner = state.0.clone();
    blocking(move || Ok(stats::sessions(&inner.dirs))).await
}

#[derive(Serialize)]
struct WorldStat {
    instance_id: String,
    dir: String,
    name: String,
    play_secs: u64,
}

/// Play time per world, over every instance, as the game counted it.
#[tauri::command]
async fn stats_worlds(state: State<'_, AppState>) -> CmdResult<Vec<WorldStat>> {
    let inner = state.0.clone();
    let ids: Vec<String> = inner.instances.lock().unwrap().instances.iter().map(|i| i.id.clone()).collect();
    blocking(move || {
        let mut out = Vec::new();
        for id in ids {
            for w in worlds::world_times(&inner.dirs, &id)? {
                if w.play_secs > 0 {
                    out.push(WorldStat { instance_id: id.clone(), dir: w.dir, name: w.name, play_secs: w.play_secs });
                }
            }
        }
        Ok(out)
    })
    .await
}

// ------------------------------------------------------------------ sharing

/// A copy of an instance: same version, loader, settings and files (worlds only when asked).
#[tauri::command]
async fn clone_instance(id: String, name: String, include_saves: bool, state: State<'_, AppState>) -> CmdResult<Snapshot> {
    let name = name.trim().to_string();
    if name.is_empty() || name.chars().count() > 40 {
        return Err("名称需要 1–40 个字符".into());
    }
    stopped_for(&state.0, &id, "复制")?;
    let inner = state.0.clone();
    let mut working = inner.instances.lock().unwrap().clone();
    let (dirs_inner, from) = (inner.clone(), id.clone());
    let copy = blocking(move || share::clone_instance(&dirs_inner.dirs, &mut working, &from, &name, include_saves)).await?;
    inner.instances.lock().unwrap().instances.push(copy);
    inner.save_instances()?;
    Ok(snapshot(&inner))
}

#[derive(Serialize)]
struct PackExport {
    path: String,
    folder: String,
    linked: usize,
    bundled: usize,
    size: u64,
}

/// Export an instance as a `.mrpack` into Downloads and show it in Explorer.
#[tauri::command]
async fn export_instance_pack(id: String, name: String, version: String, include_saves: bool, include_options: bool, app: AppHandle, state: State<'_, AppState>) -> CmdResult<PackExport> {
    let inst = state.0.instance(&id)?;
    let name = name.trim().to_string();
    if name.is_empty() || name.chars().count() > 60 {
        return Err("整合包名称需要 1–60 个字符".into());
    }
    let version = if version.trim().is_empty() { "1.0.0".to_string() } else { version.trim().chars().take(30).collect() };
    let out_dir = diagnostics::default_output_dir(state.0.dirs.root());
    let opts = share::ExportOptions { name, version, summary: None, include_saves, include_options };
    let r = share::export_mrpack(&ModrinthClient::official(), &state.0.dirs, &inst, &opts, &out_dir).await.map_err(err)?;
    let _ = app.opener().reveal_item_in_dir(&r.path);
    Ok(PackExport { path: r.path.to_string_lossy().into_owned(), folder: out_dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), linked: r.linked, bundled: r.bundled, size: r.size })
}

// ----------------------------------------------------------------- updates

#[derive(Serialize)]
struct UpdateCheck {
    /// The repository address is set, so checking is possible at all.
    configured: bool,
    current: String,
    update: Option<updater::UpdateInfo>,
}

/// Ask GitHub whether a newer launcher exists. The automatic check at startup (`manual = false`)
/// respects the settings switch and a version the user chose to skip; a manual one never does.
/// Nothing about the user is sent: it is one anonymous GET.
#[tauri::command]
async fn check_update(manual: bool, state: State<'_, AppState>) -> CmdResult<UpdateCheck> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let Some((owner, repo)) = updater::parse_repo(env!("CARGO_PKG_REPOSITORY")) else {
        return Ok(UpdateCheck { configured: false, current, update: None });
    };
    let (enabled, ignored) = {
        let s = state.0.settings.lock().unwrap();
        (s.check_updates, s.ignored_update.clone())
    };
    if !manual && !enabled {
        return Ok(UpdateCheck { configured: true, current, update: None });
    }
    let found = updater::check(updater::GITHUB_API, (&owner, &repo), &current, Duration::from_secs(8)).await.map_err(err)?;
    let update = found.filter(|u| manual || ignored.as_deref() != Some(u.version.as_str()));
    Ok(UpdateCheck { configured: true, current, update })
}

/// "Skip this version": the startup check stays quiet until a newer one comes out.
#[tauri::command]
fn ignore_update(version: String, state: State<AppState>) -> CmdResult<()> {
    state.0.settings.lock().unwrap().ignored_update = Some(version);
    state.0.save_settings()
}

// ------------------------------------------------------------ diagnostics

#[derive(Serialize)]
struct DiagResult {
    /// Full path of the saved zip.
    path: String,
    /// The folder it was saved in, as a short name for the toast.
    folder: String,
}

/// Save a diagnostics bundle to the user's Downloads folder and show it in Explorer.
#[tauri::command]
async fn export_diagnostics(instance_id: Option<String>, app: AppHandle, state: State<'_, AppState>) -> CmdResult<DiagResult> {
    let inner = state.0.clone();
    let out_dir = diagnostics::default_output_dir(inner.dirs.root());
    let names = diagnostics::user_names();

    let mut sys = format!(
        "Voxel 启动器 {}\n系统: {}\n内存: 可用 {} MB / 上限 {} MB\n数据目录: {}\n",
        env!("CARGO_PKG_VERSION"),
        diagnostics::os_description(),
        doctor::available_memory_mb(),
        config::max_memory_mb(),
        inner.dirs.root().display()
    );
    if let Ok(read) = std::fs::read_dir(inner.dirs.runtime()) {
        let runtimes: Vec<String> = read.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        sys.push_str(&format!("已下载的 Java 运行时: {}\n", if runtimes.is_empty() { "无".into() } else { runtimes.join(", ") }));
    }
    let (settings, instances) = (inner.settings.lock().unwrap().clone(), inner.instances.lock().unwrap().clone());
    let dir = out_dir.clone();
    let path = blocking(move || diagnostics::export(&inner.dirs, &settings, &instances, instance_id.as_deref(), &sys, &names, &dir)).await?;
    let _ = app.opener().reveal_item_in_dir(&path);
    Ok(DiagResult { path: path.to_string_lossy().into_owned(), folder: out_dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() })
}

// ------------------------------------------------------------------- java

#[derive(Serialize)]
struct JavaCheck {
    /// A Java runtime has to be downloaded before this instance can start.
    needed: bool,
    major: u32,
    component: String,
}

/// Does the instance need a Java runtime that is not on disk yet? Never fails the launch:
/// anything unclear (offline, odd files) answers "not needed" and the launch reports its own error.
#[tauri::command]
async fn java_check(instance_id: String, state: State<'_, AppState>) -> CmdResult<JavaCheck> {
    let inst = state.0.instance(&instance_id)?;
    let none = JavaCheck { needed: false, major: 0, component: String::new() };
    let global = state.0.settings.lock().unwrap().java_path.clone();
    if custom_java(&inst, &global).is_some() {
        return Ok(none);
    }
    let dl = make_downloader(&state.0, CancellationToken::new());
    match install::java_need(&dl, &state.0.dirs, inst.game_version()).await {
        Ok(n) => Ok(JavaCheck { needed: !n.installed, major: n.major, component: n.component }),
        Err(_) => Ok(none),
    }
}

/// Download the Java runtime an instance needs, after the user said yes.
#[tauri::command]
async fn install_java(instance_id: String, on_event: Channel<LaunchEvent>, state: State<'_, AppState>) -> CmdResult<()> {
    let inst = state.0.instance(&instance_id)?;
    let (_guard, token) = acquire_busy(&state.0)?;
    let dl = make_downloader(&state.0, token);
    let need = install::java_need(&dl, &state.0.dirs, inst.game_version()).await.map_err(err)?;
    emit(&on_event, LaunchEvent::Stage { label: format!("下载 Java {}", need.major) });
    java::ensure_runtime(&dl, &state.0.dirs, &need.component, Some(progress_fn(&on_event))).await.map_err(err)?;
    Ok(())
}

// ------------------------------------------------ servers and screenshots

/// Which of the hosts the launcher needs can be reached, and how fast.
#[tauri::command]
async fn network_test() -> Vec<netcheck::Probe> {
    netcheck::check_all(Duration::from_secs(6)).await
}

const MAX_SERVERS: usize = 50;

#[tauri::command]
fn server_add(name: String, address: String, state: State<AppState>) -> CmdResult<Snapshot> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 40 {
        return Err("服务器名称需要 1–40 个字符".into());
    }
    let (host, port) = servers::parse_address(&address).map_err(err)?;
    {
        let mut s = state.0.settings.lock().unwrap();
        if s.servers.len() >= MAX_SERVERS {
            return Err(format!("最多保存 {MAX_SERVERS} 个服务器"));
        }
        if s.servers.iter().any(|x| x.host.eq_ignore_ascii_case(&host) && x.port == port) {
            return Err("这个服务器已经添加过了".into());
        }
        let id = format!("srv-{}", now_secs() * 1000 + s.servers.len() as u64);
        s.servers.push(servers::ServerEntry { id, name: name.to_string(), host, port });
    }
    state.0.save_settings()?;
    Ok(snapshot(&state.0))
}

#[tauri::command]
fn server_remove(id: String, state: State<AppState>) -> CmdResult<Snapshot> {
    state.0.settings.lock().unwrap().servers.retain(|s| s.id != id);
    state.0.save_settings()?;
    Ok(snapshot(&state.0))
}

/// Is a saved server up, who is on it, and how far away is it.
#[tauri::command]
async fn server_ping(id: String, state: State<'_, AppState>) -> CmdResult<servers::ServerStatus> {
    let entry = state.0.settings.lock().unwrap().servers.iter().find(|s| s.id == id).cloned().ok_or("服务器不存在")?;
    servers::ping(&entry.host, entry.port, Duration::from_secs(4)).await.map_err(err)
}

#[tauri::command]
async fn screenshots_list(instance_id: String, state: State<'_, AppState>) -> CmdResult<Vec<screenshots::Screenshot>> {
    state.0.instance(&instance_id)?;
    let inner = state.0.clone();
    blocking(move || screenshots::list(&inner.dirs, &instance_id)).await
}

/// A scaled-down JPEG of a screenshot as a data URL: small for the grid, larger for viewing.
#[tauri::command]
async fn screenshot_image(instance_id: String, file: String, max_width: u32, state: State<'_, AppState>) -> CmdResult<String> {
    state.0.instance(&instance_id)?;
    let inner = state.0.clone();
    blocking(move || screenshots::image_data_url(&inner.dirs, &instance_id, &file, max_width.clamp(32, 3840))).await
}

#[tauri::command]
async fn screenshot_delete(instance_id: String, file: String, state: State<'_, AppState>) -> CmdResult<()> {
    state.0.instance(&instance_id)?;
    let inner = state.0.clone();
    blocking(move || screenshots::delete(&inner.dirs, &instance_id, &file)).await
}

#[tauri::command]
fn screenshots_open(instance_id: String, app: AppHandle, state: State<AppState>) -> CmdResult<()> {
    state.0.instance(&instance_id)?;
    let dir = state.0.dirs.instance(&instance_id).map_err(err)?.join("screenshots");
    std::fs::create_dir_all(&dir).map_err(err)?;
    app.opener().open_path(dir.to_string_lossy(), None::<&str>).map_err(err)
}

// -------------------------------------------------------------------- app

pub fn run() {
    let root = std::env::var_os("VOXEL_DATA_DIR").map(PathBuf::from).unwrap_or_else(Dirs::default_root);
    let dirs = Dirs::new(root);
    let settings: Settings = config::load_or_default(&dirs.settings_file());
    let instances: InstancesFile = config::load_or_default(&dirs.instances_file());
    let mica = config::os_build() >= 22000;
    let inner = Arc::new(Inner {
        dirs,
        settings: Mutex::new(settings),
        instances: Mutex::new(instances),
        busy: Mutex::new(None),
        mica,
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState(inner.clone()))
        .setup(move |app| {
            // Mica only exists on Windows 11; elsewhere the UI falls back to a solid gradient.
            if inner.mica {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.set_effects(
                        tauri::utils::config::WindowEffectsConfig {
                            effects: vec![tauri::utils::WindowEffect::Mica],
                            state: None,
                            radius: None,
                            color: None,
                            interactive: false,
                        },
                    );
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            list_versions,
            add_account,
            remove_account,
            select_account,
            save_settings,
            create_instance,
            update_instance,
            delete_instance,
            select_instance,
            probe_java,
            open_game_dir,
            open_data_dir,
            cancel_task,
            kill_game,
            install_version,
            install_instance,
            list_loader_versions,
            recommended_mods,
            install_extras,
            modpack_search,
            modpack_versions,
            install_modpack,
            content_search,
            content_plan,
            content_list,
            content_remove,
            content_scan,
            content_search_all,
            content_detail,
            content_plan_missing,
            mods_disable_duplicates,
            precheck_instance,
            worlds_list,
            world_delete,
            world_open,
            backup_list,
            backup_create,
            backup_restore,
            backup_delete,
            stats_sessions,
            stats_worlds,
            network_test,
            clone_instance,
            export_instance_pack,
            check_update,
            ignore_update,
            export_diagnostics,
            java_check,
            install_java,
            server_add,
            server_remove,
            server_ping,
            screenshots_list,
            screenshot_image,
            screenshot_delete,
            screenshots_open,
            mods_install,
            mods_set_enabled,
            launch_instance,
            attach_game,
            read_text_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Voxel launcher");
}

#[cfg(test)]
mod tests {
    use super::*;
    use launcher_core::loader::LoaderKind;
    use launcher_core::mods::{InstallPlan, PlanFile};

    fn vanilla() -> Instance {
        Instance { id: "a".into(), name: "A".into(), version_id: "1.21.1".into(), ..Default::default() }
    }

    fn fabric() -> Instance {
        let mut i = vanilla();
        i.version_id = "fabric-loader-0.16.9-1.21.1".into();
        i.mc_version = Some("1.21.1".into());
        i.loader = Some(LoaderRef { kind: LoaderKind::Fabric, version: "0.16.9".into() });
        i
    }

    fn plan_with(url: &str, sha1: &str) -> InstallPlan {
        InstallPlan {
            files: vec![PlanFile {
                project_id: "P".into(),
                version_id: "V".into(),
                title: "T".into(),
                version_number: "1".into(),
                icon_url: None,
                filename: "t.jar".into(),
                url: url.into(),
                sha1: sha1.into(),
                size: 1,
                required_by: None,
                user_selected: true,
                dest: launcher_core::mods::ContentKind::Mod,
            }],
            total_bytes: 1,
            warnings: vec![],
        }
    }

    #[test]
    fn vanilla_instance_has_no_loader() {
        let e = require_loader(&vanilla()).unwrap_err();
        assert!(e.contains("原版"), "{e}");
    }

    #[test]
    fn loader_instance_returns_its_loader() {
        let l = require_loader(&fabric()).unwrap();
        assert_eq!((l.kind, l.version.as_str()), (LoaderKind::Fabric, "0.16.9"));
    }

    #[test]
    fn running_instance_cannot_be_modified() {
        let mut i = fabric();
        assert!(require_stopped(&i).is_ok());
        i.running = Some(RunningGame { pid: 1, start_time: 2 });
        let e = require_stopped(&i).unwrap_err();
        assert!(e.contains("关闭游戏"), "{e}");
    }

    #[test]
    fn plan_urls_must_come_from_the_modrinth_cdn_over_https() {
        let sha = "a".repeat(40);
        assert!(validate_plan(&plan_with("https://cdn.modrinth.com/data/x/versions/y/t.jar", &sha)).is_ok());
        for bad in [
            "http://cdn.modrinth.com/data/x.jar",
            "https://cdn.modrinth.com.evil.com/x.jar",
            "https://evil.com/cdn.modrinth.com/x.jar",
            "https://cdn.modrinth.com@evil.com/x.jar",
            "file:///C:/x.jar",
            "",
        ] {
            assert!(validate_plan(&plan_with(bad, &sha)).is_err(), "{bad:?} must be rejected");
        }
    }

    #[test]
    fn resource_packs_need_no_loader_but_mods_and_shaders_do() {
        assert!(content_loader(&vanilla(), ContentKind::ResourcePack).is_ok());
        assert!(content_loader(&vanilla(), ContentKind::Shader).is_err());
        assert!(content_loader(&vanilla(), ContentKind::Mod).is_err());
        assert_eq!(content_loader(&fabric(), ContentKind::Shader).unwrap(), LoaderKind::Fabric);
    }

    fn pack_version(url: &str, sha1: &str, filename: &str) -> launcher_core::modpack::PackVersion {
        launcher_core::modpack::PackVersion {
            id: "v".into(),
            name: "n".into(),
            version_number: "1".into(),
            game_versions: vec!["1.21.1".into()],
            loaders: vec!["fabric".into()],
            version_type: "release".into(),
            file: launcher_core::mods::VersionFile { filename: filename.into(), url: url.into(), sha1: sha1.into(), size: 10, primary: true },
        }
    }

    #[test]
    fn instance_names_fall_back_to_the_pack_name_and_stay_within_40_characters() {
        assert_eq!(pack_instance_name("  My Pack  ", "Other"), "My Pack");
        assert_eq!(pack_instance_name("   ", " Fabulously Optimized "), "Fabulously Optimized");
        let long = "整".repeat(60);
        assert_eq!(pack_instance_name(&long, "x").chars().count(), 40);
        assert_eq!(pack_instance_name("", &"p".repeat(80)).chars().count(), 40);
    }

    #[test]
    fn modpack_files_must_be_mrpacks_from_the_modrinth_cdn_with_a_sha1() {
        let sha = "a".repeat(40);
        assert!(validate_pack_version(&pack_version("https://cdn.modrinth.com/data/x/versions/y/p.mrpack", &sha, "p.mrpack")).is_ok());
        assert!(validate_pack_version(&pack_version("https://cdn.modrinth.com/data/x/versions/y/p.MRPACK", &sha, "p.MRPACK")).is_ok());
        for (url, sha1, name) in [
            ("https://evil.com/p.mrpack", sha.as_str(), "p.mrpack"),
            ("http://cdn.modrinth.com/p.mrpack", sha.as_str(), "p.mrpack"),
            ("https://cdn.modrinth.com.evil.com/p.mrpack", sha.as_str(), "p.mrpack"),
            ("https://cdn.modrinth.com/p.mrpack", "zz", "p.mrpack"),
            ("https://cdn.modrinth.com/p.zip", sha.as_str(), "p.zip"),
            ("https://cdn.modrinth.com/p.mrpack", sha.as_str(), "../p.mrpack"),
        ] {
            assert!(validate_pack_version(&pack_version(url, sha1, name)).is_err(), "{url} {sha1} {name}");
        }
    }

    #[test]
    fn plan_checksums_must_be_sha1_hex() {
        let url = "https://cdn.modrinth.com/data/x.jar";
        assert!(validate_plan(&plan_with(url, "")).is_err());
        assert!(validate_plan(&plan_with(url, "zz")).is_err());
        assert!(validate_plan(&plan_with(url, &"g".repeat(40))).is_err());
    }
}
