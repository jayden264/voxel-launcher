# Forge / NeoForge Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (the user chose "Native" execution in batch 1 and told us to run the whole batch, including the real installs). Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create instances with Forge or NeoForge, install them by running the installer's steps ourselves, and reuse the batch-1 mod page.

**Architecture:** `loader.rs` grows two kinds and dispatches; a new `forge.rs` (shared by Forge and NeoForge) lists versions, parses `install_profile.json`, resolves `data`/placeholders, downloads libraries, runs processors with Java, verifies outputs, and writes the version JSON last. Pure parts are tested offline against real JSON fixtures; the Java runs are verified on this machine.

**Tech Stack:** Rust (`launcher-core`, zip, reqwest via `Downloader`, tokio process), Tauri 2, React/TS.

**Spec:** `docs/superpowers/specs/2026-11-15-forge-neoforge-design.md` (+ batch-1 spec/plan, already implemented)

## Global Constraints
- Windows x64; no new dependencies unless unavoidable (zip, serde, regex, tokio, tempfile, wiremock already present).
- Supported: Minecraft 1.7.10 and newer, official releases of Forge and NeoForge; new (`spec` + `processors`) and old (`install` + `versionInfo`) installer formats.
- All downloads go through `Downloader`; every path from remote JSON or from jar entries goes through `paths::safe_join`; `Command` args are an array, never a joined string.
- The version JSON is written last, atomically; a failed or cancelled install leaves no `versions/<id>/` directory.
- Libraries are verified by sha1 (from the profile or the `.sha1` sidecar); every processor `outputs` entry is verified by sha1 after the step.
- Canonical version ids chosen by us: Forge `{mc}-forge-{ver}`, NeoForge `neoforge-{ver}`. The installer's own `id` is rewritten to this (legacy installers use other ids).
- NeoForge instances search mods with loaders `["neoforge"]`, Forge with `["forge"]`.
- No git commits unless the user asks. Pre-existing clippy lints are left alone; new code must not add any.

## Review Focus
1. A processor exits non-zero or produces a file with the wrong sha1 → install fails with the last log lines, no `versions/<id>/` left. (Task 4)
2. Installer jar entries or profile paths containing `..`, absolute paths or drive letters → rejected before anything is written. (Task 3, 4)
3. Cancelling mid-processor → child process killed, partial outputs removed, next install starts clean. (Task 4)
4. A Minecraft version no Forge/NeoForge supports (or NeoForge for 1.7.10) → empty version list, UI option disabled. (Task 2, 6)
5. Re-running an install that already finished → no processor runs again (outputs already valid). (Task 4)

---

### Task 1: Loader kinds, ids, mirrors, mod targets

**Files:** Modify `crates/launcher-core/src/loader.rs`, `mods.rs` (`target_for`), `mirror.rs`, `src/lib/api.ts` (`LoaderKind`), `src/components/kit.tsx` (`LOADER_LABEL`). Fixtures in `crates/launcher-core/tests/fixtures/`: `forge-1.21.1-52.1.0.install_profile.json`, `forge-1.21.1-52.1.0.version.json`, `neoforge-21.1.172.install_profile.json`, `neoforge-21.1.172.version.json`, `forge-1.12.2-14.23.5.2860.install_profile.json`, `forge-1.12.2-14.23.5.2860.version.json`, `forge-1.7.10-10.13.4.1614.install_profile.json` (all extracted with `unzip -p` from the official installers; keep `icon` fields stripped to save space), `forge-maven-metadata.xml`, `neoforge-maven-metadata.xml`, `forge-promotions_slim.json` (fetched from the official sites).

**Interfaces:**
- Produces: `LoaderKind::{Fabric, Quilt, Forge, NeoForge}` (serde lowercase `forge`/`neoforge`; `label()` "Forge"/"NeoForge"); `profile_id(kind, loader_version, mc)` → `fabric-loader-{v}-{mc}` / `quilt-loader-{v}-{mc}` / `{mc}-forge-{v}` / `neoforge-{v}`; `mods::target_for` → Forge `["forge"]`, NeoForge `["neoforge"]`; TS `LoaderKind = 'fabric' | 'quilt' | 'forge' | 'neoforge'`; mirror rows for `https://maven.minecraftforge.net` and `https://maven.neoforged.net/releases` → `https://bmclapi2.bangbang93.com/maven` (verify each with a HEAD on a known artifact first; drop a row that does not return 200).

