import { AnimatePresence, motion } from 'motion/react'
import { EASE } from '@/lib/motion'
import { formatBytes, type Progress } from '@/lib/store'

/**
 * Segmented, Minecraft-style bar, in plain CSS on purpose: a progress bar is information, not
 * decoration, so it must keep working when the system asks for reduced motion. The waiting state
 * and the filled state are different elements (different keys), so one can never leave its
 * transform behind on the other.
 */
export function PixelBar({ value }: { value: number | null }) {
  return (
    <div className="px-bar" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={value === null ? undefined : Math.round(Math.min(1, Math.max(0, value)) * 100)}>
      {value === null ? (
        <div key="waiting" className="px-bar-fill px-bar-waiting progress-shimmer" />
      ) : (
        <div key="filled" className="px-bar-fill px-bar-filled progress-shimmer" style={{ transform: `scaleX(${Math.max(0.015, Math.min(1, value))})` }} />
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
