// Typed bridge to the Rust backend. When the page is opened in a plain browser
// (no Tauri), a small in-memory mock stands in so the UI can be previewed.

export type Source = 'mirror' | 'official'

export interface Account {
  name: string
  uuid: string
}

export interface Settings {
  schema_version: number
  source: Source
  java_path: string | null
  accounts: Account[]
  selected_account: string | null
  selected_instance: string | null
  max_downloads: number
  default_memory_mb: number | null
  /** Back the worlds up before every launch. */
  auto_backup: boolean
  /** Ask GitHub for a newer version each time the launcher starts. */
  check_updates: boolean
  ignored_update: string | null
  /** Servers saved for one-click joining. */
  servers: ServerEntry[]
}

export type LoaderKind = 'fabric' | 'quilt' | 'forge' | 'neoforge'

export interface LoaderRef {
  kind: LoaderKind
  version: string
}

export interface LoaderVersion {
  version: string
  stable: boolean
}

/** A mod offered (and possibly pre-ticked) when creating an instance. */
export interface Recommended {
  slug: string
  title: string
  why: string
  default_on: boolean
}

export interface Instance {
  id: string
  name: string
  /** What gets launched: a vanilla id, or a loader profile id. */
  version_id: string
  /** Vanilla Minecraft version when a loader is used (null on older instances). */
  mc_version: string | null
  loader: LoaderRef | null
  memory_mb: number | null
  jvm_args: string
  java_path: string | null
  width: number | null
  height: number | null
  running: { pid: number; start_time: number } | null
}

export interface Snapshot {
  settings: Settings
  instances: Instance[]
  system: { max_mem_mb: number; recommended_mem_mb: number; mica: boolean; version: string; repo_url: string | null }
  data_dir: string
}

export interface VersionDto {
  id: string
  kind: string
  release_time: string
}

export interface VersionList {
  latest_release: string
  latest_snapshot: string
  versions: VersionDto[]
}

export interface ModHit {
  project_id: string
  slug: string
  title: string
  description: string
  icon_url: string | null
  downloads: number
  author: string
}

export interface SearchResult {
  hits: ModHit[]
  total: number
}

/** Which kind of project, and so which folder of the instance. */
export type ContentKind = 'mod' | 'shader' | 'resourcepack'

export interface PlanFile {
  project_id: string
  version_id: string
  title: string
  version_number: string
  icon_url: string | null
  filename: string
  url: string
  sha1: string
  size: number
  /** Title of the mod that needs this one (dependencies only). */
  required_by: string | null
  user_selected: boolean
  dest: ContentKind
}

export interface InstallPlan {
  files: PlanFile[]
  total_bytes: number
  warnings: string[]
}

export interface InstalledMod {
  filename: string
  enabled: boolean
  size: number
  project_id: string | null
  title: string | null
  version_number: string | null
  icon_url: string | null
  user_installed: boolean
  mod_id: string | null
  /** Another enabled file has the same mod id. */
  duplicate: boolean
}

/** Something wrong with an instance that can be seen before the game starts. */
export interface Problem {
  code: 'duplicate-mods' | 'missing-dependency' | 'low-memory'
  title: string
  detail: string
  /** What the UI can offer: disable the extra copies, or go and install the prerequisite. */
  fix: 'disable-duplicates' | 'mods' | null
}

/** How to order and narrow a content search. */
export interface SearchOpts {
  sort?: 'relevance' | 'downloads' | 'follows' | 'newest' | 'updated'
  category?: string
}

export interface PackExport {
  path: string
  folder: string
  /** Mods the pack only links to. */
  linked: number
  /** Mods and packs carried inside because Modrinth does not host them. */
  bundled: number
  size: number
}

export interface UpdateInfo {
  version: string
  /** Release notes (Markdown). */
  notes: string
  url: string
  installer_url: string | null
}

export interface UpdateCheck {
  /** The repository address is set, so checking is possible. */
  configured: boolean
  current: string
  update: UpdateInfo | null
}

export interface DiagResult {
  path: string
  folder: string
}

export interface JavaCheck {
  /** A Java runtime must be downloaded before this instance can start. */
  needed: boolean
  major: number
  component: string
}

export interface NetProbe {
  name: string
  url: string
  ok: boolean
  status: number | null
  ms: number | null
  error: string | null
}

export interface ServerEntry {
  id: string
  name: string
  host: string
  port: number
}

export interface ServerStatus {
  motd: string
  online: number
  max: number
  version: string
  protocol: number
  latency_ms: number
  favicon: string | null
}

export interface Screenshot {
  file: string
  /** Milliseconds since the epoch. */
  taken: number
  size: number
}

/** Where to go once the game is up. */
export type QuickPlay = { kind: 'world'; name: string } | { kind: 'server'; host: string; port: number }

export interface World {
  /** Folder name inside saves. */
  dir: string
  name: string
  /** Milliseconds since the epoch. */
  last_played: number | null
  game_mode: 'survival' | 'creative' | 'adventure' | 'spectator' | null
  hardcore: boolean
  version: string | null
  size: number
  /** Seconds, as the game itself counted them. */
  play_secs: number
  icon: string | null
}

export type BackupScope = 'worlds' | 'full'
export type BackupKind = 'auto' | 'manual' | 'safety'

export interface BackupInfo {
  id: string
  /** Milliseconds since the epoch. */
  created: number
  kind: BackupKind
  scope: BackupScope
  reason: string
  size: number
  worlds: string[]
}

export interface PlaySession {
  instance_id: string
  /** Seconds since the epoch. */
  start: number
  secs: number
  crashed: boolean
}

export interface WorldStat {
  instance_id: string
  dir: string
  name: string
  play_secs: number
}

export interface DepInfo {
  project_id: string
  title: string
  icon_url: string | null
  dependency_type: 'required' | 'optional'
  installed: boolean
}

export interface DetailVersion {
  id: string
  version_number: string
  version_type: string
  game_versions: string[]
  loaders: string[]
  date_published: string
  downloads: number
}

