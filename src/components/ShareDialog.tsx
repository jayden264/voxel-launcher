import { useState } from 'react'
import { toast } from 'sonner'
import { Copy, Package } from 'lucide-react'
import { api, type Instance } from '@/lib/api'
import { formatBytes, useStore } from '@/lib/store'
import { PillButton } from '@/components/Bezel'
import { Segmented } from '@/components/kit'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { PixelCheckbox } from '@/components/ui/pixel'

type Mode = 'copy' | 'export'

/** Two plain choices for an instance: make a copy to experiment on, or export a pack to send to a friend. */
export function ShareDialog({ inst, onClose }: { inst: Instance; onClose: () => void }) {
  const [mode, setMode] = useState<Mode>('copy')
  const [copyName, setCopyName] = useState(`${inst.name} 副本`.slice(0, 40))
  const [copySaves, setCopySaves] = useState(false)
  const [packName, setPackName] = useState(inst.name)
  const [version, setVersion] = useState('1.0.0')
  const [saves, setSaves] = useState(false)
  const [options, setOptions] = useState(false)
  const [busy, setBusy] = useState(false)
  const locked = !!inst.running

  const run = async () => {
    setBusy(true)
    try {
      if (mode === 'copy') {
        useStore.setState({ snap: await api.cloneInstance(inst.id, copyName, copySaves) })
        toast.success(`已复制为「${copyName.trim()}」`)
      } else {
        const r = await api.exportInstancePack(inst.id, packName, version, saves, options)
        toast.success(`整合包已保存到「${r.folder}」文件夹并为你打开（${formatBytes(r.size)}，${r.linked} 个模组直接链接${r.bundled > 0 ? `，${r.bundled} 个打包在里面` : ''}）`, { duration: 9000 })
      }
      onClose()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog open onOpenChange={(o) => !o && !busy && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>复制或分享「{inst.name}」</DialogTitle>
          <DialogDescription>{mode === 'copy' ? '复制一份，想怎么折腾都不会影响原来的。' : '导出成一个文件，发给朋友，他们在「下载」页就能安装。'}</DialogDescription>
        </DialogHeader>
        <Segmented<Mode>
          value={mode}
          onChange={setMode}
          options={[
            { value: 'copy', label: '复制一份' },
            { value: 'export', label: '导出整合包' },
          ]}
        />
        {mode === 'copy' ? (
          <div className="space-y-3">
            <Input value={copyName} maxLength={40} onChange={(e) => setCopyName(e.target.value)} placeholder="新实例的名字" />
            <PixelCheckbox checked={copySaves} onCheckedChange={setCopySaves} className="rounded-2xl bg-black/20 p-3 text-sm">
              <span>
                同时复制存档
                <span className="block text-xs text-muted-foreground">不勾选就只复制模组和设置，更快、更省空间。</span>
              </span>
            </PixelCheckbox>
            <p className="px-1 text-xs text-muted-foreground">模组、配置、设置都会复制；日志和崩溃报告不会。</p>
          </div>
        ) : (
          <div className="space-y-3">
            <div className="grid grid-cols-[1fr_110px] gap-2">
              <Input value={packName} maxLength={60} onChange={(e) => setPackName(e.target.value)} placeholder="整合包名称" />
              <Input value={version} maxLength={30} onChange={(e) => setVersion(e.target.value)} placeholder="版本号" />
            </div>
            <PixelCheckbox checked={saves} onCheckedChange={setSaves} className="rounded-2xl bg-black/20 p-3 text-sm">
              <span>
                包含存档
                <span className="block text-xs text-muted-foreground">默认不包含：存档很大，也属于你自己的东西。</span>
              </span>
            </PixelCheckbox>
            <PixelCheckbox checked={options} onCheckedChange={setOptions} className="rounded-2xl bg-black/20 p-3 text-sm">
              <span>
                包含游戏设置（按键、画面等）
                <span className="block text-xs text-muted-foreground">默认不包含，免得把你的个人设置带给别人。</span>
              </span>
            </PixelCheckbox>
            <p className="px-1 text-xs text-muted-foreground">Modrinth 上有的模组只记下载地址，包很小；找不到的会直接打包进去，保证朋友装得上。会保存到「下载」文件夹。</p>
          </div>
        )}
        {locked && <p className="text-xs text-amber-300">游戏正在运行，复制要先关闭游戏（导出不受影响）。</p>}
        <DialogFooter>
          <PillButton tone="ghost" disabled={busy} onClick={onClose}>
            取消
          </PillButton>
          <PillButton
            disabled={busy || (mode === 'copy' ? locked || !copyName.trim() : !packName.trim())}
            icon={mode === 'copy' ? <Copy className="size-4" /> : <Package className="size-4" />}
            onClick={run}
          >
            {busy ? (mode === 'copy' ? '复制中…' : '打包中…') : mode === 'copy' ? '复制' : '导出并打开文件夹'}
          </PillButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
