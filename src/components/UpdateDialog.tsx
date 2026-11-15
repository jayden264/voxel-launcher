import { useState } from 'react'
import { toast } from 'sonner'
import { Sparkles } from 'lucide-react'
import { api } from '@/lib/api'
import { formatBytes, useStore } from '@/lib/store'
import { PillButton } from '@/components/Bezel'
import { PixelBar } from '@/components/RunProgress'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'

type Phase = { kind: 'idle' } | { kind: 'updating'; stage: string; done: number; total: number } | { kind: 'failed'; message: string }

/**
 * A newer version is out. One button does everything: download it from GitHub, check it, install it
 * and reopen the launcher. If that cannot be done safely (no checksum, no network), the user is sent
 * to the download page instead.
 */
export function UpdateDialog() {
  const update = useStore((s) => s.update)
  const close = useStore((s) => s.closeUpdate)
  const current = useStore((s) => s.snap?.system.version)
  const [phase, setPhase] = useState<Phase>({ kind: 'idle' })
  const updating = phase.kind === 'updating'
  // Without GitHub's checksum the installer cannot be verified, so it is never run automatically.
  const automatic = !!update?.installer_url && !!update.installer_sha256

  const manual = () => {
    if (!update) return
    api.openExternal(update.installer_url ?? update.url).catch((e) => toast.error(String(e.message ?? e)))
    close()
  }

  const start = async () => {
    setPhase({ kind: 'updating', stage: '正在准备', done: 0, total: 0 })
    try {
      await api.installUpdate((e) => {
        if (e.type === 'stage') setPhase((p) => (p.kind === 'updating' ? { ...p, stage: e.label } : p))
        else if (e.type === 'progress') setPhase((p) => (p.kind === 'updating' ? { ...p, done: e.done, total: e.total } : p))
        else setPhase((p) => (p.kind === 'updating' ? { ...p, stage: '安装中，启动器马上会自动重新打开…' } : p))
      })
    } catch (e) {
      setPhase({ kind: 'failed', message: e instanceof Error ? e.message : String(e) })
    }
  }

  const pct = phase.kind === 'updating' && phase.total > 0 ? phase.done / phase.total : null
  return (
    <Dialog
      open={update !== null}
      onOpenChange={(o) => {
        if (!o && !updating) {
          setPhase({ kind: 'idle' })
          close()
        }
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Sparkles className="size-5 text-brand" />
            发现新版本 v{update?.version}
          </DialogTitle>
          <DialogDescription>你现在用的是 v{current}。{automatic ? '点「立即更新」会自动下载、安装并重新打开，不用你再做别的。' : '这个版本需要手动下载安装。'}更新后，你的实例、存档和设置都会保留。</DialogDescription>
        </DialogHeader>

        {phase.kind === 'updating' ? (
          <div className="space-y-3 rounded-2xl bg-black/20 p-4">
            <div className="flex items-baseline justify-between gap-3 text-sm">
              <span className="font-medium">{phase.stage}</span>
              {pct !== null && <span className="font-mono text-brand tabular-nums">{Math.round(pct * 100)}%</span>}
            </div>
            <PixelBar value={pct} />
            <div className="text-xs text-muted-foreground">
              {phase.total > 0 && `${formatBytes(phase.done)} / ${formatBytes(phase.total)} · `}更新时启动器会短暂关闭，装好后自动重新打开；正在运行的游戏不受影响。
            </div>
          </div>
        ) : (
          <>
            {update?.notes.trim() && (
              <div className="max-h-56 overflow-auto rounded-2xl bg-black/20 p-3">
                <div className="mb-1.5 text-xs font-medium text-muted-foreground">更新内容</div>
                <pre className="selectable font-sans text-sm leading-relaxed whitespace-pre-wrap">{update.notes.replace(/^#+\s*/gm, '')}</pre>
              </div>
            )}
            {phase.kind === 'failed' && (
              <p className="rounded-2xl border border-red-400/25 bg-red-500/10 px-3 py-2 text-sm text-red-200">
                自动更新没成功：{phase.message}
                <span className="mt-1 block text-xs text-red-200/70">没有任何文件被改动。可以重试，或者点「手动下载」自己安装。</span>
              </p>
            )}
          </>
        )}

        {!updating && (
          <DialogFooter>
            {phase.kind === 'failed' ? (
              <>
                <PillButton tone="ghost" onClick={manual}>
                  手动下载
                </PillButton>
                <PillButton onClick={start}>重试</PillButton>
              </>
            ) : (
              <>
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
                <PillButton onClick={automatic ? start : manual}>{automatic ? '立即更新' : '前往下载'}</PillButton>
              </>
            )}
          </DialogFooter>
        )}
      </DialogContent>
    </Dialog>
  )
}