export interface ProjectDetail {
  id: string
  slug: string
  title: string
  description: string
  body: string
  icon_url: string | null
  gallery: { url: string; title: string | null }[]
  categories: string[]
  downloads: number
  followers: number
  license: string | null
  source_url: string | null
  issues_url: string | null
  wiki_url: string | null
  loaders: string[]
  game_versions: string[]
  /** For the chosen instance: does any version run on it? null without an instance. */
  compatible: boolean | null
  versions: DetailVersion[]
  dependencies: DepInfo[]
}

export interface MissingDep {
  project_id: string
  title: string
  icon_url: string | null
  needed_by: string[]
}

export interface UpdateItem {
  filename: string
  title: string
  current_version: string
  new_version: string
  file: PlanFile
}

export interface ScanReport {
  identified: number
  updates: UpdateItem[]
  missing: MissingDep[]
}

/** One downloadable version of a Modrinth modpack. */
export interface PackVersion {
  id: string
  name: string
  version_number: string
  game_versions: string[]
  loaders: string[]
  version_type: string
  file: { filename: string; url: string; sha1: string; size: number; primary: boolean }
}

export type LaunchEvent =
  | { type: 'stage'; label: string }
  | {
      type: 'progress'
      stage: string
      files_done: number
      files_total: number
      bytes_done: number
      bytes_total: number
    }
  | { type: 'log'; line: string }
  | { type: 'started'; pid: number }
  | { type: 'ready' }
  | { type: 'notice'; text: string }
  | { type: 'exited'; code: number | null; crash_report: string | null; diagnosis: Diagnosis | null }

/** A plain-language explanation of why the game stopped, with what to do about it. */
export interface Diagnosis {
  code: string
  title: string
  advice: string
  detail: string | null
}

export type EventHandler = (e: LaunchEvent) => void

export const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core')
  try {
    return await invoke<T>(cmd, args)
  } catch (e) {
    throw new Error(typeof e === 'string' ? e : e instanceof Error ? e.message : String(e))
  }
}

async function withChannel<T>(
  cmd: string,
  args: Record<string, unknown>,
  onEvent: EventHandler,
): Promise<T> {
  const { Channel } = await import('@tauri-apps/api/core')
  const ch = new Channel<LaunchEvent>()
  ch.onmessage = onEvent
  return call<T>(cmd, { ...args, onEvent: ch })
}

// ------------------------------------------------------------------ real api

