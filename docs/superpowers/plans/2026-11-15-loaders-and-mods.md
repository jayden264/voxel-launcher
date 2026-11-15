# Fabric/Quilt 加载器与 Modrinth 模组 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 创建实例时可选 Fabric / Quilt 及其版本；在启动器里搜索、安装、管理 Modrinth 模组。

**Architecture:** 加载器 = 官方元数据接口返回的一份版本 JSON，存入共享 `versions/<profile-id>/`，实例的 `version_id` 指向它，复用现有 `inheritsFrom` 合并。模组逻辑放在 `launcher-core` 的新模块 `mods.rs`（Modrinth 客户端 + 依赖规划 + 安装器），Tauri 只做薄命令封装，前端新增“模组”页。

**Tech Stack:** Rust（`launcher-core`、reqwest、wiremock 测试）、Tauri 2、React + TS + Tailwind v4 + shadcn/ui、zustand、motion。

**Spec:** `docs/superpowers/specs/2026-11-15-loaders-and-mods-design.md`

## Global Constraints
- 仅 Windows x64；不引入新依赖（reqwest/serde/wiremock/tempfile 已有）。
- 所有文件下载走现有 `Downloader`（`.part`、重试、校验、取消、进度）；落盘路径一律经 `paths::safe_join`。
- Modrinth API 请求的 User-Agent 固定为 `voxel-launcher/0.1`，不得含任何个人信息；限流约 300 次/分钟，遇 429 按 `Retry-After` 等待。
- 模组只校验 sha1；只允许 `.jar`，且只写入 `instances/<id>/mods/`。
- Quilt 实例搜索/依赖解析时 loaders 同时用 `["quilt","fabric"]`，Fabric 实例用 `["fabric"]`。
- 任何模组在用户确认“将安装这些文件”之前不得下载。
- 禁用 = 把 `x.jar` 重命名为 `x.jar.disabled`，不删文件。
- `Instance` 新字段必须 `serde(default)`，旧 `instances.json` 照常加载。
- **不执行 git commit**（除非用户明确要求），所以本计划没有提交步骤。
- 偏离设计文档处：不做 `list_game_versions`；界面用 `list_loader_versions(kind, mc)` 返回空列表表示“该加载器不支持此游戏版本”（已实测 Fabric 对 1.12.2 返回 `[]`）。

## Review Focus
1. 模组在当前游戏版本 + 加载器下没有任何可用版本 → 明确报“没有适配 {mc} + {loader} 的版本”，不是空白或 panic。（Task 5）
2. 依赖成环（A 需要 B，B 需要 A）→ 规划必须终止，每个项目只出现一次。（Task 5）
3. Modrinth 返回的文件名含路径成分或非 `.jar`（如 `../evil.jar`、`a/b.jar`、`x.exe`）→ 拒绝，不写盘。（Task 6）
4. 选了不支持的组合（如 Fabric + 1.12.2）→ 版本列表为空，创建/安装时给出清晰错误，不留下半成品实例的 profile。（Task 3、10）
5. 安装中途网络失败或被取消 → 已下完的文件保留并记录，失败的文件不留 `.part`、不进 `mods.json`；429 会等待后重试。（Task 4、6）

---

## 文件结构
- `crates/launcher-core/src/loader.rs`（新）：加载器种类、profile id、版本列表、安装。
- `crates/launcher-core/src/mods.rs`（新）：Modrinth 客户端、依赖规划、安装执行、已装模组管理。
- 修改：`version.rs`（Library 内联 sha1/size）、`config.rs`（Instance 字段）、`lib.rs`（导出模块）、`bin/mcl.rs`（CLI）。
- 修改：`src-tauri/src/lib.rs`（命令）、`src-tauri/tauri.conf.json`（CSP 加 `https://cdn.modrinth.com` 到 img-src）。
- 修改：`src/lib/api.ts`、`src/lib/store.ts`、`src/components/Sidebar.tsx`、`src/App.tsx`、`src/pages/Download.tsx`、`src/pages/Instances.tsx`；新增 `src/pages/Mods.tsx`。
- 测试夹具：`crates/launcher-core/tests/fixtures/`。

---

### Task 1: 库的内联 sha1/size 与真实 profile 夹具

**Files:**
- Create: `crates/launcher-core/tests/fixtures/fabric-loader-0.16.9-1.21.1.json`、`quilt-loader-0.27.1-1.21.1.json`
- Modify: `crates/launcher-core/src/version.rs`（`Library` 结构与 `resolve_library` 的坐标库分支，约 175 与 400-417 行）

