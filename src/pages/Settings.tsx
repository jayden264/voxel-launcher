import { useEffect, useState, type ReactNode } from 'react'
import { motion } from 'motion/react'
import { Check, CheckCircle2, FolderOpen, ImagePlus, XCircle } from 'lucide-react'
import { toast } from 'sonner'
import { api, type NetProbe, type Settings, type Source } from '@/lib/api'
import { rise } from '@/lib/motion'
import { useStore } from '@/lib/store'
import { ACCENTS, imageToBackground, useTheme } from '@/lib/theme'
import { Bezel, Eyebrow, PillButton } from '@/components/Bezel'
import { Field, PageBanner, Segmented } from '@/components/kit'
import { Input } from '@/components/ui/input'
import { AboutBody } from '@/components/About'
import { PixelSwitch } from '@/components/ui/pixel'
import { Slider } from '@/components/ui/slider'

function Section({ eyebrow, title, index, children }: { eyebrow: string; title: string; index: number; children: ReactNode }) {
  return (
    <motion.section variants={rise} initial="hidden" animate="show" custom={index}>
      <Bezel radius={32}>
        <div className="space-y-5 p-6">
          <div>
            <Eyebrow>{eyebrow}</Eyebrow>
            <h2 className="mt-2 text-lg font-semibold">{title}</h2>
          </div>
          {children}
        </div>
      </Bezel>
    </motion.section>
  )
}

const first = (v: number | readonly number[]) => (Array.isArray(v) ? (v as number[])[0] : (v as number))

