import { useCallback, useEffect, useMemo, useState } from 'react'
import { motion } from 'motion/react'
import { Archive, FolderOpen, HardDrive, Play, Search, ShieldCheck, Trash2, Undo2 } from 'lucide-react'
import { toast } from 'sonner'
import { api, type BackupInfo, type Instance, type PlaySession, type World, type WorldStat } from '@/lib/api'
import { fmtAgo, fmtDateTime, fmtDuration, fmtHours } from '@/lib/format'
import { rise } from '@/lib/motion'
import { formatBytes, selectedInstance, useStore } from '@/lib/store'
import { PixelSprite } from '@/components/art'
import { Bezel, PillButton } from '@/components/Bezel'
import { Chip, Empty, LOADER_LABEL, PageBanner, Segmented } from '@/components/kit'
import { ServersTab, ShotsTab } from '@/components/PlayTabs'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'

type Tab = 'worlds' | 'servers' | 'shots' | 'backups' | 'stats'

/** One-click "enter this world" needs the game's quick-play, which arrived in Minecraft 1.20. */
function canQuickPlay(mc: string) {
  const m = /^1\.(\d+)/.exec(mc)
  return m ? Number(m[1]) >= 20 : true
}

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e))
const MODE: Record<string, string> = { survival: '生存', creative: '创造', adventure: '冒险', spectator: '旁观' }
const KIND_LABEL = { auto: '启动前自动', manual: '手动', safety: '回退前保护' } as const

function instanceKind(i: Instance) {
  return i.loader ? LOADER_LABEL[i.loader.kind] : '原版'
}

/** One chip per instance (and optionally "全部"): which instance the page is about. */
function InstanceChips({ instances, value, onChange, allowAll }: { instances: Instance[]; value: string; onChange: (v: string) => void; allowAll?: boolean }) {
  const chip = (id: string, label: string, sub?: string) => {
    const active = value === id
    return (
      <button
        key={id}
        type="button"
        onClick={() => onChange(id)}
        className={`flex shrink-0 items-center gap-2 rounded-full px-3.5 py-1.5 text-sm transition-[background,color] duration-500 [transition-timing-function:var(--ease)] ${active ? 'bg-brand/16 text-foreground shadow-[inset_0_0_0_1px_color-mix(in_oklch,var(--brand)_40%,transparent)]' : 'bg-white/6 text-muted-foreground hover:bg-white/10 hover:text-foreground'}`}
      >
        <span className="max-w-[160px] truncate font-medium">{label}</span>
        {sub && <span className={`rounded-full px-1.5 py-px text-[10px] font-medium ${active ? 'bg-brand/20 text-brand' : 'bg-white/8'}`}>{sub}</span>}
      </button>
    )
  }
  return (
    <div className="mb-4 flex shrink-0 items-center gap-2 overflow-x-auto pb-1" role="tablist" aria-label="选择实例">
      {allowAll && chip('all', '全部实例')}
      {instances.map((i) => chip(i.id, i.name, `${instanceKind(i)} · ${i.mc_version ?? i.version_id}`))}
    </div>
  )
}

function Skeleton({ n = 4, h = 96 }: { n?: number; h?: number }) {
  return (
    <div className="space-y-2">
      {Array.from({ length: n }, (_, i) => (
        <div key={i} className="animate-pulse rounded-[24px] bg-white/5" style={{ height: h, animationDelay: `${i * 90}ms` }} />
      ))}
    </div>
  )
}

// ------------------------------------------------------------------- worlds

type Sort = 'recent' | 'time' | 'size'

