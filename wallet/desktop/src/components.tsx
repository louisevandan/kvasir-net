import { ReactNode, useState } from 'react'
import { useI18n } from './i18n'

export function Card({ children, className = '', style }: { children: ReactNode; className?: string; style?: React.CSSProperties }) {
  return <div className={`card ${className}`} style={style}>{children}</div>
}

export function Stat({ v, k, grad }: { v: ReactNode; k: string; grad?: boolean }) {
  return <div className="stat"><div className={`v ${grad ? 'grad-text' : ''}`}>{v}</div><div className="k">{k}</div></div>
}

export function Meter({ value, color }: { value: number; color: string }) {
  return <div className="meter"><span style={{ width: `${Math.min(100, Math.max(0, value * 100))}%`, background: color }} /></div>
}

export function MetricRow({ label, valueText, value, color }: { label: string; valueText: string; value: number; color: string }) {
  return (
    <div style={{ padding: '6px 0' }}>
      <div className="spread"><span className="small muted">{label}</span><span className="small" style={{ fontWeight: 600 }}>{valueText}</span></div>
      <div style={{ height: 6 }} /><Meter value={value} color={color} />
    </div>
  )
}

export const tierColor = (t?: string) => t === 'S' ? 'var(--warn)' : t === 'A' ? 'var(--good)' : t === 'B' ? 'var(--info)' : 'var(--text-3)'

export function TierBadge({ tier, size = 34 }: { tier: string; size?: number }) {
  return <div className="tier" style={{ width: size, height: size, background: tierColor(tier), fontSize: size * 0.42 }}>{tier}</div>
}

export function CopyButton({ text, label }: { text: string; label?: string }) {
  const { t } = useI18n()
  const [done, setDone] = useState(false)
  return (
    <button className="btn secondary" onClick={() => { navigator.clipboard?.writeText(text); setDone(true); setTimeout(() => setDone(false), 1500) }}>
      {done ? t('common.copied') : (label ?? t('common.copy'))}
    </button>
  )
}

export function Field({ label, children }: { label: string; children: ReactNode }) {
  return <label style={{ display: 'block' }}><div className="label" style={{ marginBottom: 8 }}>{label}</div>{children}</label>
}

export function StatusPill({ status }: { status: string }) {
  const map: Record<string, [string, string]> = {
    online: ['var(--good)', 'online'], idle: ['var(--warn)', 'idle'], registered: ['var(--info)', 'registered'], offline: ['var(--text-3)', 'offline'],
  }
  const [c, l] = map[status] ?? map.offline
  return <span className="pill" style={{ color: c, background: `color-mix(in srgb, ${c} 16%, transparent)` }}>● {l}</span>
}
