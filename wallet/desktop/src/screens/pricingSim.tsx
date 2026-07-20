// Interactive KVR economy simulator, embedded in the pricing screen. Reacts
// live to the pricing values being edited, and grounds every flow in the
// gateway's ACTUAL reward rules:
//   revenue / request  = basePrice + (input+output tokens) × perToken   (billing)
//   rewards / request  = (output tokens / 1000) × rewardPerUnit × mult  (1 unit
//                        == 1k output tokens, split across nodes by layer share)
//   fixed emission/day = infra uptime (hub + gateway, KVR/h) + staking APR
// Net flow is the treasury's view: revenue returns KVR to the treasury,
// rewards emit KVR into circulation — a sustained negative net is inflation.
// Reward parameters come live from the local gateway /api/config when it is
// reachable; the constants below are only fallbacks.

import { useEffect, useMemo, useRef, useState } from 'react'
import { useI18n } from '../i18n'
import { fmt } from '../api'
import { Card, Stat } from '../components'
import { ModelPricing, Staking } from '../services'

// Reference display rate only — never billing.
const USD_PER_KVR = 0.10
const AVG_INPUT_TOKENS = 250
const PERF_MULT = 1.0

// Published list prices, $/1M tokens, checked 2026-07-16 (OpenAI, Anthropic,
// Google public pricing pages). Update alongside market changes.
const COMPETITORS: { name: string; in: number; out: number }[] = [
  { name: 'GPT-5.6 Sol', in: 5, out: 30 },
  { name: 'Claude Opus 4.8', in: 5, out: 25 },
  { name: 'GPT-5.4', in: 2.5, out: 15 },
  { name: 'Claude Sonnet 4.6', in: 3, out: 15 },
  { name: 'Gemini 3 Pro', in: 2, out: 12 },
  { name: 'Claude Haiku 4.5', in: 1, out: 5 },
  { name: 'Gemini 3 Flash', in: 0.5, out: 3 },
]

interface Econ { rewardPerUnit: number; aprPercent: number; uptimePerDay: number }
const ECON_FALLBACK: Econ = { rewardPerUnit: 0.01, aprPercent: 12, uptimePerDay: (2.0 + 1.0) * 24 }

interface SimInputs { nodes: number; requests: number; growthPct: number; elasticity: number; staked: number }

interface MonthPoint { m: number; nodes: number; requests: number; revenue: number; cost: number; net: number; cum: number }

function simulate(p: ModelPricing, s: SimInputs, econ: Econ) {
  const revenuePerReq = p.basePrice + (AVG_INPUT_TOKENS + p.estOut) * p.perToken
  const rewardPerReq = (p.estOut / 1000) * econ.rewardPerUnit * PERF_MULT
  // Staking is gated on an online node (backend enforces it), so staked KVR
  // only exists from online-node operators. Total stake therefore scales with
  // the online-node base rather than floating free — its emission can't outpace
  // the same node growth that also earns the revenue that recaptures it.
  const stakeDailyBase = (s.staked * econ.aprPercent) / 100 / 365
  const months: MonthPoint[] = []
  let cum = 0
  for (let m = 1; m <= 12; m++) {
    const nodes = s.nodes * Math.pow(1 + s.growthPct / 100, m)
    const nodeScale = nodes / Math.max(1, s.nodes)
    // Virtuous-cycle coupling: demand follows node growth with elasticity ε.
    const requests = s.requests * Math.pow(nodeScale, s.elasticity)
    const revenue = requests * revenuePerReq * 30
    // uptime is the single gateway/hub (fixed); staking scales with online nodes.
    const cost = (requests * rewardPerReq + econ.uptimePerDay + stakeDailyBase * nodeScale) * 30
    cum += revenue - cost
    months.push({ m, nodes, requests, revenue, cost, net: revenue - cost, cum })
  }
  const dailyRevenue = s.requests * revenuePerReq
  const dailyCost = s.requests * rewardPerReq + econ.uptimePerDay + stakeDailyBase
  return { revenuePerReq, rewardPerReq, dailyRevenue, dailyCost, dailyNet: dailyRevenue - dailyCost, months, cum12: cum }
}