**Interfaces:**
- Produces: `Library { …, sha1: Option<String>, size: Option<u64> }`（`serde(default)`）；坐标库分支用它们填充 `LibFile.sha1/size`，且 `sidecar = lib.sha1.is_none()`。

- [ ] **Step 1: 抓取夹具**

```bash
cd crates/launcher-core/tests && mkdir -p fixtures
curl -s "https://meta.fabricmc.net/v2/versions/loader/1.21.1/0.16.9/profile/json" -o fixtures/fabric-loader-0.16.9-1.21.1.json
curl -s "https://meta.quiltmc.org/v3/versions/loader/1.21.1/0.27.1/profile/json" -o fixtures/quilt-loader-0.27.1-1.21.1.json
```
Expected: 两个文件各自是合法 JSON，`id` 分别为 `fabric-loader-0.16.9-1.21.1`、`quilt-loader-0.27.1-1.21.1`，`inheritsFrom` 为 `1.21.1`。

- [ ] **Step 2: 写失败测试**（`version.rs` 的 `#[cfg(test)]`，用 `include_str!` 读夹具）
  - `fabric_profile_libraries_carry_inline_sha1`：解析 Fabric 夹具，`resolve_libraries` 后断言每个 `LibFile` 的 `sha1.is_some()`、`sidecar == false`，且 `org.ow2.asm:asm:9.7.1` 的 sha1 为 `f0ed132a49244b042cd0e15702ab9f2ce3cc8436`、size 为 `126093`。
  - `library_without_sha1_still_uses_sidecar`：构造不带 sha1 的坐标库，断言 `sidecar == true`（保护现有行为，Quilt 夹具里的库也走这条）。
  - `fabric_jvm_arg_is_one_token`：对 Fabric 夹具调用 `jvm_tokens`，断言结果里恰好有一项等于 `-DFabricMcEmu= net.minecraft.client.main.Main `（数组形式的参数不切分）。

- [ ] **Step 3: 运行确认失败**

Run: `cargo test -p launcher-core fabric_profile_libraries_carry_inline_sha1`
Expected: FAIL（`Library` 没有 `sha1` 字段 / 断言不成立）。

- [ ] **Step 4: 实现** `Library` 增加两个可选字段；坐标库分支改用它们。若 `fabric_jvm_arg_is_one_token` 失败，修正 `jvm_tokens`，使 `arguments` 数组里的字符串不再被空白切分（只有旧格式 `minecraftArguments` 字符串才切分）。

- [ ] **Step 5: 运行确认通过**

Run: `cargo test -p launcher-core`
Expected: 全部 PASS（原有测试不退化）。

---

### Task 2: Instance 的加载器字段

**Files:**
- Modify: `crates/launcher-core/src/config.rs`（`Instance` 约 96-123 行，`InstancesFile::add` 约 144 行）

**Interfaces:**
- Consumes: `loader::LoaderKind`（Task 3 定义；本任务先在 `loader.rs` 里建好含 `LoaderKind` 与 `profile_id` 的最小文件并在 `lib.rs` 里 `pub mod loader;`，其余函数留给 Task 3）。
- Produces:
  - `pub struct LoaderRef { pub kind: LoaderKind, pub version: String }`（`Debug, Clone, PartialEq, Eq, Serialize, Deserialize`，放在 `config.rs`）。
  - `Instance { …, pub mc_version: Option<String>, pub loader: Option<LoaderRef> }`；`impl Instance { pub fn game_version(&self) -> &str }`（`mc_version` 缺省回退 `version_id`）。
  - `InstancesFile::add_with_loader(&mut self, name: &str, mc_version: &str, loader: Option<LoaderRef>) -> &Instance`：有加载器时 `version_id = loader::profile_id(kind, &version, mc_version)`，否则 `= mc_version`。原 `add` 保持不变。
  - `loader::profile_id(kind: LoaderKind, loader_version: &str, mc: &str) -> String`，返回 `fabric-loader-{loader}-{mc}` 或 `quilt-loader-{loader}-{mc}`。

- [ ] **Step 1: 写失败测试**（`config.rs` 测试模块）
  - `old_instances_json_without_loader_fields_loads`：反序列化不含新字段的旧 JSON，`loader.is_none()`，`game_version()` 等于 `version_id`。
  - `add_with_loader_sets_profile_version_id`：`add_with_loader("mods", "1.21.1", Some(LoaderRef{kind: Fabric, version: "0.16.9"}))` 后 `version_id == "fabric-loader-0.16.9-1.21.1"`、`mc_version == Some("1.21.1")`；Quilt 同理得到 `quilt-loader-…`。
  - `loader_ref_roundtrips_json`：序列化后 `kind` 为小写 `"fabric"`。

