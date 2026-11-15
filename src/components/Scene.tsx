import { useMemo, type CSSProperties } from 'react'
import { motion, useSpring, useTransform, type MotionValue } from 'motion/react'
import { H, W, renderScene, seeded, type Biome, type ParticleKind } from '@/lib/pixel'
import { cn } from '@/lib/utils'

export interface Pointer {
  x: MotionValue<number>
  y: MotionValue<number>
}

/** Pointer position in [-1, 1] for parallax. Spring-smoothed so it feels weighty. */
export function usePointer(): Pointer & { onMove: (e: React.PointerEvent<HTMLElement>) => void; onLeave: () => void } {
  const x = useSpring(0, { stiffness: 90, damping: 18, mass: 0.8 })
  const y = useSpring(0, { stiffness: 90, damping: 18, mass: 0.8 })
  return {
    x,
    y,
    onMove: (e) => {
      const r = e.currentTarget.getBoundingClientRect()
      x.set(((e.clientX - r.left) / r.width) * 2 - 1)
      y.set(((e.clientY - r.top) / r.height) * 2 - 1)
    },
    onLeave: () => {
      x.set(0)
      y.set(0)
    },
  }
}

function Layer({ src, depth, pointer, className }: { src: string; depth: number; pointer?: Pointer; className?: string }) {
  const zero = useSpring(0)
  const tx = useTransform(pointer?.x ?? zero, (v) => v * -depth * 1.4)
  const ty = useTransform(pointer?.y ?? zero, (v) => v * -depth * 0.7)
  return (
    <motion.img
      src={src}
      alt=""
      draggable={false}
      style={{ x: tx, y: ty, scale: 1.06 }}
      className={cn('px-img absolute inset-0 size-full', className)}
    />
  )
}

interface ParticleSpec {
  left: number
  top: number
  size: number
  dx: number
  dur: number
  delay: number
}

function makeParticles(seed: string, count: number, kind: ParticleKind): ParticleSpec[] {
  const r = seeded(`p:${seed}:${kind}`)
  return Array.from({ length: count }, () => ({
    left: r() * 100,
    top: kind === 'firefly' ? 45 + r() * 45 : kind === 'snow' ? r() * 30 : 55 + r() * 40,
    size: kind === 'ember' || kind === 'ender' ? 2 + Math.floor(r() * 2) : 2,
    dx: (r() - 0.5) * 60,
    dur: 5 + r() * 7,
    delay: -r() * 10,
  }))
}

/**
 * A layered pixel-art scene. Fills its (relatively positioned) parent like a
 * background image would, always aligned to the bottom/centre so the horizon
 * stays visible at any card size.
 */
export function Scene({
  seed,
  biome,
  pointer,
  particles = 0,
  glow = true,
  className,
}: {
  seed: string
  biome?: Biome
  pointer?: Pointer
  particles?: number
  glow?: boolean
  className?: string
}) {
  const scene = useMemo(() => renderScene(seed, biome), [seed, biome])
  const dots = useMemo(() => makeParticles(seed, particles, scene.particles), [seed, particles, scene.particles])

  return (
    <div className={cn('scene', className)} aria-hidden>
      <div className="scene-stack">
        <Layer src={scene.layers.sky} depth={0} />
        {glow && scene.sun && (
          <span
            className="px-glow"
            style={{
              left: `${(scene.sun.x / W) * 100}%`,
              top: `${(scene.sun.y / H) * 100}%`,
              background: `radial-gradient(closest-side, color-mix(in oklch, ${scene.glow} 55%, transparent), transparent)`,
            }}
          />
        )}
        {scene.layers.clouds && (
          <div className="px-clouds">
            <img src={scene.layers.clouds} alt="" draggable={false} className="px-img" />
          </div>
        )}
        <Layer src={scene.layers.far} depth={3} pointer={pointer} />
        <Layer src={scene.layers.mid} depth={7} pointer={pointer} />
        {dots.length > 0 && (
          <div className="absolute inset-0">
            {dots.map((p, i) => (
              <span
                key={i}
                className={`px-p px-p--${scene.particles}`}
                style={
                  {
                    left: `${p.left}%`,
                    top: `${p.top}%`,
                    width: p.size,
                    height: p.size,
                    background: scene.tint,
                    '--dx': `${p.dx}px`,
                    '--dur': `${p.dur}s`,
                    animationDelay: `${p.delay}s`,
                  } as CSSProperties
                }
              />
            ))}
          </div>
        )}
        <Layer src={scene.layers.near} depth={14} pointer={pointer} />
      </div>
    </div>
  )
}
