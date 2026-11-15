import { AnimatePresence, motion } from 'motion/react'
import { EASE } from '@/lib/motion'
import { formatBytes, type Progress } from '@/lib/store'

/** Segmented, Minecraft-style bar. The fill is a scaleX transform (GPU only). */
export function PixelBar({ value }: { value: number | null }) {
  return (
    <div className="px-bar">
      {value === null ? (
        <motion.div
          className="px-bar-fill progress-shimmer"
          style={{ right: 'auto', width: '34%' }}
          animate={{ x: ['-100%', '300%'] }}
          transition={{ repeat: Infinity, duration: 1.6, ease: EASE }}
        />
      ) : (
        // Short tween: events arrive every ~100ms, and a long ease restarted each time
        // never catches up, so the bar sat at half while the number said 100%.
        <motion.div className="px-bar-fill progress-shimmer" initial={false} animate={{ scaleX: Math.max(0.015, Math.min(1, value)) }} transition={{ duration: value >= 1 ? 0.12 : 0.18, ease: 'linear' }} />
      )}
    </div>
  )
}

export function RunProgress({ stage, progress }: { stage: string; progress: Progress | null }) {
  const pct = progress ? (progress.bytesTotal > 0 ? progress.bytesDone / progress.bytesTotal : progress.filesTotal > 0 ? progress.filesDone / progress.filesTotal : 0) : null
  return (
    <div className="space-y-3">
      <div className="flex items-baseline justify-between gap-3 text-sm">
        <div className="flex min-w-0 items-baseline gap-2 font-medium">
          <AnimatePresence mode="wait" initial={false}>
            <motion.span key={stage} initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -6 }} transition={{ duration: 0.3, ease: EASE }} className="truncate">
              {stage}
            </motion.span>
          </AnimatePresence>
          {progress && <span className="shrink-0 text-muted-foreground">· {progress.stage}</span>}
        </div>
        {pct !== null && <div className="font-mono text-base tabular-nums text-brand">{Math.round(pct * 100)}%</div>}
      </div>
      <PixelBar value={pct} />
      {progress && (
        <div className="flex flex-wrap gap-x-5 gap-y-1 text-xs text-muted-foreground tabular-nums">
          <span>
            {progress.filesDone} / {progress.filesTotal} 个文件
          </span>
          <span>
            {formatBytes(progress.bytesDone)} / {formatBytes(progress.bytesTotal)}
          </span>
          {progress.speed > 0 && <span className="text-foreground/70">{formatBytes(progress.speed)}/s</span>}
        </div>
      )}
    </div>
  )
}