- [ ] **Step 2: 运行确认失败** — `cargo test -p launcher-core old_instances_json` → FAIL（未定义）。
- [ ] **Step 3: 实现**上述类型与方法；`Instance::default()` 里新字段为 `None`。
- [ ] **Step 4: 运行确认通过** — `cargo test -p launcher-core` → 全部 PASS。

---

### Task 3: loader.rs — 版本列表与安装

**Files:**
- Modify: `crates/launcher-core/src/loader.rs`
- Test: `crates/launcher-core/tests/loader.rs`（wiremock，Downloader 用 `with_resolver` 把官方 meta 域名改写到 mock 服务器，沿用 `tests/download.rs` 的做法）

**Interfaces:**
- Consumes: `Downloader::fetch_json`/`fetch`、`install::install_version`、`version::{VersionJson, ResolvedVersion}`、`config::Dirs`、`config::save_atomic`/`paths::safe_join`。
- Produces（均在 `loader.rs`）：
  - `pub enum LoaderKind { Fabric, Quilt }`（`serde(rename_all = "lowercase")`，`Copy, Eq, Hash`）；`pub fn label(&self) -> &'static str`（"Fabric"/"Quilt"）。
  - `pub struct LoaderVersion { pub version: String, pub stable: bool, pub build: u32 }`（`Serialize`）。
  - `pub async fn list_loader_versions(dl: &Downloader, kind: LoaderKind, mc: &str) -> Result<Vec<LoaderVersion>>`：按 `build` 降序；Fabric 用接口里的 `stable`，Quilt 无该字段，`stable = !version.contains('-')`；空列表表示不支持。Fabric 地址 `https://meta.fabricmc.net/v2/versions/loader/{mc}`，Quilt 地址 `https://meta.quiltmc.org/v3/versions/loader/{mc}`。
  - `pub fn default_version(list: &[LoaderVersion]) -> Option<&LoaderVersion>`：`stable` 中 `build` 最大者；无稳定版则取 `build` 最大者。
  - `pub async fn install_loader(dl: &Downloader, dirs: &Dirs, kind: LoaderKind, mc: &str, loader_version: &str, progress: Option<ProgressFn>) -> Result<ResolvedVersion>`：幂等；profile 不存在时下载 `…/{mc}/{loader_version}/profile/json`，校验 `id == profile_id(..)` 且 `inheritsFrom == Some(mc)`，原子写入 `versions/<id>/<id>.json`，再调用 `install_version(profile_id)`。不支持的组合返回 `Error::Other("{label} 不支持 Minecraft {mc}")`，且不写任何文件。

- [ ] **Step 1: 写失败测试**（wiremock）
  - `lists_loader_versions_sorted_by_build_with_stable_flag`：mock 返回 3 项（含一个 `0.20.0-beta.9`，Quilt 样式），断言排序、`stable`、`default_version` 选稳定版最大 build。
  - `empty_list_means_unsupported`：mock 返回 `[]`，`list_loader_versions` 得到空 Vec；`install_loader` 返回含“不支持”的错误，且 `versions/` 下没有新目录。
  - `install_loader_writes_profile_and_is_idempotent`：mock 提供 Fabric 夹具作为 profile，并提供 1.21.1 原版 JSON 需要的资源会过于庞大，因此此测试只断言到“profile 文件已落盘且内容 id 正确”这一步：把 `install_version` 之前的部分拆成 `pub async fn fetch_profile(dl, dirs, kind, mc, loader_version) -> Result<String /*profile id*/>`，测试它；第二次调用不再发请求（mock 期望仅 1 次）。
  - `rejects_profile_with_wrong_inherits_from`：mock 返回 `inheritsFrom` 为别的版本的 JSON → Err，文件不落盘。
  - `rejects_profile_with_unsafe_id`：id 含 `..` 的 JSON → Err。

- [ ] **Step 2: 运行确认失败** — `cargo test -p launcher-core --test loader` → FAIL。
- [ ] **Step 3: 实现**上述函数；`install_loader` = `fetch_profile` + `install::install_version`。
- [ ] **Step 4: 运行确认通过** — `cargo test -p launcher-core` → PASS。
- [ ] **Step 5: 真实网络冒烟**（一次性，不进测试）：写一个 `#[ignore]` 测试 `real_fabric_versions_for_1_21_1`，`cargo test -p launcher-core real_ -- --ignored` 断言列表非空、`list_loader_versions(Fabric,"1.12.2")` 为空。

---

### Task 4: mods.rs — Modrinth 客户端

**Files:**
- Create: `crates/launcher-core/src/mods.rs`（本任务只写客户端部分）；`lib.rs` 加 `pub mod mods;`
- Test: `crates/launcher-core/tests/mods_client.rs`

