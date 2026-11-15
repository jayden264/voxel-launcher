//! Running an installer's steps: synthetic installer jars, a stub process
//! runner (no Java needed) and a mock maven server.

use launcher_core::config::Dirs;
use launcher_core::download::{sha1_bytes, Downloader};
use launcher_core::forge::{run_installer, ProcessOutcome, ProcessSpec, RunFn};
use launcher_core::loader::LoaderKind;
use launcher_core::mirror::Source;
use launcher_core::{Error, Result};
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const MC: &str = "1.21.1";
const VER: &str = "9.9.9";
const ID: &str = "1.21.1-forge-9.9.9";
const OUT_CONTENT: &[u8] = b"patched-client-jar";

fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in entries {
        w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap().into_inner()
}

/// A tool jar whose manifest names `x.Main`.
fn tool_jar() -> Vec<u8> {
    zip_bytes(&[("META-INF/MANIFEST.MF", b"Manifest-Version: 1.0\r\nMain-Class: x.Main\r\n\r\n")])
}

struct Env {
    server: MockServer,
    tmp: tempfile::TempDir,
    dirs: Dirs,
    dl: Downloader,
}

impl Env {
    async fn new() -> Env {
        let server = MockServer::start().await;
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::new(tmp.path());
        // The vanilla jar the steps read; `run_installer` expects it to be installed already.
        let jar = dirs.versions().join(MC).join(format!("{MC}.jar"));
        std::fs::create_dir_all(jar.parent().unwrap()).unwrap();
        std::fs::write(&jar, b"vanilla").unwrap();
        let base = server.uri();
        let dl = Downloader::new(Source::Official)
            .with_retry(0, Duration::from_millis(1))
            .with_resolver(Arc::new(move |u| vec![u.replace("https://maven.minecraftforge.net", &base)]));
        Env { server, tmp, dirs, dl }
    }

    async fn serve(&self, p: &str, data: &[u8]) {
        Mock::given(method("GET"))
            .and(path(p.to_string()))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(data.to_vec()))
            .mount(&self.server)
            .await;
    }

    /// Serve `installer` (and its .sha1) where a Forge 1.21.1-9.9.9 installer lives.
    async fn serve_installer(&self, installer: &[u8]) {
        let p = format!("/net/minecraftforge/forge/{MC}-{VER}/forge-{MC}-{VER}-installer.jar");
        self.serve(&p, installer).await;
        self.serve(&format!("{p}.sha1"), sha1_bytes(installer).as_bytes()).await;
    }

    async fn run(&self, runner: &RunFn) -> Result<String> {
        run_installer(&self.dl, &self.dirs, LoaderKind::Forge, MC, VER, Path::new("java"), runner, None).await
    }

    fn version_dir(&self) -> std::path::PathBuf {
        self.dirs.versions().join(ID)
    }

    fn out_path(&self) -> std::path::PathBuf {
        self.dirs.libraries().join("net/test/out/1/out-1.jar")
    }
}

fn version_json(id: &str, inherits: &str) -> Value {
    json!({"id": id, "inheritsFrom": inherits, "mainClass": "x.Main", "libraries": [], "arguments": {"game": [], "jvm": []}})
}

/// A modern installer: one downloaded tool library, one embedded library, two
/// client steps (one is `server`-only), producing `out-1.jar`.
fn modern_installer(env: &Env, tool: &[u8], profile_overrides: impl FnOnce(&mut Value), version: Value) -> Vec<u8> {
    let mut profile = json!({
        "spec": 1, "profile": "forge", "version": ID, "minecraft": MC, "json": "/version.json",
        "libraries": [
            {"name": "x:y:1", "downloads": {"artifact": {
                "path": "x/y/1/y-1.jar", "url": format!("{}/y-1.jar", env.server.uri()),
                "sha1": sha1_bytes(tool), "size": tool.len()}}},
            {"name": "net.test:shim:1", "downloads": {"artifact": {
                "path": "net/test/shim/1/shim-1.jar", "url": "", "sha1": "", "size": 0}}}
        ],
        "data": {
            "BIN": {"client": "/data/c.bin", "server": "/data/s.bin"},
            "OUT": {"client": "[net.test:out:1]", "server": "[net.test:out:1:server]"},
            "OUT_SHA": {"client": format!("'{}'", sha1_bytes(OUT_CONTENT)), "server": "'0'"}
        },
        "processors": [
            {"sides": ["server"], "jar": "x:y:1", "classpath": [], "args": ["--server-only"]},
            {"jar": "x:y:1", "classpath": ["x:y:1"], "args": ["--task", "PATCH", "{BIN}", "{OUT}", "{MINECRAFT_JAR}", "{SIDE}"],
             "outputs": {"{OUT}": "{OUT_SHA}"}}
        ]
    });
    profile_overrides(&mut profile);
    zip_bytes(&[
        ("install_profile.json", profile.to_string().as_bytes()),
        ("version.json", version.to_string().as_bytes()),
        ("data/c.bin", b"client-binpatch"),
        ("data/s.bin", b"server-binpatch"),
        ("maven/net/test/shim/1/shim-1.jar", b"shim-bytes"),
    ])
}

