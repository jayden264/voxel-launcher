# 常用模组提示、光影/资源包、整合包 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans (Native; the user asked to run the whole thing). Checkbox steps.

**Goal:** Recommended-mods prompt when creating an instance; shader/resource-pack content pages; one-click Modrinth modpack install.

**Architecture:** Build on `mods.rs` (plan → confirm → download). `PlanFile` gains a destination folder; a new `recommended.rs` and `modpack.rs` in `launcher-core`; thin Tauri commands; React changes in the create dialog, the mods page (becomes "内容") and the download page (整合包 tab).

**Tech Stack:** Rust (launcher-core, zip, wiremock tests), Tauri 2, React/TS.

**Spec:** `docs/superpowers/specs/2026-11-15-content-and-modpacks-design.md`

## Global Constraints
- All downloads through `Downloader`; every path from remote JSON or archive entries through `paths::safe_join`; no new dependencies.
- Modrinth API UA `voxel-launcher/0.1`; mod/shader/resource-pack files: sha1 verified, only from `https://cdn.modrinth.com/` (Tauri-side `validate_plan` keeps this rule); modpack files: hosts `cdn.modrinth.com`, `github.com`, `raw.githubusercontent.com`, `gitlab.com` only, over https, sha1 required.
- Shader and resource-pack files are `.zip` and go to `shaderpacks/` / `resourcepacks/`; mods are `.jar` in `mods/`. No enable/disable for packs.
- An unfinished modpack install must not leave its instance behind.
- No git commits unless asked. New code adds no clippy lints.

## Review Focus
1. A recommended mod with no build for this Minecraft version/loader is simply not offered (never an error dialog). (T1)
2. Two chosen extras sharing a dependency (Iris + Sodium) install the dependency once. (T1)
3. A modpack `files[].path` or `overrides/` entry with `..`, absolute or drive paths, or a non-whitelisted download host → rejected before anything is written. (T6, T7)
4. A modpack whose loader/Minecraft version cannot be installed, or a download that fails → no instance, no leftover directory. (T8)
5. A shader chosen on an instance without Iris → Iris + Sodium appear in the plan; on Forge → a warning, no Iris. (T4)

---

### Task 1: recommended.rs (core)
**Files:** Create `crates/launcher-core/src/recommended.rs` (+ `pub mod recommended;`); Test `crates/launcher-core/tests/recommended.rs`.
**Interfaces:** Consumes `mods::{ModrinthClient, Target, plan_install, InstallPlan}`, `loader::LoaderKind`. Produces:
- `pub struct Recommended { pub slug: &'static str, pub title: &'static str, pub why: &'static str, pub default_on: bool }` (`Serialize`; `slug` etc. serialise as strings).
- `pub fn recommended_for(kind: LoaderKind) -> &'static [Recommended]` — Fabric/Quilt: `fabric-api` (default_on), `sodium`, `lithium`, `modmenu`, `iris`; NeoForge: `sodium`, `lithium`, `iris`; Forge: empty.
- `pub async fn available(client: &ModrinthClient, kind: LoaderKind, mc: &str) -> Vec<Recommended>` — keeps entries whose `project_versions` for `target_for(mc, kind)` is non-empty; a failing request drops that entry.
- `pub async fn plan_many(client: &ModrinthClient, t: &Target, slugs: &[String], installed: &HashSet<String>) -> Result<InstallPlan>` — plans each slug in order with `installed ∪ already planned project ids`, skipping a slug whose project is already installed/planned (no error), merging files (user_selected only for the chosen slugs), warnings and `total_bytes`.
- [ ] Failing tests (wiremock, same helpers style as `mods_plan.rs`): `kinds_have_their_lists` (Forge empty, Fabric first is fabric-api with default_on, only it is default_on), `available_drops_entries_without_a_compatible_version`, `plan_many_installs_a_shared_dependency_once` (iris→sodium and sodium chosen: files ids [iris, sodium] once each), `plan_many_skips_already_installed`, `plan_many_keeps_going_when_one_choice_is_unavailable` (error for one slug is returned as Err naming it — decide: unavailable ⇒ Err, so the UI only offers `available` ones).
- [ ] Implement; run `cargo test -p launcher-core --test recommended` → PASS.

### Task 2: Tauri + UI for recommended mods
**Files:** Modify `src-tauri/src/lib.rs`, `src/lib/api.ts` (types + mock), `src/lib/store.ts`, `src/pages/Download.tsx`.
- Commands: `recommended_mods(kind: LoaderKind, mc_version: String) -> Vec<RecommendedDto>` (`slug,title,why,default_on`); `install_extras(instance_id: String, slugs: Vec<String>, on_event: Channel<LaunchEvent>) -> ()` (`mod_context`, `require_stopped`, busy guard, `plan_many` then `install_plan`; an empty list is a no-op).
- Store: `installInstance(id, extras?: string[])` runs the game install, then `api.installExtras` inside the same visible job (stage "下载模组"), toast mentions the count.
- Dialog: when a loader with recommendations is chosen, a "推荐模组" block with checkboxes (default_on ticked), each with its `why`; selection reset on loader change; "创建并下载" passes the ticked slugs; "仅创建" ignores them (note under the block: 仅在"创建并下载"时安装).
- [ ] Unit tests in `src-tauri` for a pure helper `fn extras_target(inst: &Instance) -> Result<(LoaderRef, Target)>` (vanilla → error). [ ] `cargo test --workspace`, `npx tsc -b`, `npm run build`; browser preview: Fabric choice shows Fabric API ticked + 4 unticked, Forge shows none, create → install → mods listed on the mods page.