**Interfaces:**
- Produces:
  - `pub struct ModrinthClient`；`ModrinthClient::new(base: impl Into<String>) -> Self`（内部自建 `reqwest::Client`，UA `voxel-launcher/0.1`，超时 20s）；`ModrinthClient::official() -> Self`（base `https://api.modrinth.com/v2`）。
  - `pub struct Target { pub mc_version: String, pub loaders: Vec<String> }`；`pub fn target_for(mc: &str, kind: LoaderKind) -> Target`（Fabric → `["fabric"]`，Quilt → `["quilt","fabric"]`）。
  - `pub struct ModHit { project_id, slug, title, description, icon_url: Option<String>, downloads: u64, author }`（`Serialize, Deserialize`）；`pub struct SearchResult { hits: Vec<ModHit>, total: u64 }`。
  - `pub async fn search(&self, t: &Target, query: &str, offset: u32) -> Result<SearchResult>`：facets 为 `project_type:mod`、`versions:{mc}`、`categories` 取 `t.loaders`（同一组内为 OR）；`limit=20`；空查询合法。
  - `pub struct VersionFile { filename, url, sha1, size: u64, primary: bool }`；`pub struct Dependency { project_id: Option<String>, version_id: Option<String>, dependency_type: String }`；`pub struct ModVersion { id, project_id, version_number, version_type, files: Vec<VersionFile>, dependencies: Vec<Dependency> }`。
  - `pub async fn project_versions(&self, project: &str, t: &Target) -> Result<Vec<ModVersion>>`（带 `game_versions`、`loaders` 过滤，Modrinth 按新到旧返回）；`pub async fn version_by_id(&self, id: &str) -> Result<ModVersion>`；`pub async fn project_title(&self, id: &str) -> Result<(String, Option<String>)>`（标题与图标，用于依赖项展示）。
  - 所有请求经内部 `get_json`：429 时读 `Retry-After`（秒，缺省 2，上限 30）等待后重试，最多 3 次；其他非 2xx 返回 `Error::Other` 含状态码。

- [ ] **Step 1: 写失败测试**（wiremock）
  - `search_sends_facets_and_user_agent`：mock 断言请求头 UA == `voxel-launcher/0.1`、query 里 `facets` 含 `versions:1.21.1` 与 `categories:fabric`；响应用抓到的 Modrinth 搜索 JSON 子集，断言 `hits[0].title`、`total`。
  - `quilt_target_searches_both_loaders`：`target_for("1.21.1", Quilt).loaders == ["quilt","fabric"]`，facets 中两者出现在同一组。
  - `project_versions_parse_files_and_dependencies`：解析含 `primary`、`hashes.sha1`、`dependencies`（含 `required`、`incompatible`）的响应。
  - `retries_after_429_using_retry_after`：第一次 429 + `Retry-After: 1`，第二次 200；断言最终成功且总耗时 ≥ 1s。
  - `gives_up_after_three_429s`：连续 429 → Err。

- [ ] **Step 2: 运行确认失败** — `cargo test -p launcher-core --test mods_client` → FAIL。
- [ ] **Step 3: 实现**；Modrinth 响应里 `hashes.sha1` 映射到 `VersionFile.sha1`（用 serde 的中间结构转换）。
- [ ] **Step 4: 运行确认通过** — 同上 → PASS。

---

### Task 5: mods.rs — 依赖规划

**Files:**
- Modify: `crates/launcher-core/src/mods.rs`
- Test: `crates/launcher-core/tests/mods_plan.rs`

**Interfaces:**
- Consumes: Task 4 的 `ModrinthClient`、`Target`、`ModVersion`。
- Produces:
  - `pub struct PlanFile { project_id, version_id, title, version_number, icon_url: Option<String>, filename, url, sha1, size: u64, required_by: Option<String> /*依赖者标题*/, user_selected: bool }`（`Serialize, Deserialize, Clone`）。
  - `pub struct InstallPlan { pub files: Vec<PlanFile>, pub total_bytes: u64, pub warnings: Vec<String> }`。
  - `pub async fn plan_install(client: &ModrinthClient, t: &Target, project_id: &str, installed: &HashSet<String> /*已装 project_id，含已禁用*/) -> Result<InstallPlan>`。
  - 规则：选版本时取 `project_versions` 里第一个 `version_type == "release"`，没有则取第一个并加警告“{标题} 只有 beta/alpha 版本”；没有任何版本 → `Err(Error::Other("没有适配 {mc} + {loaders[0]} 的版本"))`；文件取 `primary` 为真者，否则第一个；递归处理 `required` 依赖（只有 `version_id` 时先 `version_by_id` 得 project_id）；已在 `installed` 或已在计划中的项目跳过（环路因此终止）；依赖项没有适配版本 → 整体 Err 并点名该依赖；`incompatible` 且对方已安装 → 加警告；`files` 第一个是用户选择的项目（`user_selected = true`），之后是依赖。

