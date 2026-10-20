'use strict'
const { test } = require('node:test')
const assert = require('node:assert/strict')
const net = require('node:net')
const path = require('node:path')
const vm = require('node:vm')
const React = require('react')
const { renderToStaticMarkup } = require('react-dom/server')
const { buildSync } = require('esbuild')
const { probeRtt, hitProbability, planLayers, createNetworkPlanner } = require('./networkPlan.cjs')
const C = require('./constants.cjs')

const code = buildSync({ stdin: { contents: "export { NetworkPlanCard, SlowNetworkDialog, networkTooSlow, chosenBackbone } from './src/screens/networkPlan'; export { I18nProvider } from './src/i18n'", resolveDir: path.join(__dirname, '..'), loader: 'tsx' }, bundle: true, write: false, platform: 'node', format: 'cjs', jsx: 'automatic', external: ['react', 'react/jsx-runtime'] }).outputFiles[0].text
const mod = { exports: {} }
vm.runInNewContext(code, { module: mod, exports: mod.exports, require, Error })
const { NetworkPlanCard, SlowNetworkDialog, networkTooSlow, chosenBackbone, I18nProvider } = mod.exports
const render = (component, props, lang = 'en') => renderToStaticMarkup(React.createElement(I18nProvider, { initial: lang }, React.createElement(component, props)))

test('hit probability matches the hypergeometric miss for top-8 of 288 with 64 held', () => {
  assert.ok(Math.abs(hitProbability(288, 64, 8) - 0.8696) < 0.001)
  assert.equal(hitProbability(288, 288, 8), 1)
  assert.equal(hitProbability(288, 0, 8), 0)
})

test('plan: per-layer cost, rows and the largest layer count that still meets the target', () => {
  const plan = planLayers({ rttMs: 14, computeMs: 2, baseTps: 6.7, nExpert: 288, nUsed: 8, expertsPerLayer: 64, nEmbd: 4096, uploadBytesPerSecond: 5e6, maxLayers: 8, targetTps: 5, wire: 'v2f16' })
  assert.ok(Math.abs(plan.transferMs - 1.6384) < 1e-6)
  assert.ok(Math.abs(plan.perLayerMs - 17.6384) < 1e-6)
  assert.equal(plan.rows.length, 9)
  assert.ok(Math.abs(plan.rows[0].tps - 6.7) < 1e-9)
  assert.ok(plan.rows.every((r, i) => i === 0 || r.tps < plan.rows[i - 1].tps))
  assert.equal(plan.recommendedLayers, plan.rows.filter((r) => r.layers > 0 && r.tps >= 5).at(-1).layers)
  // v1 (today's wire): one f32 row per pair, ~2.04 pairs per token that touches 64/288 experts.
  const v1 = planLayers({ rttMs: 14, computeMs: 2, baseTps: 6.7, uploadBytesPerSecond: 5e6 })
  assert.equal(v1.wire, 'v1')
  assert.ok(Math.abs(v1.transferMs - (8 * 64 / 288) / hitProbability(288, 64, 8) * 4096 * 4 / 5e6 * 1000) < 1e-9)
  assert.ok(v1.perLayerMs > plan.perLayerMs)
  // The relay path measured 2026-10-19 (266 ms) cannot hold even one layer at 5 tok/s.
  assert.equal(planLayers({ rttMs: 266, baseTps: 6.7, targetTps: 5 }).recommendedLayers, 0)
})

test('probe measures real TCP handshakes, drops the warm-up sample and reports failures', async () => {
  const server = net.createServer((s) => s.destroy())
  await new Promise((r) => server.listen(0, '127.0.0.1', r))
  const port = server.address().port
  const rtt = await probeRtt({ host: '127.0.0.1', port, samples: 6 })
  assert.equal(rtt.samples, 5); assert.equal(rtt.failed, 0)
  assert.ok(rtt.minMs <= rtt.p50Ms && rtt.p50Ms <= rtt.p90Ms)
  server.close()
  await assert.rejects(probeRtt({ host: '127.0.0.1', port, samples: 2, timeoutMs: 200 }), /backbone unreachable/)
})

test('planner measures every backbone in parallel, prefers one that serves, honours the operator pick', async () => {
  const rtts = { a: 30, b: 8, c: null }
  let selected = null
  const planner = createNetworkPlanner({
    targets: [{ id: 'a', label: 'A', host: 'a', port: 1, serving: true }, { id: 'b', label: 'B', host: 'b', port: 1, serving: false }, { id: 'c', label: 'C', host: 'c', port: 1, serving: false }],
    model: C.expertPlanModel, selected: () => selected, now: () => 42,
    probe: async ({ host }) => { if (rtts[host] == null) throw new Error('no answer'); return { p50Ms: rtts[host], p90Ms: rtts[host] + 1, minMs: rtts[host], samples: 5, failed: 0 } },
  })
  assert.equal(planner.status().state, 'idle')
  const first = planner.refresh(), again = planner.refresh()
  assert.equal(first, again) // single flight
  const s = await first
  assert.equal(s.state, 'ready'); assert.equal(s.at, 42)
  assert.equal(s.bestId, 'a')       // serving beats a faster candidate with no backbone
  assert.equal(s.selectedId, 'a')
  assert.equal(s.targets.find((t) => t.id === 'c').state, 'failed')
  assert.ok(s.targets.every((t) => t.state !== 'ready' || t.rtt.source === 'tcp'))
  // A backbone this PC already relays to is planned from the relay's own pings.
  const live = await planner.refresh({}, { a: { p50Ms: 28, p90Ms: 60, samples: 9 } })
  const a = live.targets.find((t) => t.id === 'a')
  assert.equal(a.rtt.source, 'relay'); assert.equal(a.rtt.p50Ms, 28); assert.equal(a.rtt.samples, 9)
  assert.equal(live.targets.find((t) => t.id === 'b').rtt.source, 'tcp')
  selected = 'b'
  assert.equal(planner.status().selectedId, 'b')
  selected = 'unknown'
  assert.equal(planner.status().selectedId, 'a')
})

