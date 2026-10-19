'use strict'
const { test } = require('node:test')
const assert = require('node:assert/strict')
const path = require('node:path')
const vm = require('node:vm')
const React = require('react')
const { renderToStaticMarkup } = require('react-dom/server')
const { buildSync } = require('esbuild')
const code = buildSync({ stdin: { contents: "export { ExpertLayersCard, expertLayerState, expertBudget } from './src/screens/expertLayers'; export { ExpertMeasureControl, runMeasurement } from './src/screens/expertMeasure'; export { I18nProvider } from './src/i18n'", resolveDir: path.join(__dirname, '..'), loader: 'tsx' }, bundle: true, write: false, platform: 'node', format: 'cjs', jsx: 'automatic', external: ['react', 'react/jsx-runtime'] }).outputFiles[0].text
const moduleObject = { exports: {} }
vm.runInNewContext(code, { module: moduleObject, exports: moduleObject.exports, require, Error })
const { ExpertLayersCard, expertLayerState, expertBudget, ExpertMeasureControl, runMeasurement, I18nProvider } = moduleObject.exports
const fixture = {
  workerId: 'fixture-only', phase: 'serving', port: 59019,
  held: { model: 'step-3.7-flash', layer: 6, begin: 0, end: 64, nLayer: 45, nExpert: 288, denseLayers: 3, shardFile: 'L6_e000-064.gguf' },
  relay: { connected: false, lastClose: { code: 1011, reason: 'upstream unavailable', at: 100000 } },
  work: { calls: 0, rows: 0, windowMs: 32700, rowsPerSecond: 0, lastWorkAt: null }, localBenchmark: null,
}
const renderComponent = (component, props, lang = 'en') => renderToStaticMarkup(React.createElement(I18nProvider, { initial: lang }, React.createElement(component, props)))
const render = (slots, lang = 'en', status) => renderComponent(ExpertLayersCard, { slots, status }, lang)
test('seven layers collapse to one summary, default-closed table and one factual relay note', () => {
  const slots = Array.from({ length: 7 }, (_, i) => ({ ...fixture, workerId: `fixture-${i}`, held: { ...fixture.held, layer: 6 + i } }))
  const html = render(slots)
  assert.match(html, /7 layers held · 7 serving · 7 connecting\/retrying · live total 0.0 rows\/s/)
  assert.equal((html.match(/role="note"/g) || []).length, 1)
  assert.match(html, /cannot reach a coordinator/)
  assert.match(html, /<details/); assert.doesNotMatch(html, /<details[^>]*open/)
  assert.equal((html.match(/<tbody><tr|<\/tr><tr/g) || []).length, 7)
  assert.match(html, /Live rows\/s/); assert.match(html, /Bench p50\/p95 ms/)
  assert.match(html, /L12/); assert.match(html, /Worker serving · relay down/)
  const ko = render(slots, 'ko')
  assert.match(ko, /전문가 레이어/); assert.match(ko, /릴레이 끊김/); assert.doesNotMatch(ko, /ns\./)
  assert.doesNotMatch(render([{ ...fixture, relay: { connected: true, lastClose: fixture.relay.lastClose } }]), /role="note"/)
  assert.doesNotMatch(render([{ ...fixture, phase: 'starting' }]), /role="note"/)
})
test('starting and downloading are explicit rather than unavailable, per-layer benchmark is visible', () => {
  assert.equal(expertLayerState({ ...fixture, phase: 'starting', held: { ...fixture.held, shardFile: null } }), 'starting')
  assert.equal(expertLayerState({ ...fixture, phase: 'starting' }), 'cached')
  assert.equal(expertLayerState({ ...fixture, phase: 'downloading' }), 'downloading')
  assert.match(render([{ ...fixture, phase: 'downloading' }]), /Downloading/)
  const html = render([{ ...fixture, localBenchmark: { rowsPerSecond: 223, p50Ms: 2.56, p95Ms: 11.43, calls: 30, at: 100000, batchRows: 1, expertsPerRow: 8 } }])
  assert.match(html, /223.0/); assert.match(html, /2.56 \/ 11.43/); assert.match(html, /30 measured calls/)
  assert.match(render([]), /No assigned expert shard/)
})
test('budget diagnostic includes fixed cost per slot plus headroom once, flags but never allocates', () => {
  const slots = Array.from({ length: 8 }, () => fixture)
  const expertMemoryModel = { fixedBytes: 64 * 2 ** 20, scratchBytes: 320 * 2 ** 20, headroomBytes: 256 * 2 ** 20, residentBytesPerExpert: 9499993 }
  const status = { expertMemoryModel, maxExperts: 512, vramBudgetBytes: 8 * 2 ** 30 }
  const budget = expertBudget(slots, status)
  assert.equal(budget.experts, 512)
  assert.equal(budget.bytes, 8 * (384 * 2 ** 20 + 64 * 9499993) + 256 * 2 ** 20)
  assert.equal(budget.state, 'within')
  assert.equal(expertBudget(slots, { ...status, maxExperts: 511 }).state, 'over')
  assert.equal(expertBudget(slots, { ...status, vramBudgetBytes: budget.bytes - 1 }).state, 'over')
  assert.equal(expertBudget(slots).state, 'unknown')
  assert.match(render(slots, 'en', { ...status, maxExperts: 511 }), /Budget exceeded/)
})
test('renderer measurement state idle → measuring → done/error enables controls and shows progress', async () => {
  for (const outcome of ['done', 'error', 'throw', 'reassignment']) {
    let state = { running: false, error: null }, resolve
    const renderState = progress => renderComponent(ExpertMeasureControl, { state, progress, onMeasure() {} }, 'ko')
    assert.doesNotMatch(renderState(), /disabled=""/)
    const pending = runMeasurement({ benchmark: () => new Promise(done => { resolve = done }),
      refresh: async () => { if (outcome === 'throw') throw Error('status failed') }, setState: s => { state = s }, failed: 'failed', noTarget: 'no target' })
    const progress = { running: true, index: 2, count: 7, layer: 8 }
    const html = renderState(progress)
    assert.match(html, /측정 중…/); assert.match(html, /disabled=""/); assert.match(html, /measure-spinner/)
    assert.match(html, /레이어 8 측정 중 \(2\/7\)/); assert.match(html, /<progress/)
    resolve(outcome === 'error' || outcome === 'reassignment' ? { ok: false, error: outcome } : { ok: true })
    await pending
    assert.equal(state.running, false)
    assert.doesNotMatch(renderState(), /disabled=""/)
    if (outcome !== 'done') assert.match(renderState(), /role="alert"/)
  }
})
