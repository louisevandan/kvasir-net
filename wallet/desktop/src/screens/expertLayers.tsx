import type { ReactNode } from 'react'
import { Card } from '../components'
import { useI18n } from '../i18n'
import type { NodeStatus } from '../api'

type Slot = NonNullable<NodeStatus['expertWork']>['slots'][number]
export function expertLayerState(slot: Slot) {
  if (slot.phase === 'downloading') return 'downloading'
  if (slot.phase === 'serving') return slot.relay.connected ? 'serving' : 'relayDown'
  if (slot.phase === 'starting') return slot.held?.shardFile ? 'cached' : 'starting'
  return slot.phase === 'failed' ? 'failed' : 'idle'
}

// Diagnostic only: never changes allocation. Cost includes headroom once and
// fixed/scratch costs for each held slot, just like executors.slotBytes.
export function expertBudget(slots: Slot[], status?: Partial<NodeStatus> | null) {
  const held = slots.filter(s => s.held)
  const experts = held.reduce((n, s) => n + s.held!.end - s.held!.begin, 0)
  const model = status?.expertMemoryModel
  const bytes = model ? model.headroomBytes + held.reduce((n, s) => n + model.fixedBytes + model.scratchBytes +
    (s.held!.end - s.held!.begin) * model.residentBytesPerExpert, 0) : null
  const over = (status?.maxExperts != null && experts > status.maxExperts) ||
    (bytes != null && status?.vramBudgetBytes != null && bytes > status.vramBudgetBytes)
  return { experts, bytes, state: over ? 'over' : bytes != null && status?.vramBudgetBytes != null && status?.maxExperts != null ? 'within' : 'unknown' }
}

export function ExpertLayersCard({ slots = [], status, children }: {
  slots?: Slot[]; status?: Partial<NodeStatus> | null; children?: ReactNode
}) {
  const { t } = useI18n()
  const assigned = slots.filter(slot => slot.held)
  const layers = new Set(assigned.map(s => `${s.held!.model}:${s.held!.layer}`)).size
  const serving = assigned.filter(s => s.phase === 'serving').length
  const connecting = assigned.filter(s => s.phase === 'downloading' || s.phase === 'starting' || (s.phase === 'serving' && !s.relay.connected)).length
  const rows = assigned.reduce((n, s) => n + s.work.rowsPerSecond, 0)
  const lastWork = Math.max(0, ...assigned.map(s => s.work.lastWorkAt ?? 0))
  const budget = expertBudget(slots, status)
  const gib = (bytes: number | null | undefined) => bytes == null ? '—' : `${(bytes / 1024 ** 3).toFixed(2)} GiB`
  const total = status?.expertWork?.measurement?.total
  const upstreamDown = assigned.some(s => s.phase === 'serving' && !s.relay.connected && s.relay.lastClose?.code === 1011)
  return <Card>
    <div className="label">{t('ns.expertLayersTitle')}</div>
    {!assigned.length ? <div className="small muted">{t('ns.expertWorkNone')}</div> : <>
      <div className="small">{t('ns.expertNodeSummary', layers, serving, connecting, rows.toFixed(1), lastWork ? new Date(lastWork).toLocaleString() : t('ns.expertNeverWorked'))}</div>
      <div className="small muted">{t('ns.expertWorkHint')}</div>
      {upstreamDown && <div className="small" role="note" style={{ marginTop: 8 }}>{t('ns.expertUpstreamUnavailable')}</div>}
      <div className="small" style={{ color: budget.state === 'over' ? 'var(--danger)' : 'var(--muted)', marginTop: 8 }}>
        {t(`ns.expertBudget.${budget.state}`, budget.experts, status?.maxExperts ?? '—', gib(budget.bytes), gib(status?.vramBudgetBytes))}
      </div>
      {total && <div className="small" style={{ marginTop: 8 }}>
        <div>{t('ns.expertBenchTitle')}</div>
        <strong>{t('ns.expertBenchTotal', total.slots, total.rowsPerSecond.toFixed(1), total.p95Ms.toFixed(2))}</strong>
        <div className="muted">{t('ns.expertBenchSumHint')}</div>
      </div>}
      <details style={{ marginTop: 8 }}>
        <summary className="small" style={{ cursor: 'pointer' }}>{t('ns.expertLayerDetails', assigned.length)}</summary>
        <div style={{ overflowX: 'auto' }}>
          <table className="small" style={{ width: '100%', borderCollapse: 'collapse' }}>
            <thead><tr>{['layer', 'experts', 'state', 'live', 'bench', 'latency'].map(key => <th key={key} style={{ textAlign: 'left', padding: 4 }}>{t(`ns.expertColumn.${key}`)}</th>)}</tr></thead>
            <tbody>{assigned.map(slot => {
              const h = slot.held!, bench = slot.localBenchmark
              return <tr key={slot.workerId} style={{ borderTop: '1px solid var(--line)' }}>
                <td style={{ padding: 4 }} title={t('ns.expertShardFile', h.shardFile ?? '—', slot.port ?? '—')}>{h.model} · L{h.layer}</td>
                <td style={{ padding: 4 }}>[{h.begin}, {h.end}) / {h.nExpert ?? '—'}</td>
                <td style={{ padding: 4 }}>{t(`ns.expertLayerState.${expertLayerState(slot)}`)}</td>
                <td style={{ padding: 4 }} title={t('ns.expertWorkWindow', (slot.work.windowMs / 1000).toFixed(1), slot.work.calls, slot.work.rows)}>{slot.phase === 'serving' ? slot.work.rowsPerSecond.toFixed(1) : '—'}</td>
                <td style={{ padding: 4 }} title={bench ? t('ns.expertBenchDetail', bench.batchRows, bench.expertsPerRow, bench.calls, new Date(bench.at).toLocaleString()) : ''}>{bench?.rowsPerSecond.toFixed(1) ?? '—'}</td>
                <td style={{ padding: 4 }}>{bench ? `${bench.p50Ms.toFixed(2)} / ${bench.p95Ms.toFixed(2)}` : '—'}</td>
              </tr>
            })}</tbody>
          </table>
        </div>
      </details>
    </>}
    {children}
  </Card>
}
