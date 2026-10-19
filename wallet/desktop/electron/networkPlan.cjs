'use strict'
// Network path + expert layer plan shown when this PC joins as a node.
//
// What it measures: the round trip from this PC straight to the backbone host
// (GB10-1), not the public Cloudflare gateway. Expert calls are on the token's
// critical path once per remote layer, and on 2026-10-19 the two paths measured
// 21 ms direct against 266 ms through the gateway relay for the same 1-token
// call — so the plan is computed for the direct path the ring is meant to use.
//
// How: TCP handshake time to the backbone's probe port (connect() returning is
// one SYN/SYN-ACK round trip). Nothing is sent, so no service is asked to do
// work and no credential is involved.
//
// The plan is arithmetic over that RTT, the worker's own measured compute time
// and the model's routing shape — an estimate, labelled as one, until the
// backbone dispatches remote experts and real per-token timings exist.
const net = require('node:net')
const { performance } = require('node:perf_hooks')

/** One TCP handshake round trip in ms, or throws. */
function connectOnce(host, port, timeoutMs) {
  return new Promise((resolve, reject) => {
    const started = performance.now()
    const socket = net.connect({ host, port })
    const timer = setTimeout(() => { socket.destroy(); reject(new Error('timeout')) }, timeoutMs)
    socket.once('connect', () => { clearTimeout(timer); const ms = performance.now() - started; socket.destroy(); resolve(ms) })
    socket.once('error', (e) => { clearTimeout(timer); socket.destroy(); reject(e) })
  })
}

async function probeRtt({ host, port, samples = 12, timeoutMs = 2000, connect = connectOnce } = {}) {
  const times = []
  let lastError = null
  for (let i = 0; i < samples; i++) {
    try { times.push(await connect(host, port, timeoutMs)) } catch (e) { lastError = e }
  }
  if (!times.length) throw new Error(`backbone unreachable (${lastError?.message || 'no answer'})`)
  // The first handshake pays DNS/ARP/route warm-up; drop it when there are spares.
  const kept = times.length > 3 ? times.slice(1) : times
  kept.sort((a, b) => a - b)
  const at = (q) => kept[Math.min(kept.length - 1, Math.floor(kept.length * q))]
  return { p50Ms: at(0.5), p90Ms: at(0.9), minMs: kept[0], samples: kept.length, failed: samples - times.length }
}

/** P(a token's top-k selection touches at least one of `held` experts out of `total`). */
function hitProbability(total, held, used) {
  if (!(total > 0) || !(held > 0) || !(used > 0)) return 0
  if (held >= total) return 1
  let miss = 1
  for (let i = 0; i < used; i++) miss *= Math.max(0, total - held - i) / (total - i)
  return 1 - miss
}

/**
 * Expected decode speed for 0..maxLayers remote layers on this PC.
 * perLayerMs = RTT + worker compute + transfer of one combined f16 row
 * (the v2f16 wire mode returns the router-weighted sum, n_embd × 2 bytes).
 */
function planLayers({ rttMs, computeMs = 2, baseTps, nExpert = 288, nUsed = 8, expertsPerLayer = 64,
  nEmbd = 4096, uploadBytesPerSecond = 5e6, maxLayers = 8, targetTps = 4, wire = 'v1' }) {
  const p = hitProbability(nExpert, expertsPerLayer, nUsed)
  // What this PC sends back per token that touches its range. v1 (what the
  // backbone speaks today) returns one f32 row per (token, expert) pair; v2f16
  // would return one combined f16 row.
  const pairsPerHit = p > 0 ? (nUsed * expertsPerLayer / nExpert) / p : 0
  const bytes = wire === 'v2f16' ? nEmbd * 2 : pairsPerHit * nEmbd * 4
  const transferMs = bytes / uploadBytesPerSecond * 1000
  const perLayerMs = rttMs + computeMs + transferMs
  const baseMs = 1000 / baseTps
  const rows = []
  for (let k = 0; k <= maxLayers; k++) {
    const tokenMs = baseMs + k * p * perLayerMs
    rows.push({ layers: k, tokenMs, tps: 1000 / tokenMs, meetsTarget: 1000 / tokenMs >= targetTps })
  }
  const fits = rows.filter((r) => r.layers > 0 && r.meetsTarget)
  return { perLayerMs, transferMs, computeMs, wire, hitProbability: p, baseTps, targetTps, rows,
    recommendedLayers: fits.length ? fits[fits.length - 1].layers : 0 }
}

/**
 * Measures every candidate backbone in parallel and plans each one; refresh()
 * is single-flight and never throws. `serving` says whether a backbone actually
 * runs on that host today — a candidate that is only reachable is shown as such
 * and is never auto-selected over one that serves.
 */
function createNetworkPlanner({ targets, model, probe = probeRtt, now = Date.now, selected = () => null } = {}) {
  const blank = (t) => ({ id: t.id, label: t.label, host: t.host, serving: !!t.serving, state: 'idle', rtt: null, plan: null, error: null })
  let state = { state: 'idle', targets: targets.map(blank), at: null }
  let running = null
  const best = (list) => {
    const ok = list.filter((t) => t.state === 'ready' && t.plan)
    const score = (t) => [t.serving ? 1 : 0, t.plan.recommendedLayers, -t.rtt.p50Ms]
    ok.sort((a, b) => { const x = score(a), y = score(b); for (let i = 0; i < x.length; i++) if (x[i] !== y[i]) return y[i] - x[i]; return 0 })
    return ok[0]?.id ?? null
  }
  const view = () => {
    const bestId = best(state.targets)
    const chosen = selected()
    const selectedId = state.targets.some((t) => t.id === chosen) ? chosen : bestId
    return { ...state, targets: state.targets.map((t) => ({ ...t })), bestId, selectedId }
  }
  return {
    status: view,
    refresh(inputs = {}, measured = {}) {
      if (running) return running
      state = { ...state, state: 'measuring', targets: state.targets.map((t) => ({ ...t, state: 'measuring', error: null })) }
      running = (async () => {
        const results = await Promise.all(targets.map(async (t) => {
          try {
            // A backbone this PC already relays to is planned from that relay's
            // own ping round trips; others from a TCP handshake (an estimate).
            const live = measured[t.id]
            const rtt = live && live.samples > 0
              ? { p50Ms: live.p50Ms, p90Ms: live.p90Ms, minMs: live.p50Ms, samples: live.samples, failed: 0, source: 'relay' }
              : { ...(await probe({ host: t.host, port: t.port })), source: 'tcp' }
            return { ...blank(t), state: 'ready', rtt, plan: planLayers({ ...model, ...inputs, rttMs: rtt.p50Ms }) }
          } catch (e) { return { ...blank(t), state: 'failed', error: e.message } }
        }))
        state = { state: results.some((r) => r.state === 'ready') ? 'ready' : 'failed', targets: results, at: now() }
        running = null
        return view()
      })()
      return running
    },
  }
}

module.exports = { probeRtt, hitProbability, planLayers, createNetworkPlanner, connectOnce }
