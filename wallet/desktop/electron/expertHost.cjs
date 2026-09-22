'use strict'
/**
 * Hosting an expert assignment: shard on disk, worker serving it, relay dialed.
 *
 * participation.cjs gets this machine an assignment — "layer L, experts
 * [a, b)". Until now nothing acted on it, so heldSegments() stayed [] and the
 * machine was a volunteer that never held anything. This is the part that
 * holds it:
 *
 *   1. download the shard GGUF from the gateway, and check it before trusting
 *      it (length, GGUF magic, the layer/range the metadata says it holds);
 *   2. start linkcpp-expert-worker --serve on it, on a loopback port;
 *   3. report the segment only once the worker is actually serving, with a
 *      "relay:<session>" url so the bridge wires a relay target for it;
 *   4. dial /api/expert-relay and pipe the WebSocket to the worker's port.
 *      The bridge splices that socket to the backbone's dispatch listener.
 *
 * A segment is reported only in step 3 and only while the worker process
 * lives. Reporting an assignment that is still downloading would count a
 * replica that cannot compute — the bridge would stop recruiting for a range
 * nothing serves.
 *
 * The relay dial is retried with backoff for as long as the segment is held.
 * Refusals are expected: 4404 until the coverage post has wired the session,
 * and an immediate close while no backbone is listening on the bridge side.
 */
const fs = require('node:fs')
const path = require('node:path')
const net = require('node:net')
const { spawn } = require('node:child_process')

const SHARD_PATH = (model, layer, begin, end) =>
  `/api/proxy/models/${encodeURIComponent(model)}/expert-shard?layer=${layer}&expert_begin=${begin}&expert_end=${end}`
const DOWNLOAD_ATTEMPTS = 3
const WORKER_READY_MS = 120_000
const RELAY_RETRY_MIN = 5_000
const RELAY_RETRY_MAX = 60_000
const GGUF_MAGIC = Buffer.from('GGUF', 'ascii')

class ExpertHostError extends Error {
  constructor(message, code) { super(message); this.code = code }
}

/** Free loopback port: bind 0, read it, release it. */
function freePort() {
  return new Promise((resolve, reject) => {
    const srv = net.createServer()
    srv.once('error', reject)
    srv.listen(0, '127.0.0.1', () => {
      const { port } = srv.address()
      srv.close(() => resolve(port))
    })
  })
}

// ---- minimal GGUF header reader ----------------------------------------------
// Only the metadata is read: enough to check the file is the shard we asked
// for before a worker spends VRAM on it. The worker parses the rest.

const GGUF_TYPE_SIZE = { 0: 1, 1: 1, 2: 2, 3: 2, 4: 4, 5: 4, 6: 4, 7: 1, 10: 8, 11: 8, 12: 8 }

function readGgufMetadata(file) {
  const fd = fs.openSync(file, 'r')
  try {
    const head = Buffer.alloc(1 << 20)
    const n = fs.readSync(fd, head, 0, head.length, 0)
    const buf = head.subarray(0, n)
    if (buf.length < 24 || !buf.subarray(0, 4).equals(GGUF_MAGIC)) throw new ExpertHostError('not a GGUF file', 'bad_shard')
    let o = 8
    const u64 = () => { const v = Number(buf.readBigUInt64LE(o)); o += 8; return v }
    const str = () => { const len = u64(); const s = buf.toString('utf8', o, o + len); o += len; return s }
    u64() // tensor count
    const nKv = u64()
    const meta = {}
    const value = (type) => {
      switch (type) {
        case 0: return buf.readUInt8(o++)
        case 1: return buf.readInt8(o++)
        case 2: { const v = buf.readUInt16LE(o); o += 2; return v }
        case 3: { const v = buf.readInt16LE(o); o += 2; return v }
        case 4: { const v = buf.readUInt32LE(o); o += 4; return v }
        case 5: { const v = buf.readInt32LE(o); o += 4; return v }
        case 6: { const v = buf.readFloatLE(o); o += 4; return v }
        case 7: return buf.readUInt8(o++) !== 0
        case 8: return str()
        case 9: {
          const t = buf.readUInt32LE(o); o += 4
          const len = u64()
          if (t === 8) { for (let i = 0; i < len; i++) str() } else o += len * (GGUF_TYPE_SIZE[t] || 0)
          return null   // arrays are skipped, not needed here
        }
        case 10: return u64()
        case 11: { const v = Number(buf.readBigInt64LE(o)); o += 8; return v }
        case 12: { const v = buf.readDoubleLE(o); o += 8; return v }
        default: throw new ExpertHostError(`unknown GGUF value type ${type}`, 'bad_shard')
      }
    }
    for (let i = 0; i < nKv; i++) {
      const key = str()
      const type = buf.readUInt32LE(o); o += 4
      meta[key] = value(type)
    }
    return meta
  } catch (e) {
    if (e instanceof ExpertHostError) throw e
    throw new ExpertHostError(`unreadable GGUF header: ${e.message}`, 'bad_shard')
  } finally {
    fs.closeSync(fd)
  }
}

