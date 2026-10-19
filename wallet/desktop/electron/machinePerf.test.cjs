'use strict'
const { test } = require('node:test')
const assert = require('node:assert/strict')
const path = require('node:path')
const vm = require('node:vm')
const React = require('react')
const { renderToStaticMarkup } = require('react-dom/server')
const { buildSync } = require('esbuild')
const { planLayers } = require('./networkPlan.cjs')
const C = require('./constants.cjs')

const code = buildSync({ stdin: { contents: "export { MachinePerfView, machinePerf, tierFor, TIER_BANDS } from './src/screens/machinePerfView'; export { I18nProvider } from './src/i18n'", resolveDir: path.join(__dirname, '..'), loader: 'tsx' }, bundle: true, write: false, platform: 'node', format: 'cjs', jsx: 'automatic', external: ['react', 'react/jsx-runtime'] }).outputFiles[0].text
const mod = { exports: {} }
vm.runInNewContext(code, { module: mod, exports: mod.exports, require, Error, console, setTimeout, clearTimeout, setInterval, clearInterval, URL, URLSearchParams, TextEncoder, TextDecoder, crypto: globalThis.crypto, Buffer, process })
const { MachinePerfView, machinePerf, tierFor, TIER_BANDS, I18nProvider } = mod.exports
const render = (props, lang = 'en') => renderToStaticMarkup(React.createElement(I18nProvider, { initial: lang }, React.createElement(MachinePerfView, props)))

const plan = {
  state: 'ready', at: 1, bestId: 'gb10-1', selectedId: 'gb10-1', warnTps: 5,
  targets: [{ id: 'gb10-1', label: 'GB10-1', host: '1.220.225.155', serving: true, state: 'ready', error: null,
    rtt: { p50Ms: 28, p90Ms: 60, minMs: 28, samples: 9, failed: 0, source: 'relay' }, plan: planLayers({ ...C.expertPlanModel, rttMs: 28 }) }],
}
const status = {
  nodes: [], log: [], running: true, vramBudgetBytes: 9 * 2 ** 30, maxExperts: 512,
  measured: null, localBenchmark: null,
  expertWork: { measurement: { running: false, total: { slots: 8, rowsPerSecond: 4200, p95Ms: 3.1 } },
    slots: [{ workerId: 'a', phase: 'serving', held: { layer: 14 }, work: { rowsPerSecond: 12.5 } }, { workerId: 'b', phase: 'serving', held: { layer: 3 }, work: { rowsPerSecond: 0 } }] },
}

test('tier bands follow the reward table', () => {
  assert.equal(TIER_BANDS.map((b) => b.tier).join(','), 'C,B,A,S')
  assert.equal(tierFor(95), 'S'); assert.equal(tierFor(60), 'A'); assert.equal(tierFor(30), 'B'); assert.equal(tierFor(6.2), 'C')
})

test('expected ring speed uses the chosen backbone at the recommended layers (at least one)', () => {
  const p = machinePerf(status, plan)
  assert.equal(p.backbone.id, 'gb10-1')
  assert.equal(p.layers, Math.max(1, plan.targets[0].plan.recommendedLayers))
  assert.equal(p.expectedTps, plan.targets[0].plan.rows.find((r) => r.layers === p.layers).tps)
  assert.equal(p.liveRows, 12.5); assert.equal(p.heldSlots, 2); assert.equal(p.maxExperts, 512)
})

test('card shows the band bar, the settings it used, and the measure control', () => {
  const html = render({ status, plan, measure: { state: { running: false, error: null }, ready: true }, onMeasure() {}, onOpenSettings() {} })
  assert.equal((html.match(/data-band="/g) || []).length, 4)
  assert.match(html, /data-mark="expected"/); assert.doesNotMatch(html, /data-mark="local"/)
  assert.match(html, /Expected ring speed/); assert.match(html, /GB10-1/); assert.match(html, /measured: relay round trip/)
  assert.match(html, /8 slots\): 4200\.0 rows\/s/); assert.match(html, /9\.0 GiB/); assert.match(html, /Open node settings/)
  const withLocal = render({ status: { ...status, localBenchmark: { tps: 64.2 } }, plan })
  assert.match(withLocal, /data-mark="local"/); assert.match(withLocal, /band A/)
  const ko = render({ status, plan: null }, 'ko')
  assert.match(ko, /이 기기 성능 측정/); assert.match(ko, /아직 계획이 없습니다/)
  assert.match(render({ status: null, plan }), /Checking node status/)
})
