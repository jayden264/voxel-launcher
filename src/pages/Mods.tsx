import { useCallback, useEffect, useRef, useState } from 'react'
import { motion } from 'motion/react'
import { ArrowUpCircle, Download as DownloadIcon, RefreshCw, Search, Trash2, TriangleAlert } from 'lucide-react'
import { toast } from 'sonner'
import { api, type SearchOpts, type ContentKind, type InstallPlan, type InstalledMod, type Instance, type MissingDep, type ModHit, type UpdateItem } from '@/lib/api'
import { ProjectDetailView } from '@/components/ProjectDetail'
import { EASE, rise } from '@/lib/motion'
import { formatBytes, selectedInstance, useStore } from '@/lib/store'
import { PixelSprite } from '@/components/art'
import { Bezel, PillButton } from '@/components/Bezel'
import { Empty, LOADER_LABEL, PageBanner, Segmented } from '@/components/kit'
import { RunProgress } from '@/components/RunProgress'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { PixelSwitch } from '@/components/ui/pixel'

type Tab = 'installed' | 'browse'

const KIND_LABEL: Record<ContentKind, string> = { mod: '模组', shader: '光影', resourcepack: '资源包' }
const SEARCH_HINT: Record<ContentKind, string> = { mod: '搜索模组，例如 sodium', shader: '搜索光影，例如 complementary', resourcepack: '搜索资源包，例如 faithful' }

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e))

type Sort = NonNullable<SearchOpts['sort']>
const SORTS: { value: Sort; label: string }[] = [
  { value: 'relevance', label: '相关度' },
  { value: 'downloads', label: '下载量' },
  { value: 'follows', label: '关注数' },
  { value: 'updated', label: '最近更新' },
  { value: 'newest', label: '最新发布' },
]

/** Modrinth category ids with the names players use. */
const CATEGORIES: Record<ContentKind, [string, string][]> = {
  mod: [['optimization', '优化'], ['technology', '科技'], ['adventure', '冒险'], ['magic', '魔法'], ['equipment', '装备'], ['utility', '实用'], ['worldgen', '世界生成'], ['decoration', '装饰'], ['food', '食物'], ['storage', '存储'], ['transportation', '交通'], ['mobs', '生物'], ['library', '前置库'], ['management', '管理'], ['social', '社交'], ['game-mechanics', '机制'], ['minigame', '小游戏'], ['cursed', '整活']],
  shader: [['realistic', '写实'], ['fantasy', '奇幻'], ['vanilla', '原版风'], ['cursed', '整活'], ['potato', '土豆机'], ['low', '低配'], ['medium', '中配'], ['high', '高配'], ['screenshot', '截图向']],
  resourcepack: [['simplistic', '极简'], ['realistic', '写实'], ['themed', '主题'], ['vanilla-like', '原版风'], ['decoration', '装饰'], ['combat', '战斗'], ['fonts', '字体'], ['gui', '界面'], ['audio', '音效'], ['blocks', '方块'], ['items', '物品'], ['entities', '实体'], ['environment', '环境'], ['equipment', '装备'], ['locale', '语言'], ['modded', '模组适配'], ['tweaks', '微调'], ['utility', '实用']],
}