class ExpertHost {
  /**
   * @param {object} o
   * @param {string} o.baseUrl         gateway, e.g. https://gate.kvasir-ai.net
   * @param {() => Promise<string>} o.token   a valid node token (participation.ensureToken)
   * @param {() => string|null} o.workerBinary path to linkcpp-expert-worker, or null
   * @param {string} o.shardDir        where shards are kept between runs
   * @param {(line: string) => void} [o.log]
   * @param {Function} [o.WebSocket]   injected for tests; defaults to the ws package
   * @param {Function} [o.fetch]       injected for tests; defaults to global fetch
   * @param {string[]} [o.workerArgs]  extra worker flags (tests pass a fake worker's)
   */
  constructor({ baseUrl, token, workerBinary, shardDir, log = () => {}, WebSocket = null, fetch = null, workerArgs = [] }) {
    this.base = String(baseUrl || '').replace(/\/+$/, '')
    this.tokenFn = token
    this.workerBinaryFn = workerBinary
    this.shardDir = shardDir
    this.log = log
    this.WebSocket = WebSocket || require('ws')
    this.fetch = fetch || globalThis.fetch
    this.workerArgs = workerArgs
    this.phase = 'idle'          // idle | downloading | starting | serving | failed
    this.held = null             // { model, layer, begin, end, path, nEmbd }
    this.proc = null
    this.port = null
    this.session = null
    this.relay = { ws: null, tcp: null, timer: null, retryMs: RELAY_RETRY_MIN, connected: false, bytes: 0, lastClose: null }
    this.lastError = null
    this.generation = 0          // bumps on release; stale async work checks it
  }

  /** Is there a worker binary to run at all? */
  canHost() { return Boolean(this.workerBinaryFn()) }

  /** Is this machine busy with an assignment (so it must not volunteer again)? */
  busy() { return this.phase === 'downloading' || this.phase === 'starting' || this.phase === 'serving' }

  /** What it can compute right now. Empty unless the worker is up. */
  heldSegments() {
    return this.phase === 'serving' && this.held ? [[this.held.layer, this.held.begin, this.held.end]] : []
  }

  /** The coverage url: wires a relay session on the bridge, only while serving. */
  coverageUrl() { return this.phase === 'serving' && this.session ? `relay:${this.session}` : '' }

  heldExperts() { return this.held ? this.held.end - this.held.begin : 0 }

  /**
   * Take on an assignment. Resolves when the worker serves it, or rejects with
   * the reason it could not. Safe to call while another is in flight: the
   * older one is released first.
   */
  async provision(assignment, { workerId }) {
    const [begin, end] = assignment.experts
    const want = { model: assignment.model, layer: Number(assignment.layer), begin: Number(begin), end: Number(end) }
    if (this.held && this.busy() && this.held.model === want.model && this.held.layer === want.layer
      && this.held.begin === want.begin && this.held.end === want.end) return
    this.release('reassigned')
    const gen = ++this.generation
    this.held = { ...want, path: null, nEmbd: Number(assignment.n_embd) || 0 }
    this.session = `expert-${workerId}`
    this.lastError = null
    try {
      this.phase = 'downloading'
      const file = await this.download(want, gen)
      if (gen !== this.generation) return
      this.held.path = file
      this.phase = 'starting'
      await this.startWorker(gen)
      if (gen !== this.generation) return
      this.phase = 'serving'
      this.log(`expert host: serving ${want.model} layer ${want.layer} experts ${want.begin}-${want.end} on 127.0.0.1:${this.port}`)
      this.dialRelay(gen)
    } catch (e) {
      if (gen !== this.generation) return
      this.lastError = e.message
      this.log(`expert host: ${e.message}`)
      this.stopWorker()
      this.phase = 'failed'
      throw e
    }
  }

