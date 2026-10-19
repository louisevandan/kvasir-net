'use strict'
const { test } = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')
const React = require('react')
const { renderToStaticMarkup } = require('react-dom/server')
const { transformSync } = require('esbuild')
const source = fs.readFileSync(path.join(__dirname, '../src/screens/nodeSettings.tsx'), 'utf8')
const code = transformSync(source, { loader: 'tsx', jsx: 'automatic', format: 'cjs' }).code
function render(status, { electron = false, address } = {}) {
  let calls = 0
  const module = { exports: {} }
  const dependencies = {
    react: { ...React, useState: initial => [calls++ === 0 ? status : initial, () => {}], useRef: () => ({}), useEffect() {} },
    '../state': { useWallet: () => ({ os: 'macos', arch: 'arm64', address }) },
    '../i18n': { useI18n: () => ({ t: key => key }) },
    '../components': { Card: ({ children }) => React.createElement('section', null, children), MetricRow: () => null, CopyButton: () => null },
    '../qr': { QR: () => null },
    './expertLayers': { ExpertLayersCard: () => React.createElement('div', null, 'EXPERT_CARD') },
    './expertMeasure': {}, '../services': {}, '../api': { isElectron: electron, api: {} },
    './networkPlan': { NetworkPlanCard: () => null, SlowNetworkDialog: () => null, useSlowNetworkDialog: () => ({ open: false, dismiss() {} }) },
  }
  vm.runInNewContext(code, { module, exports: module.exports, require: key => dependencies[key] || require(key) })
  return renderToStaticMarkup(React.createElement(module.exports.NodeSettingsScreen))
}
test('actual node settings hides live-ring card without owned pipeline stages', () => {
  const status = { nodes: [], log: [], running: true, snapshotAt: null, expertWork: { slots: [{ phase: 'serving' }] } }
  assert.doesNotMatch(render(status), /ns.liveRingTitle/)
  assert.match(render(status), /EXPERT_CARD/)
  assert.doesNotMatch(render(null), /ns.liveRingTitle/)
  assert.match(render({ ...status, nodes: [{ nodeId: 'stage', state: 'running' }] }), /ns.liveRingTitle/)
})
test('lightweight measurement progress IPC does not execute nodeStatus or hardware probes', () => {
  const main = fs.readFileSync(path.join(__dirname, 'main.cjs'), 'utf8')
  const line = main.split('\n').find(line => line.includes("ipcMain.handle('node:benchmarkProgress'"))
  let handler, count = 0
  vm.runInNewContext(line, { ipcMain: { handle: (_name, fn) => { handler = fn } }, expertPool: { measurementStatus: () => { count++; return { running: true, layer: 8, index: 2, count: 7 } } } })
  assert.equal(handler().layer, 8); assert.equal(count, 1)
  const preload = fs.readFileSync(path.join(__dirname, 'preload.cjs'), 'utf8')
  assert.match(preload, /benchmarkProgress: \(\) => ipcRenderer.invoke\('node:benchmarkProgress'\)/)
})

test('connect card follows the main process status, so it survives leaving the screen', () => {
  const base = { nodes: [], log: [], snapshotAt: null, expertWork: { slots: [] } }
  const opts = { electron: true, address: 'JcVYk4PpWallet' }
  const connected = render({ ...base, running: true, participation: { running: true, phase: 'serving' } }, opts)
  assert.match(connected, /data-connected="true"/); assert.match(connected, /dev\.connected/)
  assert.match(connected, /data-action="disconnect"[^>]*>(<span[^>]*><\/span>)?dev\.disconnect</)
  assert.doesNotMatch(connected, /<button[^>]*>(<span[^>]*><\/span>)?dev\.connectThis/)
  const participating = render({ ...base, running: false, participation: { running: true, phase: 'volunteering' } }, opts)
  assert.match(participating, /data-connected="true"/)
  const stopped = render({ ...base, running: false, participation: { running: false, phase: 'stopped' } }, opts)
  assert.match(stopped, /data-connected="false"/); assert.match(stopped, />dev\.connectThis<\/button>/)
  assert.doesNotMatch(stopped, /data-action="disconnect"/)
  assert.match(render(null, opts), /dev\.connectChecking/)
})

test('machine name is printed once on unified Apple silicon; benchmark, gauge and log live in the right column', () => {
  const capability = { cpu: { brand: 'Apple M4 Pro', cores: 12 }, gpus: [{ name: 'Apple M4 Pro', memoryBytes: 24 * 2 ** 30 }], ramBytes: 24 * 2 ** 30, backend: 'metal' }
  const html = render({ nodes: [], log: ['line one'], running: true, snapshotAt: null, capability, expertWork: { slots: [] }, participation: { running: true, phase: 'serving' } }, { electron: true, address: 'JcVYk4PpWallet' })
  assert.equal((html.match(/Apple M4 Pro/g) || []).length, 1)
  assert.doesNotMatch(html, /data-cpu-brand/)
  const discrete = render({ nodes: [], log: [], running: false, snapshotAt: null, capability: { ...capability, cpu: { brand: 'AMD Ryzen 9', cores: 16 }, gpus: [{ name: 'RTX 4090', memoryBytes: 24 * 2 ** 30 }] }, expertWork: { slots: [] } }, { electron: true, address: 'x' })
  assert.match(discrete, /data-cpu-brand[^>]*>AMD Ryzen 9</)
  const right = html.indexOf('dev.otherHint')
  assert.ok(right > 0 && html.indexOf('ns.localDecodeTitle') > right && html.indexOf('ns.agentRunning') > right && html.indexOf('line one') > right)
})
