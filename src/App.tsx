import { useEffect } from 'react'
import { AnimatePresence, MotionConfig, motion } from 'motion/react'
import { pageVariants } from '@/lib/motion'
import { Toaster } from '@/components/ui/sonner'
import { DiagnosticsDialog } from '@/components/DiagnosticsDialog'
import { Onboarding } from '@/components/Onboarding'
import { UpdateDialog } from '@/components/UpdateDialog'
import { JavaDialog } from '@/components/JavaDialog'
import { PreflightDialog } from '@/components/PreflightDialog'
import { Sidebar } from '@/components/Sidebar'
import { TitleBar } from '@/components/TitleBar'
import { AccountsPage } from '@/pages/Accounts'
import { DownloadPage } from '@/pages/Download'
import { HomePage } from '@/pages/Home'
import { InstancesPage } from '@/pages/Instances'
import { ModsPage } from '@/pages/Mods'
import { SettingsPage } from '@/pages/Settings'
import { WorldsPage } from '@/pages/Worlds'
import { useStore, type Page } from '@/lib/store'
import { useTheme } from '@/lib/theme'

const PAGES: Record<Page, () => React.ReactElement | null> = {
  home: HomePage,
  instances: InstancesPage,
  download: DownloadPage,
  mods: ModsPage,
  worlds: WorldsPage,
  accounts: AccountsPage,
  settings: SettingsPage,
}

export default function App() {
  const page = useStore((s) => s.page)
  const ready = useStore((s) => s.snap !== null)
  const init = useStore((s) => s.init)
  const bg = useTheme((s) => s.bg)
  const bgDim = useTheme((s) => s.bgDim)

  useEffect(() => {
    init()
  }, [init])

  const Current = PAGES[page]
  return (
    <MotionConfig reducedMotion="user">
    <div className="app-backdrop relative isolate flex h-full flex-col overflow-hidden rounded-[10px]">
      {bg && (
        <>
          <div aria-hidden className="pointer-events-none absolute inset-0 -z-20 bg-cover bg-center" style={{ backgroundImage: `url(${bg})` }} />
          <div aria-hidden className="pointer-events-none absolute inset-0 -z-10" style={{ background: `oklch(0.14 0.02 255 / ${bgDim})` }} />
        </>
      )}
      <TitleBar />
      <Onboarding />
      <div className="flex min-h-0 flex-1">
        <Sidebar />
        <main className="min-w-0 flex-1 pr-5 pb-5">
          <AnimatePresence mode="wait">
            {ready && (
              <motion.div key={page} variants={pageVariants} initial="initial" animate="animate" exit="exit" className="h-full">
                <Current />
              </motion.div>
            )}
          </AnimatePresence>
        </main>
      </div>
      <PreflightDialog />
      <JavaDialog />
      <DiagnosticsDialog />
      <UpdateDialog />
      <Toaster position="bottom-right" theme="dark" richColors closeButton />
    </div>
    </MotionConfig>
  )
}
