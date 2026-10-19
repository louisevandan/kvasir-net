'use strict'
const { test } = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')
const { transformSync } = require('esbuild')
const React = require('react')
const { renderToStaticMarkup } = require('react-dom/server')

// Compile the real catalog/provider; no browser, wallet, or service is opened.
const source = fs.readFileSync(path.join(__dirname, '../src/i18n.tsx'), 'utf8')
const compiled = transformSync(source + '\nexport { catalog }', { loader: 'tsx', format: 'cjs', jsx: 'automatic' }).code
const exportsObject = { exports: {} }
vm.runInNewContext(compiled, { module: exportsObject, exports: exportsObject.exports, require })
const { catalog, I18nProvider, useI18n } = exportsObject.exports
const screen = fs.readFileSync(path.join(__dirname, '../src/screens/nodeSettings.tsx'), 'utf8')
const keys = [...screen.matchAll(/t\('(ns\.(?:liveRing|localDecode|localBenchmark|agentRunning|benchmark|expert|noMeasurementTarget|measureFailed)[^']*)'/g)].map(m => m[1])
for (const state of ['live', 'idle', 'stale', 'unavailable']) keys.push(`ns.liveRingState.${state}`)
for (const reason of ['agentNotInspected', 'agentUnavailable', 'agentStale', 'expertOnly', 'noStage', 'feedUnavailable', 'feedHttp', 'noMatch', 'missingMetadata']) keys.push(`ns.liveRingReason.${reason}`)
for (const key of Object.keys(catalog.en).filter(key => key.startsWith('ns.expert'))) keys.push(key)

test('every live-ring and local benchmark UI key has English and Korean translations', () => {
  assert.ok(keys.length >= 27)
  for (const key of new Set(keys)) {
    assert.ok(catalog.en[key], `English: ${key}`)
    assert.ok(catalog.ko[key], `Korean: ${key}`)
    assert.match(catalog.ko[key], /[가-힣]/, key)
    assert.deepEqual(catalog.en[key].match(/\{\d+\}/g), catalog.ko[key].match(/\{\d+\}/g), key)
  }
  for (const literal of ['Live ring work', 'Achieved request throughput', 'Agent running', 'Last work:', 'Source:', 'Window:', 'Expert relay activity:', 'tok/s · local decode benchmark', 'Last local benchmark', 'last local benchmark']) {
    assert.ok(!screen.replace(/\/\*[\s\S]*?\*\//g, '').includes(literal), `hard-coded UI text: ${literal}`)
  }
  assert.ok(!screen.includes('{live.reason}') && !screen.includes('{stage.reason}'))
})
test('real provider renders localized labels, HTTP reasons and interpolated benchmark data', () => {
  function Probe() {
    const { t } = useI18n()
    return React.createElement('span', null, [t('ns.liveRingTitle'), t('ns.liveRingReason.feedHttp', 401),
      t('ns.localBenchmarkMetric', 'Metal'), t('ns.localBenchmarkLast', 'TIME')].join(' | '))
  }
  const render = lang => renderToStaticMarkup(React.createElement(I18nProvider, { initial: lang }, React.createElement(Probe)))
  assert.match(render('en'), /Live ring work.*Contribution feed HTTP 401.*Metal · last local benchmark.*TIME/)
  assert.match(render('ko'), /실시간 링 작업.*기여 피드 HTTP 401.*Metal · 마지막 로컬 벤치마크.*TIME/)
  assert.ok(!render('ko').includes('Live ring work'))
  assert.match(render('ja'), /Live ring work/) // existing English fallback for keys not requested in this task
})
