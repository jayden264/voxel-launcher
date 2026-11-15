import { useMemo } from 'react'
import { SPRITES, seeded } from '@/lib/pixel'
import { cn } from '@/lib/utils'

/** Blocky 8x8 mirrored avatar, tinted from the seed. */
export function PixelAvatar({ seed, size = 40, className = '' }: { seed: string; size?: number; className?: string }) {
  const { cells, hue } = useMemo(() => {
    const r = seeded(seed)
    const hue = Math.floor(r() * 360)
    const cells: boolean[][] = []
    for (let y = 0; y < 8; y++) {
      const half = Array.from({ length: 4 }, () => r() > 0.42)
      cells.push([...half, ...[...half].reverse()])
    }
    return { cells, hue }
  }, [seed])
  const fg = `oklch(0.78 0.15 ${hue})`
  const bg = `oklch(0.3 0.06 ${hue})`
  return (
    <svg width={size} height={size} viewBox="0 0 8 8" className={cn('shrink-0 rounded-lg', className)} shapeRendering="crispEdges">
      <rect width="8" height="8" fill={bg} />
      {cells.map((row, y) => row.map((on, x) => (on ? <rect key={`${x}-${y}`} x={x} y={y} width="1" height="1" fill={fg} /> : null)))}
    </svg>
  )
}

/** A hand-drawn 10x10 pixel sprite (see SPRITES). */
export function PixelSprite({ name, size = 40, bob = false, className }: { name: keyof typeof SPRITES; size?: number; bob?: boolean; className?: string }) {
  const s = SPRITES[name]
  const rects = useMemo(() => {
    const out: { x: number; y: number; c: string }[] = []
    s.rows.forEach((row, y) => {
      for (let x = 0; x < row.length; x++) {
        const c = s.map[row[x]]
        if (c) out.push({ x, y, c })
      }
    })
    return out
  }, [s])
  return (
    <svg width={size} height={size} viewBox="0 0 10 10" shapeRendering="crispEdges" className={cn('shrink-0', bob && 'px-bob', className)} aria-hidden>
      {rects.map((r) => (
        <rect key={`${r.x}-${r.y}`} x={r.x} y={r.y} width="1" height="1" fill={r.c} />
      ))}
    </svg>
  )
}

export function LogoMark({ size = 28 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 64 64" aria-hidden>
      <defs>
        <linearGradient id="lm-top" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#8bf0a8" />
          <stop offset="1" stopColor="#34c26a" />
        </linearGradient>
      </defs>
      <polygon points="32,6 56,19 32,32 8,19" fill="url(#lm-top)" />
      <polygon points="8,19 32,32 32,58 8,45" fill="#1f9450" />
      <polygon points="56,19 32,32 32,58 56,45" fill="#0f5a30" />
    </svg>
  )
}
