import { useState } from 'react'
import { AnimatePresence, motion } from 'motion/react'
import { Check, Plus, Trash2 } from 'lucide-react'
import { api } from '@/lib/api'
import { EASE, rise } from '@/lib/motion'
import { useStore } from '@/lib/store'
import { PixelAvatar, PixelSprite } from '@/components/art'
import { Bezel, Eyebrow, PillButton } from '@/components/Bezel'
import { PageBanner } from '@/components/kit'
import { Input } from '@/components/ui/input'

const NAME_RE = /^[A-Za-z0-9_]{3,16}$/

export function AccountsPage() {
  const snap = useStore((s) => s.snap)
  const mutate = useStore((s) => s.mutate)
  const [name, setName] = useState('')
  if (!snap) return null
  const accounts = snap.settings.accounts
  const selected = snap.settings.selected_account
  const trimmed = name.trim()
  const invalid = trimmed.length > 0 && !NAME_RE.test(trimmed)

  const add = async () => {
    if (!NAME_RE.test(trimmed)) return
    if (await mutate(() => api.addAccount(trimmed), `已添加 ${trimmed}`)) setName('')
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageBanner biome="night" seed="accounts" eyebrow="Accounts" title="账号" subtitle="离线账号只需要一个玩家名，适合单人游戏和支持离线登录的服务器。" />
      <div className="min-h-0 flex-1 overflow-auto pr-1">
        <div className="mx-auto grid max-w-4xl gap-5 pb-4 lg:grid-cols-[minmax(0,1fr)_300px]">
          <div className="space-y-3">
            <AnimatePresence initial={false} mode="popLayout">
              {accounts.map((a, i) => {
                const active = a.name === selected
                return (
                  <motion.div key={a.name} layout variants={rise} initial="hidden" animate="show" exit={{ opacity: 0, x: -24, transition: { duration: 0.3, ease: EASE } }} custom={i}>
                    <Bezel radius={30} interactive={!active} className={active ? '!shadow-[0_0_0_1px_color-mix(in_oklch,var(--brand)_45%,transparent),0_36px_70px_-38px_var(--brand)]' : ''}>
                      <div className="flex items-center gap-4 p-4">
                        <PixelAvatar seed={a.uuid} size={56} className="rounded-2xl" />
                        <div className="min-w-0 flex-1">
                          <div className="flex items-center gap-2">
                            <span className="truncate text-lg font-semibold">{a.name}</span>
                            {active && <Eyebrow>使用中</Eyebrow>}
                          </div>
                          <div className="selectable mt-0.5 truncate font-mono text-xs text-muted-foreground">{a.uuid}</div>
                        </div>
                        {!active && (
                          <PillButton size="sm" tone="soft" icon={<Check className="size-3.5" />} onClick={() => mutate(() => api.selectAccount(a.name))}>
                            使用
                          </PillButton>
                        )}
                        <button type="button" aria-label="删除账号" onClick={() => mutate(() => api.removeAccount(a.name), '已删除')} className="grid size-9 place-items-center rounded-full text-muted-foreground transition-[background,color,transform] duration-500 [transition-timing-function:var(--ease)] hover:bg-red-500/15 hover:text-red-300 active:scale-90">
                          <Trash2 className="size-4" />
                        </button>
                      </div>
                    </Bezel>
                  </motion.div>
                )
              })}
            </AnimatePresence>
            {accounts.length === 0 && (
              <Bezel radius={30}>
                <div className="flex flex-col items-center gap-3 px-6 py-12 text-center text-sm text-muted-foreground">
                  <PixelSprite name="ender" size={56} bob />
                  还没有账号，在右侧输入一个玩家名开始。
                </div>
              </Bezel>
            )}
          </div>

          <div className="space-y-4">
            <Bezel radius={30}>
              <div className="p-5">
                <Eyebrow>添加玩家</Eyebrow>
                <Input value={name} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && add()} placeholder="玩家名，例如 Steve" maxLength={16} aria-invalid={invalid} className="mt-4 h-11 rounded-full px-4 text-base" />
                <p className={`mt-2 px-1 text-xs transition-colors duration-500 ${invalid ? 'text-red-300' : 'text-muted-foreground'}`}>3–16 位，只能包含字母、数字和下划线。</p>
                <PillButton className="mt-4 w-full justify-between" icon={<Plus className="size-4" />} onClick={add} disabled={!NAME_RE.test(trimmed)}>
                  添加账号
                </PillButton>
              </div>
            </Bezel>
            <Bezel radius={30}>
              <div className="flex items-start gap-3 p-5 text-sm text-muted-foreground">
                <PixelSprite name="heart" size={28} bob className="mt-0.5" />
                <p>离线模式仅供没有正版账号时体验。如果你喜欢 Minecraft，欢迎购买正版支持开发者；正版登录将在后续版本加入。</p>
              </div>
            </Bezel>
          </div>
        </div>
      </div>
    </div>
  )
}
