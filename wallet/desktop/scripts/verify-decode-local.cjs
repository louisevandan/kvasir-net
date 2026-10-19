'use strict'
// Local-only benchmark probe. Never opens the operator's wallet/profile.
// Usage: PLAYWRIGHT_MODULE=/path/to/playwright node scripts/verify-decode-local.cjs APP_EXECUTABLE PROFILE MODEL
const fs = require('node:fs')
const path = require('node:path')
const { execFileSync } = require('node:child_process')
const { _electron } = require(process.env.PLAYWRIGHT_MODULE || 'playwright')
const [executablePath, profile, model] = process.argv.slice(2)
if (!executablePath || !profile || !model) throw new Error('expected executable, NEW profile, GGUF')
if (fs.existsSync(profile)) throw new Error('profile must be new, not an existing wallet profile')
fs.mkdirSync(path.join(profile, 'models'), { recursive: true })
fs.symlinkSync(path.resolve(model), path.join(profile, 'models', path.basename(model)))
;(async () => {
  let app
  try {
    app = await _electron.launch({ executablePath, env: {
      ...process.env, KVASIR_USER_DATA: path.resolve(profile), KVASIR_DEBUG_WALLET: '0',
      LINKCPP_LLAMA_SERVER: '/opt/homebrew/bin/llama-server',
    }, timeout: 60000 })
    app.process().stderr.on('data', d => process.stderr.write(d))
    // Capture just the timing/usage fields directly from the actual localhost
    // HTTP response, before the application's response mapping drops anything.
    await app.evaluate(() => {
      const original = global.fetch
      global.__decodeEvidence = []
      global.fetch = async (...args) => {
        const response = await original(...args)
        const url = new URL(String(args[0]))
        if (url.hostname === '127.0.0.1' && url.pathname === '/v1/chat/completions') {
          const data = await response.clone().json()
          global.__decodeEvidence.push({ timings: data.timings, usage: data.usage })
        }
        return response
      }
    })
    const page = await app.firstWindow()
    await page.waitForFunction(() => !!window.kvasir?.node)
    const results = []
    for (let i = 0; i < 2; i++) results.push(await page.evaluate(() => window.kvasir.node.benchmark(64)))
    const evidence = await app.evaluate(({ app }) => ({ version: app.getVersion(), server: global.__decodeEvidence }))
    console.log(JSON.stringify({ executablePath, profile, results, ...evidence }, null, 2))
  } finally {
    if (app) {
      // main.cjs does not yet stop its local llama-server on quit. Stop only
      // llama-server children of THIS isolated test app, never another node.
      let children = []
      try { children = execFileSync('/usr/bin/pgrep', ['-P', String(app.process().pid)], { encoding: 'utf8' }).trim().split('\n') } catch {}
      for (const pid of children) {
        try {
          const comm = execFileSync('/bin/ps', ['-p', pid, '-o', 'comm='], { encoding: 'utf8' }).trim()
          if (path.basename(comm) === 'llama-server') process.kill(Number(pid), 'SIGTERM')
        } catch {}
      }
      await app.close()
    }
  }
})().catch(e => { console.error(e.message); process.exitCode = 1 })
