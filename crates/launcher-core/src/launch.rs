//! Building the command line, preparing natives/assets and spawning the game.

use crate::account::Account;
use crate::config::{Dirs, RunningGame};
use crate::install::AssetIndex;
use crate::version::{
    self, expand_tokens, game_tokens, jvm_tokens, resolve_libraries, LibFile, ResolvedVersion,
    RuleContext,
};
use crate::{paths, Error, Result};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub const LAUNCHER_NAME: &str = "MinecraftLauncher";
/// CreateProcess limit is 32767 chars; stay well below it.
pub const MAX_CMDLINE: usize = 30_000;

/// `G1NewSizePercent`/`G1ReservePercent`/`G1HeapRegionSize` are experimental
/// flags: the unlock option must come first or the JVM refuses to start.
const DEFAULT_JVM_ARGS: &[&str] = &[
    "-XX:+UnlockExperimentalVMOptions",
    "-XX:+UseG1GC",
    "-XX:G1NewSizePercent=20",
    "-XX:G1ReservePercent=20",
    "-XX:MaxGCPauseMillis=50",
    "-XX:G1HeapRegionSize=32M",
];

pub struct LaunchRequest<'a> {
    pub dirs: &'a Dirs,
    pub resolved: &'a ResolvedVersion,
    pub account: &'a dyn Account,
    pub java: PathBuf,
    pub java_major: u32,
    pub game_dir: PathBuf,
    /// Value for `${game_assets}` (see [`prepare_assets`]).
    pub game_assets: PathBuf,
    pub natives_dir: PathBuf,
    pub memory_mb: u32,
    /// Extra JVM arguments; applied after `-Xmx`, so they take precedence.
    pub custom_jvm_args: &'a str,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Jump straight into a world or onto a server once the game is up.
    pub quick_play: Option<QuickPlay>,
}

/// Where to go when the game starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuickPlay {
    /// A singleplayer world, by its folder name inside `saves`.
    World(String),
    Server { host: String, port: u16 },
}

#[derive(Debug, Clone)]
pub struct LaunchPlan {
    pub java: PathBuf,
    /// Final argument vector for the process (may be `@argfile` + main class + game args).
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// If the command line was too long: (path, contents) of the JVM argfile.
    pub argfile: Option<(PathBuf, String)>,
    pub classpath: Vec<PathBuf>,
}

/// Quote one argument for a Java `@argfile` (inside quotes `\` is an escape).
pub fn escape_argfile(arg: &str) -> String {
    let mut s = String::with_capacity(arg.len() + 2);
    s.push('"');
    for c in arg.chars() {
        match c {
            '\\' => s.push_str("\\\\"),
            '"' => s.push_str("\\\""),
            '\n' => s.push_str("\\n"),
            '\r' => s.push_str("\\r"),
            c => s.push(c),
        }
    }
    s.push('"');
    s
}

fn classpath_files(libs: &[LibFile], dirs: &Dirs, resolved: &ResolvedVersion) -> Result<Vec<PathBuf>> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for lib in libs.iter().filter(|l| l.on_classpath) {
        let p = paths::real_path(&paths::safe_join(&dirs.libraries(), &lib.rel_path)?);
        if seen.insert(p.clone()) {
            out.push(p);
        }
    }
    let jar = version::version_json_path(&dirs.versions(), &resolved.jar_id)?.with_extension("jar");
    out.push(paths::real_path(&jar));
    Ok(out)
}

/// The target comes from the UI: plain names and host names only.
fn check_quick_play(qp: &QuickPlay) -> Result<()> {
    match qp {
        QuickPlay::World(w) => {
            if w.is_empty() || w == "." || w == ".." || w.contains(['/', '\\', '\0']) {
                return Err(Error::Launch(format!("无效的存档名: {w:?}")));
            }
        }
        QuickPlay::Server { host, port } => {
            let ok = !host.is_empty()
                && host.len() <= 253
                && !host.starts_with('-')
                && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '[' | ']' | ':'))
                && *port != 0;
            if !ok {
                return Err(Error::Launch(format!("无效的服务器地址: {host}:{port}")));
            }
        }
    }
    Ok(())
}

