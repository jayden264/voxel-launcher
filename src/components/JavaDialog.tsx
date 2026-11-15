import { Coffee } from 'lucide-react'
import { useStore } from '@/lib/store'
import { PillButton } from '@/components/Bezel'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'

/** Asked before the launcher downloads a Java runtime, so nothing big arrives unannounced. */
export function JavaDialog() {
  const prompt = useStore((s) => s.javaPrompt)
  const dismiss = useStore((s) => s.dismissJavaPrompt)
  const confirm = useStore((s) => s.confirmJava)
  const name = useStore((s) => s.snap?.instances.find((i) => i.id === s.javaPrompt?.instanceId)?.name)

  return (
    <Dialog open={prompt !== null} onOpenChange={(o) => !o && dismiss()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Coffee className="size-5 text-brand" />
            需要下载 Java {prompt?.major}
          </DialogTitle>
          <DialogDescription>
            {name ? `「${name}」` : '这个版本'}运行需要 Java {prompt?.major}，但这台电脑上还没有。点「确定」后会从 Mojang 官方下载（约 100–200 MB），在后台自动完成，下完接着继续，不用再点一次。
          </DialogDescription>
        </DialogHeader>
        <p className="rounded-2xl bg-black/20 px-3 py-2 text-xs text-muted-foreground">只会下载一次，之后同系列的版本都能共用。想用自己装好的 Java，可以在「设置」里指定路径。</p>
        <DialogFooter>
          <PillButton tone="ghost" onClick={dismiss}>
            取消
          </PillButton>
          <PillButton onClick={confirm}>确定</PillButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
