import { create } from 'zustand'
import { toast } from 'sonner'
import { api, type UpdateInfo, type QuickPlay, type Diagnosis, type InstallPlan, type Instance, type LaunchEvent, type PackVersion, type Problem, type Settings, type Snapshot, type VersionList } from './api'

export type Page = 'home' | 'instances' | 'download' | 'mods' | 'worlds' | 'accounts' | 'settings'

export interface Progress {
  stage: string
  filesDone: number
  filesTotal: number
  bytesDone: number
  bytesTotal: number
  speed: number // bytes / s
}

export interface RunState {
  phase: 'idle' | 'working' | 'running'
  /** Running only: false while the process is up but the game window is not on screen yet. */
  ready: boolean
  kind: 'launch' | 'install' | null
  instanceId: string | null
  stage: string
  progress: Progress | null
  /** When the game window appeared (ms since the epoch); null before that. */
  since: number | null
}

export interface ExitInfo {
  instanceId: string
  instanceName: string
  code: number | null
  crashReport: string | null
  diagnosis: Diagnosis | null
}

interface Store {
  snap: Snapshot | null
  versions: VersionList | null
  versionsError: string | null
  page: Page
  /** Pages visited before / after this one, for the back and forward buttons. */
  navBack: Page[]
  navForward: Page[]
  run: RunState
  logs: string[]
  logOpen: boolean
  lastExit: ExitInfo | null
  /** Problems found before launching; the user decides whether to go on. */
  preflight: { instanceId: string; problems: Problem[]; quick?: QuickPlay } | null
  /** A Java runtime has to be downloaded first; the user is asked before anything is fetched. */
  javaPrompt: { instanceId: string; major: number; resume: () => void } | null
  /** The "export diagnostics" popup; `instanceId` picks whose logs go in. */
  diag: { instanceId: string | null } | null
  /** A newer launcher version found on GitHub. */
  update: UpdateInfo | null
  /** The first-run guide is showing. */
  onboarding: boolean

  init: () => Promise<void>
  setPage: (p: Page) => void
  goBack: () => void
  goForward: () => void
  setLogOpen: (v: boolean) => void
  dismissExit: () => void
  dismissPreflight: () => void
  dismissJavaPrompt: () => void
  closeUpdate: () => void
  openOnboarding: () => void
  closeOnboarding: () => void
  openDiag: (instanceId?: string | null) => void
  closeDiag: () => void
  confirmJava: () => Promise<void>
  loadVersions: (force?: boolean) => Promise<void>
  mutate: (fn: () => Promise<Snapshot>, okMsg?: string) => Promise<boolean>
  saveSettings: (s: Settings) => Promise<boolean>
  updateInstance: (i: Instance) => Promise<boolean>
  /** `force` skips the pre-launch check (the user chose to go ahead anyway). */
  launch: (id: string, force?: boolean, quick?: QuickPlay) => Promise<void>
  install: (versionId: string) => Promise<boolean>
  /** Download an instance's game files (and loader). */
  installInstance: (id: string, extras?: string[]) => Promise<boolean>
  /** Create an instance from a modpack version (downloads everything). */
  installModpack: (name: string, version: PackVersion) => Promise<boolean>
  /** Download the files of a confirmed mod plan into an instance. */
  modsInstall: (instanceId: string, plan: InstallPlan) => Promise<boolean>
  cancel: () => Promise<void>
  kill: (id: string) => Promise<void>
}

const IDLE: RunState = { phase: 'idle', ready: false, kind: null, instanceId: null, stage: '', progress: null, since: null }
const MAX_LOG_LINES = 3000

function msg(e: unknown) {
  return e instanceof Error ? e.message : String(e)
}

