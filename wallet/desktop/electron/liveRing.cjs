'use strict'
// This is request throughput attributed to owned stages, never local decode or
// relay bandwidth. Older contribution feeds lack last-work/model metadata; do
// not upgrade a successful poll into evidence of fresh inference.
function liveRing({ agent, experts, contributions, source, now = Date.now(), error = null, httpStatus = null }) {
  const relayBytes = (experts?.slots || []).reduce((n, s) => n + (Number(s.relay?.bytes) || 0), 0)
  const base = { state: 'unavailable', source, metric: 'achieved request throughput',
    observedAt: now, lastWorkAt: null, window: 'bridge EWMA (0.7 previous + 0.3 request)',
    relayBytes, stages: [], reason: null, reasonCode: null, httpStatus }
  if (agent.snapshotAt == null) return { ...base, reasonCode: 'agentNotInspected', reason: 'Owned agent has not been inspected yet' }
  if (now - agent.snapshotAt > 15000) return { ...base, state: 'stale', reasonCode: 'agentStale', reason: 'Owned-agent inspection is stale' }
  if (!agent.running || agent.lastError) return { ...base, reasonCode: 'agentUnavailable', reason: 'Owned-agent inspection is currently unavailable' }
  if (!agent.nodes.length) return { ...base, state: experts?.servingSlots ? 'unavailable' : 'idle',
    reasonCode: experts?.servingSlots ? 'expertOnly' : 'noStage',
    reason: experts?.servingSlots ? 'No pipeline stage; expert workers expose relay activity, not token timing' : 'Idle — no stage on this agent' }
  if (error || !Array.isArray(contributions)) return { ...base, reasonCode: httpStatus ? 'feedHttp' : 'feedUnavailable', reason: error || 'Contribution feed unavailable' }
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
      reasonCode: !row ? 'noMatch' : !at || !model ? 'missingMetadata' : null,
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
    if (!res.ok) return liveRing({ agent, experts, source, error: `Contribution feed HTTP ${res.status}`, httpStatus: res.status })
    const data = await res.json()
    return liveRing({ agent, experts, source, contributions: data.contributions })
  } catch { return liveRing({ agent, experts, source, error: 'Contribution feed unavailable' }) }
}
// Poll-driven background cache: never await the network in nodeStatus. Keep at
// most one request in flight, including across source/agent changes. Old requests
// cannot populate a new identity's cache. Failed requests are throttled too.
function createLiveRingCache({ observe = observeLiveRing, now = Date.now, refreshMs = 10000 } = {}) {
  let entry = null
  let inFlight = false
  let lastStarted = -Infinity
  return function cachedLiveRing(agent, experts, baseUrl) {
    const at = now()
    const key = JSON.stringify([baseUrl, agent.address, agent.pid, agent.running,
      agent.nodes.map(n => [n.nodeId, n.generation]).sort()])
    if (entry?.key !== key) entry = { key, value: null }
    const current = entry
    let source = null
    try {
      const url = new URL('/api/contributions', baseUrl)
      if (['http:', 'https:'].includes(url.protocol) && !url.username && !url.password) source = url.href
    } catch { /* invalid sources remain unavailable */ }
    const fallback = liveRing({ agent, experts, source, now: at })
    if (fallback.reasonCode?.startsWith('agent') || !agent.nodes.length) {
      // A failed inspection must invalidate previously live work as well.
      entry = null
      return fallback
    }
    if (!inFlight && at - lastStarted >= refreshMs) {
      inFlight = true
      lastStarted = at
      Promise.resolve().then(() => observe(agent, experts, baseUrl)).then(value => {
        if (entry === current) current.value = value
      }, () => {
        if (entry === current) current.value = liveRing({ agent, experts, source, now: now() })
      }).finally(() => { inFlight = false })
    }
    if (!current.value) return fallback
    const value = current.value
    // Polling a cached result is not fresh work. Age it on every read without
    // changing observedAt, and update relay bytes independently of token timing.
    const stages = value.stages.map(s => s.lastWorkAt && at - s.lastWorkAt > 60000
      ? { ...s, state: 'stale', tps: null } : s)
    const state = stages.length ? stages.some(s => s.state === 'live') ? 'live'
      : stages.some(s => s.state === 'stale') ? 'stale'
      : stages.every(s => s.state === 'idle') ? 'idle' : 'unavailable' : value.state
    return { ...value, stages, state, relayBytes: fallback.relayBytes }
  }
}
module.exports = { liveRing, observeLiveRing, createLiveRingCache }
