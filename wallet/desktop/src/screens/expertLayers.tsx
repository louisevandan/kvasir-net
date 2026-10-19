import { Card, MetricRow } from '../components'
import { useI18n } from '../i18n'
import type { NodeStatus } from '../api'

type Slot = NonNullable<NodeStatus['expertWork']>['slots'][number]
export function expertLayerState(slot: Slot) {
  if (slot.phase === 'downloading') return 'downloading'
  if (slot.phase === 'serving') return slot.relay.connected ? 'serving' : 'relayDown'
  if (slot.phase === 'starting' && slot.held?.shardFile) return 'cached'
  return slot.phase === 'failed' ? 'failed' : 'idle'
}

export function ExpertLayersCard({ slots = [] }: { slots?: Slot[] }) {
  const { t } = useI18n()
  const assigned = slots.filter(slot => slot.held)
  const models = [...new Set(assigned.map(slot => slot.held!.model))]
  return <Card>
    <div className="label">{t('ns.expertLayersTitle')}</div>
    {!assigned.length && <div className="small muted">{t('ns.expertWorkNone')}</div>}
    {models.map(model => {
      const held = assigned.filter(slot => slot.held!.model === model)
      // Assignment/GGUF metadata only: unknown topology is not guessed from the
      // model's display name. Bounds keep malformed metadata from huge renders.
      const topology = held.find(slot => slot.held!.nLayer)?.held
      const layers = topology?.nLayer && topology.nLayer > 0 && topology.nLayer <= 512 ? topology.nLayer : null
      const dense = topology?.denseLayers
      return <div key={model} style={{ marginTop: 10 }}>
        <strong>{model}</strong>
        <div className="small muted">{t('ns.expertLayersLegend')}</div>
        {layers ? <div style={{ display: 'grid', gridTemplateColumns: 'repeat(9, 1fr)', gap: 3, marginTop: 6 }}>
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
        {held.map(slot => {
          const h = slot.held!, work = slot.work
          const count = h.nExpert && h.nExpert > 0 && h.nExpert <= 4096 ? h.nExpert : null
          const points = work.recentRows ?? []
          const max = Math.max(1, ...points)
          return <div key={slot.workerId} style={{ borderTop: '1px solid var(--line)', paddingTop: 8, marginTop: 8 }}>
            <div className="spread small"><span>{t('ns.expertWorkShard', model, h.layer, h.begin, h.end)}</span>
              <span className="chip">{t(`ns.expertLayerState.${expertLayerState(slot)}`)}</span></div>
            {count ? <>
              <MetricRow label={t('ns.expertHeldRange')} value={(h.end - h.begin) / count} valueText={`${h.begin}–${h.end} / ${count}`} color="var(--pink)" />
              <div aria-label={t('ns.expertHeldRange')} style={{ display: 'grid', gridTemplateColumns: 'repeat(36, 1fr)', gap: 1 }}>
                {Array.from({ length: count }, (_, expert) => <span key={expert} data-expert={expert} data-held={expert >= h.begin && expert < h.end}
                  style={{ height: 5, background: expert >= h.begin && expert < h.end ? 'var(--pink)' : 'var(--surface-2)' }} />)}
              </div>
            </> : <div className="small muted">{t('ns.expertTopologyUnknown')}</div>}
            <div className="small mono muted" style={{ overflowWrap: 'anywhere' }}>{t('ns.expertShardFile', h.shardFile ?? '—', slot.port ?? '—')}</div>
            {slot.relay.lastClose && !slot.relay.connected && <div className="small muted">{t('ns.expertRelayClose', slot.relay.lastClose.code)}</div>}
            <div className="small">{t('ns.expertWorkRates', work.rowsPerSecond.toFixed(1), work.callsPerSecond.toFixed(2))}</div>
            <div className="small muted">{t('ns.expertWorkLatency', work.p50Ms?.toFixed(2) ?? '—', work.p95Ms?.toFixed(2) ?? '—')}</div>
            <div className="small muted">{t('ns.expertWorkWindow', (work.windowMs / 1000).toFixed(1), work.calls, work.rows)}</div>
            <div className="small muted">{t('ns.liveRingLastWork', work.lastWorkAt ? new Date(work.lastWorkAt).toLocaleString() : t('ns.liveRingUnknown'))}</div>
            <svg role="img" aria-label={t('ns.expertRecentActivity')} viewBox="0 0 120 24" style={{ width: '100%', height: 32 }}>
              <polyline fill="none" stroke="var(--good)" strokeWidth="1.5" points={points.map((n, i) => `${i * 120 / Math.max(1, points.length - 1)},${23 - n / max * 22}`).join(' ')} />
            </svg>
            <div className="small muted">{t('ns.expertRecentActivity')}</div>
          </div>
        })}
      </div>
    })}
  </Card>
}
