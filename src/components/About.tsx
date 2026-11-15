import { useState } from 'react'
import { toast } from 'sonner'
import { Compass, ExternalLink, FileDown, RefreshCw } from 'lucide-react'
import { api } from '@/lib/api'
import { selectedInstance, useStore } from '@/lib/store'
import { PixelSprite } from '@/components/art'
import { PillButton } from '@/components/Bezel'
import { PixelSwitch } from '@/components/ui/pixel'

const THANKS: [string, string][] = [
  ['Mojang Studios', 'Minecraft 本体'],
  ['BMCLAPI（bangbang93）', '国内下载加速镜像'],
  ['Modrinth', '模组、光影、资源包、整合包的来源与接口'],
  ['Fabric · Quilt · Forge · NeoForge', '模组加载器'],
  ['Tauri · Rust · React', '做出这个启动器的开源项目'],
]

/** Version, update switch, diagnostics, the plain-language disclaimer and thanks. */
export function AboutBody() {
  const snap = useStore((s) => s.snap)
  const openDiag = useStore((s) => s.openDiag)
  const openOnboarding = useStore((s) => s.openOnboarding)
  const [checking, setChecking] = useState(false)
  if (!snap) return null
  const repo = snap.system.repo_url
  const inst = selectedInstance(snap)

  const check = async () => {
    setChecking(true)
    try {
      const r = await api.checkUpdate(true)
      if (!r.configured) toast.info('还没有设置开源地址，所以无法检查更新')
      else if (r.update) useStore.setState({ update: r.update })
      else toast.success(`已经是最新版本（v${r.current}）`)
    } catch (e) {
      toast.error(`检查更新失败：${e instanceof Error ? e.message : String(e)}`)
    } finally {
      setChecking(false)
    }
  }

  return (
    <div className="space-y-5">
      <div className="flex items-start gap-4">
        <PixelSprite name="grass" size={44} bob />
        <div className="min-w-0 flex-1">
          <div className="text-base font-semibold">Voxel 启动器</div>
          <div className="mt-0.5 text-sm text-muted-foreground">v{snap.system.version} · 一个好看、稳、容易上手的 Minecraft 启动器</div>
          <div className="mt-3 flex flex-wrap gap-2">
            <PillButton size="sm" tone="soft" disabled={checking} icon={<RefreshCw className={`size-4 ${checking ? 'animate-spin' : ''}`} />} onClick={check}>
              {checking ? '检查中…' : '检查更新'}
            </PillButton>
            <PillButton size="sm" tone="soft" icon={<FileDown className="size-4" />} onClick={() => openDiag(inst?.id ?? null)}>
              导出诊断包
            </PillButton>
            <PillButton size="sm" tone="soft" icon={<Compass className="size-4" />} onClick={openOnboarding}>
              重新打开新手引导
            </PillButton>
            {repo && (
              <PillButton size="sm" tone="ghost" icon={<ExternalLink className="size-4" />} onClick={() => api.openExternal(repo).catch((e) => toast.error(String(e.message ?? e)))}>
                开源地址
              </PillButton>
            )}
          </div>
        </div>
      </div>

      <div className="flex items-center gap-4">
        <div className="min-w-0 flex-1">
          <div className="text-sm font-medium">启动时检查更新</div>
          <div className="mt-0.5 text-xs text-muted-foreground">
            {repo ? '每次打开启动器，会问一下 GitHub 有没有新版本；只发一次匿名请求，不上传任何个人信息。' : '还没有设置开源地址，设置后这里才会生效。'}
          </div>
        </div>
        <PixelSwitch checked={snap.settings.check_updates} label="启动时检查更新" onCheckedChange={(v) => useStore.getState().saveSettings({ ...snap.settings, check_updates: v })} />
      </div>

      <div className="space-y-2 rounded-2xl bg-black/20 p-4 text-xs leading-relaxed text-muted-foreground">
        <div className="text-sm font-medium text-foreground">免责声明</div>
        <p>Voxel 是爱好者独立开发的第三方启动器，与 Mojang Studios、Microsoft 没有任何关联，也未获得它们的授权或认可。Minecraft 是 Mojang Studios / Microsoft 的商标。</p>
        <p>本启动器不分发游戏本体：游戏文件、资源和 Java 运行时，都是你下载或启动时直接从 Mojang 官方服务器（或镜像）获取的。</p>
        <p>离线账号只是方便没有正版账号时体验、或进入离线服务器。如果你喜欢 Minecraft，请购买正版支持开发者。</p>
        <p>模组、光影、资源包的版权属于各自作者，来自 Modrinth，请遵守它们各自的许可证。</p>
        <p>本软件按“现状”提供，不作任何担保。重要存档请自行备份；启动器默认会在每次启动前自动备份。</p>
      </div>

      <div>
        <div className="mb-2 text-sm font-medium">致谢</div>
        <ul className="grid gap-x-6 gap-y-1.5 text-xs sm:grid-cols-2">
          {THANKS.map(([name, what]) => (
            <li key={name}>
              <span className="font-medium text-foreground">{name}</span>
              <span className="text-muted-foreground"> · {what}</span>
            </li>
          ))}
        </ul>
      </div>
    </div>
  )
}
