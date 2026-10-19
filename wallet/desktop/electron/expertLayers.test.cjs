'use strict'
const { test } = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')
const React = require('react')
const { renderToStaticMarkup } = require('react-dom/server')
const { buildSync } = require('esbuild')
const code = buildSync({ stdin: { contents: "export { ExpertLayersCard, expertLayerState } from './src/screens/expertLayers'; export { I18nProvider } from './src/i18n'", resolveDir: path.join(__dirname, '..'), loader: 'tsx' }, bundle: true, write: false, platform: 'node', format: 'cjs', jsx: 'automatic', external: ['react', 'react/jsx-runtime'] }).outputFiles[0].text
const moduleObject = { exports: {} }
vm.runInNewContext(code, { module: moduleObject, exports: moduleObject.exports, require })
const { ExpertLayersCard, expertLayerState, I18nProvider } = moduleObject.exports
const fixture = {
  workerId: 'fixture-only', phase: 'serving', port: 59019,
  held: { model: 'step-3.7-flash', layer: 3, begin: 0, end: 64, nLayer: 45, nExpert: 288, denseLayers: 3, shardFile: 'L3_e000-064.gguf' },
  relay: { connected: false, lastClose: { code: 1011, reason: 'upstream unavailable', at: 100000 } },
  work: { state: 'live', calls: 12, rows: 64, windowMs: 60000, windowLimitMs: 60000, callsPerSecond: 0.2, rowsPerSecond: 1.07, p50Ms: 8, p95Ms: 14, lastWorkAt: 100000, recentRows: [0, 1, 4, 0, 8, 4, 3, 10, 2, 1, 0, 6] },
  localBenchmark: null,
}
const render = (slots, lang = 'en') => renderToStaticMarkup(React.createElement(I18nProvider, { initial: lang }, React.createElement(ExpertLayersCard, { slots })))
test('expert layers renders 45 layers, dense prefix, held range, relay state and measured activity from fixture', () => {
  const html = render([fixture])
  assert.equal((html.match(/data-layer=/g) || []).length, 45)
  assert.equal((html.match(/data-dense="true"/g) || []).length, 3)
  assert.equal((html.match(/data-expert=/g) || []).length, 288)
  assert.equal((html.match(/data-held="true"/g) || []).length, 65) // layer + 64 experts
  for (const value of ['step-3.7-flash', 'L3_e000-064.gguf', '59019', 'relay down', '1011', '1.1 rows/s', 'p95 14.00 ms', 'polyline']) assert.ok(html.includes(value), value)
  const ko = render([fixture], 'ko')
  assert.ok(ko.includes('전문가 레이어')); assert.ok(ko.includes('릴레이 끊김')); assert.ok(!ko.includes('ns.'))
  if (process.env.EXPERT_LAYERS_FIXTURE_HTML) {
    const css = fs.readFileSync(path.join(__dirname, '../src/theme.css'), 'utf8')
    fs.writeFileSync(process.env.EXPERT_LAYERS_FIXTURE_HTML, `<!doctype html><meta charset="utf-8"><style>${css}</style><main style="max-width:660px;margin:24px auto"><p>FIXTURE — synthetic display data, not live measurements</p>${html}</main>`)
  }
})
test('layer card states are driven by cached assignment/relay state and missing topology is not guessed', () => {
  assert.equal(expertLayerState({ ...fixture, phase: 'downloading' }), 'downloading')
  assert.equal(expertLayerState({ ...fixture, phase: 'starting' }), 'cached')
  assert.equal(expertLayerState({ ...fixture, relay: { connected: true } }), 'serving')
  assert.equal(expertLayerState({ ...fixture, phase: 'failed' }), 'failed')
  assert.ok(render([]).includes('No assigned expert shard'))
  const html = render([{ ...fixture, held: { ...fixture.held, nLayer: null, nExpert: null, denseLayers: null } }])
  assert.ok(html.includes('Complete model topology unavailable'))
  assert.ok(!html.includes('data-layer=')); assert.ok(!html.includes('data-expert='))
})