  shardFile({ model, layer, begin, end }) {
    const safe = String(model).replace(/[^A-Za-z0-9._-]/g, '_')
    return path.join(this.shardDir, safe, `L${layer}_e${String(begin).padStart(3, '0')}-${String(end).padStart(3, '0')}.gguf`)
  }

  /**
   * Download to a temporary name and rename into place only after the file
   * checks out. A shard cut short mid-transfer (seen in practice: a 19 MB
   * shard ending 400 KB early) must never be left where the next start would
   * pick it up.
   */
  async download(want, gen) {
    const file = this.shardFile(want)
    if (fs.existsSync(file)) {
      try { this.checkShard(file, want); this.log(`expert host: using cached shard ${path.basename(file)}`); return file }
      catch (e) { this.log(`expert host: cached shard rejected (${e.message}), downloading again`); fs.rmSync(file, { force: true }) }
    }
    fs.mkdirSync(path.dirname(file), { recursive: true })
    const tmp = `${file}.part`
    let lastErr = null
    for (let attempt = 1; attempt <= DOWNLOAD_ATTEMPTS; attempt++) {
      if (gen !== this.generation) throw new ExpertHostError('released during download', 'released')
      try {
        await this.fetchTo(SHARD_PATH(want.model, want.layer, want.begin, want.end), tmp)
        this.checkShard(tmp, want)
        fs.renameSync(tmp, file)
        return file
      } catch (e) {
        lastErr = e
        fs.rmSync(tmp, { force: true })
        this.log(`expert host: shard download attempt ${attempt} failed: ${e.message}`)
      }
    }
    throw new ExpertHostError(`shard download failed: ${lastErr && lastErr.message}`, 'download_failed')
  }

  async fetchTo(urlPath, dest) {
    const token = await this.tokenFn()
    const res = await this.fetch(this.base + urlPath, {
      headers: { Authorization: `Bearer ${token}`, 'User-Agent': 'kvasir-wallet-desktop' },
    })
    if (!res.ok) throw new ExpertHostError(`gateway answered ${res.status}`, 'http')
    const expected = Number(res.headers.get('content-length'))
    const out = fs.createWriteStream(dest)
    let got = 0
    try {
      for await (const chunk of res.body) {
        got += chunk.length
        if (!out.write(chunk)) await new Promise((r) => out.once('drain', r))
      }
    } finally {
      await new Promise((r) => out.end(r))
    }
    if (Number.isFinite(expected) && expected > 0 && got !== expected) {
      throw new ExpertHostError(`body ended at ${got} of ${expected} bytes`, 'truncated')
    }
  }

  /** The metadata must name the shard we asked for; anything else is refused. */
  checkShard(file, want) {
    const m = readGgufMetadata(file)
    const layer = m['kvasir.expert_shard.layer']
    const begin = m['kvasir.expert_shard.expert_begin'] ?? m['linkcpp.expert_shard.expert_begin']
    const end = m['kvasir.expert_shard.expert_end'] ?? m['linkcpp.expert_shard.expert_end']
    if ((layer != null && layer !== want.layer) || begin !== want.begin || end !== want.end) {
      throw new ExpertHostError(`shard holds layer ${layer} experts ${begin}-${end}, not layer ${want.layer} ${want.begin}-${want.end}`, 'bad_shard')
    }
    return m
  }

  /** Start the worker and wait until its port accepts a connection. */
  async startWorker(gen) {
    const bin = this.workerBinaryFn()
    if (!bin) throw new ExpertHostError('no expert worker binary is installed', 'no_worker')
    this.port = await freePort()
    const args = [...this.workerArgs, '--model', this.held.path, '--serve', String(this.port),
      '--layer', String(this.held.layer), '--n-embd', String(this.held.nEmbd || 0)]
    const proc = spawn(bin, args, { windowsHide: true, stdio: ['ignore', 'ignore', 'pipe'] })
    this.proc = proc
    let tail = ''
    proc.stderr.on('data', (d) => { tail = (tail + String(d)).slice(-2000) })
    proc.on('exit', (code, signal) => {
      if (this.proc !== proc) return
      this.proc = null
      if (gen === this.generation && this.phase === 'serving') {
        // A dead worker holds nothing: stop reporting it, and let the next
        // tick volunteer again rather than advertise a range nobody serves.
        this.lastError = `worker exited (${signal || code}): ${tail.trim().split(/\r?\n/).pop() || ''}`
        this.log(`expert host: ${this.lastError}`)
        this.closeRelay()
        this.phase = 'failed'
      }
    })
    const deadline = Date.now() + WORKER_READY_MS
    while (Date.now() < deadline) {
      if (gen !== this.generation) throw new ExpertHostError('released while starting', 'released')
      if (!this.proc) {
        throw new ExpertHostError(`worker exited before serving: ${tail.trim().split(/\r?\n/).pop() || 'no output'}`, 'worker_failed')
      }
      if (await canConnect(this.port)) return
      await new Promise((r) => setTimeout(r, 250))
    }
    throw new ExpertHostError('worker did not start serving within 2 minutes', 'worker_timeout')
  }

