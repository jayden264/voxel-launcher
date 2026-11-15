// Pixel-art scene engine.
//
// Scenes are painted on a tiny 128x72 canvas, one real pixel per art pixel, then
// scaled up with `image-rendering: pixelated` so every "pixel" is a crisp block.
// Each scene is split into layers (sky / far / mid / near / clouds) so the UI can
// move them at different speeds (parallax) using transforms only.

export const W = 128
export const H = 72

/** Small deterministic PRNG so the same seed always paints the same picture. */
export function seeded(seed: string) {
  let h = 1779033703 ^ seed.length
  for (let i = 0; i < seed.length; i++) {
    h = Math.imul(h ^ seed.charCodeAt(i), 3432918353)
    h = (h << 13) | (h >>> 19)
  }
  return () => {
    h = Math.imul(h ^ (h >>> 16), 2246822507)
    h = Math.imul(h ^ (h >>> 13), 3266489909)
    h ^= h >>> 16
    return (h >>> 0) / 4294967296
  }
}
type Rng = () => number

export type Biome = 'meadow' | 'taiga' | 'desert' | 'nether' | 'end' | 'night'
export const BIOMES: Biome[] = ['meadow', 'night', 'taiga', 'desert', 'nether', 'end']
export type ParticleKind = 'pollen' | 'firefly' | 'snow' | 'dust' | 'ember' | 'ender'

export interface SceneData {
  biome: Biome
  layers: { sky: string; far: string; mid: string; near: string; clouds: string | null }
  /** Sun / moon centre in art pixels (for the glow overlay). */
  sun: { x: number; y: number } | null
  glow: string
  particles: ParticleKind
  /** Particle tint. */
  tint: string
}

// ----------------------------------------------------------------- colour

type RGB = [number, number, number]
const rgb = (hex: string): RGB => [parseInt(hex.slice(1, 3), 16), parseInt(hex.slice(3, 5), 16), parseInt(hex.slice(5, 7), 16)]
const css = (c: RGB) => `rgb(${Math.round(c[0])},${Math.round(c[1])},${Math.round(c[2])})`
const mix = (a: RGB, b: RGB, t: number): RGB => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
const shade = (c: RGB, f: number): RGB => (f < 0 ? mix(c, [0, 0, 0], -f) : mix(c, [255, 255, 255], f))

type Ctx = CanvasRenderingContext2D
interface Layers {
  sky: Ctx
  far: Ctx
  mid: Ctx
  near: Ctx
}

function makeCanvas(width = W): [HTMLCanvasElement, Ctx] {
  const c = document.createElement('canvas')
  c.width = width
  c.height = H
  const ctx = c.getContext('2d')!
  ctx.imageSmoothingEnabled = false
  return [c, ctx]
}

const dot = (ctx: Ctx, x: number, y: number, color: string | RGB, w = 1, h = 1) => {
  ctx.fillStyle = typeof color === 'string' ? color : css(color)
  ctx.fillRect(Math.round(x), Math.round(y), w, h)
}

// ------------------------------------------------------------------ sprites

type Sprite = { rows: string[]; map: Record<string, string> }

function stamp(ctx: Ctx, s: Sprite, cx: number, groundY: number) {
  const h = s.rows.length
  const w = s.rows[0].length
  const x0 = Math.round(cx - w / 2)
  const y0 = Math.round(groundY - h + 1)
  for (let j = 0; j < h; j++) {
    for (let i = 0; i < w; i++) {
      const c = s.map[s.rows[j][i]]
      if (c) dot(ctx, x0 + i, y0 + j, c)
    }
  }
}

