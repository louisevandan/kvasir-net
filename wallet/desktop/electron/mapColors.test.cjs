'use strict'
const { test } = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const { transformSync } = require('esbuild')

// map.tsx pulls in the live API at import; evaluate only its palette helpers.
const source = fs.readFileSync(path.join(__dirname, '../src/screens/map.tsx'), 'utf8')
const components = fs.readFileSync(path.join(__dirname, '../src/components.tsx'), 'utf8')
const palette = components.slice(components.indexOf('export const TIER_COLORS'), components.indexOf('\n', components.indexOf('export const tierColor')))
const start = source.indexOf('export const MAP_TIER_COLORS'), end = source.indexOf("'#64748b'", start) + "'#64748b'".length
const { code } = transformSync(palette + '\n' + source.slice(start, end), { loader: 'ts', format: 'cjs' })
const mod = { exports: {} }; new Function('module', 'exports', code)(mod, mod.exports)

test('online nodes: S red, A yellow, B blue, C green; offline grey', () => {
  const { mapNodeColor } = mod.exports
  assert.equal(mapNodeColor('S'), '#ef4444'); assert.equal(mapNodeColor('A'), '#facc15')
  assert.equal(mapNodeColor('B'), '#3b82f6'); assert.equal(mapNodeColor('C'), '#22c55e')
  assert.equal(mapNodeColor('S', false), '#64748b')
  assert.doesNotMatch(source, /tierColor/)
  // The tier table and badges use the same palette as the map.
  const { tierColor } = mod.exports
  for (const t of ['S', 'A', 'B', 'C']) assert.equal(tierColor(t), mapNodeColor(t))
})

test('land dots are brighter than the old stroke colour', () => {
  const css = fs.readFileSync(path.join(__dirname, '../src/theme.css'), 'utf8')
  assert.match(css, /\.land-dot \{ fill: color-mix\(in srgb, var\(--text-2\) 60%, transparent\); \}/)
  assert.doesNotMatch(css, /\.land-dot \{ fill: var\(--stroke\); \}/)
})
