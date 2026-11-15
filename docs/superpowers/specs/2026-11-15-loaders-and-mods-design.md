# 加载器（Fabric / Quilt）与模组下载 — 第一批设计

日期：2026-11-15　状态：待你审阅

## 意图与原因
- 启动器要给全世界的玩家用，走离线路线（用户已定）。离线玩家里玩模组的比例很高，所以“选加载器 + 装模组”是最能留住人的功能。
- 用户的原话：下载时可以选 Fabric / Forge 等“包括但不限于”的加载器，并且能同时下载模组；分两批做，**先做第一批**。
- 第一批 = **Fabric、Quilt + Modrinth 模组搜索与安装**。第二批 = Forge / NeoForge（另起设计）。

## 目标
1. 创建实例时可选加载器（原版 / Fabric / Quilt）和加载器版本，一步下完。
2. 在启动器里搜索 Modrinth 上的模组，一键安装到当前实例，必需依赖自动补齐。
3. 管理已装模组：列表、启用/禁用、删除。
4. 加载器的结构可扩展：第二批加 Forge/NeoForge 时，不改模组那一套。

## 非目标（第一批不做）
- Forge / NeoForge（第二批）；CurseForge（要申请密钥、分发条款限制）。
- 整合包（.mrpack）导入、资源包 / 光影下载、模组更新检查与一键更新（都是自然的下一步，但不在这一批）。
- 皮肤、正版登录。

## 反目标（做成了也算失败）
- 任何模组被静默安装：用户点安装之前，必须看到“将安装哪些文件（含依赖）”。
- 写到实例 `mods` 目录之外，或接受来自远程的恶意文件名。
- 因为换了加载器，把用户原有实例/存档弄坏。

## 约束
- 沿用现有架构：Rust 内核 `launcher-core`（纯库）+ Tauri 薄封装 + React 前端；Windows x64。
- 下载全部走现有 `Downloader`（`.part`、重试、校验、取消、进度、`safe_join`）。
- 已实测可用的接口（2026-11-15）：
  - Fabric：`meta.fabricmc.net/v2/versions/loader/{mc}`（列表）与 `.../{mc}/{loader}/profile/json`（直接是可用的版本 JSON，带 `inheritsFrom`，库自带 `sha1`、`size`）。BMCLAPI 已有 `fabric-meta` / `maven` 镜像。
  - Quilt：`meta.quiltmc.org/v3/versions/loader/{mc}` 与对应 `profile/json`（返回 200）；Quilt 的 maven 无镜像，直接走官方。
  - Modrinth：`api.modrinth.com/v2/search`（facets 过滤版本/加载器）、`/project/{id}/version`（`game_versions`、`loaders` 过滤）；文件有 `sha1`/`sha512`/`size`/`primary`；依赖带 `dependency_type`。**必须带 User-Agent**；下载在 `cdn.modrinth.com`，无镜像。

## 已考虑的做法
- **A（选定）：把加载器当成“会生成一份版本 JSON 的东西”。** Fabric/Quilt 官方接口直接给版本 JSON，存进共享的 `versions/<profile-id>/`，实例的 `version_id` 指向它；现有的 `inheritsFrom` 合并、库去重、无 sha1 库的下载都已经能用。优点：改动最小，第二批的 Forge 装完也会产出同样的东西。缺点：需要给 `Instance` 多记一份“加载器信息”。
- B：为每个加载器写独立的启动逻辑。重复、容易分叉，第二批会更难接，放弃。
- C：直接用第三方整合包格式当唯一入口。绕开了“选加载器”，不符合用户要求，放弃。

## 设计

### 内核（`launcher-core`）
- 新模块 `loader.rs`：
  - `enum LoaderKind { Fabric, Quilt }`（第二批加 `Forge`、`NeoForge`）。
  - `list_game_versions(kind)`：该加载器支持哪些游戏版本，用来在界面里禁用不支持的组合（如 1.14 之前没有 Fabric）。
  - `list_loader_versions(kind, mc)`：返回版本列表并标出 `stable`，默认选最新稳定版。
  - `install_loader(kind, mc, loader_ver) -> profile_id`：下载 profile JSON，校验 `inheritsFrom == mc` 且 id 合法后写入 `versions/<id>/<id>.json`，再调用现有 `install_version(profile_id)` 补全原版与库。同一 mc + 加载器版本只生成一份，多个实例共享。
