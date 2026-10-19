import { useState } from 'react'
import { Card } from '../components'
import { useI18n } from '../i18n'
import type { BackboneCandidate, NetworkPlanStatus } from '../api'

/** The backbone this PC would attach to: the operator's pick, else the best measured one. */
export function chosenBackbone(status?: NetworkPlanStatus | null): BackboneCandidate | null {
  if (!status) return null
  return status.targets.find((t) => t.id === status.selectedId) ?? status.targets.find((t) => t.id === status.bestId) ?? null
}

/** Expected speed if this PC takes one remote layer on that backbone: the cheapest useful participation. */
export function oneLayerTps(target?: BackboneCandidate | null) {
  return target?.plan?.rows.find((r) => r.layers === 1)?.tps ?? null
}

/** True when even one layer on the chosen backbone would leave the ring at or below the warning speed. */
export function networkTooSlow(status?: NetworkPlanStatus | null) {
  const target = chosenBackbone(status)
  const tps = oneLayerTps(target)
  return status?.state !== 'measuring' && target?.state === 'ready' && tps != null && tps <= (status?.warnTps ?? 5)
}

function PlanTable({ target }: { target: BackboneCandidate }) {
  const { t } = useI18n()
  const plan = target.plan!
  return <>
    <div className="small muted">{t('ns.netPlanPerLayer', plan.perLayerMs.toFixed(1), (plan.hitProbability * 100).toFixed(0), plan.baseTps.toFixed(1))}</div>
    <table className="small" style={{ width: '100%', borderCollapse: 'collapse', marginTop: 6 }}>
      <thead><tr>
        <th style={{ textAlign: 'left', padding: 3 }}>{t('ns.netPlanColLayers')}</th>
        <th style={{ textAlign: 'left', padding: 3 }}>{t('ns.netPlanColToken')}</th>
        <th style={{ textAlign: 'left', padding: 3 }}>{t('ns.netPlanColTps')}</th>
      </tr></thead>
      <tbody>{plan.rows.map((r) => {
        const pick = r.layers === plan.recommendedLayers && r.layers > 0
        return <tr key={r.layers} data-layers={r.layers} data-recommended={pick}
          style={{ borderTop: '1px solid var(--line)', fontWeight: pick ? 700 : 400, color: r.meetsTarget || r.layers === 0 ? undefined : 'var(--muted)' }}>
          <td style={{ padding: 3 }}>{r.layers}{pick ? ` · ${t('ns.netPlanRecommended')}` : ''}</td>
          <td style={{ padding: 3 }}>{r.tokenMs.toFixed(0)} ms</td>
          <td style={{ padding: 3 }}>{r.tps.toFixed(1)}</td>
        </tr>
      })}</tbody>
    </table>
    <div className="small" style={{ marginTop: 6 }}>
      {plan.recommendedLayers > 0 ? t('ns.netPlanPick', plan.recommendedLayers, plan.targetTps.toFixed(1)) : t('ns.netPlanNone', plan.targetTps.toFixed(1))}
    </div>
  </>
}