/** Which download hosts answer from here, and how fast: the first thing to check when downloads crawl. */
function NetworkTest({ source }: { source: Source }) {
  const [probes, setProbes] = useState<NetProbe[] | null>(null)
  const [busy, setBusy] = useState(false)
  const run = async () => {
    setBusy(true)
    try {
      setProbes(await api.networkTest())
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }
  const mirror = probes?.find((p) => p.name.includes('BMCLAPI'))
  const official = probes?.find((p) => p.name.includes('Mojang'))
  let advice: string | null = null
  if (mirror && official) {
    if (source === 'mirror' && !mirror.ok && official.ok) advice = '镜像现在连不上，但官方源可以：建议把下载源切到「Mojang 官方」（镜像不可用时启动器也会自动改用官方）。'
    else if (source === 'official' && !official.ok && mirror.ok) advice = '官方源现在连不上，但镜像可以：建议把下载源切到「BMCLAPI 镜像」。'
    else if (source === 'official' && mirror.ok && official.ok && (mirror.ms ?? 0) * 2 < (official.ms ?? 0)) advice = `镜像（${mirror.ms} ms）比官方源（${official.ms} ms）快得多，国内建议用镜像。`
    else if (!mirror.ok && !official.ok) advice = '两个下载源都连不上，请检查网络、代理或防火墙。'
  }
  return (
    <Field label="网络测试" hint="看看下载用到的几个地址能不能连上、延迟多少">
      <div className="space-y-3">
        <PillButton size="sm" tone="soft" disabled={busy} onClick={run}>
          {busy ? '测试中…' : probes ? '重新测试' : '开始测试'}
        </PillButton>
        {probes && (
          <ul className="space-y-1.5">
            {probes.map((p) => (
              <li key={p.url} className="flex items-center gap-3 rounded-2xl bg-black/20 px-3 py-2 text-sm">
                {p.ok ? <CheckCircle2 className="size-4 shrink-0 text-brand" /> : <XCircle className="size-4 shrink-0 text-red-400" />}
                <span className="min-w-0 flex-1 truncate">{p.name}</span>
                <span className={`shrink-0 text-xs tabular-nums ${p.ok ? ((p.ms ?? 0) < 250 ? 'text-brand' : 'text-amber-300') : 'text-red-300'}`}>{p.ok ? `${p.ms} ms` : (p.error ?? '失败')}</span>
              </li>
            ))}
          </ul>
        )}
        {advice && <p className="rounded-2xl border border-amber-400/25 bg-amber-400/10 px-3 py-2 text-xs text-amber-200">{advice}</p>}
      </div>
    </Field>
  )
}

/** Colour, black level and a picture behind the interface. Kept on this device, applied instantly. */
function Appearance() {
  const theme = useTheme()
  const pick = async (file: File | undefined) => {
    if (!file) return
    try {
      const bg = await imageToBackground(file)
      if (!theme.set({ bg })) toast.warning('图片太大，没能保存；这次可以用，重启后会恢复默认')
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e))
    }
  }
  return (
    <>
      <Field label="强调色" hint="按钮、选中状态和光晕的颜色">
        <div className="flex flex-wrap gap-2.5" role="radiogroup" aria-label="强调色">
          {ACCENTS.map((a) => {
            const on = theme.accent === a.id
            return (
              <button
                key={a.id}
                type="button"
                role="radio"
                aria-checked={on}
                onClick={() => theme.set({ accent: a.id })}
                className={`group flex items-center gap-2 rounded-2xl px-3 py-2 text-sm transition-[background,box-shadow] duration-500 [transition-timing-function:var(--ease)] ${on ? 'bg-white/10 shadow-[inset_0_0_0_1.5px_var(--sw)]' : 'bg-white/5 hover:bg-white/9'}`}
                style={{ ['--sw' as string]: `oklch(0.8 0.19 ${a.a})` }}
              >
                <span className="grid size-6 place-items-center rounded-md" style={{ background: `linear-gradient(135deg, oklch(0.8 0.19 ${a.a}), oklch(0.72 0.17 ${a.b}))` }}>
                  {on && <Check className="size-3.5 text-black/70" strokeWidth={3} />}
                </span>
                {a.label}
              </button>
            )
          })}
        </div>
      </Field>
      <div className="flex items-center gap-4">
        <div className="min-w-0 flex-1">
          <div className="text-sm font-medium">OLED 纯黑</div>
          <div className="mt-0.5 text-xs text-muted-foreground">把背景换成纯黑，OLED 屏幕更省电、更深邃。</div>
        </div>
        <PixelSwitch checked={theme.oled} label="OLED 纯黑" onCheckedChange={(v) => theme.set({ oled: v })} />
      </div>
      <Field label="自定义背景" hint={theme.bg ? '图片只保存在这台电脑上' : '选一张喜欢的图片垫在界面后面'}>
        <div className="flex flex-wrap items-center gap-3">
          {theme.bg && <img src={theme.bg} alt="" className="h-14 w-24 rounded-xl object-cover shadow-[0_0_0_1px_oklch(1_0_0/12%)]" />}
          <label className="pill pill--soft pill--sm cursor-pointer">
            <span className="pill-label">{theme.bg ? '换一张' : '选择图片'}</span>
            <span className="pill-icon">
              <ImagePlus className="size-4" />
            </span>
            <input
              type="file"
              accept="image/*"
              className="sr-only"
              onChange={(e) => {
                pick(e.target.files?.[0])
                e.target.value = ''
              }}
            />
          </label>
          {theme.bg && (
            <PillButton size="sm" tone="ghost" onClick={() => theme.set({ bg: null })}>
              清除
            </PillButton>
          )}
        </div>
        {theme.bg && (
          <div className="pt-3">
            <div className="mb-1.5 text-xs text-muted-foreground">界面遮罩 {Math.round(theme.bgDim * 100)}%（越高字越清楚，图片越淡）</div>
            <Slider min={30} max={90} step={2} value={[Math.round(theme.bgDim * 100)]} onValueChange={(v) => theme.set({ bgDim: first(v) / 100 })} />
          </div>
        )}
      </Field>
    </>
  )
}