const usd = (v: number) => `$${Number(v.toFixed(4)).toLocaleString(undefined, { maximumFractionDigits: 4 })}`

function Slider({ label, value, onChange, min, max, step, format }: {
  label: string; value: number; onChange: (v: number) => void
  min: number; max: number; step: number; format?: (v: number) => string
}) {
  return (
    <label style={{ display: 'block', minWidth: 150, flex: 1 }}>
      <div className="spread">
        <span className="small muted">{label}</span>
        <span className="small mono" style={{ fontWeight: 600 }}>{format ? format(value) : fmt(value)}</span>
      </div>
      <input type="range" min={min} max={max} step={step} value={value}
        onChange={(e) => onChange(Number(e.target.value))} style={{ width: '100%', padding: 0 }} />
    </label>
  )
}

// Horizontal bar with a rounded data-end, direct value label, native tooltip.
function HBar({ label, value, max, color, valueText, bold }: {
  label: string; value: number; max: number; color: string; valueText: string; bold?: boolean
}) {
  const w = max > 0 ? Math.max(0.5, (value / max) * 100) : 0
  return (
    <div className="row" style={{ gap: 8, padding: '2px 0' }} title={`${label}: ${valueText}`}>
      <div style={{ flex: 1, height: 8, background: 'var(--surface-2)', borderRadius: 4 }}>
        <div style={{ width: `${w}%`, height: 8, background: color, borderRadius: 4 }} />
      </div>
      <span className="small mono" style={{ width: 86, textAlign: 'right', fontWeight: bold ? 700 : 500 }}>{valueText}</span>
    </div>
  )
}

// 12-month cumulative net-flow line chart with crosshair + tooltip.
function CumulativeChart({ months, symbol }: { months: MonthPoint[]; symbol: string }) {
  const { t } = useI18n()
  const [hover, setHover] = useState<number | null>(null)
  const svgRef = useRef<SVGSVGElement>(null)
  const W = 560, H = 190, PAD_L = 8, PAD_R = 60, PAD_T = 14, PAD_B = 20
  const values = months.map((p) => p.cum)
  const lo = Math.min(0, ...values), hi = Math.max(0, ...values)
  const span = hi - lo || 1
  const x = (m: number) => PAD_L + ((m - 1) / 11) * (W - PAD_L - PAD_R)
  const y = (v: number) => PAD_T + (1 - (v - lo) / span) * (H - PAD_T - PAD_B)
  const path = months.map((p, i) => `${i === 0 ? 'M' : 'L'}${x(p.m).toFixed(1)},${y(p.cum).toFixed(1)}`).join(' ')
  const last = months[months.length - 1]
  const onMove = (e: React.MouseEvent<SVGSVGElement>) => {
    const rect = svgRef.current?.getBoundingClientRect()
    if (!rect) return
    const px = ((e.clientX - rect.left) / rect.width) * W
    const m = Math.round(((px - PAD_L) / (W - PAD_L - PAD_R)) * 11) + 1
    setHover(Math.min(12, Math.max(1, m)))
  }
  const hp = hover ? months[hover - 1] : null
  return (
    <div style={{ position: 'relative' }}>
      <svg ref={svgRef} viewBox={`0 0 ${W} ${H}`} style={{ width: '100%', display: 'block' }}
        onMouseMove={onMove} onMouseLeave={() => setHover(null)}>
        {/* zero baseline */}
        <line x1={PAD_L} x2={W - PAD_R} y1={y(0)} y2={y(0)} stroke="var(--text-3)" strokeDasharray="3 4" strokeWidth={1} />
        <text x={W - PAD_R + 4} y={y(0) + 3} fontSize={10} fill="var(--text-3)">0</text>
        {/* month ticks */}
        {[1, 4, 8, 12].map((m) => (
          <text key={m} x={x(m)} y={H - 6} fontSize={10} fill="var(--text-3)" textAnchor="middle">{m}{t('sim.monthSuffix')}</text>
        ))}
        <path d={path} fill="none" stroke={last.cum >= 0 ? 'var(--chart-1)' : 'var(--chart-2)'} strokeWidth={2} strokeLinejoin="round" />
        {/* end-point direct label */}
        <circle cx={x(12)} cy={y(last.cum)} r={3.5} fill={last.cum >= 0 ? 'var(--chart-1)' : 'var(--chart-2)'} />
        <text x={W - PAD_R + 4} y={y(last.cum) + 3} fontSize={10} fill="var(--text)" fontWeight={700}>
          {fmt(Math.round(last.cum))}
        </text>
        {hp && (
          <g>
            <line x1={x(hp.m)} x2={x(hp.m)} y1={PAD_T} y2={H - PAD_B} stroke="var(--text-3)" strokeWidth={1} />
            <circle cx={x(hp.m)} cy={y(hp.cum)} r={4} fill="none" stroke="var(--text)" strokeWidth={1.5} />
          </g>
        )}
      </svg>
      {hp && (
        <div className="card" style={{
          position: 'absolute', top: 0, left: `${(x(hp.m) / W) * 100}%`,
          transform: `translateX(${hp.m > 8 ? '-105%' : '8px'})`,
          padding: '6px 10px', pointerEvents: 'none', boxShadow: 'var(--shadow)', zIndex: 5,
        }}>
          <div className="small" style={{ fontWeight: 700 }}>{hp.m}{t('sim.monthSuffix')}</div>
          <div className="small mono">{t('sim.cumNet')}: {fmt(Math.round(hp.cum))} {symbol}</div>
          <div className="small muted mono">{t('sim.nodes')}: {Math.round(hp.nodes)} · {t('sim.requests')}: {fmt(Math.round(hp.requests))}</div>
        </div>
      )}
    </div>
  )
}

