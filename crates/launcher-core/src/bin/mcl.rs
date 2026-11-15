//! Minimal CLI used to verify launcher-core end to end (M1).
//!
//!   mcl list [--all]
//!   mcl install <version>
//!   mcl launch <version> [--name Steve] [--mem 2048] [--java PATH] [--detach] [--instance ID]
//!   mcl loader-versions <fabric|quilt|forge|neoforge> <mc>
//!   mcl install-loader <fabric|quilt|forge|neoforge> <mc> [loader-version] [--java PATH]
//!   mcl mod-search <query> --loader fabric|quilt|forge|neoforge --mc 1.21.1
//!   mcl mod-add <project> --instance ID --loader fabric|quilt|forge|neoforge --mc 1.21.1
//!   mcl mods --instance ID
//!
//! Global flags: --data-dir DIR, --official (skip the BMCLAPI mirror)

use launcher_core::account::{Account, OfflineAccount};
use launcher_core::config::{self, Dirs};
use launcher_core::download::{Downloader, Progress, ProgressFn};
use launcher_core::loader::{self, LoaderKind};
use launcher_core::mirror::Source;
use launcher_core::mods::{self, ModrinthClient};
use launcher_core::{install, java, launch, version};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

struct Args {
    cmd: String,
    positional: Vec<String>,
    flags: std::collections::HashMap<String, String>,
}

fn parse_args() -> Args {
    let mut it = std::env::args().skip(1);
    let cmd = it.next().unwrap_or_default();
    let mut positional = Vec::new();
    let mut flags = std::collections::HashMap::new();
    let rest: Vec<String> = it.collect();
    let mut i = 0;
    while i < rest.len() {
        if let Some(name) = rest[i].strip_prefix("--") {
            let is_bool = matches!(name, "all" | "official" | "detach");
            if is_bool {
                flags.insert(name.to_string(), "1".into());
            } else {
                i += 1;
                flags.insert(name.to_string(), rest.get(i).cloned().unwrap_or_default());
            }
        } else {
            positional.push(rest[i].clone());
        }
        i += 1;
    }
    Args { cmd, positional, flags }
}

fn progress_printer() -> ProgressFn {
    let last = std::sync::Mutex::new((String::new(), 0u64));
    Arc::new(move |p: &Progress| {
        let mut l = last.lock().unwrap();
        if l.0 != p.stage {
            if !l.0.is_empty() {
                println!();
            }
            l.0 = p.stage.clone();
        }
        l.1 += 1;
        print!(
            "\r[{}] {}/{} 文件  {:.1}/{:.1} MB      ",
            p.stage,
            p.files_done,
            p.files_total,
            p.bytes_done as f64 / 1_048_576.0,
            p.bytes_total as f64 / 1_048_576.0
        );
        let _ = std::io::stdout().flush();
    })
}

fn parse_kind(s: &str) -> launcher_core::Result<LoaderKind> {
    match s {
        "fabric" => Ok(LoaderKind::Fabric),
        "quilt" => Ok(LoaderKind::Quilt),
        "forge" => Ok(LoaderKind::Forge),
        "neoforge" => Ok(LoaderKind::NeoForge),
        other => Err(launcher_core::Error::Other(format!("未知加载器 `{other}`（可用: fabric, quilt, forge, neoforge）"))),
    }
}

fn flag<'a>(args: &'a Args, name: &str) -> launcher_core::Result<&'a str> {
    args.flags
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| launcher_core::Error::Other(format!("缺少参数 --{name}")))
}

fn positional(args: &Args, i: usize, usage: &str) -> launcher_core::Result<String> {
    args.positional.get(i).cloned().ok_or_else(|| launcher_core::Error::Other(format!("用法: {usage}")))
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("\n错误: {e}");
        std::process::exit(1);
    }
}

