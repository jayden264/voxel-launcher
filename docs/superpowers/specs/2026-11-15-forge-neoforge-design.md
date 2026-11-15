# Forge / NeoForge 加载器 — 第二批设计

日期：2026-11-15　状态：待你审阅　前置：[第一批设计](2026-11-15-loaders-and-mods-design.md)（已完成）

## 意图与原因
- 第一批做了 Fabric / Quilt。Forge 和 NeoForge 是模组玩家最多的两个加载器（1.20.1 及以前的大型模组基本在 Forge 上），不支持就留不住人。用户原话：下载时可选 Forge 等加载器并能装模组，“包括但不限于”，所以结构要能继续加别的。
- 用户已选定做法：**启动器自己执行安装步骤**，不调用官方安装器的无界面模式。

## 目标
1. 创建实例时可选 Forge / NeoForge 及版本，一步下完，装完能直接启动。
2. 复用第一批的模组页：Forge 实例搜索 `forge` 模组，NeoForge 实例搜索 `neoforge` 模组，安装、依赖、启用/禁用、删除都不改。
3. 安装过程有进度、可取消、走现有下载器和 BMCLAPI 镜像（大陆用户不会卡在 Forge 官方源）。
4. 支持范围：Minecraft 1.7.10 及以上、Forge 与 NeoForge 官方正式发布的版本；新旧两种安装器格式都处理。

## 非目标
- 服务端安装；快照版的加载器；整合包导入；LiteLoader、OptiFine 等其他加载器（结构留好扩展点，不实现）。
- Forge/NeoForge 之间互相兼容（NeoForge 实例只搜索 `neoforge` 模组，不搜 `forge`）。

## 反目标
- 装到一半失败却留下“看起来已安装”的版本：版本 JSON 必须最后写入。
- 执行安装器里的步骤却不校验：库要校验 sha1，每一步的输出要校验 sha1。
- 因为换加载器弄坏已有实例或共享文件。

## 已查明的事实（静态检查了真实安装器，没有执行任何东西）
- 新格式安装器（Forge 1.12.2 的最新版及以后、NeoForge 全部）包含 `install_profile.json`、`version.json`、`data/*.lzma`、`maven/…`（内置的库，如 Forge 的 shim jar）。1.21.1 的 Forge：21 个库、7 个步骤；NeoForge 21.1.172：70 个库、10 个步骤。
- `install_profile.json` 的结构：
  - `libraries`：步骤所需的库（`downloads.artifact.{path,url,sha1}`，部分 url 为空 = 在安装器的 `maven/` 目录里）。
  - `data`：命名变量，值有三种写法：`[maven坐标]`（解析成 `libraries/` 下的路径，支持 `@扩展名` 和分类器）、`'字面量'`、`/路径`（从安装器 jar 里解压到临时文件）；每项有 `client` / `server` 两个取值。
  - `processors`：有序步骤，每步有 `jar`（maven 坐标，主类取自它的 MANIFEST）、`classpath`（库坐标列表）、`args`（含 `{变量}` 占位符）、可选 `sides`（只在该侧运行；缺省=两侧）、可选 `outputs`（输出文件 → 期望 sha1，可含变量）。
  - 内置变量：`{ROOT}`（游戏根目录，对应我们的 `shared/`）、`{INSTALLER}`（安装器 jar）、`{MINECRAFT_JAR}`（原版客户端 jar）、`{SIDE}`（取 `client`）、`{MINECRAFT_VERSION}`、`{LIBRARY_DIR}`。
  - `version.json` 是最终要写入 `versions/<id>/` 的版本 JSON，带 `inheritsFrom`，与第一批的 Fabric profile 同一类东西，现有合并逻辑可直接用。
- 旧格式安装器（1.7.10 及更早的 1.12.x 构建）：`install_profile.json` 里是 `install{…}` + `versionInfo{…}`，通用 jar 在安装器里，没有步骤要执行。
- 版本号：Forge 的 maven 版本形如 `1.21.1-52.1.0`（前缀是 Minecraft 版本；1.7.10 是 `1.7.10-10.13.4.1614-1.7.10`），`promotions_slim.json` 标出 recommended / latest。NeoForge 的 maven 版本如 `21.1.172`，Minecraft 1.21.1 ↔ `21.1.*`、1.21 ↔ `21.0.*`，新命名 Minecraft 26.3 ↔ `26.3.0.*`；带 `-alpha`/`-beta` 的是测试版。
- 版本 id：Forge `1.21.1-forge-52.1.0`，NeoForge `neoforge-21.1.172`（以安装器里 `version.json` 的 `id` 为准，必须与我们按规则算出的 id 一致，不一致就报错）。
- Forge 的 `install_profile.json` 里有一条声明：请不要自动化下载安装 Forge，如要自动化请考虑赞助（Patreon）。NeoForge 没有。
- 官方安装器有 6 MB 左右，Forge 与 NeoForge 的 maven 站都能直接下载；BMCLAPI 另有 Forge/NeoForge 的镜像路径（实施时先实测映射，失败即回退官方）。

## 已考虑的做法
- **A：调用官方安装器的无界面模式。** 代码少；但安装器自己去官方源下载、不走镜像、没有进度、取消只能杀进程。已被用户否决。
- **B（选定）：启动器自己读 `install_profile.json` 并执行步骤。** 复用下载器（校验、镜像、进度、取消），步骤的解析与参数替换可以用真实 JSON 做成离线测试；代价是代码更多、Forge 将来改格式要跟。

