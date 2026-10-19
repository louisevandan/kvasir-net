'use strict'
const { performance } = require('node:perf_hooks')
const net = require('node:net')
const WINDOW_MS = 60000
const MAX_PENDING = 128

class ExpertMetrics {
  constructor({ now = Date.now } = {}) { this.now = now; this.startedAt = now(); this.samples = []; this.lastWorkAt = null; this.truncatedAt = null }
  record(rows, latencyMs) {
    const at = this.now()
    this.samples.push({ at, rows, latencyMs })
    this.lastWorkAt = at
    this.prune(at)
  }
  prune(at) {
    this.samples = this.samples.filter(s => at - s.at < WINDOW_MS)
    if (this.samples.length > 10000) {
      this.truncatedAt = this.samples[this.samples.length - 10000 - 1].at
      this.samples = this.samples.slice(-10000)
    }
  }
  status() {
    const at = this.now(); this.prune(at)
    const samples = this.samples
    const windowMs = Math.max(1, Math.min(WINDOW_MS, at - Math.max(this.startedAt, this.truncatedAt ?? this.startedAt)))
    const latencies = samples.map(s => s.latencyMs).sort((a, b) => a - b)
    const percentile = p => latencies.length ? latencies[Math.ceil(latencies.length * p) - 1] : null
    const recentRows = Array.from({ length: 12 }, (_, i) => {
      const end = at - (11 - i) * 5000
      return samples.filter(s => s.at <= end && s.at > end - 5000).reduce((n, s) => n + s.rows, 0)
    })
    return { recentRows, state: samples.length ? 'live' : 'idle', calls: samples.length,
      rows: samples.reduce((n, s) => n + s.rows, 0), windowMs, windowLimitMs: WINDOW_MS,
      callsPerSecond: samples.length / windowMs * 1000,
      rowsPerSecond: samples.reduce((n, s) => n + s.rows, 0) / windowMs * 1000,
      p50Ms: percentile(0.5), p95Ms: percentile(0.95), lastWorkAt: this.lastWorkAt }
  }
}

// Passive framing observer; hidden states never retained. Native handle_conn in
// apps/linkcpp-expert-worker/main.cpp defines these little-endian wire shapes.
// Any lost framing disables metrics for this connection, not the actual relay.
class ExpertFrames {
  constructor(nEmbd, metrics, clock = () => performance.now()) {
    this.nEmbd = nEmbd; this.metrics = metrics; this.clock = clock
    this.header = Buffer.alloc(12); this.used = 0; this.bodyLeft = 0
    this.pending = []; this.current = null; this.disabled = !Number.isInteger(nEmbd) || nEmbd <= 0 || nEmbd > 65536
  }
  disable() { this.disabled = true; this.pending = []; this.current = null }
  request(data) {
    if (this.disabled) return
    let offset = 0
    while (offset < data.length) {
      if (this.bodyLeft) {
        const size = Math.min(this.bodyLeft, data.length - offset)
        this.bodyLeft -= size; offset += size
        if (!this.bodyLeft) { this.current.complete = true; this.current = null; this.used = 0 }
        continue
      }
      if (!this.used) this.started = this.clock()
      const need = this.used < 8 ? 8 : 12
      const size = Math.min(need - this.used, data.length - offset)
      data.copy(this.header, this.used, offset, offset + size); offset += size; this.used += size
      if (this.used < 8) continue
      const mode = this.header.readInt32LE(0), rows = this.header.readInt32LE(4)
      if (rows < 1 || rows > 8192 || !((mode >= 1 && mode <= 64) || mode === -2 || mode === -3)) return this.disable()
      if (mode < 0 && this.used < 12) continue
      const pairs = mode < 0 ? this.header.readInt32LE(8) : mode * rows
      if (pairs < 1 || (mode < 0 && pairs > 65536) || this.pending.length >= MAX_PENDING) return this.disable()
      const width = mode === -3 ? 2 : 4
      this.bodyLeft = this.nEmbd * rows * width + (mode < 0 ? pairs * (8 + width) : pairs * 4)
      const responseBytes = this.nEmbd * width * (mode < 0 ? rows : pairs)
      this.current = { rows, started: this.started, left: responseBytes, complete: false }
      this.pending.push(this.current)
    }
  }
  response(data) {
    if (this.disabled) return
    let left = data.length
    while (left) {
      const item = this.pending[0]
      if (!item || !item.complete) return this.disable()
      const size = Math.min(item.left, left); item.left -= size; left -= size
      if (!item.left) {
        this.metrics.record(item.rows, Math.max(0, this.clock() - item.started))
        this.pending.shift()
      }
    }
  }
}

