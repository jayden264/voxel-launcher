import { useEffect, useState } from 'react'
import { motion } from 'motion/react'
import { ArrowLeft, Download as DownloadIcon, ExternalLink, Heart } from 'lucide-react'
import Markdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { toast } from 'sonner'
import { api, type ContentKind, type Instance, type ModHit, type ProjectDetail } from '@/lib/api'
import { EASE } from '@/lib/motion'
import { Bezel, PillButton } from '@/components/Bezel'
import { Empty, Segmented } from '@/components/kit'

type DetailTab = 'about' | 'deps' | 'versions' | 'gallery'

const LOADER_NAME: Record<string, string> = { fabric: 'Fabric', quilt: 'Quilt', forge: 'Forge', neoforge: 'NeoForge', iris: 'Iris', minecraft: '原版', optifine: 'OptiFine' }
const loaderName = (l: string) => LOADER_NAME[l] ?? l

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e))

function count(n: number) {
  if (n >= 1e9) return `${(n / 1e9).toFixed(1)}B`
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)}M`
  if (n >= 1e3) return `${(n / 1e3).toFixed(1)}K`
  return String(n)
}

function Chip({ children, tone = 'plain' }: { children: React.ReactNode; tone?: 'plain' | 'brand' | 'warn' | 'bad' }) {
  const cls = { plain: 'bg-white/8 text-foreground/75', brand: 'bg-brand/14 text-brand', warn: 'bg-amber-400/14 text-amber-300', bad: 'bg-red-500/14 text-red-300' }[tone]
  return <span className={`inline-flex shrink-0 items-center rounded-full px-2.5 py-0.5 text-[11px] font-medium ${cls}`}>{children}</span>
}

function Icon({ url, size = 72 }: { url: string | null; size?: number }) {
  const [broken, setBroken] = useState(false)
  const style = { width: size, height: size }
  if (url && !broken) return <img src={url} alt="" onError={() => setBroken(true)} style={style} className="shrink-0 rounded-3xl bg-black/25 object-cover" />
  return <div style={style} className="shrink-0 rounded-3xl bg-black/25" />
}

/** Modrinth descriptions are Markdown with the odd image and link; raw HTML is dropped. */
function Body({ text }: { text: string }) {
  return (
    <div className="space-y-3 text-sm leading-relaxed text-foreground/85 [&_blockquote]:border-l-2 [&_blockquote]:border-white/20 [&_blockquote]:pl-3 [&_blockquote]:text-muted-foreground [&_code]:rounded [&_code]:bg-black/30 [&_code]:px-1 [&_code]:py-0.5 [&_code]:text-[12px] [&_h1]:text-xl [&_h1]:font-semibold [&_h2]:pt-2 [&_h2]:text-lg [&_h2]:font-semibold [&_h3]:pt-1 [&_h3]:font-semibold [&_hr]:border-white/10 [&_li]:my-0.5 [&_ol]:list-decimal [&_ol]:pl-5 [&_pre]:overflow-auto [&_pre]:rounded-2xl [&_pre]:bg-black/30 [&_pre]:p-3 [&_table]:w-full [&_td]:border [&_td]:border-white/10 [&_td]:px-2 [&_td]:py-1 [&_th]:border [&_th]:border-white/10 [&_th]:px-2 [&_th]:py-1 [&_ul]:list-disc [&_ul]:pl-5">
      <Markdown
        remarkPlugins={[remarkGfm]}
        skipHtml
        components={{
          a: ({ href, children }) => (
            <a
              href={href}
              onClick={(e) => {
                e.preventDefault()
                if (href && /^https?:\/\//.test(href)) api.openExternal(href).catch(() => {})
              }}
              className="text-brand underline-offset-2 hover:underline"
            >
              {children}
            </a>
          ),
          img: ({ src, alt }) => <img src={src} alt={alt ?? ''} loading="lazy" onError={(e) => (e.currentTarget.style.display = 'none')} className="max-w-full rounded-xl" />,
        }}
      >
        {text}
      </Markdown>
    </div>
  )
}

export function ProjectDetailView({
  hit,
  kind,
  inst,
  installed,
  locked,
  onBack,
  onInstall,
  onInstallMissing,
}: {
  hit: ModHit
  kind: ContentKind
  /** The instance the page is for; null when browsing everything. */
  inst: Instance | null
  installed: boolean
  locked: boolean
  onBack: () => void
  onInstall: (hit: ModHit) => void
  /** Install the required prerequisites this installed mod lacks. */
  onInstallMissing: (hit: ModHit, projectIds: string[]) => void
}) {
  const [detail, setDetail] = useState<ProjectDetail | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [tab, setTab] = useState<DetailTab>('about')
  const [attempt, setAttempt] = useState(0)
  // A vanilla instance cannot run mods or shaders, so there is nothing to check against.
  const target = inst && (kind === 'resourcepack' || inst.loader) ? inst.id : null

  useEffect(() => {
    let live = true
    setDetail(null)
    setError(null)
    api.contentDetail(hit.project_id, kind, target).then(
      (d) => live && setDetail(d),
      (e) => live && setError(msg(e)),
    )
    return () => {
      live = false
    }
  }, [hit.project_id, kind, target, attempt])

  const back = (
    <button type="button" onClick={onBack} className="mb-3 flex items-center gap-1.5 text-sm text-muted-foreground transition-colors duration-300 hover:text-foreground">
      <ArrowLeft className="size-4" />
      返回列表
    </button>
  )

  if (error) {
    return (
      <div className="pb-4">
        {back}
        <Empty sprite="tnt" title="加载详情失败" hint={error} action={<PillButton onClick={() => setAttempt((n) => n + 1)}>重试</PillButton>} />
      </div>
    )
  }
  if (!detail) {
    return (
      <div className="pb-4">
        {back}
        <div className="h-40 animate-pulse rounded-[30px] bg-white/5" />
        <div className="mt-3 h-64 animate-pulse rounded-[30px] bg-white/5" />
      </div>
    )
  }

  const d = detail
  const missing = d.dependencies.filter((x) => x.dependency_type === 'required' && !x.installed)
  const incompatible = d.compatible === false
  const tabs: { value: DetailTab; label: string }[] = [
    { value: 'about', label: '简介' },
    { value: 'deps', label: `前置${d.dependencies.length ? `（${d.dependencies.length}）` : ''}` },
    { value: 'versions', label: `版本${d.versions.length ? `（${d.versions.length}）` : ''}` },
    ...(d.gallery.length ? [{ value: 'gallery' as const, label: '图集' }] : []),
  ]
  const link = (label: string, url: string | null) =>
    url && /^https?:\/\//.test(url) ? (
      <button key={label} type="button" onClick={() => api.openExternal(url).catch((e) => toast.error(msg(e)))} className="flex items-center gap-1 text-xs text-muted-foreground transition-colors duration-300 hover:text-brand">
        <ExternalLink className="size-3" />
        {label}
      </button>
    ) : null

  return (
    <motion.div initial={{ opacity: 0, y: 14 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.5, ease: EASE }} className="pb-4">
      {back}
      <Bezel radius={30}>
        <div className="flex items-start gap-5 p-5">
          <Icon url={d.icon_url} />
          <div className="min-w-0 flex-1">
            <h2 className="truncate text-2xl font-semibold tracking-tight">{d.title}</h2>
            <p className="mt-1 text-sm text-muted-foreground">{d.description}</p>
            <div className="mt-3 flex flex-wrap items-center gap-x-4 gap-y-1.5 text-xs text-muted-foreground">
              <span className="flex items-center gap-1">
                <DownloadIcon className="size-3.5" />
                {count(d.downloads)} 次下载
              </span>
              <span className="flex items-center gap-1">
                <Heart className="size-3.5" />
                {count(d.followers)} 关注
              </span>
              {d.license && <span>许可：{d.license}</span>}
              {link('源码', d.source_url)}
              {link('问题反馈', d.issues_url)}
              {link('文档', d.wiki_url)}
              {link('在 Modrinth 打开', `https://modrinth.com/project/${d.slug}`)}
            </div>
            <div className="mt-3 flex flex-wrap gap-1.5">
              {d.loaders.map((l) => (
                <Chip key={l} tone="brand">
                  {loaderName(l)}
                </Chip>
              ))}
              {d.categories.map((c) => (
                <Chip key={c}>{c}</Chip>
              ))}
            </div>
          </div>
          <div className="flex shrink-0 flex-col items-end gap-2">
            <PillButton size="md" tone={installed ? 'ghost' : 'brand'} disabled={installed || locked || incompatible} icon={<DownloadIcon className="size-4" />} onClick={() => onInstall(hit)}>
              {installed ? '已安装' : inst ? '安装到此实例' : '选择实例安装'}
            </PillButton>
            {installed && missing.length > 0 && (
              <PillButton size="sm" tone="soft" disabled={locked} onClick={() => onInstallMissing(hit, missing.map((m) => m.project_id))}>
                补装缺少的前置（{missing.length}）
              </PillButton>
            )}
          </div>
        </div>
        {incompatible && inst && (
          <div className="mx-5 mb-5 rounded-2xl border border-amber-400/25 bg-amber-400/10 px-3 py-2 text-xs text-amber-200">
            没有适配「{inst.name}」（Minecraft {inst.mc_version ?? inst.version_id}）的版本，所以不能安装到它上面。
          </div>
        )}
      </Bezel>

      <div className="mt-4 mb-3">
        <Segmented<DetailTab> value={tab} onChange={setTab} options={tabs} />
      </div>

      {tab === 'about' && (
        <Bezel radius={26}>
          <div className="p-5">
            {d.body.trim() ? <Body text={d.body} /> : <div className="text-sm text-muted-foreground">作者没有写详细介绍。</div>}
            {d.game_versions.length > 0 && (
              <div className="mt-5 border-t border-white/8 pt-4">
                <div className="mb-2 text-xs text-muted-foreground">支持的游戏版本</div>
                <div className="flex flex-wrap gap-1.5">
                  {[...d.game_versions].reverse().slice(0, 24).map((v) => (
                    <Chip key={v}>{v}</Chip>
                  ))}
                </div>
              </div>
            )}
          </div>
        </Bezel>
      )}

      {tab === 'deps' &&
        (d.dependencies.length === 0 ? (
          <Empty sprite="chest" title="没有前置模组" hint={inst ? '这个模组可以单独使用。' : '最新版本没有声明前置模组。'} />
        ) : (
          <div className="space-y-2">
            {missing.length > 0 && inst && !installed && (
              <p className="rounded-2xl border border-amber-400/25 bg-amber-400/10 px-3 py-2 text-xs text-amber-200">还有 {missing.length} 个必需前置没装，点安装时会询问是否一并安装。</p>
            )}
            {d.dependencies.map((x) => (
              <Bezel key={x.project_id} radius={22}>
                <div className="flex items-center gap-3 px-3.5 py-2.5">
                  <Icon url={x.icon_url} size={40} />
                  <div className="min-w-0 flex-1 truncate font-medium">{x.title}</div>
                  <Chip tone={x.dependency_type === 'required' ? 'warn' : 'plain'}>{x.dependency_type === 'required' ? '必需' : '可选'}</Chip>
                  {inst && <Chip tone={x.installed ? 'brand' : 'bad'}>{x.installed ? '已安装' : '未安装'}</Chip>}
                </div>
              </Bezel>
            ))}
          </div>
        ))}

      {tab === 'versions' &&
        (d.versions.length === 0 ? (
          <Empty sprite="diamond" title="没有版本" hint={inst ? `没有适配 ${inst.mc_version ?? inst.version_id} 的版本。` : undefined} />
        ) : (
          <div className="space-y-2">
            {d.versions.map((v) => (
              <Bezel key={v.id} radius={22}>
                <div className="flex items-center gap-3 px-3.5 py-2.5">
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <span className="truncate font-medium">{v.version_number}</span>
                      <Chip tone={v.version_type === 'release' ? 'brand' : 'warn'}>{v.version_type === 'release' ? '正式' : v.version_type === 'beta' ? '测试' : 'Alpha'}</Chip>
                    </div>
                    <div className="mt-0.5 truncate text-xs text-muted-foreground">
                      {v.game_versions.slice(-4).join(' · ')} · {v.loaders.map(loaderName).join(' / ')}
                    </div>
                  </div>
                  <div className="shrink-0 text-right text-xs text-muted-foreground tabular-nums">
                    <div>{v.date_published ? new Date(v.date_published).toLocaleDateString('zh-CN') : ''}</div>
                    <div>{count(v.downloads)} 下载</div>
                  </div>
                </div>
              </Bezel>
            ))}
          </div>
        ))}

      {tab === 'gallery' && (
        <div className="grid grid-cols-2 gap-3">
          {d.gallery.map((g) => (
            <figure key={g.url} className="overflow-hidden rounded-3xl bg-black/25">
              <img src={g.url} alt={g.title ?? ''} loading="lazy" onError={(e) => (e.currentTarget.style.display = 'none')} className="aspect-video w-full object-cover" />
              {g.title && <figcaption className="px-3 py-2 text-xs text-muted-foreground">{g.title}</figcaption>}
            </figure>
          ))}
        </div>
      )}
    </motion.div>
  )
}
