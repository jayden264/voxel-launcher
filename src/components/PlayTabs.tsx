import { useCallback, useEffect, useRef, useState } from 'react'
import { motion } from 'motion/react'
import { ChevronLeft, ChevronRight, FolderOpen, Play, Plus, RefreshCw, Trash2 } from 'lucide-react'
import { toast } from 'sonner'
import { api, type Instance, type Screenshot, type ServerEntry, type ServerStatus } from '@/lib/api'
import { fmtAgo, fmtDateTime } from '@/lib/format'
import { rise } from '@/lib/motion'
import { formatBytes, useStore } from '@/lib/store'
import { PixelSprite } from '@/components/art'
import { Bezel, PillButton } from '@/components/Bezel'
import { Chip, Empty } from '@/components/kit'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e))

/** The game's own minor version out of a string like "Paper 1.21.1" or "1.20.4", when there is one. */
function minorOf(text: string): string | null {
  const m = /(?:^|\D)(1\.\d{1,2})(?:\.\d+)?(?!\d)/.exec(text)
  return m ? m[1] : null
}

function latencyTone(ms: number): 'brand' | 'warn' | 'bad' {
  return ms < 90 ? 'brand' : ms < 220 ? 'warn' : 'bad'
}

// ------------------------------------------------------------------ servers

type Ping = { kind: 'loading' } | { kind: 'ok'; st: ServerStatus } | { kind: 'down'; err: string }

function ServerCard({ s, inst, idx, canJoin, onJoin, onRemove }: { s: ServerEntry; inst: Instance; idx: number; canJoin: boolean; onJoin: () => void; onRemove: () => void }) {
  const [ping, setPing] = useState<Ping>({ kind: 'loading' })
  const run = useCallback(() => {
    setPing({ kind: 'loading' })
    api.serverPing(s.id).then(
      (st) => setPing({ kind: 'ok', st }),
      (e) => setPing({ kind: 'down', err: msg(e) }),
    )
  }, [s.id])
  useEffect(run, [run])

  const mc = inst.mc_version ?? inst.version_id
  const theirs = ping.kind === 'ok' ? minorOf(ping.st.version) : null
  const mine = minorOf(mc)
  const mismatch = theirs && mine && theirs !== mine
  const icon = 'grid size-9 place-items-center rounded-full text-muted-foreground transition-[background,color,transform] duration-500 [transition-timing-function:var(--ease)] hover:bg-white/10 hover:text-foreground active:scale-90'

  return (
    <motion.div variants={rise} initial="hidden" animate="show" custom={idx % 8}>
      <Bezel radius={26}>
        <div className="flex items-center gap-4 p-4">
          <div className="relative grid size-16 shrink-0 place-items-center overflow-hidden rounded-2xl bg-black/30">
            {ping.kind === 'ok' && ping.st.favicon ? <img src={ping.st.favicon} alt="" className="size-full object-cover [image-rendering:pixelated]" /> : <PixelSprite name="ender" size={40} />}
            <span className={`absolute right-1 bottom-1 size-3 rounded-full border-2 border-[oklch(0.17_0.02_255)] ${ping.kind === 'ok' ? 'bg-brand' : ping.kind === 'down' ? 'bg-red-400' : 'animate-pulse bg-amber-300'}`} />
          </div>
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-2">
              <span className="truncate text-base font-semibold">{s.name}</span>
              <span className="truncate text-xs text-muted-foreground">
                {s.host}
                {s.port !== 25565 && `:${s.port}`}
              </span>
            </div>
            {ping.kind === 'loading' && <div className="mt-1.5 h-4 w-2/3 animate-pulse rounded bg-white/8" />}
            {ping.kind === 'down' && <div className="mt-1 text-sm text-red-300/90">{ping.err}</div>}
            {ping.kind === 'ok' && (
              <>
                <div className="mt-1 line-clamp-2 text-sm text-foreground/75">{ping.st.motd || '（没有介绍）'}</div>
                <div className="mt-2 flex flex-wrap items-center gap-1.5">
                  <Chip tone="plain">
                    {ping.st.online} / {ping.st.max} 在线
                  </Chip>
                  <Chip tone={latencyTone(ping.st.latency_ms)}>{ping.st.latency_ms} ms</Chip>
                  {ping.st.version && <Chip>{ping.st.version}</Chip>}
                  {mismatch && <Chip tone="warn">服务器 {theirs} · 此实例 {mine}</Chip>}
                </div>
              </>
            )}
          </div>
          <div className="flex shrink-0 items-center gap-1">
            <button type="button" aria-label="刷新状态" title="刷新状态" className={icon} onClick={run}>
              <RefreshCw className={`size-4 ${ping.kind === 'loading' ? 'animate-spin' : ''}`} />
            </button>
            <button type="button" aria-label="删除服务器" title="删除" className={`${icon} hover:!bg-red-500/15 hover:!text-red-300`} onClick={onRemove}>
              <Trash2 className="size-4" />
            </button>
            <PillButton size="sm" disabled={!canJoin} icon={<Play className="size-4 fill-current" />} onClick={onJoin} title={mismatch ? '版本可能不匹配，进不去时请换一个实例' : undefined}>
              加入
            </PillButton>
          </div>
        </div>
      </Bezel>
    </motion.div>
  )
}