// Reuses ONLY a currently owned worker; synthetic bytes do not traverse the
// relay and cannot be counted as real work or sent to contribution accounting.
async function benchmarkExpert({ port, nEmbd, experts, isCurrent, iterations = 5, timeoutMs = 10000 }) {
  if (!Number.isInteger(port) || port < 1 || port > 65535 || !Number.isInteger(nEmbd) || nEmbd < 1 || nEmbd > 65536
    || !Number.isInteger(experts) || experts < 1 || !isCurrent()) throw Error('expert worker unavailable')
  const rows = 1, nUsed = Math.min(experts, 8)
  const frame = Buffer.alloc(8 + nEmbd * rows * 4 + nUsed * rows * 4)
  frame.writeInt32LE(nUsed, 0); frame.writeInt32LE(rows, 4)
  for (let i = 0; i < nEmbd; i++) frame.writeFloatLE(0.05 * ((i % 97) - 48), 8 + i * 4)
  for (let i = 0; i < nUsed; i++) frame.writeInt32LE(i, 8 + nEmbd * 4 + i * 4)
  const expected = nEmbd * nUsed * rows * 4
  return new Promise((resolve, reject) => {
    const socket = net.connect(port, '127.0.0.1')
    const latencies = []
    let received = 0, completed = 0, started = 0, settled = false
    let floatTail = Buffer.alloc(0)
    const finish = (error, result) => {
      if (settled) return
      settled = true; clearTimeout(timer); socket.destroy()
      error ? reject(error) : resolve(result)
    }
    const timer = setTimeout(() => finish(Error('expert benchmark timed out')), timeoutMs)
    const send = () => {
      if (!isCurrent()) return finish(Error('expert assignment changed'))
      received = 0; started = performance.now(); socket.write(frame)
    }
    socket.setNoDelay(true)
    socket.on('connect', send)
    socket.on('error', error => finish(error))
    socket.on('close', () => finish(Error('expert worker closed before benchmark completed')))
    socket.on('data', data => {
      const values = Buffer.concat([floatTail, data])
      const full = values.length - values.length % 4
      for (let i = 0; i < full; i += 4) {
        if (!Number.isFinite(values.readFloatLE(i))) return finish(Error('non-finite expert output'))
      }
      floatTail = Buffer.from(values.subarray(full))
      received += data.length
      if (!isCurrent()) return finish(Error('expert assignment changed'))
      if (received > expected) return finish(Error('unexpected expert response length'))
      if (received < expected) return
      if (completed++) latencies.push(Math.max(0.001, performance.now() - started)) // exclude warmup
      if (completed <= iterations) return send()
      const elapsedMs = latencies.reduce((a, b) => a + b, 0)
      latencies.sort((a, b) => a - b)
      finish(null, { kind: 'synthetic-expert-ffn', rowsPerSecond: iterations * rows / elapsedMs * 1000,
        callsPerSecond: iterations / elapsedMs * 1000, p50Ms: latencies[Math.ceil(iterations * 0.5) - 1],
        p95Ms: latencies[Math.ceil(iterations * 0.95) - 1], elapsedMs, calls: iterations,
        rows: iterations * rows, batchRows: rows, expertsPerRow: nUsed, at: Date.now() })
    })
  })
}
module.exports = { ExpertMetrics, ExpertFrames, benchmarkExpert }
