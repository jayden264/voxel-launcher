import { useEffect, useRef, useState } from 'react'
import { motion } from 'motion/react'
import { Download as DownloadIcon, Search } from 'lucide-react'
import { toast } from 'sonner'
import { api, type ModHit, type PackVersion } from '@/lib/api'
import { rise } from '@/lib/motion'
import { formatBytes, useStore } from '@/lib/store'
import { PixelSprite } from '@/components/art'
import { Bezel, PillButton } from '@/components/Bezel'
import { Empty } from '@/components/kit'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { PixelSelect } from '@/components/ui/pixel'

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e))

function formatCount(n: number) {
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)}M`
  if (n >= 1e3) return `${(n / 1e3).toFixed(1)}K`
  return String(n)
}

function PackIcon({ url }: { url: string | null }) {
  const [broken, setBroken] = useState(false)
  if (url && !broken) return <img src={url} alt="" loading="lazy" onError={() => setBroken(true)} className="size-12 shrink-0 rounded-2xl bg-black/25 object-cover" />
  return (
    <div className="grid size-12 shrink-0 place-items-center rounded-2xl bg-black/25">
      <PixelSprite name="chest" size={30} />
    </div>
  )
}

const versionLabel = (v: PackVersion) => {
  const mc = v.game_versions[v.game_versions.length - 1] ?? '?'
  return `${v.version_number} · Minecraft ${mc}${v.loaders.length ? ` · ${v.loaders.join('/')}` : ''}`
}

/** Pick a version of a modpack and create an instance from it. */
function PackDialog({ pack, onClose }: { pack: ModHit; onClose: () => void }) {
  const installModpack = useStore((s) => s.installModpack)
  const setPage = useStore((s) => s.setPage)
  const busy = useStore((s) => s.run.phase !== 'idle')
  const [versions, setVersions] = useState<PackVersion[] | 'loading' | { error: string }>('loading')
  const [pickedId, setPickedId] = useState<string | null>(null)
  const [typedName, setTypedName] = useState<string | null>(null)

  useEffect(() => {
    let alive = true
    api.modpackVersions(pack.project_id).then(
      (list) => alive && setVersions(list),
      (e) => alive && setVersions({ error: msg(e) }),
    )
    return () => {
      alive = false
    }
  }, [pack.project_id])

  const list = Array.isArray(versions) ? versions : []
  const chosen = list.find((v) => v.id === pickedId) ?? list.find((v) => v.version_type === 'release') ?? list[0] ?? null
  const name = typedName ?? pack.title
  const valid = !!chosen && name.trim().length > 0 && name.trim().length <= 40

  const install = async () => {
    if (!chosen || !valid) return
    onClose()
    if (await installModpack(name.trim(), chosen)) setPage('instances')
  }

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[88vh] overflow-y-auto sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>安装整合包「{pack.title}」</DialogTitle>
          <DialogDescription>将新建一个实例，包含这个整合包的游戏版本、加载器、全部模组和配置。</DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <div className="space-y-1.5">
            <label className="text-sm font-medium">实例名称</label>
            <Input value={name} onChange={(e) => setTypedName(e.target.value)} maxLength={40} />
          </div>
          <div className="space-y-1.5">
            <label className="text-sm font-medium">版本</label>
            {versions === 'loading' ? (
              <p className="px-1 text-xs text-muted-foreground">正在获取版本…</p>
            ) : !Array.isArray(versions) ? (
              <p className="px-1 text-xs text-red-300">{versions.error}</p>
            ) : list.length === 0 ? (
              <p className="px-1 text-xs text-muted-foreground">这个整合包没有可安装的版本。</p>
            ) : (
              <PixelSelect
                aria-label="整合包版本"
                value={chosen?.id ?? null}
                onValueChange={setPickedId}
                options={list.map((v) => ({ value: v.id, label: versionLabel(v), tag: v.version_type === 'release' ? undefined : v.version_type }))}
              />
            )}
            {chosen && <p className="px-1 text-xs text-muted-foreground">整合包文件 {formatBytes(chosen.file.size)}，模组会在安装时逐个下载。</p>}
          </div>
        </div>
        <DialogFooter>
          <PillButton tone="ghost" onClick={onClose}>
            取消
          </PillButton>
          <PillButton icon={<DownloadIcon className="size-4" />} onClick={install} disabled={!valid || busy}>
            创建并安装
          </PillButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

/** Search Modrinth modpacks and install one as a new instance. */
export function ModpackBrowser() {
  const [q, setQ] = useState('')
  const [term, setTerm] = useState('')
  const [hits, setHits] = useState<ModHit[]>([])
  const [total, setTotal] = useState(0)
  const [loading, setLoading] = useState(true)
  const [moreLoading, setMoreLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [attempt, setAttempt] = useState(0)
  const [picked, setPicked] = useState<ModHit | null>(null)
  const generation = useRef(0)

  useEffect(() => {
    const t = setTimeout(() => setTerm(q.trim()), 400)
    return () => clearTimeout(t)
  }, [q])

  useEffect(() => {
    const mine = ++generation.current
    setLoading(true)
    setError(null)
    api.modpackSearch(term, 0).then(
      (r) => {
        if (mine !== generation.current) return
        setHits(r.hits)
        setTotal(r.total)
        setLoading(false)
      },
      (e) => {
        if (mine !== generation.current) return
        setError(msg(e))
        setLoading(false)
      },
    )
  }, [term, attempt])

  const loadMore = async () => {
    const mine = generation.current
    setMoreLoading(true)
    try {
      const r = await api.modpackSearch(term, hits.length)
      if (mine === generation.current) setHits((h) => [...h, ...r.hits])
    } catch (e) {
      toast.error(msg(e))
    } finally {
      setMoreLoading(false)
    }
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="relative mb-4 max-w-sm">
        <Search className="pointer-events-none absolute top-1/2 left-4 size-4 -translate-y-1/2 text-muted-foreground" />
        <Input value={q} onChange={(e) => setQ(e.target.value)} placeholder="搜索整合包，例如 fabulously" className="h-10 rounded-full pl-10" />
      </div>

      <div className="min-h-0 flex-1 overflow-auto pr-1">
        {error ? (
          <Empty sprite="tnt" title="搜索失败" hint={error} action={<PillButton onClick={() => setAttempt((n) => n + 1)}>重试</PillButton>} />
        ) : loading ? (
          <div className="space-y-2">
            {Array.from({ length: 5 }, (_, i) => (
              <div key={i} className="h-[76px] animate-pulse rounded-[24px] bg-white/5" style={{ animationDelay: `${i * 90}ms` }} />
            ))}
          </div>
        ) : hits.length === 0 ? (
          <Empty sprite="diamond" title="没有找到整合包" hint="换个关键词试试。" />
        ) : (
          <div className="space-y-2 pb-4">
            {hits.map((h, idx) => (
              <motion.div key={h.project_id} variants={rise} initial="hidden" animate="show" custom={idx % 8}>
                <Bezel radius={24}>
                  <div className="flex items-center gap-4 px-3.5 py-3">
                    <PackIcon url={h.icon_url} />
                    <div className="min-w-0 flex-1">
                      <div className="flex items-baseline gap-2">
                        <span className="truncate font-semibold">{h.title}</span>
                        <span className="shrink-0 text-xs text-muted-foreground">
                          {h.author} · {formatCount(h.downloads)} 次下载
                        </span>
                      </div>
                      <div className="mt-0.5 line-clamp-2 text-sm text-muted-foreground">{h.description}</div>
                    </div>
                    <PillButton size="sm" onClick={() => setPicked(h)}>
                      安装
                    </PillButton>
                  </div>
                </Bezel>
              </motion.div>
            ))}
            {hits.length < total && (
              <div className="flex justify-center pt-2">
                <PillButton tone="soft" onClick={loadMore} disabled={moreLoading}>
                  {moreLoading ? '加载中…' : `加载更多（${hits.length}/${total}）`}
                </PillButton>
              </div>
            )}
          </div>
        )}
      </div>

      {picked && <PackDialog key={picked.project_id} pack={picked} onClose={() => setPicked(null)} />}
    </div>
  )
}