const real = {
  getSnapshot: () => call<Snapshot>('get_snapshot'),
  listVersions: () => call<VersionList>('list_versions'),
  addAccount: (name: string) => call<Snapshot>('add_account', { name }),
  removeAccount: (name: string) => call<Snapshot>('remove_account', { name }),
  selectAccount: (name: string) => call<Snapshot>('select_account', { name }),
  saveSettings: (settings: Settings) => call<Snapshot>('save_settings', { settings }),
  /** `mcVersion` is the Minecraft version; with a loader the instance launches its profile. */
  createInstance: (name: string, mcVersion: string, loader?: LoaderRef | null) =>
    call<Snapshot>('create_instance', { name, versionId: mcVersion, loader: loader ?? null }),
  installInstance: (id: string, on: EventHandler) => withChannel<void>('install_instance', { id }, on),
  recommendedMods: (kind: LoaderKind, mcVersion: string) =>
    call<Recommended[]>('recommended_mods', { kind, mcVersion }),
  installExtras: (instanceId: string, slugs: string[], on: EventHandler) =>
    withChannel<void>('install_extras', { instanceId, slugs }, on),
  /** Open a web page in the system browser. */
  openExternal: async (url: string): Promise<void> => (await import('@tauri-apps/plugin-opener')).openUrl(url),
  listLoaderVersions: (kind: LoaderKind, mcVersion: string) =>
    call<LoaderVersion[]>('list_loader_versions', { kind, mcVersion }),
  contentSearch: (instanceId: string, kind: ContentKind, query: string, offset: number, opts?: SearchOpts) =>
    call<SearchResult>('content_search', { instanceId, kind, query, offset, opts: opts ?? null }),
  contentPlan: (instanceId: string, kind: ContentKind, projectId: string) =>
    call<InstallPlan>('content_plan', { instanceId, kind, projectId }),
  contentList: (instanceId: string, kind: ContentKind) => call<InstalledMod[]>('content_list', { instanceId, kind }),
  contentRemove: (instanceId: string, kind: ContentKind, filename: string) =>
    call<void>('content_remove', { instanceId, kind, filename }),
  /** Search everything of a kind, regardless of any instance. */
  contentSearchAll: (kind: ContentKind, query: string, offset: number, opts?: SearchOpts) => call<SearchResult>('content_search_all', { kind, query, offset, opts: opts ?? null }),
  /** A project page; with an instance, versions and prerequisites are those for it. */
  contentDetail: (projectId: string, kind: ContentKind, instanceId: string | null) =>
    call<ProjectDetail>('content_detail', { projectId, kind, instanceId }),
  contentPlanMissing: (instanceId: string, projectIds: string[]) => call<InstallPlan>('content_plan_missing', { instanceId, projectIds }),
  networkTest: () => call<NetProbe[]>('network_test'),
  cloneInstance: (id: string, name: string, includeSaves: boolean) => call<Snapshot>('clone_instance', { id, name, includeSaves }),
  exportInstancePack: (id: string, name: string, version: string, includeSaves: boolean, includeOptions: boolean) =>
    call<PackExport>('export_instance_pack', { id, name, version, includeSaves, includeOptions }),
  checkUpdate: (manual: boolean) => call<UpdateCheck>('check_update', { manual }),
  ignoreUpdate: (version: string) => call<void>('ignore_update', { version }),
  /** Save a diagnostics zip to Downloads and show it in Explorer. */
  exportDiagnostics: (instanceId: string | null) => call<DiagResult>('export_diagnostics', { instanceId }),
  javaCheck: (instanceId: string) => call<JavaCheck>('java_check', { instanceId }),
  installJava: (instanceId: string, on: EventHandler) => withChannel<void>('install_java', { instanceId }, on),
  serverAdd: (name: string, address: string) => call<Snapshot>('server_add', { name, address }),
  serverRemove: (id: string) => call<Snapshot>('server_remove', { id }),
  serverPing: (id: string) => call<ServerStatus>('server_ping', { id }),
  screenshotsList: (instanceId: string) => call<Screenshot[]>('screenshots_list', { instanceId }),
  /** A scaled-down JPEG as a data URL: small for the grid, larger for viewing. */
  screenshotImage: (instanceId: string, file: string, maxWidth: number) => call<string>('screenshot_image', { instanceId, file, maxWidth }),
  screenshotDelete: (instanceId: string, file: string) => call<void>('screenshot_delete', { instanceId, file }),
  screenshotsOpen: (instanceId: string) => call<void>('screenshots_open', { instanceId }),
  worldsList: (instanceId: string) => call<World[]>('worlds_list', { instanceId }),
  worldDelete: (instanceId: string, world: string) => call<void>('world_delete', { instanceId, world }),
  worldOpen: (instanceId: string, world: string) => call<void>('world_open', { instanceId, world }),
  backupList: (instanceId: string) => call<BackupInfo[]>('backup_list', { instanceId }),
  /** Null when there was nothing to back up. */
  backupCreate: (instanceId: string, scope: BackupScope, reason?: string) => call<BackupInfo | null>('backup_create', { instanceId, scope, reason: reason ?? null }),
  /** Roll back a whole backup, or just one world of it. The current state is backed up first. */
  backupRestore: (instanceId: string, backupId: string, world?: string) => call<void>('backup_restore', { instanceId, backupId, world: world ?? null }),
  backupDelete: (instanceId: string, backupId: string) => call<void>('backup_delete', { instanceId, backupId }),
  statsSessions: () => call<PlaySession[]>('stats_sessions'),
  statsWorlds: () => call<WorldStat[]>('stats_worlds'),
  /** Problems in the instance's mods that would stop the game from starting. */
  precheck: (instanceId: string) => call<Problem[]>('precheck_instance', { instanceId }),
  /** Disable all but the newest copy of each duplicated mod; returns the disabled file names. */
  modsDisableDuplicates: (instanceId: string) => call<string[]>('mods_disable_duplicates', { instanceId }),
  /** Name unknown files via Modrinth and list newer versions. */
  contentScan: (instanceId: string, kind: ContentKind) => call<ScanReport>('content_scan', { instanceId, kind }),
  /** Download every file of a confirmed plan (mods, shaders, resource packs). */
  modsInstall: (instanceId: string, plan: InstallPlan, on: EventHandler) =>
    withChannel<void>('mods_install', { instanceId, plan }, on),
  modsSetEnabled: (instanceId: string, filename: string, enabled: boolean) =>
    call<void>('mods_set_enabled', { instanceId, filename, enabled }),
  modpackSearch: (query: string, offset: number) => call<SearchResult>('modpack_search', { query, offset }),
  modpackVersions: (projectId: string) => call<PackVersion[]>('modpack_versions', { projectId }),
  /** Creates a new, fully set-up instance from a modpack version and returns the updated snapshot. */
  installModpack: (name: string, version: PackVersion, on: EventHandler) =>
    withChannel<Snapshot>('install_modpack', { name, version }, on),
  updateInstance: (instance: Instance) => call<Snapshot>('update_instance', { instance }),
  deleteInstance: (id: string, deleteFiles: boolean) =>
    call<Snapshot>('delete_instance', { id, deleteFiles }),
  selectInstance: (id: string) => call<Snapshot>('select_instance', { id }),
  probeJava: (path: string) => call<{ major: number; version: string }>('probe_java', { path }),
  openGameDir: (id: string) => call<void>('open_game_dir', { id }),
  openDataDir: () => call<void>('open_data_dir'),
  cancelTask: () => call<void>('cancel_task'),
  killGame: (id: string) => call<boolean>('kill_game', { id }),
  installVersion: (versionId: string, on: EventHandler) =>
    withChannel<void>('install_version', { versionId }, on),
  launchInstance: (id: string, on: EventHandler, quickPlay?: QuickPlay) => withChannel<void>('launch_instance', { id, quickPlay: quickPlay ?? null }, on),
  attachGame: (id: string, on: EventHandler) => withChannel<boolean>('attach_game', { id }, on),
  readTextFile: (path: string) => call<string>('read_text_file', { path }),
  pickJava: async (): Promise<string | null> => {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const r = await open({
      multiple: false,
      title: '选择 java.exe',
      filters: [{ name: 'Java', extensions: ['exe'] }],
    })
    return typeof r === 'string' ? r : null
  },
  windowMinimize: async () => (await import('@tauri-apps/api/window')).getCurrentWindow().minimize(),
  windowToggleMax: async () =>
    (await import('@tauri-apps/api/window')).getCurrentWindow().toggleMaximize(),
  windowClose: async () => (await import('@tauri-apps/api/window')).getCurrentWindow().close(),
}

// ----------------------------------------------------------------- mock api

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))