- [ ] **Step 1: 写失败测试**（wiremock，用路径区分项目）
  - `plans_selected_mod_plus_required_dependency`：A 依赖 B（required）、C（optional）→ files 为 [A, B]，B 的 `required_by == Some(A.title)`，`total_bytes` 为二者 size 之和，C 不在其中。
  - `skips_already_installed_dependency`：`installed` 含 B → files 只有 A。
  - `terminates_on_dependency_cycle`：A→B→A → files 恰为 [A, B]，每个项目一次。
  - `errors_when_no_compatible_version`：`project_versions` 返回 `[]` → 错误信息含 `1.21.1` 与 `fabric`。
  - `errors_naming_dependency_without_version`：A 依赖的 B 没有适配版本 → 错误含 B 的标题。
  - `prefers_release_over_beta_and_warns_when_only_beta`。
  - `resolves_dependency_given_only_version_id`。
  - `warns_on_incompatible_installed_mod`。

- [ ] **Step 2: 运行确认失败** — `cargo test -p launcher-core --test mods_plan` → FAIL。
- [ ] **Step 3: 实现**（迭代用栈或递归皆可，用 `HashSet<String>` 记 visited）。
- [ ] **Step 4: 运行确认通过** → PASS。

---

### Task 6: mods.rs — 安装、列表、启用/禁用、删除

**Files:**
- Modify: `crates/launcher-core/src/mods.rs`；`config.rs`（`Dirs` 增加 `pub fn mods_dir(&self, instance_id: &str) -> Result<PathBuf>` = `instances/<id>/mods`，经 `safe_join`；`pub fn mods_record(&self, instance_id) -> Result<PathBuf>` = `instances/<id>/.voxel/mods.json`）
- Test: `crates/launcher-core/tests/mods_install.rs`