### Task 3: ContentKind and destination-aware install (core)
**Files:** Modify `mods.rs`, `config.rs` (`Dirs::content_dir(id, kind)`, `Dirs::content_record(id, kind)`), `src-tauri/src/lib.rs` (tests/validate_plan literals), tests `mods_install.rs`, `mods_plan.rs`, `mods_client.rs` (add `dest`/use new fields).
**Interfaces:** `pub enum ContentKind { Mod, Shader, ResourcePack }` (serde lowercase, `Copy`) with `dir_name()`, `extension()` (".jar"/".zip"), `project_type()`; `PlanFile.dest: ContentKind` (`serde(default)` = Mod); `Target.content: ContentKind` (`target_for` sets Mod; new `pub fn content_target(mc: &str, loader: LoaderKind, kind: ContentKind) -> Target`: Mod → `target_for`; Shader → loaders `["iris"]`; ResourcePack → `["minecraft"]`); `search` facets: Mod unchanged; others `[["project_type:<t>"],["versions:<mc>"]]`; `list_content(dirs, id, kind) -> Vec<InstalledMod>`, `remove_content(dirs, id, kind, filename)`; `install_plan` routes each file to `content_dir(dest)` and validates the filename with that kind's extension; records per kind; `installed_project_ids` unchanged (mods).
- [ ] Failing tests: `shader_and_pack_files_go_to_their_folders_with_zip_names`, `a_jar_name_is_rejected_for_a_shader_and_a_zip_for_a_mod`, `content_records_are_kept_per_kind`, `list_and_remove_content_for_packs`, `search_uses_project_type_facets_for_shaders` (facets exactly `[["project_type:shader"],["versions:1.21.1"]]`), `content_targets_use_the_right_loaders`. Existing tests keep passing. [ ] Implement; `cargo test --workspace` green.