function mockUuid(name: string) {
  let h = 0
  for (const c of name) h = (h * 31 + c.charCodeAt(0)) >>> 0
  const hex = (h.toString(16).padStart(8, '0') + '0'.repeat(24)).slice(0, 32)
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-3${hex.slice(13, 16)}-a${hex.slice(17, 20)}-${hex.slice(20, 32)}`
}

const MOCK_VERSIONS: VersionDto[] = [
  ['26.3', 'release', '2026-09-15'],
  ['26.2', 'release', '2026-06-16'],
  ['26.1.2', 'release', '2026-04-09'],
  ['1.21.11', 'release', '2025-12-09'],
  ['1.21.4', 'release', '2024-12-03'],
  ['1.21.1', 'release', '2024-08-08'],
  ['1.20.1', 'release', '2023-06-12'],
  ['1.19.4', 'release', '2023-03-14'],
  ['1.18.2', 'release', '2022-02-28'],
  ['1.16.5', 'release', '2021-01-15'],
  ['1.12.2', 'release', '2017-09-18'],
  ['1.8.9', 'release', '2015-12-09'],
  ['1.7.10', 'release', '2014-06-26'],
  ['26.4-snapshot-8', 'snapshot', '2026-11-10'],
  ['26.4-snapshot-7', 'snapshot', '2026-11-03'],
  ['25w44a', 'snapshot', '2025-10-28'],
].map(([id, kind, d]) => ({ id, kind, release_time: `${d}T12:00:00+00:00` }))

let mock: Snapshot = {
  settings: {
    schema_version: 1,
    source: 'mirror',
    java_path: null,
    accounts: [{ name: 'Steve', uuid: mockUuid('Steve') }],
    selected_account: 'Steve',
    selected_instance: 'survival-1-21-1',
    max_downloads: 24,
    default_memory_mb: null,
    auto_backup: true,
    check_updates: true,
    ignored_update: null,
    servers: [{ id: 'srv-1', name: '示例服务器', host: 'play.example.com', port: 25565 }, { id: 'srv-2', name: '朋友的服', host: '192.168.1.8', port: 25566 }],
  },
  instances: [
    {
      id: 'fabric-1-21-1',
      name: 'Fabric 模组生存',
      version_id: 'fabric-loader-0.20.1-1.21.1',
      mc_version: '1.21.1',
      loader: { kind: 'fabric', version: '0.20.1' },
      memory_mb: 4096,
      jvm_args: '',
      java_path: null,
      width: null,
      height: null,
      running: null,
    },
    {
      id: 'survival-1-21-1',
      name: '生存世界',
      version_id: '1.21.1',
      mc_version: null,
      loader: null,
      memory_mb: 4096,
      jvm_args: '',
      java_path: null,
      width: null,
      height: null,
      running: null,
    },
    {
      id: 'pvp-1-8-9',
      name: '经典 PVP',
      version_id: '1.8.9',
      mc_version: null,
      loader: null,
      memory_mb: null,
      jvm_args: '',
      java_path: null,
      width: null,
      height: null,
      running: null,
    },
    {
      id: 'redstone-1-20-1',
      name: '红石实验室',
      version_id: '1.20.1',
      mc_version: null,
      loader: null,
      memory_mb: 3072,
      jvm_args: '',
      java_path: null,
      width: 1280,
      height: 720,
      running: null,
    },
  ],
  system: { max_mem_mb: 12288, recommended_mem_mb: 4096, mica: false, version: '0.1.0', repo_url: 'https://github.com/acme/voxel' },
  data_dir: 'C:\\Users\\you\\AppData\\Roaming\\MinecraftLauncher',
}
const snap = () => structuredClone(mock)

// Mock Modrinth: a few mods with dependencies, one with no compatible version.
interface MockMod extends ModHit {
  size: number
  requires: string[]
  unsupported?: boolean
}
const mockMod = (slug: string, title: string, author: string, downloads: number, description: string, size: number, requires: string[] = [], unsupported = false): MockMod => ({
  project_id: slug,
  slug,
  title,
  author,
  downloads,
  description,
  icon_url: null,
  size,
  requires,
  unsupported,
})
const MOCK_CATALOG: MockMod[] = [
  mockMod('fabric-api', 'Fabric API', 'modmuss50', 287_695_787, 'Essential hooks for modding with Fabric.', 2_340_000),
  mockMod('sodium', 'Sodium', 'jellysquid3', 252_637_937, 'A high-performance rendering engine replacement.', 1_574_609),
  mockMod('lithium', 'Lithium', 'jellysquid3', 118_204_113, 'General-purpose game logic optimisation.', 640_000),
  mockMod('iris', 'Iris Shaders', 'coderbot', 195_069_093, 'A modern shaders mod compatible with Sodium.', 2_100_000, ['sodium']),
  mockMod('modmenu', 'Mod Menu', 'Prospector', 140_551_000, 'Adds a screen listing your installed mods.', 520_000, ['fabric-api']),
  mockMod('journeymap', 'JourneyMap', 'techbrew', 96_300_000, 'Real-time mapping in game or in a web browser.', 3_800_000, ['fabric-api']),
  mockMod('old-mod', 'Old Mod (示例：没有适配)', 'someone', 1200, 'A mod that has no build for this version.', 100_000, [], true),
]
const MOCK_SHADERS: MockMod[] = [
  mockMod('complementary-reimagined', 'Complementary Reimagined', 'EminGT', 9_100_000, 'A customisable, good-looking and performant shader.', 1_200_000),
  mockMod('bsl-shaders', 'BSL Shaders', 'CaptTatsu', 5_300_000, 'Classic realistic shader with soft shadows.', 2_800_000),
  mockMod('euphoria-patches', 'Euphoria Patches', 'SpacEagle17', 3_900_000, 'Colorful lights and fancy clouds for Complementary.', 900_000),
]
const MOCK_PACKS: MockMod[] = [
  mockMod('faithful-32x', 'Faithful 32x', 'Faithful Team', 7_600_000, 'The classic look at twice the resolution.', 6_100_000),
  mockMod('bare-bones', 'Bare Bones', 'Bare Bones Team', 4_100_000, 'A minimal, flat-coloured pack.', 400_000),
  mockMod('default-dark-mode', 'Default Dark Mode', 'Dragon', 2_800_000, 'A dark theme for the game’s interface.', 300_000),
]
const MOCK_PACKS_CATALOG: MockMod[] = [
  mockMod('fabulously-optimized', 'Fabulously Optimized', 'Fabulously Team', 12_000_000, 'Improve graphics and performance in Minecraft.', 177_589),
  mockMod('cobblemon-fabric', 'Cobblemon [Fabric]', 'Cobblemon Team', 8_400_000, 'Catch and battle Pokémon in Minecraft.', 900_000),
  mockMod('adrenaline', 'Adrenaline', 'Aeriae', 1_300_000, 'A lightweight performance pack that stays vanilla-like.', 120_000),
]
const catalogFor = (kind: ContentKind) => (kind === 'mod' ? MOCK_CATALOG : kind === 'shader' ? MOCK_SHADERS : MOCK_PACKS)
let mockContent: Record<string, InstalledMod[]> = {}
let mockMods: Record<string, InstalledMod[]> = {
  'fabric-1-21-1': [
    {
      filename: 'fabric-api-0.116.18+1.21.1.jar',
      enabled: true,
      size: 2_340_000,
      project_id: 'fabric-api',
      title: 'Fabric API',
      version_number: '0.116.18+1.21.1',
      icon_url: null,
      user_installed: false, mod_id: null, duplicate: false,
    },
  ],
}

/** The mutable list behind (instance, kind) in the browser mock. */
function contentStore(id: string, kind: ContentKind): InstalledMod[] {
  if (kind === 'mod') return (mockMods[id] ??= [])
  return (mockContent[`${id}:${kind}`] ??= [])
}

async function mockRun(on: EventHandler, launch: boolean) {
  on({ type: 'stage', label: '检查游戏文件' })
  const stages = ['游戏文件', '游戏资源', 'Java 运行时']
  for (const stage of stages) {
    const total = stage === '游戏资源' ? 3890 : stage === '游戏文件' ? 72 : 402
    const bytes = stage === '游戏资源' ? 790e6 : stage === '游戏文件' ? 89e6 : 94e6
    for (let i = 0; i <= 20; i++) {
      on({
        type: 'progress',
        stage,
        files_done: Math.round((total * i) / 20),
        files_total: total,
        bytes_done: Math.round((bytes * i) / 20),
        bytes_total: bytes,
      })
      await sleep(70)
    }
  }
  if (!launch) return
  on({ type: 'stage', label: '启动游戏（Steve）' })
  await sleep(500)
  on({ type: 'started', pid: 12345 })
  setTimeout(() => on({ type: 'ready' }), 3000)
  const lines = [
    '[07:34:23] [Datafixer Bootstrap/INFO]: 226 Datafixer optimizations took 264 milliseconds',
    '[07:34:28] [Render thread/INFO]: Setting user: Steve',
    '[07:34:28] [Render thread/INFO]: Backend library: LWJGL version 3.3.3+5',
    '[07:34:30] [Render thread/INFO]: Reloading ResourceManager: vanilla',
    '[07:34:32] [Render thread/WARN]: Missing sound for event: minecraft:item.goat_horn.play',
    '[07:34:33] [Render thread/INFO]: Sound engine started',
    '[07:34:33] [Render thread/INFO]: Created: 1024x512x4 minecraft:textures/atlas/blocks.png-atlas',
    '[07:35:23] [Render thread/INFO]: Loaded 1290 recipes',
    '[07:35:23] [Render thread/ERROR]: Example error line (mock)',
  ]
  for (const line of lines) {
    on({ type: 'log', line })
    await sleep(350)
  }
}

const fakeApi: typeof real = {
  getSnapshot: async () => snap(),
  listVersions: async () => ({
    latest_release: '26.3',
    latest_snapshot: '26.4-snapshot-8',
    versions: MOCK_VERSIONS,
  }),
  addAccount: async (name) => {
    if (!/^[A-Za-z0-9_]{3,16}$/.test(name)) throw new Error('玩家名无效: 3-16 位字母、数字或下划线')
    if (mock.settings.accounts.some((a) => a.name.toLowerCase() === name.toLowerCase()))
      throw new Error('已存在同名账号')
    mock.settings.accounts.push({ name, uuid: mockUuid(name) })
    mock.settings.selected_account = name
    return snap()
  },
  removeAccount: async (name) => {
    mock.settings.accounts = mock.settings.accounts.filter((a) => a.name !== name)
    if (mock.settings.selected_account === name)
      mock.settings.selected_account = mock.settings.accounts[0]?.name ?? null
    return snap()
  },
  selectAccount: async (name) => {
    mock.settings.selected_account = name
    return snap()
  },
  saveSettings: async (s) => {
    mock.settings = { ...mock.settings, ...s, accounts: mock.settings.accounts }
    return snap()
  },
  createInstance: async (name, mcVersion, loader) => {
    const id = `${name.toLowerCase().replace(/[^a-z0-9]+/g, '-') || 'instance'}-${mock.instances.length + 1}`
    mock.instances.push({
      id,
      name,
      version_id: loader ? `${loader.kind}-loader-${loader.version}-${mcVersion}` : mcVersion,
      mc_version: loader ? mcVersion : null,
      loader: loader ?? null,
      memory_mb: null,
      jvm_args: '',
      java_path: null,
      width: null,
      height: null,
      running: null,
    })
    mock.settings.selected_instance = id
    return snap()
  },
  updateInstance: async (i) => {
    mock.instances = mock.instances.map((x) => (x.id === i.id ? { ...x, ...i } : x))
    return snap()
  },
  deleteInstance: async (id) => {
    mock.instances = mock.instances.filter((x) => x.id !== id)
    if (mock.settings.selected_instance === id) mock.settings.selected_instance = null
    return snap()
  },
  selectInstance: async (id) => {
    mock.settings.selected_instance = id
    return snap()
  },
  probeJava: async () => ({ major: 21, version: '21.0.4' }),
  openGameDir: async () => {},
  openDataDir: async () => {},
  cancelTask: async () => {},
  killGame: async () => true,
  installVersion: async (_v, on) => mockRun(on, false),
  launchInstance: async (_id, on) => {
    await mockRun(on, true)
  },
  attachGame: async () => false,
  installInstance: async (_id, on) => mockRun(on, false),
  recommendedMods: async (kind, mc) => {
    await sleep(300)
    if (kind === 'forge') return []
    if (kind === 'neoforge' && ['1.20.1', '1.19.4', '1.18.2'].includes(mc)) return []
    const all: Recommended[] = [
      { slug: 'fabric-api', title: 'Fabric API', why: '绝大多数 Fabric 模组都需要它', default_on: true },
      { slug: 'sodium', title: 'Sodium', why: '大幅提升帧数', default_on: false },
      { slug: 'lithium', title: 'Lithium', why: '优化游戏逻辑，减少卡顿', default_on: false },
      { slug: 'modmenu', title: 'Mod Menu', why: '在游戏里查看和管理已装模组', default_on: false },
      { slug: 'iris', title: 'Iris Shaders', why: '使用光影包（会自动带上 Sodium）', default_on: false },
    ]
    return kind === 'neoforge' ? all.filter((r) => ['sodium', 'lithium', 'iris'].includes(r.slug)) : all
  },
  installExtras: async (id, slugs, on) => {
    on({ type: 'stage', label: '下载模组' })
    for (let i = 0; i <= 6; i++) {
      on({ type: 'progress', stage: '模组', files_done: i, files_total: 6, bytes_done: i * 400_000, bytes_total: 2_400_000 })
      await sleep(100)
    }
    const list = mockMods[id] ?? []
    for (const slug of slugs) {
      const m = MOCK_CATALOG.find((c) => c.slug === slug)
      if (m && !list.some((x) => x.project_id === m.project_id))
        list.push({ filename: `${slug}-1.0.0.jar`, enabled: true, size: m.size, project_id: m.project_id, title: m.title, version_number: '1.0.0', icon_url: null, user_installed: true, mod_id: null, duplicate: false })
    }
    mockMods = { ...mockMods, [id]: list }
  },
  openExternal: async (url) => {
    window.open(url, '_blank', 'noopener')
  },
  listLoaderVersions: async (kind, mc) => {
    await sleep(250)
    const old = ['1.12.2', '1.8.9', '1.7.10', '1.16.5'].includes(mc)
    if (kind === 'forge') {
      return [
        { version: '52.1.20', stable: true },
        { version: '52.1.21', stable: true },
        { version: '52.1.0', stable: true },
      ]
    }
    if (kind === 'neoforge') {
      return old || ['1.20.1', '1.19.4', '1.18.2'].includes(mc)
        ? []
        : [
            { version: '21.1.266', stable: true },
            { version: '21.1.172', stable: true },
            { version: '21.1.0-beta', stable: false },
          ]
    }
    if (old) return []
    return kind === 'fabric'
      ? [
          { version: '0.20.1', stable: true },
          { version: '0.20.0', stable: true },
          { version: '0.19.5', stable: true },
          { version: '0.19.0-beta.2', stable: false },
        ]
      : [
          { version: '0.31.0-beta.6', stable: false },
          { version: '0.30.2', stable: true },
          { version: '0.29.1', stable: true },
        ]
  },
  contentSearch: async (_id, kind, query, offset) => {
    await sleep(300)
    const q = query.trim().toLowerCase()
    const hits = catalogFor(kind).filter((m) => !q || m.title.toLowerCase().includes(q) || m.slug.includes(q))
    return { hits: hits.slice(offset, offset + 20), total: hits.length }
  },
  contentSearchAll: async (kind, query, offset) => {
    await sleep(300)
    const q = query.trim().toLowerCase()
    const hits = catalogFor(kind).filter((m) => !q || m.title.toLowerCase().includes(q) || m.slug.includes(q))
    return { hits: hits.slice(offset, offset + 20), total: hits.length }
  },
  contentDetail: async (projectId, kind, instanceId) => {
    await sleep(400)
    const hit = catalogFor(kind).find((m) => m.project_id === projectId)
    if (!hit) throw new Error('Modrinth 请求失败: HTTP 404 Not Found')
    const have = new Set((instanceId ? mockMods[instanceId] ?? [] : []).map((m) => m.project_id))
    return {
      id: hit.project_id, slug: hit.slug, title: hit.title, description: hit.description, icon_url: hit.icon_url,
      body: `# ${hit.title}\n\n${hit.description}\n\n## 特性\n\n- 更快的渲染\n- 更少的卡顿\n- 兼容大部分模组\n\n> 这是浏览器预览用的假数据。`,
      gallery: [], categories: ['optimization', 'utility'], downloads: hit.downloads, followers: 1234, license: 'MIT',
      source_url: 'https://github.com/example/mod', issues_url: null, wiki_url: null,
      loaders: ['fabric', 'quilt', 'neoforge'], game_versions: ['1.20.1', '1.21.1', '1.21.4'],
      compatible: instanceId ? !hit.unsupported : null,
      versions: [
        { id: 'v2', version_number: '1.2.0', version_type: 'release', game_versions: ['1.21.1'], loaders: ['fabric'], date_published: '2025-03-01T00:00:00Z', downloads: 5000 },
        { id: 'v1', version_number: '1.1.0', version_type: 'beta', game_versions: ['1.21.1'], loaders: ['fabric'], date_published: '2025-01-01T00:00:00Z', downloads: 1200 },
      ],
      dependencies: [{ project_id: 'fabric-api', title: 'Fabric API', icon_url: null, dependency_type: 'required', installed: have.has('fabric-api') }],
    }
  },
  contentPlanMissing: async (_id, projectIds) => {
    await sleep(300)
    return {
      files: projectIds.map((p) => ({ project_id: p, version_id: 'v', title: p, version_number: '1.0', icon_url: null, filename: `${p}.jar`, url: 'https://cdn.modrinth.com/x.jar', sha1: 'd'.repeat(40), size: 2_000_000, required_by: null, user_selected: false, dest: 'mod' as const })),
      total_bytes: projectIds.length * 2_000_000,
      warnings: [],
    }
  },
  modsDisableDuplicates: async () => [],
  precheck: async () => [],
  worldsList: async (id) => {
    await sleep(350)
    const d = (n: number) => Date.now() - n * 86_400_000
    return id === 'fabric-1-21-1'
      ? [
          { dir: 'Redstone Base', name: '红石基地', last_played: d(0.2), game_mode: 'creative', hardcore: false, version: '1.21.1', size: 412_000_000, play_secs: 41_400, icon: null },
          { dir: 'New World', name: '生存世界', last_played: d(2), game_mode: 'survival', hardcore: false, version: '1.21.1', size: 188_000_000, play_secs: 129_600, icon: null },
          { dir: 'Hardcore', name: '极限挑战', last_played: d(9), game_mode: 'survival', hardcore: true, version: '1.21.1', size: 96_000_000, play_secs: 7_800, icon: null },
          { dir: 'Old Map', name: 'Old Map', last_played: null, game_mode: null, hardcore: false, version: null, size: 3_100_000, play_secs: 0, icon: null },
        ]
      : []
  },
  cloneInstance: async (id, name) => {
    await sleep(700)
    const src = mock.instances.find((i) => i.id === id)!
    mock.instances = [...mock.instances, { ...structuredClone(src), id: `${id}-copy`, name, running: null }]
    return structuredClone(mock)
  },
  exportInstancePack: async () => {
    await sleep(1100)
    return { path: 'C:/Users/<user>/Downloads/pack-1.0.0.mrpack', folder: 'Downloads', linked: 18, bundled: 2, size: 4_200_000 }
  },
  checkUpdate: async (manual) => {
    await sleep(600)
    return { configured: true, current: '0.1.0', update: manual ? null : { version: '0.2.0', notes: '## 这次更新\n- 新增：存档备份与回退\n- 新增：服务器列表\n- 修复：若干崩溃问题', url: 'https://github.com/acme/voxel/releases/tag/v0.2.0', installer_url: null } }
  },
  ignoreUpdate: async () => {},
  exportDiagnostics: async () => {
    await sleep(800)
    return { path: 'C:/Users/<user>/Downloads/voxel-diagnostics-20261115-120000.zip', folder: 'Downloads' }
  },
  javaCheck: async () => ({ needed: false, major: 21, component: 'java-runtime-delta' }),
  installJava: async (_id, on) => mockRun(on, false),
  networkTest: async () => {
    await sleep(900)
    return [
      { name: 'BMCLAPI 镜像', url: 'https://bmclapi2.bangbang93.com/', ok: true, status: 200, ms: 48, error: null },
      { name: 'Mojang 官方', url: 'https://piston-meta.mojang.com/', ok: true, status: 200, ms: 412, error: null },
      { name: 'Modrinth 接口', url: 'https://api.modrinth.com/v2/', ok: true, status: 200, ms: 133, error: null },
      { name: 'Modrinth 下载', url: 'https://cdn.modrinth.com/', ok: false, status: null, ms: null, error: '连接超时' },
    ]
  },
  serverAdd: async (name, address) => {
    const [host, port] = address.includes(':') ? address.split(':') : [address, '25565']
    mock.settings.servers = [...mock.settings.servers, { id: `srv-${Date.now()}`, name, host, port: Number(port) }]
    return structuredClone(mock)
  },
  serverRemove: async (id) => {
    mock.settings.servers = mock.settings.servers.filter((s) => s.id !== id)
    return structuredClone(mock)
  },
  serverPing: async (id) => {
    await sleep(500 + Math.random() * 900)
    if (id === 'srv-2') throw new Error('连接超时，服务器可能已离线')
    return { motd: '欢迎来到示例服务器！生存 · 红石 · 建筑', online: 12, max: 100, version: 'Paper 1.21.1', protocol: 767, latency_ms: 38, favicon: null }
  },
  screenshotsList: async (id) => {
    await sleep(300)
    return id === 'fabric-1-21-1'
      ? Array.from({ length: 9 }, (_, i) => ({ file: `2026-11-${String(15 - i).padStart(2, '0')}_18.${20 + i}.05.png`, taken: Date.now() - i * 86_400_000 * 1.3, size: 1_400_000 + i * 90_000 }))
      : []
  },
  screenshotImage: async (_id, file) => {
    // A pixel-art gradient stands in for a real picture in browser previews.
    const c = document.createElement('canvas')
    c.width = 160
    c.height = 90
    const g = c.getContext('2d')!
    const hue = [...file].reduce((n, ch) => n + ch.charCodeAt(0), 0) % 360
    for (let y = 0; y < 90; y += 6) for (let x = 0; x < 160; x += 6) {
      g.fillStyle = `hsl(${hue + x / 8}, 55%, ${28 + (y / 90) * 32 + ((x / 6 + y / 6) % 2) * 3}%)`
      g.fillRect(x, y, 6, 6)
    }
    return c.toDataURL('image/png')
  },
  screenshotDelete: async () => {},
  screenshotsOpen: async () => {},
  worldDelete: async () => {},
  worldOpen: async () => {},
  backupList: async () => {
    await sleep(250)
    const t = (h: number) => Date.now() - h * 3_600_000
    return [
      { id: '3-auto', created: t(0.5), kind: 'auto', scope: 'worlds', reason: '启动前自动备份', size: 214_000_000, worlds: ['Redstone Base', 'New World', 'Hardcore'] },
      { id: '2-safety', created: t(5), kind: 'safety', scope: 'worlds', reason: '回退前的状态', size: 208_000_000, worlds: ['Redstone Base', 'New World'] },
      { id: '1-manual', created: t(52), kind: 'manual', scope: 'full', reason: '更新模组前', size: 640_000_000, worlds: ['New World'] },
    ]
  },
  backupCreate: async () => null,
  backupRestore: async () => {
    await sleep(600)
  },
  backupDelete: async () => {},
  statsSessions: async () => {
    // A few weeks of plausible play, repeatable between reloads.
    const out: PlaySession[] = []
    let seed = 7
    const rnd = () => ((seed = (seed * 16807) % 2147483647) / 2147483647)
    const now = Math.floor(Date.now() / 1000)
    for (let day = 40; day >= 0; day--) {
      if (rnd() < 0.35) continue
      const n = 1 + Math.floor(rnd() * 2)
      for (let k = 0; k < n; k++) {
        out.push({ instance_id: rnd() < 0.7 ? 'fabric-1-21-1' : 'survival-1-21-1', start: now - day * 86400 - Math.floor(rnd() * 40000), secs: Math.floor(900 + rnd() * rnd() * 14000), crashed: rnd() < 0.06 })
      }
    }
    return out
  },
  statsWorlds: async () => [
    { instance_id: 'fabric-1-21-1', dir: 'New World', name: '生存世界', play_secs: 129_600 },
    { instance_id: 'fabric-1-21-1', dir: 'Redstone Base', name: '红石基地', play_secs: 41_400 },
    { instance_id: 'fabric-1-21-1', dir: 'Hardcore', name: '极限挑战', play_secs: 7_800 },
  ],
  contentPlan: async (id, kind, projectId) => {
    await sleep(350)
    const have = new Set(contentStore(id, kind).map((m) => m.project_id))
    const haveMods = new Set((mockMods[id] ?? []).map((m) => m.project_id))
    const picked = catalogFor(kind).find((m) => m.project_id === projectId)
    if (!picked) throw new Error('Modrinth 请求失败: HTTP 404 Not Found')
    if (picked.unsupported) throw new Error(`「${picked.title}」没有适配 1.21.1 + fabric 的版本`)
    if (have.has(projectId)) throw new Error(`「${picked.title}」已安装`)
    const file = (m: MockMod, by: string | null, dest: ContentKind): PlanFile => ({
      project_id: m.project_id,
      version_id: `${m.slug}-v1`,
      title: m.title,
      version_number: '1.0.0',
      icon_url: null,
      filename: `${m.slug}-1.0.0${dest === 'mod' ? '.jar' : '.zip'}`,
      url: `https://cdn.modrinth.com/data/${m.slug}/versions/v1/${m.slug}-1.0.0.${dest === 'mod' ? 'jar' : 'zip'}`,
      sha1: 'a'.repeat(40),
      size: m.size,
      required_by: by,
      user_selected: by === null,
      dest,
    })
    const files = [file(picked, null, kind)]
    if (kind === 'mod') {
      for (const dep of picked.requires) {
        const d = MOCK_CATALOG.find((m) => m.project_id === dep)
        if (d && !haveMods.has(dep)) files.push(file(d, picked.title, 'mod'))
      }
    } else if (kind === 'shader' && !haveMods.has('iris')) {
      // Shaders need Iris (and Sodium) on the instance.
      const iris = MOCK_CATALOG.find((m) => m.slug === 'iris')!
      const sodium = MOCK_CATALOG.find((m) => m.slug === 'sodium')!
      files.push(file(iris, picked.title, 'mod'))
      if (!haveMods.has('sodium')) files.push(file(sodium, iris.title, 'mod'))
    }
    return { files, total_bytes: files.reduce((n, f) => n + f.size, 0), warnings: picked.slug === 'journeymap' ? ['「JourneyMap」只有 beta 版本，可能不稳定'] : [] }
  },
  modsInstall: async (id, plan, on) => {
    on({ type: 'stage', label: '下载' })
    for (let i = 0; i <= 10; i++) {
      on({
        type: 'progress',
        stage: '模组',
        files_done: Math.round((plan.files.length * i) / 10),
        files_total: plan.files.length,
        bytes_done: Math.round((plan.total_bytes * i) / 10),
        bytes_total: plan.total_bytes,
      })
      await sleep(120)
    }
    for (const f of plan.files) {
      contentStore(id, f.dest).push({
        filename: f.filename,
        enabled: true,
        size: f.size,
        project_id: f.project_id,
        title: f.title,
        version_number: f.version_number,
        icon_url: f.icon_url,
        user_installed: f.user_selected, mod_id: null, duplicate: false,
      })
    }
  },
  contentList: async (id, kind) => structuredClone(contentStore(id, kind)),
  contentScan: async (id, kind) => {
    await sleep(700)
    const first = contentStore(id, kind).find((m) => m.enabled)
    if (!first) return { identified: 0, updates: [], missing: [] }
    const file: PlanFile = {
      project_id: first.project_id ?? 'mock', version_id: 'v-new', title: first.title ?? first.filename, version_number: '9.9.9',
      icon_url: first.icon_url, filename: `new-${first.filename}`, url: 'https://cdn.modrinth.com/data/mock/new.jar',
      sha1: 'c'.repeat(40), size: 1_200_000, required_by: null, user_selected: true, dest: kind,
    }
    return { identified: 0, missing: [{ project_id: "fabric-api", title: "Fabric API", icon_url: null, needed_by: [first.title ?? first.filename] }], updates: [{ filename: first.filename, title: file.title, current_version: first.version_number ?? '?', new_version: '9.9.9', file }] }
  },
  modsSetEnabled: async (id, filename, enabled) => {
    mockMods[id] = (mockMods[id] ?? []).map((m) => (m.filename === filename ? { ...m, enabled } : m))
  },
  modpackSearch: async (query, offset) => {
    await sleep(300)
    const q = query.trim().toLowerCase()
    const hits = MOCK_PACKS_CATALOG.filter((m) => !q || m.title.toLowerCase().includes(q) || m.slug.includes(q))
    return { hits: hits.slice(offset, offset + 20), total: hits.length }
  },
  modpackVersions: async (projectId) => {
    await sleep(350)
    const v = (id: string, num: string, mc: string, type = 'release'): PackVersion => ({
      id: `${projectId}-${id}`,
      name: `${num} for ${mc}`,
      version_number: num,
      game_versions: [mc],
      loaders: ['fabric'],
      version_type: type,
      file: { filename: `${projectId}-${num}.mrpack`, url: `https://cdn.modrinth.com/data/${projectId}/versions/${id}/${projectId}-${num}.mrpack`, sha1: 'b'.repeat(40), size: 177_589, primary: true },
    })
    return [v('3', '6.5.0', '1.21.4'), v('2', '6.5.0', '1.21.1'), v('1', '6.4.0-beta', '1.21.1', 'beta')]
  },
  installModpack: async (name, version, on) => {
    on({ type: 'stage', label: '下载整合包' })
    await sleep(300)
    await mockRun(on, false)
    on({ type: 'stage', label: '安装整合包文件' })
    for (let i = 0; i <= 10; i++) {
      on({ type: 'progress', stage: '整合包文件', files_done: i * 5, files_total: 50, bytes_done: i * 3_000_000, bytes_total: 30_000_000 })
      await sleep(100)
    }
    const mc = version.game_versions[version.game_versions.length - 1]
    const loader = { kind: 'fabric' as const, version: '0.19.3' }
    const id = `${name.toLowerCase().replace(/[^a-z0-9]+/g, '-') || 'pack'}-${mock.instances.length + 1}`
    mock.instances.push({
      id,
      name,
      version_id: `fabric-loader-${loader.version}-${mc}`,
      mc_version: mc,
      loader,
      memory_mb: null,
      jvm_args: '',
      java_path: null,
      width: null,
      height: null,
      running: null,
    })
    mock.settings.selected_instance = id
    mockMods[id] = Array.from({ length: 6 }, (_, i) => ({
      filename: `pack-mod-${i + 1}.jar`,
      enabled: true,
      size: 400_000,
      project_id: null,
      title: null,
      version_number: null,
      icon_url: null,
      user_installed: true, mod_id: null, duplicate: false,
    }))
    return snap()
  },
  contentRemove: async (id, kind, filename) => {
    const rest = contentStore(id, kind).filter((m) => m.filename !== filename)
    if (kind === 'mod') mockMods[id] = rest
    else mockContent[`${id}:${kind}`] = rest
  },
  readTextFile: async () => '---- Minecraft Crash Report ----\n(mock)',
  pickJava: async () => 'C:\\Program Files\\Java\\jdk-21\\bin\\java.exe',
  windowMinimize: async () => {},
  windowToggleMax: async () => {},
  windowClose: async () => {},
}

export const api = isTauri ? real : fakeApi