function WorldCard({ w, idx, locked, playable, playHint, onPlay, onOpen, onRestore, onDelete }: { w: World; idx: number; locked: boolean; playable: boolean; playHint?: string; onPlay: () => void; onOpen: () => void; onRestore: () => void; onDelete: () => void }) {
  const iconBtn = 'grid size-9 place-items-center rounded-full text-muted-foreground transition-[background,color,transform] duration-500 [transition-timing-function:var(--ease)] hover:bg-white/10 hover:text-foreground active:scale-90 disabled:cursor-not-allowed disabled:opacity-40'
  return (
    <motion.div variants={rise} initial="hidden" animate="show" custom={idx % 8}>
      <Bezel radius={26}>
        <div className="p-4">
          <div className="flex items-start gap-3.5">
            <div className="grid size-16 shrink-0 place-items-center overflow-hidden rounded-2xl bg-black/30">
              {w.icon ? <img src={w.icon} alt="" className="size-full object-cover [image-rendering:pixelated]" /> : <PixelSprite name="grass" size={40} />}
            </div>
            <div className="min-w-0 flex-1">
              <div className="truncate text-base font-semibold" title={w.name}>
                {w.name}
              </div>
              {w.name !== w.dir && <div className="truncate text-xs text-muted-foreground">{w.dir}</div>}
              <div className="mt-1.5 flex flex-wrap gap-1.5">
                {w.hardcore ? <Chip tone="bad">极限</Chip> : w.game_mode && <Chip tone={w.game_mode === 'creative' ? 'brand' : 'plain'}>{MODE[w.game_mode]}</Chip>}
                {w.version && <Chip>{w.version}</Chip>}
              </div>
            </div>
          </div>
          <div className="mt-3.5 grid grid-cols-3 gap-2 text-xs">
            <div className="rounded-xl bg-black/20 px-2.5 py-1.5">
              <div className="text-muted-foreground">上次游玩</div>
              <div className="mt-0.5 font-medium tabular-nums">{w.last_played ? fmtAgo(w.last_played) : '—'}</div>
            </div>
            <div className="rounded-xl bg-black/20 px-2.5 py-1.5">
              <div className="text-muted-foreground">游玩时长</div>
              <div className="mt-0.5 font-medium tabular-nums">{w.play_secs > 0 ? fmtDuration(w.play_secs) : '—'}</div>
            </div>
            <div className="rounded-xl bg-black/20 px-2.5 py-1.5">
              <div className="text-muted-foreground">占用空间</div>
              <div className="mt-0.5 font-medium tabular-nums">{formatBytes(w.size)}</div>
            </div>
          </div>
          <div className="mt-2.5 flex items-center justify-end gap-0.5">
            <PillButton size="sm" className="mr-auto" disabled={!playable} title={playHint} icon={<Play className="size-3.5 fill-current" />} onClick={onPlay}>
              进入
            </PillButton>
            <button type="button" aria-label="打开存档文件夹" title="打开文件夹" className={iconBtn} onClick={onOpen}>
              <FolderOpen className="size-4" />
            </button>
            <button type="button" aria-label="从备份回退这个存档" title="从备份回退" disabled={locked} className={iconBtn} onClick={onRestore}>
              <Undo2 className="size-4" />
            </button>
            <button type="button" aria-label="删除存档" title="删除" disabled={locked} className={`${iconBtn} hover:!bg-red-500/15 hover:!text-red-300`} onClick={onDelete}>
              <Trash2 className="size-4" />
            </button>
          </div>
        </div>
      </Bezel>
    </motion.div>
  )
}

