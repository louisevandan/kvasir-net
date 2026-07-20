import { useEffect, useMemo, useState } from 'react'
import { useWallet } from '../state'
import { useI18n } from '../i18n'
import { fmt } from '../api'
import { Card, Stat, tierColor } from '../components'
import { Staking, GlobalNode, GlobalNodeStatus } from '../services'

const W = 1000, H = 500
const project = (lng: number, lat: number) => ({ x: (lng + 180) / 360 * W, y: (90 - lat) / 180 * H })

// Very rough continent outlines (lng,lat) — rendered as a dotted map, so exactness
// doesn't matter; the dots read as a stylised world map.
const CONTINENTS: [number, number][][] = [
  [[-165, 68], [-100, 72], [-60, 60], [-55, 48], [-70, 42], [-80, 26], [-98, 17], [-108, 23], [-124, 40], [-140, 60], [-165, 68]], // N. America
  [[-45, 60], [-20, 70], [-30, 83], [-55, 80], [-52, 64]], // Greenland
  [[-80, 8], [-60, 10], [-50, 0], [-35, -8], [-40, -23], [-58, -52], [-70, -52], [-72, -30], [-81, -6]], // S. America
  [[-10, 36], [0, 50], [-5, 58], [12, 60], [30, 58], [40, 47], [28, 40], [10, 38], [-10, 36]], // Europe
  [[-17, 14], [-16, 28], [10, 34], [32, 31], [43, 12], [51, 12], [40, -4], [40, -16], [22, -35], [15, -30], [8, 4], [-8, 4]], // Africa
  [[30, 45], [55, 52], [90, 72], [140, 71], [170, 67], [180, 62], [145, 45], [140, 34], [122, 30], [108, 10], [95, 8], [92, 20], [78, 8], [70, 22], [55, 26], [42, 40]], // Asia
  [[95, 5], [120, 6], [140, -6], [122, -9], [100, -2]], // SE Asia
  [[113, -20], [130, -12], [143, -12], [153, -28], [150, -38], [120, -34], [113, -20]], // Australia
]

function inPoly(x: number, y: number, poly: [number, number][]): boolean {
  let hit = false
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
    const [xi, yi] = poly[i], [xj, yj] = poly[j]
    if ((yi > y) !== (yj > y) && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) hit = !hit
  }
  return hit
}

// deterministic hash → node lng/lat, clustered on plausible hubs so markers land on soil.
const HUBS: [number, number][] = [
  [-122.4, 37.8], [-74, 40.7], [-79.4, 43.7], [-46.6, -23.5], [-0.1, 51.5], [4.9, 52.4],
  [8.7, 50.1], [127, 37.6], [139.7, 35.7], [103.8, 1.35], [77.6, 13], [151.2, -33.9], [18.4, -33.9], [55.3, 25.3],
]
function hash(s: string): number { let h = 2166136261; for (let i = 0; i < s.length; i++) { h ^= s.charCodeAt(i); h = Math.imul(h, 16777619) } return h >>> 0 }
function nodeLngLat(id: string): [number, number] {
  const h = hash(id)
  const [lng, lat] = HUBS[h % HUBS.length]
  const jx = ((h >>> 8) % 100) / 100 - 0.5, jy = ((h >>> 16) % 100) / 100 - 0.5
  return [lng + jx * 12, lat + jy * 10]
}

export function MapScreen() {
  const { t } = useI18n()
  const { stakingUrl } = useWallet()
  const [data, setData] = useState<GlobalNodeStatus | null>(null)
  const [hover, setHover] = useState<{ n: GlobalNode; x: number; y: number } | null>(null)

  useEffect(() => {
    const load = () => new Staking(stakingUrl).allNodes().then(setData).catch(() => setData(null))
    load(); const id = setInterval(load, 15000); return () => clearInterval(id)
  }, [stakingUrl])

  const landDots = useMemo(() => {
    const dots: { x: number; y: number }[] = []
    for (let lat = 78; lat >= -55; lat -= 3) {
      for (let lng = -178; lng <= 178; lng += 3) {
        if (CONTINENTS.some((c) => inPoly(lng, lat, c))) { const p = project(lng, lat); dots.push(p) }
      }
    }
    return dots
  }, [])

  const totals = data?.totals
  return (
    <div className="grid" style={{ gap: 18 }}>
      <div className="grid cols-4">
        <Card><Stat grad v={totals?.nodes ?? 0} k={t('map.total')} /></Card>
        <Card><Stat grad v={totals?.online ?? 0} k={t('map.online')} /></Card>
        <Card><Stat grad v={totals?.owners ?? 0} k={t('map.owners')} /></Card>
        <Card><Stat grad v={fmt(totals?.effectiveUnits ?? 0)} k={t('map.effective')} /></Card>
      </div>

      <Card>
        <div className="spread" style={{ marginBottom: 12 }}>
          <div><div className="label">{t('map.title')}</div><div className="small muted">{t('map.subtitle')}</div></div>
          <div className="row" style={{ gap: 12 }}>
            {['S', 'A', 'B', 'C'].map((tr) => (
              <span key={tr} className="row small" style={{ gap: 5 }}>
                <span className="badge-dot" style={{ background: tierColor(tr) }} />{tr}{totals?.byTier?.[tr] ? ` ${totals.byTier[tr]}` : ''}
              </span>
            ))}
          </div>
        </div>

        <div className="mapwrap" onMouseLeave={() => setHover(null)}>
          <svg viewBox={`0 0 ${W} ${H}`} width="100%" style={{ display: 'block' }}>
            {landDots.map((d, i) => <circle key={i} className="land-dot" cx={d.x} cy={d.y} r={1.4} />)}
            {data?.nodes.map((n) => {
              const [lng, lat] = nodeLngLat(n.nodeId)
              const { x, y } = project(lng, lat)
              const c = tierColor(n.tier)
              const r = 4 + Math.min(6, (n.effectiveUnits || 0) / 300)
              return (
                <g key={n.nodeId}
                  onMouseEnter={() => setHover({ n, x, y })}
                  onMouseMove={() => setHover({ n, x, y })}>
                  {n.status === 'online' && <circle className="node-ring" cx={x} cy={y} r={4} fill="none" stroke={c} strokeWidth={2} />}
                  <circle className="node-mark" cx={x} cy={y} r={r} fill={c} stroke="#fff" strokeWidth={0.8} fillOpacity={0.9} />
                </g>
              )
            })}
          </svg>
          {hover && (
            <div className="map-tip" style={{ left: `${(hover.x / W) * 100}%`, top: `${(hover.y / H) * 100}%`, transform: 'translate(12px, -50%)' }}>
              <div style={{ fontWeight: 700 }}>{hover.n.label}</div>
              <div className="muted">{hover.n.os} · <span style={{ color: tierColor(hover.n.tier) }}>{hover.n.tier} ×{fmt(hover.n.perfMultiplier)}</span> · {hover.n.status}</div>
              <div className="muted">{fmt(hover.n.effectiveUnits)} eff · {hover.n.ownerShort}</div>
            </div>
          )}
        </div>
        <div className="small muted" style={{ marginTop: 8 }}>※ {t('map.note')}</div>
      </Card>

      {(!data || data.nodes.length === 0) && <Card><div className="muted small">{t('map.empty')}</div></Card>}
    </div>
  )
}