**Interfaces:**
- Consumes: Task 5 的 `InstallPlan/PlanFile`；`Downloader::{download_all, with_cancel}`、`DownloadTask::rel`。
- Produces:
  - `pub struct InstalledMod { pub filename: String /*不含 .disabled*/, pub enabled: bool, pub size: u64, pub project_id: Option<String>, pub title: Option<String>, pub version_number: Option<String>, pub icon_url: Option<String>, pub user_installed: bool }`（`Serialize`）。
  - 记录文件 `mods.json`：`{ "version": 1, "mods": { "<filename>": { project_id, version_id, version_number, title, icon_url, sha1, user_installed } } }`；损坏则视为空（不 panic），写入用 `save_atomic`。
  - `pub async fn install_plan(dl: &Downloader, dirs: &Dirs, instance_id: &str, plan: &InstallPlan, progress: Option<ProgressFn>) -> Result<()>`：逐文件校验 `filename`（不含 `/`、`\`、`:`、`..`，且以 `.jar` 结尾，大小写不敏感）→ 否则 `Err` 且**不写任何文件**；通过 `DownloadTask::rel(&mods_dir, filename, url).sha1(Some(sha1)).size(Some(size))` 下载；成功的文件写入记录；若记录里已有同一 `project_id` 但文件名不同，下载成功后删除旧文件并替换记录；部分失败时已成功的保留并记录，返回首个错误。
  - `pub fn list_installed(dirs: &Dirs, instance_id: &str) -> Result<Vec<InstalledMod>>`：扫描 `mods/`，识别 `.jar` 与 `.jar.disabled`，与记录合并；记录里存在但文件已不在的条目忽略；不在记录里的文件（用户手动放入）`project_id = None`、`user_installed = true`。
  - `pub fn installed_project_ids(dirs, instance_id) -> Result<HashSet<String>>`（含已禁用）。
  - `pub fn set_enabled(dirs, instance_id, filename: &str, enabled: bool) -> Result<()>`；`pub fn remove_mod(dirs, instance_id, filename: &str) -> Result<()>`（同时清理记录）。文件名同样经上述校验，防止越界。

- [ ] **Step 1: 写失败测试**（tempfile + wiremock）
  - `installs_files_and_records_them`：两个文件下载到 `mods/`，`mods.json` 含两条，`list_installed` 返回标题与 `user_installed`（用户选的为 true，依赖为 false）。
  - `rejects_malicious_filenames_before_writing`：对 `../evil.jar`、`a/b.jar`、`a\b.jar`、`x.exe`、`C:evil.jar`、空字符串分别构造计划 → 都 `Err`，`mods/` 目录里没有任何文件、没有 `.part`。
  - `bad_checksum_leaves_no_file_and_no_record`：sha1 不符 → Err，无 jar、无 `.part`、记录里没有该条。
  - `partial_failure_keeps_successful_files`：第一个文件成功、第二个 404 → 第一个在 `mods/` 且已记录，第二个无痕迹，函数返回 Err。
  - `reinstall_same_project_replaces_old_file`：已记录 `a-1.0.jar`（project P），安装 P 的 `a-2.0.jar` → 旧文件被删，记录指向新文件。
  - `disable_and_enable_roundtrip`：`set_enabled(false)` 后文件名为 `x.jar.disabled`，`list_installed` 里 `enabled == false` 且 `filename == "x.jar"`；再启用恢复。
  - `disabled_mod_still_counts_as_installed_project`：`installed_project_ids` 含被禁用模组的 project_id。
  - `remove_mod_deletes_file_and_record_and_rejects_traversal`。
  - `corrupt_mods_json_is_treated_as_empty`：写入乱码，`list_installed` 仍列出 jar 文件。
  - `manually_added_jar_is_listed_without_project`。

- [ ] **Step 2: 运行确认失败** — `cargo test -p launcher-core --test mods_install` → FAIL。
- [ ] **Step 3: 实现**；文件名校验写成一个 `fn check_mod_filename(name: &str) -> Result<()>` 供安装、启用、删除共用。
- [ ] **Step 4: 运行确认通过** — `cargo test -p launcher-core` → 全部 PASS；再跑 `cargo clippy -p launcher-core -- -D warnings`，Expected: 无警告。

---

### Task 7: mcl CLI 与真机启动验证

**Files:**
- Modify: `crates/launcher-core/src/bin/mcl.rs`（先读现有 `list/install/launch` 的参数风格，保持一致）

**Interfaces:**
- Produces 新子命令：`mcl loader-versions <fabric|quilt> <mc>`、`mcl install-loader <fabric|quilt> <mc> [loader_version]`（缺省用 `default_version`）、`mcl mod-search <instance> <query>`、`mcl mod-add <instance> <project_slug_or_id>`（规划 → 打印清单 → 直接安装，CLI 不需要确认框）、`mcl mods <instance>`。

- [ ] **Step 1: 实现**上述子命令，直接调用 `loader`/`mods` 公共函数。
- [ ] **Step 2: 真机验证 Fabric**（用一个临时数据目录，不污染用户数据：设置 `VOXEL_DATA_DIR` 或 `mcl` 现有的等价参数）
  1. `mcl install-loader fabric 1.21.1`
  2. 创建实例并 `mcl mod-add <instance> fabric-api`、`mcl mod-add <instance> sodium`
  3. 用 `mcl launch` 以该 profile id 启动（参数见 `mcl.rs`）。
  Expected：`logs/latest.log` 中出现 `Loading Minecraft 1.21.1 with Fabric Loader` 以及已加载模组列表含 `fabric-api` 与 `sodium`；游戏窗口出现后正常结束，无 `crash-reports/` 新文件。
- [ ] **Step 3: 真机验证 Quilt**：同样 `install-loader quilt 1.21.1`，并装一个 Fabric 模组（`fabric-api` 对应的 Quilt 方案：`quilted-fabric-api`），日志出现 `Quilt Loader`。
- [ ] **Step 4:** 记录两次启动的日志摘录与结果；任一失败则回到对应任务修复，不跳过。

---

### Task 8: Tauri 命令

**Files:**
- Modify: `src-tauri/src/lib.rs`（命令与 `generate_handler!` 列表，约 255、363、689 行；`prepare_and_spawn` 约 423 行）、`src-tauri/tauri.conf.json`

**Interfaces:**
- Consumes: Task 2-6 的 core API；现有 `acquire_busy`、`make_downloader`、`progress_fn`、`emit`、`LaunchEvent`。
- Produces（前端调用名）：
  - `list_loader_versions(kind: LoaderKind, mc_version: String) -> Vec<LoaderVersion>`
  - `create_instance(name: String, version_id: String, loader: Option<LoaderRef>) -> Snapshot`：`version_id` 在这里表示 Minecraft 版本；内部调用 `add_with_loader`；校验名称规则不变。
  - `install_instance(id: String, on_event: Channel<LaunchEvent>) -> ()`：有 `loader` 时调用 `loader::install_loader`，否则 `install::install_version(version_id)`；用 `acquire_busy` 并转发进度。
  - `mods_search(instance_id: String, query: String, offset: u32) -> SearchResult`
  - `mods_plan(instance_id: String, project_id: String) -> InstallPlan`
  - `mods_install(instance_id: String, plan: InstallPlan, on_event: Channel<LaunchEvent>) -> ()`（`acquire_busy`）
  - `mods_list(instance_id: String) -> Vec<InstalledMod>`、`mods_set_enabled(instance_id, filename, enabled) -> ()`、`mods_remove(instance_id, filename) -> ()`
  - 三个读写模组的命令在实例没有 `loader` 时返回错误“当前实例是原版，没有加载器”；`mods_*` 在游戏运行中（`refresh_running` 后 `running.is_some()`）拒绝修改并提示先关闭游戏。
- 现有 `prepare_and_spawn` 里调用 `install_version` 的位置：实例有 `loader` 时改调 `loader::install_loader`，保证“仅创建”后直接启动也能自动补装。
- `tauri.conf.json` 的 CSP `img-src` 增加 `https://cdn.modrinth.com`（正式与 dev CSP 都改）。

