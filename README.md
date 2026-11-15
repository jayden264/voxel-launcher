# Voxel 启动器

一个好看、稳、容易上手的 Minecraft Java 版启动器。像素 + 玻璃质感的界面，用 Rust 写的核心，目前支持 Windows。

> **非官方项目。** Voxel 与 Mojang Studios、Microsoft 没有任何关联，也未获得它们的授权或认可。Minecraft 是 Mojang Studios / Microsoft 的商标。本项目不分发游戏本体，游戏文件在你下载或启动时直接从 Mojang 官方服务器（或镜像）获取。

## 功能

- **启动更稳**：启动前自动检查重复模组、缺失前置、可用内存和磁盘空间；崩溃后用中文告诉你可能的原因和怎么办。
- **加载器**：Fabric、Quilt、Forge、NeoForge。
- **内容**：从 Modrinth 搜索、安装模组、光影、资源包和整合包；自动补齐前置；检测更新；识别重复模组。
- **存档管理**：存档列表、启动前自动备份、一键回退（可只回退单个存档）、游戏时长统计。
- **一键进入**：直接进入某个存档（Minecraft 1.20+）或保存的服务器，并显示服务器在线状态。
- **截图管理、实例复制、`.mrpack` 整合包导出、诊断包导出**。
- **离线账号**（暂未支持微软正版登录）。
- 首次运行引导、强调色与 OLED 主题、自定义背景。

## 下载

在 [Releases](../../releases) 页面下载 Windows 安装包。安装包目前没有代码签名，首次运行 Windows 可能提示“未知发布者”，点「更多信息 → 仍要运行」即可。

## 从源码构建

需要：[Rust](https://rustup.rs)、[Node.js](https://nodejs.org) 20 或更新、Windows 上的 Visual Studio 生成工具（C++）和 WebView2（Windows 11 自带）。

```bash
npm install
npm run tauri dev      # 开发模式
npx tauri build        # 生成安装包，输出在 target/release/bundle/nsis/
cargo test --workspace # 运行 Rust 测试
npx tsc -b             # 前端类型检查
```

项目结构：

| 目录 | 内容 |
|---|---|
| `crates/launcher-core` | 与界面无关的核心：下载、安装、启动、模组、备份等，附带测试 |
| `src-tauri` | Tauri 外壳，把核心功能暴露给界面 |
| `src` | React + TypeScript + Tailwind 界面 |

## 发布新版本

启动器启动时会向 GitHub 查询最新的 Release。发现新版本后，用户点「立即更新」，启动器会自动下载安装包、用 GitHub 提供的 SHA-256 校验、安装并重新打开，不用手动操作；没有校验值或校验不通过时不会安装，只会引导到下载页。

发布新版本时：把 `package.json`、`src-tauri/Cargo.toml` 和 `src-tauri/tauri.conf.json` 里的版本号改高，打包后在 GitHub 新建一个 Release，标签写成 `v` 加版本号（例如 `v0.2.0`），并把 `.exe` 安装包附到 Release 上。

## 致谢

[BMCLAPI](https://bmclapi2.bangbang93.com)（下载镜像）、[Modrinth](https://modrinth.com)（内容与接口）、Fabric / Quilt / Forge / NeoForge、[Tauri](https://tauri.app)、React。

## 许可证

[GPL-3.0](LICENSE)。你可以自由使用、修改和分发，但修改后的版本也必须以 GPL-3.0 开源，并保留原作者署名。