### Task 4: plan_content (shaders need Iris)
**Files:** Modify `mods.rs`; Test `crates/launcher-core/tests/content_plan.rs`.
**Interfaces:** `pub async fn plan_content(client: &ModrinthClient, loader: LoaderKind, mc: &str, kind: ContentKind, project: &str, installed_mods: &HashSet<String>, installed_content: &HashSet<String>) -> Result<InstallPlan>` — Mod ⇒ `plan_install` with `content_target`; Shader/ResourcePack ⇒ newest release version of the project for `content_target`, primary file, `dest = kind`, error "没有适配 …" when none, "已安装" when in `installed_content`; for Shader: loader ∈ {Fabric, Quilt, NeoForge} and Iris not in `installed_mods` ⇒ append `plan_install(client, mod target, "iris", installed_mods)` files with `required_by = Some(shader title)`; Forge ⇒ warning `Forge 上的光影需要 Oculus 或 OptiFine，启动器暂不自动安装`.
- [ ] Failing tests (Review Focus 5): `shader_on_fabric_without_iris_adds_iris_and_sodium`, `shader_with_iris_installed_adds_nothing_else` (installed_mods contains Iris' project id from the mock), `shader_on_forge_warns_and_adds_no_mods`, `resource_pack_has_no_prerequisites`, `no_compatible_shader_version_is_a_clear_error`, `already_installed_pack_is_refused`. [ ] Implement; PASS.

### Task 5: Tauri + UI: 内容页
**Files:** Modify `src-tauri/src/lib.rs`, `api.ts`, `store.ts`, `Sidebar.tsx` (label 内容), `pages/Mods.tsx` (type switch).
- Commands take `kind: ContentKind`: `content_search(instance_id, kind, query, offset)`, `content_plan(instance_id, kind, project_id)`, `content_list(instance_id, kind)`, `content_remove(instance_id, kind, filename)`; mods keep `mods_set_enabled`; `mods_install` accepts plans with any `dest` (validate: https cdn only, filename extension per `dest`). Old `mods_*` search/plan/list/remove stay as thin wrappers for Mod or are replaced by the generic ones (frontend switches to the generic ones).
- UI: a 模组 / 光影 / 资源包 segmented control in the page header; installed tab hides the enable switch for packs; shader/resource pack requirements: Vanilla/Forge instances can still browse resource packs (loader not required for resource packs: `require_loader` only for Mod and Shader), the empty state text adapts.
- [ ] `cargo test --workspace`, tsc, build; browser preview with mock data (shader plan shows Iris + Sodium; resource pack plan single file).

### Task 6: modpack index parsing (core)
**Files:** Create `crates/launcher-core/src/modpack.rs` (+ `pub mod modpack;`), fixture `tests/fixtures/fabulously-optimized-6.5.0.modrinth.index.json` (extracted from the real `.mrpack`); Test `crates/launcher-core/tests/modpack_index.rs`.
**Interfaces:** `pub struct MrFile { pub path: String, pub sha1: String, pub size: u64, pub downloads: Vec<String>, pub client_required: bool }`; `pub struct MrIndex { pub name: String, pub version_id: String, pub dependencies: BTreeMap<String,String>, pub files: Vec<MrFile> }`; `pub fn parse_index(bytes: &[u8]) -> Result<MrIndex>` (checks `formatVersion == 1`, `game == "minecraft"`, every `path` via `safe_join`, every download url https with a whitelisted host, sha1 present; `env.client == "unsupported"` ⇒ `client_required = false`); `pub struct PackTarget { pub mc: String, pub loader: Option<(LoaderKind, String)> }`; `pub fn pack_target(idx: &MrIndex) -> Result<PackTarget>` (`fabric-loader`→Fabric, `quilt-loader`→Quilt, `forge`→Forge, `neoforge`→NeoForge; missing `minecraft` ⇒ Err).
- [ ] Failing tests: parse the real fixture (50 files, deps fabric-loader 0.19.3 + minecraft 1.21.1, loader Fabric), `rejects_path_traversal`, `rejects_other_hosts_and_http`, `missing_sha1_is_rejected`, `unsupported_client_files_are_flagged`, `unknown_format_version_is_rejected`, `pack_target_maps_all_four_loaders`. [ ] Implement; PASS.

### Task 7: modpack search, versions, file install (core)
**Files:** Modify `modpack.rs`, `mods.rs` (`ModrinthClient::search_modpacks`, `pack_versions`); Test `tests/modpack_install.rs`.
**Interfaces:** `ModrinthClient::search_modpacks(&self, query: &str, offset: u32) -> Result<SearchResult>` (facets `[["project_type:modpack"]]`); `pub struct PackVersion { pub id: String, pub name: String, pub version_number: String, pub game_versions: Vec<String>, pub loaders: Vec<String>, pub version_type: String, pub file: VersionFile }` and `ModrinthClient::pack_versions(&self, project: &str) -> Result<Vec<PackVersion>>` (primary `.mrpack` file; versions without one are dropped); `pub async fn install_pack_files(dl: &Downloader, dirs: &Dirs, instance_id: &str, mrpack: &Path, idx: &MrIndex, progress: Option<ProgressFn>) -> Result<()>` — downloads `client_required` files to `instance dir / path` (sha1 + size), then extracts `overrides/` and `client-overrides/` entries (names via `safe_join`, directories skipped, existing files overwritten).
- [ ] Failing tests (synthetic `.mrpack` zips built in the test, wiremock for files and the search/versions endpoints): `search_modpacks_uses_modpack_facets`, `pack_versions_keep_only_mrpack_primary_files`, `installs_files_and_overrides_into_the_instance`, `client_overrides_win_over_overrides`, `unsupported_client_files_are_not_downloaded`, `zip_slip_override_entry_is_rejected_and_nothing_escapes` (Review Focus 3), `bad_sha1_fails_and_names_the_file`, `cancel_stops_the_install`. [ ] Implement; PASS.

### Task 8: Tauri + UI: 整合包
**Files:** Modify `src-tauri/src/lib.rs`, `api.ts`, `store.ts`, `pages/Download.tsx`.
- Commands: `modpack_search(query, offset) -> SearchResult`, `modpack_versions(project_id) -> Vec<PackVersion>`, `install_modpack(name: String, version: PackVersion, on_event) -> Snapshot`: validate `version.file.url` (https cdn.modrinth.com) → busy guard → download the `.mrpack` to `shared/modpacks/<sha1>.mrpack` (sha1) → read + `parse_index` + `pack_target` → create the instance (`add_with_loader`) → `ensure_instance_version` (Java for Forge/NeoForge as before) → `install_pack_files`; any failure after the instance was created removes the instance record and its directory (Review Focus 4) and returns the error; success selects the instance and returns the snapshot.
- Pure helper `fn pack_instance_name(requested: &str, pack_name: &str) -> String` (trimmed, ≤ 40 chars, falls back to the pack name) with a unit test.
- UI: Download page gets a 游戏版本 / 整合包 switch; the 整合包 view has a search box (debounced), result cards (icon, title, author, downloads, description) and a 安装 button → dialog: name (default pack title), version select (newest first, label `{version_number} · {game_versions[0]} · {loaders}`), note "将新建一个实例，包含该整合包的全部模组和配置", buttons 取消 / 创建并安装; progress uses the existing run card; mock API simulates the flow.
- [ ] tests, tsc, build, browser preview.

### Task 9: Verification
- [ ] `cargo test --workspace`, tsc, build all green; clippy shows no new lints.
- [ ] Real: (1) `install_extras` through the real Tauri window via the WebView2 debug port: create a Fabric 1.21.1 instance with Fabric API + Sodium + Iris, check files; (2) install a real shader pack into it, launch with the CLI/`launch_instance` and check the log for Iris; (3) install Fabulously Optimized from Modrinth through `install_modpack`, launch it and check the log; clean up test instances afterwards; close the debug port and restart the dev app without it.
