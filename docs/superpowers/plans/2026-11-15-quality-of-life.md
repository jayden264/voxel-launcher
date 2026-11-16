# 「更顺手」Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give Voxel a consistent, adjustable, keyboard-friendly interface and six day-to-day features: global search, drag-and-drop install, a task center, a better log viewer, system notifications and an instance detail page.

**Architecture:** Pure logic (ranking, log parsing, shortcut matching, job registry, scaling) lives in small TypeScript modules with Vitest tests; components stay thin. The only sizeable new backend work is `launcher-core` modules for dropped files and local `.mrpack` import, tested with Rust integration tests. UI preferences live in localStorage; nothing new leaves the machine.

**Tech Stack:** Tauri 2, Rust (`launcher-core`), React + TypeScript, Tailwind v4, Base UI/shadcn, zustand, motion, new dev-dependency Vitest, new plugin `tauri-plugin-notification`.

**Spec:** `docs/superpowers/specs/2026-11-15-quality-of-life-design.md`

## Global Constraints

- Windows only; stack stays Tauri 2 / React / Tailwind v4 / shadcn+Base UI. Only new runtime dependency: `tauri-plugin-notification` (+ its JS package); Vitest is dev-only.
- Motion tiers: fast 150ms, base 300ms, slow 600ms, with the existing easing (`EASE` in `src/lib/motion.ts`). Respect "reduce motion".
- UI scale range 80%–150% (webview zoom; CSS `zoom` only as browser-preview fallback). Density: `comfortable` | `compact`.
- UI preferences (density, scale, notifications) are stored in localStorage, never in the settings file.
- Dropped files: always confirm first, never overwrite (rename `name (2).ext`), refuse while the instance runs, undo available for 8 seconds, all paths through `paths::safe_join`.
- Notifications fire only when the window is not in the foreground and the setting is on (default on).
- Performance: 250-mod list and 3000-line log stay smooth (windowed rendering).
- Anti-goals: no telemetry or any new outbound data; no blocking animations; no removal of existing pages or flows; no parallel jobs; no pinyin search.
- Rust changes get integration tests in `crates/launcher-core/tests/`; TS logic gets Vitest tests next to the module; UI is also checked in the browser against the mock API (`npx vite --port 1420`) and in the real window where noted.
- Before any `cargo test`/`cargo check`, stop a running dev app (`taskkill //F //IM voxel-launcher.exe`); the Tauri dev server must not hold port 1420 when previewing with plain Vite.
- Commits are local only. Commit with `git -c user.name=jayden264 -c user.email=223105849+jayden264@users.noreply.github.com commit` and end the message with `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`. **Never push or publish a release without the user asking.**

## Review Focus

1. A dropped file whose name already exists, is too big, has a hostile name (`..\x.jar`), or is not a recognisable pack → never overwritten, never written outside the instance, refused with a plain reason. (Task 9 tests)
2. UI scale 80% and 150% with a dialog and the palette open → nothing overflows the window or becomes unclickable. (Task 4 browser check)
3. A 3000-line log with very long lines → only a few dozen DOM rows exist, scrolling stays smooth. (Task 7 test on `visibleRange` + DOM count check)
4. Typing in an input must not trigger plain-key shortcuts such as `?`; Ctrl+K still works there. (Task 5 tests)
5. Notifications: window focused → none; setting off → none; crash while minimised → exactly one. (Task 8 tests)
6. Search with no instances, no servers, or no matches → a clear empty state, never a crash. (Task 6 tests)

---

### Task 1: Test tooling and motion tokens

**Files:**
- Modify: `package.json` (devDependency `vitest`, script `"test": "vitest run"`), `vite.config.ts` (`test: { environment: 'node' }`), `src/lib/motion.ts`, `src/index.css`, and every `src/**/*.tsx` that hard-codes a duration.
- Test: `src/lib/motion.test.ts`