- [ ] **Step 1: Write failing tests** — `profile_ids_for_all_kinds` (the four ids above for `1.21.1`), `loader_kind_serde_is_lowercase` (`"forge"`, `"neoforge"`), `forge_targets_use_their_own_loader_name`, mirror tests for the verified rows (`to_mirror("https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml")` etc.).
- [ ] **Step 2: Run** `cargo test -p launcher-core profile_ids` → FAIL (variants missing).
- [ ] **Step 3: Implement** the variants, `profile_id`, `target_for`, mirror rows; make `meta_url` / `check_segment` callers compile (Forge/NeoForge arms of `list_loader_versions` return `Err` until Task 2).
- [ ] **Step 4: Run** `cargo test -p launcher-core` → all pass; `npx tsc -b` → the TS union change compiles.

### Task 2: Version lists

**Files:** Create `crates/launcher-core/src/forge.rs` (+ `pub mod forge;`); Modify `loader.rs` (dispatch `list_loader_versions` for Forge/NeoForge). Test: `crates/launcher-core/tests/forge_versions.rs`.

**Interfaces:**
- Produces in `forge.rs`:
  - `pub fn parse_forge_versions(metadata_xml: &str, promotions_json: &str, mc: &str) -> Vec<LoaderVersion>` — versions whose maven string starts with `{mc}-`; `LoaderVersion.version` = the part after `{mc}-` with a trailing `-{mc}` removed (`1.7.10-10.13.4.1614-1.7.10` → `10.13.4.1614`); newest first by `cmp_versions` (reuse from loader.rs, make it `pub(crate)`); `stable` = true for all; the promotions `recommended` version (else `latest`) is moved to the front.
  - `pub fn parse_neoforge_versions(metadata_xml: &str, mc: &str) -> Vec<LoaderVersion>` using `neoforge_prefix(mc) -> Option<String>`: `1.A.B` → `"A.B."` (`1.21` → `"21.0."`); new scheme `YY.N` or `YY.N.P` (first number ≥ 26) → `"YY.N.P."` with P=0 default; anything else (1.7–1.20.1 → None because NeoForge starts at 1.20.1: `1.20.1` → `"20.1."`, versions below 1.20.1 → None). `stable` = no `-` in the version.
  - `pub fn forge_installer_candidates(mc: &str, loader_version: &str) -> Vec<String>` — full maven versions to try (`{mc}-{v}` then `{mc}-{v}-{mc}`) as installer URLs `https://maven.minecraftforge.net/net/minecraftforge/forge/{full}/forge-{full}-installer.jar`; `pub fn neoforge_installer_url(loader_version: &str) -> String`.
  - `pub async fn list_versions(dl: &Downloader, kind: LoaderKind, mc: &str) -> Result<Vec<LoaderVersion>>` (fetches the two Forge documents / the NeoForge metadata through `Downloader::fetch`).