test('each backbone is planned from its own ring speed, and the shipped MI250-02 serves via GB10-1', async () => {
  const planner = createNetworkPlanner({
    targets: [{ id: 'slow', label: 'S', host: 's', port: 1, serving: true, baseTps: 6.7 }, { id: 'fast', label: 'F', host: 'f', port: 1, serving: true, baseTps: 16, via: 'slow' }],
    model: C.expertPlanModel,
    probe: async () => ({ p50Ms: 20, p90Ms: 21, minMs: 20, samples: 5, failed: 0 }),
  })
  const s = await planner.refresh()
  const slow = s.targets.find((t) => t.id === 'slow'), fast = s.targets.find((t) => t.id === 'fast')
  assert.equal(slow.plan.baseTps, 6.7); assert.equal(fast.plan.baseTps, 16)
  assert.equal(fast.via, 'slow')
  assert.ok(fast.plan.rows[0].tps > slow.plan.rows[0].tps)
  const mi = C.backbones.find((b) => b.id === 'mi250-02')
  assert.ok(mi.serving && mi.bridgeId === 'mi250-02' && mi.via === 'gb10-1')
  assert.ok(!C.backbones.find((b) => b.id === 'mi250-01').serving)
})

const status = (rttA, extra = {}) => ({
  state: 'ready', at: 1, bestId: 'gb10-1', selectedId: null, warnTps: 5,
  targets: [
    { id: 'gb10-1', label: 'GB10-1', host: '1.220.225.155', serving: true, state: 'ready', error: null,
      rtt: { p50Ms: rttA, p90Ms: rttA + 1, minMs: rttA, samples: 11, failed: 0 }, plan: planLayers({ ...C.expertPlanModel, rttMs: rttA }) },
    { id: 'mi250-02', label: 'MI250-02', host: '1.214.116.122', serving: false, state: 'ready', error: null,
      rtt: { p50Ms: 10, p90Ms: 11, minMs: 10, samples: 11, failed: 0 }, plan: planLayers({ ...C.expertPlanModel, rttMs: 10 }) },
  ], ...extra })

test('card lists every backbone, marks best/chosen, shows the chosen plan and the direct path only', () => {
  const html = render(NetworkPlanCard, { status: status(14), onMeasure() {}, onSelect() {} })
  assert.equal((html.match(/data-backbone="/g) || []).length, 2)
  assert.match(html, /data-backbone="gb10-1" data-chosen="true"/)
  assert.match(html, /GB10-1 \(1\.220\.225\.155\), direct/)
  assert.match(html, /candidate · no backbone/)
  assert.match(html, /data-recommended="true"/)
  assert.doesNotMatch(html, /gate\.kvasir-ai\.net|Cloudflare/)
  assert.match(html, /data-selected="true"[^>]*style="[^"]*color:var\(--good\)/)
  assert.match(html, /estimate: TCP handshake/)
  const relayed = status(28); relayed.targets[0].rtt.source = 'relay'
  assert.match(render(NetworkPlanCard, { status: relayed, onSelect() {} }), /data-rtt-source="relay"[\s\S]*measured: relay round trip/)
  const ko = render(NetworkPlanCard, { status: status(14, { selectedId: 'mi250-02' }) }, 'ko')
  assert.match(ko, /data-backbone="mi250-02" data-chosen="true"/); assert.match(ko, /아직 백본이 없습니다/)
  assert.match(render(NetworkPlanCard, { status: null }), /When this PC joins/)
})

test('slow-network dialog opens only when one layer on the chosen backbone is at or below 5 tok/s', () => {
  assert.equal(networkTooSlow(status(14)), false)
  assert.equal(networkTooSlow(status(266)), true)
  assert.equal(networkTooSlow({ ...status(266), state: 'measuring' }), false)
  assert.equal(chosenBackbone(status(266, { selectedId: 'mi250-02' })).id, 'mi250-02')
  assert.equal(networkTooSlow(status(266, { selectedId: 'mi250-02' })), false)
  const dialog = render(SlowNetworkDialog, { status: status(266), onClose() {}, onMeasure() {} }, 'ko')
  assert.match(dialog, /role="dialog"/); assert.match(dialog, /네트워크가 느립니다/); assert.match(dialog, /빠른 네트워크/)
})