export function SettingsPage() {
  const snap = useStore((s) => s.snap)
  const save = useStore((s) => s.saveSettings)
  const [draft, setDraft] = useState<Settings | null>(snap?.settings ?? null)
  const [javaCheck, setJavaCheck] = useState<{ ok: boolean; text: string } | null>(null)

  useEffect(() => {
    if (snap) setDraft(snap.settings)
  }, [snap])
  if (!snap || !draft) return null

  const commit = (patch: Partial<Settings>) => {
    const next = { ...draft, ...patch }
    setDraft(next)
    save(next)
  }
  const maxMem = Math.max(2048, snap.system.max_mem_mb)
  const mem = draft.default_memory_mb ?? snap.system.recommended_mem_mb

  const checkJava = async () => {
    if (!draft.java_path) return
    try {
      const r = await api.probeJava(draft.java_path)
      setJavaCheck({ ok: true, text: `可用：Java ${r.major}（${r.version}）` })
    } catch (e) {
      setJavaCheck({ ok: false, text: e instanceof Error ? e.message : String(e) })
    }
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageBanner biome="end" seed="settings" eyebrow="Settings" title="设置" subtitle="修改后自动保存。" />
      <div className="min-h-0 flex-1 overflow-auto pr-1">
        <div className="max-w-3xl space-y-5 pb-6">
          <Section eyebrow="Look" title="外观" index={0}>
            <Appearance />
          </Section>

          <Section eyebrow="Network" title="下载" index={1}>
            <Field label="下载源" hint="镜像不可用时会自动改用官方源">
              <Segmented<Source>
                value={draft.source}
                onChange={(v) => commit({ source: v })}
                options={[
                  { value: 'mirror', label: 'BMCLAPI 镜像（国内推荐）' },
                  { value: 'official', label: 'Mojang 官方' },
                ]}
              />
            </Field>
            <NetworkTest source={draft.source} />
            <Field label="并发下载数" hint={`${draft.max_downloads}`}>
              <Slider min={4} max={64} step={2} value={[draft.max_downloads]} onValueChange={(v) => setDraft({ ...draft, max_downloads: first(v) })} onValueCommitted={(v) => commit({ max_downloads: first(v) })} />
            </Field>
          </Section>

          <Section eyebrow="Game" title="游戏" index={2}>
            <Field label="默认最大内存" hint={`${(mem / 1024).toFixed(1)} GB · 建议 ${(snap.system.recommended_mem_mb / 1024).toFixed(0)} GB · 上限为物理内存的 75%`}>
              <Slider min={1024} max={maxMem} step={256} value={[Math.min(mem, maxMem)]} onValueChange={(v) => setDraft({ ...draft, default_memory_mb: first(v) })} onValueCommitted={(v) => commit({ default_memory_mb: first(v) })} />
              <p className="pt-1.5 text-xs text-muted-foreground">启动时如果电脑可用内存不够，会自动调低这一次的内存并提醒你，不会改动这里的设置。</p>
            </Field>
            <div className="flex items-center gap-4">
              <div className="min-w-0 flex-1">
                <div className="text-sm font-medium">启动前自动备份存档</div>
                <div className="mt-0.5 text-xs text-muted-foreground">每次启动前备份存档和设置，崩溃或存档损坏时可以在「存档」页回退。存档没变化时会跳过，最多保留 10 份。</div>
              </div>
              <PixelSwitch checked={draft.auto_backup} label="启动前自动备份存档" onCheckedChange={(v) => commit({ auto_backup: v })} />
            </div>
            <Field label="Java 路径" hint="留空即可：启动器会按版本自动下载官方 Java 运行时">
              <div className="flex gap-2">
                <Input
                  value={draft.java_path ?? ''}
                  placeholder="自动"
                  className="h-10 rounded-full px-4 font-mono"
                  onChange={(e) => {
                    setJavaCheck(null)
                    setDraft({ ...draft, java_path: e.target.value || null })
                  }}
                  onBlur={() => save(draft)}
                />
                <PillButton
                  size="sm"
                  tone="soft"
                  onClick={async () => {
                    const p = await api.pickJava()
                    if (p) {
                      setJavaCheck(null)
                      commit({ java_path: p })
                    }
                  }}
                >
                  浏览
                </PillButton>
                <PillButton size="sm" tone="soft" onClick={checkJava} disabled={!draft.java_path}>
                  检测
                </PillButton>
              </div>
              {javaCheck && (
                <motion.div initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0 }} className={`mt-2 flex items-center gap-1.5 px-1 text-sm ${javaCheck.ok ? 'text-brand' : 'text-red-300'}`}>
                  {javaCheck.ok ? <CheckCircle2 className="size-4" /> : <XCircle className="size-4" />}
                  {javaCheck.text}
                </motion.div>
              )}
            </Field>
            <p className="text-xs text-muted-foreground">实例设置里的内存、JVM 参数和 Java 路径会覆盖这里的默认值；JVM 参数中后出现的同名选项生效。</p>
          </Section>

          <Section eyebrow="Storage" title="数据" index={3}>
            <Field label="数据目录" hint="游戏文件、Java 运行时和所有实例都保存在这里">
              <div className="flex items-center gap-2">
                <div className="selectable min-w-0 flex-1 truncate rounded-full bg-black/25 px-4 py-2.5 font-mono text-xs shadow-[inset_0_0_0_1px_oklch(1_0_0/8%)]">{snap.data_dir}</div>
                <PillButton size="sm" tone="soft" icon={<FolderOpen className="size-3.5" />} onClick={() => api.openDataDir().catch((e) => toast.error(String(e.message ?? e)))}>
                  打开
                </PillButton>
              </div>
            </Field>
          </Section>

          <Section eyebrow="About" title="关于" index={4}>
            <AboutBody />
          </Section>
        </div>
      </div>
    </div>
  )
}
