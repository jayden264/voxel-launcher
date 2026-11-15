import { create } from 'zustand'

/** The two hues (OKLCH degrees) the accent gradient runs between. */
export const ACCENTS = [
  { id: 'emerald', label: '翠绿', a: 152, b: 195 },
  { id: 'azure', label: '湛蓝', a: 245, b: 215 },
  { id: 'amethyst', label: '紫晶', a: 305, b: 265 },
  { id: 'sunset', label: '落日', a: 55, b: 25 },
  { id: 'rose', label: '玫红', a: 350, b: 320 },
  { id: 'gold', label: '金黄', a: 95, b: 70 },
] as const

export type AccentId = (typeof ACCENTS)[number]['id']

export interface Theme {
  accent: AccentId
  /** Pure black surfaces, for OLED screens. */
  oled: boolean
  /** A picture behind the interface, as a data URL. */
  bg: string | null
  /** How much the interface covers the picture, 0.3–0.9. */
  bgDim: number
}

const KEY = 'voxel.theme'
const DEFAULT: Theme = { accent: 'emerald', oled: false, bg: null, bgDim: 0.72 }

function load(): Theme {
  try {
    const raw = localStorage.getItem(KEY)
    if (!raw) return DEFAULT
    const t = JSON.parse(raw) as Partial<Theme>
    return {
      accent: ACCENTS.some((a) => a.id === t.accent) ? (t.accent as AccentId) : DEFAULT.accent,
      oled: t.oled === true,
      bg: typeof t.bg === 'string' && t.bg.startsWith('data:image/') ? t.bg : null,
      bgDim: typeof t.bgDim === 'number' ? Math.min(0.9, Math.max(0.3, t.bgDim)) : DEFAULT.bgDim,
    }
  } catch {
    return DEFAULT
  }
}

/** Returns false when the browser refused to store it (quota, private mode). */
function save(t: Theme): boolean {
  try {
    localStorage.setItem(KEY, JSON.stringify(t))
    return true
  } catch {
    return false
  }
}

export function applyTheme(t: Theme) {
  const a = ACCENTS.find((x) => x.id === t.accent) ?? ACCENTS[0]
  const root = document.documentElement
  root.style.setProperty('--hue-a', String(a.a))
  root.style.setProperty('--hue-b', String(a.b))
  root.dataset.oled = t.oled ? '1' : '0'
}

interface ThemeStore extends Theme {
  set: (patch: Partial<Theme>) => boolean
}

export const useTheme = create<ThemeStore>((set, get) => ({
  ...load(),
  set: (patch) => {
    const { accent, oled, bg, bgDim } = { ...get(), ...patch }
    const next = { accent, oled, bg, bgDim }
    const saved = save(next)
    applyTheme(next)
    set(next)
    return saved
  },
}))

/** Apply the saved theme before the first paint. */
export function initTheme() {
  applyTheme(useTheme.getState())
}

/** Shrink a picked image to something a stylesheet and localStorage can carry. */
export async function imageToBackground(file: File): Promise<string> {
  const url = URL.createObjectURL(file)
  try {
    const img = await new Promise<HTMLImageElement>((resolve, reject) => {
      const i = new Image()
      i.onload = () => resolve(i)
      i.onerror = () => reject(new Error('无法读取这张图片'))
      i.src = url
    })
    const scale = Math.min(1, 1920 / img.width)
    const canvas = document.createElement('canvas')
    canvas.width = Math.max(1, Math.round(img.width * scale))
    canvas.height = Math.max(1, Math.round(img.height * scale))
    canvas.getContext('2d')!.drawImage(img, 0, 0, canvas.width, canvas.height)
    return canvas.toDataURL('image/jpeg', 0.82)
  } finally {
    URL.revokeObjectURL(url)
  }
}