// Every candidate backbone, measured directly from this PC, with what its round
// trip costs the ring per layer this PC would hold — and which one to attach to.
export function NetworkPlanCard({ status, onMeasure, onSelect }: {
  status?: NetworkPlanStatus | null; onMeasure?: () => void; onSelect?: (id: string) => void
}) {
  const { t } = useI18n()
  const measuring = status?.state === 'measuring'
  const chosen = chosenBackbone(status)
  return <Card>
    <div className="spread">
      <div className="label">{t('ns.netPlanTitle')}</div>
      {onMeasure && <button className="btn ghost" style={{ width: 'auto' }} disabled={measuring} aria-busy={measuring} onClick={onMeasure}>
        {measuring && <span className="measure-spinner" aria-hidden="true" />}{measuring ? t('ns.measuring') : t('ns.netPlanRemeasure')}
      </button>}
    </div>
    {!status || status.state === 'idle' ? <div className="small muted">{t('ns.netPlanIdle')}</div> : <>
      {measuring && <div role="status" className="small muted">{t('ns.netPlanMeasuring')}</div>}
      <table className="small" style={{ width: '100%', borderCollapse: 'collapse', marginTop: 6 }}>
        <thead><tr>
          <th style={{ textAlign: 'left', padding: 3 }}>{t('ns.netPlanColBackbone')}</th>
          <th style={{ textAlign: 'left', padding: 3 }}>{t('ns.netPlanColRtt')}</th>
          <th style={{ textAlign: 'left', padding: 3 }}>{t('ns.netPlanColBest')}</th>
          <th style={{ textAlign: 'left', padding: 3 }} />
        </tr></thead>
        <tbody>{status.targets.map((c) => {
          const isChosen = c.id === chosen?.id
          return <tr key={c.id} data-backbone={c.id} data-chosen={isChosen} style={{ borderTop: '1px solid var(--line)', fontWeight: isChosen ? 700 : 400 }}>
            <td style={{ padding: 3 }}>
              <div>{c.label}{c.id === status.bestId ? ` · ${t('ns.netPlanBest')}` : ''}</div>
              <div className="muted mono" style={{ fontSize: 10 }}>{c.host} · {c.serving ? t('ns.netPlanServing') : t('ns.netPlanNotServing')}</div>
            </td>
            <td style={{ padding: 3 }}>{c.state === 'measuring' ? '…' : c.rtt ? `${c.rtt.p50Ms.toFixed(1)} ms` : c.state === 'failed' ? t('ns.netPlanUnreachable') : '—'}</td>
            <td style={{ padding: 3 }}>{c.plan ? t('ns.netPlanBestCell', c.plan.recommendedLayers, (c.plan.rows.find((r) => r.layers === Math.max(1, c.plan!.recommendedLayers))?.tps ?? 0).toFixed(1)) : '—'}</td>
            <td style={{ padding: 3 }}>{onSelect && <button className="btn ghost" style={{ width: 'auto', padding: '2px 8px' }}
              disabled={isChosen || c.state !== 'ready'} onClick={() => onSelect(c.id)}>{isChosen ? t('ns.netPlanSelected') : t('ns.netPlanSelect')}</button>}</td>
          </tr>
        })}</tbody>
      </table>
      {chosen && <div style={{ marginTop: 10 }}>
        <div className="small">{t('ns.netPlanPath', chosen.label, chosen.host)}</div>
        {chosen.rtt && <div className="small">{t('ns.netPlanRtt', chosen.rtt.p50Ms.toFixed(1), chosen.rtt.p90Ms.toFixed(1), chosen.rtt.samples)}</div>}
        {chosen.error && <div role="alert" className="small" style={{ color: 'var(--danger)' }}>{t('ns.netPlanFailed', chosen.error)}</div>}
        {!chosen.serving && <div className="small" style={{ color: 'var(--warn)' }}>{t('ns.netPlanCandidateNote', chosen.label)}</div>}
        {chosen.plan && <PlanTable target={chosen} />}
      </div>}
      <div className="small muted" style={{ marginTop: 6 }}>{t('ns.netPlanEstimate')}</div>
    </>}
  </Card>
}

/** Shown once per measurement when the chosen backbone cannot carry even one layer from here fast enough. */
export function SlowNetworkDialog({ status, onClose, onMeasure }: { status: NetworkPlanStatus; onClose: () => void; onMeasure: () => void }) {
  const { t } = useI18n()
  const target = chosenBackbone(status)
  const tps = oneLayerTps(target)
  return <div onClick={onClose} role="presentation"
    style={{ position: 'fixed', inset: 0, background: 'rgba(0,0,0,.5)', display: 'flex', alignItems: 'center', justifyContent: 'center', zIndex: 50 }}>
    <div className="card" role="dialog" aria-modal="true" aria-label={t('ns.netSlowTitle')} onClick={(e) => e.stopPropagation()}
      style={{ width: 420, maxWidth: '90vw', display: 'flex', flexDirection: 'column', gap: 10 }}>
      <div style={{ fontWeight: 700, fontSize: 16 }}>{t('ns.netSlowTitle')}</div>
      {target && <div className="small">{t('ns.netPlanPath', target.label, target.host)}</div>}
      {target?.rtt && <div className="small">{t('ns.netPlanRtt', target.rtt.p50Ms.toFixed(1), target.rtt.p90Ms.toFixed(1), target.rtt.samples)}</div>}
      <div className="small">{t('ns.netSlowBody', tps == null ? '—' : tps.toFixed(1), (status.warnTps ?? 5).toFixed(1))}</div>
      <div className="small muted">{t('ns.netSlowHint')}</div>
      <div className="row" style={{ gap: 8, justifyContent: 'flex-end' }}>
        <button className="btn ghost" onClick={onClose}>{t('common.close')}</button>
        <button className="btn" onClick={onMeasure}>{t('ns.netPlanRemeasure')}</button>
      </div>
    </div>
  </div>
}

/** Keeps the dialog to once per completed measurement (keyed by its timestamp). */
export function useSlowNetworkDialog(status?: NetworkPlanStatus | null) {
  const [dismissedAt, setDismissedAt] = useState<number | null>(null)
  const open = networkTooSlow(status) && status?.at != null && status.at !== dismissedAt
  return { open, dismiss: () => setDismissedAt(status?.at ?? null) }
}