export const useStore = create<Store>((set, get) => {
  let lastSample = { t: 0, bytes: 0, stage: '' }

  function handler(instanceId: string, kind: 'launch' | 'install'): (e: LaunchEvent) => void {
    return (e) => {
      switch (e.type) {
        case 'stage':
          set((s) => ({ run: { ...s.run, phase: s.run.phase === 'running' ? 'running' : 'working', kind, instanceId, stage: e.label } }))
          break
        case 'progress': {
          const now = performance.now()
          let speed = get().run.progress?.speed ?? 0
          if (lastSample.stage !== e.stage) lastSample = { t: now, bytes: e.bytes_done, stage: e.stage }
          else if (now - lastSample.t >= 700) {
            const inst = ((e.bytes_done - lastSample.bytes) / (now - lastSample.t)) * 1000
            speed = speed ? speed * 0.6 + inst * 0.4 : inst
            lastSample = { t: now, bytes: e.bytes_done, stage: e.stage }
          }
          set((s) => ({
            run: {
              ...s.run,
              progress: {
                stage: e.stage,
                filesDone: e.files_done,
                filesTotal: e.files_total,
                bytesDone: e.bytes_done,
                bytesTotal: e.bytes_total,
                speed: Math.max(0, speed),
              },
            },
          }))
          break
        }
        case 'log':
          set((s) => {
            const logs = s.logs.length >= MAX_LOG_LINES ? s.logs.slice(-MAX_LOG_LINES + 200) : s.logs
            return { logs: [...logs, e.line] }
          })
          break
        case 'started':
          set((s) => ({ run: { ...s.run, phase: 'running', ready: false, progress: null, stage: '游戏加载中' } }))
          break
        case 'notice':
          toast.info(e.text, { duration: 7000 })
          break
        case 'ready':
          set((s) => ({ run: { ...s.run, ready: true, stage: '游戏运行中', since: Date.now() } }))
          break
        case 'exited': {
          const inst = get().snap?.instances.find((i) => i.id === instanceId)
          set({
            run: IDLE,
            lastExit: { instanceId, instanceName: inst?.name ?? instanceId, code: e.code, crashReport: e.crash_report, diagnosis: e.diagnosis ?? null },
          })
          api.getSnapshot().then((snap) => set({ snap }))
          break
        }
      }
    }
  }

  /** True when the Java this instance needs is there; otherwise asks the user and returns false. */
  async function ensureJava(id: string, resume: () => void): Promise<boolean> {
    const jc = await api.javaCheck(id).catch(() => null)
    if (jc?.needed) {
      set({ run: IDLE, javaPrompt: { instanceId: id, major: jc.major, resume } })
      return false
    }
    return true
  }

  /** Run a download task as the one visible background job; resolves true on success. */
  async function runInstall(
    stage: string,
    task: (on: (e: LaunchEvent) => void) => Promise<unknown>,
    okMsg: string,
  ): Promise<boolean> {
    if (get().run.phase !== 'idle') return false
    lastSample = { t: 0, bytes: 0, stage: '' }
    set({ run: { phase: 'working', ready: false, kind: 'install', instanceId: null, stage, progress: null, since: null } })
    try {
      await task(handler('', 'install'))
      set({ run: IDLE })
      toast.success(okMsg)
      return true
    } catch (e) {
      set({ run: IDLE })
      const m = msg(e)
      if (m !== '已取消') toast.error(m, { duration: 8000 })
      return false
    }
  }

  return {
    snap: null,
    versions: null,
    versionsError: null,
    page: 'home',
    navBack: [],
    navForward: [],
    run: IDLE,
    logs: [],
    logOpen: false,
    lastExit: null,
    preflight: null,
    javaPrompt: null,
    diag: null,
    update: null,
    onboarding: false,

    async init() {
      try {
        const snap = await api.getSnapshot()
        document.documentElement.dataset.mica = snap.system.mica ? '1' : '0'
        set({ snap })
        // A brand-new setup (no account, no instance, never guided) gets the first-run guide.
        let seen = false
        try {
          seen = localStorage.getItem('voxel.onboarded') === '1'
        } catch {
          /* treat as not seen */
        }
        if (!seen && snap.settings.accounts.length === 0 && snap.instances.length === 0) set({ onboarding: true })
        // Ask GitHub about a newer launcher; failing quietly (offline, rate limit) is fine.
        api.checkUpdate(false).then(
          (r) => r.update && set({ update: r.update }),
          () => {},
        )
        // Re-attach to a game that outlived the previous launcher session.
        const running = snap.instances.find((i) => i.running)
        if (running) {
          const ok = await api.attachGame(running.id, handler(running.id, 'launch'))
          if (ok) set({ run: { ...IDLE, phase: 'running', kind: 'launch', instanceId: running.id, stage: '游戏加载中' } })
        }
      } catch (e) {
        toast.error(`初始化失败: ${msg(e)}`)
      }
    },

    setPage: (page) =>
      set((s) => (page === s.page ? s : { page, navBack: [...s.navBack.slice(-30), s.page], navForward: [] })),
    goBack: () =>
      set((s) => (s.navBack.length === 0 ? s : { page: s.navBack[s.navBack.length - 1], navBack: s.navBack.slice(0, -1), navForward: [s.page, ...s.navForward] })),
    goForward: () =>
      set((s) => (s.navForward.length === 0 ? s : { page: s.navForward[0], navBack: [...s.navBack, s.page], navForward: s.navForward.slice(1) })),
    setLogOpen: (logOpen) => set({ logOpen }),
    dismissExit: () => set({ lastExit: null }),
    dismissPreflight: () => set({ preflight: null }),
    dismissJavaPrompt: () => set({ javaPrompt: null }),
    closeUpdate: () => set({ update: null }),
    openOnboarding: () => set({ onboarding: true }),
    closeOnboarding: () => {
      try {
        localStorage.setItem('voxel.onboarded', '1')
      } catch {
        /* a private window just shows it again next time */
      }
      set({ onboarding: false })
    },
    openDiag: (instanceId = null) => set({ diag: { instanceId } }),
    closeDiag: () => set({ diag: null }),
    async confirmJava() {
      const p = get().javaPrompt
      if (!p) return
      set({ javaPrompt: null })
      const ok = await runInstall(`下载 Java ${p.major}`, (on) => api.installJava(p.instanceId, on), `Java ${p.major} 已就绪`)
      if (ok) p.resume()
    },

    async loadVersions(force) {
      if (get().versions && !force) return
      set({ versionsError: null })
      try {
        set({ versions: await api.listVersions() })
      } catch (e) {
        set({ versionsError: msg(e) })
      }
    },

    async mutate(fn, okMsg) {
      try {
        set({ snap: await fn() })
        if (okMsg) toast.success(okMsg)
        return true
      } catch (e) {
        toast.error(msg(e))
        return false
      }
    },

    saveSettings: (s) => get().mutate(() => api.saveSettings(s)),
    updateInstance: (i) => get().mutate(() => api.updateInstance(i), '已保存'),

    async launch(id, force = false, quick) {
      if (get().run.phase !== 'idle') return
      lastSample = { t: 0, bytes: 0, stage: '' }
      set({ logs: [], lastExit: null, preflight: null, javaPrompt: null, run: { phase: 'working', ready: false, kind: 'launch', instanceId: id, stage: '检查环境', progress: null, since: null } })
      try {
        if (!(await ensureJava(id, () => void get().launch(id, force, quick)))) return
        if (!force) set((s) => ({ run: { ...s.run, stage: '检查模组' } }))
        if (!force) {
          // A failed check must never stop a launch: only real findings do.
          const problems = await api.precheck(id).catch(() => [])
          if (problems.length > 0) {
            set({ run: IDLE, preflight: { instanceId: id, problems, quick } })
            return
          }
        }
        await api.launchInstance(id, handler(id, 'launch'), quick)
        api.getSnapshot().then((snap) => set({ snap }))
      } catch (e) {
        set({ run: IDLE })
        const m = msg(e)
        if (m !== '已取消') toast.error(m, { duration: 8000 })
      }
    },

    install: (versionId) =>
      runInstall(`下载 ${versionId}`, (on) => api.installVersion(versionId, on), `${versionId} 已下载完成`),

    installInstance: async (id, extras = []) => {
      const inst = get().snap?.instances.find((i) => i.id === id)
      const label = inst?.version_id ?? id
      const more = extras.length > 0 ? `，并装好 ${extras.length} 个推荐模组` : ''
      if (get().run.phase !== 'idle') return false
      set({ run: { phase: 'working', ready: false, kind: 'install', instanceId: null, stage: '检查环境', progress: null, since: null } })
      if (!(await ensureJava(id, () => void get().installInstance(id, extras)))) return false
      set({ run: IDLE })
      return runInstall(
        `下载 ${label}`,
        async (on) => {
          await api.installInstance(id, on)
          if (extras.length > 0) await api.installExtras(id, extras, on)
        },
        `${inst?.name ?? label} 已下载完成${more}`,
      )
    },

    installModpack: (name, version) =>
      runInstall(
        '下载整合包',
        async (on) => {
          set({ snap: await api.installModpack(name, version, on) })
        },
        `整合包「${name}」已安装完成`,
      ),

    modsInstall: (instanceId, plan) => {
      const n = plan.files.length
      return runInstall('下载模组', (on) => api.modsInstall(instanceId, plan, on), `已安装 ${n} 个文件`)
    },

    async cancel() {
      await api.cancelTask()
    },

    async kill(id) {
      try {
        await api.killGame(id)
      } catch (e) {
        toast.error(msg(e))
      }
    },
  }
})

export function selectedInstance(snap: Snapshot | null): Instance | null {
  if (!snap) return null
  return (
    snap.instances.find((i) => i.id === snap.settings.selected_instance) ?? snap.instances[0] ?? null
  )
}

export function selectedAccount(snap: Snapshot | null) {
  if (!snap) return null
  return snap.settings.accounts.find((a) => a.name === snap.settings.selected_account) ?? snap.settings.accounts[0] ?? null
}

export function formatBytes(n: number) {
  if (n >= 1e9) return `${(n / 1e9).toFixed(2)} GB`
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)} MB`
  if (n >= 1e3) return `${(n / 1e3).toFixed(0)} KB`
  return `${n} B`
}