async fn run() -> launcher_core::Result<()> {
    let args = parse_args();
    let root = args.flags.get("data-dir").map(PathBuf::from).unwrap_or_else(Dirs::default_root);
    let dirs = Dirs::new(root);
    let source = if args.flags.contains_key("official") { Source::Official } else { Source::Mirror };
    let dl = Downloader::new(source);

    match args.cmd.as_str() {
        "list" => {
            let m = install::fetch_manifest(&dl).await?;
            let all = args.flags.contains_key("all");
            println!("最新正式版 {}  最新快照 {}", m.latest.release, m.latest.snapshot);
            for e in m.supported().into_iter().filter(|e| all || e.kind == "release").take(if all { 80 } else { 40 }) {
                println!("{:<14} {:<9} {}", e.id, e.kind, e.release_time);
            }
        }
        "install" => {
            let id = args.positional.first().ok_or_else(|| launcher_core::Error::Other("用法: mcl install <version>".into()))?;
            install::install_version(&dl, &dirs, id, Some(progress_printer())).await?;
            println!("\n已安装 {id}");
        }
        "launch" => {
            let id = args.positional.first().ok_or_else(|| launcher_core::Error::Other("用法: mcl launch <version>".into()))?;
            let name = args.flags.get("name").map(String::as_str).unwrap_or("Steve");
            let account = OfflineAccount::new(name)?;
            let memory = args
                .flags
                .get("mem")
                .and_then(|m| m.parse().ok())
                .unwrap_or_else(config::recommended_memory_mb);

            let resolved = install::install_version(&dl, &dirs, id, Some(progress_printer())).await?;
            println!();

            // Java: user-supplied (validated) or Mojang's runtime for this version.
            let (java_path, java_major) = match args.flags.get("java") {
                Some(p) => {
                    let info = java::probe_java(std::path::Path::new(p)).await?;
                    println!("使用指定 Java {} (主版本 {})", info.version, info.major);
                    if let Some(jv) = &resolved.json.java_version {
                        if info.major < jv.major_version {
                            eprintln!("警告: 该版本需要 Java {}，当前只有 {}", jv.major_version, info.major);
                        }
                    }
                    (PathBuf::from(p), info.major)
                }
                None => {
                    let (component, major) = match &resolved.json.java_version {
                        Some(jv) => (jv.component.clone(), jv.major_version),
                        None => (java::LEGACY_COMPONENT.to_string(), java::LEGACY_MAJOR),
                    };
                    let p = java::ensure_runtime(&dl, &dirs, &component, Some(progress_printer())).await?;
                    println!("\n使用 Mojang 运行时 {component} (Java {major})");
                    (p, major)
                }
            };

            let game_dir = match args.flags.get("instance") {
                Some(inst) => dirs.instance(inst)?,
                None => dirs.instance(&format!("cli-{}", id.replace(|c: char| !c.is_ascii_alphanumeric(), "-")))?,
            };
            std::fs::create_dir_all(&game_dir)?;
            let natives = launch::prepare_natives(&dirs, &resolved)?;
            let index_id = resolved.json.asset_index.as_ref().map(|a| a.id.clone()).unwrap_or_default();
            let index = install::load_asset_index(&dirs, &index_id)?;
            let game_assets = launch::prepare_assets(&dirs, &index_id, &index, &game_dir)?;

            let plan = launch::build_plan(&launch::LaunchRequest {
                quick_play: None,
                dirs: &dirs,
                resolved: &resolved,
                account: &account,
                java: java_path,
                java_major,
                game_dir: game_dir.clone(),
                game_assets,
                natives_dir: natives,
                memory_mb: memory,
                custom_jvm_args: "",
                width: None,
                height: None,
            })?;
            let mut launched = launch::spawn(&plan)?;
            println!("游戏已启动: pid {}  玩家 {} ({})", launched.pid, account.player_name(), account.uuid());
            println!("日志: {}", launch::latest_log_path(&game_dir).display());
            println!("标准输出: {}", launched.stdio_log.display());
            if args.flags.contains_key("detach") {
                return Ok(());
            }
            let status = tokio::task::spawn_blocking(move || launched.child.wait())
                .await
                .map_err(|e| launcher_core::Error::Other(e.to_string()))??;
            println!("游戏已退出: {status}");
            if !status.success() {
                if let Some(c) = launch::latest_crash_report(&game_dir) {
                    println!("崩溃报告: {}", c.display());
                }
            }
        }
        "loader-versions" => {
            let usage = "mcl loader-versions <fabric|quilt|forge|neoforge> <mc>";
            let kind = parse_kind(&positional(&args, 0, usage)?)?;
            let mc = positional(&args, 1, usage)?;
            let list = loader::list_loader_versions(&dl, kind, &mc).await?;
            if list.is_empty() {
                println!("{} 不支持 Minecraft {mc}", kind.label());
            }
            let default = loader::default_version(&list).map(|v| v.version.clone());
            for v in list.iter().take(15) {
                let mark = if Some(&v.version) == default.as_ref() { "  <- 默认" } else if v.stable { "" } else { "  (测试版)" };
                println!("{}{mark}", v.version);
            }
        }
        "install-loader" => {
            let usage = "mcl install-loader <fabric|quilt|forge|neoforge> <mc> [loader-version] [--java PATH]";
            let kind = parse_kind(&positional(&args, 0, usage)?)?;
            let mc = positional(&args, 1, usage)?;
            let version = match args.positional.get(2) {
                Some(v) => v.clone(),
                None => {
                    let list = loader::list_loader_versions(&dl, kind, &mc).await?;
                    loader::default_version(&list)
                        .map(|v| v.version.clone())
                        .ok_or_else(|| launcher_core::Error::Other(format!("{} 不支持 Minecraft {mc}", kind.label())))?
                }
            };
            // Forge/NeoForge run installer steps with Java: --java PATH, else Mojang's runtime for this Minecraft version.
            let java_path = if kind.runs_installer() {
                match args.flags.get("java") {
                    Some(p) => Some(PathBuf::from(p)),
                    None => {
                        let vanilla = install::install_version(&dl, &dirs, &mc, Some(progress_printer())).await?;
                        let component = vanilla
                            .json
                            .java_version
                            .as_ref()
                            .map(|j| j.component.clone())
                            .unwrap_or_else(|| java::LEGACY_COMPONENT.to_string());
                        Some(java::ensure_runtime(&dl, &dirs, &component, Some(progress_printer())).await?)
                    }
                }
            } else {
                None
            };
            loader::install_loader(&dl, &dirs, kind, &mc, &version, java_path.as_deref(), Some(progress_printer())).await?;
            println!("\n已安装 {}", loader::profile_id(kind, &version, &mc));
        }
        "mod-search" => {
            let query = args.positional.first().cloned().unwrap_or_default();
            let t = mods::target_for(flag(&args, "mc")?, parse_kind(flag(&args, "loader")?)?);
            let r = ModrinthClient::official().search(&t, &query, 0).await?;
            println!("共 {} 个结果", r.total);
            for h in r.hits {
                println!("{:<28} {:<10} {}", h.slug, h.downloads, h.title);
            }
        }
        "mod-add" => {
            let project = positional(&args, 0, "mcl mod-add <project> --instance ID --loader fabric|quilt|forge|neoforge --mc 1.21.1")?;
            let instance = flag(&args, "instance")?;
            let t = mods::target_for(flag(&args, "mc")?, parse_kind(flag(&args, "loader")?)?);
            let client = ModrinthClient::official();
            let installed = mods::installed_project_ids(&dirs, instance)?;
            let plan = mods::plan_install(&client, &t, &project, &installed).await?;
            for f in &plan.files {
                let why = f.required_by.as_deref().map(|b| format!("（{b} 需要）")).unwrap_or_default();
                println!("  {} {} {:.2} MB {why}", f.title, f.version_number, f.size as f64 / 1_048_576.0);
            }
            for w in &plan.warnings {
                eprintln!("警告: {w}");
            }
            mods::install_plan(&dl, &dirs, instance, &plan, Some(progress_printer())).await?;
            println!("\n已安装 {} 个文件", plan.files.len());
        }
        "mods" => {
            let instance = flag(&args, "instance")?;
            for m in mods::list_installed(&dirs, instance)? {
                println!("{} {:<44} {}", if m.enabled { "on " } else { "off" }, m.filename, m.title.unwrap_or_default());
            }
        }
        _ => {
            println!("用法:\n  mcl list [--all]\n  mcl install <版本>\n  mcl launch <版本> [--name Steve] [--mem MB] [--java PATH] [--detach] [--instance ID]\n  mcl loader-versions <fabric|quilt|forge|neoforge> <mc>\n  mcl install-loader <fabric|quilt|forge|neoforge> <mc> [加载器版本] [--java PATH]\n  mcl mod-search <关键词> --loader fabric|quilt|forge|neoforge --mc 1.21.1\n  mcl mod-add <项目> --instance ID --loader fabric|quilt|forge|neoforge --mc 1.21.1\n  mcl mods --instance ID\n全局: --data-dir DIR  --official");
        }
    }
    let _ = version::OLDEST_SUPPORTED;
    Ok(())
}
