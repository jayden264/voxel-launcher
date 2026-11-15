import { useState } from 'react'
import { motion } from 'motion/react'
import { FolderOpen, Play, Plus, Settings2, Share2, Trash2 } from 'lucide-react'
import { toast } from 'sonner'
import { api, type Instance } from '@/lib/api'
import { rise } from '@/lib/motion'
import { useStore } from '@/lib/store'
import { Bezel, Eyebrow, PillButton } from '@/components/Bezel'
import { Scene } from '@/components/Scene'
import { ShareDialog } from '@/components/ShareDialog'
import { Empty, Field, LoaderBadge, PageBanner } from '@/components/kit'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { PixelCheckbox } from '@/components/ui/pixel'
import { Slider } from '@/components/ui/slider'

const num = (v: string) => {
  const n = parseInt(v, 10)
  return Number.isFinite(n) && n > 0 ? n : null
}

function EditDialog({ inst, onClose }: { inst: Instance; onClose: () => void }) {
  const snap = useStore((s) => s.snap)!
  const update = useStore((s) => s.updateInstance)
  const [draft, setDraft] = useState<Instance>(inst)
  const max = Math.max(2048, snap.system.max_mem_mb)
  const memory = draft.memory_mb ?? snap.settings.default_memory_mb ?? snap.system.recommended_mem_mb

  const browse = async () => {
    const p = await api.pickJava()
    if (p) setDraft({ ...draft, java_path: p })
  }

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>实例设置</DialogTitle>
          <DialogDescription>Minecraft {inst.mc_version ?? inst.version_id}</DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <Field label="名称">
            <Input value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} maxLength={40} />
          </Field>
          <Field label="最大内存" hint={`${(memory / 1024).toFixed(1)} GB · 上限 ${(max / 1024).toFixed(0)} GB`}>
            <Slider min={1024} max={max} step={256} value={[Math.min(memory, max)]} onValueChange={(v) => setDraft({ ...draft, memory_mb: Array.isArray(v) ? v[0] : v })} />
          </Field>
          <Field label="额外 JVM 参数" hint="写在内存设置之后，同名参数以它为准">
            <Input value={draft.jvm_args} onChange={(e) => setDraft({ ...draft, jvm_args: e.target.value })} placeholder="-Dfoo=bar" className="font-mono" />
          </Field>
          <Field label="Java 路径" hint="留空 = 自动下载官方运行时">
            <div className="flex gap-2">
              <Input value={draft.java_path ?? ''} onChange={(e) => setDraft({ ...draft, java_path: e.target.value || null })} placeholder="自动" className="font-mono" />
              <PillButton size="sm" tone="soft" onClick={browse}>
                浏览
              </PillButton>
            </div>
          </Field>
          <div className="grid grid-cols-2 gap-3">
            <Field label="窗口宽度">
              <Input inputMode="numeric" value={draft.width ?? ''} onChange={(e) => setDraft({ ...draft, width: num(e.target.value) })} placeholder="默认" />
            </Field>
            <Field label="窗口高度">
              <Input inputMode="numeric" value={draft.height ?? ''} onChange={(e) => setDraft({ ...draft, height: num(e.target.value) })} placeholder="默认" />
            </Field>
          </div>
        </div>
        <DialogFooter>
          <PillButton size="md" tone="ghost" onClick={onClose}>
            取消
          </PillButton>
          <PillButton
            size="md"
            onClick={async () => {
              if (await update(draft)) onClose()
            }}
          >
            保存
          </PillButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