- `Library` 增加可选的内联 `sha1`、`size` 字段（Fabric 的库自带），下载器优先用它做校验。
- `Instance` 增加 `mc_version`（原版号）和 `loader: Option<{kind, version}>`，用 `serde(default)`，旧配置自动兼容（缺失的 `mc_version` 回退为 `version_id`）。
- 数组形式的 JVM 参数每项当一个整体，不再切分（Fabric profile 里有 `-DFabricMcEmu= net.minecraft.client.main.Main ` 这种带空格的值，要原样传）。
- 新模块 `mods.rs`（Modrinth 客户端）：
  - `search(query, mc, loader, offset)`、`versions(project, mc, loaders)`。
  - **Quilt 实例同时搜 `quilt` 和 `fabric`**（Quilt 能加载 Fabric 模组）。
  - `plan_install(project, mc, loader) -> InstallPlan`：选该项目对应版本里的最新正式版，递归解析 `required` 依赖（已装的跳过，`incompatible` 给警告），返回要下载的文件清单（名称、大小、来源）。**不下载**，由界面确认后再执行。
  - `install(plan)`：文件名过 `safe_join`，下载到 `instances/<id>/mods/`，用 sha1 校验；成功后记录到 `instances/<id>/.voxel/mods.json`（文件名 → project_id、version_id、sha1、是否用户主动安装）。
  - `list_installed()`：扫描 `mods/`，结合记录补全名称/图标；识别 `.jar` 与 `.jar.disabled`。
  - `set_enabled(file, bool)`：重命名为 `.disabled` 后缀，不删除。
  - `remove(file)`：只删除该文件；被别的模组依赖时给提示但不阻止。
- Modrinth 限流（约 300 次/分钟）：请求串行加小缓存，遇 429 按 `Retry-After` 等待。User-Agent 使用通用名 `voxel-launcher/0.1`，不带任何个人信息。

### Tauri 命令
`list_loader_versions`、`create_instance`（新增 loader 参数）、`install_version`（沿用）、`mods_search`、`mods_plan`、`mods_install`（带 Channel 进度）、`mods_list`、`mods_set_enabled`、`mods_remove`。前端只通过这些命令访问。

### 界面
- **下载页的创建实例对话框**：新增“加载器”分段选择（原版 / Fabric / Quilt）；选了之后出现加载器版本下拉（默认最新稳定版）；该游戏版本不支持时对应项置灰并说明原因。Forge、NeoForge 先不显示，第二批再出现。
- **新增“模组”页**（侧栏第 6 项）：作用于当前实例；上方“已安装 / 浏览”两个标签。
  - 已安装：列表 + 启用开关 + 删除；空状态用现有像素插画。
  - 浏览：搜索框 + 结果卡片（图标、名称、下载量、简介）；点安装 → 弹出“将安装这些文件”确认框（列出模组与依赖及总大小）→ 进度条 → 完成提示。
  - 没有选中带加载器的实例时，提示“当前实例是原版，先创建 Fabric/Quilt 实例”，并给出跳转按钮。
- 实例卡片显示加载器徽章（如 Fabric 0.20.1）。
- 视觉沿用现有的像素风与双层边框，不引入新风格。

## 验证
- 单测/集成（wiremock）：profile JSON 合并（用 2026-11-15 抓到的真实 Fabric/Quilt 样本做 fixture）、Library 内联 sha1、依赖解析（含循环、已装跳过、`incompatible`）、恶意文件名被拒、`.disabled` 重命名、429 重试。
- 真机：在本机创建 Fabric 1.21.1 实例 + Fabric API + Sodium，启动，日志里确认加载器和模组都已加载；再做一个 Quilt 实例验证。
- 未覆盖的会在交付时如实说明（例如其他游戏版本、干净系统）。

## 留给实施者决定
- 模组页的具体排版与动效；搜索结果的分页方式；图标缓存策略。
- 加载器版本下拉里是否折叠非稳定版（默认折叠）。

## 我替你做的决定（请过目）
1. 新增“模组”页作为侧栏独立入口，而不是塞进实例设置里。
2. Quilt 实例允许安装 Fabric 模组。
3. 只校验 sha1（现有下载器支持；Modrinth 同时给了 sha1，足够）。
4. 必需依赖自动列入安装清单，但必须经用户确认后才下载。
5. “禁用”用重命名 `.disabled` 实现，不删除文件。
6. 本批不提交到 git，等你说再提交。