- [ ] **Step 1: Failing tests** with the real fixtures: `forge_1_21_1_lists_52_1_x_newest_first_recommended_first` (first = `52.1.20` per the fixture's promotions, contains `52.1.0`), `forge_1_7_10_strips_both_mc_suffixes` (`10.13.4.1614` present), `forge_unknown_mc_is_empty` (`1.2.3`), `neoforge_prefix_rules` (`1.21.1`→`21.1.`, `1.21`→`21.0.`, `1.20.1`→`20.1.`, `26.3`→`26.3.0.`, `1.19.4`→None, `1.7.10`→None), `neoforge_1_21_1_lists_21_1_x_with_betas_unstable`, `installer_urls_match_known_artifacts` (the two URLs HEAD-verified earlier: Forge `1.21.1-52.1.0`, NeoForge `21.1.172`).
- [ ] **Step 2: Run** `cargo test -p launcher-core --test forge_versions` → FAIL.
- [ ] **Step 3: Implement** parsing with `regex` (`<version>…</version>`), no XML crate.
- [ ] **Step 4: Run** → PASS; plus an `#[ignore]` live test `real_forge_and_neoforge_versions` (`cargo test -p launcher-core --test forge_versions -- --ignored`): Forge 1.21.1 and NeoForge 1.21.1 non-empty, NeoForge 1.19.4 empty.

### Task 3: Install profile parsing and placeholder resolution (pure)

**Files:** Modify `forge.rs`. Test: `crates/launcher-core/tests/forge_profile.rs`.

**Interfaces:**
- Produces:
  - `pub struct Processor { pub sides: Option<Vec<String>>, pub jar: String, pub classpath: Vec<String>, pub args: Vec<String>, pub outputs: BTreeMap<String, String> }`.
  - `pub struct ProfileLibrary { pub name: String, pub path: String, pub url: String, pub sha1: Option<String> }` (`url` empty ⇒ embedded in the installer's `maven/`).
  - `pub enum InstallProfile { Modern { minecraft: String, libraries: Vec<ProfileLibrary>, data: BTreeMap<String, DataEntry>, processors: Vec<Processor>, version_json_entry: String }, Legacy { version_info: serde_json::Value, universal_entry: String, universal_coord: String } }` and `pub struct DataEntry { pub client: String, pub server: String }`.
  - `pub fn parse_profile(install_profile_json: &[u8]) -> Result<InstallProfile>` — `Modern` when `processors` is present (version JSON entry from the `json` field, default `/version.json`), else `Legacy` from `install` + `versionInfo` (`filePath` → entry name, `path` → coordinate).
  - `pub struct Resolver<'a> { … }` with `Resolver::new(root: &Path, installer: &Path, minecraft_jar: &Path, mc: &str, libraries_dir: &Path, extract_dir: &Path) -> Self` and `pub fn data_value(&self, entry: &DataEntry) -> Result<DataValue>`; `pub enum DataValue { Path(PathBuf), Literal(String), InstallerFile(String /*entry name*/) }` (`[coord]` → `libraries/<maven path>` honouring `@ext` and classifier; `'x'` → literal; `/x` → installer entry that will be extracted to `extract_dir/x`).
  - `pub fn expand_arg(&self, arg: &str, vars: &BTreeMap<String, String>) -> Result<String>` — replaces `{NAME}` from `vars` then built-ins `{ROOT}`, `{INSTALLER}`, `{MINECRAFT_JAR}`, `{SIDE}`=`client`, `{MINECRAFT_VERSION}`, `{LIBRARY_DIR}`; an argument that is itself `[coord]` becomes the library path; unknown `{X}` → error naming X.
  - `pub fn runs_on_client(p: &Processor) -> bool` (`sides` absent or contains `client`).

- [ ] **Step 1: Failing tests** (fixtures): `modern_forge_profile_parses_seven_processors_and_21_libraries`; `neoforge_profile_parses_ten_processors`; `forge_1_12_2_is_modern` (processors present); `forge_1_7_10_is_legacy` with `universal_coord`; `client_side_filter` (Forge 1.21.1: processors that run on client = 4 of 7 — the three `server`-only ones and the two jar-splitter-style server ones are excluded; assert the exact count from the fixture); `data_value_forms` (`[net.minecraft:client:1.21.1:mappings@tsrg]` → `libraries/net/minecraft/client/1.21.1/client-1.21.1-mappings.tsrg`; `'2a80…'` literal; `/data/client.lzma` installer entry); `expand_arg_builtins_and_data` and `expand_arg_unknown_placeholder_errors`; `unsafe_library_paths_rejected` (a profile library path `../x.jar` → error from `parse_profile` consumers via `safe_join`).
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement**. **Step 4: Run** → PASS.

### Task 4: Install executor

**Files:** Modify `forge.rs`; Modify `loader.rs` (`install_loader` gains `java: Option<&Path>` and dispatches). Test: `crates/launcher-core/tests/forge_install.rs` (wiremock + a fake `java` is not possible on Windows without a binary, so the process runner takes a `trait`-free seam: `pub type RunFn = Arc<dyn Fn(&ProcessSpec) -> Result<ProcessOutcome> + Send + Sync>` defaulting to the real runner; tests inject a stub).

**Interfaces:**
- Produces: `pub struct ProcessSpec { pub java: PathBuf, pub args: Vec<String>, pub log: PathBuf }`, `pub struct ProcessOutcome { pub code: i32 }`; `pub async fn install_forge_like(dl: &Downloader, dirs: &Dirs, kind: LoaderKind, mc: &str, loader_version: &str, java: &Path, progress: Option<ProgressFn>) -> Result<String>` returning the canonical profile id; internal `install_forge_like_with(…, run: RunFn)` used by tests.
- Behaviour: (1) vanilla `install_version(mc)` first; (2) installer jar → `shared/installers/<file>` (`.sha1` sidecar); (3) `parse_profile`; (4) Modern: download `libraries` (embedded ones extracted from `maven/…` entries of the jar with `safe_join`), extract needed `data` entries, then for each client processor: skip if all `outputs` already valid (sha1 of resolved paths), else spawn `java -cp <jar>;<classpath…> <Main-Class> <expanded args>` (cwd `shared/`), log to `shared/logs/forge-<id>.log`, non-zero exit or later output mismatch ⇒ `Error::Other` containing the last 20 log lines; the child is killed when `dl.cancel_token()` fires and partial outputs removed; (5) read the version JSON, rewrite `id` to the canonical id (Modern: require `inheritsFrom == mc`; Legacy: set `inheritsFrom = mc` when absent and extract the universal jar to `libraries/<coord path>`), write `versions/<id>/<id>.json` atomically **last**, then `install_version(id)` so remaining libraries/assets download.

- [ ] **Step 1: Failing tests:** `runs_processors_in_order_with_expanded_args` (stub records `ProcessSpec`s; assert order, `-cp` content, expanded `{MINECRAFT_JAR}`), `skips_processors_whose_outputs_are_valid` (pre-create outputs with correct sha1 → stub never called; second install call also never calls it — Review Focus 5), `failing_processor_leaves_no_version_dir_and_reports_log_tail` (stub returns code 1 after writing a log line; assert error contains the line and `versions/<id>` absent — Review Focus 1), `output_hash_mismatch_fails` , `zip_slip_entry_in_installer_is_rejected` (installer with entry `maven/../../evil.jar` → Err, nothing outside `libraries/` created — Review Focus 2), `cancel_removes_partial_outputs_and_kills` (stub observes cancellation, returns `Err(Cancelled)`; partial output file deleted — Review Focus 3), `legacy_installer_extracts_universal_and_writes_version` (small synthetic legacy installer zip built in the test from the 1.7.10 fixture JSON), `canonical_id_rewrites_installer_id`. Build tiny installer zips in the test with the `zip` crate; wiremock serves them and their `.sha1`.
- [ ] **Step 2: Run** `cargo test -p launcher-core --test forge_install` → FAIL. **Step 3: Implement** (real runner: `tokio::process::Command`, `CREATE_NO_WINDOW`, stdout+stderr appended to the log file, `kill_on_drop` + select on cancel). **Step 4: Run** → PASS; then `cargo test -p launcher-core` → all pass.

### Task 5: Wiring (Tauri, CLI)

**Files:** Modify `src-tauri/src/lib.rs` (`ensure_instance_version`, `install_instance`), `crates/launcher-core/src/bin/mcl.rs`.

**Interfaces:** `ensure_instance_version` for Forge/NeoForge first installs the vanilla version, then resolves the Java runtime for that version's `javaVersion.component` (via `java::ensure_runtime`, honouring the instance/global `java_path` override with `probe_java`), then calls `loader::install_loader(…, java, …)`. `mcl` gains `loader-versions`/`install-loader` support for `forge|neoforge` (`parse_kind`) and `mcl install-loader <kind> <mc> [ver] [--java PATH]`.

- [ ] **Step 1:** failing test in `src-tauri`: `forge_like_kinds_need_java` — a pure helper `fn needs_java(kind: LoaderKind) -> bool` (true for Forge/NeoForge) used by `ensure_instance_version`. **Step 2:** run → FAIL. **Step 3:** implement helper + wiring. **Step 4:** `cargo test --workspace` → pass; `cargo build -p launcher-core --bin mcl` ok.

### Task 6: Frontend

**Files:** Modify `src/pages/Download.tsx` (`LOADERS`, notice), `src/components/kit.tsx` (`LOADER_LABEL`), `src/lib/api.ts` (mock list for forge/neoforge, 1.7.10/1.12.2 supported for Forge only), `src/pages/Mods.tsx` (nothing expected).

- [ ] Behaviour: the loader segment shows 原版 / Fabric / Quilt / Forge / NeoForge; unsupported ones disabled with the tooltip; choosing Forge shows one muted line "Forge 官方希望自动安装时赞助项目" with a link to `https://www.patreon.com/LexManos/` opened with the system browser (`@tauri-apps/plugin-opener` `openUrl`; plain `<a target=_blank>` in the mock); the version select/beta toggle work as for Fabric; stage text during install already comes from the backend.
- [ ] Verify: `npx tsc -b && npm run build`; browser pane (mock): 1.21.1 dialog shows all four loaders, 1.7.10 shows only Forge enabled, Forge selected shows the notice; Instances card badge shows `Forge 52.1.20`.

### Task 7: Real installs and launches (user-approved)

Use the user's data dir with temporary instances named `verify-*`, removed afterwards (selection restored).

- [ ] Forge 1.21.1 (`mcl install-loader forge 1.21.1`), NeoForge 21.1.172 (`neoforge 1.21.1 21.1.172`), Forge 1.12.2 (default version), Forge 1.7.10 (`10.13.4.1614`): each installs, `mcl mod-add <project> --loader <kind> --mc <mc>` with a mod that exists for that loader/version (pick via `mcl mod-search`), `mcl launch <profile id> --instance verify-…`.
- [ ] Expected for each: `logs/latest.log` shows the loader name and the mod, no `/ERROR]` lines caused by the launcher, no crash report. Failures are fixed in the owning task (not skipped).
- [ ] Re-run `install-loader` for one of them and confirm no processor runs (log unchanged, fast).
- [ ] In the real window (WebView2 debug port as in batch 1, closed afterwards): create a NeoForge instance through `api.createInstance` + `installInstance`, install a mod through `modsPlan`/`modsInstall`, launch, kill, delete the instance.

### Task 8: Final checks

- [ ] `cargo test --workspace`, `npx tsc -b`, `npm run build` green; restart the dev app without the debug port; ledger, rulings and deferred minors reported.
