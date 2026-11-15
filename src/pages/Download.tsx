import { useEffect, useMemo, useState } from 'react'
import { motion } from 'motion/react'
import { ArrowUpRight, CloudOff, Download as DownloadIcon, RefreshCw, Search, Sparkles } from 'lucide-react'
import { api, type LoaderKind, type LoaderVersion, type Recommended, type VersionDto } from '@/lib/api'
import { EASE, rise } from '@/lib/motion'
import { useStore } from '@/lib/store'
import { PixelSprite } from '@/components/art'
import { Bezel, PillButton } from '@/components/Bezel'
import { Empty, Field, KindBadge, LOADER_LABEL, PageBanner, Segmented } from '@/components/kit'
import { RunProgress } from '@/components/RunProgress'
import { ModpackBrowser } from '@/pages/Modpacks'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { PixelCheckbox, PixelSelect } from '@/components/ui/pixel'

type Filter = 'release' | 'snapshot' | 'all'

type Choice = 'vanilla' | LoaderKind
type KindState = { status: 'loading' } | { status: 'error'; message: string } | { status: 'ready'; versions: LoaderVersion[] }

const LOADERS: LoaderKind[] = ['fabric', 'quilt', 'forge', 'neoforge']
const perLoader = <T,>(value: T) => Object.fromEntries(LOADERS.map((k) => [k, value])) as Record<LoaderKind, T>
const FORGE_SUPPORT_URL = 'https://www.patreon.com/LexManos/'

