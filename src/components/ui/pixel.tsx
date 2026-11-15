"use client"

import type { CSSProperties, ReactNode } from "react"
import { Checkbox } from "@base-ui/react/checkbox"
import { Select } from "@base-ui/react/select"
import { Switch } from "@base-ui/react/switch"
import { cn } from "@/lib/utils"

/** A 5-step staircase chevron. `crispEdges` keeps every block sharp at any zoom. */
function PixelChevron({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 10 6" width="12" height="7" shapeRendering="crispEdges" className={className} aria-hidden>
      <path fill="currentColor" d="M0 0h2v2H0zM2 2h2v2H2zM4 4h2v2H4zM6 2h2v2H6zM8 0h2v2H8z" />
    </svg>
  )
}

/** A staircase tick built from 2x2 blocks. */
function PixelTick({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 10 8" width="11" height="9" shapeRendering="crispEdges" className={className} aria-hidden>
      <path fill="currentColor" d="M0 4h2v2H0zM2 6h2v2H2zM4 4h2v2H4zM6 2h2v2H6zM8 0h2v2H8z" />
    </svg>
  )
}

export interface PixelOption {
  value: string
  label: string
  /** Small badge after the label, e.g. "测试版". */
  tag?: string
}

/**
 * A drop-down made for this launcher: a bevelled "pixel" trigger and a frosted
 * popup (the one place where glass earns its blur: it floats above content).
 * Rows light up with a stepped highlight and a pixel cursor; the popup grows
 * from the trigger and rows settle in one after another.
 */
export function PixelSelect({
  value,
  onValueChange,
  options,
  placeholder = "请选择",
  className,
  "aria-label": ariaLabel,
}: {
  value: string | null
  onValueChange: (v: string) => void
  options: PixelOption[]
  placeholder?: string
  className?: string
  "aria-label"?: string
}) {
  const items = options.map((o) => ({ value: o.value, label: o.label }))
  return (
    <Select.Root value={value} onValueChange={(v) => v !== null && onValueChange(v)} items={items}>
      <Select.Trigger aria-label={ariaLabel} className={cn("px-select-trigger", className)}>
        <Select.Value placeholder={placeholder} className="min-w-0 flex-1 truncate" />
        <Select.Icon className="px-select-chevron">
          <PixelChevron />
        </Select.Icon>
      </Select.Trigger>
      <Select.Portal>
        <Select.Positioner sideOffset={8} alignItemWithTrigger={false} style={{ zIndex: 60 }}>
          <Select.Popup className="px-select-popup">
            <Select.List className="px-select-list">
              {options.map((o, i) => (
                <Select.Item key={o.value} value={o.value} className="px-select-item" style={{ "--i": i } as CSSProperties}>
                  <span className="px-cursor" aria-hidden />
                  <Select.ItemText className="min-w-0 flex-1 truncate">{o.label}</Select.ItemText>
                  {o.tag && <span className="px-tag">{o.tag}</span>}
                  <Select.ItemIndicator className="px-select-tick">
                    <PixelTick />
                  </Select.ItemIndicator>
                </Select.Item>
              ))}
            </Select.List>
          </Select.Popup>
        </Select.Positioner>
      </Select.Portal>
    </Select.Root>
  )
}

/**
 * Chunky on/off switch: stepped corners, a recessed track that lights up and a
 * square thumb that slides with a little overshoot and squashes when pressed.
 * Solid on purpose; glass would make a control this small hard to read.
 */
export function PixelSwitch({
  checked,
  onCheckedChange,
  disabled,
  label,
}: {
  checked: boolean
  onCheckedChange: (v: boolean) => void
  disabled?: boolean
  label: string
}) {
  return (
    <Switch.Root checked={checked} onCheckedChange={onCheckedChange} disabled={disabled} aria-label={label} className="px-switch">
      <Switch.Thumb className="px-switch-thumb" />
    </Switch.Root>
  )
}

/** Square checkbox with stepped corners; the tick pops in when checked. */
export function PixelCheckbox({
  checked,
  onCheckedChange,
  tone = "brand",
  children,
  className,
}: {
  checked: boolean
  onCheckedChange: (v: boolean) => void
  tone?: "brand" | "danger"
  children?: ReactNode
  className?: string
}) {
  const box = (
    <Checkbox.Root checked={checked} onCheckedChange={onCheckedChange} className="px-checkbox" data-tone={tone}>
      <Checkbox.Indicator className="px-checkbox-mark" keepMounted>
        <PixelTick />
      </Checkbox.Indicator>
    </Checkbox.Root>
  )
  if (!children) return box
  return (
    <label className={cn("flex cursor-pointer items-start gap-3", className)}>
      {box}
      {children}
    </label>
  )
}
