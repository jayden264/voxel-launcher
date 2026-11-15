import { useState } from 'react'
import { toast } from 'sonner'
import { Check, ShieldCheck, X } from 'lucide-react'
import { api } from '@/lib/api'
import { useStore } from '@/lib/store'
import { PillButton } from '@/components/Bezel'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'

const INCLUDED = ['系统和启动器版本', '设置（账号只留数量）', '所选实例的最新日志', '最近 3 份崩溃报告', '模组列表']
const LEFT_OUT = ['账号名称', '保存的服务器', '存档和截图', '你的 Windows 用户名（已替换为 <user>）']

/** One click: the bundle is saved to Downloads and shown in Explorer, ready to send to whoever is helping. */
export function DiagnosticsDialog() {
  const diag = useStore((s) => s.diag)
  const close = useStore((s) => s.closeDiag)
  const name = useStore((s) => s.snap?.instances.find((i) => i.id === s.diag?.instanceId)?.name)
  const [busy, setBusy] = useState(false)

  const run = async () => {
    if (!diag) return
    setBusy(true)
    try {
      const r = await api.exportDiagnostics(diag.instanceId)
      toast.success(`诊断包已保存到「${r.folder}」文件夹，并已为你打开`, { duration: 8000 })
      close()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const list = (items: string[], icon: React.ReactNode) => (
    <ul className="space-y-1.5 text-sm">
      {items.map((t) => (
        <li key={t} className="flex items-start gap-2">
          <span className="mt-0.5 shrink-0">{icon}</span>
          <span>{t}</span>
        </li>
      ))}
    </ul>
  )

  return (
    <Dialog open={diag !== null} onOpenChange={(o) => !o && !busy && close()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <ShieldCheck className="size-5 text-brand" />
            导出诊断包
          </DialogTitle>
          <DialogDescription>
            把出问题时的现场打成一个 zip{name ? `（包含「${name}」的日志）` : ''}，发给能帮你的人就行。点下面的按钮会直接保存到「下载」文件夹，不用选路径。
          </DialogDescription>
        </DialogHeader>
        <div className="grid grid-cols-2 gap-3">
          <div className="rounded-2xl bg-black/20 p-3">
            <div className="mb-2 text-xs font-medium text-brand">会包含</div>
            {list(INCLUDED, <Check className="size-3.5 text-brand" />)}
          </div>
          <div className="rounded-2xl bg-black/20 p-3">
            <div className="mb-2 text-xs font-medium text-muted-foreground">不会包含</div>
            {list(LEFT_OUT, <X className="size-3.5 text-muted-foreground" />)}
          </div>
        </div>
        <DialogFooter>
          <PillButton tone="ghost" disabled={busy} onClick={close}>
            取消
          </PillButton>
          <PillButton disabled={busy} onClick={run}>
            {busy ? '正在打包…' : '导出并打开文件夹'}
          </PillButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