function InstallDialog({ version, onClose }: { version: VersionDto; onClose: () => void }) {
  const mutate = useStore((s) => s.mutate)
  const installInstance = useStore((s) => s.installInstance)
  const setPage = useStore((s) => s.setPage)
  const [choice, setChoice] = useState<Choice>('vanilla')
  const [states, setStates] = useState<Record<LoaderKind, KindState>>(() => perLoader<KindState>({ status: 'loading' }))
  const [picked, setPicked] = useState<Record<LoaderKind, string | null>>(() => perLoader<string | null>(null))
  const [showBeta, setShowBeta] = useState(false)
  const [typedName, setTypedName] = useState<string | null>(null)
  const [recs, setRecs] = useState<Partial<Record<LoaderKind, Recommended[] | 'loading'>>>({})
  const [ticked, setTicked] = useState<Record<string, boolean>>({})

  // Mods worth installing alongside, asked once per loader after its versions are known.
  useEffect(() => {
    if (choice === 'vanilla' || states[choice].status !== 'ready' || recs[choice]) return
    setRecs((r) => ({ ...r, [choice]: 'loading' }))
    api.recommendedMods(choice, version.id).then(
      (list) => {
        setRecs((r) => ({ ...r, [choice]: list }))
        setTicked((t) => ({ ...t, ...Object.fromEntries(list.map((x) => [`${choice}:${x.slug}`, x.default_on])) }))
      },
      () => setRecs((r) => ({ ...r, [choice]: [] })),
    )
  }, [choice, states, recs, version.id])

  // Ask each loader which of its versions run this Minecraft version (an empty list = unsupported).
  useEffect(() => {
    let alive = true
    for (const kind of LOADERS) {
      api.listLoaderVersions(kind, version.id).then(
        (versions) => alive && setStates((s) => ({ ...s, [kind]: { status: 'ready', versions } })),
        (e) => alive && setStates((s) => ({ ...s, [kind]: { status: 'error', message: e instanceof Error ? e.message : String(e) } })),
      )
    }
    return () => {
      alive = false
    }
  }, [version.id])

  const current = choice === 'vanilla' ? null : states[choice]
  const all = current?.status === 'ready' ? current.versions : []
  const stableOnly = all.filter((v) => v.stable)
  const shown = showBeta || stableOnly.length === 0 ? all : stableOnly
  const chosenVersion = choice === 'vanilla' ? null : (shown.find((v) => v.version === picked[choice])?.version ?? shown[0]?.version ?? null)

  const recList = choice === 'vanilla' ? undefined : recs[choice]
  const extras = Array.isArray(recList) ? recList.filter((r) => ticked[`${choice}:${r.slug}`]).map((r) => r.slug) : []

  const name = typedName ?? (choice === 'vanilla' ? `Minecraft ${version.id}` : `${LOADER_LABEL[choice]} ${version.id}`)
  const valid = name.trim().length > 0 && name.trim().length <= 40 && (choice === 'vanilla' || chosenVersion !== null)

  const options = [
    { value: 'vanilla' as Choice, label: '原版' },
    ...LOADERS.map((kind) => {
      const st = states[kind]
      const unsupported = st.status === 'ready' && st.versions.length === 0
      return {
        value: kind as Choice,
        label: LOADER_LABEL[kind],
        disabled: st.status !== 'ready' || unsupported,
        hint: st.status === 'loading' ? '正在查询…' : st.status === 'error' ? st.message : unsupported ? `${LOADER_LABEL[kind]} 不支持 ${version.id}` : undefined,
      }
    }),
  ]
  const kindStates = LOADERS.map((k) => states[k])
  const note = kindStates.some((s) => s.status === 'loading')
    ? '正在查询可用的加载器…'
    : kindStates.every((s) => s.status === 'error')
      ? '加载器信息获取失败，可先创建原版实例。'
      : kindStates.every((s) => s.status === 'ready' && s.versions.length === 0)
        ? `${version.id} 暂时没有可用的加载器。`
        : null

  const create = async (download: boolean) => {
    if (!valid) return
    const loader = choice === 'vanilla' || !chosenVersion ? null : { kind: choice, version: chosenVersion }
    if (!(await mutate(() => api.createInstance(name.trim(), version.id, loader), '实例已创建'))) return
    onClose()
    if (download) {
      const id = useStore.getState().snap?.settings.selected_instance
      if (id) installInstance(id, extras)
    } else {
      setPage('instances')
    }
  }

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[88vh] overflow-y-auto sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>创建实例</DialogTitle>
          <DialogDescription>
            Minecraft {version.id} · {version.release_time.slice(0, 10)}
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <Field label="实例名称">
            <Input value={name} onChange={(e) => setTypedName(e.target.value)} maxLength={40} autoFocus onKeyDown={(e) => e.key === 'Enter' && create(true)} />
          </Field>
          <Field label="加载器" hint={note ?? undefined}>
            <Segmented<Choice> value={choice} onChange={setChoice} options={options} />
          </Field>
          {choice !== 'vanilla' && shown.length > 0 && (
            <Field label={`${LOADER_LABEL[choice]} 版本`}>
              <PixelSelect
                aria-label={`${LOADER_LABEL[choice]} 版本`}
                value={chosenVersion}
                onValueChange={(v) => setPicked((p) => ({ ...p, [choice]: v }))}
                options={shown.map((v) => ({ value: v.version, label: v.version, tag: v.stable ? undefined : '测试版' }))}
              />
              {stableOnly.length > 0 && stableOnly.length < all.length && (
                <PixelCheckbox checked={showBeta} onCheckedChange={setShowBeta} className="mt-2 items-center gap-2 px-1 text-xs text-muted-foreground">
                  显示测试版
                </PixelCheckbox>
              )}
            </Field>
          )}
          {choice !== 'vanilla' && recList === 'loading' && <p className="px-1 text-xs text-muted-foreground">正在查询推荐模组…</p>}
          {choice !== 'vanilla' && Array.isArray(recList) && recList.length > 0 && (
            <Field label="推荐模组" hint="点“创建并下载”时自动装好，依赖会一起补齐">
              <div className="space-y-1.5">
                {recList.map((r) => {
                  const key = `${choice}:${r.slug}`
                  return (
                    <PixelCheckbox key={key} checked={!!ticked[key]} onCheckedChange={(v) => setTicked((t) => ({ ...t, [key]: v }))} className="items-center rounded-2xl bg-black/20 px-3 py-2.5 transition-colors duration-500 [transition-timing-function:var(--ease)] hover:bg-black/30">
                      <span className="min-w-0">
                        <span className="text-sm font-medium">{r.title}</span>
                        <span className="block text-xs text-muted-foreground">{r.why}</span>
                      </span>
                    </PixelCheckbox>
                  )
                })}
              </div>
            </Field>
          )}
          {(choice === 'forge' || choice === 'neoforge') && (
            <p className="rounded-2xl bg-black/20 px-3 py-2 text-xs leading-relaxed text-muted-foreground">
              {LOADER_LABEL[choice]} 首次安装需要运行它的安装步骤，可能要几分钟。
              {choice === 'forge' && (
                <>
                  {' '}
                  Forge 官方希望自动安装时赞助项目，
                  <button type="button" onClick={() => api.openExternal(FORGE_SUPPORT_URL)} className="text-brand underline-offset-2 hover:underline">
                    了解如何支持
                  </button>
                  。
                </>
              )}
            </p>
          )}
          <p className="text-xs text-muted-foreground">游戏文件由所有实例共享，不会重复下载。</p>
        </div>
        <DialogFooter>
          <PillButton tone="ghost" onClick={() => create(false)} disabled={!valid}>
            仅创建
          </PillButton>
          <PillButton icon={<DownloadIcon className="size-4" />} onClick={() => create(true)} disabled={!valid}>
            创建并下载
          </PillButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

export function DownloadPage() {
  const versions = useStore((s) => s.versions)
  const error = useStore((s) => s.versionsError)
  const load = useStore((s) => s.loadVersions)
  const run = useStore((s) => s.run)
  const cancel = useStore((s) => s.cancel)
  const [filter, setFilter] = useState<Filter>('release')
  const [q, setQ] = useState('')
  const [picked, setPicked] = useState<VersionDto | null>(null)
  const [mode, setMode] = useState<'versions' | 'modpacks'>('versions')

  useEffect(() => {
    load()
  }, [load])

  const list = useMemo(() => {
    const all = versions?.versions ?? []
    const term = q.trim().toLowerCase()
    return all.filter((v) => (filter === 'all' || v.kind === filter) && (!term || v.id.toLowerCase().includes(term)))
  }, [versions, filter, q])

  const latest = versions?.versions.find((v) => v.id === versions.latest_release)

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageBanner
        biome="desert"
        seed="download"
        eyebrow="Download"
        title="下载"
        subtitle={mode === 'versions' ? '选择一个版本创建实例。默认使用 BMCLAPI 镜像，不通时自动回退官方源。' : '搜索 Modrinth 上的整合包，一键装好游戏版本、加载器和全部模组。'}
        actions={
          <>
            <Segmented<'versions' | 'modpacks'>
              value={mode}
              onChange={setMode}
              options={[
                { value: 'versions', label: '游戏版本' },
                { value: 'modpacks', label: '整合包' },
              ]}
            />
            {mode === 'versions' && (
              <>
            <button type="button" aria-label="刷新" onClick={() => load(true)} className="grid size-10 place-items-center rounded-full bg-black/30 text-foreground/80 transition-[background,transform] duration-500 [transition-timing-function:var(--ease)] hover:bg-black/45 active:scale-90">
              <RefreshCw className="size-4" />
            </button>
            {latest && (
              <PillButton icon={<ArrowUpRight className="size-4" />} onClick={() => setPicked(latest)}>
                最新 {latest.id}
              </PillButton>
            )}
              </>
            )}
          </>
        }
      />

      {run.kind === 'install' && run.phase === 'working' && (
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
      )}

      {mode === 'modpacks' ? (
        <ModpackBrowser />
      ) : (
        <>
      <div className="mb-4 flex items-center gap-3">
        <div className="relative max-w-xs flex-1">
          <Search className="pointer-events-none absolute top-1/2 left-4 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input value={q} onChange={(e) => setQ(e.target.value)} placeholder="搜索版本，例如 1.21" className="h-10 rounded-full pl-10" />
        </div>
        <Segmented<Filter>
          value={filter}
          onChange={setFilter}
          options={[
            { value: 'release', label: '正式版' },
            { value: 'snapshot', label: '快照' },
            { value: 'all', label: '全部' },
          ]}
        />
      </div>

      <div className="min-h-0 flex-1 overflow-auto pr-1">
        {error ? (
          <Empty sprite="tnt" title="无法获取版本列表" hint={error} action={<PillButton onClick={() => load(true)}>重试</PillButton>} />
        ) : !versions ? (
          <div className="space-y-2">
            {Array.from({ length: 7 }, (_, i) => (
              <div key={i} className="h-[66px] animate-pulse rounded-[22px] bg-white/5" style={{ animationDelay: `${i * 90}ms` }} />
            ))}
            <div className="flex items-center justify-center gap-2 pt-3 text-xs text-muted-foreground">
              <CloudOff className="size-3.5" />
              正在获取版本列表…镜像不通时会自动改用官方源
            </div>
          </div>
        ) : list.length === 0 ? (
          <Empty sprite="diamond" title="没有匹配的版本" hint="换个关键词，或切换上方的类型筛选。" />
        ) : (
          <div className="space-y-2 pb-4">
            {list.map((v, idx) => {
              const isLatest = v.id === versions.latest_release
              return (
                <motion.div key={v.id} variants={rise} initial="hidden" animate="show" custom={idx}>
                  <Bezel radius={24} interactive>
                    <button type="button" onClick={() => setPicked(v)} className="group flex w-full items-center gap-4 px-3.5 py-2.5 text-left">
                      <div className="grid size-11 shrink-0 place-items-center rounded-2xl bg-black/25">
                        <PixelSprite name={v.kind === 'release' ? 'grass' : 'tnt'} size={30} />
                      </div>
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-2">
                          <span className="font-semibold">{v.id}</span>
                          <KindBadge kind={v.kind} />
                          {isLatest && (
                            <span className="flex items-center gap-1 rounded-full bg-brand-2/15 px-2 py-0.5 text-[11px] font-medium text-brand-2">
                              <Sparkles className="size-3" />
                              最新
                            </span>
                          )}
                        </div>
                        <div className="mt-0.5 text-xs text-muted-foreground">{v.release_time.slice(0, 10)}</div>
                      </div>
                      <span className="grid size-9 place-items-center rounded-full bg-white/8 text-foreground/70 transition-transform duration-500 [transition-timing-function:var(--ease)] group-hover:-translate-y-px group-hover:translate-x-1 group-hover:scale-105 group-hover:bg-brand/20 group-hover:text-brand">
                        <ArrowUpRight className="size-4" />
                      </span>
                    </button>
                  </Bezel>
                </motion.div>
              )
            })}
          </div>
        )}
      </div>
        </>
      )}

      {picked && <InstallDialog key={picked.id} version={picked} onClose={() => setPicked(null)} />}
    </div>
  )
}
