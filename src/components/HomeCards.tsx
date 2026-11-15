import { useEffect, useMemo, useState } from 'react'
import { motion } from 'motion/react'
import { ChevronLeft, ChevronRight, Play } from 'lucide-react'
import { api, type Instance, type PlaySession, type World } from '@/lib/api'
import { fmtAgo, fmtDuration } from '@/lib/format'
import { rise } from '@/lib/motion'
import { useStore } from '@/lib/store'
import { PixelSprite } from '@/components/art'
import { Bezel } from '@/components/Bezel'

const MODE: Record<string, string> = { survival: '生存', creative: '创造', adventure: '冒险', spectator: '旁观' }

function canQuickPlay(mc: string) {
  const m = /^1\.(\d+)/.exec(mc)
  return m ? Number(m[1]) >= 20 : true
}

/** The worlds played last, each one click from being back in it. Shows nothing when there are none. */
export function ContinueCard({ inst }: { inst: Instance }) {
  const [worlds, setWorlds] = useState<World[]>([])
  const idle = useStore((s) => s.run.phase === 'idle')
  const launch = useStore((s) => s.launch)
  const ok = canQuickPlay(inst.mc_version ?? inst.version_id) && !inst.running

  useEffect(() => {
    let live = true
    setWorlds([])
    api.worldsList(inst.id).then(
      (l) => live && setWorlds(l.filter((w) => w.last_played !== null).slice(0, 3)),
      () => {},
    )
    return () => {
      live = false
    }
  }, [inst.id, inst.running])

  if (worlds.length === 0) return null
  return (
    <motion.div variants={rise} initial="hidden" animate="show" custom={4}>
      <Bezel radius={30}>
        <div className="p-4">
          <div className="eyebrow mb-3">继续游玩</div>
          <div className="space-y-1">
            {worlds.map((w) => (
              <button
                key={w.dir}
                type="button"
                disabled={!idle || !ok}
                title={!canQuickPlay(inst.mc_version ?? inst.version_id) ? '这个版本不支持直接进入存档（需要 Minecraft 1.20 或更新）' : `启动并进入「${w.name}」`}
                onClick={() => launch(inst.id, false, { kind: 'world', name: w.dir })}
                className="group flex w-full items-center gap-3 rounded-2xl p-2 text-left transition-colors duration-500 [transition-timing-function:var(--ease)] hover:bg-white/6 disabled:opacity-55"
              >
                <div className="grid size-10 shrink-0 place-items-center overflow-hidden rounded-xl bg-black/30">
                  {w.icon ? <img src={w.icon} alt="" className="size-full object-cover [image-rendering:pixelated]" /> : <PixelSprite name="grass" size={26} />}
                </div>
                <div className="min-w-0 flex-1">
                  <div className="truncate text-sm font-medium">{w.name}</div>
                  <div className="truncate text-xs text-muted-foreground">
                    {w.last_played ? fmtAgo(w.last_played) : ''}
                    {w.game_mode && ` · ${w.hardcore ? '极限' : MODE[w.game_mode]}`}
                    {w.play_secs > 0 && ` · ${fmtDuration(w.play_secs)}`}
                  </div>
                </div>
                <span className="grid size-8 shrink-0 place-items-center rounded-full bg-brand/14 text-brand transition-transform duration-500 [transition-timing-function:var(--ease)] group-hover:scale-110 group-disabled:opacity-40">
                  <Play className="size-3.5 fill-current" />
                </span>
              </button>
            ))}
          </div>
        </div>
      </Bezel>
    </motion.div>
  )
}

const WEEK = ['一', '二', '三', '四', '五', '六', '日']
const key = (d: Date) => `${d.getFullYear()}-${d.getMonth() + 1}-${d.getDate()}`

/** A month at a glance: darker squares are days you played more. */
export function MonthCard() {
  const [sessions, setSessions] = useState<PlaySession[] | null>(null)
  const [back, setBack] = useState(0)
  useEffect(() => {
    api.statsSessions().then(setSessions, () => setSessions([]))
  }, [])

  const m = useMemo(() => {
    const base = new Date()
    base.setDate(1)
    base.setMonth(base.getMonth() - back)
    const year = base.getFullYear()
    const month = base.getMonth()
    const perDay = new Map<string, number>()
    for (const s of sessions ?? []) {
      const d = new Date(s.start * 1000)
      perDay.set(key(d), (perDay.get(key(d)) ?? 0) + s.secs)
    }
    const days = new Date(year, month + 1, 0).getDate()
    const lead = (new Date(year, month, 1).getDay() + 6) % 7
    const cells = Array.from({ length: days }, (_, i) => {
      const d = new Date(year, month, i + 1)
      return { day: i + 1, secs: perDay.get(key(d)) ?? 0, label: d.toLocaleDateString('zh-CN', { month: 'long', day: 'numeric' }), today: key(d) === key(new Date()) }
    })
    const total = cells.reduce((n, c) => n + c.secs, 0)
    return { title: `${year} 年 ${month + 1} 月`, lead, cells, total, active: cells.filter((c) => c.secs > 0).length, max: Math.max(...cells.map((c) => c.secs), 1) }
  }, [sessions, back])

  const shade = (secs: number) => {
    if (secs === 0) return 'bg-white/6'
    const r = secs / m.max
    return r < 0.25 ? 'bg-brand/25' : r < 0.5 ? 'bg-brand/45' : r < 0.75 ? 'bg-brand/70' : 'bg-brand'
  }
  const arrow = 'grid size-7 place-items-center rounded-full text-muted-foreground transition-colors duration-300 hover:bg-white/10 hover:text-foreground disabled:opacity-30 disabled:hover:bg-transparent'
  return (
    <motion.div variants={rise} initial="hidden" animate="show" custom={5}>
      <Bezel radius={30}>
        <div className="p-4">
          <div className="mb-3 flex items-center justify-between">
            <div className="eyebrow">{back === 0 ? '本月游玩' : m.title}</div>
            <div className="flex items-center">
              <button type="button" aria-label="上个月" className={arrow} onClick={() => setBack((b) => b + 1)}>
                <ChevronLeft className="size-4" />
              </button>
              <button type="button" aria-label="下个月" disabled={back === 0} className={arrow} onClick={() => setBack((b) => b - 1)}>
                <ChevronRight className="size-4" />
              </button>
            </div>
          </div>
          <div className="grid grid-cols-7 gap-1 text-center text-[10px] text-muted-foreground">
            {WEEK.map((w) => (
              <div key={w}>{w}</div>
            ))}
            {Array.from({ length: m.lead }, (_, i) => (
              <div key={`l${i}`} />
            ))}
            {m.cells.map((c) => (
              <div
                key={c.day}
                title={`${c.label} · ${c.secs > 0 ? fmtDuration(c.secs) : '没有游玩'}`}
                className={`grid aspect-square place-items-center rounded-[6px] text-[10px] tabular-nums transition-colors duration-500 ${shade(c.secs)} ${c.secs > 0 ? 'text-black/70' : ''} ${c.today ? 'ring-[1.5px] ring-brand ring-offset-1 ring-offset-[oklch(0.17_0.02_255)]' : ''}`}
              >
                {c.day}
              </div>
            ))}
          </div>
          <div className="mt-3 text-xs text-muted-foreground">
            {sessions === null ? '读取中…' : m.total > 0 ? `共 ${fmtDuration(m.total)} · ${m.active} 天有游玩` : '这个月还没有游玩记录'}
          </div>
        </div>
      </Bezel>
    </motion.div>
  )
}