type Calls = Arc<Mutex<Vec<ProcessSpec>>>;

/// A runner that records every step and then calls `act`.
fn runner(calls: &Calls, act: impl Fn(&ProcessSpec) -> Result<i32> + Send + Sync + 'static) -> RunFn {
    let calls = calls.clone();
    Arc::new(move |spec, _cancel| {
        calls.lock().unwrap().push(spec.clone());
        let r = act(&spec);
        Box::pin(async move { r.map(|code| ProcessOutcome { code }) })
    })
}

fn writes_good_output(out: std::path::PathBuf) -> impl Fn(&ProcessSpec) -> Result<i32> + Send + Sync + 'static {
    move |_| {
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        std::fs::write(&out, OUT_CONTENT).unwrap();
        Ok(0)
    }
}

async fn standard_env() -> (Env, Vec<u8>) {
    let env = Env::new().await;
    let tool = tool_jar();
    env.serve("/y-1.jar", &tool).await;
    // The tool's sidecar is not needed: the profile gives its sha1.
    let installer = modern_installer(&env, &tool, |_| {}, version_json("1.21.1-forge-OLD-ID", MC));
    env.serve_installer(&installer).await;
    (env, tool)
}

#[tokio::test]
async fn runs_client_steps_in_order_with_expanded_arguments_and_writes_the_version_last() {
    let (env, _) = standard_env().await;
    let calls: Calls = Default::default();
    let id = env.run(&runner(&calls, writes_good_output(env.out_path()))).await.unwrap();
    assert_eq!(id, ID);

    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "the server-only step must not run");
    let args = &calls[0].args;
    assert_eq!(args[0], "-cp");
    assert!(args[1].contains("y-1.jar"), "{args:?}");
    assert_eq!(args[2], "x.Main", "main class comes from the tool's manifest");
    assert_eq!(&args[3..5], ["--task", "PATCH"]);
    assert!(args[5].replace('\\', "/").ends_with("data/c.bin"), "{args:?}");
    assert!(args[6].replace('\\', "/").ends_with("net/test/out/1/out-1.jar"), "{args:?}");
    assert!(args[7].replace('\\', "/").ends_with("versions/1.21.1/1.21.1.jar"), "{args:?}");
    assert_eq!(args[8], "client");
    assert!(std::fs::read(args[5].as_str()).is_err(), "extracted files are cleaned up afterwards");

    // The version JSON has our canonical id (the installer's own id was different) and no temp file is left.
    let file = env.version_dir().join(format!("{ID}.json"));
    let v: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(v["id"], ID);
    assert_eq!(v["inheritsFrom"], MC);
    assert!(!env.version_dir().join(format!("{ID}.json.tmp")).exists());
    // Embedded library was extracted from the installer's maven/ folder.
    assert_eq!(std::fs::read(env.dirs.libraries().join("net/test/shim/1/shim-1.jar")).unwrap(), b"shim-bytes");
}

#[tokio::test]
async fn skips_steps_whose_outputs_are_already_valid_and_a_finished_install_does_no_work() {
    let (env, _) = standard_env().await;
    std::fs::create_dir_all(env.out_path().parent().unwrap()).unwrap();
    std::fs::write(env.out_path(), OUT_CONTENT).unwrap();
    let calls: Calls = Default::default();
    let r = runner(&calls, |_| panic!("no step may run"));
    assert_eq!(env.run(&r).await.unwrap(), ID);
    assert!(calls.lock().unwrap().is_empty());

    // Second run: the version JSON exists, so not even the installer is fetched again.
    let before = env.server.received_requests().await.unwrap().len();
    assert_eq!(env.run(&r).await.unwrap(), ID);
    assert_eq!(env.server.received_requests().await.unwrap().len(), before);
}

#[tokio::test]
async fn a_failing_step_reports_the_log_tail_and_leaves_no_version() {
    let (env, _) = standard_env().await;
    let calls: Calls = Default::default();
    let r = runner(&calls, |spec| {
        std::fs::create_dir_all(spec.log.parent().unwrap()).unwrap();
        std::fs::write(&spec.log, "starting\nERROR: cannot patch the client jar\n").unwrap();
        Ok(1)
    });
    let err = env.run(&r).await.unwrap_err().to_string();
    assert!(err.contains("cannot patch the client jar"), "{err}");
    assert!(!env.version_dir().exists(), "a failed install must not leave a version");
}

#[tokio::test]
async fn an_output_with_the_wrong_hash_fails_and_is_removed() {
    let (env, _) = standard_env().await;
    let calls: Calls = Default::default();
    let out = env.out_path();
    let r = runner(&calls, move |_| {
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        std::fs::write(&out, b"something else").unwrap();
        Ok(0)
    });
    let err = env.run(&r).await.unwrap_err().to_string();
    assert!(err.contains("校验"), "{err}");
    assert!(!env.out_path().exists());
    assert!(!env.version_dir().exists());
}