/// Pure command-line construction (no I/O except path validation).
pub fn build_plan(req: &LaunchRequest) -> Result<LaunchPlan> {
    let v = &req.resolved.json;
    let mut ctx = RuleContext::current();
    if req.width.is_some() && req.height.is_some() {
        ctx.features.insert("has_custom_resolution".into(), true);
    }
    if let Some(qp) = &req.quick_play {
        check_quick_play(qp)?;
        // The version file says how it wants quick play spelled; we only switch the features on.
        ctx.features.insert("has_quick_plays_support".into(), true);
        let kind = if matches!(qp, QuickPlay::World(_)) { "is_quick_play_singleplayer" } else { "is_quick_play_multiplayer" };
        ctx.features.insert(kind.into(), true);
    }

    let libs = resolve_libraries(v, &ctx);
    let classpath = classpath_files(&libs, req.dirs, req.resolved)?;
    let sep = if cfg!(windows) { ";" } else { ":" };
    let classpath_str = classpath.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>().join(sep);

    let assets_index = v
        .asset_index
        .as_ref()
        .map(|a| a.id.clone())
        .or_else(|| v.assets.clone())
        .unwrap_or_else(|| "legacy".into());
    let token = req.account.access_token();

    let mut vars: HashMap<String, String> = HashMap::new();
    let mut set = |k: &str, val: String| {
        vars.insert(k.to_string(), val);
    };
    set("auth_player_name", req.account.player_name().to_string());
    set("auth_uuid", req.account.uuid().to_string());
    set("auth_access_token", token.clone());
    set("auth_session", format!("token:{token}:{}", req.account.uuid()));
    set("user_type", req.account.user_type().to_string());
    set("user_properties", "{}".into());
    set("version_name", v.id.clone());
    set("version_type", v.kind.clone().unwrap_or_else(|| "release".into()));
    set("game_directory", req.game_dir.to_string_lossy().into_owned());
    set("assets_root", req.dirs.assets().to_string_lossy().into_owned());
    set("assets_index_name", assets_index);
    set("game_assets", req.game_assets.to_string_lossy().into_owned());
    set("natives_directory", req.natives_dir.to_string_lossy().into_owned());
    set("launcher_name", LAUNCHER_NAME.into());
    set("launcher_version", env!("CARGO_PKG_VERSION").into());
    set("classpath", classpath_str);
    set("classpath_separator", sep.into());
    set("library_directory", req.dirs.libraries().to_string_lossy().into_owned());
    if let (Some(w), Some(h)) = (req.width, req.height) {
        set("resolution_width", w.to_string());
        set("resolution_height", h.to_string());
    }
    match &req.quick_play {
        Some(QuickPlay::World(w)) => {
            set("quickPlayPath", req.game_dir.join("quickPlay").join("log.json").to_string_lossy().into_owned());
            set("quickPlaySingleplayer", w.clone());
        }
        Some(QuickPlay::Server { host, port }) => {
            set("quickPlayPath", req.game_dir.join("quickPlay").join("log.json").to_string_lossy().into_owned());
            set("quickPlayMultiplayer", format!("{host}:{port}"));
        }
        None => {}
    }

    // ---- JVM arguments: -Xmx first, then defaults, encoding, log4j, custom, version-defined.
    let mut jvm: Vec<String> = vec![format!("-Xmx{}M", req.memory_mb)];
    jvm.extend(DEFAULT_JVM_ARGS.iter().map(|s| s.to_string()));
    jvm.push("-Dfile.encoding=UTF-8".into());
    if req.java_major >= 19 {
        jvm.push("-Dstdout.encoding=UTF-8".into());
        jvm.push("-Dstderr.encoding=UTF-8".into());
    }
    if let Some(c) = v.logging.as_ref().and_then(|l| l.client.as_ref()) {
        let path = paths::safe_join(&req.dirs.assets().join("log_configs"), &c.file.id)?;
        jvm.push(c.argument.replace("${path}", &path.to_string_lossy()));
    }
    jvm.extend(req.custom_jvm_args.split_whitespace().map(str::to_string));
    jvm.extend(expand_tokens(jvm_tokens(v, &ctx), &vars));

    let main_class = v
        .main_class
        .clone()
        .ok_or_else(|| Error::Launch("版本缺少 mainClass".into()))?;
    let mut game = expand_tokens(game_tokens(v, &ctx), &vars);
    if let Some(qp) = &req.quick_play {
        let supported = game.iter().any(|a| a == "--quickPlaySingleplayer" || a == "--quickPlayMultiplayer");
        if !supported {
            match qp {
                QuickPlay::World(_) => return Err(Error::Launch("这个版本不支持直接进入存档（需要 Minecraft 1.20 或更新），请正常启动后在游戏里选择存档".into())),
                // Before quick play existed, a server was joined with these two flags.
                QuickPlay::Server { host, port } => game.extend(["--server".to_string(), host.clone(), "--port".to_string(), port.to_string()]),
            }
        }
    }

    let mut args = jvm.clone();
    args.push(main_class.clone());
    args.extend(game.iter().cloned());

    let cmdline_len = req.java.to_string_lossy().len() + 2 + args.iter().map(|a| a.len() + 3).sum::<usize>();
    let mut argfile = None;
    if cmdline_len > MAX_CMDLINE {
        if req.java_major < 9 {
            return Err(Error::Launch(format!(
                "命令行过长（{cmdline_len} 字符），Java 8 不支持 @argfile，请缩短数据目录路径"
            )));
        }
        let content = jvm.iter().map(|a| escape_argfile(a)).collect::<Vec<_>>().join("\n");
        let path = req.game_dir.join("jvm-args.txt");
        let mut short = vec![format!("@{}", path.to_string_lossy())];
        short.push(main_class);
        short.extend(game);
        args = short;
        argfile = Some((path, content));
    }

    Ok(LaunchPlan { java: req.java.clone(), args, cwd: req.game_dir.clone(), argfile, classpath })
}