function formatCount(n: number) {
  if (n >= 1e9) return `${(n / 1e9).toFixed(1)}B`
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)}M`
  if (n >= 1e3) return `${(n / 1e3).toFixed(1)}K`
  return String(n)
}

function ModIcon({ url }: { url: string | null }) {
  const [broken, setBroken] = useState(false)
  if (url && !broken) {
    return <img src={url} alt="" loading="lazy" onError={() => setBroken(true)} className="size-12 shrink-0 rounded-2xl bg-black/25 object-cover" />
  }
  return (
    <div className="grid size-12 shrink-0 place-items-center rounded-2xl bg-black/25">
      <PixelSprite name="diamond" size={30} />
    </div>
  )
}

// ----------------------------------------------------------------- installed

function InstalledTab({ inst, kind, mods, locked, reload, goBrowse, onInstall, onOpen }: { inst: Instance; kind: ContentKind; mods: InstalledMod[] | null; locked: boolean; reload: () => Promise<void>; goBrowse: () => void; onInstall: (plan: InstallPlan) => Promise<void>; onOpen: (hit: ModHit) => void }) {
  const [removing, setRemoving] = useState<InstalledMod | null>(null)
  const [missing, setMissing] = useState<MissingDep[] | null>(null)
  const [fixing, setFixing] = useState(false)
  const [query, setQuery] = useState('')
  const [scanning, setScanning] = useState(false)
  // null: not checked yet for this instance/kind.
  const [updates, setUpdates] = useState<Record<string, UpdateItem> | null>(null)
  const label = KIND_LABEL[kind]

  useEffect(() => {
    setUpdates(null)
    setQuery('')
  }, [inst.id, kind])

  const scan = async () => {
    setScanning(true)
    try {
      const r = await api.contentScan(inst.id, kind)
      setUpdates(Object.fromEntries(r.updates.map((u) => [u.filename, u])))
      if (r.identified > 0) await reload()
      if (r.missing.length > 0) setMissing(r.missing)
      toast.success(r.updates.length > 0 ? `发现 ${r.updates.length} 个可更新${r.identified > 0 ? `，并识别了 ${r.identified} 个文件` : ''}` : r.identified > 0 ? `已识别 ${r.identified} 个文件，全部是最新` : '全部是最新版本')
    } catch (e) {
      toast.error(msg(e))
    } finally {
      setScanning(false)
    }
  }

  const installMissing = async () => {
    const list = missing
    if (!list) return
    setFixing(true)
    try {
      const plan = await api.contentPlanMissing(inst.id, list.map((m) => m.project_id))
      setMissing(null)
      if (plan.files.length === 0) toast.info('这些前置已经装好了')
      else await onInstall(plan)
    } catch (e) {
      toast.error(msg(e))
    } finally {
      setFixing(false)
    }
  }

  const fixDuplicates = async () => {
    try {
      const off = await api.modsDisableDuplicates(inst.id)
      toast.success(off.length > 0 ? `已禁用 ${off.length} 个旧的重复文件（可随时重新启用）` : '没有需要处理的重复模组')
      await reload()
    } catch (e) {
      toast.error(msg(e))
    }
  }

  const applyUpdates = async (items: UpdateItem[]) => {
    const plan: InstallPlan = { files: items.map((u) => u.file), total_bytes: items.reduce((n, u) => n + u.file.size, 0), warnings: [] }
    const done = new Set(items.map((u) => u.filename))
    // Updates replace jars; a full backup first makes a bad update one click to undo.
    try {
      await api.backupCreate(inst.id, 'full', '更新模组前')
    } catch (e) {
      toast.warning(`更新前的备份失败，仍会继续更新：${msg(e)}`)
    }
    await onInstall(plan)
    setUpdates((cur) => (cur ? Object.fromEntries(Object.entries(cur).filter(([k]) => !done.has(k))) : cur))
  }

  if (mods === null) return <div className="py-10 text-center text-sm text-muted-foreground">正在读取{label}…</div>
  if (mods.length === 0) {
    return (
      <Empty
        sprite="chest"
        title={`还没有安装${label}`}
        hint={kind === 'shader' ? '去「浏览」搜索光影，需要的 Iris 会自动补齐。' : '去「浏览」搜索并安装，依赖会自动补齐。'}
        action={<PillButton onClick={goBrowse}>浏览{label}</PillButton>}
      />
    )
  }

  const toggle = async (m: InstalledMod, enabled: boolean) => {
    try {
      await api.modsSetEnabled(inst.id, m.filename, enabled)
      await reload()
    } catch (e) {
      toast.error(msg(e))
    }
  }

  const q = query.trim().toLowerCase()
  const shown = q ? mods.filter((m) => `${m.title ?? ''} ${m.filename}`.toLowerCase().includes(q)) : mods
  const pending = updates ? Object.values(updates) : []
  const dupGroups = new Map<string, InstalledMod[]>()
  for (const m of mods) if (m.duplicate && m.mod_id) dupGroups.set(m.mod_id, [...(dupGroups.get(m.mod_id) ?? []), m])

  return (
    <div className="space-y-2 pb-4">
      {dupGroups.size > 0 && (
        <div className="rounded-3xl border border-red-400/25 bg-red-500/10 p-4">
          <div className="flex items-start gap-3">
            <TriangleAlert className="mt-0.5 size-5 shrink-0 text-red-300" />
            <div className="min-w-0 flex-1">
              <div className="font-semibold text-red-200">发现 {dupGroups.size} 个重复模组，游戏很可能无法启动</div>
              <ul className="mt-1.5 space-y-0.5 text-xs text-red-200/80">
                {[...dupGroups.entries()].slice(0, 6).map(([id, files]) => (
                  <li key={id} className="truncate">
                    <span className="font-medium">{files[0].title ?? id}</span>：{files.map((f) => f.filename).join('、')}
                  </li>
                ))}
                {dupGroups.size > 6 && <li>…还有 {dupGroups.size - 6} 组</li>}
              </ul>
              <p className="mt-1.5 text-xs text-red-200/70">一键处理会保留最近修改的那个，其余改名为 .disabled（不会删除，可以随时重新启用）。</p>
            </div>
            <PillButton size="sm" tone="danger" disabled={locked} onClick={fixDuplicates}>
              一键处理
            </PillButton>
          </div>
        </div>
      )}
      <div className="flex items-center gap-2 pb-1">
        <div className="relative min-w-0 flex-1">
          <Search className="pointer-events-none absolute top-1/2 left-3.5 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input value={query} onChange={(e) => setQuery(e.target.value)} placeholder={`在 ${mods.length} 个${label}中查找`} className="pl-10" />
        </div>
        {pending.length > 0 ? (
          <PillButton size="sm" disabled={locked} icon={<ArrowUpCircle className="size-4" />} onClick={() => applyUpdates(pending)}>
            全部更新（{pending.length}）
          </PillButton>
        ) : (
          <PillButton size="sm" tone="soft" disabled={scanning || locked} icon={<RefreshCw className={`size-4 ${scanning ? 'animate-spin' : ''}`} />} onClick={scan}>
            {scanning ? '检查中…' : '检查更新'}
          </PillButton>
        )}
      </div>
      <div className="px-1 pb-1 text-xs text-muted-foreground">
        {kind === 'mod' ? `${mods.length} 个模组 · ${mods.filter((m) => m.enabled).length} 个已启用` : `${mods.length} 个${label}`}
        {q && ` · 找到 ${shown.length} 个`}
        {updates && pending.length === 0 && ' · 已是最新'}
      </div>
      {shown.length === 0 && <div className="py-8 text-center text-sm text-muted-foreground">没有匹配「{query}」的{label}</div>}
      {shown.map((m, idx) => (
        <motion.div key={m.filename} variants={rise} initial="hidden" animate="show" custom={idx}>
          <Bezel radius={24}>
            <div className={`flex items-center gap-4 px-3.5 py-2.5 transition-opacity duration-500 ${m.enabled ? '' : 'opacity-55'}`}>
              <ModIcon url={m.icon_url} />
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  {m.project_id ? (
                    <button type="button" onClick={() => onOpen({ project_id: m.project_id!, slug: m.project_id!, title: m.title ?? m.filename, description: '', icon_url: m.icon_url, downloads: 0, author: '' })} className="truncate text-left font-semibold transition-colors duration-300 hover:text-brand">
                      {m.title ?? m.filename}
                    </button>
                  ) : (
                    <span className="truncate font-semibold">{m.title ?? m.filename}</span>
                  )}
                  {m.duplicate && <span className="shrink-0 rounded-full bg-red-500/20 px-2 py-0.5 text-[11px] font-medium text-red-300">重复</span>}
                  {!m.user_installed && <span className="shrink-0 rounded-full bg-white/10 px-2 py-0.5 text-[11px] text-muted-foreground">依赖</span>}
                </div>
                <div className="mt-0.5 truncate text-xs text-muted-foreground">
                  {m.version_number ? `${m.version_number} · ` : ''}
                  {m.filename} · {formatBytes(m.size)}
                </div>
              </div>
              {updates?.[m.filename] && (
                <PillButton size="sm" tone="soft" disabled={locked} icon={<ArrowUpCircle className="size-4" />} onClick={() => applyUpdates([updates[m.filename]])}>
                  {updates[m.filename].new_version}
                </PillButton>
              )}
              {kind === 'mod' && <PixelSwitch checked={m.enabled} disabled={locked} label={m.enabled ? `禁用 ${m.title ?? m.filename}` : `启用 ${m.title ?? m.filename}`} onCheckedChange={(v) => toggle(m, v)} />}
              <button
                type="button"
                aria-label="删除模组"
                disabled={locked}
                onClick={() => setRemoving(m)}
                className="grid size-9 place-items-center rounded-full text-muted-foreground transition-[background,color,transform] duration-500 [transition-timing-function:var(--ease)] hover:bg-red-500/15 hover:text-red-300 active:scale-90 disabled:cursor-not-allowed disabled:opacity-40"
              >
                <Trash2 className="size-4" />
              </button>
            </div>
          </Bezel>
        </motion.div>
      ))}

      <Dialog open={missing !== null} onOpenChange={(o) => !o && !fixing && setMissing(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>缺少 {missing?.length} 个前置模组</DialogTitle>
            <DialogDescription>已安装的模组需要下面这些前置，但没有找到。是否一并安装？</DialogDescription>
          </DialogHeader>
          <ul className="space-y-1.5">
            {missing?.map((m) => (
              <li key={m.project_id} className="flex items-center gap-3 rounded-2xl bg-black/20 px-3 py-2 text-sm">
                <ModIcon url={m.icon_url} />
                <div className="min-w-0 flex-1">
                  <div className="truncate font-medium">{m.title}</div>
                  <div className="truncate text-xs text-muted-foreground">{m.needed_by.join('、')} 需要</div>
                </div>
              </li>
            ))}
          </ul>
          <DialogFooter>
            <PillButton tone="ghost" disabled={fixing} onClick={() => setMissing(null)}>
              暂不
            </PillButton>
            <PillButton icon={<DownloadIcon className="size-4" />} disabled={fixing} onClick={installMissing}>
              {fixing ? '准备中…' : '一并安装'}
            </PillButton>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={removing !== null} onOpenChange={(o) => !o && setRemoving(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>删除「{removing?.title ?? removing?.filename}」？</DialogTitle>
            <DialogDescription>只会删除这一个文件（{removing?.filename}）{kind === 'mod' ? '，不会处理依赖它的其他模组' : ''}。</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <PillButton tone="ghost" onClick={() => setRemoving(null)}>
              取消
            </PillButton>
            <PillButton
              tone="danger"
              onClick={async () => {
                const m = removing
                setRemoving(null)
                if (!m) return
                try {
                  await api.contentRemove(inst.id, kind, m.filename)
                  toast.success('已删除')
                  await reload()
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

// ------------------------------------------------------------------- browse

type PlanDialogState = { hit: ModHit; plan: InstallPlan } | { hit: ModHit; error: string }

function PlanDialog({ state, onClose, onConfirm }: { state: PlanDialogState; onClose: () => void; onConfirm: (plan: InstallPlan) => void }) {
  const failed = 'error' in state
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{failed ? `无法安装「${state.hit.title}」` : `安装「${state.hit.title}」`}</DialogTitle>
          <DialogDescription>{failed ? state.error : state.plan.files.some((f) => f.required_by) ? `「${state.hit.title}」还需要下面的前置模组，是否一并安装？确认后才会开始下载。` : '将下载以下文件，确认后才会开始。'}</DialogDescription>
        </DialogHeader>
        {!failed && (
          <div className="space-y-3">
            <ul className="space-y-1.5">
              {state.plan.files.map((f) => (
                <li key={f.project_id} className="flex items-center gap-3 rounded-2xl bg-black/20 px-3 py-2 text-sm">
                  <div className="min-w-0 flex-1">
                    <div className="truncate font-medium">
                      {f.title} <span className="font-normal text-muted-foreground">{f.version_number}</span>
                    </div>
                    {f.required_by && <div className="text-xs text-muted-foreground">{f.required_by} 需要</div>}
                  </div>
                  <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{formatBytes(f.size)}</span>
                </li>
              ))}
            </ul>
            {state.plan.warnings.map((w) => (
              <p key={w} className="rounded-2xl border border-amber-400/25 bg-amber-400/10 px-3 py-2 text-xs text-amber-200">
                {w}
              </p>
            ))}
            <div className="px-1 text-xs text-muted-foreground">
              共 {state.plan.files.length} 个文件 · {formatBytes(state.plan.total_bytes)}
            </div>
          </div>
        )}
        <DialogFooter>
          <PillButton tone="ghost" onClick={onClose}>
            {failed ? '关闭' : '取消'}
          </PillButton>
          {!failed && (
            <PillButton icon={<DownloadIcon className="size-4" />} onClick={() => onConfirm(state.plan)}>
              安装
            </PillButton>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function BrowseTab({ inst, kind, installedIds, locked, planningId, onInstall, onOpen }: { inst: Instance | null; kind: ContentKind; installedIds: Set<string>; locked: boolean; planningId: string | null; onInstall: (hit: ModHit) => void; onOpen: (hit: ModHit) => void }) {
  const [q, setQ] = useState('')
  const [term, setTerm] = useState('')
  const [hits, setHits] = useState<ModHit[]>([])
  const [total, setTotal] = useState(0)
  const [loading, setLoading] = useState(true)
  const [moreLoading, setMoreLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [attempt, setAttempt] = useState(0)
  const [sort, setSort] = useState<Sort>('relevance')
  const [category, setCategory] = useState<string | null>(null)
  const generation = useRef(0)
  const opts: SearchOpts = { sort, category: category ?? undefined }
  const search = (offset: number) => (inst ? api.contentSearch(inst.id, kind, term, offset, opts) : api.contentSearchAll(kind, term, offset, opts))

  useEffect(() => {
    const t = setTimeout(() => setTerm(q.trim()), 400)
    return () => clearTimeout(t)
  }, [q])

  useEffect(() => {
    const mine = ++generation.current
    setLoading(true)
    setError(null)
    search(0).then(
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
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [inst?.id, kind, term, attempt, sort, category])

  const loadMore = async () => {
    const mine = generation.current
    setMoreLoading(true)
    try {
      const r = await search(hits.length)
      if (mine === generation.current) setHits((h) => [...h, ...r.hits])
    } catch (e) {
      toast.error(msg(e))
    } finally {
      setMoreLoading(false)
    }
  }

  return (
    <div className="pb-4">
      <div className="mb-3 flex flex-wrap items-center gap-3">
        <div className="relative min-w-[220px] max-w-sm flex-1">
          <Search className="pointer-events-none absolute top-1/2 left-4 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input value={q} onChange={(e) => setQ(e.target.value)} placeholder={SEARCH_HINT[kind]} className="h-10 rounded-full pl-10" />
        </div>
        <Segmented<Sort> value={sort} onChange={setSort} options={SORTS} />
      </div>
      <div className="mb-4 flex items-center gap-1.5 overflow-x-auto pb-1" role="group" aria-label="按分类筛选">
        {[[null, '全部分类'] as const, ...CATEGORIES[kind]].map(([id, label]) => {
          const on = category === id
          return (
            <button
              key={id ?? 'all'}
              type="button"
              aria-pressed={on}
              onClick={() => setCategory(id)}
              className={`shrink-0 rounded-full px-3 py-1 text-xs transition-[background,color] duration-300 ${on ? 'bg-brand/18 font-medium text-brand shadow-[inset_0_0_0_1px_color-mix(in_oklch,var(--brand)_40%,transparent)]' : 'bg-white/6 text-muted-foreground hover:bg-white/10 hover:text-foreground'}`}
            >
              {label}
            </button>
          )
        })}
      </div>

      {error ? (
        <Empty sprite="tnt" title="搜索失败" hint={error} action={<PillButton onClick={() => setAttempt((n) => n + 1)}>重试</PillButton>} />
      ) : loading ? (
        <div className="space-y-2">
          {Array.from({ length: 5 }, (_, i) => (
            <div key={i} className="h-[76px] animate-pulse rounded-[24px] bg-white/5" style={{ animationDelay: `${i * 90}ms` }} />
          ))}
        </div>
      ) : hits.length === 0 ? (
        <Empty sprite="diamond" title="没有找到模组" hint={category ? '这个分类下没有结果，换个分类或关键词试试。' : inst ? `换个关键词试试；只显示适配 ${inst.mc_version ?? inst.version_id} 的内容。` : '换个关键词试试。'} />
      ) : (
        <div className="space-y-2">
          {hits.map((h, idx) => {
            const installed = installedIds.has(h.project_id)
            return (
              <motion.div key={h.project_id} variants={rise} initial="hidden" animate="show" custom={idx % 8}>
                <Bezel radius={24}>
                  <div className="flex items-center gap-4 px-3.5 py-3">
                    <button type="button" onClick={() => onOpen(h)} className="flex min-w-0 flex-1 items-center gap-4 text-left" aria-label={`查看「${h.title}」详情`}>
                      <ModIcon url={h.icon_url} />
                      <div className="min-w-0 flex-1">
                        <div className="flex items-baseline gap-2">
                          <span className="truncate font-semibold">{h.title}</span>
                          <span className="shrink-0 text-xs text-muted-foreground">
                            {h.author} · {formatCount(h.downloads)} 次下载
                          </span>
                        </div>
                        <div className="mt-0.5 line-clamp-2 text-sm text-muted-foreground">{h.description}</div>
                      </div>
                    </button>
                    <PillButton size="sm" tone={installed ? 'ghost' : 'brand'} disabled={installed || locked || planningId !== null} onClick={() => onInstall(h)}>
                      {installed ? '已安装' : planningId === h.project_id ? '查询中…' : '安装'}
                    </PillButton>
                  </div>
                </Bezel>
              </motion.div>
            )
          })}
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
  )
}

// ------------------------------------------------------------ scope + picker

type Scope = 'all' | string

/** "Fabric" / "原版": what an instance can run, for the chips and the picker. */
function instanceKind(i: Instance) {
  return i.loader ? LOADER_LABEL[i.loader.kind] : '原版'
}

/** Which instance the page is about: everything, or one instance (its loader and version decide what fits). */
function ScopeBar({ instances, scope, onChange }: { instances: Instance[]; scope: Scope; onChange: (s: Scope) => void }) {
  const chip = (value: Scope, label: string, sub?: string) => {
    const active = scope === value
    return (
      <button
        key={value}
        type="button"
        onClick={() => onChange(value)}
        className={`flex shrink-0 items-center gap-2 rounded-full px-3.5 py-1.5 text-sm transition-[background,color] duration-500 [transition-timing-function:var(--ease)] ${active ? 'bg-brand/16 text-foreground shadow-[inset_0_0_0_1px_color-mix(in_oklch,var(--brand)_40%,transparent)]' : 'bg-white/6 text-muted-foreground hover:bg-white/10 hover:text-foreground'}`}
      >
        <span className="max-w-[160px] truncate font-medium">{label}</span>
        {sub && <span className={`rounded-full px-1.5 py-px text-[10px] font-medium ${active ? 'bg-brand/20 text-brand' : 'bg-white/8'}`}>{sub}</span>}
      </button>
    )
  }
  return (
    <div className="mb-4 flex shrink-0 items-center gap-2 overflow-x-auto pb-1" role="tablist" aria-label="选择实例">
      {chip('all', '全部', '不限版本')}
      {instances.map((i) => chip(i.id, i.name, `${instanceKind(i)} · ${i.mc_version ?? i.version_id}`))}
    </div>
  )
}

/** When browsing everything: which instance should the mod go to? */
function PickInstanceDialog({ hit, instances, kind, onClose, onPick }: { hit: ModHit; instances: Instance[]; kind: ContentKind; onClose: () => void; onPick: (id: string) => void }) {
  const usable = (i: Instance) => kind === 'resourcepack' || !!i.loader
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>把「{hit.title}」安装到哪个实例？</DialogTitle>
          <DialogDescription>选好后会检查它是否适配该实例的游戏版本和加载器，并列出需要一并安装的前置。</DialogDescription>
        </DialogHeader>
        {instances.length === 0 ? (
          <p className="text-sm text-muted-foreground">还没有实例，先去「下载」页创建一个。</p>
        ) : (
          <ul className="max-h-72 space-y-1.5 overflow-auto">
            {instances.map((i) => (
              <li key={i.id}>
                <button
                  type="button"
                  disabled={!usable(i)}
                  onClick={() => onPick(i.id)}
                  className="flex w-full items-center gap-3 rounded-2xl bg-black/20 px-3 py-2.5 text-left transition-colors duration-300 hover:bg-white/10 disabled:cursor-not-allowed disabled:opacity-45"
                >
                  <div className="min-w-0 flex-1">
                    <div className="truncate font-medium">{i.name}</div>
                    <div className="text-xs text-muted-foreground">
                      {instanceKind(i)} · Minecraft {i.mc_version ?? i.version_id}
                      {!usable(i) && ' · 原版实例不能装这个'}
                    </div>
                  </div>
                </button>
              </li>
            ))}
          </ul>
        )}
        <DialogFooter>
          <PillButton tone="ghost" onClick={onClose}>
            取消
          </PillButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

/** "全部" on the installed tab: what each instance has, as a way into one of them. */
function AllInstalled({ instances, kind, onPick }: { instances: Instance[]; kind: ContentKind; onPick: (id: string) => void }) {
  const [counts, setCounts] = useState<Record<string, { n: number; dup: number } | null>>({})
  useEffect(() => {
    let live = true
    for (const i of instances) {
      api.contentList(i.id, kind).then(
        (l) => live && setCounts((c) => ({ ...c, [i.id]: { n: l.length, dup: l.filter((m) => m.duplicate).length } })),
        () => live && setCounts((c) => ({ ...c, [i.id]: null })),
      )
    }
    return () => {
      live = false
    }
  }, [instances, kind])

  if (instances.length === 0) return <Empty sprite="chest" title="还没有实例" hint="先去「下载」页创建一个实例。" />
  return (
    <div className="space-y-2 pb-4">
      <div className="px-1 pb-1 text-xs text-muted-foreground">选一个实例查看和管理它的{KIND_LABEL[kind]}</div>
      {instances.map((i, idx) => {
        const c = counts[i.id]
        return (
          <motion.div key={i.id} variants={rise} initial="hidden" animate="show" custom={idx}>
            <Bezel radius={24}>
              <button type="button" onClick={() => onPick(i.id)} className="flex w-full items-center gap-4 px-4 py-3 text-left transition-colors duration-300 hover:bg-white/4">
                <div className="min-w-0 flex-1">
                  <div className="truncate font-semibold">{i.name}</div>
                  <div className="text-xs text-muted-foreground">
                    {instanceKind(i)} · Minecraft {i.mc_version ?? i.version_id}
                  </div>
                </div>
                {c === undefined ? (
                  <span className="text-xs text-muted-foreground">读取中…</span>
                ) : c === null ? (
                  <span className="text-xs text-muted-foreground">不适用</span>
                ) : (
                  <div className="flex items-center gap-2 text-sm">
                    <span className="text-muted-foreground">{c.n} 个</span>
                    {c.dup > 0 && <span className="rounded-full bg-red-500/20 px-2 py-0.5 text-[11px] font-medium text-red-300">{c.dup} 个重复</span>}
                  </div>
                )}
              </button>
            </Bezel>
          </motion.div>
        )
      })}
    </div>
  )
}

// --------------------------------------------------------------------- page

export function ModsPage() {
  const snap = useStore((s) => s.snap)
  const run = useStore((s) => s.run)
  const setPage = useStore((s) => s.setPage)
  const cancel = useStore((s) => s.cancel)
  const modsInstall = useStore((s) => s.modsInstall)
  const [tab, setTab] = useState<Tab>('installed')
  const [kind, setKind] = useState<ContentKind>('mod')
  const [mods, setMods] = useState<InstalledMod[] | null>(null)
  const [pickedScope, setScope] = useState<Scope | null>(null)
  const [detail, setDetail] = useState<ModHit | null>(null)
  const [picking, setPicking] = useState<ModHit | null>(null)
  const [planning, setPlanning] = useState<string | null>(null)
  const [dialog, setDialog] = useState<(PlanDialogState & { instanceId: string }) | null>(null)

  const instances = snap?.instances ?? []
  const selected = selectedInstance(snap)
  const scope: Scope = pickedScope && (pickedScope === 'all' || instances.some((i) => i.id === pickedScope)) ? pickedScope : (selected?.id ?? 'all')
  const inst = scope === 'all' ? null : (instances.find((i) => i.id === scope) ?? null)
  // Mods and shaders need the instance's loader; resource packs work anywhere.
  const usable = !!inst && (kind === 'resourcepack' || !!inst.loader)
  const instId = usable ? inst.id : null

  const reload = useCallback(async () => {
    if (!instId) return
    try {
      setMods(await api.contentList(instId, kind))
    } catch (e) {
      toast.error(msg(e))
    }
  }, [instId, kind])

  useEffect(() => {
    setMods(null)
    reload()
  }, [reload])

  const changeScope = (s: Scope) => {
    setScope(s)
    setDetail(null)
  }

  const kindSwitch = (
    <Segmented<ContentKind>
      value={kind}
      onChange={(k) => {
        setKind(k)
        setTab('installed')
        setDetail(null)
      }}
      options={[
        { value: 'mod', label: '模组' },
        { value: 'shader', label: '光影' },
        { value: 'resourcepack', label: '资源包' },
      ]}
    />
  )

  const busy = run.phase !== 'idle'
  const locked = !!inst?.running || busy
  const installing = run.kind === 'install' && run.phase === 'working'
  const installedIds = new Set((mods ?? []).flatMap((m) => (m.project_id ? [m.project_id] : [])))

  // Reload even on failure: files that arrived before the error are installed.
  // A shader plan may also bring Iris and Sodium (mods), so the list shown is refreshed either way.
  const install = async (plan: InstallPlan, instanceId: string = inst?.id ?? '') => {
    await modsInstall(instanceId, plan)
    if (instanceId === instId) await reload()
  }

  /** Ask what installing `hit` into `instanceId` would take, prerequisites included, then let the user confirm. */
  const requestInstall = async (hit: ModHit, instanceId: string) => {
    setPlanning(hit.project_id)
    try {
      setDialog({ hit, plan: await api.contentPlan(instanceId, kind, hit.project_id), instanceId })
    } catch (e) {
      setDialog({ hit, error: msg(e), instanceId })
    } finally {
      setPlanning(null)
    }
  }

  const onInstallClick = (hit: ModHit) => (inst ? requestInstall(hit, inst.id) : setPicking(hit))

  const installMissingFor = async (hit: ModHit, ids: string[]) => {
    if (!inst) return
    setPlanning(hit.project_id)
    try {
      const plan = await api.contentPlanMissing(inst.id, ids)
      setDialog({ hit: { ...hit, title: `${hit.title} 的前置` }, plan, instanceId: inst.id })
    } catch (e) {
      toast.error(msg(e))
    } finally {
      setPlanning(null)
    }
  }

  const scopeBar = <ScopeBar instances={instances} scope={scope} onChange={changeScope} />

  const banner = (subtitle: string, actions: React.ReactNode) => <PageBanner biome="nether" seed="mods" eyebrow="Content" title="内容" subtitle={subtitle} actions={actions} />

  const planDialog = dialog && (
    <PlanDialog
      state={dialog}
      onClose={() => setDialog(null)}
      onConfirm={(plan) => {
        const id = dialog.instanceId
        setDialog(null)
        install(plan, id)
      }}
    />
  )
  const pickDialog = picking && (
    <PickInstanceDialog
      hit={picking}
      instances={instances}
      kind={kind}
      onClose={() => setPicking(null)}
      onPick={(id) => {
        const hit = picking
        setPicking(null)
        requestInstall(hit, id)
      }}
    />
  )

  const progress = installing && (
    <motion.div initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.5, ease: EASE }} className="mb-4">
      <Bezel radius={26}>
        <div className="flex items-center gap-4 p-4">
          <div className="min-w-0 flex-1">
            <RunProgress stage={run.stage} progress={run.progress} />
          </div>
          <PillButton size="sm" tone="soft" onClick={cancel}>
            取消
          </PillButton>
        </div>
      </Bezel>
    </motion.div>
  )

  let body: React.ReactNode
  if (detail) {
    body = (
      <ProjectDetailView
        hit={detail}
        kind={kind}
        inst={inst}
        installed={installedIds.has(detail.project_id)}
        locked={locked}
        onBack={() => setDetail(null)}
        onInstall={onInstallClick}
        onInstallMissing={installMissingFor}
      />
    )
  } else if (scope === 'all') {
    body =
      tab === 'installed' ? (
        <AllInstalled instances={instances} kind={kind} onPick={(id) => changeScope(id)} />
      ) : (
        <BrowseTab key={`all:${kind}`} inst={null} kind={kind} installedIds={installedIds} locked={locked} planningId={planning} onInstall={onInstallClick} onOpen={setDetail} />
      )
  } else if (!inst || !usable) {
    body = (
      <Empty
        sprite="chest"
        title={inst ? `「${inst.name}」是原版，装${KIND_LABEL[kind]}需要加载器` : '还没有实例'}
        hint={`先创建一个 Fabric、Quilt、Forge 或 NeoForge 实例，再来安装${KIND_LABEL[kind]}。`}
        action={<PillButton onClick={() => setPage('download')}>去创建实例</PillButton>}
      />
    )
  } else {
    body =
      tab === 'installed' ? (
        <InstalledTab inst={inst} kind={kind} mods={mods} locked={locked} reload={reload} goBrowse={() => setTab('browse')} onInstall={(plan) => install(plan, inst.id)} onOpen={setDetail} />
      ) : (
        <BrowseTab key={`${inst.id}:${kind}`} inst={inst} kind={kind} installedIds={installedIds} locked={locked} planningId={planning} onInstall={onInstallClick} onOpen={setDetail} />
      )
  }

  const tabSwitch = (
    <Segmented<Tab>
      value={tab}
      onChange={(t) => {
        setTab(t)
        setDetail(null)
      }}
      options={[
        { value: 'installed', label: '已安装' },
        { value: 'browse', label: '浏览' },
      ]}
    />
  )
  const subtitle = inst ? `${inst.name} · ${inst.loader ? `${LOADER_LABEL[inst.loader.kind]} ${inst.loader.version}` : '原版'} · Minecraft ${inst.mc_version ?? inst.version_id}` : '浏览全部内容，或选择一个实例来管理它'

  return (
    <div className="flex h-full min-h-0 flex-col">
      {banner(
        subtitle,
        <div className="flex flex-col items-end gap-2">
          {kindSwitch}
          {tabSwitch}
        </div>,
      )}
      {scopeBar}
      {progress}
      {inst?.running && <p className="mb-3 px-1 text-xs text-amber-300">游戏正在运行，请先关闭游戏再修改模组。</p>}
      <div className="min-h-0 flex-1 overflow-auto pr-1">{body}</div>
      {planDialog}
      {pickDialog}
    </div>
  )
}