## 设计

### 内核（`launcher-core`）
- `loader.rs`：`LoaderKind` 增加 `Forge`、`NeoForge`；`profile_id`、`list_loader_versions`、`default_version`、`install_loader` 按种类分派。Fabric/Quilt 的现有行为与测试保持不变。
- 新模块 `forge.rs`（Forge 与 NeoForge 共用，因为安装器格式相同）：
  - 版本列表：Forge 用官方 `maven-metadata.xml` + `promotions_slim.json`（recommended 标为稳定默认）；NeoForge 用官方 `maven-metadata.xml` 并按上面的规则映射 Minecraft 版本。结果同样是 `Vec<LoaderVersion>`（最新在前，`stable` 表示正式版）；空列表 = 不支持该游戏版本。
  - `install_forge_like(dl, dirs, kind, mc, loader_version, java, progress) -> id`：
    1. 下载安装器 jar（`…/{坐标}-installer.jar`，校验 `.sha1` sidecar）到 `shared/installers/`；确认原版已安装（`install_version(mc)`）。
    2. 读 `install_profile.json`（新格式：库 + 步骤；旧格式：转换 `versionInfo`，解压通用 jar）。路径一律经 `safe_join`，解压的条目名同样（含 zip-slip 测试）。
    3. 下载 `libraries`（含步骤的 classpath 所需），安装器内置的库从 jar 里解压到 `libraries/` 对应路径。
    4. 解析 `data`；对每个步骤：`sides` 不含 `client` 就跳过；所有 `outputs` 已存在且 sha1 正确就跳过（安装可重复）；否则用 Java 运行（`-cp` 步骤 jar + classpath，主类取 MANIFEST，参数替换后逐个传给 `Command`，不拼字符串）；结束后再校验 `outputs`，不符报错并附日志末尾几行。
    5. 全部成功后才把 `version.json` 原子写入 `versions/<id>/<id>.json`，并校验 `id` 与 `inheritsFrom`。
  - 步骤的输出写入 `shared/logs/forge-<id>.log`；取消（`CancellationToken`）会结束子进程并清掉不完整的输出文件。
- Java：步骤用 Mojang 运行时中该 Minecraft 版本对应的那一个（1.21.x 为 Java 21，1.12.2 为 Java 8）；`install_loader` 增加 `java` 参数，由调用方（Tauri 层）准备好后传入。
- `mods.rs`：`target_for` 增加 `Forge → ["forge"]`、`NeoForge → ["neoforge"]`。
- `mirror.rs`：新增 `maven.minecraftforge.net` 与 `maven.neoforged.net/releases` 的映射（先实测），带单测。
- 第一批里写死的“仅 Fabric/Quilt”之处（`check_segment`、`LOADER_LABEL` 等）改成按种类分派。

### Tauri 与界面
- 命令基本复用：`list_loader_versions(kind, mc)`、`create_instance`、`install_instance`、`mods_*` 的参数不变；`install_instance` 在有加载器时先准备 Java 再调用 `install_loader`。
- 创建实例对话框：加载器分段选择变为 原版 / Fabric / Quilt / Forge / NeoForge，规则同前（不支持的置灰）。选中 Forge 时显示一行说明：“Forge 官方希望自动安装时赞助项目”，附链接 `https://www.patreon.com/LexManos/`（用系统浏览器打开）。
- 实例卡片的徽章、模组页的标题行、模组搜索都已通用，只需 `LOADER_LABEL` 增加两项。
- 进度：步骤阶段显示“运行 Forge 安装步骤（3/7）”，沿用现有进度条与取消。

## 验证
- 离线单测（用真实 JSON 夹具）：Forge 1.21.1-52.1.0、NeoForge 21.1.172、Forge 1.12.2-14.23.5.2860 的 `install_profile.json`/`version.json`；旧格式（1.7.10）夹具；`data` 三种写法、`@扩展名`/分类器坐标、变量替换、`sides` 过滤、`outputs` 校验、版本映射（NeoForge ↔ Minecraft）、版本列表解析（真实 maven-metadata 夹具）、恶意路径/条目被拒、取消清理。
- 真机（**需要执行 Forge / NeoForge 官方发布的安装工具，见下**）：Forge 1.21.1、NeoForge 21.1.172、Forge 1.12.2、Forge 1.7.10 各装一次并启动，日志里确认加载器和一个模组（Modrinth 安装）都已加载。
- 未覆盖的会如实说明。

## 需要你决定
- **能否在本机执行这些安装步骤用于验证？** 它们是 Forge / NeoForge 官方发布的、由安装器携带或从官方 maven 下载的 Java 工具，会在 `shared/` 里生成文件。上次我试跑安装器被自动模式拦下了；这次的做法与安装器等价，真机验证同样需要执行它们。需要你放行（在会话里同意，或在权限设置里允许），否则我只做离线可测的部分，真机验证交给你手动执行。

## 我替你做的决定（请过目）
1. Forge 与 NeoForge 共用一个 `forge.rs`，因为格式相同。
2. NeoForge 实例只搜 `neoforge` 模组（不兼容 Forge 模组的版本占多数）。
3. NeoForge 与 Minecraft 版本的对应用规则推出，不依赖 BMCLAPI 的列表接口；实施时先对照真实 maven-metadata 校验。
4. Forge 的赞助说明只出现在选择 Forge 时的对话框里，不弹窗打扰。
5. 步骤失败时保留日志文件，错误信息只显示日志末尾几行。
6. 本批同样不提交 git，等你说。
