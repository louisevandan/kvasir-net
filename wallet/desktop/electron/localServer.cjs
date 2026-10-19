'use strict'
const net = require('node:net')
const path = require('node:path')
const { randomUUID } = require('node:crypto')
const { spawn } = require('node:child_process')

// Ask the OS for a loopback port, never adopt the public/fixed 8123 listener.
async function freePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer()
    server.once('error', reject)
    server.listen(0, '127.0.0.1', () => {
      const port = server.address().port
      server.close(error => error ? reject(error) : resolve(port))
    })
  })
}

class LocalServer {
  constructor({ spawnProcess = spawn, request = (...args) => fetch(...args), allocatePort = freePort,
    sleep = ms => new Promise(resolve => setTimeout(resolve, ms)),
    timeouts = {} } = {}) {
    Object.assign(this, { spawnProcess, request, allocatePort, sleep })
    this.timeouts = { health: 1000, props: 5000, completion: 120000, ...timeouts }
    this.current = null
    this.busy = false
  }
  stop() {
    const current = this.current
    this.current = null
    if (current) { current.alive = false; current.child.kill() }
  }
  async generate(bin, model, prompt, maxTokens) {
    if (this.busy) return { ok: false, error: 'local generation already in progress' }
    this.busy = true
    try {
      if (this.current && this.current.model !== model) this.stop()
      if (!this.current) {
        const port = await this.allocatePort()
        // A fresh public model alias distinguishes this launch even from an
        // unrelated server serving the same file during the port bind race.
        const alias = `wallet-local-${randomUUID()}`
        const child = this.spawnProcess(bin, ['-m', model, '-ngl', '999', '--host', '127.0.0.1',
          '--port', String(port), '--alias', alias, '-c', '2048', '--no-mmap'],
        { stdio: 'ignore', env: { ...process.env, DYLD_LIBRARY_PATH: path.dirname(bin) } })
        const current = { child, model, port, alias, alive: true }
        this.current = current
        const ended = () => {
          current.alive = false
          if (this.current === current) this.current = null
        }
        child.once('exit', ended)
        child.once('error', ended)
      }
      const current = this.current
      const base = `http://127.0.0.1:${current.port}`
      let ready = false
      for (let i = 0; i < 60; i++) {
        await this.sleep(1000)
        if (!current.alive) throw new Error('local llama-server exited before readiness')
        try {
          const health = await this.request(`${base}/health`, { signal: AbortSignal.timeout(this.timeouts.health) })
          if (health.ok) { ready = true; break }
        } catch {}
      }
      if (!ready) throw new Error('local llama-server readiness timed out')
      // Also reject a different model if a listener wins the bind race between
      // releasing the reservation and the child binding. Never trust health alone.
      const props = await this.request(`${base}/props`, { signal: AbortSignal.timeout(this.timeouts.props) })
      const info = await props.json()
      if (!props.ok || info.model_path !== model || !current.alive) {
        throw new Error('local llama-server model ownership check failed')
      }
      const models = await this.request(`${base}/v1/models`, { signal: AbortSignal.timeout(this.timeouts.props) })
      const listing = await models.json()
      if (!models.ok || !listing.data?.some(entry => entry.id === current.alias) || !current.alive) {
        throw new Error('local llama-server launch ownership check failed')
      }
      const response = await this.request(`${base}/v1/chat/completions`, {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ model: current.alias, messages: [{ role: 'user', content: prompt }], max_tokens: maxTokens, stream: false }),
        signal: AbortSignal.timeout(this.timeouts.completion),
      })
      const data = await response.json()
      if (!response.ok) throw new Error(`local generation failed (HTTP ${response.status})`)
      if (!current.alive) throw new Error('local llama-server exited during generation')
      return { ok: true, text: data.choices?.[0]?.message?.content ?? '', usage: data.usage || {}, timings: data.timings || null }
    } catch (error) {
      this.stop()
      return { ok: false, error: String(error) }
    } finally {
      this.busy = false
    }
  }
}
module.exports = { LocalServer, freePort }