export function ServersTab({ inst }: { inst: Instance }) {
  const servers = useStore((s) => s.snap?.settings.servers ?? [])
  const busy = useStore((s) => s.run.phase !== 'idle')
  const launch = useStore((s) => s.launch)
  const setPage = useStore((s) => s.setPage)
  const [adding, setAdding] = useState(false)
  const [name, setName] = useState('')
  const [address, setAddress] = useState('')
  const [working, setWorking] = useState(false)
  const [removing, setRemoving] = useState<ServerEntry | null>(null)
  const canJoin = !busy && !inst.running

  const add = async () => {
    setWorking(true)
    try {
      const snap = await api.serverAdd(name, address)
      useStore.setState({ snap })
      setAdding(false)
      setName('')
      setAddress('')
      toast.success('已添加服务器')
    } catch (e) {
      toast.error(msg(e))
    } finally {
      setWorking(false)
    }
  }

  return (
    <div className="pb-4">
      <div className="mb-4 flex flex-wrap items-center gap-3">
        <div className="text-sm text-muted-foreground">
          点「加入」会用 <span className="font-medium text-foreground">{inst.name}</span> 启动游戏并直接进入服务器；想换实例，点上面的实例名。
        </div>
        <PillButton size="sm" className="ml-auto" icon={<Plus className="size-4" />} onClick={() => setAdding(true)}>
          添加服务器
        </PillButton>
      </div>

      {servers.length === 0 ? (
        <Empty sprite="ender" title="还没有保存的服务器" hint="添加常去的服务器后，能直接看到它在不在线、有几个人，一键加入。" action={<PillButton onClick={() => setAdding(true)}>添加服务器</PillButton>} />
      ) : (
        <div className="space-y-2.5">
          {servers.map((s, i) => (
            <ServerCard
              key={s.id}
              s={s}
              inst={inst}
              idx={i}
              canJoin={canJoin}
              onRemove={() => setRemoving(s)}
              onJoin={() => {
                launch(inst.id, false, { kind: 'server', host: s.host, port: s.port })
                setPage('home')
              }}
            />
          ))}
        </div>
      )}

      <Dialog open={adding} onOpenChange={(o) => !o && !working && setAdding(false)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>添加服务器</DialogTitle>
            <DialogDescription>地址可以只写域名，也可以带端口，例如 play.example.com:25566。</DialogDescription>
          </DialogHeader>
          <div className="space-y-3">
            <Input value={name} maxLength={40} onChange={(e) => setName(e.target.value)} placeholder="名称，例如：生存服" autoFocus />
            <Input value={address} onChange={(e) => setAddress(e.target.value)} placeholder="地址，例如：play.example.com" className="font-mono" onKeyDown={(e) => e.key === 'Enter' && name.trim() && address.trim() && add()} />
          </div>
          <DialogFooter>
            <PillButton tone="ghost" disabled={working} onClick={() => setAdding(false)}>
              取消
            </PillButton>
            <PillButton disabled={working || !name.trim() || !address.trim()} onClick={add}>
              {working ? '添加中…' : '添加'}
            </PillButton>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={removing !== null} onOpenChange={(o) => !o && setRemoving(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>删除「{removing?.name}」？</DialogTitle>
            <DialogDescription>只是从列表里移除，不影响服务器本身。</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <PillButton tone="ghost" onClick={() => setRemoving(null)}>
              取消
            </PillButton>
            <PillButton
              tone="danger"
              onClick={async () => {
                const s = removing
                setRemoving(null)
                if (!s) return
                try {
                  useStore.setState({ snap: await api.serverRemove(s.id) })
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

// -------------------------------------------------------------- screenshots

/** A preview that is only fetched once it scrolls into view. */
function Thumb({ instanceId, file, width, alt, className }: { instanceId: string; file: string; width: number; alt: string; className?: string }) {
  const ref = useRef<HTMLDivElement>(null)
  const [seen, setSeen] = useState(false)
  const [src, setSrc] = useState<string | null>(null)
  const [failed, setFailed] = useState(false)

  useEffect(() => {
    const el = ref.current
    if (!el) return
    const io = new IntersectionObserver(
      ([e]) => {
        if (e.isIntersecting) {
          setSeen(true)
          io.disconnect()
        }
      },
      { rootMargin: '240px' },
    )
    io.observe(el)
    return () => io.disconnect()
  }, [])

  useEffect(() => {
    if (!seen) return
    let live = true
    setSrc(null)
    setFailed(false)
    api.screenshotImage(instanceId, file, width).then(
      (u) => live && setSrc(u),
      () => live && setFailed(true),
    )
    return () => {
      live = false
    }
  }, [seen, instanceId, file, width])

  return (
    <div ref={ref} className={`relative overflow-hidden bg-black/30 ${className ?? ''}`}>
      {src ? <img src={src} alt={alt} className="size-full object-cover" /> : failed ? <div className="grid size-full place-items-center text-xs text-muted-foreground">无法预览</div> : <div className="size-full animate-pulse bg-white/5" />}
    </div>
  )
}

function Lightbox({ inst, shots, index, onIndex, onClose, onDelete }: { inst: Instance; shots: Screenshot[]; index: number; onIndex: (i: number) => void; onClose: () => void; onDelete: (s: Screenshot) => void }) {
  const s = shots[index]
  const [confirm, setConfirm] = useState(false)
  useEffect(() => setConfirm(false), [index])
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key === 'ArrowLeft' && index > 0) onIndex(index - 1)
      if (e.key === 'ArrowRight' && index < shots.length - 1) onIndex(index + 1)
    }
    window.addEventListener('keydown', key)
    return () => window.removeEventListener('keydown', key)
  }, [index, shots.length, onIndex])
  if (!s) return null
  const nav = 'absolute top-1/2 grid size-11 -translate-y-1/2 place-items-center rounded-full bg-black/45 text-foreground backdrop-blur-sm transition-[background,opacity] duration-300 hover:bg-black/65 disabled:opacity-0'
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-5xl">
        <DialogHeader>
          <DialogTitle>{fmtDateTime(s.taken)}</DialogTitle>
          <DialogDescription>
            {s.file} · {formatBytes(s.size)} · {index + 1} / {shots.length}
          </DialogDescription>
        </DialogHeader>
        <div className="relative">
          <Thumb key={s.file} instanceId={inst.id} file={s.file} width={1600} alt={s.file} className="aspect-video w-full rounded-2xl" />
          <button type="button" aria-label="上一张" disabled={index === 0} onClick={() => onIndex(index - 1)} className={`${nav} left-3`}>
            <ChevronLeft className="size-5" />
          </button>
          <button type="button" aria-label="下一张" disabled={index === shots.length - 1} onClick={() => onIndex(index + 1)} className={`${nav} right-3`}>
            <ChevronRight className="size-5" />
          </button>
        </div>
        <DialogFooter>
          {confirm ? (
            <>
              <span className="mr-auto self-center text-sm text-red-300">删除后不能恢复，确定吗？</span>
              <PillButton tone="ghost" onClick={() => setConfirm(false)}>
                不删了
              </PillButton>
              <PillButton tone="danger" onClick={() => onDelete(s)}>
                确定删除
              </PillButton>
            </>
          ) : (
            <>
              <PillButton tone="ghost" icon={<FolderOpen className="size-4" />} onClick={() => api.screenshotsOpen(inst.id).catch((e) => toast.error(msg(e)))}>
                打开文件夹
              </PillButton>
              <PillButton tone="soft" icon={<Trash2 className="size-4" />} onClick={() => setConfirm(true)}>
                删除
              </PillButton>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

export function ShotsTab({ inst, rev, bump }: { inst: Instance; rev: number; bump: () => void }) {
  const [shots, setShots] = useState<Screenshot[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [open, setOpen] = useState<number | null>(null)

  useEffect(() => {
    let live = true
    setShots(null)
    setError(null)
    api.screenshotsList(inst.id).then(
      (l) => live && setShots(l),
      (e) => live && setError(msg(e)),
    )
    return () => {
      live = false
    }
  }, [inst.id, rev])

  if (error) return <Empty sprite="tnt" title="读取截图失败" hint={error} action={<PillButton onClick={bump}>重试</PillButton>} />
  if (shots === null) return <div className="grid grid-cols-[repeat(auto-fill,minmax(230px,1fr))] gap-3">{Array.from({ length: 8 }, (_, i) => <div key={i} className="aspect-video animate-pulse rounded-3xl bg-white/5" style={{ animationDelay: `${i * 80}ms` }} />)}</div>
  if (shots.length === 0) {
    return <Empty sprite="diamond" title="这个实例还没有截图" hint="在游戏里按 F2 截图，之后会出现在这里。" action={<PillButton onClick={() => api.screenshotsOpen(inst.id).catch((e) => toast.error(msg(e)))}>打开截图文件夹</PillButton>} />
  }
  const total = shots.reduce((n, s) => n + s.size, 0)
  return (
    <div className="pb-4">
      <div className="mb-4 flex items-center gap-3 text-xs text-muted-foreground">
        <span className="tabular-nums">
          {shots.length} 张 · 共 {formatBytes(total)}
        </span>
        <PillButton size="sm" tone="ghost" className="ml-auto" icon={<FolderOpen className="size-4" />} onClick={() => api.screenshotsOpen(inst.id).catch((e) => toast.error(msg(e)))}>
          打开文件夹
        </PillButton>
      </div>
      <div className="grid grid-cols-[repeat(auto-fill,minmax(230px,1fr))] gap-3">
        {shots.map((s, i) => (
          <motion.button key={s.file} type="button" variants={rise} initial="hidden" animate="show" custom={i % 8} onClick={() => setOpen(i)} className="group text-left" aria-label={`查看截图 ${fmtDateTime(s.taken)}`}>
            <Bezel radius={24}>
              <Thumb instanceId={inst.id} file={s.file} width={360} alt={s.file} className="aspect-video w-full transition-transform duration-700 [transition-timing-function:var(--ease)] group-hover:scale-[1.03]" />
              <div className="flex items-center justify-between px-3 py-2 text-xs">
                <span className="font-medium">{fmtDateTime(s.taken)}</span>
                <span className="text-muted-foreground">{fmtAgo(s.taken)}</span>
              </div>
            </Bezel>
          </motion.button>
        ))}
      </div>
      {open !== null && (
        <Lightbox
          inst={inst}
          shots={shots}
          index={Math.min(open, shots.length - 1)}
          onIndex={setOpen}
          onClose={() => setOpen(null)}
          onDelete={async (s) => {
            try {
              await api.screenshotDelete(inst.id, s.file)
              toast.success('已删除截图')
              setOpen(shots.length > 1 ? Math.min(open, shots.length - 2) : null)
              bump()
            } catch (e) {
              toast.error(msg(e))
            }
          }}
        />
      )}
    </div>
  )
}