// ------------------------------------------------------------------ natives

/// Extract all native libraries into `natives_dir` (skipped if already done).
pub fn extract_natives(libs: &[LibFile], libraries_dir: &Path, natives_dir: &Path) -> Result<()> {
    let marker = natives_dir.join(".done");
    if marker.is_file() {
        return Ok(());
    }
    std::fs::create_dir_all(natives_dir)?;
    for lib in libs.iter().filter(|l| l.extract) {
        let jar = paths::safe_join(libraries_dir, &lib.rel_path)?;
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&jar).map_err(|_| Error::Missing(jar.clone()))?)?;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i)?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name()?.to_string();
            let excluded = lib
                .extract_exclude
                .iter()
                .chain(std::iter::once(&"META-INF/".to_string()))
                .any(|ex| name.starts_with(ex.as_str()));
            if excluded {
                continue;
            }
            let dest = paths::safe_join(natives_dir, &name)?; // zip-slip guard
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out = std::fs::File::create(&dest)?;
            std::io::copy(&mut entry, &mut out)?;
        }
    }
    std::fs::write(marker, b"ok")?;
    Ok(())
}

/// Natives directory for a version, shared by every instance.
pub fn natives_dir(dirs: &Dirs, version_id: &str) -> Result<PathBuf> {
    paths::safe_join(&dirs.versions(), &format!("{version_id}/natives"))
}

pub fn prepare_natives(dirs: &Dirs, resolved: &ResolvedVersion) -> Result<PathBuf> {
    let dir = natives_dir(dirs, &resolved.json.id)?;
    let libs = resolve_libraries(&resolved.json, &RuleContext::current());
    extract_natives(&libs, &dirs.libraries(), &dir)?;
    Ok(dir)
}

// ------------------------------------------------------------------- assets