const oak = (leaf: string, dark: string, trunk: string): Sprite => ({
  rows: [
    '..GGGGG..',
    '.GGGGGGG.',
    'GGGdGGGGG',
    'GGGGGGdGG',
    'GdGGGGGGG',
    '.GGGGGdG.',
    '..GGGGG..',
    '....T....',
    '....T....',
    '....T....',
  ],
  map: { G: leaf, d: dark, T: trunk },
})
const oakSmall = (leaf: string, dark: string, trunk: string): Sprite => ({
  rows: ['.GGG.', 'GGdGG', 'GGGGG', '.GGG.', '..T..', '..T..'],
  map: { G: leaf, d: dark, T: trunk },
})
const spruce = (leaf: string, snow: string | null, trunk: string): Sprite => ({
  rows: [
    '...S...',
    '..GGG..',
    '...G...',
    '..GGG..',
    '.GGSGG.',
    '..GGG..',
    '.GGGGG.',
    'GGSGGGG',
    '...T...',
    '...T...',
  ],
  map: { G: leaf, S: snow ?? leaf, T: trunk },
})
const cactus = (c: string, d: string): Sprite => ({
  rows: ['..C..', 'C.C.C', 'C.CdC', 'CCCC.', '..C..', '..Cd.', '..C..'],
  map: { C: c, d },
})
const cabin = (wall: string, roof: string, lit: string): Sprite => ({
  rows: [
    '...RRRRRR...',
    '..RRRRRRRR..',
    '.RRRRRRRRRR.',
    'RRRRRRRRRRRR',
    '.WWWWWWWWWW.',
    '.WLLWWWWDDW.',
    '.WLLWWWWDDW.',
    '.WWWWWWWDDW.',
  ],
  map: { W: wall, R: roof, L: lit, D: '#1a1210' },
})
const crystal = (core: string, edge: string): Sprite => ({
  rows: ['.E.', 'ECE', 'ECE', '.E.'],
  map: { E: edge, C: core },
})

// -------------------------------------------------------------- primitives

/** Integer height map built from a few stacked sine waves. */
function heights(r: Rng, base: number, amp: number): number[] {
  const waves = 3
  const phase = Array.from({ length: waves }, () => r() * Math.PI * 2)
  const freq = Array.from({ length: waves }, (_, i) => ((1 + i * 1.7) * (0.9 + r() * 0.4) * Math.PI * 2 * 1.1) / W)
  return Array.from({ length: W }, (_, x) => {
    let v = 0
    for (let i = 0; i < waves; i++) v += Math.sin(x * freq[i] + phase[i]) / (i + 1)
    return Math.max(Math.round(H * 0.28), Math.min(H - 2, Math.round(base + v * amp * 0.6)))
  })
}

function fillHills(ctx: Ctx, ys: number[], base: RGB, top = 0.26) {
  for (let x = 0; x < W; x++) {
    for (let y = ys[x]; y < H; y++) {
      const d = y - ys[x]
      let c: RGB
      if (d === 0) c = shade(base, top)
      else if (d === 1) c = shade(base, top * 0.35)
      else {
        const depth = Math.min(d / 20, 1)
        c = shade(base, -0.28 * depth)
        // ordered dither between darkness steps keeps the pixel-art look
        if ((x + y) % 2 === 0 && d % 7 === 0) c = shade(c, -0.07)
      }
      dot(ctx, x, y, c)
    }
  }
}

function skyGradient(ctx: Ctx, stops: string[]) {
  const cols = stops.map(rgb)
  const bands = 9
  const limit = Math.round(H * 0.86)
  const bandColor = (b: number): RGB => {
    const t = Math.min(1, Math.max(0, b / (bands - 1)))
    const seg = t * (cols.length - 1)
    const i = Math.min(cols.length - 2, Math.floor(seg))
    return mix(cols[i], cols[i + 1], seg - i)
  }
  for (let y = 0; y < limit; y++) {
    const t = (y / limit) * bands
    const b = Math.floor(t)
    const f = t - b
    for (let x = 0; x < W; x++) {
      let band = b
      if (f > 0.72) band += (x + y) & 1
      else if (f > 0.5) band += x & 1 && y & 1 ? 1 : 0
      dot(ctx, x, y, bandColor(band))
    }
  }
  // below the lowest band: continue with the horizon colour
  for (let y = limit; y < H; y++) for (let x = 0; x < W; x++) dot(ctx, x, y, cols[cols.length - 1])
}

function stars(ctx: Ctx, r: Rng, count: number, color: string, maxY = H * 0.5) {
  for (let i = 0; i < count; i++) {
    const x = Math.floor(r() * W)
    const y = Math.floor(r() * maxY)
    const bright = r() > 0.82
    ctx.globalAlpha = bright ? 0.95 : 0.35 + r() * 0.35
    dot(ctx, x, y, color)
    if (bright && r() > 0.5) {
      ctx.globalAlpha = 0.5
      dot(ctx, x - 1, y, color)
      dot(ctx, x + 1, y, color)
      dot(ctx, x, y - 1, color)
      dot(ctx, x, y + 1, color)
    }
  }
  ctx.globalAlpha = 1
}

