'use strict'
// Direct expert relay over SSH, for an operator-configured wallet only.
//
// The public relay path (gateway → Cloudflare → tunnel → bridge) measured
// 266 ms per expert call on 2026-10-19; the same call over SSH straight to the
// backbone host measured 21 ms. Until the bridge has a public direct port, an
// operator can give one machine a forward-only SSH key to the bridge host and
// list the wallet addresses allowed to use it. Nothing here is on by default
// and no key ships with the app: config.directRelay names a key file on disk.
//
//   config.directRelay = {
//     host, port = 22, user, keyPath,          // the bridge host and a forward-only key
//     bridgePort = 19000,                      // the bridge's local HTTP/WS port there
//     allowAddresses: ['<wallet address>'],    // only these wallets use it
//   }
//
// The key is expected to be restricted on the server side
// (`restrict,port-forwarding,permitopen="127.0.0.1:19000",command="/bin/false"`),
// so holding it grants a forward to the bridge and nothing else.
const net = require('node:net')
const fs = require('node:fs')
const { spawn } = require('node:child_process')

function freePort() {
  return new Promise((resolve, reject) => {
    const s = net.createServer()
    s.once('error', reject)
    s.listen(0, '127.0.0.1', () => { const p = s.address().port; s.close(() => resolve(p)) })
  })
}

function waitForPort(port, deadlineMs, connect = net.connect) {
  const until = Date.now() + deadlineMs
  return new Promise((resolve, reject) => {
    const attempt = () => {
      const sock = connect({ host: '127.0.0.1', port })
      sock.once('connect', () => { sock.destroy(); resolve() })
      sock.once('error', () => { sock.destroy(); Date.now() > until ? reject(new Error('ssh forward did not open')) : setTimeout(attempt, 150) })
    }
    attempt()
  })
}

/** Validated settings, or null when the direct path does not apply to this wallet. */
function directRelaySettings(config, address) {
  const d = config && config.directRelay
  if (!d || typeof d !== 'object') return null
  const allow = Array.isArray(d.allowAddresses) ? d.allowAddresses : []
  if (!address || !allow.includes(address)) return null
  if (typeof d.host !== 'string' || !d.host || typeof d.user !== 'string' || !d.user || typeof d.keyPath !== 'string' || !d.keyPath) return null
  const port = Number.isInteger(d.port) ? d.port : 22
  const bridgePort = Number.isInteger(d.bridgePort) ? d.bridgePort : 19000
  return { host: d.host, port, user: d.user, keyPath: d.keyPath, bridgePort }
}

class DirectRelay {
  constructor({ settings, log = () => {}, spawnProcess = spawn, allocatePort = freePort, wait = waitForPort, exists = fs.existsSync, retryMs = 15000 } = {}) {
    Object.assign(this, { settings, log, spawnProcess, allocatePort, wait, exists, retryMs })
    this.proc = null; this.port = null; this.state = 'off'; this.error = null; this.timer = null; this.stopped = true
  }
  /** ws:// base for the relay when the forward is up, else null (callers fall back to the gateway). */
  base() { return this.state === 'up' && this.port ? `ws://127.0.0.1:${this.port}` : null }
  status() {
    return { state: this.state, via: this.settings ? `${this.settings.user}@${this.settings.host}:${this.settings.port}` : null, localPort: this.port, error: this.error }
  }
  async start() {
    if (!this.settings) return this.status()
    this.stopped = false
    if (this.proc || this.state === 'starting') return this.status()
    if (!this.exists(this.settings.keyPath)) { this.state = 'failed'; this.error = 'ssh key not found'; return this.status() }
    this.state = 'starting'; this.error = null
    try {
      const port = await this.allocatePort()
      const s = this.settings
      const proc = this.spawnProcess('ssh', ['-N', '-T',
        '-o', 'BatchMode=yes', '-o', 'ExitOnForwardFailure=yes', '-o', 'ServerAliveInterval=15', '-o', 'ServerAliveCountMax=3',
        '-o', 'StrictHostKeyChecking=accept-new', '-o', 'IdentitiesOnly=yes', '-i', s.keyPath, '-p', String(s.port),
        '-L', `127.0.0.1:${port}:127.0.0.1:${s.bridgePort}`, `${s.user}@${s.host}`], { stdio: 'ignore' })
      this.proc = proc
      proc.once('exit', (code) => {
        if (this.proc !== proc) return
        this.proc = null; this.port = null
        this.state = this.stopped ? 'off' : 'failed'
        if (!this.stopped) { this.error = `ssh exited (${code})`; this.log(`direct relay: ${this.error}; using the gateway, retrying in ${this.retryMs / 1000}s`); this.timer = setTimeout(() => { this.timer = null; void this.start() }, this.retryMs) }
      })
      await this.wait(port, 10000)
      if (this.proc !== proc) return this.status()
      this.port = port; this.state = 'up'
      this.log(`direct relay: ssh forward up via ${s.user}@${s.host} → bridge 127.0.0.1:${s.bridgePort}`)
    } catch (e) {
      this.state = 'failed'; this.error = e.message
      if (this.proc) { const p = this.proc; this.proc = null; p.kill() }
      if (!this.stopped && !this.timer) this.timer = setTimeout(() => { this.timer = null; void this.start() }, this.retryMs)
    }
    return this.status()
  }
  stop() {
    this.stopped = true
    if (this.timer) { clearTimeout(this.timer); this.timer = null }
    const p = this.proc; this.proc = null; this.port = null; this.state = 'off'
    if (p) p.kill()
  }
}

module.exports = { DirectRelay, directRelaySettings, freePort, waitForPort }
