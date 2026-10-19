'use strict'
// This is request throughput attributed to owned stages, never local decode or
// relay bandwidth. Older contribution feeds lack last-work/model metadata; do
// not upgrade a successful poll into evidence of fresh inference.
function liveRing({ agent, experts, contributions, source, now = Date.now(), error = null }) {
  const relayBytes = (experts?.slots || []).reduce((n, s) => n + (Number(s.relay?.bytes) || 0), 0)
  const base = { state: 'unavailable', source, metric: 'achieved request throughput',
    observedAt: now, lastWorkAt: null, window: 'bridge EWMA (0.7 previous + 0.3 request)',
    relayBytes, stages: [], reason: null }
  if (!agent.running || !agent.snapshotAt || now - agent.snapshotAt > 15000 || agent.lastError) {
    return { ...base, state: 'stale', reason: 'Owned-agent inspection is unavailable or stale' }
  }
  if (!agent.nodes.length) return { ...base, state: experts?.servingSlots ? 'unavailable' : 'idle',
    reason: experts?.servingSlots ? 'No pipeline stage; expert workers expose relay activity, not token timing' : 'Idle — no stage on this agent' }
  if (error || !Array.isArray(contributions)) return { ...base, reason: error || 'Contribution feed unavailable' }
  const stages = agent.nodes.map(node => {
    // Require both the stage ID and agent address; an owner-wallet match alone
    // can incorrectly attribute a different machine's serving ring to this Mac.
    const matches = contributions.filter(row => row.node_id === node.nodeId && row.agent === agent.address)
    const row = matches.length === 1 ? matches[0] : null
    const rate = Number.isFinite(row?.perf_tps) && row.perf_tps > 0 ? row.perf_tps : null
    const at = Number.isFinite(row?.last_work_at_ms) && row.last_work_at_ms > 0 && row.last_work_at_ms <= now
      ? row.last_work_at_ms : null
    const model = typeof row?.model === 'string' && row.model ? row.model : null
    const state = !row ? 'unavailable' : at === null || !model ? 'unavailable'
      : now - at > 60000 ? 'stale' : rate === null ? 'idle' : 'live'
    return { nodeId: node.nodeId, model, lastWorkAt: at, state,
      tps: state === 'live' ? rate : null,
      reason: !row ? 'No unique stage/agent contribution match' : !at || !model
        ? 'Feed lacks model or last-work time; polling is not proof of live work' : null }
  })
  const times = stages.map(s => s.lastWorkAt).filter(Number.isFinite)
  const state = stages.some(s => s.state === 'live') ? 'live' : stages.some(s => s.state === 'stale') ? 'stale'
    : stages.every(s => s.state === 'idle') ? 'idle' : 'unavailable'
  return { ...base, stages, state, lastWorkAt: times.length ? Math.max(...times) : null }
}
async function observeLiveRing(agent, experts, baseUrl, request = fetch) {
  let source = null
  try {
    const url = new URL('/api/contributions', baseUrl)
    if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password) throw new Error('Invalid contribution source')
    source = url.href
    if (!agent.running || !agent.snapshotAt || Date.now() - agent.snapshotAt > 15000 || agent.lastError || !agent.nodes.length) {
      return liveRing({ agent, experts, source })
    }
    // No credentials, API keys, registration, or follow-on redirect to another
    // service. Auth-only deployments intentionally surface unavailable.
    const res = await request(source, { redirect: 'error', credentials: 'omit', signal: AbortSignal.timeout(4000) })
    if (!res.ok) return liveRing({ agent, experts, source, error: `Contribution feed HTTP ${res.status}` })
    const data = await res.json()
    return liveRing({ agent, experts, source, contributions: data.contributions })
  } catch { return liveRing({ agent, experts, source, error: 'Contribution feed unavailable' }) }
}
module.exports = { liveRing, observeLiveRing }
