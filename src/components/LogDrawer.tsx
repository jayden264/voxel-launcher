import { useEffect, useRef, useState } from 'react'
import { AnimatePresence, motion } from 'motion/react'
import { ChevronDown, Copy, Eraser } from 'lucide-react'
import { toast } from 'sonner'
import { useStore } from '@/lib/store'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

function levelClass(line: string) {
  if (/\/(ERROR|FATAL)\]/.test(line) || /^(Exception|Caused by|\tat )/.test(line)) return 'text-red-300'
  if (/\/WARN\]/.test(line)) return 'text-amber-300'
  if (/\/DEBUG\]/.test(line)) return 'text-muted-foreground/60'
  if (line.startsWith('[启动器]')) return 'text-brand'
  return 'text-foreground/80'
}

export function LogDrawer() {
  const open = useStore((s) => s.logOpen)
  const setOpen = useStore((s) => s.setLogOpen)
  const logs = useStore((s) => s.logs)
  const clear = () => useStore.setState({ logs: [] })
  const box = useRef<HTMLDivElement>(null)
  const [stick, setStick] = useState(true)

  useEffect(() => {
    if (open && stick && box.current) box.current.scrollTop = box.current.scrollHeight
  }, [logs, open, stick])

  return (
    <AnimatePresence>
      {open && (
        <motion.div
          initial={{ height: 0, opacity: 0 }}
          animate={{ height: 260, opacity: 1 }}
          exit={{ height: 0, opacity: 0 }}
          transition={{ type: 'spring', stiffness: 400, damping: 38 }}
          className="overflow-hidden"
        >
          <div className="panel mt-4 flex h-[260px] flex-col overflow-hidden">
            <div className="flex items-center justify-between border-b border-border px-4 py-2">
              <div className="text-sm font-medium">
                游戏日志 <span className="ml-1 text-xs text-muted-foreground">{logs.length} 行</span>
              </div>
              <div className="flex items-center gap-1">
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label="复制日志"
                  onClick={() => navigator.clipboard.writeText(logs.join('\n')).then(() => toast.success('日志已复制'))}
                >
                  <Copy />
                </Button>
                <Button variant="ghost" size="icon-sm" aria-label="清空日志" onClick={clear}>
                  <Eraser />
                </Button>
                <Button variant="ghost" size="icon-sm" aria-label="收起" onClick={() => setOpen(false)}>
                  <ChevronDown />
                </Button>
              </div>
            </div>
            <div
              ref={box}
              onScroll={(e) => {
                const el = e.currentTarget
                setStick(el.scrollHeight - el.scrollTop - el.clientHeight < 24)
              }}
              className="selectable flex-1 overflow-auto px-4 py-2 font-mono text-[12px] leading-5"
            >
              {logs.length === 0 ? (
                <div className="grid h-full place-items-center text-muted-foreground">启动游戏后，日志会实时显示在这里</div>
              ) : (
                logs.map((l, i) => (
                  <div key={i} className={cn('break-all whitespace-pre-wrap', levelClass(l))}>
                    {l}
                  </div>
                ))
              )}
            </div>
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  )
}
