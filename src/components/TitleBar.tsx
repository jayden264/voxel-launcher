import { useEffect, useState } from 'react'
import { ChevronLeft, ChevronRight, Minus, Square, X } from 'lucide-react'
import { api } from '@/lib/api'
import { useStore } from '@/lib/store'
import { cn } from '@/lib/utils'

function clock(secs: number) {
  const h = Math.floor(secs / 3600)
  const m = Math.floor((secs % 3600) / 60)
  const s = secs % 60
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}` : `${m}:${String(s).padStart(2, '0')}`
}

/** What the launcher is doing right now, in the corner, wherever you are: click to go home. */
function StatusPill() {
  const run = useStore((s) => s.run)
  const name = useStore((s) => s.snap?.instances.find((i) => i.id === s.run.instanceId)?.name)
  const setPage = useStore((s) => s.setPage)
  const [now, setNow] = useState(() => Date.now())
  const ticking = run.phase === 'running' && run.ready && run.since !== null
  useEffect(() => {
    if (!ticking) return
    const t = setInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(t)
  }, [ticking])

  if (run.phase === 'idle') return null
  const label = run.phase === 'working' ? run.stage : run.ready ? `运行中${name ? ` · ${name}` : ''}` : `加载中${name ? ` · ${name}` : ''}`
  return (
    <button
      type="button"
      onClick={() => setPage('home')}
      title="回到首页查看"
      className="mr-2 flex max-w-[280px] items-center gap-2 self-center rounded-full bg-white/7 px-3 py-1 text-xs text-foreground/85 transition-colors duration-300 hover:bg-white/12"
    >
      <span className="relative flex size-2 shrink-0">
        <span className="absolute inline-flex size-full animate-ping rounded-full bg-brand opacity-60" />
        <span className="relative inline-flex size-2 rounded-full bg-brand" />
      </span>
      <span className="truncate">{label}</span>
      {ticking && run.since !== null && <span className="shrink-0 tabular-nums text-muted-foreground">{clock(Math.max(0, Math.floor((now - run.since) / 1000)))}</span>}
    </button>
  )
}

function NavButton({ label, disabled, onClick, children }: { label: string; disabled: boolean; onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      disabled={disabled}
      onClick={onClick}
      className={cn('grid size-7 place-items-center rounded-full text-muted-foreground transition-[background,color] duration-300', disabled ? 'opacity-30' : 'hover:bg-white/10 hover:text-foreground')}
    >
      {children}
    </button>
  )
}

/** Custom window chrome. `data-tauri-drag-region` makes the bar draggable. */
export function TitleBar() {
  const canBack = useStore((s) => s.navBack.length > 0)
  const canForward = useStore((s) => s.navForward.length > 0)
  const goBack = useStore((s) => s.goBack)
  const goForward = useStore((s) => s.goForward)

  // Alt+←/→ and the mouse's back/forward buttons, like a browser.
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (!e.altKey) return
      if (e.key === 'ArrowLeft') goBack()
      if (e.key === 'ArrowRight') goForward()
    }
    const mouse = (e: MouseEvent) => {
      if (e.button === 3) goBack()
      if (e.button === 4) goForward()
    }
    window.addEventListener('keydown', key)
    window.addEventListener('mouseup', mouse)
    return () => {
      window.removeEventListener('keydown', key)
      window.removeEventListener('mouseup', mouse)
    }
  }, [goBack, goForward])

  return (
    <div data-tauri-drag-region className="flex h-10 shrink-0 items-center justify-between pl-4">
      <div data-tauri-drag-region className="flex items-center gap-2 text-xs text-muted-foreground">
        <span data-tauri-drag-region className="font-medium tracking-wide text-foreground/80">
          VOXEL
        </span>
        <span data-tauri-drag-region className="opacity-50">
          启动器
        </span>
        <div className="ml-2 flex items-center">
          <NavButton label="后退（Alt+←）" disabled={!canBack} onClick={goBack}>
            <ChevronLeft className="size-4" />
          </NavButton>
          <NavButton label="前进（Alt+→）" disabled={!canForward} onClick={goForward}>
            <ChevronRight className="size-4" />
          </NavButton>
        </div>
      </div>
      <div className="flex h-full">
        <StatusPill />
        <button
          type="button"
          aria-label="最小化"
          onClick={() => api.windowMinimize()}
          className="grid w-11 place-items-center text-muted-foreground transition-colors hover:bg-white/8 hover:text-foreground"
        >
          <Minus className="size-4" />
        </button>
        <button
          type="button"
          aria-label="最大化"
          onClick={() => api.windowToggleMax()}
          className="grid w-11 place-items-center text-muted-foreground transition-colors hover:bg-white/8 hover:text-foreground"
        >
          <Square className="size-3.5" />
        </button>
        <button
          type="button"
          aria-label="关闭"
          onClick={() => api.windowClose()}
          className="grid w-11 place-items-center text-muted-foreground transition-colors hover:bg-red-500/80 hover:text-white"
        >
          <X className="size-4" />
        </button>
      </div>
    </div>
  )
}