function disc(ctx: Ctx, cx: number, cy: number, radius: number, color: string, glow: string) {
  const g = rgb(glow)
  for (let y = -radius - 3; y <= radius + 3; y++) {
    for (let x = -radius - 3; x <= radius + 3; x++) {
      const d2 = x * x + y * y
      if (d2 <= radius * radius) dot(ctx, cx + x, cy + y, d2 > (radius - 1.4) ** 2 ? shade(rgb(color), -0.1) : color)
      else if (d2 <= (radius + 2) ** 2 && (x + y) % 2 === 0) {
        ctx.globalAlpha = 0.55
        dot(ctx, cx + x, cy + y, g)
        ctx.globalAlpha = 1
      } else if (d2 <= (radius + 3.4) ** 2 && (x + y) % 4 === 0) {
        ctx.globalAlpha = 0.25
        dot(ctx, cx + x, cy + y, g)
        ctx.globalAlpha = 1
      }
    }
  }
}

function cloudStrip(r: Rng, color: string, count: number): HTMLCanvasElement {
  const [canvas, ctx] = makeCanvas(W * 2)
  const c = rgb(color)
  const shadow = shade(c, -0.18)
  const clouds = Array.from({ length: count }, (_, i) => ({
    x: Math.floor(((i + r() * 0.7) / count) * W),
    y: 5 + Math.floor(r() * 20),
    w: 14 + Math.floor(r() * 16),
  }))
  for (const k of [0, W]) {
    for (const cl of clouds) {
      ctx.globalAlpha = 0.92
      dot(ctx, cl.x + k + 3, cl.y, c, cl.w - 8, 1)
      dot(ctx, cl.x + k, cl.y + 1, c, cl.w, 2)
      ctx.globalAlpha = 0.8
      dot(ctx, cl.x + k + 2, cl.y + 3, shadow, cl.w - 5, 1)
    }
  }
  ctx.globalAlpha = 1
  return canvas
}

// ------------------------------------------------------------------- biomes

interface BiomeDef {
  sky: string[]
  stars: number
  starColor?: string
  sun: { color: string; glow: string; r: number; y: number; x: [number, number] } | null
  hills: { far: [number, number]; mid: [number, number]; near: [number, number] }
  colors: { far: string; mid: string; near: string }
  clouds?: { color: string; count: number }
  particles: ParticleKind
  tint: string
  paint: (L: Layers, r: Rng, ys: { far: number[]; mid: number[]; near: number[] }) => void
}

function scatter(r: Rng, from: number, to: number, gap: [number, number], fn: (x: number) => void) {
  let x = from + Math.floor(r() * gap[0])
  while (x < to) {
    fn(x)
    x += gap[0] + Math.floor(r() * (gap[1] - gap[0]))
  }
}

