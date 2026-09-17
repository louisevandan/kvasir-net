'use strict'
/**
 * The node this app runs.
 *
 * "Run node" used to be a registration and a heartbeat: the app told the
 * settlement service it was serving, and nothing on the machine ran. On p4 a
 * node is a real process — a p4 agent that owns this machine's accelerators and
 * hosts whatever stages an operator's placement plan gives it. So the app
 * supervises that process and reports what it actually finds.
 *
 * Placement stays out of here on purpose. p4 loads a model from a plan the
 * operator prepares and gates the load on its own integrity checks; the app can
 * offer the machine, not decide what runs on it.
 */
const fs = require('node:fs')
const path = require('node:path')
const { spawn } = require('node:child_process')
const { connect } = require('kvasir-p4-bridge/wire')

const DEFAULT_PORT = Number(process.env.KVASIR_P4_PORT || 42031)
const LOG_LINES = 200

/** Where a p4-agent binary may live: explicit env, packaged resources, repo build. */
function agentBinary() {
  const explicit = process.env.KVASIR_P4_AGENT
  if (explicit && fs.existsSync(explicit)) return explicit
  const candidates = []
  if (process.resourcesPath) candidates.push(path.join(process.resourcesPath, 'p4', 'p4-agent'))
  const repo = path.resolve(__dirname, '..', '..', '..')
  candidates.push(
    path.join(repo, 'p4', 'build', 'release', 'p4-agent'),
    path.join(repo, 'p4', 'target', 'release', 'p4-agent'),
    path.join(repo, 'build', 'release', 'p4-agent'),
  )
  return candidates.find((candidate) => fs.existsSync(candidate)) || null
}

class P4Node {
  constructor({ port = DEFAULT_PORT } = {}) {
    this.port = port
    this.address = `tcp://127.0.0.1:${port}`
    this.proc = null
    this.startedAt = null
    this.log = []
    this.lastError = null
    this.lastExit = null
    this.snapshot = null
    this.snapshotAt = null
  }

  _record(line) {
    for (const part of String(line).split('\n')) {
      if (!part.trim()) continue
      this.log.push(`${new Date().toISOString().slice(11, 19)} ${part}`)
    }
    if (this.log.length > LOG_LINES) this.log.splice(0, this.log.length - LOG_LINES)
  }

  start() {
    if (this.proc) return this.status()
    const binary = agentBinary()
    if (!binary) {
      this.lastError = 'no p4-agent binary found (set KVASIR_P4_AGENT)'
      return this.status()
    }
    try {
      // The agent takes its bind address and the address it answers to; both are
      // loopback, so nothing is exposed until an operator fronts it.
      this.proc = spawn(binary, [`127.0.0.1:${this.port}`, this.address], {
        stdio: ['ignore', 'pipe', 'pipe'],
        windowsHide: true,
      })
    } catch (error) {
      this.lastError = String(error.message ?? error)
      this.proc = null
      return this.status()
    }
    this.startedAt = Date.now()
    this.lastError = null
    this.lastExit = null
    this.proc.stdout?.on('data', (chunk) => this._record(chunk))
    this.proc.stderr?.on('data', (chunk) => this._record(chunk))
    this.proc.on('exit', (code, signal) => {
      this.lastExit = { code, signal, at: Date.now() }
      this._record(`agent exited code=${code} signal=${signal ?? 'none'}`)
      this.proc = null
      this.startedAt = null
      this.snapshot = null
    })
    this.proc.on('error', (error) => { this.lastError = String(error.message ?? error) })
    return this.status()
  }

  /** Ask politely, then insist. */
  async stop({ graceMs = 5000 } = {}) {
    const proc = this.proc
    if (!proc) return this.status()
    proc.kill('SIGTERM')
    await new Promise((resolve) => {
      const timer = setTimeout(() => { try { proc.kill('SIGKILL') } catch {} resolve() }, graceMs)
      proc.once('exit', () => { clearTimeout(timer); resolve() })
    })
    return this.status()
  }

  /**
   * Ask the agent what it is holding. This is the only honest source for the
   * node list: the app does not decide what is loaded here.
   */
  async inspect({ timeoutMs = 8000 } = {}) {
    if (!this.proc) return null
    let client = null
    try {
      client = await connect({
        host: '127.0.0.1', port: this.port, address: this.address,
        channel: `wallet-${process.pid.toString(16)}`, deadlineMs: timeoutMs,
      })
      const snapshot = await client.inspect(this.address, { timeoutMs })
      this.snapshot = snapshot
      this.snapshotAt = Date.now()
      this.lastError = null
      return snapshot
    } catch (error) {
      this.lastError = String(error.message ?? error)
      return null
    } finally {
      client?.close?.()
    }
  }

  status() {
    const snapshot = this.snapshot
    return {
      running: Boolean(this.proc),
      pid: this.proc?.pid ?? null,
      port: this.port,
      address: this.address,
      binary: agentBinary(),
      uptimeMs: this.startedAt ? Date.now() - this.startedAt : 0,
      lastError: this.lastError,
      lastExit: this.lastExit,
      // What the agent reports, not what the app assumed.
      nodes: (snapshot?.nodes ?? []).map((node) => ({
        nodeId: node.node_id, state: node.state,
        generation: node.generation, adapterKind: node.adapter_kind,
      })),
      gpus: (snapshot?.machine?.capability?.gpus ?? []).map((gpu) => ({
        name: gpu.name, backend: gpu.backend, memoryBytes: gpu.memory_total_bytes ?? null,
      })),
      snapshotAt: this.snapshotAt,
      log: this.log.slice(-40),
    }
  }
}

module.exports = { P4Node, agentBinary }