- [ ] **Step 1:** 实现命令并加入 `generate_handler!`。
- [ ] **Step 2:** `cargo check -p voxel-launcher` → Expected: 通过无警告；`cargo test --workspace` → 全部 PASS。
- [ ] **Step 3:** 在 `src-tauri` 内为“实例没有加载器时 mods_* 报错”与“运行中拒绝修改”写两个单元测试（把判断抽成纯函数 `fn require_loader(inst: &Instance) -> Result<LoaderRef, String>` 与 `fn require_stopped(inst: &Instance) -> Result<(), String>`），先红后绿。

---

### Task 9: 前端 API、类型与 store

**Files:**
- Modify: `src/lib/api.ts`（类型约 22-65 行，`real` 约 94-115 行，`fakeApi` 浏览器模拟）、`src/lib/store.ts`

**Interfaces:**
- Produces（`api.ts`）：
  - `type LoaderKind = 'fabric' | 'quilt'`；`interface LoaderRef { kind: LoaderKind; version: string }`；`Instance` 增加 `mc_version: string | null`、`loader: LoaderRef | null`。
  - `interface LoaderVersion { version: string; stable: boolean; build: number }`。
  - `interface ModHit { project_id; slug; title; description; icon_url: string | null; downloads: number; author }`；`interface SearchResult { hits: ModHit[]; total: number }`。
  - `interface PlanFile { project_id; version_id; title; version_number; icon_url: string | null; filename; url; sha1; size: number; required_by: string | null; user_selected: boolean }`；`interface InstallPlan { files: PlanFile[]; total_bytes: number; warnings: string[] }`。
  - `interface InstalledMod { filename; enabled: boolean; size: number; project_id: string | null; title: string | null; version_number: string | null; icon_url: string | null; user_installed: boolean }`。
  - `api.listLoaderVersions(kind, mc)`、`api.createInstance(name, mcVersion, loader?: LoaderRef | null)`、`api.installInstance(id, on)`（Channel）、`api.modsSearch(instanceId, query, offset)`、`api.modsPlan(instanceId, projectId)`、`api.modsInstall(instanceId, plan, on)`（Channel）、`api.modsList(instanceId)`、`api.modsSetEnabled(instanceId, filename, enabled)`、`api.modsRemove(instanceId, filename)`。
  - `fakeApi` 为每个新方法提供模拟实现（Fabric 版本列表含一个不稳定版；`1.12.2` 返回空列表；模组搜索返回 4-5 条带依赖关系的假数据；安装模拟进度），使浏览器预览能走完整流程。
- Produces（`store.ts`）：`Page` 增加 `'mods'`；`installInstance(id: string): Promise<boolean>`（与现有 `install` 同样驱动 `run` 状态与 toast）；`modsInstall(instanceId: string, plan: InstallPlan): Promise<boolean>`（同样驱动 `run`，`kind` 为 `'install'`）。

- [ ] **Step 1:** 先改类型与 `fakeApi`，运行 `npx tsc -b` 看到新类型缺失导致的错误（预期红）。
- [ ] **Step 2:** 补全 `real` 与 store；`npx tsc -b` → Expected: 0 错误。

---

### Task 10: 创建实例对话框选择加载器

**Files:**
- Modify: `src/pages/Download.tsx`（`InstallDialog` 约 16-59 行）、`src/pages/Instances.tsx`（实例卡片，约 127-130 行）

**Interfaces:**
- Consumes: Task 9 的 `api.listLoaderVersions`、`api.createInstance`、`store.installInstance`。

