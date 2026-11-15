import { useState } from 'react'
import { toast } from 'sonner'
import { TriangleAlert } from 'lucide-react'
import { api } from '@/lib/api'
import { useStore } from '@/lib/store'
import { PillButton } from '@/components/Bezel'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'

/** Shown instead of starting a game that the mod files alone already say will not start. */
export function PreflightDialog() {
  const pre = useStore((s) => s.preflight)
  const dismiss = useStore((s) => s.dismissPreflight)
  const launch = useStore((s) => s.launch)
  const setPage = useStore((s) => s.setPage)
  const instance = useStore((s) => s.snap?.instances.find((i) => i.id === pre?.instanceId))
  const [busy, setBusy] = useState(false)

  const hasDup = pre?.problems.some((p) => p.fix === 'disable-duplicates')
  const hasMissing = pre?.problems.some((p) => p.fix === 'mods')

  const fixAndLaunch = async () => {
    if (!pre) return
    setBusy(true)
    try {
      const off = await api.modsDisableDuplicates(pre.instanceId)
      toast.success(`已禁用 ${off.length} 个旧的重复文件（可随时重新启用）`)
      const id = pre.instanceId
      dismiss()
      // Re-check: handling duplicates can leave a missing prerequisite to report.
      await launch(id, false, pre.quick)
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog open={pre !== null} onOpenChange={(o) => !o && !busy && dismiss()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <TriangleAlert className="size-5 text-amber-300" />
            「{instance?.name}」可能无法启动
          </DialogTitle>
          <DialogDescription>启动前检查了模组文件，发现下面的问题：</DialogDescription>
        </DialogHeader>
        <ul className="max-h-72 space-y-2 overflow-auto">
          {pre?.problems.map((p) => (
            <li key={p.title} className="rounded-2xl bg-black/20 px-3 py-2.5 text-sm">
              <div className="font-medium">{p.title}</div>
              <div className="mt-0.5 text-xs text-muted-foreground">{p.detail}</div>
            </li>
          ))}
        </ul>
        <DialogFooter>
          <PillButton
            tone="ghost"
            disabled={busy}
            onClick={() => {
              if (!pre) return
              const id = pre.instanceId
              dismiss()
              launch(id, true, pre.quick)
            }}
          >
            仍然启动
          </PillButton>
          {hasMissing && (
            <PillButton
              tone="soft"
              disabled={busy}
              onClick={() => {
                dismiss()
                setPage('mods')
              }}
            >
              去内容页处理
            </PillButton>
          )}
          {hasDup && (
            <PillButton disabled={busy} onClick={fixAndLaunch}>
              {busy ? '处理中…' : '一键处理重复并启动'}
            </PillButton>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