**Interfaces:**
- Produces: `export const DUR = { fast: 0.15, base: 0.3, slow: 0.6 } as const` (seconds, for `motion`); CSS variables `--dur-fast: 150ms; --dur-base: 300ms; --dur-slow: 600ms`; Tailwind usage `duration-(--dur-base)`.

- [ ] **Step 1: Write the failing tests** in `src/lib/motion.test.ts`:
  - `DUR` equals `{ fast: 0.15, base: 0.3, slow: 0.6 }`.
  - `no_stray_durations`: reads every `src/**/*.{ts,tsx}` except `motion.ts`, `Scene.tsx`, `art.tsx` (ambient loops) and asserts none contains `duration: <number>` or a Tailwind `duration-<digits>` class.
- [ ] **Step 2: Run** `npm install -D vitest && npx vitest run` → Expected: FAIL (`DUR` missing; stray durations listed).
- [ ] **Step 3: Implement** `DUR` and the CSS variables; replace each literal with the nearest tier (≤200ms fast, 201–400 base, >400 slow; Tailwind classes become `duration-(--dur-fast|base|slow)`). Include `src/components/ui/*`. Add `@media (prefers-reduced-motion: reduce)` setting the three variables to `0ms` in `index.css`.
- [ ] **Step 4: Run** `npx vitest run && npx tsc -b` → Expected: PASS, no type errors.
- [ ] **Step 5: Browser check**: open each of the 7 pages and one dialog in the mock preview; confirm nothing jumps or snaps (entrance, hover, dialog open/close all animate).
- [ ] **Step 6: Commit** "feat(ui): one motion rhythm and Vitest".

### Task 2: Preferences store

**Files:**
- Create: `src/lib/prefs.ts`
- Test: `src/lib/prefs.test.ts`

**Interfaces:**
- Produces: `type Density = 'comfortable' | 'compact'`; `SCALE_MIN = 0.8`, `SCALE_MAX = 1.5`, `SCALE_STEP = 0.05`; `clampScale(n: number): number`; `interface Prefs { density: Density; scale: number; notify: boolean }`; `loadPrefs(storage?: Pick<Storage,'getItem'>): Prefs`; `savePrefs(p: Prefs, storage?: Pick<Storage,'setItem'>): boolean`; `usePrefs` (zustand) with `set(patch: Partial<Prefs>): void` that clamps, saves and applies `document.documentElement.dataset.density`.

- [ ] **Step 1: Write the failing tests**: `clampScale(0.5)=0.8`, `(2)=1.5`, `(1.07)=1.05`, `(NaN)=1`; `loadPrefs` with corrupt JSON → defaults `{comfortable, 1, true}`; unknown density → `comfortable`; save→load round trip; a storage whose `getItem`/`setItem` throws → defaults / returns `false`, no throw.
- [ ] **Step 2: Run** `npx vitest run src/lib/prefs.test.ts` → Expected: FAIL (module missing).
- [ ] **Step 3: Implement** `prefs.ts` (storage key `voxel.prefs`); call `initPrefs()` from `src/main.tsx` before render, next to `initTheme()`.
- [ ] **Step 4: Run** the test → Expected: PASS.
- [ ] **Step 5: Commit** "feat(ui): preferences store".

### Task 3: Density (compact mode)

**Files:**
- Modify: `src/index.css`, `src/components/Bezel.tsx`, `src/components/kit.tsx`, list rows in `src/pages/{Mods,Instances}.tsx` and `src/components/{PlayTabs,HomeCards}.tsx`, `src/pages/Worlds.tsx`.

**Interfaces:**
- Consumes: `usePrefs` from Task 2 (sets `data-density` on `<html>`).
- Produces: CSS variables `--pad-card`, `--gap-list`, `--row-pad-y` with `html[data-density="compact"]` overrides (roughly 25% tighter).

