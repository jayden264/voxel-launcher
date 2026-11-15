import { useEffect, useState } from 'react'
import { AnimatePresence, motion } from 'motion/react'
import { AlertTriangle, Cpu, FolderOpen, Play, ScrollText, Square, UserRound, X } from 'lucide-react'
import { toast } from 'sonner'
import { api } from '@/lib/api'
import { EASE, rise } from '@/lib/motion'
import { formatBytes, selectedAccount, selectedInstance, useStore } from '@/lib/store'
import { PixelAvatar } from '@/components/art'
import { Bezel, Eyebrow, PillButton } from '@/components/Bezel'
import { ContinueCard, MonthCard } from '@/components/HomeCards'
import { LogDrawer } from '@/components/LogDrawer'
import { RunProgress } from '@/components/RunProgress'
import { Scene, usePointer } from '@/components/Scene'
import { Empty } from '@/components/kit'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { cn } from '@/lib/utils'

function ExitBanner() {
  const exit = useStore((s) => s.lastExit)
  const dismiss = useStore((s) => s.dismissExit)
  const setLogOpen = useStore((s) => s.setLogOpen)
  const [report, setReport] = useState<string | null>(null)
  const snap = useStore((s) => s.snap)
  const launch = useStore((s) => s.launch)
  const updateInstance = useStore((s) => s.updateInstance)
  const setPage = useStore((s) => s.setPage)
  const openDiag = useStore((s) => s.openDiag)
  const [acting, setActing] = useState(false)
  const [rollback, setRollback] = useState<{ id: string; created: number; worlds: number } | null>(null)
  const [confirmRollback, setConfirmRollback] = useState(false)
  const crashedId = exit && exit.code !== 0 ? exit.instanceId : null

  // The backup made just before the crashed launch is the state to go back to.
  useEffect(() => {
    setRollback(null)
    if (!crashedId) return
    let live = true
    api.backupList(crashedId).then(
      (l) => {
        const b = l.find((x) => x.kind === 'auto')
        if (live && b) setRollback({ id: b.id, created: b.created, worlds: b.worlds.length })
      },
      () => {},
    )
    return () => {
      live = false
    }
  }, [crashedId])

  if (!exit || exit.code === 0) return null

  // One button that does what the diagnosis advises, when the launcher can do it itself.
  const inst = snap?.instances.find((i) => i.id === exit.instanceId)
  let action: { label: string; run: () => Promise<void> } | null = null
  const code = exit.diagnosis?.code
  if (inst && snap) {
    if (code === 'out-of-memory') {
      const cur = inst.memory_mb ?? snap.settings.default_memory_mb ?? snap.system.recommended_mem_mb
      const max = Math.max(2048, snap.system.max_mem_mb)
      const next = Math.min(max, Math.round((cur * 1.5) / 512) * 512)
      if (next > cur) {
        action = {
          label: `内存调到 ${(next / 1024).toFixed(1)} GB 并重试`,
          run: async () => {
            if (await updateInstance({ ...inst, memory_mb: next })) await launch(inst.id)
          },
        }
      }
    } else if (code === 'duplicate-mods') {
      action = {
        label: '处理重复并重试',
        run: async () => {
          const off = await api.modsDisableDuplicates(inst.id)
          toast.success(`已禁用 ${off.length} 个旧的重复文件（可随时重新启用）`)
          await launch(inst.id)
        },
      }
    } else if (code === 'corrupt-file') {
      action = { label: '补全文件并重试', run: () => launch(inst.id) }
    } else if (code === 'mod-dependency' || code === 'mixin-conflict' || code === 'mod-version-mismatch' || code === 'mod-crash') {
      action = { label: '去管理模组', run: async () => setPage('mods') }
    }
  }
  const runAction = async () => {
    if (!action) return
    setActing(true)
    try {
      await action.run()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e))
    } finally {
      setActing(false)
    }
  }

  const openReport = async () => {
    if (!exit.crashReport) return
    try {
      setReport(await api.readTextFile(exit.crashReport))
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e))
    }
  }

  return (
    <>
      <motion.div initial={{ opacity: 0, y: -10 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.6, ease: EASE }} className="mb-4">
        <Bezel radius={24} className="!bg-red-500/10">
          <div className="flex items-center gap-3 px-4 py-3 text-sm">
            <AlertTriangle className="size-5 shrink-0 text-red-300" />
            <div className="min-w-0 flex-1">
              <div className="font-medium text-red-200">「{exit.instanceName}」异常退出{exit.code != null && `（退出码 ${exit.code}）`}</div>
              {exit.diagnosis ? (
                <div className="mt-0.5 space-y-0.5 text-red-200/80">
                  <div className="font-medium text-red-100">可能的原因：{exit.diagnosis.title}</div>
                  <div>{exit.diagnosis.advice}</div>
                  {exit.diagnosis.detail && <div className="selectable truncate font-mono text-xs text-red-200/50">{exit.diagnosis.detail}</div>}
                </div>
              ) : (
                <div className="text-red-200/70">{exit.crashReport ? '游戏生成了崩溃报告，可以查看原因。' : '可以展开日志查看最后的输出。'}</div>
              )}
            </div>
            {action && (
              <PillButton size="sm" disabled={acting} onClick={runAction}>
                {action.label}
              </PillButton>
            )}
            {rollback && (
              <PillButton size="sm" tone="soft" disabled={acting} onClick={() => setConfirmRollback(true)}>
                回退存档
              </PillButton>
            )}
            <PillButton size="sm" tone="soft" onClick={() => setLogOpen(true)}>
              查看日志
            </PillButton>
            <PillButton size="sm" tone="soft" onClick={() => openDiag(exit.instanceId)}>
              导出诊断包
            </PillButton>
            {exit.crashReport && (
              <PillButton size="sm" tone="soft" onClick={openReport}>
                崩溃报告
              </PillButton>
            )}
            <button type="button" aria-label="关闭" onClick={dismiss} className="grid size-8 place-items-center rounded-full text-red-200/70 transition-colors hover:bg-white/10">
              <X className="size-4" />
            </button>
          </div>
        </Bezel>
      </motion.div>
      <Dialog open={confirmRollback} onOpenChange={(o) => !o && !acting && setConfirmRollback(false)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>把存档回退到启动前？</DialogTitle>
            <DialogDescription>
              会恢复成启动「{exit.instanceName}」之前的样子（{rollback ? new Date(rollback.created).toLocaleString('zh-CN', { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit', hour12: false }) : ''} 的备份，{rollback?.worlds} 个存档）。这次游戏里的进度会消失，但回退前会先保护备份当前状态，随时可以再回退回来。
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <PillButton tone="ghost" disabled={acting} onClick={() => setConfirmRollback(false)}>
              取消
            </PillButton>
            <PillButton
              disabled={acting}
              onClick={async () => {
                if (!rollback) return
                setActing(true)
                try {
                  await api.backupRestore(exit.instanceId, rollback.id)
                  toast.success('已回退到启动前的存档')
                  setConfirmRollback(false)
                  dismiss()
                } catch (e) {
                  toast.error(e instanceof Error ? e.message : String(e))
                } finally {
                  setActing(false)
                }
              }}
            >
              {acting ? '回退中…' : '回退'}
            </PillButton>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <Dialog open={report !== null} onOpenChange={(o) => !o && setReport(null)}>
        <DialogContent className="max-h-[80vh] sm:max-w-3xl">
          <DialogHeader>
            <DialogTitle>崩溃报告</DialogTitle>
          </DialogHeader>
          <pre className="selectable max-h-[60vh] overflow-auto rounded-lg bg-black/30 p-3 font-mono text-xs leading-5 whitespace-pre-wrap">{report}</pre>
        </DialogContent>
      </Dialog>
    </>
  )
}

/** Title that slides up out of a mask, word by word. */
function RevealTitle({ text }: { text: string }) {
  const words = text.split(/(\s+)/)
  return (
    <h2 className="flex flex-wrap text-[44px] leading-[1.05] font-semibold tracking-tight drop-shadow-[0_6px_24px_oklch(0_0_0/45%)]">
      {words.map((w, i) =>
        /^\s+$/.test(w) ? (
          <span key={i}>&nbsp;</span>
        ) : (
          <span key={i} className="overflow-hidden pb-1">
            <motion.span className="inline-block" initial={{ y: '115%' }} animate={{ y: 0 }} transition={{ duration: 0.85, ease: EASE, delay: 0.12 + i * 0.06 }}>
              {w}
            </motion.span>
          </span>
        ),
      )}
    </h2>
  )
}

export function HomePage() {
  const snap = useStore((s) => s.snap)
  const run = useStore((s) => s.run)
  const launch = useStore((s) => s.launch)
  const cancel = useStore((s) => s.cancel)
  const kill = useStore((s) => s.kill)
  const setPage = useStore((s) => s.setPage)
  const setLogOpen = useStore((s) => s.setLogOpen)
  const logOpen = useStore((s) => s.logOpen)
  const mutate = useStore((s) => s.mutate)
  const pointer = usePointer()

  const inst = selectedInstance(snap)
  const account = selectedAccount(snap)

  if (!snap) return null
  if (!inst) {
    return (
      <Empty
        sprite="chest"
        title="还没有游戏实例"
        hint="去「下载」页挑一个版本，创建你的第一个实例。"
        action={
          <PillButton icon={<Play className="size-4 fill-current" />} onClick={() => setPage('download')}>
            选择版本
          </PillButton>
        }
      />
    )
  }

  const busyHere = run.instanceId === inst.id || run.kind === 'install'
  const working = run.phase === 'working' && busyHere
  const running = run.phase === 'running' && run.instanceId === inst.id
  const otherBusy = run.phase !== 'idle' && !busyHere
  const memory = inst.memory_mb ?? snap.settings.default_memory_mb ?? snap.system.recommended_mem_mb

  const onLaunch = () => {
    if (!account) {
      toast.error('请先添加一个玩家名')
      setPage('accounts')
      return
    }
    launch(inst.id)
  }

  const state = running ? 'running' : working ? 'working' : 'idle'

  return (
    <div className="flex h-full min-h-0 flex-col">
      <ExitBanner />
      <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_304px] gap-5">
        {/* ---- hero ---- */}
        <div className="flex min-h-0 flex-col">
          <motion.div key={inst.id} initial={{ opacity: 0, y: 24, scale: 0.985 }} animate={{ opacity: 1, y: 0, scale: 1 }} transition={{ duration: 0.9, ease: EASE }} className="min-h-[300px] flex-1" onPointerMove={pointer.onMove} onPointerLeave={pointer.onLeave}>
            <Bezel radius={38} className="group h-full" coreClassName="flex flex-col justify-end">
              <Scene seed={inst.id} pointer={pointer} particles={16} className="" />
              <div className="absolute inset-0 bg-gradient-to-t from-[oklch(0.13_0.02_255)]/92 via-[oklch(0.13_0.02_255)]/28 to-transparent" />
              <div className="relative flex items-end justify-between gap-6 p-7">
                <div className="min-w-0">
                  <motion.div variants={rise} initial="hidden" animate="show" custom={0}>
                    <Eyebrow>当前实例</Eyebrow>
                  </motion.div>
                  <div className="mt-3">
                    <RevealTitle text={inst.name} />
                  </div>
                  <motion.div variants={rise} initial="hidden" animate="show" custom={3} className="mt-4 flex flex-wrap items-center gap-2 text-sm">
                    <span className="rounded-full bg-white/12 px-3 py-1 font-medium shadow-[inset_0_1px_0_oklch(1_0_0/12%)]">Minecraft {inst.version_id}</span>
                    <span className="flex items-center gap-1.5 rounded-full bg-white/8 px-3 py-1 text-foreground/70">
                      <Cpu className="size-3.5" />
                      {(memory / 1024).toFixed(memory % 1024 ? 1 : 0)} GB
                    </span>
                    <button type="button" onClick={() => setPage('accounts')} className="flex items-center gap-1.5 rounded-full bg-white/8 py-1 pr-3 pl-1.5 text-foreground/70 transition-colors duration-500 [transition-timing-function:var(--ease)] hover:bg-white/14 hover:text-foreground">
                      {account ? <PixelAvatar seed={account.uuid} size={18} className="rounded-sm" /> : <UserRound className="size-3.5" />}
                      {account?.name ?? '未选择账号'}
                    </button>
                  </motion.div>
                </div>

                <div className="shrink-0">
                  <AnimatePresence mode="wait" initial={false}>
                    <motion.div key={state} initial={{ opacity: 0, y: 10, scale: 0.94 }} animate={{ opacity: 1, y: 0, scale: 1 }} exit={{ opacity: 0, y: -8, scale: 0.96 }} transition={{ duration: 0.4, ease: EASE }} className="flex flex-col items-end gap-2">
                      {state === 'running' ? (
                        <PillButton size="xl" tone="danger" icon={<Square className="size-4 fill-current" />} onClick={() => kill(inst.id)}>
                          结束游戏
                        </PillButton>
                      ) : state === 'working' ? (
                        <PillButton size="xl" tone="soft" icon={<X className="size-5" />} onClick={cancel}>
                          取消
                        </PillButton>
                      ) : (
                        <PillButton size="xl" icon={<Play className="size-5 fill-current" />} disabled={otherBusy} onClick={onLaunch}>
                          启动游戏
                        </PillButton>
                      )}
                      {state === 'running' && <div className="mr-3 text-xs text-brand">{run.ready ? '游戏运行中' : '游戏加载中，窗口马上出现…'}</div>}
                    </motion.div>
                  </AnimatePresence>
                </div>
              </div>
            </Bezel>
          </motion.div>

          <AnimatePresence>
            {(working || (running && !run.ready) || (run.phase === 'working' && run.kind === 'install')) && (
              <motion.div initial={{ opacity: 0, y: 16 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: 8 }} transition={{ duration: 0.5, ease: EASE }} className="mt-4">
                <Bezel radius={26}>
                  <div className="p-4">
                    <RunProgress stage={run.stage} progress={run.progress} />
                  </div>
                </Bezel>
              </motion.div>
            )}
          </AnimatePresence>

          <motion.div variants={rise} initial="hidden" animate="show" custom={5} className="mt-3 flex items-center gap-1.5">
            <PillButton size="sm" tone={logOpen ? 'soft' : 'ghost'} onClick={() => setLogOpen(!logOpen)} icon={<ScrollText className="size-3.5" />}>
              游戏日志
            </PillButton>
            <PillButton size="sm" tone="ghost" icon={<FolderOpen className="size-3.5" />} onClick={() => api.openGameDir(inst.id).catch((e) => toast.error(String(e.message ?? e)))}>
              打开游戏目录
            </PillButton>
            {run.progress && run.phase === 'working' && <span className="ml-auto pr-2 text-xs text-muted-foreground tabular-nums">{run.progress.speed > 0 && `${formatBytes(run.progress.speed)}/s`}</span>}
          </motion.div>
          <LogDrawer />
        </div>

        {/* ---- side ---- */}
        <div className="flex min-h-0 flex-col gap-4 overflow-auto">
          <motion.div variants={rise} initial="hidden" animate="show" custom={2}>
            <Bezel radius={30}>
              <div className="p-4">
                <div className="eyebrow mb-3">切换实例</div>
                <div className="space-y-1">
                  {snap.instances.map((i) => {
                    const active = i.id === inst.id
                    return (
                      <button
                        key={i.id}
                        type="button"
                        disabled={run.phase !== 'idle'}
                        onClick={() => mutate(() => api.selectInstance(i.id))}
                        className={cn('group relative flex w-full items-center gap-3 rounded-2xl p-2 text-left transition-colors duration-500 [transition-timing-function:var(--ease)] disabled:opacity-60', active ? 'bg-brand/12 shadow-[inset_0_0_0_1px_color-mix(in_oklch,var(--brand)_28%,transparent)]' : 'hover:bg-white/6')}
                      >
                        <div className="relative size-10 shrink-0 overflow-hidden rounded-xl">
                          <Scene seed={i.id} glow={false} />
                        </div>
                        <div className="min-w-0 flex-1">
                          <div className="truncate text-sm font-medium">{i.name}</div>
                          <div className="text-xs text-muted-foreground">{i.version_id}</div>
                        </div>
                        {i.running && <span className="size-2 rounded-full bg-brand shadow-[0_0_10px_var(--brand)]" />}
                      </button>
                    )
                  })}
                </div>
                <PillButton size="sm" tone="ghost" className="mt-2 w-full justify-center" onClick={() => setPage('download')}>
                  + 新建实例
                </PillButton>
              </div>
            </Bezel>
          </motion.div>

          <motion.div variants={rise} initial="hidden" animate="show" custom={3}>
            <Bezel radius={30}>
              <div className="p-4">
                <div className="eyebrow mb-3">当前账号</div>
                {account ? (
                  <div className="flex items-center gap-3">
                    <PixelAvatar seed={account.uuid} size={46} className="rounded-xl" />
                    <div className="min-w-0">
                      <div className="truncate font-semibold">{account.name}</div>
                      <div className="text-xs text-muted-foreground">离线账号</div>
                    </div>
                  </div>
                ) : (
                  <PillButton tone="soft" className="w-full justify-center" onClick={() => setPage('accounts')}>
                    添加玩家名
                  </PillButton>
                )}
              </div>
            </Bezel>
          </motion.div>

          <ContinueCard inst={inst} />
          <MonthCard />
        </div>
      </div>
    </div>
  )
}
