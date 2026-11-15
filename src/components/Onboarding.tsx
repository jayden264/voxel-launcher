import { useEffect, useState } from 'react'
import { AnimatePresence, motion } from 'motion/react'
import { ArrowLeft, ArrowRight, Check, Package, Rocket } from 'lucide-react'
import { toast } from 'sonner'
import { api, type Recommended } from '@/lib/api'
import { EASE } from '@/lib/motion'
import { useStore } from '@/lib/store'
import { PixelAvatar, PixelSprite } from '@/components/art'
import { Bezel, Eyebrow, PillButton } from '@/components/Bezel'
import { Input } from '@/components/ui/input'
import { Slider } from '@/components/ui/slider'
import { cn } from '@/lib/utils'

type Play = 'vanilla' | 'fabric' | 'pack'
const STEPS = ['欢迎', '玩家名', '玩法', '内存', '开始']
const NAME_OK = /^[A-Za-z0-9_]{3,16}$/

interface Latest {
  mc: string
  fabric: string | null
  recs: Recommended[]
}

function Choice({ on, disabled, icon, title, desc, tag, onClick }: { on: boolean; disabled?: boolean; icon: React.ReactNode; title: string; desc: string; tag?: string; onClick: () => void }) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={on}
      disabled={disabled}
      onClick={onClick}
      className={cn(
        'group relative flex w-full items-center gap-4 rounded-3xl p-4 text-left transition-[background,box-shadow,transform] duration-500 [transition-timing-function:var(--ease)] active:scale-[0.99] disabled:cursor-not-allowed disabled:opacity-45',
        on ? 'bg-brand/12 shadow-[inset_0_0_0_1.5px_color-mix(in_oklch,var(--brand)_55%,transparent)]' : 'bg-white/5 hover:bg-white/9',
      )}
    >
      <div className="grid size-14 shrink-0 place-items-center rounded-2xl bg-black/30">{icon}</div>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="font-semibold">{title}</span>
          {tag && <span className="rounded-full bg-brand/16 px-2 py-0.5 text-[10px] font-medium text-brand">{tag}</span>}
        </div>
        <div className="mt-0.5 text-sm text-muted-foreground">{desc}</div>
      </div>
      <span className={cn('grid size-6 shrink-0 place-items-center rounded-full border transition-colors duration-500', on ? 'border-brand bg-brand text-black/75' : 'border-white/20')}>{on && <Check className="size-3.5" strokeWidth={3} />}</span>
    </button>
  )
}

