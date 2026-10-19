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
function render(status) {
  let calls = 0
  const module = { exports: {} }
  const dependencies = {
    react: { ...React, useState: initial => [calls++ === 0 ? status : initial, () => {}], useRef: () => ({}), useEffect() {} },
    '../state': { useWallet: () => ({ os: 'macos', arch: 'arm64' }) },
    '../i18n': { useI18n: () => ({ t: key => key }) },
    '../components': { Card: ({ children }) => React.createElement('section', null, children), MetricRow: () => null, CopyButton: () => null },
    '../qr': { QR: () => null },
    './expertLayers': { ExpertLayersCard: () => React.createElement('div', null, 'EXPERT_CARD') },
    './expertMeasure': {}, '../services': {}, '../api': { isElectron: false },
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
