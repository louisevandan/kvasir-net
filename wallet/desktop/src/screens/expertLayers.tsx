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

const MB = 1024 * 1024
// Fills as bytes arrive (fed by the fast download poll). Without a
// content-length the bar is indeterminate rather than a made-up percentage.
export function ShardDownloadBar({ download, now = Date.now() }: { download: { bytes: number; totalBytes: number | null; startedAt: number } | null; now?: number }) {
  const { t } = useI18n()
  const ratio = download?.totalBytes ? Math.min(1, download.bytes / download.totalBytes) : null
  const secs = download ? Math.max(0.001, (now - download.startedAt) / 1000) : 0
  const rate = download ? download.bytes / MB / secs : 0
  return <div style={{ gridColumn: '1 / -1', marginTop: 2 }}>
    <div role="progressbar" aria-label={t('ns.expertDownloading')} aria-valuemin={0} aria-valuemax={100}
      aria-valuenow={ratio == null ? undefined : Math.round(ratio * 100)}
      className={ratio == null ? 'shard-progress indeterminate' : 'shard-progress'}
      style={{ height: 4, borderRadius: 2, background: 'var(--surface-2)', overflow: 'hidden' }}>
      <div style={{ height: '100%', width: ratio == null ? '30%' : `${(ratio * 100).toFixed(1)}%`, background: 'var(--pink)', transition: 'width 0.35s linear' }} />
    </div>
    <div className="small muted">{download
      ? t('ns.expertDownloadProgress', (download.bytes / MB).toFixed(1), download.totalBytes ? (download.totalBytes / MB).toFixed(1) : '?', ratio == null ? '—' : Math.round(ratio * 100), rate.toFixed(1))
      : t('ns.expertDownloading')}</div>
  </div>
}

// The picture of what this node holds: the model's layer stack with held layers
// lit, and one strip per held layer showing its expert range. Metadata only —
// topology the assignment/GGUF did not state is shown as unknown, not guessed.
function ExpertLayerMap({ assigned }: { assigned: Slot[] }) {
  const { t } = useI18n()
  const models = [...new Set(assigned.map(slot => slot.held!.model))]
  return <>{models.map(model => {
    const held = assigned.filter(slot => slot.held!.model === model)
    const topology = held.find(slot => slot.held!.nLayer)?.held
    const layers = topology?.nLayer && topology.nLayer > 0 && topology.nLayer <= 512 ? topology.nLayer : null
    const dense = topology?.denseLayers
    return <div key={model} style={{ marginTop: 10 }}>
      <strong className="small">{model}</strong>
      <div className="small muted">{t('ns.expertLayersLegend')}</div>
      {layers ? <div style={{ display: 'grid', gridTemplateColumns: 'repeat(15, 1fr)', gap: 3, marginTop: 6 }}>
        {Array.from({ length: layers }, (_, layer) => {
          const owned = held.some(slot => slot.held!.layer === layer)
          const isDense = dense != null && layer < dense
          return <div key={layer} data-layer={layer} data-held={owned} data-dense={isDense}
            title={t('ns.expertLayerCell', layer, isDense ? t('ns.expertDense') : owned ? t('ns.expertHeld') : t('ns.expertUnheld'))}
            style={{ textAlign: 'center', fontSize: 10, padding: 3, borderRadius: 3,
              border: '1px solid var(--line)', background: owned ? 'var(--pink)' : isDense ? 'var(--surface-2)' : 'transparent',
              color: owned ? 'var(--bg)' : 'var(--muted)' }}>{layer}{isDense ? ' D' : ''}</div>
        })}
      </div> : <div className="small muted">{t('ns.expertTopologyUnknown')}</div>}
      {dense == null && <div className="small muted">{t('ns.expertDenseUnknown')}</div>}
      <div style={{ marginTop: 8 }}>
        {held.slice().sort((a, b) => a.held!.layer - b.held!.layer).map(slot => {
          const h = slot.held!
          const count = h.nExpert && h.nExpert > 0 && h.nExpert <= 4096 ? h.nExpert : null
          return <div key={slot.workerId} style={{ display: 'grid', gridTemplateColumns: '3.2em 1fr auto', alignItems: 'center', gap: 6, marginTop: 3 }}>
            <span className="small mono muted">L{h.layer}</span>
            {count ? <div aria-label={t('ns.expertHeldRange')} title={`${h.begin}–${h.end} / ${count}`}
              style={{ display: 'grid', gridTemplateColumns: `repeat(${count}, 1fr)`, height: 6, borderRadius: 2, overflow: 'hidden' }}>
              {Array.from({ length: count }, (_, expert) => <span key={expert} data-expert={expert} data-held={expert >= h.begin && expert < h.end}
                style={{ background: expert >= h.begin && expert < h.end ? 'var(--pink)' : 'var(--surface-2)' }} />)}
            </div> : <span className="small muted">{t('ns.expertTopologyUnknown')}</span>}
            <span className="chip" style={{ fontSize: 10 }}>{t(`ns.expertLayerState.${expertLayerState(slot)}`)}</span>
            {slot.phase === 'downloading' && <ShardDownloadBar download={slot.download ?? null} />}
          </div>
        })}
      </div>
    </div>
  })}</>
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
      <ExpertLayerMap assigned={assigned} />
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
