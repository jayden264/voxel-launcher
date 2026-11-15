//! Version manifest, version JSON model, rule evaluation, `inheritsFrom`
//! merging, library resolution and launch-argument expansion.

use crate::paths::{self, Maven};
use crate::{Error, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

pub const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
pub const DEFAULT_LIBRARY_BASE: &str = "https://libraries.minecraft.net/";
/// Oldest version we support; its `releaseTime` is the lower bound for the version list.
pub const OLDEST_SUPPORTED: &str = "1.7.10";

// ------------------------------------------------------------------ manifest

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VersionManifest {
    pub latest: Latest,
    pub versions: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Latest {
    pub release: String,
    pub snapshot: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ManifestEntry {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
    #[serde(default)]
    pub time: String,
    #[serde(rename = "releaseTime")]
    pub release_time: String,
    #[serde(default)]
    pub sha1: Option<String>,
}

impl VersionManifest {
    pub fn find(&self, id: &str) -> Option<&ManifestEntry> {
        self.versions.iter().find(|v| v.id == id)
    }

    /// `release`/`snapshot` entries released no earlier than 1.7.10.
    /// The lower bound is read from the manifest itself. Mojang's `releaseTime`
    /// values share one ISO-8601 format, so string comparison is chronological.
    pub fn supported(&self) -> Vec<&ManifestEntry> {
        let floor = self.find(OLDEST_SUPPORTED).map(|e| e.release_time.as_str());
        self.versions
            .iter()
            .filter(|v| v.kind == "release" || v.kind == "snapshot")
            .filter(|v| floor.map_or(true, |f| v.release_time.as_str() >= f))
            .collect()
    }
}

// -------------------------------------------------------------- version json

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionJson {
    pub id: String,
    #[serde(default)]
    pub inherits_from: Option<String>,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub main_class: Option<String>,
    #[serde(default)]
    pub minecraft_arguments: Option<String>,
    #[serde(default)]
    pub arguments: Option<Arguments>,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(default)]
    pub asset_index: Option<AssetIndexRef>,
    #[serde(default)]
    pub assets: Option<String>,
    #[serde(default)]
    pub downloads: Option<Downloads>,
    #[serde(default)]
    pub java_version: Option<JavaVersion>,
    #[serde(default)]
    pub logging: Option<Logging>,
    #[serde(default)]
    pub release_time: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<ArgValue>,
    #[serde(default)]
    pub jvm: Vec<ArgValue>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ArgValue {
    Plain(String),
    Conditional { rules: Vec<Rule>, value: OneOrMany },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Artifact {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Downloads {
    #[serde(default)]
    pub client: Option<Artifact>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndexRef {
    pub id: String,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersion {
    pub component: String,
    pub major_version: u32,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Logging {
    #[serde(default)]
    pub client: Option<LoggingClient>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LoggingClient {
    pub argument: String,
    pub file: LoggingFile,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LoggingFile {
    pub id: String,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    pub url: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub downloads: Option<LibDownloads>,
    /// Maven repository base for coordinate-only libraries (Fabric, Quilt, ...).
    #[serde(default)]
    pub url: Option<String>,
    /// Inline checksum/size, present on Fabric profile libraries.
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub rules: Option<Vec<Rule>>,
    #[serde(default)]
    pub natives: Option<HashMap<String, String>>,
    #[serde(default)]
    pub extract: Option<Extract>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct LibDownloads {
    #[serde(default)]
    pub artifact: Option<Artifact>,
    #[serde(default)]
    pub classifiers: Option<HashMap<String, Artifact>>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Extract {
    #[serde(default)]
    pub exclude: Vec<String>,
}

// -------------------------------------------------------------------- rules

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Rule {
    pub action: String,
    #[serde(default)]
    pub os: Option<OsRule>,
    #[serde(default)]
    pub features: Option<HashMap<String, bool>>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct OsRule {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arch: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RuleContext {
    pub os_name: String,
    pub os_arch: String,
    pub os_version: String,
    pub features: HashMap<String, bool>,
}

impl RuleContext {
    pub fn current() -> Self {
        let os_name = if cfg!(target_os = "windows") {
            "windows"
        } else if cfg!(target_os = "macos") {
            "osx"
        } else {
            "linux"
        };
        let os_arch = match std::env::consts::ARCH {
            "x86_64" => "x86_64",
            "x86" => "x86",
            "aarch64" => "arm64",
            other => other,
        };
        Self {
            os_name: os_name.into(),
            os_arch: os_arch.into(),
            os_version: "10.0".into(),
            features: HashMap::new(),
        }
    }
}

fn rule_matches(rule: &Rule, ctx: &RuleContext) -> bool {
    if let Some(os) = &rule.os {
        if os.name.as_deref().map_or(false, |n| n != ctx.os_name) {
            return false;
        }
        if os.arch.as_deref().map_or(false, |a| a != ctx.os_arch) {
            return false;
        }
        if let Some(v) = &os.version {
            match Regex::new(v) {
                Ok(re) if re.is_match(&ctx.os_version) => {}
                _ => return false,
            }
        }
    }
    if let Some(features) = &rule.features {
        for (k, want) in features {
            if ctx.features.get(k).copied().unwrap_or(false) != *want {
                return false;
            }
        }
    }
    true
}

/// Mojang semantics: no rules => allowed; otherwise start disallowed and let
/// each matching rule, in order, set the result.
pub fn rules_allow(rules: &[Rule], ctx: &RuleContext) -> bool {
    if rules.is_empty() {
        return true;
    }
    let mut allowed = false;
    for r in rules {
        if rule_matches(r, ctx) {
            allowed = r.action == "allow";
        }
    }
    allowed
}

// ---------------------------------------------------------------- libraries

#[derive(Debug, Clone)]
pub struct LibFile {
    pub key: String,
    /// Path relative to the shared `libraries/` directory.
    pub rel_path: String,
    pub url: String,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    /// Maven-only library without a checksum: try `<url>.sha1`.
    pub sidecar: bool,
    pub on_classpath: bool,
    /// Extract into the natives directory at launch.
    pub extract: bool,
    pub extract_exclude: Vec<String>,
}

fn join_base(base: &str, path: &str) -> String {
    if base.ends_with('/') {
        format!("{base}{path}")
    } else {
        format!("{base}/{path}")
    }
}

fn lib_key(name: &str) -> String {
    Maven::parse(name).map(|m| m.key()).unwrap_or_else(|| name.to_string())
}

/// Expand one library entry into the files we need, honouring rules and both
/// the legacy `natives` mechanism and the modern classifier-in-name style.
pub fn resolve_library(lib: &Library, ctx: &RuleContext) -> Vec<LibFile> {
    if let Some(rules) = &lib.rules {
        if !rules_allow(rules, ctx) {
            return vec![];
        }
    }
    let mut out = Vec::new();
    let artifact = lib.downloads.as_ref().and_then(|d| d.artifact.as_ref());
    let key = lib_key(&lib.name);

    // Legacy natives: `natives: {"windows": "natives-windows"}` + classifiers.
    if let Some(natives) = &lib.natives {
        if let Some(tmpl) = natives.get(&ctx.os_name) {
            let arch_bits = if ctx.os_arch == "x86" { "32" } else { "64" };
            let classifier = tmpl.replace("${arch}", arch_bits);
            let from_json = lib
                .downloads
                .as_ref()
                .and_then(|d| d.classifiers.as_ref())
                .and_then(|c| c.get(&classifier));
            let m = Maven::parse(&lib.name);
            let (rel, url, sha1, size, sidecar) = match (from_json, &m) {
                (Some(a), _) if a.path.is_some() => (
                    a.path.clone().unwrap(),
                    a.url.clone(),
                    a.sha1.clone(),
                    a.size,
                    false,
                ),
                (_, Some(m)) => {
                    let mut with = m.clone();
                    with.classifier = Some(classifier.clone());
                    let rel = with.path();
                    let base = lib.url.as_deref().unwrap_or(DEFAULT_LIBRARY_BASE);
                    (rel.clone(), join_base(base, &rel), None, None, true)
                }
                _ => return out,
            };
            out.push(LibFile {
                key: format!("{key}:{classifier}"),
                rel_path: rel,
                url,
                sha1,
                size,
                sidecar,
                on_classpath: false,
                extract: true,
                extract_exclude: lib
                    .extract
                    .as_ref()
                    .map(|e| e.exclude.clone())
                    .unwrap_or_else(|| vec!["META-INF/".into()]),
            });
        }
    }

    // Main artifact (also covers modern `...:natives-windows` libraries).
    match artifact {
        Some(a) if a.path.is_some() => {
            out.push(LibFile {
                key: key.clone(),
                rel_path: a.path.clone().unwrap(),
                // An empty url marks a file the loader's installer generates: no download source.
                url: a.url.clone(),
                sha1: a.sha1.clone().filter(|h| !h.is_empty()),
                size: a.size.filter(|n| *n > 0),
                sidecar: false,
                on_classpath: true,
                extract: lib.extract.is_some() && lib.natives.is_none(),
                extract_exclude: lib.extract.as_ref().map(|e| e.exclude.clone()).unwrap_or_default(),
            });
        }
        None if lib.natives.is_none() => {
            // Coordinate-only library (Fabric/Quilt/Forge style).
            if let Some(m) = Maven::parse(&lib.name) {
                let rel = m.path();
                let base = lib.url.as_deref().unwrap_or(DEFAULT_LIBRARY_BASE);
                out.push(LibFile {
                    key,
                    url: join_base(base, &rel),
                    rel_path: rel,
                    sha1: lib.sha1.clone(),
                    size: lib.size,
                    sidecar: lib.sha1.is_none(),
                    on_classpath: true,
                    extract: false,
                    extract_exclude: vec![],
                });
            }
        }
        _ => {}
    }
    out
}

pub fn resolve_libraries(version: &VersionJson, ctx: &RuleContext) -> Vec<LibFile> {
    version.libraries.iter().flat_map(|l| resolve_library(l, ctx)).collect()
}

// -------------------------------------------------------------- inheritance

/// Merge a child (e.g. Fabric profile) over its parent (vanilla).
pub fn merge(parent: VersionJson, child: VersionJson) -> VersionJson {
    // Child libraries first; dedupe on group:artifact[:classifier] so the
    // child's version wins over the parent's.
    let mut seen = std::collections::HashSet::new();
    let mut libraries = Vec::new();
    for lib in child.libraries.into_iter().chain(parent.libraries) {
        if seen.insert(lib_key(&lib.name)) {
            libraries.push(lib);
        }
    }
    let arguments = match (parent.arguments, child.arguments) {
        (Some(p), Some(c)) => Some(Arguments {
            game: p.game.into_iter().chain(c.game).collect(),
            jvm: p.jvm.into_iter().chain(c.jvm).collect(),
        }),
        (p, c) => c.or(p),
    };
    VersionJson {
        id: child.id,
        inherits_from: None,
        kind: child.kind.or(parent.kind),
        main_class: child.main_class.or(parent.main_class),
        minecraft_arguments: child.minecraft_arguments.or(parent.minecraft_arguments),
        arguments,
        libraries,
        asset_index: child.asset_index.or(parent.asset_index),
        assets: child.assets.or(parent.assets),
        downloads: child.downloads.or(parent.downloads),
        java_version: child.java_version.or(parent.java_version),
        logging: child.logging.or(parent.logging),
        release_time: child.release_time.or(parent.release_time),
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedVersion {
    pub json: VersionJson,
    /// Id whose `.jar` holds the game classes (root of the inheritance chain).
    pub jar_id: String,
    /// `[requested, parent, grandparent, ...]`
    pub chain: Vec<String>,
}

pub fn version_json_path(versions_dir: &Path, id: &str) -> Result<std::path::PathBuf> {
    paths::safe_join(versions_dir, &format!("{id}/{id}.json"))
}

/// Read `<versions>/<id>/<id>.json` and everything it inherits from.
pub fn load_resolved(versions_dir: &Path, id: &str) -> Result<ResolvedVersion> {
    let mut chain = vec![id.to_string()];
    let mut jsons = Vec::new();
    let mut current = id.to_string();
    loop {
        let path = version_json_path(versions_dir, &current)?;
        let bytes = std::fs::read(&path).map_err(|_| Error::Missing(path.clone()))?;
        let v: VersionJson = serde_json::from_slice(&bytes)?;
        let parent = v.inherits_from.clone();
        jsons.push(v);
        match parent {
            Some(p) => {
                if chain.len() >= 8 || chain.contains(&p) {
                    return Err(Error::Other(format!("版本继承链异常: {chain:?}")));
                }
                chain.push(p.clone());
                current = p;
            }
            None => break,
        }
    }
    let jar_id = chain.last().cloned().unwrap();
    let mut it = jsons.into_iter().rev();
    let mut merged = it.next().unwrap();
    for child in it {
        merged = merge(merged, child);
    }
    Ok(ResolvedVersion { json: merged, jar_id, chain })
}

// ---------------------------------------------------------------- arguments

fn placeholder_re() -> Regex {
    Regex::new(r"\$\{([^}]+)\}").unwrap()
}

/// Replace `${name}` placeholders. Returns `None` if any placeholder has no value.
pub fn substitute(token: &str, vars: &HashMap<String, String>) -> Option<String> {
    let re = placeholder_re();
    let mut missing = false;
    let out = re.replace_all(token, |caps: &regex::Captures| match vars.get(&caps[1]) {
        Some(v) => v.clone(),
        None => {
            missing = true;
            String::new()
        }
    });
    if missing {
        None
    } else {
        Some(out.into_owned())
    }
}

/// Substitute placeholders token by token. A token whose placeholder cannot be
/// resolved is dropped together with the flag that precedes it.
/// Substitution happens *after* tokenising, so values with spaces stay intact.
pub fn expand_tokens(tokens: Vec<String>, vars: &HashMap<String, String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(tokens.len());
    for t in tokens {
        match substitute(&t, vars) {
            Some(s) => out.push(s),
            None => {
                if let Some(prev) = out.last() {
                    if prev.starts_with('-') && !prev.contains('=') {
                        out.pop();
                    }
                }
            }
        }
    }
    out
}

fn flatten(values: &[ArgValue], ctx: &RuleContext) -> Vec<String> {
    let mut out = Vec::new();
    for v in values {
        match v {
            ArgValue::Plain(s) => out.push(s.clone()),
            ArgValue::Conditional { rules, value } => {
                if rules_allow(rules, ctx) {
                    match value {
                        OneOrMany::One(s) => out.push(s.clone()),
                        OneOrMany::Many(m) => out.extend(m.iter().cloned()),
                    }
                }
            }
        }
    }
    out
}

/// Raw (unexpanded) JVM argument tokens from the version JSON.
pub fn jvm_tokens(version: &VersionJson, ctx: &RuleContext) -> Vec<String> {
    match &version.arguments {
        Some(a) if !a.jvm.is_empty() => flatten(&a.jvm, ctx),
        // Legacy versions carry no JVM arguments: supply the standard ones.
        _ => vec![
            "-Djava.library.path=${natives_directory}".into(),
            "-cp".into(),
            "${classpath}".into(),
        ],
    }
}

/// Raw game argument tokens (new `arguments.game` or legacy `minecraftArguments`).
pub fn game_tokens(version: &VersionJson, ctx: &RuleContext) -> Vec<String> {
    if let Some(a) = &version.arguments {
        if !a.game.is_empty() {
            return flatten(&a.game, ctx);
        }
    }
    version
        .minecraft_arguments
        .as_deref()
        .map(|s| s.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn ctx() -> RuleContext {
        RuleContext {
            os_name: "windows".into(),
            os_arch: "x86_64".into(),
            os_version: "10.0".into(),
            features: HashMap::new(),
        }
    }

    #[test]
    fn rules_semantics() {
        let allow_all: Vec<Rule> = serde_json::from_str(r#"[{"action":"allow"}]"#).unwrap();
        assert!(rules_allow(&allow_all, &ctx()));
        let not_osx: Vec<Rule> = serde_json::from_str(
            r#"[{"action":"allow"},{"action":"disallow","os":{"name":"osx"}}]"#,
        )
        .unwrap();
        assert!(rules_allow(&not_osx, &ctx()));
        let only_osx: Vec<Rule> =
            serde_json::from_str(r#"[{"action":"allow","os":{"name":"osx"}}]"#).unwrap();
        assert!(!rules_allow(&only_osx, &ctx()));
        let x86_only: Vec<Rule> =
            serde_json::from_str(r#"[{"action":"allow","os":{"name":"windows","arch":"x86"}}]"#)
                .unwrap();
        assert!(!rules_allow(&x86_only, &ctx()));
        let win10: Vec<Rule> = serde_json::from_str(
            r#"[{"action":"allow","os":{"name":"windows","version":"^10\\."}}]"#,
        )
        .unwrap();
        assert!(rules_allow(&win10, &ctx()));
        let demo: Vec<Rule> =
            serde_json::from_str(r#"[{"action":"allow","features":{"is_demo_user":true}}]"#)
                .unwrap();
        assert!(!rules_allow(&demo, &ctx()));
        let mut c = ctx();
        c.features.insert("is_demo_user".into(), true);
        assert!(rules_allow(&demo, &c));
    }

    #[test]
    fn unresolved_placeholders_drop_flag_and_value() {
        let v = vars(&[("auth_player_name", "Steve")]);
        let tokens = ["--username", "${auth_player_name}", "--clientId", "${clientid}", "--xuid", "${auth_xuid}", "--demo"]
            .map(String::from)
            .to_vec();
        assert_eq!(
            expand_tokens(tokens, &v),
            vec!["--username", "Steve", "--demo"]
        );
    }

    #[test]
    fn tokenise_before_substitute_keeps_spaces() {
        let ver = VersionJson {
            id: "1.8.9".into(),
            minecraft_arguments: Some("--gameDir ${game_directory} --username ${auth_player_name}".into()),
            ..Default::default()
        };
        let v = vars(&[("game_directory", r"C:\Users\张 三\mc"), ("auth_player_name", "Steve")]);
        let out = expand_tokens(game_tokens(&ver, &ctx()), &v);
        assert_eq!(out, vec!["--gameDir", r"C:\Users\张 三\mc", "--username", "Steve"]);
    }

    #[test]
    fn merge_dedupes_libraries_by_key_child_wins() {
        let parent: VersionJson = serde_json::from_str(
            r#"{"id":"1.21.1","mainClass":"net.minecraft.client.main.Main",
                "libraries":[{"name":"org.ow2.asm:asm:9.3"},{"name":"com.google.guava:guava:31.0"}],
                "arguments":{"game":["--a"],"jvm":["-cp","${classpath}"]}}"#,
        )
        .unwrap();
        let child: VersionJson = serde_json::from_str(
            r#"{"id":"fabric-loader-0.16.9-1.21.1","inheritsFrom":"1.21.1",
                "mainClass":"net.fabricmc.loader.impl.launch.knot.KnotClient",
                "libraries":[{"name":"org.ow2.asm:asm:9.6","url":"https://maven.fabricmc.net/"}],
                "arguments":{"game":["--b"],"jvm":["-DFabricMcEmu= net.minecraft.client.main.Main "]}}"#,
        )
        .unwrap();
        let m = merge(parent, child);
        assert_eq!(m.id, "fabric-loader-0.16.9-1.21.1");
        assert_eq!(m.main_class.as_deref(), Some("net.fabricmc.loader.impl.launch.knot.KnotClient"));
        let names: Vec<_> = m.libraries.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, vec!["org.ow2.asm:asm:9.6", "com.google.guava:guava:31.0"]);
        assert_eq!(m.arguments.unwrap().game.len(), 2);
    }

    #[test]
    fn resolves_legacy_natives_and_modern_libs() {
        let lib: Library = serde_json::from_str(
            r#"{"name":"org.lwjgl.lwjgl:lwjgl-platform:2.9.4-nightly-20150209",
                "natives":{"windows":"natives-windows","linux":"natives-linux"},
                "extract":{"exclude":["META-INF/"]},
                "downloads":{"classifiers":{"natives-windows":{
                    "path":"org/lwjgl/lwjgl/lwjgl-platform/2.9.4-nightly-20150209/lwjgl-platform-2.9.4-nightly-20150209-natives-windows.jar",
                    "sha1":"abc","size":10,"url":"https://libraries.minecraft.net/x.jar"}}}}"#,
        )
        .unwrap();
        let files = resolve_library(&lib, &ctx());
        assert_eq!(files.len(), 1);
        assert!(files[0].extract && !files[0].on_classpath);
        assert!(files[0].rel_path.ends_with("natives-windows.jar"));

        // Modern: separate classifier library gated by rules.
        let modern: Library = serde_json::from_str(
            r#"{"name":"org.lwjgl:lwjgl:3.3.3:natives-windows-x86",
                "rules":[{"action":"allow","os":{"name":"windows","arch":"x86"}}],
                "downloads":{"artifact":{"path":"p.jar","sha1":"s","size":1,"url":"https://libraries.minecraft.net/p.jar"}}}"#,
        )
        .unwrap();
        assert!(resolve_library(&modern, &ctx()).is_empty());
    }

    #[test]
    fn coordinate_only_library_uses_repo_base_and_sidecar() {
        let lib: Library = serde_json::from_str(
            r#"{"name":"net.fabricmc:fabric-loader:0.16.9","url":"https://maven.fabricmc.net/"}"#,
        )
        .unwrap();
        let f = resolve_library(&lib, &ctx());
        assert_eq!(f.len(), 1);
        assert_eq!(
            f[0].url,
            "https://maven.fabricmc.net/net/fabricmc/fabric-loader/0.16.9/fabric-loader-0.16.9.jar"
        );
        assert!(f[0].sidecar && f[0].sha1.is_none() && f[0].on_classpath);
    }

    const FABRIC_PROFILE: &str = include_str!("../tests/fixtures/fabric-loader-0.16.9-1.21.1.json");

    #[test]
    fn fabric_profile_libraries_carry_inline_sha1() {
        let v: VersionJson = serde_json::from_str(FABRIC_PROFILE).unwrap();
        let files = resolve_libraries(&v, &ctx());
        assert!(!files.is_empty());
        // Real profile: six libraries carry sha1; loader + intermediary do not
        // and must keep using the `.sha1` sidecar.
        for f in &files {
            assert_eq!(f.sidecar, f.sha1.is_none(), "{}", f.rel_path);
        }
        assert_eq!(files.iter().filter(|f| f.sha1.is_some()).count(), 6);
        assert_eq!(files.iter().filter(|f| f.sidecar).count(), 2);
        let asm = files.iter().find(|f| f.key == "org.ow2.asm:asm").expect("asm");
        assert_eq!(asm.sha1.as_deref(), Some("f0ed132a49244b042cd0e15702ab9f2ce3cc8436"));
        assert_eq!(asm.size, Some(126093));
    }

    #[test]
    fn library_without_sha1_still_uses_sidecar() {
        let lib: Library = serde_json::from_str(
            r#"{"name":"org.quiltmc:quilt-json5:1.0.4","url":"https://maven.quiltmc.org/repository/release/"}"#,
        )
        .unwrap();
        let f = resolve_library(&lib, &ctx());
        assert_eq!(f.len(), 1);
        assert!(f[0].sidecar && f[0].sha1.is_none());
    }

    #[test]
    fn fabric_jvm_arg_is_one_token() {
        let v: VersionJson = serde_json::from_str(FABRIC_PROFILE).unwrap();
        let toks = jvm_tokens(&v, &ctx());
        let n = toks.iter().filter(|t| t.as_str() == "-DFabricMcEmu= net.minecraft.client.main.Main ").count();
        assert_eq!(n, 1, "{toks:?}");
    }

    #[test]
    fn installer_generated_library_without_url_stays_on_the_classpath() {
        // Forge/NeoForge list files their installer produces (`...:client`, `...:universal`)
        // with an empty url and sha1: nothing to download, but they must be on the classpath.
        let lib: Library = serde_json::from_str(
            r#"{"name":"net.minecraftforge:forge:1.21.1-52.1.0:client",
                "downloads":{"artifact":{"path":"net/minecraftforge/forge/1.21.1-52.1.0/forge-1.21.1-52.1.0-client.jar",
                                          "url":"","sha1":"","size":0}}}"#,
        )
        .unwrap();
        let f = resolve_library(&lib, &ctx());
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].rel_path, "net/minecraftforge/forge/1.21.1-52.1.0/forge-1.21.1-52.1.0-client.jar");
        assert!(f[0].on_classpath);
        assert!(f[0].url.is_empty(), "no download source");
        assert!(f[0].sha1.is_none(), "an empty sha1 is no sha1");
    }

    #[test]
    fn supported_versions_use_manifest_floor() {
        let m: VersionManifest = serde_json::from_str(
            r#"{"latest":{"release":"1.21.1","snapshot":"x"},"versions":[
              {"id":"1.21.1","type":"release","url":"u","releaseTime":"2024-08-08T12:24:45+00:00"},
              {"id":"1.7.10","type":"release","url":"u","releaseTime":"2014-06-26T00:00:00+00:00"},
              {"id":"1.7.9","type":"release","url":"u","releaseTime":"2014-04-14T00:00:00+00:00"},
              {"id":"b1.7.3","type":"old_beta","url":"u","releaseTime":"2011-07-08T00:00:00+00:00"},
              {"id":"14w02a","type":"snapshot","url":"u","releaseTime":"2014-01-09T00:00:00+00:00"},
              {"id":"1.8-pre1","type":"snapshot","url":"u","releaseTime":"2014-07-01T00:00:00+00:00"}]}"#,
        )
        .unwrap();
        let ids: Vec<_> = m.supported().iter().map(|e| e.id.clone()).collect();
        assert_eq!(ids, vec!["1.21.1", "1.7.10", "1.8-pre1"]);
    }
}
