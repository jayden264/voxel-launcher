import { motion } from 'motion/react'
import { Download, Globe, House, Layers, Puzzle, Settings, UserRound } from 'lucide-react'
import type { ReactNode } from 'react'
import { Bezel } from './Bezel'
import { LogoMark } from './art'
import { useStore, type Page } from '@/lib/store'
import { cn } from '@/lib/utils'

const ITEMS: { page: Page; label: string; icon: ReactNode }[] = [
  { page: 'home', label: '首页', icon: <House className="size-[22px]" /> },
  { page: 'instances', label: '实例', icon: <Layers className="size-[22px]" /> },
  { page: 'download', label: '下载', icon: <Download className="size-[22px]" /> },
  { page: 'mods', label: '内容', icon: <Puzzle className="size-[22px]" /> },
  { page: 'worlds', label: '存档', icon: <Globe className="size-[22px]" /> },
  { page: 'accounts', label: '账号', icon: <UserRound className="size-[22px]" /> },
]

/** A floating glass island rather than an edge-glued rail. */
export function Sidebar() {
  const page = useStore((s) => s.page)
  const setPage = useStore((s) => s.setPage)
  const phase = useStore((s) => s.run.phase)
  const ready = useStore((s) => s.run.ready)

  const item = (p: Page, label: string, icon: ReactNode) => {
    const active = page === p
    return (
      <button
        key={p}
        type="button"
        onClick={() => setPage(p)}
        className={cn(
          'group relative flex w-full flex-col items-center gap-1 rounded-[20px] py-3 text-[11px] font-medium transition-colors duration-500 [transition-timing-function:var(--ease)]',
          active ? 'text-brand' : 'text-muted-foreground hover:text-foreground',
        )}
      >
        {active && (
          <motion.span
            layoutId="nav-active"
            className="absolute inset-0 rounded-[20px] bg-brand/12 shadow-[inset_0_0_0_1px_color-mix(in_oklch,var(--brand)_28%,transparent),0_10px_30px_-14px_var(--brand)]"
            transition={{ type: 'spring', stiffness: 460, damping: 36 }}
          />
        )}
        <span className="relative transition-transform duration-500 [transition-timing-function:var(--ease)] group-hover:-translate-y-0.5 group-active:scale-90">{icon}</span>
        <span className="relative">{label}</span>
      </button>
    )
  }

  return (
    <nav className="flex shrink-0 pr-3 pb-5 pl-4">
      <Bezel radius={30} className="h-full" coreClassName="flex w-[78px] flex-col items-center gap-1 px-2 py-3">
        <div className="mb-3 grid size-11 place-items-center">
          <LogoMark size={32} />
        </div>
        <div className="flex w-full flex-col gap-1">{ITEMS.map((i) => item(i.page, i.label, i.icon))}</div>
        <div className="mt-auto flex w-full flex-col items-center gap-2">
          {phase !== 'idle' && (
            <motion.div initial={{ opacity: 0, scale: 0.8 }} animate={{ opacity: 1, scale: 1 }} className="flex items-center gap-1.5 text-[10px] text-brand">
              <span className="relative flex size-2">
                <span className="absolute inline-flex size-full animate-ping rounded-full bg-brand opacity-60" />
                <span className="relative inline-flex size-2 rounded-full bg-brand" />
              </span>
              {phase === 'running' ? (ready ? '运行中' : '加载中') : '处理中'}
            </motion.div>
          )}
          {item('settings', '设置', <Settings className="size-[22px]" />)}
        </div>
      </Bezel>
    </nav>
  )
}
