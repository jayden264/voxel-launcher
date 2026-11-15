import type { ReactNode } from 'react'
import { motion } from 'motion/react'
import { cn } from '@/lib/utils'
import { rise } from '@/lib/motion'
import type { LoaderKind, LoaderRef } from '@/lib/api'
import type { Biome } from '@/lib/pixel'
import type { SPRITES } from '@/lib/pixel'
import { Bezel, Eyebrow } from './Bezel'
import { PixelSprite } from './art'
import { Scene } from './Scene'

/** Illustrated page header: a pixel landscape strip with the title set over it. */
export function PageBanner({
  biome,
  eyebrow,
  title,
  subtitle,
  actions,
  seed,
}: {
  biome: Biome
  eyebrow: string
  title: string
  subtitle?: string
  actions?: ReactNode
  seed?: string
}) {
  return (
    <Bezel radius={30} className="mb-5 shrink-0">
      <div className="relative h-[150px]">
        <Scene seed={seed ?? title} biome={biome} particles={7} />
        <div className="absolute inset-0 bg-gradient-to-r from-[oklch(0.13_0.02_255)]/90 via-[oklch(0.13_0.02_255)]/45 to-transparent" />
        <div className="relative flex h-full items-end justify-between gap-6 p-6">
          <div className="min-w-0">
            <motion.div variants={rise} initial="hidden" animate="show" custom={0}>
              <Eyebrow>{eyebrow}</Eyebrow>
            </motion.div>
            <div className="mt-2.5 overflow-hidden">
              <motion.h1
                initial={{ y: '110%' }}
                animate={{ y: 0 }}
                transition={{ duration: 0.8, ease: [0.32, 0.72, 0, 1], delay: 0.08 }}
                className="text-[34px] leading-[1.1] font-semibold tracking-tight"
              >
                {title}
              </motion.h1>
            </div>
            {subtitle && (
              <motion.p variants={rise} initial="hidden" animate="show" custom={2} className="mt-1.5 max-w-xl text-sm text-foreground/65">
                {subtitle}
              </motion.p>
            )}
          </div>
          {actions && (
            <motion.div variants={rise} initial="hidden" animate="show" custom={3} className="flex shrink-0 items-center gap-2">
              {actions}
            </motion.div>
          )}
        </div>
      </div>
    </Bezel>
  )
}

export function Segmented<T extends string>({
  value,
  onChange,
  options,
  className,
}: {
  value: T
  onChange: (v: T) => void
  options: { value: T; label: string; disabled?: boolean; hint?: string }[]
  className?: string
}) {
  return (
    <div className={cn('relative inline-flex rounded-full bg-black/25 p-1 shadow-[inset_0_0_0_1px_oklch(1_0_0/8%)]', className)}>
      {options.map((o) => {
        const active = value === o.value
        return (
          <button
            key={o.value}
            type="button"
            disabled={o.disabled}
            title={o.hint}
            onClick={() => onChange(o.value)}
            className={cn('relative rounded-full px-3.5 py-1.5 text-sm whitespace-nowrap transition-colors duration-500 [transition-timing-function:var(--ease)]', o.disabled && 'cursor-not-allowed opacity-40', active ? 'text-foreground' : 'text-muted-foreground', !o.disabled && !active && 'hover:text-foreground')}
          >
            {active && (
              <motion.span
                layoutId={`seg-${options.map((x) => x.value).join('-')}`}
                className="absolute inset-0 rounded-full bg-white/12 shadow-[inset_0_1px_0_oklch(1_0_0/14%)]"
                transition={{ type: 'spring', stiffness: 500, damping: 38 }}
              />
            )}
            <span className="relative">{o.label}</span>
          </button>
        )
      })}
    </div>
  )
}

export function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div className="space-y-1.5">
      <div className="flex items-baseline justify-between gap-3">
        <label className="text-sm font-medium">{label}</label>
        {hint && <span className="text-xs text-muted-foreground">{hint}</span>}
      </div>
      {children}
    </div>
  )
}

export const LOADER_LABEL: Record<LoaderKind, string> = { fabric: 'Fabric', quilt: 'Quilt', forge: 'Forge', neoforge: 'NeoForge' }

/** Small pill showing an instance's loader, e.g. "Fabric 0.20.1". */
export function LoaderBadge({ loader, className }: { loader: LoaderRef; className?: string }) {
  return (
    <span className={cn('inline-flex items-center rounded-full bg-brand-2/15 px-2 py-0.5 text-[11px] font-medium text-brand-2', className)}>
      {LOADER_LABEL[loader.kind]} {loader.version}
    </span>
  )
}

export function KindBadge({ kind }: { kind: string }) {
  const release = kind === 'release'
  return (
    <span className={cn('inline-flex items-center rounded-full px-2 py-0.5 text-[11px] font-medium', release ? 'bg-brand/14 text-brand' : 'bg-amber-400/14 text-amber-300')}>
      {release ? '正式版' : '快照'}
    </span>
  )
}

export function Chip({ children, tone = 'plain' }: { children: ReactNode; tone?: 'plain' | 'brand' | 'warn' | 'bad' }) {
  const cls = { plain: 'bg-white/8 text-foreground/75', brand: 'bg-brand/14 text-brand', warn: 'bg-amber-400/14 text-amber-300', bad: 'bg-red-500/16 text-red-300' }[tone]
  return <span className={cn('inline-flex shrink-0 items-center rounded-full px-2 py-0.5 text-[11px] font-medium', cls)}>{children}</span>
}

export function Empty({ sprite, title, hint, action }: { sprite: keyof typeof SPRITES; title: string; hint?: string; action?: ReactNode }) {
  return (
    <Bezel radius={30}>
      <div className="relative flex flex-col items-center justify-center gap-4 px-6 py-16 text-center">
        <div className="absolute top-1/2 left-1/2 size-56 -translate-x-1/2 -translate-y-[62%] rounded-full bg-brand/10 blur-3xl" />
        <PixelSprite name={sprite} size={72} bob className="relative drop-shadow-[0_10px_18px_oklch(0_0_0/40%)]" />
        <div className="relative">
          <div className="text-lg font-semibold">{title}</div>
          {hint && <div className="mt-1.5 text-sm text-muted-foreground">{hint}</div>}
        </div>
        {action && <div className="relative">{action}</div>}
      </div>
    </Bezel>
  )
}