#[tokio::test]
async fn cancelling_a_step_removes_its_partial_output() {
    let (env, _) = standard_env().await;
    let calls: Calls = Default::default();
    let out = env.out_path();
    let r = runner(&calls, move |_| {
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        std::fs::write(&out, b"half-written").unwrap();
        Err(Error::Cancelled)
    });
    let err = env.run(&r).await.unwrap_err();
    assert!(matches!(err, Error::Cancelled), "{err}");
    assert!(!env.out_path().exists());
    assert!(!env.version_dir().exists());
}

#[tokio::test]
async fn embedded_library_paths_cannot_escape_the_libraries_directory() {
    let env = Env::new().await;
    let tool = tool_jar();
    env.serve("/y-1.jar", &tool).await;
    let installer = modern_installer(
        &env,
        &tool,
        |p| p["libraries"][1]["downloads"]["artifact"]["path"] = json!("../../evil.jar"),
        version_json(ID, MC),
    );
    env.serve_installer(&installer).await;
    let calls: Calls = Default::default();
    assert!(env.run(&runner(&calls, writes_good_output(env.out_path()))).await.is_err());
    assert!(!env.tmp.path().join("evil.jar").exists());
    assert!(!env.tmp.path().parent().unwrap().join("evil.jar").exists());
    assert!(calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_version_json_for_another_minecraft_version_is_rejected() {
    let env = Env::new().await;
    let tool = tool_jar();
    env.serve("/y-1.jar", &tool).await;
    let installer = modern_installer(&env, &tool, |_| {}, version_json(ID, "1.20.4"));
    env.serve_installer(&installer).await;
    let calls: Calls = Default::default();
    let err = env.run(&runner(&calls, writes_good_output(env.out_path()))).await.unwrap_err().to_string();
    assert!(err.contains("1.21.1"), "{err}");
    assert!(!env.version_dir().exists());
}

#[tokio::test]
async fn a_step_whose_library_is_missing_names_it() {
    let env = Env::new().await;
    let tool = tool_jar();
    // Not served: the library download fails.
    let installer = modern_installer(&env, &tool, |_| {}, version_json(ID, MC));
    env.serve_installer(&installer).await;
    let calls: Calls = Default::default();
    assert!(env.run(&runner(&calls, writes_good_output(env.out_path()))).await.is_err());
    assert!(calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn legacy_installers_extract_the_universal_jar_and_get_a_canonical_id() {
    let env = Env::new().await;
    let legacy_profile = json!({
        "install": {"profileName": "Forge", "target": "1.21.1-Forge9.9.9", "path": "net.minecraftforge:forge:1.21.1-9.9.9",
                    "version": "Forge 9.9.9", "filePath": "forge-1.21.1-9.9.9-universal.jar", "minecraft": MC},
        "versionInfo": {"id": "1.21.1-Forge9.9.9", "type": "release", "mainClass": "net.minecraft.launchwrapper.Launch",
                         "inheritsFrom": MC, "jar": MC, "minecraftArguments": "--tweakClass x.Tweaker",
                         "libraries": [{"name": "net.minecraftforge:forge:1.21.1-9.9.9", "url": "https://maven.minecraftforge.net/"}]}
    });
    let installer = zip_bytes(&[
        ("install_profile.json", legacy_profile.to_string().as_bytes()),
        ("forge-1.21.1-9.9.9-universal.jar", b"universal-bytes"),
    ]);
    env.serve_installer(&installer).await;
    let calls: Calls = Default::default();
    let id = env.run(&runner(&calls, |_| panic!("legacy installers have no steps"))).await.unwrap();
    assert_eq!(id, ID);
    assert_eq!(
        std::fs::read(env.dirs.libraries().join("net/minecraftforge/forge/1.21.1-9.9.9/forge-1.21.1-9.9.9.jar")).unwrap(),
        b"universal-bytes"
    );
    let v: Value = serde_json::from_slice(&std::fs::read(env.version_dir().join(format!("{ID}.json"))).unwrap()).unwrap();
    assert_eq!(v["id"], ID);
    assert_eq!(v["inheritsFrom"], MC);
    assert_eq!(v["minecraftArguments"], "--tweakClass x.Tweaker");
}

#[tokio::test]
async fn legacy_profiles_without_inherits_from_inherit_from_the_minecraft_version() {
    let env = Env::new().await;
    let profile = json!({
        "install": {"path": "net.minecraftforge:forge:1.21.1-9.9.9", "filePath": "u.jar", "minecraft": MC},
        "versionInfo": {"id": "old", "mainClass": "m", "libraries": []}
    });
    env.serve_installer(&zip_bytes(&[("install_profile.json", profile.to_string().as_bytes()), ("u.jar", b"u")])).await;
    let calls: Calls = Default::default();
    env.run(&runner(&calls, |_| Ok(0))).await.unwrap();
    let v: Value = serde_json::from_slice(&std::fs::read(env.version_dir().join(format!("{ID}.json"))).unwrap()).unwrap();
    assert_eq!(v["inheritsFrom"], MC);
}