  stopWorker() {
    const p = this.proc
    this.proc = null
    if (p) { try { p.kill() } catch { /* already gone */ } }
  }

  // ---- relay ---------------------------------------------------------------

  relayUrl(token) {
    const ws = this.base.replace(/^http/, 'ws')
    const q = new URLSearchParams({ session: this.session, token })
    return `${ws}/api/expert-relay?${q}`
  }

  async dialRelay(gen) {
    if (gen !== this.generation || this.phase !== 'serving') return
    let token
    try { token = await this.tokenFn() } catch (e) { return this.scheduleRelay(gen, `no token: ${e.message}`) }
    const ws = new this.WebSocket(this.relayUrl(token), { headers: { 'User-Agent': 'kvasir-wallet-desktop' } })
    this.relay.ws = ws
    let tcp = null
    ws.binaryType = 'nodebuffer'
    ws.on('open', () => {
      // Connect to the worker only once the bridge has accepted the session:
      // the worker serves each connection on its own thread, so idle
      // connections opened for refused dials would pile up there.
      tcp = net.connect(this.port, '127.0.0.1')
      tcp.setNoDelay(true)
      this.relay.tcp = tcp
      tcp.on('data', (d) => { if (ws.readyState === 1) ws.send(d) })
      tcp.on('close', () => { try { ws.close() } catch { /* closing */ } })
      tcp.on('error', () => { try { ws.close() } catch { /* closing */ } })
    })
    ws.on('message', (data) => {
      this.relay.connected = true
      this.relay.retryMs = RELAY_RETRY_MIN
      this.relay.bytes += data.length
      if (tcp && !tcp.destroyed) tcp.write(data)
    })
    ws.on('error', () => { /* 'close' follows and carries the reason */ })
    ws.on('close', (code, reason) => {
      this.relay.connected = false
      this.relay.lastClose = { code, reason: String(reason || ''), at: Date.now() }
      if (tcp) { try { tcp.destroy() } catch { /* gone */ } }
      if (this.relay.ws === ws) { this.relay.ws = null; this.relay.tcp = null }
      this.scheduleRelay(gen, `relay closed (${code}${reason ? ` ${reason}` : ''})`)
    })
  }

  scheduleRelay(gen, why) {
    if (gen !== this.generation || this.phase !== 'serving') return
    const wait = this.relay.retryMs
    // Back off while nothing is on the other end; a bridge with no backbone
    // listening refuses every dial, and hammering it helps nobody.
    this.relay.retryMs = Math.min(RELAY_RETRY_MAX, wait * 2)
    if (why !== this.relay.lastWhy) this.log(`expert host: ${why}; redialing in ${Math.round(wait / 1000)}s`)
    this.relay.lastWhy = why
    clearTimeout(this.relay.timer)
    this.relay.timer = setTimeout(() => this.dialRelay(gen), wait)
    this.relay.timer.unref?.()
  }

  closeRelay() {
    clearTimeout(this.relay.timer)
    const { ws, tcp } = this.relay
    this.relay = { ws: null, tcp: null, timer: null, retryMs: RELAY_RETRY_MIN, connected: false, bytes: this.relay.bytes, lastClose: this.relay.lastClose }
    if (ws) { try { ws.terminate() } catch { /* gone */ } }
    if (tcp) { try { tcp.destroy() } catch { /* gone */ } }
  }

  /** Drop the assignment: worker stopped, relay closed, nothing reported. */
  release(why = 'released') {
    if (this.phase === 'idle' && !this.proc) return
    this.generation++
    this.closeRelay()
    this.stopWorker()
    if (this.held) this.log(`expert host: released layer ${this.held.layer} experts ${this.held.begin}-${this.held.end} (${why})`)
    this.held = null
    this.session = null
    this.port = null
    this.phase = 'idle'
  }