行为（逐条可验证）：
- 对话框在实例名称下方增加“加载器”分段选择（复用 `kit.tsx` 的 `Segmented`）：原版 / Fabric / Quilt。
- 打开对话框时并行请求两个加载器对该 Minecraft 版本的版本列表；返回空列表的选项置灰并在旁边显示“不支持 {版本}”；请求进行中显示加载态；请求失败显示错误并允许只选“原版”。
- 选了加载器后出现版本下拉：默认选 `stable` 中 `build` 最大者，非稳定版放在折叠的“显示测试版”后面。
- “创建并下载”：`createInstance(name, mc, loader)` 成功后对返回快照里被选中的实例调用 `installInstance(id)`；“仅创建”只创建。实例名称默认值带上加载器，如 `Fabric 1.21.1`（用户未改名时随选择更新）。
- 实例卡片的版本行显示加载器徽章（例如 `Fabric 0.16.9`），原版实例不显示。

- [ ] **Step 1:** 实现上述界面与逻辑。
- [ ] **Step 2:** `npx tsc -b && npm run build` → Expected: 通过。
- [ ] **Step 3:** 浏览器预览（`mcp__Claude_Browser__*`，在 `http://localhost:1420` 用模拟数据）：打开 1.21.1 的创建对话框，截图确认三个选项与版本下拉；打开 1.12.2 的对话框，确认 Fabric/Quilt 置灰且文案正确；选 Fabric 创建并下载，确认实例卡片出现 `Fabric …` 徽章。

---

### Task 11: “模组”页

**Files:**
- Create: `src/pages/Mods.tsx`
- Modify: `src/components/Sidebar.tsx`（`ITEMS` 增加第 6 项，图标用 lucide 的 `Puzzle`，label“模组”）、`src/App.tsx`（`PAGES` 增加 `mods: ModsPage`）、`src/components/kit.tsx`（如需新的小组件）

**Interfaces:**
- Consumes: Task 9 的 api/store；`PageBanner`（biome 用 `nether`、seed `mods`、eyebrow `Mods`）、`Bezel`、`PillButton`、`Empty`、`RunProgress`、`PixelSprite`。

行为：
- 作用于当前选中的实例。没有选中实例，或实例没有 `loader` → 整页空状态：“当前实例是原版，先创建 Fabric 或 Quilt 实例”，按钮跳转到“下载”页。
- 顶部两个标签（`Segmented`）：已安装 / 浏览。
- 已安装：列表项显示图标（无则像素占位）、标题（无记录时显示文件名）、版本、大小；启用开关（调用 `modsSetEnabled`）；删除按钮（确认对话框，文案说明只删除该文件）。空状态用 `chest` 精灵图。依赖项（`user_installed == false`）带“依赖”小标签。
- 浏览：搜索框（输入停止 400ms 后搜索，空查询显示热门）、结果卡片（图标、标题、作者、下载量、简介截断、“安装”按钮）、“加载更多”按钮（offset += 20）。已安装的项目按钮显示“已安装”并禁用。
- 点击“安装” → `modsPlan` → 弹出确认对话框：列出每个文件（标题、版本、大小；依赖项标注“{依赖者} 需要”）、总大小、警告（黄色）；规划失败时对话框直接显示错误文案（含“没有适配 …”）。确认后调用 `store.modsInstall`，对话框关闭，页面顶部显示 `RunProgress` 与“取消”；完成后刷新已安装列表并 toast。
- 游戏运行中：安装/启用/删除按钮禁用并提示“先关闭游戏”。

- [ ] **Step 1:** 实现页面、侧栏项与路由。
- [ ] **Step 2:** `npx tsc -b && npm run build` → Expected: 通过。
- [ ] **Step 3:** 浏览器预览逐项验证：无加载器实例时的空状态；有 Fabric 实例时搜索→安装确认框（含依赖与总大小）→进度→已安装列表出现；启用开关与删除确认；窗口宽度 940 时布局不溢出。截图留证。

---

### Task 12: 全量验证

- [ ] **Step 1:** `cargo test --workspace`、`cargo clippy --workspace -- -D warnings`、`npx tsc -b`、`npm run build` → Expected: 全部通过。
- [ ] **Step 2:** `npx tauri dev` 打开真实窗口（若后台已有 `tauri dev` 先复用，不重复启动）：创建 Fabric 1.21.1 实例并下载 → 在“模组”页搜索并安装 Sodium（确认框里出现 Fabric API 依赖）→ 启动游戏 → 读 `latest.log` 确认加载器与模组均已加载。
- [ ] **Step 3:** 用一个 Quilt 实例重复 Step 2 的关键路径（至少创建 + 安装 `quilted-fabric-api` + 启动）。
- [ ] **Step 4:** 向用户汇报：通过项、未覆盖项（如其他游戏版本、干净系统、Fabric/Quilt 之外的加载器），以及 Review Focus 五项各自对应的测试名。
