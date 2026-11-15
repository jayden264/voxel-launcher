import { toast } from 'sonner'
import { Sparkles } from 'lucide-react'
import { api } from '@/lib/api'
import { useStore } from '@/lib/store'
import { PillButton } from '@/components/Bezel'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'

/** A newer version is out. Download goes through the browser; nothing is installed behind the user's back. */
export function UpdateDialog() {
  const update = useStore((s) => s.update)
  const close = useStore((s) => s.closeUpdate)
  const current = useStore((s) => s.snap?.system.version)

  const download = () => {
    if (!update) return
    api.openExternal(update.installer_url ?? update.url).catch((e) => toast.error(String(e.message ?? e)))
    close()
  }

  return (
    <Dialog open={update !== null} onOpenChange={(o) => !o && close()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Sparkles className="size-5 text-brand" />
            发现新版本 v{update?.version}
          </DialogTitle>
          <DialogDescription>你现在用的是 v{current}。更新后，你的实例、存档和设置都会保留。</DialogDescription>
        </DialogHeader>
        {update?.notes.trim() && (
          <div className="max-h-56 overflow-auto rounded-2xl bg-black/20 p-3">
            <div className="mb-1.5 text-xs font-medium text-muted-foreground">更新内容</div>
            <pre className="selectable font-sans text-sm leading-relaxed whitespace-pre-wrap">{update.notes.replace(/^#+\s*/gm, '')}</pre>
          </div>
        )}
        <DialogFooter>
          <PillButton
            tone="ghost"
            onClick={() => {
              if (update) api.ignoreUpdate(update.version).catch(() => {})
              close()
            }}
          >
            忽略此版本
          </PillButton>
          <PillButton tone="soft" onClick={close}>
            稍后提醒
          </PillButton>
          <PillButton onClick={download}>{update?.installer_url ? '下载安装包' : '前往下载'}</PillButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