/** A short, one-question-at-a-time setup: who you are, what to play, how much memory. Can be opened again from Settings. */
export function Onboarding() {
  const open = useStore((s) => s.onboarding)
  const close = useStore((s) => s.closeOnboarding)
  const snap = useStore((s) => s.snap)
  const mutate = useStore((s) => s.mutate)
  const installInstance = useStore((s) => s.installInstance)
  const setPage = useStore((s) => s.setPage)

  const [step, setStep] = useState(0)
  const [dir, setDir] = useState(1)
  const [name, setName] = useState('')
  const [play, setPlay] = useState<Play>('fabric')
  const [memory, setMemory] = useState<number | null>(null)
  const [latest, setLatest] = useState<Latest | 'loading' | 'failed'>('loading')
  const [busy, setBusy] = useState(false)

  const existing = snap?.settings.accounts[0]?.name ?? null
  const recommended = snap?.system.recommended_mem_mb ?? 4096
  const maxMem = Math.max(2048, snap?.system.max_mem_mb ?? 8192)
  const mem = memory ?? recommended

  // Reset each time it is opened, and prefill with what is already set up.
  useEffect(() => {
    if (!open) return
    setStep(0)
    setDir(1)
    setName(existing ?? '')
    setPlay('fabric')
    setMemory(null)
    setLatest('loading')
    let live = true
    api.listVersions().then(
      async (v) => {
        const mc = v.latest_release
        const lv = await api.listLoaderVersions('fabric', mc).catch(() => [])
        const fabric = lv.find((x) => x.stable)?.version ?? lv[0]?.version ?? null
        const recs = fabric ? await api.recommendedMods('fabric', mc).catch(() => []) : []
        if (live) setLatest({ mc, fabric, recs })
      },
      () => live && setLatest('failed'),
    )
    return () => {
      live = false
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open])

  if (!open || !snap) return null

  const ready = latest !== 'loading' && latest !== 'failed' ? latest : null
  const fabricOk = !!ready?.fabric
  const nameValid = NAME_OK.test(name.trim()) || (existing !== null && name.trim() === existing)
  const go = (n: number) => {
    setDir(n > step ? 1 : -1)
    setStep(n)
  }
  const canNext = step === 1 ? nameValid : true

  const finish = async () => {
    setBusy(true)
    try {
      const trimmed = name.trim()
      if (trimmed && trimmed !== existing) {
        if (!(await mutate(() => api.addAccount(trimmed)))) return
      }
      if (memory !== null) {
        const s = useStore.getState().snap!.settings
        await useStore.getState().saveSettings({ ...s, default_memory_mb: memory })
      }
      close()
      if (play === 'pack') {
        setPage('download')
        toast.info('在「下载」页的整合包里挑一个，点安装就行')
        return
      }
      if (!ready) {
        toast.info('现在连不上版本列表，等网络好了在「下载」页创建实例就行')
        return
      }
      const loader = play === 'fabric' && ready.fabric ? { kind: 'fabric' as const, version: ready.fabric } : null
      const label = loader ? `Fabric ${ready.mc}` : `Minecraft ${ready.mc}`
      if (!(await mutate(() => api.createInstance(label, ready.mc, loader)))) return
      const id = useStore.getState().snap?.settings.selected_instance
      setPage('home')
      if (id) installInstance(id, loader ? ready.recs.filter((r) => r.default_on).map((r) => r.slug) : [])
    } finally {
      setBusy(false)
    }
  }

  const body = (() => {
    switch (step) {
      case 0:
        return (
          <div className="flex flex-col items-center gap-5 py-4 text-center">
            <PixelSprite name="grass" size={84} bob className="drop-shadow-[0_14px_24px_oklch(0_0_0/40%)]" />
            <div>
              <h2 className="text-3xl font-semibold tracking-tight">欢迎来到 Voxel</h2>
              <p className="mt-2 text-muted-foreground">一分钟，把第一个 Minecraft 世界准备好。</p>
            </div>
            <div className="flex flex-wrap justify-center gap-2 text-sm">
              {['启动前自动备份存档', '一键进入存档和服务器', '崩溃了告诉你为什么'].map((t) => (
                <span key={t} className="rounded-full bg-white/8 px-3 py-1.5 text-foreground/80">
                  {t}
                </span>
              ))}
            </div>
          </div>
        )
      case 1:
        return (
          <div className="space-y-5 py-2">
            <div>
              <h2 className="text-2xl font-semibold tracking-tight">你想叫什么名字？</h2>
              <p className="mt-1.5 text-sm text-muted-foreground">这是在游戏里显示的名字。{existing ? `已经有账号「${existing}」了，可以直接用，也可以换一个新的。` : '3–16 个字母、数字或下划线，之后随时可以改。'}</p>
            </div>
            <div className="flex items-center gap-4">
              <div className="grid size-20 shrink-0 place-items-center rounded-3xl bg-black/30">
                <PixelAvatar seed={name.trim() || 'steve'} size={60} className="rounded-2xl" />
              </div>
              <div className="min-w-0 flex-1">
                <Input autoFocus value={name} maxLength={16} onChange={(e) => setName(e.target.value.replace(/\s/g, ''))} placeholder="例如：Steve" className="h-12 rounded-2xl text-lg" onKeyDown={(e) => e.key === 'Enter' && nameValid && go(2)} />
                <div className={cn('mt-1.5 h-4 text-xs transition-colors', name && !nameValid ? 'text-amber-300' : 'text-muted-foreground')}>{name && !nameValid ? '只能用字母、数字和下划线，长度 3–16' : '这是离线账号，不需要登录'}</div>
              </div>
            </div>
          </div>
        )
      case 2:
        return (
          <div className="space-y-4 py-2">
            <div>
              <h2 className="text-2xl font-semibold tracking-tight">你想怎么玩？</h2>
              <p className="mt-1.5 text-sm text-muted-foreground">不确定就选中间那个，以后随时能在「下载」页再建别的。</p>
            </div>
            <div className="space-y-2.5" role="radiogroup" aria-label="选择玩法">
              <Choice on={play === 'vanilla'} icon={<PixelSprite name="grass" size={36} />} title="原版，纯净" desc={ready ? `Minecraft ${ready.mc}，什么模组都不装` : latest === 'failed' ? '暂时连不上版本列表' : '正在查询最新版本…'} onClick={() => setPlay('vanilla')} />
              <Choice
                on={play === 'fabric'}
                disabled={latest !== 'loading' && !fabricOk}
                icon={<PixelSprite name="diamond" size={36} />}
                title="Fabric，能装模组"
                tag="推荐"
                desc={ready ? (fabricOk ? `Minecraft ${ready.mc} + 几个让游戏更流畅的优化模组` : '这个版本暂时没有 Fabric') : latest === 'failed' ? '暂时连不上版本列表' : '正在查询…'}
                onClick={() => setPlay('fabric')}
              />
              <Choice on={play === 'pack'} icon={<Package className="size-7 text-brand" />} title="我想直接装整合包" desc="去挑一个现成的整合包，一键装好" onClick={() => setPlay('pack')} />
            </div>
          </div>
        )
      case 3:
        return (
          <div className="space-y-5 py-2">
            <div>
              <h2 className="text-2xl font-semibold tracking-tight">给游戏多少内存？</h2>
              <p className="mt-1.5 text-sm text-muted-foreground">你的电脑最多能给 {(maxMem / 1024).toFixed(0)} GB。我们按你的配置推荐了 {(recommended / 1024).toFixed(0)} GB，一般不用改。</p>
            </div>
            <div className="rounded-3xl bg-black/20 p-5">
              <div className="mb-3 flex items-baseline justify-between">
                <span className="text-sm text-muted-foreground">最大内存</span>
                <span className="font-mono text-2xl font-semibold tabular-nums text-brand">{(mem / 1024).toFixed(1)} GB</span>
              </div>
              <Slider min={1024} max={maxMem} step={256} value={[Math.min(mem, maxMem)]} onValueChange={(v) => setMemory(Array.isArray(v) ? v[0] : (v as number))} />
            </div>
            <ul className="space-y-1.5 text-sm text-muted-foreground">
              <li className="flex gap-2">
                <Check className="mt-0.5 size-4 shrink-0 text-brand" />
                启动时如果电脑内存不够，会自动调低并提醒你，不会因此打不开。
              </li>
              <li className="flex gap-2">
                <Check className="mt-0.5 size-4 shrink-0 text-brand" />
                不用自己装 Java：需要时会先问你一声，再从官方下载。
              </li>
            </ul>
          </div>
        )
      default:
        return (
          <div className="space-y-5 py-2">
            <div className="text-center">
              <PixelSprite name="chest" size={64} bob className="mx-auto" />
              <h2 className="mt-3 text-2xl font-semibold tracking-tight">都准备好了</h2>
              <p className="mt-1.5 text-sm text-muted-foreground">点下面的按钮，启动器会把游戏下载好。下完就能玩。</p>
            </div>
            <dl className="divide-y divide-white/8 rounded-3xl bg-black/20 text-sm">
              {[
                ['玩家名', name.trim() || existing || '（稍后添加）'],
                ['玩法', play === 'pack' ? '去挑整合包' : play === 'fabric' ? `Fabric ${ready?.mc ?? ''} + 优化模组` : `原版 ${ready?.mc ?? ''}`],
                ['内存', `${(mem / 1024).toFixed(1)} GB`],
              ].map(([k, v]) => (
                <div key={k} className="flex items-center justify-between px-4 py-3">
                  <dt className="text-muted-foreground">{k}</dt>
                  <dd className="font-medium">{v}</dd>
                </div>
              ))}
            </dl>
          </div>
        )
    }
  })()

  return (
    <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ duration: 0.4 }} className="absolute inset-x-0 top-10 bottom-0 z-50 grid place-items-center bg-[oklch(0.1_0.02_255)]/82 p-6 backdrop-blur-md">
      <motion.div initial={{ opacity: 0, y: 24, scale: 0.97 }} animate={{ opacity: 1, y: 0, scale: 1 }} transition={{ duration: 0.7, ease: EASE }} className="w-full max-w-[620px]">
        <Bezel radius={36}>
          <div className="p-7">
            <div className="mb-5 flex items-center justify-between">
              <Eyebrow>新手引导 · {step + 1}/{STEPS.length}</Eyebrow>
              <button type="button" onClick={close} className="text-xs text-muted-foreground transition-colors hover:text-foreground">
                跳过
              </button>
            </div>
            <div className="mb-5 flex gap-1.5" aria-hidden>
              {STEPS.map((s, i) => (
                <div key={s} className="h-1 flex-1 overflow-hidden rounded-full bg-white/10">
                  <motion.div className="h-full origin-left rounded-full bg-brand" initial={false} animate={{ scaleX: i <= step ? 1 : 0 }} transition={{ duration: 0.5, ease: EASE }} />
                </div>
              ))}
            </div>
            <div className="min-h-[330px] overflow-hidden">
              <AnimatePresence mode="wait" custom={dir} initial={false}>
                <motion.div key={step} custom={dir} initial={{ opacity: 0, x: 28 * dir }} animate={{ opacity: 1, x: 0 }} exit={{ opacity: 0, x: -28 * dir }} transition={{ duration: 0.32, ease: EASE }}>
                  {body}
                </motion.div>
              </AnimatePresence>
            </div>
            <div className="mt-5 flex items-center justify-between">
              <PillButton tone="ghost" disabled={step === 0 || busy} icon={<ArrowLeft className="size-4" />} onClick={() => go(step - 1)} className={step === 0 ? 'invisible' : ''}>
                上一步
              </PillButton>
              {step < STEPS.length - 1 ? (
                <PillButton disabled={!canNext} icon={<ArrowRight className="size-4" />} onClick={() => go(step + 1)}>
                  {step === 0 ? '开始' : '下一步'}
                </PillButton>
              ) : (
                <PillButton disabled={busy} icon={<Rocket className="size-4" />} onClick={finish}>
                  {busy ? '准备中…' : play === 'pack' ? '去挑整合包' : '开始下载'}
                </PillButton>
              )}
            </div>
          </div>
        </Bezel>
      </motion.div>
    </motion.div>
  )
}