const DEFS: Record<Biome, BiomeDef> = {
  // Dusky green meadow: oaks, flowers, tall grass, a soft pale sun.
  meadow: {
    sky: ['#0b3342', '#14594f', '#3f9b6a'],
    stars: 14,
    starColor: '#e8fff2',
    sun: { color: '#f7e9a0', glow: '#f7e9a0', r: 5, y: 21, x: [0.42, 0.58] },
    hills: { far: [46, 8], mid: [54, 7], near: [62, 5] },
    colors: { far: '#2f7a55', mid: '#236143', near: '#173f2c' },
    clouds: { color: '#d6f5e4', count: 3 },
    particles: 'pollen',
    tint: '#f7f2b0',
    paint({ mid, near }, r, ys) {
      const small = oakSmall('#1f6a44', '#17503a', '#3a2a1c')
      scatter(r, 4, W - 4, [9, 16], (x) => stamp(mid, small, x, ys.mid[x]))
      const big = oak('#2f8a52', '#1f6a44', '#4a3322')
      scatter(r, 8, W - 8, [18, 30], (x) => stamp(near, big, x, ys.near[x]))
      for (let x = 0; x < W; x++) {
        const y = ys.near[x]
        if (r() < 0.2) dot(near, x, y - 1, '#4fbf7a')
        if (r() < 0.035) dot(near, x, y - 1, r() > 0.5 ? '#ff6b7a' : '#ffd86b')
      }
    },
  },
  // Quiet spruce forest under a pale moon.
  night: {
    sky: ['#050f1c', '#0c2a3f', '#1d5a66'],
    stars: 44,
    starColor: '#dff3ff',
    sun: { color: '#e3f6ff', glow: '#9fe8ff', r: 5, y: 17, x: [0.36, 0.52] },
    hills: { far: [48, 7], mid: [55, 6], near: [63, 4] },
    colors: { far: '#12384a', mid: '#0e2c3b', near: '#091d28' },
    clouds: { color: '#6aa7b8', count: 2 },
    particles: 'firefly',
    tint: '#d8ff8a',
    paint({ far, mid, near }, r, ys) {
      const tiny = spruce('#0b2430', null, '#05121a')
      scatter(r, 2, W - 2, [6, 10], (x) => stamp(far, tiny, x, ys.far[x] + 1))
      const pine = spruce('#0a2a2c', null, '#061a1a')
      scatter(r, 3, W - 3, [8, 14], (x) => stamp(mid, pine, x, ys.mid[x] + 1))
      const cx = Math.round(W * 0.7)
      stamp(near, cabin('#2c3b3a', '#14262a', '#ffd36a'), cx, ys.near[cx] + 1)
      const front = spruce('#082024', null, '#050d12')
      scatter(r, 5, W - 5, [14, 24], (x) => {
        if (Math.abs(x - cx) > 9) stamp(near, front, x, ys.near[x] + 2)
      })
    },
  },
  // Snowy taiga: pale sky, drifted snow, frosted spruces.
  taiga: {
    sky: ['#18304a', '#4a6f96', '#b4d0e2'],
    stars: 18,
    starColor: '#ffffff',
    sun: { color: '#f4f8ff', glow: '#cfe6ff', r: 4, y: 18, x: [0.55, 0.7] },
    hills: { far: [46, 7], mid: [53, 6], near: [61, 5] },
    colors: { far: '#7f9db9', mid: '#a9c1d7', near: '#d3e3f0' },
    clouds: { color: '#e9f3fb', count: 3 },
    particles: 'snow',
    tint: '#ffffff',
    paint({ mid, near }, r, ys) {
      const far = spruce('#2d5f55', '#e8f3fb', '#3a2a1c')
      scatter(r, 3, W - 3, [8, 14], (x) => stamp(mid, far, x, ys.mid[x]))
      const big = spruce('#1f4a3f', '#f4f9ff', '#2e2016')
      scatter(r, 6, W - 6, [14, 24], (x) => stamp(near, big, x, ys.near[x] + 1))
      for (let x = 0; x < W; x++) if (r() < 0.05) dot(near, x, ys.near[x] + 2 + Math.floor(r() * 4), '#ffffff')
    },
  },
  // Dusk desert: huge low sun, dunes, a pyramid on the horizon.
  desert: {
    sky: ['#2f1b4a', '#b4493c', '#f4a85a'],
    stars: 12,
    starColor: '#ffe9c8',
    sun: { color: '#ffe9a8', glow: '#ffc470', r: 8, y: 36, x: [0.42, 0.56] },
    hills: { far: [50, 4], mid: [57, 5], near: [64, 4] },
    colors: { far: '#b45f3c', mid: '#d98a4a', near: '#e9b064' },
    particles: 'dust',
    tint: '#ffd9a0',
    paint({ mid, near }, r, ys) {
      const px = Math.round(W * 0.28)
      const gy = ys.mid[px]
      for (let i = 0; i < 12; i++) {
        const half = 12 - i
        dot(mid, px - half, gy - i, '#c98a52', half, 1)
        dot(mid, px, gy - i, '#9e6538', half, 1)
      }
      const cac = cactus('#3f8a4a', '#2b6a38')
      scatter(r, 10, W - 6, [18, 30], (x) => stamp(near, cac, x, ys.near[x] + 1))
      for (let x = 0; x < W; x++) if (r() < 0.14) dot(near, x, ys.near[x] + 1 + Math.floor(r() * 3), '#f6cd86')
    },
  },
  // Nether: crimson haze, glowstone ceiling, a lava river.
  nether: {
    sky: ['#1a0508', '#4e1013', '#a2371a'],
    stars: 0,
    sun: null,
    hills: { far: [48, 6], mid: [56, 6], near: [63, 4] },
    colors: { far: '#521316', mid: '#390c0f', near: '#260709' },
    particles: 'ember',
    tint: '#ff9a3c',
    paint({ far, mid, near }, r, ys) {
      // fortress silhouette on the far layer
      const fx = Math.round(W * 0.3)
      for (let i = 0; i < 18; i += 1) dot(far, fx + i, ys.far[fx + i] - 6 - ((i % 6) < 3 ? 2 : 0), '#2a0a0d', 1, 8)
      // ceiling with stalactites
      for (let x = 0; x < W; x++) {
        const depth = 4 + Math.floor(Math.abs(Math.sin(x * 0.21) * 4 + Math.sin(x * 0.07) * 3)) + Math.floor(r() * 2)
        for (let y = 0; y < depth; y++) dot(near, x, y, y > depth - 2 ? '#3a0c10' : '#2c080b')
        if (x % 17 === 5) {
          dot(near, x, depth, '#ffd24a', 2, 2)
          dot(near, x + 1, depth + 2, '#ff9a2a')
        }
      }
      // lava river with a bright core
      const x0 = Math.round(W * 0.38)
      for (let x = x0; x < x0 + 46; x++) {
        const y = ys.near[x]
        const t = (x - x0) / 46
        const wob = Math.round(Math.sin(t * Math.PI * 3) * 1)
        for (let i = 0; i < 3; i++) dot(near, x, y + i + wob, i === 0 ? '#ffb43a' : i === 1 ? '#ff6a1a' : '#c43a12')
      }
      void mid
    },
  },
  // The End: violet void, floating islands, obsidian spires with crystals.
  end: {
    sky: ['#07030f', '#150a28', '#2c1352'],
    stars: 64,
    starColor: '#e6d0ff',
    sun: { color: '#10051f', glow: '#a45bff', r: 7, y: 22, x: [0.46, 0.6] },
    hills: { far: [52, 6], mid: [58, 5], near: [65, 3] },
    colors: { far: '#2c1b48', mid: '#4a3770', near: '#948f66' },
    particles: 'ender',
    tint: '#d9a8ff',
    paint({ far, mid, near }, r, ys) {
      // floating islands
      for (let k = 0; k < 3; k++) {
        const cx = 18 + Math.floor(r() * (W - 36))
        const cy = 20 + Math.floor(r() * 18)
        const w = 8 + Math.floor(r() * 8)
        for (let i = -w; i <= w; i++) {
          dot(far, cx + i, cy, '#a49c72')
          dot(far, cx + i, cy + 1, '#7f7858')
          const drop = Math.round((1 - Math.abs(i) / w) * (4 + r() * 3))
          for (let j = 0; j < drop; j++) dot(far, cx + i, cy + 2 + j, j % 2 ? '#52476e' : '#645a85')
        }
      }
      // obsidian spires with crystals
      const cr = crystal('#ff7ad9', '#ffb0ee')
      scatter(r, 8, W - 8, [16, 26], (x) => {
        const h = 12 + Math.floor(r() * 10)
        const gy = ys.near[x]
        dot(near, x - 1, gy - h, '#120a1c', 3, h + 1)
        dot(near, x - 1, gy - h, '#2a1a3e', 1, h)
        stamp(near, cr, x, gy - h)
      })
      void mid
    },
  },
}

