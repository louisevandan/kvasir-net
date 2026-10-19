import { Card, TierBadge, tierColor } from '../components'
import { useI18n } from '../i18n'
import type { NetworkPlanStatus, NodeStatus } from '../api'
import { chosenBackbone } from './networkPlan'
import { ExpertMeasureControl } from './expertMeasure'
import type { MeasureState } from './expertMeasure'

// Tier bands of the reward table (decode tok/s), lowest first, and the scale the
// band bar is drawn on.
export const TIER_BANDS: { tier: string; from: number; to: number }[] = [
  { tier: 'C', from: 0, to: 30 }, { tier: 'B', from: 30, to: 60 }, { tier: 'A', from: 60, to: 90 }, { tier: 'S', from: 90, to: 120 },
]
export const tierFor = (tps: number) => (tps >= 90 ? 'S' : tps >= 60 ? 'A' : tps >= 30 ? 'B' : 'C')

/**
 * What the node settings say about this machine, reduced to the numbers this
 * card shows. Expected ring speed comes from the chosen backbone's plan at the
 * recommended layer count (at least one layer); the local decode benchmark is
 * shown beside it when one has run.
 */
export function machinePerf(status?: NodeStatus | null, plan?: NetworkPlanStatus | null) {
  const backbone = chosenBackbone(plan)
  const layers = backbone?.plan ? Math.max(1, backbone.plan.recommendedLayers) : null
  const expectedTps = backbone?.plan?.rows.find((r) => r.layers === layers)?.tps ?? null
  const localTps = status?.localBenchmark?.tps ?? status?.measured?.tps ?? null
  const total = status?.expertWork?.measurement?.total ?? null
  const liveRows = (status?.expertWork?.slots ?? []).reduce((n, s) => n + (s.work?.rowsPerSecond ?? 0), 0)
  return { backbone, layers, expectedTps, localTps, total, liveRows,
    budgetBytes: status?.vramBudgetBytes ?? null, maxExperts: status?.maxExperts ?? null,
    heldSlots: (status?.expertWork?.slots ?? []).filter((s) => s.held).length }
}

/** C→S band bar with a marker for each measured speed. */
export function TierBandBar({ marks }: { marks: { label: string; tps: number }[] }) {
  const max = TIER_BANDS[TIER_BANDS.length - 1].to
  return <div style={{ position: 'relative', marginTop: 8, paddingTop: 18 }}>
    <div style={{ display: 'flex', height: 10, borderRadius: 5, overflow: 'hidden' }}>
      {TIER_BANDS.map((b) => <div key={b.tier} data-band={b.tier} title={`${b.tier} · ${b.from}–${b.to} tok/s`}
        style={{ flex: b.to - b.from, background: `color-mix(in srgb, ${tierColor(b.tier)} 45%, transparent)` }} />)}
    </div>
    <div className="small muted" style={{ display: 'flex' }}>
      {TIER_BANDS.map((b) => <div key={b.tier} style={{ flex: b.to - b.from, textAlign: 'center', fontSize: 10 }}>{b.tier} · {b.from}{b.tier === 'S' ? '+' : `–${b.to}`}</div>)}
    </div>
    {marks.map((m) => {
      const left = Math.min(100, Math.max(0, (m.tps / max) * 100))
      return <div key={m.label} data-mark={m.label} style={{ position: 'absolute', top: 0, left: `${left}%`, transform: 'translateX(-50%)', textAlign: 'center' }}>
        <div className="mono" style={{ fontSize: 10, fontWeight: 700, color: tierColor(tierFor(m.tps)), whiteSpace: 'nowrap' }}>{m.label} {m.tps.toFixed(1)}</div>
        <div style={{ width: 2, height: 14, margin: '0 auto', background: tierColor(tierFor(m.tps)) }} />
      </div>
    })}
  </div>
}

export function MachinePerfView({ status, plan, measure, onMeasure, onOpenSettings }: {
  status?: NodeStatus | null; plan?: NetworkPlanStatus | null
  measure?: { state: MeasureState; progress?: any; ready: boolean; disabled?: boolean } | null
  onMeasure?: () => void; onOpenSettings?: () => void
}) {
  const { t } = useI18n()
  const p = machinePerf(status, plan)
  const gib = (b: number | null) => (b == null ? '—' : `${(b / 1024 ** 3).toFixed(1)} GiB`)
  const marks = [
    ...(p.expectedTps != null ? [{ label: t('node.perfExpectedMark'), tps: p.expectedTps }] : []),
    ...(p.localTps != null ? [{ label: t('node.perfLocalMark'), tps: p.localTps }] : []),
  ]
  return <Card>
    <div className="spread">
      <div className="label">{t('node.perfTitle')}</div>
      {p.expectedTps != null && <TierBadge tier={tierFor(p.expectedTps)} size={24} />}
    </div>
    <div className="small muted" style={{ marginBottom: 8 }}>{t('node.perfDesc')}</div>
    {!status ? <div className="small muted">{t('dev.connectChecking')}</div> : <>
      {marks.length ? <TierBandBar marks={marks} /> : <div className="small muted">{t('node.perfNoPlan')}</div>}
      <div className="small" style={{ marginTop: 10 }}>
        {p.expectedTps != null && <div data-expected>{t('node.perfExpected', p.expectedTps.toFixed(1), p.layers ?? 0, p.backbone?.label ?? '—')}</div>}
        {p.backbone?.rtt && <div className="muted">{t('node.perfPath', p.backbone.label, p.backbone.rtt.p50Ms.toFixed(1),
          p.backbone.rtt.source === 'relay' ? t('ns.netPlanSourceRelay') : t('ns.netPlanSourceTcp'))}</div>}
        {p.total && <div data-bench>{t('node.perfBench', p.total.slots, p.total.rowsPerSecond.toFixed(1), p.total.p95Ms.toFixed(2))}</div>}
        <div className="muted">{t('node.perfLive', p.liveRows.toFixed(1))}</div>
        <div className="muted">{t('node.perfBudget', gib(p.budgetBytes), p.maxExperts ?? '—', p.heldSlots)}</div>
        {p.localTps != null && <div className="muted">{t('node.perfLocal', p.localTps.toFixed(1), tierFor(p.localTps))}</div>}
      </div>
      <div className="small muted" style={{ marginTop: 8 }}>{t('node.perfExpertNote')}</div>
      {measure && onMeasure && <ExpertMeasureControl state={measure.state} progress={measure.progress} ready={measure.ready} disabled={measure.disabled} onMeasure={onMeasure} />}
      {onOpenSettings && <button className="btn ghost block" style={{ marginTop: 8 }} onClick={onOpenSettings}>{t('node.perfOpenSettings')}</button>}
    </>}
  </Card>
}