/// Handle the legacy asset layouts and return the value for `${game_assets}`.
/// Modern versions simply use the shared assets root.
pub fn prepare_assets(dirs: &Dirs, index_id: &str, index: &AssetIndex, game_dir: &Path) -> Result<PathBuf> {
    let target = if index.is_virtual {
        paths::safe_join(&dirs.assets().join("virtual"), index_id)?
    } else if index.map_to_resources {
        game_dir.join("resources")
    } else {
        return Ok(dirs.assets());
    };
    let objects = dirs.assets().join("objects");
    for (name, obj) in &index.objects {
        let dest = paths::safe_join(&target, name)?;
        if dest.is_file() {
            continue;
        }
        let src = paths::safe_join(&objects, &format!("{}/{}", &obj.hash[..2], obj.hash))?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&src, &dest).map_err(|_| Error::Missing(src))?;
    }
    Ok(target)
}

// ----------------------------------------------------------------- spawning

pub struct Launched {
    pub child: std::process::Child,
    pub pid: u32,
    pub stdio_log: PathBuf,
    pub running: RunningGame,
}

/// Start the game as an independent process. stdout/stderr go to a file (not a
/// pipe) so the game keeps working if the launcher exits.
pub fn spawn(plan: &LaunchPlan) -> Result<Launched> {
    for p in &plan.classpath {
        if !p.is_file() {
            return Err(Error::Missing(p.clone()));
        }
    }
    if !plan.java.is_file() {
        return Err(Error::Missing(plan.java.clone()));
    }
    std::fs::create_dir_all(&plan.cwd)?;
    if let Some((path, content)) = &plan.argfile {
        std::fs::write(path, content)?;
    }
    let stdio_log = plan.cwd.join("launcher-stdio.log");
    let out = std::fs::File::create(&stdio_log)?;
    let err = out.try_clone()?;

    let mut cmd = std::process::Command::new(&plan.java);
    cmd.args(&plan.args)
        .current_dir(&plan.cwd)
        .stdin(std::process::Stdio::null())
        .stdout(out)
        .stderr(err);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let child = cmd
        .spawn()
        .map_err(|e| Error::Launch(format!("无法启动 {}: {e}", plan.java.display())))?;
    let pid = child.id();
    let start_time = process_start_time(pid).unwrap_or(0);
    Ok(Launched { child, pid, stdio_log, running: RunningGame { pid, start_time } })
}

fn process_snapshot(pid: u32) -> Option<(u64, String)> {
    use sysinfo::{Pid, ProcessesToUpdate, System};
    let mut sys = System::new().ok()?;
    let pid = Pid::from_u32(pid);
    sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    sys.process(pid)
        .map(|p| (p.start_time(), p.name().to_string_lossy().to_ascii_lowercase()))
}

pub fn process_start_time(pid: u32) -> Option<u64> {
    process_snapshot(pid).map(|(t, _)| t)
}

/// A remembered game process is still ours only if the pid is alive, its start
/// time matches (guards against pid reuse) and it is a Java process.
pub fn is_game_running(g: &RunningGame) -> bool {
    matches!(process_snapshot(g.pid), Some((t, name)) if t == g.start_time && name.starts_with("java"))
}

/// Terminate a remembered game process (only if it is verifiably still ours).
pub fn kill_game(g: &RunningGame) -> bool {
    use sysinfo::{Pid, ProcessesToUpdate, System};
    if !is_game_running(g) {
        return false;
    }
    let Ok(mut sys) = System::new() else { return false };
    let pid = Pid::from_u32(g.pid);
    sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    sys.process(pid).map(|p| p.kill()).unwrap_or(false)
}

pub fn latest_log_path(game_dir: &Path) -> PathBuf {
    game_dir.join("logs").join("latest.log")
}