// -------------------------------------------------------------------- render

const cache = new Map<string, SceneData>()

export function pickBiome(seed: string): Biome {
  const r = seeded(`biome:${seed}`)
  return BIOMES[Math.floor(r() * BIOMES.length)]
}

export function renderScene(seed: string, biome: Biome = pickBiome(seed)): SceneData {
  const key = `${biome}|${seed}`
  const hit = cache.get(key)
  if (hit) return hit

  const def = DEFS[biome]
  const r = seeded(key)
  const [sky, skyCtx] = makeCanvas()
  const [far, farCtx] = makeCanvas()
  const [mid, midCtx] = makeCanvas()
  const [near, nearCtx] = makeCanvas()

  skyGradient(skyCtx, def.sky)
  if (def.stars) stars(skyCtx, r, def.stars, def.starColor ?? '#ffffff')
  let sun: SceneData['sun'] = null
  if (def.sun) {
    const cx = Math.round(W * (def.sun.x[0] + r() * (def.sun.x[1] - def.sun.x[0])))
    disc(skyCtx, cx, def.sun.y, def.sun.r, def.sun.color, def.sun.glow)
    sun = { x: cx, y: def.sun.y }
  }

  const ys = {
    far: heights(r, ...def.hills.far),
    mid: heights(r, ...def.hills.mid),
    near: heights(r, ...def.hills.near),
  }
  fillHills(farCtx, ys.far, rgb(def.colors.far))
  fillHills(midCtx, ys.mid, rgb(def.colors.mid))
  fillHills(nearCtx, ys.near, rgb(def.colors.near), 0.22)
  def.paint({ sky: skyCtx, far: farCtx, mid: midCtx, near: nearCtx }, r, ys)

  const clouds = def.clouds ? cloudStrip(r, def.clouds.color, def.clouds.count).toDataURL() : null
  const data: SceneData = {
    biome,
    layers: { sky: sky.toDataURL(), far: far.toDataURL(), mid: mid.toDataURL(), near: near.toDataURL(), clouds },
    sun,
    glow: def.sun?.glow ?? def.tint,
    particles: def.particles,
    tint: def.tint,
  }
  cache.set(key, data)
  return data
}