  status() {
    return {
      phase: this.phase,
      held: this.held ? { model: this.held.model, layer: this.held.layer, begin: this.held.begin, end: this.held.end } : null,
      port: this.port,
      session: this.session,
      relay: { connected: this.relay.connected, bytes: this.relay.bytes, lastClose: this.relay.lastClose },
      lastError: this.lastError,
    }
  }
}

function canConnect(port) {
  return new Promise((resolve) => {
    const s = net.connect(port, '127.0.0.1')
    s.once('connect', () => { s.destroy(); resolve(true) })
    s.once('error', () => resolve(false))
  })
}

/**
 * Several ExpertHosts — slots — so a machine can hold more than one shard.
 *
 * One worker serves one layer's shard of at most 64 experts, so a machine
 * lending, say, 4 GiB holds its share as several slots. Each slot is its own
 * identity to the bridge: worker id, relay session, coverage post. Slot 0 is
 * the machine's plain id (desktop-XXXXXXXX), so a single-slot machine looks
 * exactly as it did before; further slots are <id>-2, <id>-3, … All report the
 * same owner wallet, so rewards land in one place.
 *
 * Separate identities are what make growing safe: a held slot never asks for
 * work again (the bridge would hand that id a different range every poll),
 * while the next slot volunteers as a newcomer.
 */
class ExpertPool {
  /**
   * @param {object} o
   * @param {(index: number) => ExpertHost} o.makeHost  builds slot `index`
   * @param {() => string} o.workerId    the machine's id (slot 0's)
   * @param {(heldCounts: number[]) => number} o.nextWindow  experts to ask for next, 0 = none
   * @param {(heldCounts: number[]) => number} o.fits  how many held slots still fit
   */
  constructor({ makeHost, workerId, nextWindow, fits }) {
    this.makeHost = makeHost
    this.workerIdFn = workerId
    this.nextWindowFn = nextWindow
    this.fitsFn = fits
    this.slots = []
  }

  slot(i) {
    while (this.slots.length <= i) this.slots.push(this.makeHost(this.slots.length))
    return this.slots[i]
  }

  slotId(i) {
    const base = this.workerIdFn()
    return i === 0 ? base : `${base}-${i + 1}`
  }

  canHost() { return this.slot(0).canHost() }

  /** Held or being fetched, in slot order. */
  busySlots() { return this.slots.filter((s) => s.busy()) }

  heldCounts() { return this.busySlots().map((s) => s.heldExperts()) }

  heldExperts() { return this.heldCounts().reduce((a, b) => a + b, 0) }

  /** Experts to volunteer for now; 0 while a slot is still being set up. */
  nextWindow() {
    if (this.slots.some((s) => s.phase === 'downloading' || s.phase === 'starting')) return 0
    return this.nextWindowFn(this.heldCounts())
  }

  /** Release the newest slots the budget no longer covers. */
  trim(why = 'the VRAM budget no longer fits it') {
    const busy = this.busySlots()
    const keep = this.fitsFn(busy.map((s) => s.heldExperts()))
    for (let i = busy.length - 1; i >= keep; i--) busy[i].release(why)
  }

  /** Put an assignment in the first free slot. */
  provision(assignment) {
    let i = 0
    while (this.slot(i).busy()) i++
    return this.slot(i).provision(assignment, { workerId: this.slotId(i) })
  }

  /**
   * One coverage report per slot to send this tick. Slot 0 always reports,
   * holding or not — that is what keeps the machine in the census; a free
   * slot beyond it has nothing to say.
   */
  reports() {
    const out = []
    this.slot(0)
    this.slots.forEach((s, i) => {
      if (i === 0 || s.busy()) {
        out.push({ workerId: this.slotId(i), segments: s.heldSegments(), url: s.coverageUrl(),
          model: s.held ? s.held.model : null })
      }
    })
    return out
  }

  /** Compatibility with single-host callers. */
  heldSegments() { return this.slots.flatMap((s) => s.heldSegments()) }

  release(why = 'released') { for (const s of this.slots) s.release(why) }

  status() {
    const slots = this.slots.map((s, i) => ({ workerId: this.slotId(i), ...s.status() }))
    const serving = slots.filter((s) => s.phase === 'serving')
    return {
      phase: serving.length ? 'serving' : (slots.find((s) => s.phase !== 'idle') || { phase: 'idle' }).phase,
      slots,
      heldExperts: this.heldExperts(),
      servingSlots: serving.length,
    }
  }
}

module.exports = { ExpertHost, ExpertPool, ExpertHostError, readGgufMetadata }