/// Newest file in `<gameDir>/crash-reports`, if any.
pub fn latest_crash_report(game_dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(game_dir.join("crash-reports"))
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "txt"))
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
        .map(|e| e.path())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::OfflineAccount;
    use crate::version::VersionJson;
    use std::io::Write;

    fn resolved(json: &str) -> ResolvedVersion {
        let v: VersionJson = serde_json::from_str(json).unwrap();
        ResolvedVersion { jar_id: v.id.clone(), chain: vec![v.id.clone()], json: v }
    }

    fn request<'a>(
        dirs: &'a Dirs,
        r: &'a ResolvedVersion,
        acct: &'a OfflineAccount,
        major: u32,
    ) -> LaunchRequest<'a> {
        LaunchRequest {
            dirs,
            resolved: r,
            account: acct,
            java: PathBuf::from("C:/jre/bin/java.exe"),
            java_major: major,
            game_dir: PathBuf::from("C:/Users/张 三/game"),
            game_assets: dirs.assets(),
            natives_dir: PathBuf::from("C:/n"),
            memory_mb: 3072,
            custom_jvm_args: "-Xmx4G -Dfoo=bar",
            width: None,
            height: None,
            quick_play: None,
        }
    }

    const MODERN: &str = r#"{
      "id":"1.21.1","type":"release","mainClass":"net.minecraft.client.main.Main",
      "assetIndex":{"id":"17","url":"u"},
      "libraries":[{"name":"com.mojang:logging:1.2.7","downloads":{"artifact":{"path":"com/mojang/logging/1.2.7/logging-1.2.7.jar","url":"u","sha1":"s","size":1}}}],
      "arguments":{
        "game":["--username","${auth_player_name}","--version","${version_name}",
                {"rules":[{"action":"allow","features":{"has_custom_resolution":true}}],"value":["--width","${resolution_width}"]},
                "--clientId","${clientid}","--xuid","${auth_xuid}"],
        "jvm":[{"rules":[{"action":"allow","os":{"name":"windows"}}],"value":"-XX:HeapDumpPath=MojangTricksIntelDriversForPerformance_javaw.exe_minecraft.exe.heapdump"},
               "-Djava.library.path=${natives_directory}","-cp","${classpath}"]}}"#;

    #[test]
    fn modern_command_line() {
        let dirs = Dirs::new("C:/data");
        let r = resolved(MODERN);
        let acct = OfflineAccount::new("Steve").unwrap();
        let plan = build_plan(&request(&dirs, &r, &acct, 21)).unwrap();
        let a = &plan.args;
        assert_eq!(a[0], "-Xmx3072M");
        // custom args come after the slider's -Xmx so they win
        let slider = a.iter().position(|x| x == "-Xmx3072M").unwrap();
        let custom = a.iter().position(|x| x == "-Xmx4G").unwrap();
        assert!(custom > slider);
        assert!(a.contains(&"-Dfile.encoding=UTF-8".to_string()));
        assert!(a.contains(&"-Dstdout.encoding=UTF-8".to_string()));
        let cp_idx = a.iter().position(|x| x == "-cp").unwrap();
        assert!(a[cp_idx + 1].contains("logging-1.2.7.jar") && a[cp_idx + 1].ends_with("1.21.1.jar"));
        let main = a.iter().position(|x| x == "net.minecraft.client.main.Main").unwrap();
        assert!(main > cp_idx);
        assert_eq!(&a[main + 1..], ["--username", "Steve", "--version", "1.21.1"]);
        assert!(!a.iter().any(|x| x.contains("${")));
    }

    #[test]
    fn experimental_jvm_flags_are_preceded_by_the_unlock_option() {
        let unlock = DEFAULT_JVM_ARGS.iter().position(|a| *a == "-XX:+UnlockExperimentalVMOptions");
        for (i, a) in DEFAULT_JVM_ARGS.iter().enumerate() {
            if a.starts_with("-XX:G1NewSizePercent") || a.starts_with("-XX:G1ReservePercent") {
                assert!(unlock.is_some_and(|u| u < i), "{a} needs the unlock option before it");
            }
        }
    }


    const QUICK: &str = r#"{
      "id":"1.21.1","type":"release","mainClass":"net.minecraft.client.main.Main",
      "assetIndex":{"id":"17","url":"u"},"libraries":[],
      "arguments":{
        "game":["--username","${auth_player_name}",
                {"rules":[{"action":"allow","features":{"has_quick_plays_support":true}}],"value":["--quickPlayPath","${quickPlayPath}"]},
                {"rules":[{"action":"allow","features":{"is_quick_play_singleplayer":true}}],"value":["--quickPlaySingleplayer","${quickPlaySingleplayer}"]},
                {"rules":[{"action":"allow","features":{"is_quick_play_multiplayer":true}}],"value":["--quickPlayMultiplayer","${quickPlayMultiplayer}"]}],
        "jvm":["-cp","${classpath}"]}}"#;

    fn quick_plan(json: &str, qp: Option<QuickPlay>) -> Result<LaunchPlan> {
        let dirs = Dirs::new("C:/data");
        let r = resolved(json);
        let acct = OfflineAccount::new("Steve").unwrap();
        let mut req = request(&dirs, &r, &acct, 21);
        req.quick_play = qp;
        build_plan(&req)
    }

    #[test]
    fn quick_play_world_uses_the_versions_own_arguments() {
        let plan = quick_plan(QUICK, Some(QuickPlay::World("My World".into()))).unwrap();
        let a = &plan.args;
        assert!(a.windows(2).any(|w| w[0] == "--quickPlaySingleplayer" && w[1] == "My World"), "{a:?}");
        assert!(a.iter().any(|x| x == "--quickPlayPath"), "the game needs somewhere to write its quick-play log");
        assert!(!a.iter().any(|x| x == "--quickPlayMultiplayer"));
        assert!(!a.iter().any(|x| x.contains("${")));
    }

    #[test]
    fn quick_play_server_passes_host_and_port() {
        let plan = quick_plan(QUICK, Some(QuickPlay::Server { host: "play.example.com".into(), port: 25565 })).unwrap();
        assert!(plan.args.windows(2).any(|w| w[0] == "--quickPlayMultiplayer" && w[1] == "play.example.com:25565"));
        assert!(!plan.args.iter().any(|x| x == "--quickPlaySingleplayer"));
    }

    #[test]
    fn without_quick_play_no_quick_play_flag_is_passed() {
        let plan = quick_plan(QUICK, None).unwrap();
        assert!(!plan.args.iter().any(|x| x.starts_with("--quickPlay")));
    }

    #[test]
    fn an_older_version_cannot_jump_into_a_world_but_can_join_a_server_the_old_way() {
        let err = quick_plan(MODERN, Some(QuickPlay::World("W".into()))).unwrap_err().to_string();
        assert!(err.contains("1.20"), "{err}");
        let plan = quick_plan(MODERN, Some(QuickPlay::Server { host: "mc.example.org".into(), port: 25566 })).unwrap();
        assert!(plan.args.windows(2).any(|w| w == ["--server", "mc.example.org"]));
        assert!(plan.args.windows(2).any(|w| w == ["--port", "25566"]));
    }

    #[test]
    fn quick_play_targets_are_validated() {
        for bad in ["", "a/b", "a\\b", "..", "x\0y"] {
            assert!(quick_plan(QUICK, Some(QuickPlay::World(bad.into()))).is_err(), "{bad:?}");
        }
        for host in ["", "bad host", "a;b", "-x", "a/b"] {
            assert!(quick_plan(QUICK, Some(QuickPlay::Server { host: host.into(), port: 25565 })).is_err(), "{host:?}");
        }
        assert!(quick_plan(QUICK, Some(QuickPlay::Server { host: "ok.example.com".into(), port: 0 })).is_err());
    }

    #[test]
    fn custom_resolution_feature() {
        let dirs = Dirs::new("C:/data");
        let r = resolved(MODERN);
        let acct = OfflineAccount::new("Steve").unwrap();
        let mut req = request(&dirs, &r, &acct, 21);
        req.width = Some(1280);
        req.height = Some(720);
        let plan = build_plan(&req).unwrap();
        assert!(plan.args.windows(2).any(|w| w == ["--width", "1280"]));
    }

    #[test]
    fn legacy_command_line_keeps_spaces_and_injects_log4j() {
        let dirs = Dirs::new("C:/data");
        let r = resolved(
            r#"{"id":"1.12.2","type":"release","mainClass":"net.minecraft.client.main.Main",
                "assets":"1.12","assetIndex":{"id":"1.12","url":"u"},
                "minecraftArguments":"--username ${auth_player_name} --gameDir ${game_directory} --userProperties ${user_properties}",
                "logging":{"client":{"argument":"-Dlog4j.configurationFile=${path}","file":{"id":"client-1.12.xml","url":"u"}}},
                "libraries":[]}"#,
        );
        let acct = OfflineAccount::new("Alex").unwrap();
        let plan = build_plan(&request(&dirs, &r, &acct, 8)).unwrap();
        let a = &plan.args;
        assert!(a.iter().any(|x| x.starts_with("-Dlog4j.configurationFile=") && x.ends_with("client-1.12.xml")));
        assert!(a.windows(2).any(|w| w[0] == "-cp"));
        assert!(!a.contains(&"-Dstdout.encoding=UTF-8".to_string())); // Java 8
        let g = a.iter().position(|x| x == "--gameDir").unwrap();
        assert_eq!(a[g + 1], "C:/Users/张 三/game");
        assert_eq!(a.last().unwrap(), "{}");
    }

    #[test]
    fn long_command_line_uses_argfile_on_java9_plus_and_errors_on_java8() {
        let dirs = Dirs::new("C:/data");
        let r = resolved(MODERN);
        let acct = OfflineAccount::new("Steve").unwrap();
        let mut req = request(&dirs, &r, &acct, 21);
        let long = format!("-Dx={}", "a".repeat(MAX_CMDLINE));
        req.custom_jvm_args = &long;
        let plan = build_plan(&req).unwrap();
        let (path, content) = plan.argfile.clone().unwrap();
        assert_eq!(plan.args[0], format!("@{}", path.to_string_lossy()));
        assert_eq!(plan.args[1], "net.minecraft.client.main.Main");
        assert!(content.contains("-cp"));

        let mut req8 = request(&dirs, &r, &acct, 8);
        req8.custom_jvm_args = &long;
        assert!(build_plan(&req8).is_err());
    }

    #[test]
    fn argfile_escapes_backslashes_and_quotes() {
        assert_eq!(escape_argfile(r"C:\Users\张 三\x"), r#""C:\\Users\\张 三\\x""#);
        assert_eq!(escape_argfile(r#"a"b"#), r#""a\"b""#);
    }

    #[test]
    fn natives_extraction_excludes_meta_inf_and_blocks_zip_slip() {
        let dir = tempfile::tempdir().unwrap();
        let libs_dir = dir.path().join("libs");
        std::fs::create_dir_all(libs_dir.join("a")).unwrap();
        let jar = libs_dir.join("a/native.jar");
        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(&jar).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            w.start_file("lwjgl.dll", opts).unwrap();
            w.write_all(b"dll").unwrap();
            w.start_file("META-INF/MANIFEST.MF", opts).unwrap();
            w.write_all(b"x").unwrap();
            w.finish().unwrap();
        }
        let lib = LibFile {
            key: "k".into(),
            rel_path: "a/native.jar".into(),
            url: String::new(),
            sha1: None,
            size: None,
            sidecar: false,
            on_classpath: false,
            extract: true,
            extract_exclude: vec!["META-INF/".into()],
        };
        let out = dir.path().join("natives");
        extract_natives(&[lib.clone()], &libs_dir, &out).unwrap();
        assert!(out.join("lwjgl.dll").is_file());
        assert!(!out.join("META-INF").exists());

        // malicious archive
        let evil = libs_dir.join("a/evil.jar");
        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(&evil).unwrap());
            w.start_file("../../evil.dll", zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(b"x").unwrap();
            w.finish().unwrap();
        }
        let mut bad = lib;
        bad.rel_path = "a/evil.jar".into();
        let out2 = dir.path().join("natives2");
        assert!(extract_natives(&[bad], &libs_dir, &out2).is_err());
        assert!(!dir.path().join("evil.dll").exists());
    }
}