export function InstancesPage() {
  const snap = useStore((s) => s.snap)
  const run = useStore((s) => s.run)
  const mutate = useStore((s) => s.mutate)
  const launch = useStore((s) => s.launch)
  const setPage = useStore((s) => s.setPage)
  const [editing, setEditing] = useState<Instance | null>(null)
  const [deleting, setDeleting] = useState<Instance | null>(null)
  const [sharing, setSharing] = useState<Instance | null>(null)
  const [deleteFiles, setDeleteFiles] = useState(false)
  if (!snap) return null

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageBanner
        biome="meadow"
        seed="instances"
        eyebrow="Instances"
        title="实例"
        subtitle="每个实例有独立的存档、模组和设置，共用同一份游戏文件。"
        actions={
          <PillButton icon={<Plus className="size-4" />} onClick={() => setPage('download')}>
            新建实例
          </PillButton>
        }
      />
      <div className="min-h-0 flex-1 overflow-auto pr-1">
        {snap.instances.length === 0 ? (
          <Empty sprite="chest" title="还没有实例" hint="去「下载」页选择版本来创建。" action={<PillButton onClick={() => setPage('download')}>选择版本</PillButton>} />
        ) : (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(280px,1fr))] gap-5 pb-4">
            {snap.instances.map((i, idx) => {
              const selected = snap.settings.selected_instance === i.id
              const memory = i.memory_mb ?? snap.settings.default_memory_mb ?? snap.system.recommended_mem_mb
              return (
                <motion.div key={i.id} variants={rise} initial="hidden" animate="show" custom={idx}>
                  <Bezel radius={32} interactive className="group" coreClassName="flex flex-col">
                    <div className="relative h-36">
                      <Scene seed={i.id} />
                      <div className="absolute inset-0 bg-gradient-to-t from-[oklch(0.2_0.022_250)] via-transparent to-transparent" />
                      {selected && <Eyebrow className="absolute top-3.5 left-3.5 bg-[oklch(0.16_0.03_160)]/70">当前</Eyebrow>}
                      {i.running && <span className="absolute top-3.5 right-3.5 rounded-full bg-black/55 px-2.5 py-0.5 text-[11px] text-brand">运行中</span>}
                    </div>
                    <div className="p-4 pt-3">
                      <div className="truncate text-lg font-semibold">{i.name}</div>
                      <div className="mt-0.5 flex flex-wrap items-center gap-x-2 gap-y-1 text-sm text-muted-foreground">
                        <span>
                          {i.mc_version ?? i.version_id} · {(memory / 1024).toFixed(memory % 1024 ? 1 : 0)} GB
                        </span>
                        {i.loader && <LoaderBadge loader={i.loader} />}
                      </div>
                      <div className="mt-4 flex items-center gap-1.5">
                        <PillButton
                          size="sm"
                          icon={<Play className="size-3.5 fill-current" />}
                          disabled={run.phase !== 'idle'}
                          onClick={async () => {
                            await mutate(() => api.selectInstance(i.id))
                            setPage('home')
                            launch(i.id)
                          }}
                        >
                          启动
                        </PillButton>
                        {!selected && (
                          <PillButton size="sm" tone="ghost" onClick={() => mutate(() => api.selectInstance(i.id))}>
                            设为当前
                          </PillButton>
                        )}
                        <div className="ml-auto flex gap-0.5">
                          <IconBtn label="设置" onClick={() => setEditing(i)}>
                            <Settings2 />
                          </IconBtn>
                          <IconBtn label="复制或分享" onClick={() => setSharing(i)}>
                            <Share2 />
                          </IconBtn>
                          <IconBtn label="打开目录" onClick={() => api.openGameDir(i.id).catch((e) => toast.error(String(e.message ?? e)))}>
                            <FolderOpen />
                          </IconBtn>
                          <IconBtn
                            label="删除"
                            danger
                            onClick={() => {
                              setDeleteFiles(false)
                              setDeleting(i)
                            }}
                          >
                            <Trash2 />
                          </IconBtn>
                        </div>
                      </div>
                    </div>
                  </Bezel>
                </motion.div>
              )
            })}
          </div>
        )}
      </div>

      {sharing && <ShareDialog key={sharing.id} inst={sharing} onClose={() => setSharing(null)} />}
      {editing && <EditDialog key={editing.id} inst={editing} onClose={() => setEditing(null)} />}

      <Dialog open={deleting !== null} onOpenChange={(o) => !o && setDeleting(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>删除「{deleting?.name}」？</DialogTitle>
            <DialogDescription>默认只移除这个实例，存档和模组文件会保留在磁盘上。</DialogDescription>
          </DialogHeader>
          <PixelCheckbox checked={deleteFiles} onCheckedChange={setDeleteFiles} tone="danger" className="rounded-2xl border border-red-400/20 bg-red-500/8 p-3 text-sm">
            <span>
              同时永久删除该实例的所有文件（存档、模组、截图）。<b className="text-red-300">此操作无法撤销。</b>
            </span>
          </PixelCheckbox>
          <DialogFooter>
            <PillButton tone="ghost" onClick={() => setDeleting(null)}>
              取消
            </PillButton>
            <PillButton
              tone="danger"
              onClick={async () => {
                if (deleting && (await mutate(() => api.deleteInstance(deleting.id, deleteFiles), '已删除'))) setDeleting(null)
              }}
            >
              {deleteFiles ? '永久删除' : '移除实例'}
            </PillButton>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}

function IconBtn({ label, onClick, danger, children }: { label: string; onClick: () => void; danger?: boolean; children: React.ReactNode }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className={`grid size-8 place-items-center rounded-full text-muted-foreground transition-[background,color,transform] duration-500 [transition-timing-function:var(--ease)] active:scale-90 [&_svg]:size-4 ${danger ? 'hover:bg-red-500/15 hover:text-red-300' : 'hover:bg-white/10 hover:text-foreground'}`}
    >
      {children}
    </button>
  )
}