export function PricingSim({ pricing, symbol, gwUrl }: { pricing: ModelPricing; symbol: string; gwUrl: string }) {
  const { t } = useI18n()
  const [econ, setEcon] = useState<Econ>(ECON_FALLBACK)
  const [inputs, setInputs] = useState<SimInputs>({ nodes: 10, requests: 2000, growthPct: 15, elasticity: 0.8, staked: 100000 })
  const [showTable, setShowTable] = useState(false)

  // Ground the reward parameters (and the node-count default) in the live gateway.
  useEffect(() => {
    let alive = true
    const st = new Staking(gwUrl)
    st.config().then((c: any) => {
      if (!alive || !c) return
      setEcon({
        rewardPerUnit: Number(c.rewardPerUnit) || ECON_FALLBACK.rewardPerUnit,
        aprPercent: Number(c.aprPercent) || ECON_FALLBACK.aprPercent,
        uptimePerDay: ((Number(c.hubUptimePerHour) || 2) + (Number(c.gatewayUptimePerHour) || 1)) * 24,
      })
    }).catch(() => {})
    st.allNodes().then((g) => {
      if (alive && g?.totals?.online > 0) setInputs((s) => ({ ...s, nodes: g.totals.online }))
    }).catch(() => {})
    return () => { alive = false }
  }, [gwUrl])

  const sim = useMemo(() => simulate(pricing, inputs, econ), [pricing, inputs, econ])
  const set = (k: keyof SimInputs) => (v: number) => setInputs((s) => ({ ...s, [k]: v }))

  const kvUsdPerM = pricing.perToken * 1e6 * USD_PER_KVR
  const compareMax = Math.max(...COMPETITORS.map((c) => c.out), kvUsdPerM, 1)
  const flagship = COMPETITORS[0]
  const cheaperX = kvUsdPerM > 0 ? flagship.out / kvUsdPerM : 0
  const annualNet = sim.dailyNet * 365
  const sustainable = sim.cum12 >= 0

  return (
    <Card>
      <div className="spread" style={{ marginBottom: 4 }}>
        <div className="label">{t('sim.title')}</div>
        {/* status is icon + text, never color alone */}
        <span className="pill" style={{
          color: sustainable ? 'var(--good)' : 'var(--danger)',
          background: `color-mix(in srgb, ${sustainable ? 'var(--good)' : 'var(--danger)'} 16%, transparent)`,
        }}>
          {sustainable ? '✓ ' + t('sim.sustainable') : '⚠ ' + t('sim.inflation')}
        </span>
      </div>
      <div className="small muted" style={{ marginBottom: 14 }}>{t('sim.subtitle')}</div>

      {/* inputs */}
      <div className="row" style={{ gap: 18, flexWrap: 'wrap', marginBottom: 14 }}>
        <Slider label={t('sim.nodes')} value={inputs.nodes} onChange={set('nodes')} min={1} max={500} step={1} format={(v) => String(v)} />
        <Slider label={t('sim.requests')} value={inputs.requests} onChange={set('requests')} min={0} max={100000} step={100} format={(v) => fmt(v)} />
        <Slider label={t('sim.growth')} value={inputs.growthPct} onChange={set('growthPct')} min={0} max={50} step={1} format={(v) => `${v}%`} />
        <Slider label={t('sim.elasticity')} value={inputs.elasticity} onChange={set('elasticity')} min={0} max={1.5} step={0.05} format={(v) => v.toFixed(2)} />
        <Slider label={t('sim.staked')} value={inputs.staked} onChange={set('staked')} min={0} max={5000000} step={10000} format={(v) => fmt(v)} />
      </div>
      <div className="small muted" style={{ marginTop: -4, marginBottom: 12 }}>
        {t('sim.stakeGated')}
      </div>

      {/* two columns: KVR flows (left) · competitor comparison (right) */}
      <div className="grid" style={{ gridTemplateColumns: '1fr 1fr', gap: 24, alignItems: 'start' }}>
        <div>
          {/* daily flows: stat tiles + two labeled bars */}
          <div className="grid cols-3" style={{ gap: 10, marginBottom: 10 }}>
            <Stat v={`${fmt(Math.round(sim.dailyRevenue))} ${symbol}`} k={t('sim.dailyRevenue')} />
            <Stat v={`${fmt(Math.round(sim.dailyCost))} ${symbol}`} k={t('sim.dailyRewards')} />
            <Stat v={<span style={{ color: sim.dailyNet >= 0 ? 'var(--good)' : 'var(--danger)' }}>
              {sim.dailyNet >= 0 ? '▲' : '▼'} {fmt(Math.round(Math.abs(sim.dailyNet)))} {symbol}
            </span>} k={t('sim.dailyNet')} />
          </div>
          <div style={{ marginBottom: 4 }}>
            <div className="small muted" style={{ marginBottom: 2 }}>{t('sim.dailyRevenue')}</div>
            <HBar label={t('sim.dailyRevenue')} value={sim.dailyRevenue} max={Math.max(sim.dailyRevenue, sim.dailyCost)}
              color="var(--chart-1)" valueText={fmt(Math.round(sim.dailyRevenue))} />
            <div className="small muted" style={{ margin: '6px 0 2px' }}>{t('sim.dailyRewards')}</div>
            <HBar label={t('sim.dailyRewards')} value={sim.dailyCost} max={Math.max(sim.dailyRevenue, sim.dailyCost)}
              color="var(--chart-2)" valueText={fmt(Math.round(sim.dailyCost))} />
          </div>
          <div className="small muted" style={{ marginBottom: 14 }}>
            {t('sim.perReq')}: {fmt(sim.revenuePerReq)} {symbol} → {fmt(sim.rewardPerReq)} {symbol} · {t('sim.annual')}: {annualNet >= 0 ? '+' : '−'}{fmt(Math.round(Math.abs(annualNet)))} {symbol}
          </div>

          {/* 12-month cumulative net flow */}
          <div className="spread" style={{ marginBottom: 4 }}>
            <div className="small" style={{ fontWeight: 700 }}>{t('sim.cumTitle')}</div>
            <button className="btn ghost small" onClick={() => setShowTable((v) => !v)}>{showTable ? t('set.hide') : t('sim.table')}</button>
          </div>
          {!showTable ? (
            <CumulativeChart months={sim.months} symbol={symbol} />
          ) : (
            <div style={{ overflowX: 'auto', marginBottom: 8 }}>
              <table className="small mono" style={{ borderCollapse: 'collapse', width: '100%' }}>
                <thead><tr>
                  {[t('sim.monthSuffix'), t('sim.nodes'), t('sim.requests'), t('sim.dailyRevenue'), t('sim.dailyRewards'), t('sim.cumNet')].map((h) => (
                    <th key={h} style={{ textAlign: 'right', padding: '4px 8px', borderBottom: '1px solid var(--stroke)', color: 'var(--text-2)', fontWeight: 600 }}>{h}</th>
                  ))}
                </tr></thead>
                <tbody>
                  {sim.months.map((p) => (
                    <tr key={p.m}>
                      <td style={{ textAlign: 'right', padding: '3px 8px' }}>{p.m}</td>
                      <td style={{ textAlign: 'right', padding: '3px 8px' }}>{Math.round(p.nodes)}</td>
                      <td style={{ textAlign: 'right', padding: '3px 8px' }}>{fmt(Math.round(p.requests))}</td>
                      <td style={{ textAlign: 'right', padding: '3px 8px' }}>{fmt(Math.round(p.revenue))}</td>
                      <td style={{ textAlign: 'right', padding: '3px 8px' }}>{fmt(Math.round(p.cost))}</td>
                      <td style={{ textAlign: 'right', padding: '3px 8px', fontWeight: 700 }}>{fmt(Math.round(p.cum))}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>

        <div>
          {/* competitor price comparison */}
          <div className="spread" style={{ marginBottom: 4 }}>
            <div className="small" style={{ fontWeight: 700 }}>{t('sim.compareTitle')}</div>
            <span className="row" style={{ gap: 10 }}>
              <span className="small muted"><span style={{ display: 'inline-block', width: 9, height: 9, borderRadius: 2, background: 'var(--chart-1)', marginRight: 4 }} />{t('sim.input')}</span>
              <span className="small muted"><span style={{ display: 'inline-block', width: 9, height: 9, borderRadius: 2, background: 'var(--chart-2)', marginRight: 4 }} />{t('sim.output')}</span>
            </span>
          </div>
          {cheaperX > 1 && (
            <div className="small" style={{ color: 'var(--good)', marginBottom: 8 }}>
              {t('sim.cheaper', flagship.name, cheaperX >= 10 ? String(Math.round(cheaperX)) : cheaperX.toFixed(1))}
            </div>
          )}
          <div className="grid" style={{ gap: 8 }}>
            {[{ name: 'Kvasir', in: kvUsdPerM, out: kvUsdPerM, self: true }, ...COMPETITORS].map((c: any) => (
              <div key={c.name}>
                <div className="small" style={{ fontWeight: c.self ? 700 : 500, marginBottom: 1 }}>
                  {c.self ? '★ ' : ''}{c.name}{c.self ? ` (${t('sim.you')})` : ''}
                </div>
                <HBar label={`${c.name} ${t('sim.input')}`} value={c.in} max={compareMax} color="var(--chart-1)" valueText={usd(c.in)} bold={c.self} />
                <HBar label={`${c.name} ${t('sim.output')}`} value={c.out} max={compareMax} color="var(--chart-2)" valueText={usd(c.out)} bold={c.self} />
              </div>
            ))}
          </div>
          <div className="small muted" style={{ marginTop: 10 }}>
            {t('sim.assumptions', String(AVG_INPUT_TOKENS), String(pricing.estOut), `${econ.aprPercent}`, usd(USD_PER_KVR))}
          </div>
        </div>
      </div>
    </Card>
  )
}
