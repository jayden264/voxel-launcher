# 常用模组提示、光影/资源包、整合包 — 设计

日期：2026-11-15　前置：加载器与模组（第一、二批，已完成）

## 意图与原因
用户的目标是“省心”：玩家想玩什么就在启动器里搜、点一下装好，该有的前置自动补齐。用户原话：创建 Fabric 等实例时先询问要不要自动装 Fabric API、Iris 这类常用模组；能下载光影包、资源包；能在启动器里搜索整合包并一键装好。三件事按顺序做：**1 常用模组提示 → 2 光影/资源包 → 3 整合包**（用户已确认顺序）。

## 目标
1. 创建 Fabric / Quilt / NeoForge 实例时，对话框列出“推荐模组”复选项（已查可用的才显示），勾选的在安装游戏后自动装好，依赖一起补齐。
2. “模组”页变成“内容”页：顶部切换 模组 / 光影 / 资源包，同样的“已安装 / 浏览”流程；装光影时若实例没有 Iris（及其依赖 Sodium），确认框里自动列出并一起装。
3. 下载页新增“整合包”标签：搜索 Modrinth 整合包，选版本，点“安装”就新建一个配好游戏版本、加载器、全部模组与配置的实例。

## 非目标
- CurseForge 整合包（要密钥）；导入本地 `.mrpack` 文件；数据包（要存档管理）；Forge 的光影（需要 Oculus，只给出提示）；整合包更新；服务端文件。
- 资源包/光影不做“启用/禁用”（游戏内自己选），只有安装、删除。

## 反目标
- 用户没点之前下载任何东西：推荐模组是用户勾选的；整合包与光影的确认框列出所有文件（含大小）。
- 整合包里的路径越出实例目录，或从未知主机下载文件。
- 整合包装到一半失败却留下一个半成品实例。

## 事实（已实测 2026-11-15）
- Modrinth `project_type` 有 `shader`、`resourcepack`、`modpack`。光影的版本 `loaders` 为 `["iris","optifine"]`，文件是 `.zip`，放 `shaderpacks/`；资源包 `loaders` 为 `["minecraft"]`，`.zip`，放 `resourcepacks/`。Iris 与 Sodium 支持 fabric / neoforge / quilt（不支持 forge）。
- `.mrpack` 是 zip：`modrinth.index.json`（`formatVersion` 1、`dependencies`：`minecraft` 与 `fabric-loader` / `quilt-loader` / `forge` / `neoforge` 的版本、`files[]`：`path`、`hashes.sha1/sha512`、`downloads[]`、`fileSize`、`env.client`）+ `overrides/`（及可选 `client-overrides/`）。样本 Fabulously Optimized 6.5.0：50 个文件，64 个 overrides，全部来自 `cdn.modrinth.com`，路径 `mods/`、`resourcepacks/`。规范允许的下载主机：`cdn.modrinth.com`、`github.com`、`raw.githubusercontent.com`、`gitlab.com`。

## 做法
- **A（选定）：沿用现有内核。** 推荐模组、光影/资源包都建立在 `mods.rs` 的“规划 → 确认 → 下载”之上，给计划里的每个文件加上“放哪个文件夹”；整合包是一个新模块 `modpack.rs`，创建实例 → 装游戏和加载器（沿用 `ensure_instance_version`）→ 下载文件 → 解压 overrides。
- B：整合包也做成“计划 + 确认框”再装。放弃：整合包动辄上百个文件，确认框没有信息量，用户要的是一键。整合包只在安装前显示名称、版本、游戏版本、加载器、文件数与总大小。

## 设计
### 内核
- `mods.rs`：`ContentKind { Mod, Shader, ResourcePack }`（`dir_name`：`mods`/`shaderpacks`/`resourcepacks`；`extension`：`.jar`/`.zip`；`project_type`；记录文件 `mods.json`/`shaders.json`/`resourcepacks.json`）。`PlanFile.dest`（缺省 Mod）；`install_plan`、`list_content`、`remove_content` 按 `dest` 工作，文件名按该类型的扩展名校验；原有 `list_installed`/`set_enabled`/`remove_mod` 保持不变（Mod）。`Target` 带上内容类型：搜索 facets 对光影/资源包只用 `project_type` 和 `versions`；光影版本查询用 `loaders=["iris"]`，资源包用 `["minecraft"]`。
- `plan_content(client, ctx, kind, project)`：Mod 走原有 `plan_install`；光影/资源包选该游戏版本的最新正式版的主文件；光影且加载器支持 Iris 且实例没有 Iris 时，再并入 `plan_install(iris)`（含 Sodium），标注“{光影} 需要”；Forge 实例加一条警告“Forge 上的光影需要 Oculus 或 OptiFine，启动器暂不自动安装”。
- `recommended.rs`：每种加载器的推荐清单（Fabric/Quilt：Fabric API 默认勾选，Sodium、Lithium、Mod Menu、Iris；NeoForge：Sodium、Lithium、Iris；Forge：无），`available(client, kind, mc)` 只保留在该版本有适配版本的，`plan_many` 把多个选择合并成一个计划（共同依赖只装一次）。
- `modpack.rs`：`search_modpacks`、`pack_versions`、`.mrpack` 解析（`parse_index`：校验 `formatVersion`、`game`、每个路径用 `safe_join`、下载主机白名单、sha1 必填）、`pack_target`（依赖 → 游戏版本 + `LoaderKind` + 加载器版本）、`install_pack_files`（下载 `env.client != "unsupported"` 的文件，校验 sha1/大小，解压 `overrides/` 与 `client-overrides/` 到实例目录，条目名经 `safe_join`，覆盖已有文件）。
### Tauri 与界面
- 命令：`recommended_mods(instance_kind, mc)`、`install_extras(instance_id, slugs, on_event)`；内容页复用 `mods_*` 并加 `kind` 参数；`modpack_search`、`modpack_versions`、`install_modpack(name, version, on_event) -> Snapshot`（失败时清理新建的实例与目录）。
- 创建实例对话框增加“推荐模组”区；“模组”页改名“内容”，加类型切换；下载页顶部加 “游戏版本 / 整合包” 切换，整合包用卡片列表 + 安装对话框（名称、版本下拉、概要）。

## 验证
离线测试（wiremock + 真实样本 `.mrpack` 索引做夹具）：计划合并与去重、光影自动补 Iris、`.zip`/`.jar` 扩展名与目录分流、恶意路径/条目被拒、白名单主机、`env` 过滤、失败清理。真机：创建 Fabric 实例勾选推荐模组并启动；装一个光影包进 Iris 实例并启动；安装 Fabulously Optimized（Fabric 1.21.1）并启动，确认模组加载。

## 我替你做的决定
1. 推荐清单是内置的（Fabric API 默认勾选，其余默认不勾），可以以后调整。
2. 整合包版本下拉默认选最新的正式版，并按版本列出游戏版本与加载器。
3. 整合包里已存在的同名文件直接覆盖（实例是新建的）。
4. 内容页的侧栏入口仍叫“模组”以外改为“内容”。
5. 本批同样不提交 git。