- [ ] **Step 1: Implement** the variables (comfortable = today's values) and make the shared components and list rows read them instead of fixed `p-4`/`gap-2` values.
- [ ] **Step 2: Verify in the browser**: run `usePrefs.getState().set({ density: 'compact' })` via the preview console; measure the first mod row, world card and server card heights before/after → Expected: each at least 12% shorter, no clipped text, no horizontal scrollbar. Switch back → heights restore exactly.
- [ ] **Step 3:** `npx tsc -b`. **Commit** "feat(ui): compact density".

### Task 4: UI scale and the "界面" settings section

**Files:**
- Create: `src/lib/scale.ts`
- Modify: `src-tauri/capabilities/default.json` (add `core:webview:allow-set-webview-zoom`), `src/pages/Settings.tsx` (new section), `src/main.tsx`
- Test: `src/lib/scale.test.ts`

**Interfaces:**
- Consumes: `clampScale`, `usePrefs`.
- Produces: `applyScale(scale: number, deps?: { setZoom?: (n: number) => Promise<void>; root?: { style: { zoom: string } } }): Promise<void>` — uses `getCurrentWebview().setZoom` inside Tauri, else sets `root.style.zoom`.

- [ ] **Step 1: Write the failing tests** with injected deps: `applyScale(1.2, { setZoom })` calls `setZoom(1.2)`; `applyScale(9)` clamps to `1.5`; without `setZoom` it sets `root.style.zoom = '1.2'`; a rejecting `setZoom` falls back to `root.style.zoom` instead of throwing.
- [ ] **Step 2: Run** `npx vitest run src/lib/scale.test.ts` → Expected: FAIL.
- [ ] **Step 3: Implement** `scale.ts`; call it on startup and whenever `usePrefs` scale changes.
- [ ] **Step 4: Settings "界面" section**: density `Segmented` (舒适/紧凑), scale slider 80–150 with preset buttons 90/100/110/125/150 and "恢复默认", a note that "减弱动效" follows the system.
- [ ] **Step 5: Run** tests → PASS. **Browser check (Review Focus 2)**: with scale 0.8 and 1.5, open the share dialog and the settings page; every dialog's `getBoundingClientRect()` lies inside `window.innerWidth/innerHeight` and its buttons are clickable.
- [ ] **Step 6: Commit** "feat(ui): UI scale and appearance settings".

### Task 5: Keyboard everywhere

**Files:**
- Create: `src/lib/shortcuts.ts`, `src/components/ShortcutsHelp.tsx`
- Modify: `src/components/Sidebar.tsx`, `src/components/kit.tsx` (`Segmented`), `src/index.css` (focus ring), `src/App.tsx`, `src/lib/store.ts` (`helpOpen`)
- Test: `src/lib/shortcuts.test.ts`

**Interfaces:**
- Produces: `type KeyEventLike = { key: string; ctrlKey: boolean; altKey: boolean; shiftKey: boolean; target?: { tagName?: string; isContentEditable?: boolean } | null }`; `matchShortcut(e: KeyEventLike, keys: string): boolean` for strings like `"ctrl+k"`, `"ctrl+1"`, `"ctrl+="`, `"?"`; `SHORTCUTS: { id: string; keys: string; label: string }[]` (palette, pages 1–7, help, zoom in/out/reset); `useShortcuts(handlers: Record<string, () => void>): void`.

- [ ] **Step 1: Write the failing tests**: `"ctrl+k"` matches Ctrl+K and `K` with ctrl, not plain `k`; `"?"` does **not** match when `target.tagName` is `INPUT`, `TEXTAREA`, `SELECT` or `isContentEditable` (Review Focus 4) but `"ctrl+k"` still does; `"ctrl+="` matches `=` and `+` with ctrl; modifiers must match exactly (Ctrl+Shift+K ≠ `"ctrl+k"`).
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement** `shortcuts.ts`; wire `useShortcuts` in `App.tsx` (palette handler is a no-op until Task 6, pages via `setPage`, `?` opens `ShortcutsHelp`, zoom keys call `usePrefs.set`).
- [ ] **Step 4: Focus and roving**: visible pixel-style `:focus-visible` ring on every interactive element; Sidebar: ↑/↓ move focus between items, Enter activates; `Segmented`: ←/→ move selection; worlds/servers/mod lists: ↑/↓ move focus between rows.
- [ ] **Step 5: Run** tests → PASS. **Browser check**: press Tab 40 times across home, instances, content and settings; for each stop assert `getComputedStyle(document.activeElement).outlineStyle !== 'none'` (or a box-shadow focus ring) and that no stop is an element with `tabindex=-1` trap; confirm Esc closes every dialog.
- [ ] **Step 6: Commit** "feat(ui): keyboard navigation and shortcuts".

### Task 6: Global search (Ctrl+K)

**Files:**
- Create: `src/lib/search.ts`, `src/components/Palette.tsx`
- Modify: `src/lib/store.ts` (`paletteOpen`, `openPalette`, `closePalette`), `src/App.tsx`, `src/components/TitleBar.tsx` (search button)
- Test: `src/lib/search.test.ts`

**Interfaces:**
- Consumes: shortcut `ctrl+k` (Task 5), `usePrefs` (for the "切换紧凑模式" action).
- Produces: `interface Entry { id: string; group: '页面' | '实例' | '服务器' | '存档' | '操作'; title: string; subtitle?: string; keywords?: string; disabled?: string; run: () => void }`; `rank(query: string, entries: Entry[], recents: string[]): Entry[]` (max 30); `buildEntries(snap: Snapshot, ctx: { worlds: World[]; go: (p: Page) => void; launch: ...}): Entry[]`; recents persisted under `voxel.recent` (last 8 ids).

- [ ] **Step 1: Write the failing tests**: exact title > prefix > substring > subsequence; case-insensitive; Chinese substring (`"红石"` finds 「红石基地」); matches `keywords`; empty query → recents first (in recency order) then pages; no match → `[]` (Review Focus 6); no instances/servers → still returns pages and actions; result length ≤ 30; entries with `disabled` stay listed after enabled ones.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement** `search.ts`.
- [ ] **Step 4: `Palette.tsx`**: Base UI dialog; input autofocus; grouped results; ↑/↓ select, Enter runs and closes and records the recent, Esc closes and returns focus; worlds of the current instance loaded lazily when it opens; actions: check update, export diagnostics, back up now, toggle compact, open settings, shortcuts help.
- [ ] **Step 5: Browser check**: Ctrl+K from three different pages; type "红石" → worlds/instances match; Enter on an instance starts it (mock) and the palette closes; with the mock instance list emptied the palette still opens and shows the empty state.
- [ ] **Step 6: Commit** "feat: global search".

### Task 7: Log viewer

**Files:**
- Create: `src/lib/logparse.ts`
- Modify: `src/components/LogDrawer.tsx`
- Test: `src/lib/logparse.test.ts`

**Interfaces:**
- Produces: `type Level = 'debug' | 'info' | 'warn' | 'error' | 'unknown'`; `parseLine(line: string): { level: Level; time?: string; thread?: string; text: string }` (format `[12:00:01] [Render thread/INFO]: text`; lines without it, such as stack frames, inherit `unknown`); `filterLines(lines: string[], opts: { levels: Level[]; query: string }): number[]` (indices; stack-frame lines stay with the line above); `errorSummary(lines: string[]): string | null` (first ERROR line plus following lines that start with whitespace, `at `, `Caused by` or `...`, max 40 lines); `visibleRange(scrollTop: number, viewport: number, rowHeight: number, total: number, overscan = 10): { start: number; end: number }`.

- [ ] **Step 1: Write the failing tests**: `parseLine` for INFO/WARN/ERROR/FATAL(→error), a line without brackets, an empty string, a 5000-character line (no regex blow-up, finishes < 50 ms); `filterLines` with levels `['error']` keeps the error and its indented stack frames but not neighbouring info lines; query is case-insensitive; `errorSummary` returns the first block only and `null` when there is no error; `visibleRange(0, 400, 20, 3000)` → `{0, 30}`, `visibleRange(59000, 400, 20, 3000)` ends at `3000` (Review Focus 3: at most `ceil(400/20) + 2*10` rows).
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement** `logparse.ts`.
- [ ] **Step 4: Rebuild `LogDrawer.tsx`**: level chips (全部/信息/警告/错误) with counts, search box with highlight, match count, Enter/Shift+Enter to jump, wrap toggle (wrapped mode shows the last 500 lines with a notice), auto-scroll that pauses when the user scrolls up and resumes with a "跳到最新" button, copy buttons (visible lines / all / 报错摘要), windowed rows of fixed height.
- [ ] **Step 5: Run** tests → PASS. **Browser check**: feed 3000 lines (one 5000 characters) through the mock; assert the log container has < 80 row elements and scrolling to the bottom keeps up; filter to errors and copy the summary → clipboard text starts with the first error line.
- [ ] **Step 6: Commit** "feat: log viewer with filters and search".

### Task 8: Task center and system notifications

**Files:**
- Create: `src/lib/jobs.ts`, `src/lib/notify.ts`, `src/components/TaskCenter.tsx`
- Modify: `src/lib/store.ts` (`runInstall` registers jobs), `src/components/TitleBar.tsx`, `src-tauri/Cargo.toml` + `src-tauri/src/lib.rs` (`.plugin(tauri_plugin_notification::init())`), `src-tauri/capabilities/default.json` (`notification:default`), `package.json` (`@tauri-apps/plugin-notification`), Settings "界面" section (notifications switch)
- Test: `src/lib/jobs.test.ts`, `src/lib/notify.test.ts`

**Interfaces:**
- Consumes: `usePrefs().notify`.
- Produces: `interface Job { id: string; title: string; state: 'running' | 'done' | 'failed' | 'cancelled'; progress: number | null; startedAt: number; endedAt?: number; error?: string; cancel?: () => void }`; `useJobs` (zustand); `trackJob<T>(title: string, run: (h: { setProgress: (p: number | null) => void }) => Promise<T>, opts?: { cancel?: () => void }): Promise<T>` (rethrows; keeps the last 10 finished jobs); `shouldNotify(s: { enabled: boolean; focused: boolean }): boolean`; `notify(title: string, body: string): Promise<void>` (no-op when `shouldNotify` is false).

- [ ] **Step 1: Write the failing tests**: `trackJob` success → `done` with `endedAt`; rejection → `failed` with the message and the promise still rejects; `setProgress(0.4)` is visible while running; the 11th finished job evicts the oldest; calling `job.cancel` invokes the callback once and marks `cancelled`; `shouldNotify({enabled:true, focused:false})` true, focused true → false, enabled false → false (Review Focus 5).
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement** `jobs.ts` and `notify.ts`; wrap `runInstall` and the manual backup, export and diagnostics calls in `trackJob`; the existing cancel button calls `api.cancelTask`.
- [ ] **Step 4: `TaskCenter.tsx`**: title-bar icon with a count badge and a ring while running; popover listing running jobs (progress, speed, cancel) and the last 10 results (✓/✗, time, error text).
- [ ] **Step 5: Notifications**: on the `exited` event with a non-zero code call `notify('游戏崩溃了', diagnosis title or '点开启动器查看原因')`; on a finished install/download job call `notify`. Add the plugin to Rust, capability and npm; add the 通知 switch.
- [ ] **Step 6: Run** `npx vitest run && npx tsc -b && cargo check -p voxel-launcher` → PASS. **Browser check**: start the mock install, open the popover, cancel it; run a failing job and see it listed as failed. **Note for the final report:** the Windows toast itself cannot be verified without the real window; verify it there and say so.
- [ ] **Step 7: Commit** "feat: task center and notifications".

### Task 9: Dropped files — core

**Files:**
- Create: `crates/launcher-core/src/dropin.rs`; add `pub mod dropin;` to `lib.rs`
- Test: `crates/launcher-core/tests/dropin.rs`

**Interfaces:**
- Produces:
  - `enum DropKind { Mod, ModPack, Shader, ResourcePack, ZipUnknown, Unsupported { reason: String } }` (`Serialize`)
  - `fn classify(path: &Path) -> Result<DropKind>` (`.jar` with `fabric.mod.json`, `quilt.mod.json` or `META-INF/(neoforge.)mods.toml` → `Mod`; `.mrpack` or a zip with `modrinth.index.json` → `ModPack`; zip with a `shaders/` folder → `Shader`; zip with `pack.mcmeta` at root → `ResourcePack`; other zip → `ZipUnknown`; anything else → `Unsupported`)
  - `struct DropItem { src: PathBuf, kind: DropKind, dest_rel: Option<String>, renamed: bool }`, `struct DropPlan { items: Vec<DropItem> }`
  - `fn plan(dirs: &Dirs, inst: &Instance, paths: &[PathBuf], zip_as: &HashMap<PathBuf, DropKind>) -> Result<DropPlan>`
  - `struct Receipt { copied: Vec<PathBuf> }`, `fn apply(dirs: &Dirs, inst: &Instance, plan: &DropPlan) -> Result<Receipt>`, `fn undo(receipt: &Receipt) -> Result<usize>`
  - Limits: jar ≤ 300 MiB, zip/mrpack ≤ 1 GiB.

- [ ] **Step 1: Write the failing tests** (build jars/zips in a temp dir with the `zip` crate, same helpers as `tests/precheck.rs`): `classify` table over every kind above plus a corrupt file and a `.txt`; `plan` puts a mod at `mods/<name>` only for an instance with a loader and returns `Unsupported` (reason mentions 原版) for a vanilla instance; an existing `sodium.jar` makes the item `renamed` with `dest_rel = "mods/sodium (2).jar"`, and a third copy gets `(3)`; a file named `..\\evil.jar` or `a/b.jar` is rejected (Review Focus 1); over-cap files are `Unsupported` with a size reason; `zip_as` resolves a `ZipUnknown` to the chosen kind; `apply` writes the files and `undo` removes exactly what was copied and nothing else; `apply` never overwrites (re-checks for a file that appeared after `plan`).
- [ ] **Step 2: Run** `cargo test -p launcher-core --test dropin` → FAIL (module missing).
- [ ] **Step 3: Implement** `dropin.rs`: classify by extension then by zip entries; destination folders via `Dirs::content_dir(id, ContentKind)`; names validated with the same rules as `mods::check_filename`; copy via a `.part` file then rename.
- [ ] **Step 4: Run** → PASS. **Step 5: Commit** "feat(core): dropped file planning".

### Task 10: Local modpack import and the drop commands

**Files:**
- Modify: `crates/launcher-core/src/modpack.rs` (add `local_pack_summary`), `src-tauri/src/lib.rs` (new commands), `src/lib/api.ts` (types, calls, mocks)
- Test: `crates/launcher-core/tests/modpack_local.rs`

**Interfaces:**
- Consumes: `read_index`, `pack_target`, `install_pack_files` (existing), `dropin::{plan, apply, undo}`.
- Produces: `struct PackSummary { name: String, mc: String, loader: Option<String>, linked_files: usize, bundled_files: usize }`; `fn local_pack_summary(mrpack: &Path) -> Result<PackSummary>`; Tauri commands `drop_plan(instance_id: String, paths: Vec<String>, zip_as: HashMap<String, DropKind>) -> DropPlan`, `drop_apply(instance_id: String, plan: DropPlan) -> Receipt`, `drop_undo(receipt: Receipt) -> usize`, `pack_summary(path: String) -> PackSummary`, `import_mrpack(path: String, name: String, on_event: Channel<LaunchEvent>) -> Snapshot` (same steps as `install_modpack`, minus the download); `api.ts` equivalents.

- [ ] **Step 1: Write the failing tests**: `local_pack_summary` on a pack exported by `share::export_mrpack` (reuse the fixtures from `tests/instance_share.rs`) reports the right name, `mc`, loader and counts; a zip without `modrinth.index.json` → error naming the problem; a pack whose index lists a disallowed host → error (reuses `check_download`).
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement** `local_pack_summary`; add the commands (all refuse while the instance is running via `stopped_for`, run file work in `blocking`); `import_mrpack` mirrors `install_modpack` in `src-tauri/src/lib.rs`, reading the pack from the given path; cap the file at 1 GiB.
- [ ] **Step 4: Run** `cargo test --workspace && cargo check -p voxel-launcher` → PASS. **Step 5: Commit** "feat: drop and local pack commands".

### Task 11: Drag-and-drop install UI

**Files:**
- Create: `src/components/DropInstall.tsx`
- Modify: `src/App.tsx`, `src/lib/store.ts` (reuse `trackJob` from Task 8)

**Interfaces:**
- Consumes: `api.dropPlan/dropApply/dropUndo/packSummary/importMrpack`, `trackJob`.

- [ ] **Step 1: Implement `DropInstall`**: listen to Tauri's webview drag-drop event (`getCurrentWebview().onDragDropEvent`); while dragging show a full-window overlay "松开即可安装到「{实例}」"; on drop call `dropPlan`; for `ZipUnknown` items ask "这是光影还是资源包？" inline; a mod pack goes through `packSummary` and then a confirm "导入整合包「{name}」（Minecraft {mc}，{loader}）"; show **one** confirm dialog listing every item ("将装 3 个模组、1 个资源包到 Fabric 26.3；重名的会自动加序号"), Enter confirms; rejected items are listed with their reason and never block the others.
- [ ] **Step 2: After apply** show a toast with "撤销" for 8 seconds calling `dropUndo`; install work runs inside `trackJob`. Dragging while a game is running shows the refusal reason instead of the confirm.
- [ ] **Step 3: Browser check** (mock: simulate the drop by calling the same handler with fake paths): confirm dialog lists items; confirming adds rows to the mock installed list; "撤销" within 8 seconds removes them; a refused item shows its reason.
- [ ] **Step 4:** `npx tsc -b`. **Real-window check:** drop a real `.jar` into a scratch instance created for the test and confirm it appears in `mods/`; say in the report if not done. **Step 5: Commit** "feat: drag-and-drop install".

### Task 12: Instance detail page

**Files:**
- Create: `src/pages/InstanceDetail.tsx`, `src/components/InstanceMods.tsx`, `src/components/InstanceSettingsForm.tsx`; core fn `launch::read_log_tail`
- Modify: `crates/launcher-core/src/launch.rs`, `src-tauri/src/lib.rs` (`read_instance_log`), `src/lib/api.ts`, `src/lib/store.ts` (`Page` gains `'instance'`, `detailId`, `openInstance(id)`), `src/App.tsx`, `src/pages/Instances.tsx` (card click), `src/pages/Mods.tsx` and `src/pages/Instances.tsx` (extract shared pieces)
- Test: `crates/launcher-core/tests/log_tail.rs`

**Interfaces:**
- Produces: `fn read_log_tail(dirs: &Dirs, instance_id: &str, max_lines: usize) -> Result<Vec<String>>` (last `max_lines` lines of `logs/latest.log`, empty when missing); Tauri `read_instance_log(instance_id: String) -> Vec<String>` (3000 lines max); `InstanceMods({ inst })` (the installed + browse tabs of the content page for one fixed instance, extracted from `Mods.tsx`); `InstanceSettingsForm({ inst, onDone })` (the fields of today's `EditDialog`).

- [ ] **Step 1: Write the failing tests**: `read_log_tail` returns the last N lines in order, `[]` for a missing file, handles a final line without a newline, and never returns more than `max_lines` for a 10 000-line file.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement** `read_log_tail` (seek from the end, decode lossily) and the command.
- [ ] **Step 4: Extract** `InstanceMods` and `InstanceSettingsForm` from the existing pages without changing their behaviour (`Mods.tsx` and `Instances.tsx` now use them too).
- [ ] **Step 5: `InstanceDetail.tsx`**: header (cover scene, name, version, loader chip, launch/stop), tabs 概览 (play time from `statsSessions`, last backup from `backupList`, problems from `precheck`), 模组 (`InstanceMods`), 存档 (`WorldsTab`), 截图 (`ShotsTab`), 日志 (Task 7 viewer fed by `read_instance_log`), 设置 (`InstanceSettingsForm`). Clicking an instance card (not the launch button) opens it; Esc/back returns.
- [ ] **Step 6: Run** `cargo test --workspace && npx tsc -b`; **browser check** every tab renders for the mock Fabric instance and the vanilla one (模组 tab shows the "需要加载器" empty state); existing pages behave as before. **Step 7: Commit** "feat: instance detail page".

### Task 13: Transparency and plain-language errors

**Files:**
- Create: `src/lib/errors.ts`; Rust `netcheck::disclosure()`
- Modify: `crates/launcher-core/src/netcheck.rs`, `src-tauri/src/lib.rs` (`network_disclosure`), `src/lib/api.ts`, `src/components/About.tsx`, toast call sites that format errors
- Test: `crates/launcher-core/tests/disclosure.rs`, `src/lib/errors.test.ts`

**Interfaces:**
- Produces: `struct Disclosure { host: String, why: String }`, `fn disclosure() -> Vec<Disclosure>` (one entry per host the launcher can contact); `friendly(e: unknown): string`.

- [ ] **Step 1: Write the failing tests**: Rust — `disclosure()` contains `api.modrinth.com`, `cdn.modrinth.com`, `api.github.com`, `bmclapi2.bangbang93.com`, `piston-meta.mojang.com`, `launchermeta.mojang.com`, `resources.download.minecraft.net`, every entry has a non-empty Chinese `why`, no duplicate hosts. TS — `friendly(new Error('HTTP 429'))` mentions 请求过于频繁; `os error 5`/`拒绝访问` → a permission explanation; `os error 2` → file missing; a message that is already Chinese passes through unchanged; non-Error values become strings.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement**; replace ad-hoc `e instanceof Error ? e.message : String(e)` in toasts with `friendly`.
- [ ] **Step 4: "关于" gets a 透明度 block**: the disclosure table (host, why), where data lives (`snap.data_dir`) with a button to open it, and the sentence "不收集、不上传任何使用数据".
- [ ] **Step 5: Run** all tests → PASS; browser-check the About page. **Step 6: Commit** "feat: transparency and friendlier errors".

### Task 14: Whole-feature verification

**Files:** none new (fixes only, each with a test where it is logic).

- [ ] **Step 1: Run everything**: `npx vitest run`, `npx tsc -b`, `cargo test --workspace`; Expected: all pass, 0 warnings introduced.
- [ ] **Step 2: Keyboard-only walkthrough** in the browser: start from a fresh load, reach and use every new feature with the keyboard only (palette, help, scale, task center, log search, detail tabs, drop confirm).
- [ ] **Step 3: Performance**: mock 250 installed mods and 3000 log lines; Expected: the mods list renders in < 500 ms and the log container keeps < 80 row elements.
- [ ] **Step 4: Real window** (`npx tauri dev`): check scale zoom is crisp at 125%, a notification appears for a crash while the window is minimised, and a `.jar` drop lands in a scratch instance. Anything not verified goes in the final report as unverified.
- [ ] **Step 5: Build** `npx tauri build`; confirm the installer icon and size are unchanged in character (≈4.5 MB). **Do not publish.**
- [ ] **Step 6: Commit** any fixes; update `README.md` features list (global search, drag-and-drop, task center, instance page, scale/compact). Report what was and was not verified, and ask the user before pushing or releasing v0.2.0.