// ------------------------------------------------------------- icon sprites

/** 10x10 decorative sprites for empty states, badges and list icons. */
export const SPRITES: Record<string, Sprite> = {
  grass: {
    rows: [
      '..GGGGGG..',
      '.GGgGGgGG.',
      'GGGGGGGGGG',
      'GdGGdGGGdG',
      'DdDDdDDdDD',
      'DDDDDDDDdD',
      'DdDDDdDDDD',
      'DDDDDDDdDD',
      'dDDdDDDDDd',
      '.dddddddd.',
    ],
    map: { G: '#58c26a', g: '#7ee08c', d: '#6b4a2f', D: '#8a6240' },
  },
  tnt: {
    rows: [
      'RRRRRRRRRR',
      'RrRRrRRrRR',
      'WWWWWWWWWW',
      'WKKWKWKKKW',
      'WKWKKWKWKW',
      'WWWWWWWWWW',
      'RrRRrRRrRR',
      'RRRRRRRRRR',
      'RrRRrRRrRR',
      'rrrrrrrrrr',
    ],
    map: { R: '#d9473f', r: '#b8342e', W: '#ece6dc', K: '#2a2a2a' },
  },
  chest: {
    rows: [
      '.BBBBBBBB.',
      'BbbbbbbbbB',
      'BbbbbbbbbB',
      'DDDDYYDDDD',
      'BbbbYYbbbB',
      'BbbbbbbbbB',
      'BbbbbbbbbB',
      'BbbbbbbbbB',
      'BBBBBBBBBB',
      '.dddddddd.',
    ],
    map: { B: '#6b4423', b: '#8d5e32', D: '#3a2412', Y: '#e8c24a', d: '#3a2412' },
  },
  torch: {
    rows: [
      '....yy....',
      '...yOOy...',
      '...yOOy...',
      '....OO....',
      '....BB....',
      '....BB....',
      '....BB....',
      '....BB....',
      '....bb....',
      '....bb....',
    ],
    map: { y: '#ffe27a', O: '#ff9a2a', B: '#8d5e32', b: '#6b4423' },
  },
  heart: {
    rows: [
      '.RR...RR..',
      'RrrR.RrrR.',
      'RrWrRrrrR.',
      'RrrrrrrrR.',
      '.RrrrrrR..',
      '..RrrrR...',
      '...RrR....',
      '....R.....',
      '..........',
      '..........',
    ],
    map: { R: '#7a1a24', r: '#ef4a5a', W: '#ffd0d6' },
  },
  diamond: {
    rows: [
      '..........',
      '...CCCC...',
      '..CwCCcC..',
      '.CwCCCcCC.',
      'CCCCCCCcCC',
      '.CCCCCcCC.',
      '..CCCcCC..',
      '...CCcC...',
      '....CC....',
      '..........',
    ],
    map: { C: '#4be0d2', c: '#2aa8a0', w: '#c8fff8' },
  },
  ender: {
    rows: [
      '...PPPP...',
      '..PpppPP..',
      '.PpGGGpPP.',
      'PpGKKKGpPP',
      'PpGKKKGpPP',
      'PpGGGGGpPP',
      '.PpppppPP.',
      '..PPppPP..',
      '...PPPP...',
      '..........',
    ],
    map: { P: '#1f7a64', p: '#2fae8c', G: '#7ee8b8', K: '#0a2a22' },
  },
}
