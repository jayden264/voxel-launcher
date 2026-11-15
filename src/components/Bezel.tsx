import type { ButtonHTMLAttributes, CSSProperties, ReactNode } from 'react'
import { cn } from '@/lib/utils'

/**
 * "Double bezel": an outer tray holding an inner core with its own highlight
 * and a concentric (smaller) radius, like glass seated in machined metal.
 */
export function Bezel({
  children,
  className,
  coreClassName,
  radius = 28,
  interactive = false,
  ...rest
}: {
  children: ReactNode
  className?: string
  coreClassName?: string
  radius?: number
  interactive?: boolean
} & Omit<React.HTMLAttributes<HTMLDivElement>, 'children'>) {
  return (
    <div {...rest} className={cn('bezel', interactive && 'bezel-interactive', className)} style={{ '--r': `${radius}px`, ...rest.style } as CSSProperties}>
      <div className={cn('bezel-core', coreClassName)}>{children}</div>
    </div>
  )
}

type PillProps = {
  icon?: ReactNode
  tone?: 'brand' | 'soft' | 'danger' | 'ghost'
  size?: 'sm' | 'md' | 'xl'
} & ButtonHTMLAttributes<HTMLButtonElement>

/**
 * Pill button with the trailing icon nested in its own circle (button-in-button).
 * On hover the inner circle drifts diagonally; pressing scales the whole pill.
 */
export function PillButton({ icon, tone = 'brand', size = 'md', className, children, ...props }: PillProps) {
  return (
    <button {...props} className={cn('pill group', `pill--${tone}`, `pill--${size}`, className)}>
      <span className="pill-label">{children}</span>
      {icon && <span className="pill-icon">{icon}</span>}
    </button>
  )
}

export function Eyebrow({ children, className }: { children: ReactNode; className?: string }) {
  return <span className={cn('eyebrow-pill', className)}>{children}</span>
}