/** Pick one backup that holds `world` and put just that world back. */
function WorldRestoreDialog({ inst, world, onClose, onDone }: { inst: Instance; world: World; onClose: () => void; onDone: () => void }) {
  const [backups, setBackups] = useState<BackupInfo[] | null>(null)
  const [busy, setBusy] = useState<string | null>(null)
  useEffect(() => {
    api.backupList(inst.id).then(
      (l) => setBackups(l.filter((b) => b.worlds.includes(world.dir))),
      (e) => {
        toast.error(msg(e))
        setBackups([])
      },
    )
  }, [inst.id, world.dir])

  const restore = async (b: BackupInfo) => {
    setBusy(b.id)
    try {
      await api.backupRestore(inst.id, b.id, world.dir)
      toast.success(`「${world.name}」已回退到 ${fmtDateTime(b.created)} 的样子`)
      onDone()
      onClose()
    } catch (e) {
      toast.error(msg(e))
    } finally {
      setBusy(null)
    }
  }

  return (
    <Dialog open onOpenChange={(o) => !o && !busy && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>把「{world.name}」回退到哪个时间点？</DialogTitle>
          <DialogDescription>只会替换这一个存档，其他存档不受影响。回退前会先给现在的状态做一份保护备份，随时可以再回退回来。</DialogDescription>
        </DialogHeader>
        {backups === null ? (
          <div className="py-6 text-center text-sm text-muted-foreground">正在读取备份…</div>
        ) : backups.length === 0 ? (
          <div className="py-6 text-center text-sm text-muted-foreground">还没有包含这个存档的备份。下次启动游戏前会自动备份。</div>
        ) : (
          <ul className="max-h-72 space-y-1.5 overflow-auto">
            {backups.map((b) => (
              <li key={b.id} className="flex items-center gap-3 rounded-2xl bg-black/20 px-3 py-2.5 text-sm">
                <div className="min-w-0 flex-1">
                  <div className="font-medium">{fmtDateTime(b.created)}</div>
                  <div className="text-xs text-muted-foreground">
                    {fmtAgo(b.created)} · {b.reason}
                  </div>
                </div>
                <PillButton size="sm" tone="soft" disabled={busy !== null} onClick={() => restore(b)}>
                  {busy === b.id ? '回退中…' : '回退到这里'}
                </PillButton>
              </li>
            ))}
          </ul>
        )}
        <DialogFooter>
          <PillButton tone="ghost" disabled={busy !== null} onClick={onClose}>
            关闭
          </PillButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function WorldsTab({ inst, rev, bump }: { inst: Instance; rev: number; bump: () => void }) {
  const [worlds, setWorlds] = useState<World[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [query, setQuery] = useState('')
  const [sort, setSort] = useState<Sort>('recent')
  const [deleting, setDeleting] = useState<World | null>(null)
  const [restoring, setRestoring] = useState<World | null>(null)
  const [busy, setBusy] = useState(false)
  const locked = !!inst.running
  const idle = useStore((s) => s.run.phase === 'idle')
  const launch = useStore((s) => s.launch)
  const setPage = useStore((s) => s.setPage)
  const quickOk = canQuickPlay(inst.mc_version ?? inst.version_id)

  useEffect(() => {
    let live = true
    setWorlds(null)
    setError(null)
    api.worldsList(inst.id).then(
      (l) => live && setWorlds(l),
      (e) => live && setError(msg(e)),
    )
    return () => {
      live = false
    }
  }, [inst.id, rev])

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase()
    const list = (worlds ?? []).filter((w) => !q || `${w.name} ${w.dir}`.toLowerCase().includes(q))
    const by = { recent: (w: World) => w.last_played ?? 0, time: (w: World) => w.play_secs, size: (w: World) => w.size }[sort]
    return [...list].sort((a, b) => by(b) - by(a))
  }, [worlds, query, sort])

  if (error) return <Empty sprite="tnt" title="读取存档失败" hint={error} action={<PillButton onClick={bump}>重试</PillButton>} />
  if (worlds === null) return <Skeleton n={3} h={190} />
  if (worlds.length === 0) {
    return <Empty sprite="grass" title="这个实例还没有存档" hint="进游戏创建一个世界后，这里会出现它。" action={<PillButton onClick={() => api.openGameDir(inst.id).catch((e) => toast.error(msg(e)))}>打开游戏目录</PillButton>} />
  }

  const total = worlds.reduce((n, w) => n + w.size, 0)
  return (
    <div className="pb-4">
      <div className="mb-4 flex flex-wrap items-center gap-2.5">
        <div className="relative min-w-[200px] max-w-xs flex-1">
          <Search className="pointer-events-none absolute top-1/2 left-3.5 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="按名称查找存档" className="pl-10" />
        </div>
        <Segmented<Sort>
          value={sort}
          onChange={setSort}
          options={[
            { value: 'recent', label: '最近游玩' },
            { value: 'time', label: '游玩时长' },
            { value: 'size', label: '占用空间' },
          ]}
        />
        <div className="ml-auto text-xs text-muted-foreground tabular-nums">
          {worlds.length} 个存档 · 共 {formatBytes(total)}
        </div>
      </div>
      {locked && <p className="mb-3 px-1 text-xs text-amber-300">游戏正在运行，删除和回退需要先关闭游戏。</p>}
      {shown.length === 0 ? (
        <div className="py-10 text-center text-sm text-muted-foreground">没有匹配「{query}」的存档</div>
      ) : (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(330px,1fr))] gap-3">
          {shown.map((w, i) => (
            <WorldCard
              key={w.dir}
              w={w}
              idx={i}
              locked={locked}
              playable={idle && !locked && quickOk}
              playHint={!quickOk ? '这个版本不支持直接进入存档（需要 Minecraft 1.20 或更新）' : undefined}
              onPlay={() => {
                launch(inst.id, false, { kind: 'world', name: w.dir })
                setPage('home')
              }}
              onOpen={() => api.worldOpen(inst.id, w.dir).catch((e) => toast.error(msg(e)))} onRestore={() => setRestoring(w)} onDelete={() => setDeleting(w)} />
          ))}
        </div>
      )}

      <Dialog open={deleting !== null} onOpenChange={(o) => !o && !busy && setDeleting(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>删除「{deleting?.name}」？</DialogTitle>
            <DialogDescription>删除前会先自动备份这个存档，之后可以在「备份」页找回。</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <PillButton tone="ghost" disabled={busy} onClick={() => setDeleting(null)}>
              取消
            </PillButton>
            <PillButton
              tone="danger"
              disabled={busy}
              onClick={async () => {
                const w = deleting
                if (!w) return
                setBusy(true)
                try {
                  await api.worldDelete(inst.id, w.dir)
                  toast.success(`已删除「${w.name}」，备份里可以找回`)
                  setDeleting(null)
                  bump()
                } catch (e) {
                  toast.error(msg(e))
                } finally {
                  setBusy(false)
                }
              }}
            >
              {busy ? '处理中…' : '删除'}
            </PillButton>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      {restoring && <WorldRestoreDialog inst={inst} world={restoring} onClose={() => setRestoring(null)} onDone={bump} />}
    </div>
  )
}

// ------------------------------------------------------------------ backups

function RestoreDialog({ inst, backup, onClose, onDone }: { inst: Instance; backup: BackupInfo; onClose: () => void; onDone: () => void }) {
  const [busy, setBusy] = useState<string | null>(null)
  const run = async (world?: string) => {
    setBusy(world ?? '*')
    try {
      await api.backupRestore(inst.id, backup.id, world)
      toast.success(world ? `「${world}」已回退` : '已回退到这个备份，回退前的状态也已保存')
      onDone()
      onClose()
    } catch (e) {
      toast.error(msg(e))
    } finally {
      setBusy(null)
    }
  }
  return (
    <Dialog open onOpenChange={(o) => !o && !busy && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>回退到 {fmtDateTime(backup.created)}？</DialogTitle>
          <DialogDescription>
            会把存档、设置{backup.scope === 'full' ? '和模组' : ''}恢复成当时的样子，之后新建的存档会被移走。回退前会自动给现在的状态做一份保护备份，随时可以再回退回来。
          </DialogDescription>
        </DialogHeader>
        {backup.worlds.length > 0 && (
          <div>
            <div className="mb-1.5 px-1 text-xs text-muted-foreground">或者只回退其中一个存档：</div>
            <ul className="max-h-44 space-y-1.5 overflow-auto">
              {backup.worlds.map((w) => (
                <li key={w} className="flex items-center gap-3 rounded-2xl bg-black/20 px-3 py-2 text-sm">
                  <span className="min-w-0 flex-1 truncate">{w}</span>
                  <PillButton size="sm" tone="soft" disabled={busy !== null} onClick={() => run(w)}>
                    {busy === w ? '回退中…' : '只回退它'}
                  </PillButton>
                </li>
              ))}
            </ul>
          </div>
        )}
        <DialogFooter>
          <PillButton tone="ghost" disabled={busy !== null} onClick={onClose}>
            取消
          </PillButton>
          <PillButton disabled={busy !== null} icon={<Undo2 className="size-4" />} onClick={() => run()}>
            {busy === '*' ? '回退中…' : '全部回退'}
          </PillButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function BackupsTab({ inst, rev, bump }: { inst: Instance; rev: number; bump: () => void }) {
  const [backups, setBackups] = useState<BackupInfo[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [restoring, setRestoring] = useState<BackupInfo | null>(null)
  const [deleting, setDeleting] = useState<BackupInfo | null>(null)
  const [making, setMaking] = useState<'worlds' | 'full' | null>(null)
  const autoBackup = useStore((s) => s.snap?.settings.auto_backup ?? true)
  const locked = !!inst.running

  useEffect(() => {
    let live = true
    setBackups(null)
    setError(null)
    api.backupList(inst.id).then(
      (l) => live && setBackups(l),
      (e) => live && setError(msg(e)),
    )
    return () => {
      live = false
    }
  }, [inst.id, rev])

  const make = async (scope: 'worlds' | 'full') => {
    setMaking(scope)
    try {
      const b = await api.backupCreate(inst.id, scope)
      toast.success(b ? `已备份（${formatBytes(b.size)}）` : '没有可备份的内容')
      bump()
    } catch (e) {
      toast.error(msg(e))
    } finally {
      setMaking(null)
    }
  }

  const total = (backups ?? []).reduce((n, b) => n + b.size, 0)
  return (
    <div className="pb-4">
      <Bezel radius={26} className="mb-4">
        <div className="flex flex-wrap items-center gap-3 p-4">
          <div className="grid size-11 shrink-0 place-items-center rounded-2xl bg-brand/14 text-brand">
            <ShieldCheck className="size-5" />
          </div>
          <div className="min-w-[220px] flex-1">
            <div className="font-semibold">{autoBackup ? '每次启动前自动备份存档' : '自动备份已关闭'}</div>
            <div className="mt-0.5 text-xs text-muted-foreground">
              {autoBackup ? '存档没变化时会跳过，自动备份最多保留 10 份；手动备份不会被清理。' : '可以在「设置」里重新打开。'}
              {backups && backups.length > 0 && ` 现有 ${backups.length} 份，共 ${formatBytes(total)}。`}
            </div>
          </div>
          <PillButton size="sm" tone="soft" disabled={making !== null} icon={<Archive className="size-4" />} onClick={() => make('worlds')}>
            {making === 'worlds' ? '备份中…' : '备份存档'}
          </PillButton>
          <PillButton size="sm" tone="soft" disabled={making !== null} icon={<HardDrive className="size-4" />} onClick={() => make('full')}>
            {making === 'full' ? '备份中…' : '完整备份（含模组）'}
          </PillButton>
        </div>
      </Bezel>

      {error ? (
        <Empty sprite="tnt" title="读取备份失败" hint={error} action={<PillButton onClick={bump}>重试</PillButton>} />
      ) : backups === null ? (
        <Skeleton n={3} h={76} />
      ) : backups.length === 0 ? (
        <Empty sprite="chest" title="还没有备份" hint="启动一次游戏会自动备份，也可以点上面的按钮现在备份。" />
      ) : (
        <ol className="relative space-y-2 pl-5 before:absolute before:top-3 before:bottom-3 before:left-[7px] before:w-px before:bg-white/10">
          {backups.map((b, idx) => (
            <motion.li key={b.id} variants={rise} initial="hidden" animate="show" custom={idx % 8} className="relative">
              <span className={`absolute top-1/2 -left-5 size-[15px] -translate-y-1/2 rounded-full border-[3px] border-[oklch(0.17_0.02_255)] ${b.kind === 'safety' ? 'bg-amber-300' : b.kind === 'manual' ? 'bg-brand-2' : 'bg-brand'}`} />
              <Bezel radius={22}>
                <div className="flex items-center gap-3 px-3.5 py-2.5">
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="font-medium">{fmtDateTime(b.created)}</span>
                      <span className="text-xs text-muted-foreground">{fmtAgo(b.created)}</span>
                      <Chip tone={b.kind === 'safety' ? 'warn' : b.kind === 'manual' ? 'brand' : 'plain'}>{KIND_LABEL[b.kind]}</Chip>
                      {b.scope === 'full' && <Chip tone="brand">含模组</Chip>}
                    </div>
                    <div className="mt-0.5 truncate text-xs text-muted-foreground">
                      {b.reason} · {b.worlds.length} 个存档{b.worlds.length > 0 && `（${b.worlds.slice(0, 3).join('、')}${b.worlds.length > 3 ? '…' : ''}）`} · {formatBytes(b.size)}
                    </div>
                  </div>
                  <PillButton size="sm" tone="soft" disabled={locked} icon={<Undo2 className="size-4" />} onClick={() => setRestoring(b)}>
                    回退
                  </PillButton>
                  <button
                    type="button"
                    aria-label="删除这份备份"
                    title="删除这份备份"
                    onClick={() => setDeleting(b)}
                    className="grid size-9 place-items-center rounded-full text-muted-foreground transition-[background,color,transform] duration-500 [transition-timing-function:var(--ease)] hover:bg-red-500/15 hover:text-red-300 active:scale-90"
                  >
                    <Trash2 className="size-4" />
                  </button>
                </div>
              </Bezel>
            </motion.li>
          ))}
        </ol>
      )}
      {locked && backups && backups.length > 0 && <p className="mt-3 px-1 text-xs text-amber-300">游戏正在运行，回退需要先关闭游戏。</p>}

      {restoring && <RestoreDialog inst={inst} backup={restoring} onClose={() => setRestoring(null)} onDone={bump} />}
      <Dialog open={deleting !== null} onOpenChange={(o) => !o && setDeleting(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>删除这份备份？</DialogTitle>
            <DialogDescription>
              {deleting && `${fmtDateTime(deleting.created)} 的备份（${formatBytes(deleting.size)}）会被永久删除，不能恢复。`}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <PillButton tone="ghost" onClick={() => setDeleting(null)}>
              取消
            </PillButton>
            <PillButton
              tone="danger"
              onClick={async () => {
                const b = deleting
                setDeleting(null)
                if (!b) return
                try {
                  await api.backupDelete(inst.id, b.id)
                  bump()
                } catch (e) {
                  toast.error(msg(e))
                }
              }}
            >
              删除
            </PillButton>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}

// -------------------------------------------------------------------- stats

const DAYS = 30
const WEEKDAYS = ['周一', '周二', '周三', '周四', '周五', '周六', '周日']

function localKey(d: Date) {
  return `${d.getFullYear()}-${d.getMonth() + 1}-${d.getDate()}`
}

function Kpi({ label, value, sub }: { label: string; value: string; sub?: string }) {
  return (
    <Bezel radius={24}>
      <div className="px-4 py-3.5">
        <div className="text-xs text-muted-foreground">{label}</div>
        <div className="mt-1 text-2xl font-semibold tracking-tight tabular-nums">{value}</div>
        {sub && <div className="mt-0.5 truncate text-xs text-muted-foreground">{sub}</div>}
      </div>
    </Bezel>
  )
}

/** Magnitude in one hue: the biggest in full colour, the rest a step lighter. Values are always written out. */
function RankBars({ items, empty }: { items: { key: string; label: string; sub?: string; secs: number }[]; empty: string }) {
  if (items.length === 0) return <div className="py-6 text-center text-sm text-muted-foreground">{empty}</div>
  const max = Math.max(...items.map((i) => i.secs), 1)
  return (
    <ul className="space-y-3">
      {items.map((it, i) => (
        <li key={it.key}>
          <div className="flex items-baseline justify-between gap-3 text-sm">
            <span className="min-w-0 truncate font-medium">
              {it.label}
              {it.sub && <span className="ml-2 text-xs font-normal text-muted-foreground">{it.sub}</span>}
            </span>
            <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{fmtDuration(it.secs)}</span>
          </div>
          <div className="mt-1.5 h-2 overflow-hidden rounded-full bg-white/8">
            <motion.div
              className={`h-full origin-left rounded-full ${i === 0 ? 'bg-brand' : 'bg-brand/55'}`}
              initial={{ scaleX: 0 }}
              animate={{ scaleX: Math.max(0.02, it.secs / max) }}
              transition={{ duration: 0.7, delay: i * 0.05, ease: [0.32, 0.72, 0, 1] }}
              style={{ width: '100%' }}
            />
          </div>
        </li>
      ))}
    </ul>
  )
}

function ChartCard({ title, hint, children }: { title: string; hint?: React.ReactNode; children: React.ReactNode }) {
  return (
    <Bezel radius={28}>
      <div className="p-5">
        <div className="mb-4 flex items-baseline justify-between gap-3">
          <h3 className="font-semibold">{title}</h3>
          {hint && <div className="text-xs text-muted-foreground tabular-nums">{hint}</div>}
        </div>
        {children}
      </div>
    </Bezel>
  )
}

function Bars({ data, hover, setHover, labelEvery, height = 152 }: { data: { key: string; label: string; secs: number }[]; hover: number | null; setHover: (i: number | null) => void; labelEvery: number; height?: number }) {
  const max = Math.max(...data.map((d) => d.secs), 1)
  return (
    <div>
      <div className="relative" style={{ height }}>
        <div className="pointer-events-none absolute inset-x-0 top-0 border-t border-dashed border-white/10" />
        <div className="pointer-events-none absolute inset-x-0 top-1/2 border-t border-dashed border-white/6" />
        <span className="pointer-events-none absolute top-0.5 left-0 rounded bg-[oklch(0.17_0.02_255)]/80 pr-1.5 text-[10px] text-muted-foreground tabular-nums">{fmtHours(max)}</span>
        <div className="absolute inset-0 flex items-end gap-[3px]" onMouseLeave={() => setHover(null)}>
          {data.map((d, i) => (
            <div key={d.key} className="flex h-full flex-1 items-end" onMouseEnter={() => setHover(i)}>
              <div
                className={`w-full rounded-t-[4px] transition-[height,opacity] duration-500 [transition-timing-function:var(--ease)] ${d.secs > 0 ? 'bg-brand' : 'bg-white/10'}`}
                style={{ height: d.secs > 0 ? `max(3px, ${(d.secs / max) * 100}%)` : '2px', opacity: hover === null || hover === i ? 1 : 0.4 }}
              />
            </div>
          ))}
        </div>
      </div>
      <div className="mt-2 flex gap-[3px] text-[10px] text-muted-foreground">
        {data.map((d, i) => (
          <div key={d.key} className="flex-1 text-center whitespace-nowrap">
            {i % labelEvery === 0 || i === data.length - 1 ? d.label : ''}
          </div>
        ))}
      </div>
    </div>
  )
}

function StatsTab({ instances, scope }: { instances: Instance[]; scope: string }) {
  const [sessions, setSessions] = useState<PlaySession[] | null>(null)
  const [worldStats, setWorldStats] = useState<WorldStat[]>([])
  const [error, setError] = useState<string | null>(null)
  const [hoverDay, setHoverDay] = useState<number | null>(null)
  const [hoverWeek, setHoverWeek] = useState<number | null>(null)

  const load = useCallback(() => {
    setError(null)
    Promise.all([api.statsSessions(), api.statsWorlds()]).then(
      ([s, w]) => {
        setSessions(s)
        setWorldStats(w)
      },
      (e) => setError(msg(e)),
    )
  }, [])
  useEffect(load, [load])

  const names = useMemo(() => new Map(instances.map((i) => [i.id, i.name])), [instances])
  const mine = useMemo(() => (sessions ?? []).filter((s) => scope === 'all' || s.instance_id === scope), [sessions, scope])

  const calc = useMemo(() => {
    const total = mine.reduce((n, s) => n + s.secs, 0)
    const longest = mine.reduce<PlaySession | null>((b, s) => (!b || s.secs > b.secs ? s : b), null)
    const byInst = new Map<string, number>()
    for (const s of mine) byInst.set(s.instance_id, (byInst.get(s.instance_id) ?? 0) + s.secs)
    const instRank = [...byInst.entries()].map(([id, secs]) => ({ key: id, label: names.get(id) ?? id, secs })).sort((a, b) => b.secs - a.secs)

    const today = new Date()
    today.setHours(0, 0, 0, 0)
    const perDay = new Map<string, number>()
    const perWeekday = new Array(7).fill(0)
    for (const s of mine) {
      const d = new Date(s.start * 1000)
      perDay.set(localKey(d), (perDay.get(localKey(d)) ?? 0) + s.secs)
      perWeekday[(d.getDay() + 6) % 7] += s.secs
    }
    const days = Array.from({ length: DAYS }, (_, i) => {
      const d = new Date(today)
      d.setDate(d.getDate() - (DAYS - 1 - i))
      return { key: localKey(d), label: `${d.getMonth() + 1}/${d.getDate()}`, full: d.toLocaleDateString('zh-CN', { month: 'long', day: 'numeric', weekday: 'short' }), secs: perDay.get(localKey(d)) ?? 0 }
    })
    const activeDays = new Set(perDay.keys()).size
    const recent = days.reduce((n, d) => n + d.secs, 0)

    const worlds = worldStats
      .filter((w) => scope === 'all' || w.instance_id === scope)
      .map((w) => ({ key: `${w.instance_id}/${w.dir}`, label: w.name, sub: scope === 'all' ? (names.get(w.instance_id) ?? w.instance_id) : undefined, secs: w.play_secs }))
      .sort((a, b) => b.secs - a.secs)
    return { total, longest, instRank, days, activeDays, recent, perWeekday, worlds }
  }, [mine, names, worldStats, scope])

  if (error) return <Empty sprite="tnt" title="读取统计失败" hint={error} action={<PillButton onClick={load}>重试</PillButton>} />
  if (sessions === null) return <Skeleton n={3} h={150} />
  if (mine.length === 0 && calc.worlds.length === 0) {
    return <Empty sprite="diamond" title="还没有游玩记录" hint="玩一局之后，这里会出现时长、最常玩的存档和每天的游玩情况。" />
  }

  const { total, longest, instRank, days, activeDays, recent, perWeekday, worlds } = calc
  const day = hoverDay === null ? null : days[hoverDay]
  const topInst = instRank[0]
  const maxWeek = Math.max(...perWeekday, 0)
  return (
    <div className="space-y-4 pb-4">
      <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        <Kpi label="总游戏时长" value={fmtDuration(total)} sub={`${mine.length} 次游玩`} />
        <Kpi label="最长一次" value={longest ? fmtDuration(longest.secs) : '—'} sub={longest ? `${new Date(longest.start * 1000).toLocaleDateString('zh-CN')} · ${names.get(longest.instance_id) ?? longest.instance_id}` : undefined} />
        <Kpi label="平均每次" value={mine.length ? fmtDuration(total / mine.length) : '—'} sub={`${activeDays} 天有游玩`} />
        <Kpi label="最常玩的实例" value={topInst ? topInst.label : '—'} sub={topInst && total > 0 ? `${fmtDuration(topInst.secs)} · 占 ${Math.round((topInst.secs / total) * 100)}%` : undefined} />
      </div>

      <ChartCard
        title="最近 30 天"
        hint={
          day ? (
            <span className="text-foreground">
              {day.full} · {day.secs > 0 ? fmtDuration(day.secs) : '没有游玩'}
            </span>
          ) : (
            `共 ${fmtDuration(recent)}`
          )
        }
      >
        <Bars data={days} hover={hoverDay} setHover={setHoverDay} labelEvery={5} />
      </ChartCard>

      <div className="grid gap-4 lg:grid-cols-2">
        <ChartCard title="最常玩的存档" hint="按游戏内统计">
          <RankBars items={worlds.slice(0, 8)} empty="还没有存档的游玩统计（游戏需要保存过一次世界）" />
        </ChartCard>
        <ChartCard title="各实例游玩时长">
          <RankBars items={instRank.slice(0, 8)} empty="还没有实例的游玩记录" />
        </ChartCard>
      </div>

      <ChartCard
        title="一周里什么时候玩"
        hint={hoverWeek !== null ? <span className="text-foreground">{WEEKDAYS[hoverWeek]} · {fmtDuration(perWeekday[hoverWeek])}</span> : maxWeek > 0 ? `最常在${WEEKDAYS[perWeekday.indexOf(maxWeek)]}` : undefined}
      >
        <Bars data={WEEKDAYS.map((label, i) => ({ key: label, label, secs: perWeekday[i] }))} hover={hoverWeek} setHover={setHoverWeek} labelEvery={1} height={112} />
      </ChartCard>
      <p className="px-1 text-xs text-muted-foreground">游玩时长从游戏窗口出现开始算；存档时长是游戏自己记录的，两者可能略有差别。</p>
    </div>
  )
}

// --------------------------------------------------------------------- page

export function WorldsPage() {
  const snap = useStore((s) => s.snap)
  const [tab, setTab] = useState<Tab>('worlds')
  const [picked, setPicked] = useState<string | null>(null)
  const [rev, setRev] = useState(0)
  const bump = useCallback(() => setRev((n) => n + 1), [])

  const instances = snap?.instances ?? []
  const selected = selectedInstance(snap)
  const fallback = selected?.id ?? instances[0]?.id ?? ''
  const scope = picked && (picked === 'all' ? tab === 'stats' : instances.some((i) => i.id === picked)) ? picked : fallback
  const inst = instances.find((i) => i.id === scope) ?? null

  const tabs = (
    <Segmented<Tab>
      value={tab}
      onChange={setTab}
      options={[
        { value: 'worlds', label: '存档' },
        { value: 'servers', label: '服务器' },
        { value: 'shots', label: '截图' },
        { value: 'backups', label: '备份' },
        { value: 'stats', label: '统计' },
      ]}
    />
  )
  const subtitle = tab === 'stats' ? '你玩了多久、玩了什么' : inst ? `${inst.name} · ${instanceKind(inst)} · Minecraft ${inst.mc_version ?? inst.version_id}` : '管理存档，自动备份，随时回退'

  if (instances.length === 0) {
    return (
      <div className="flex h-full min-h-0 flex-col">
        <PageBanner biome="meadow" seed="worlds" eyebrow="Worlds" title="存档" subtitle="管理存档，自动备份，随时回退" />
        <Empty sprite="grass" title="还没有实例" hint="先去「下载」页创建一个实例，进游戏玩过之后这里就有存档了。" />
      </div>
    )
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageBanner biome="meadow" seed="worlds" eyebrow="Worlds" title="存档" subtitle={subtitle} actions={tabs} />
      <InstanceChips instances={instances} value={tab === 'stats' && picked === 'all' ? 'all' : scope} onChange={setPicked} allowAll={tab === 'stats'} />
      <div className="min-h-0 flex-1 overflow-auto pr-1">
        {tab === 'stats' ? (
          <StatsTab instances={instances} scope={picked === 'all' ? 'all' : scope} />
        ) : inst ? (
          tab === 'worlds' ? (
            <WorldsTab key={inst.id} inst={inst} rev={rev} bump={bump} />
          ) : tab === 'servers' ? (
            <ServersTab key={inst.id} inst={inst} />
          ) : tab === 'shots' ? (
            <ShotsTab key={inst.id} inst={inst} rev={rev} bump={bump} />
          ) : (
            <BackupsTab key={inst.id} inst={inst} rev={rev} bump={bump} />
          )
        ) : null}
      </div>
    </div>
  )
}

