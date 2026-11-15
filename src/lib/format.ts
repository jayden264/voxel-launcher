/** "45 秒" / "12 分钟" / "3 小时 20 分". */
export function fmtDuration(secs: number): string {
  if (secs < 60) return `${Math.max(0, Math.round(secs))} 秒`
  const mins = Math.round(secs / 60)
  if (mins < 60) return `${mins} 分钟`
  const h = Math.floor(secs / 3600)
  const m = Math.round((secs % 3600) / 60)
  return m === 0 || m === 60 ? `${m === 60 ? h + 1 : h} 小时` : `${h} 小时 ${m} 分`
}

/** Shorter, for chart axes: "2.5 小时" / "40 分钟". */
export function fmtHours(secs: number): string {
  if (secs < 3600) return `${Math.max(1, Math.round(secs / 60))} 分钟`
  const h = secs / 3600
  return `${h >= 10 ? Math.round(h) : h.toFixed(1)} 小时`
}

/** "刚刚" / "5 分钟前" / "3 天前"; a date once it is more than a month old. */
export function fmtAgo(ms: number, now = Date.now()): string {
  const s = Math.max(0, (now - ms) / 1000)
  if (s < 60) return '刚刚'
  if (s < 3600) return `${Math.floor(s / 60)} 分钟前`
  if (s < 86400) return `${Math.floor(s / 3600)} 小时前`
  if (s < 30 * 86400) return `${Math.floor(s / 86400)} 天前`
  return new Date(ms).toLocaleDateString('zh-CN')
}

/** "11月15日 14:30". */
export function fmtDateTime(ms: number): string {
  return new Date(ms).toLocaleString('zh-CN', { month: 'long', day: 'numeric', hour: '2-digit', minute: '2-digit', hour12: false })
}
