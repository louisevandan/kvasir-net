'use strict'
/**
 * The CUDA expert-worker pack: downloaded on demand, never bundled.
 *
 * The Windows CUDA worker carries its GPU kernels for every architecture from
 * Pascal to Blackwell, which makes it large — and useless to anyone without
 * an NVIDIA GPU or who never hosts experts. So it ships as a versioned pack,
 * fetched only when this machine has a GPU that can run it and the operator
 * asks for it. (It needs no NVIDIA DLLs: cuBLAS is delay-loaded and never
 * called — see scripts/build-expert-worker.cjs — and cudart is static.)
 *
 * Trust comes from the app, not from the server the pack is on: the expected
 * SHA-256 is compiled in (PACKS below). A hash file next to the archive would
 * prove nothing about an archive an attacker could replace alongside it.
 *
 * Install is atomic: download to <dir>/<version>.zip.part (resumable with a
 * Range request), verify the hash, extract into <dir>/<version>.tmp — refusing
 * any entry that would land outside it — then rename to <dir>/<version>. A
 * half-written pack is never where the executor probe looks.
 */
const fs = require('node:fs')
const path = require('node:path')
const crypto = require('node:crypto')
const zlib = require('node:zlib')
const { execFile } = require('node:child_process')

/**
 * One entry per platform. `url` is null until the pack is published; the UI
 * then says so instead of offering a download that cannot work.
 */
const PACKS = {
  win32: {
    version: '2026.09.23-99afdb2a',
    worker: 'linkcpp-expert-worker.exe',
    // cuBLAS + FP32 for wide batches, ggml's vector kernels for T <= 8. The
    // arch list starts at 6.1; older GPUs are refused before any download.
    minComputeCapability: 6.1,
    minCudaMajor: 12,
    parts: [
      {
        id: 'runtime',
        // Content hash of the two DLLs. A rebuild against the same CUDA makes
        // the same version but a different zip (entry timestamps move), so the
        // published zip stays pinned and is never re-uploaded: that would
        // invalidate every installed runtime for a header change.
        version: 'cublas12-2d5628eb470e',
        url: 'https://pub-3fa7c08233cd497dbd39f89a9093c965.r2.dev/expert-worker/kvasir-cuda-runtime-win-x64-cublas12-2d5628eb470e.zip',
        sha256: 'f8b2d46f6238b8b16d5c0b86be567ec79035f2a2821318e9d329a2d0672acba9',
        bytes: 552_860_542,
        files: ['cublas64_12.dll', 'cublasLt64_12.dll'],
      },
      {
        id: 'worker',
        version: '2026.09.23-99afdb2a',
        url: 'https://pub-3fa7c08233cd497dbd39f89a9093c965.r2.dev/expert-worker/kvasir-expert-worker-win-x64-cuda12-2026.09.23-99afdb2a.zip',
        sha256: '4a398324bd6b0f5fef6c82f55b2607e2c79c0ee4e1516be82172f080dcacc5bd',
        bytes: 167_463_599,
        files: ['linkcpp-expert-worker.exe'],
      },
    ],
  },
}

const run = (cmd, args) => new Promise((resolve) => {
  execFile(cmd, args, { timeout: 5000, windowsHide: true }, (err, stdout) => resolve(err ? null : String(stdout)))
})

/**
 * Can this machine run the pack at all? Checked before anything is
 * downloaded: a large download for a GPU that cannot load the kernels is the
 * worst way to find out.
 */
async function eligibility(pack = PACKS[process.platform]) {
  if (!pack) return { ok: false, reason: `no CUDA pack is built for ${process.platform}` }
  const caps = await run('nvidia-smi', ['--query-gpu=name,compute_cap,driver_version', '--format=csv,noheader'])
  if (!caps) return { ok: false, reason: 'no NVIDIA driver responding (nvidia-smi not found)' }
  const banner = await run('nvidia-smi', [])
  const cuda = banner && (banner.match(/CUDA Version:\s*([0-9]+)\.([0-9]+)/) || [])
  const cudaMajor = cuda && cuda[1] ? Number(cuda[1]) : null
  const gpus = caps.trim().split(/\r?\n/).map((l) => {
    const [name, cc, driver] = l.split(',').map((s) => s.trim())
    return { name, computeCapability: Number(cc), driver }
  })
  const usable = gpus.filter((g) => g.computeCapability >= pack.minComputeCapability)
  if (!usable.length) {
    return { ok: false, gpus, reason: `GPU compute capability ${gpus.map((g) => g.computeCapability).join(', ')} is below ${pack.minComputeCapability}` }
  }
  if (cudaMajor == null || cudaMajor < pack.minCudaMajor) {
    return { ok: false, gpus, reason: `the NVIDIA driver supports CUDA ${cudaMajor ?? 'unknown'}; this pack needs CUDA ${pack.minCudaMajor} — update the GPU driver` }
  }
  return { ok: true, gpus, cudaMajor }
}

// ---- zip (stored + deflate, no zip64: the pack is well under 4 GiB) --------

function readZipEntries(file) {
  const fd = fs.openSync(file, 'r')
  try {
    const size = fs.fstatSync(fd).size
    const tailLen = Math.min(size, 65_557)
    const tail = Buffer.alloc(tailLen)
    fs.readSync(fd, tail, 0, tailLen, size - tailLen)
    const eocd = tail.lastIndexOf(Buffer.from([0x50, 0x4b, 0x05, 0x06]))
    if (eocd < 0) throw new Error('not a zip file (no end of central directory)')
    const count = tail.readUInt16LE(eocd + 10)
    const cdSize = tail.readUInt32LE(eocd + 12)
    const cdOffset = tail.readUInt32LE(eocd + 16)
    const cd = Buffer.alloc(cdSize)
    fs.readSync(fd, cd, 0, cdSize, cdOffset)
    const entries = []
    let o = 0
    for (let i = 0; i < count; i++) {
      if (cd.readUInt32LE(o) !== 0x02014b50) throw new Error('corrupt central directory')
      const method = cd.readUInt16LE(o + 10)
      const crc = cd.readUInt32LE(o + 16)
      const compressed = cd.readUInt32LE(o + 20)
      const uncompressed = cd.readUInt32LE(o + 24)
      const nameLen = cd.readUInt16LE(o + 28)
      const extraLen = cd.readUInt16LE(o + 30)
      const commentLen = cd.readUInt16LE(o + 32)
      const localOffset = cd.readUInt32LE(o + 42)
      const name = cd.toString('utf8', o + 46, o + 46 + nameLen)
      entries.push({ name, method, crc, compressed, uncompressed, localOffset })
      o += 46 + nameLen + extraLen + commentLen
    }
    return entries
  } finally { fs.closeSync(fd) }
}

/** Extract into `dest`; any entry that would escape it aborts the whole install. */
async function extractZip(file, dest) {
  const entries = readZipEntries(file)
  const root = path.resolve(dest)
  for (const e of entries) {
    const target = path.resolve(root, e.name)
    if (target !== root && !target.startsWith(root + path.sep)) throw new Error(`zip entry escapes the install dir: ${e.name}`)
  }
  const fd = fs.openSync(file, 'r')
  try {
    for (const e of entries) {
      const target = path.resolve(root, e.name)
      if (e.name.endsWith('/')) { fs.mkdirSync(target, { recursive: true }); continue }
      fs.mkdirSync(path.dirname(target), { recursive: true })
      const local = Buffer.alloc(30)
      fs.readSync(fd, local, 0, 30, e.localOffset)
      if (local.readUInt32LE(0) !== 0x04034b50) throw new Error(`corrupt local header for ${e.name}`)
      const start = e.localOffset + 30 + local.readUInt16LE(26) + local.readUInt16LE(28)
      const input = fs.createReadStream(file, { start, end: start + e.compressed - 1 })
      const output = fs.createWriteStream(target)
      const { pipeline } = require('node:stream/promises')
      if (e.method === 0) await pipeline(input, output)
      else if (e.method === 8) await pipeline(input, zlib.createInflateRaw(), output)
      else throw new Error(`unsupported zip method ${e.method} for ${e.name}`)
      const got = fs.statSync(target).size
      if (got !== e.uncompressed) throw new Error(`${e.name}: extracted ${got} bytes, expected ${e.uncompressed}`)
    }
  } finally { fs.closeSync(fd) }
}

/**
 * A pack is one or more parts, each its own zip with its own pinned hash:
 *
 *   runtime  the NVIDIA libraries (cuBLAS). Hundreds of MB, versioned by the
 *            CUDA release, so fetched once and kept across worker updates.
 *   worker   our binary. Changes with every worker fix.
 *
 * Splitting them means an update to the worker does not make a volunteer
 * download the NVIDIA libraries again: the size that loses installs on home
 * connections is paid once. Parts install into <dir>/<id>/<version>/; the
 * runtime's files are then hard-linked (copied if linking is refused) next to
 * the worker, where Windows looks for a DLL first, so nothing depends on PATH.
 *
 * A single-zip pack (url/sha256/bytes at the top level, as first published)
 * is read as one worker part at <dir>/<version>, so an installed older pack
 * keeps working.
 */
function partsOf(pack) {
  if (!pack) return []
  if (Array.isArray(pack.parts)) return pack.parts
  return pack.url || pack.sha256
    ? [{ id: 'worker', version: pack.version, url: pack.url, sha256: pack.sha256, bytes: pack.bytes, files: [pack.worker] }]
    : []
}

class CudaPack {
  /**
   * @param {object} o
   * @param {string} o.dir     app-owned install root, e.g. <userData>/cuda-pack
   * @param {object} [o.pack]  defaults to this platform's PACKS entry
   * @param {Function} [o.fetch]
   * @param {(l: string) => void} [o.log]
   * @param {Function} [o.check]  eligibility check, injectable for tests
   */
  constructor({ dir, pack = PACKS[process.platform] || null, fetch = null, log = () => {}, check = eligibility }) {
    this.dir = dir
    this.pack = pack
    this.parts = partsOf(pack)
    this.split = Boolean(pack && Array.isArray(pack.parts))
    this.fetch = fetch || globalThis.fetch
    this.log = log
    this.check = check
    this.state = { phase: 'idle', received: 0, total: this.missingBytes(), error: null }
    this.abort = null
  }

  partDir(part) {
    return this.split ? path.join(this.dir, part.id, part.version) : path.join(this.dir, part.version)
  }

  partInstalled(part) {
    const d = this.partDir(part)
    return (part.files || []).every((f) => fs.existsSync(path.join(d, f)))
  }

  workerPart() { return this.parts.find((p) => p.id === 'worker') || null }

  missingBytes() { return this.parts.filter((p) => !this.partInstalled(p)).reduce((s, p) => s + (p.bytes || 0), 0) }

  /** The worker binary, if every part is installed and the runtime sits beside it. */
  workerPath() {
    const w = this.workerPart()
    if (!w || !this.parts.every((p) => this.partInstalled(p))) return null
    const dir = this.partDir(w)
    for (const p of this.parts) {
      if (p === w) continue
      for (const f of p.files || []) if (!fs.existsSync(path.join(dir, f))) return null
    }
    const exe = path.join(dir, this.pack.worker)
    return fs.existsSync(exe) ? exe : null
  }

  status() {
    return {
      available: this.parts.length > 0 && this.parts.every((p) => p.url),
      version: this.pack ? this.pack.version : null,
      // What installing would download now. A worker update after the
      // runtime is in place is only the worker part.
      bytes: this.missingBytes(),
      installed: Boolean(this.workerPath()),
      ...this.state,
    }
  }

  cancel() { if (this.abort) this.abort.abort() }

  async install() {
    const pack = this.pack
    if (!pack || !this.parts.length) throw new Error(`no CUDA pack for ${process.platform}`)
    if (!this.parts.every((p) => p.url)) throw new Error('the CUDA pack has not been published yet')
    if (this.workerPath()) return this.workerPath()
    if (['downloading', 'verifying', 'installing'].includes(this.state.phase)) throw new Error('already installing')
    const elig = await this.check(pack)
    if (!elig.ok) { this.state = { ...this.state, phase: 'failed', error: elig.reason }; throw new Error(elig.reason) }
    fs.mkdirSync(this.dir, { recursive: true })
    const todo = this.parts.filter((p) => !this.partInstalled(p))
    this.state = { phase: 'downloading', received: 0, total: todo.reduce((s, p) => s + p.bytes, 0), error: null }
    let base = 0
    try {
      for (const part of todo) {
        await this.installPart(part, base)
        base += part.bytes
      }
      this.linkRuntime()
      if (!this.workerPath()) throw new Error('the pack installed but its worker is not complete')
      this.state = { phase: 'installed', received: this.state.total, total: this.state.total, error: null }
      this.log(`cuda pack: installed ${pack.version}`)
      this.prune()
      return this.workerPath()
    } catch (e) {
      this.state = { ...this.state, phase: 'failed', error: e.name === 'AbortError' ? 'cancelled' : e.message }
      this.log(`cuda pack: ${this.state.error}`)
      throw e
    } finally { this.abort = null }
  }

  async installPart(part, base) {
    const zip = path.join(this.dir, `${part.id}-${part.version}.zip.part`)
    const tmp = `${this.partDir(part)}.tmp`
    try {
      this.state.phase = 'downloading'
      await this.download(part, zip, base)
      this.state.phase = 'verifying'
      const digest = await sha256File(zip)
      if (digest !== part.sha256) {
        fs.rmSync(zip, { force: true })   // a bad archive must not be resumed from
        throw new Error(`the downloaded ${part.id} does not match its expected hash (${digest.slice(0, 12)}...)`)
      }
      this.state.phase = 'installing'
      fs.rmSync(tmp, { recursive: true, force: true })
      await extractZip(zip, tmp)
      for (const f of part.files || []) {
        if (!fs.existsSync(path.join(tmp, f))) throw new Error(`the ${part.id} archive has no ${f}`)
      }
      fs.mkdirSync(path.dirname(this.partDir(part)), { recursive: true })
      fs.rmSync(this.partDir(part), { recursive: true, force: true })
      fs.renameSync(tmp, this.partDir(part))
      fs.rmSync(zip, { force: true })
    } catch (e) {
      fs.rmSync(tmp, { recursive: true, force: true })
      throw e
    }
  }

  /** Put the runtime's files beside the worker: a hard link, or a copy if refused. */
  linkRuntime() {
    const w = this.workerPart()
    const dest = this.partDir(w)
    for (const p of this.parts) {
      if (p === w) continue
      for (const f of p.files || []) {
        const to = path.join(dest, f)
        if (fs.existsSync(to)) continue
        const from = path.join(this.partDir(p), f)
        try { fs.linkSync(from, to) } catch { fs.copyFileSync(from, to) }
      }
    }
  }

  /**
   * Remove versions this pack no longer names, including a first-generation
   * single-zip install at <dir>/<version>. Best effort: a worker still
   * running from an old version holds its files open on Windows, and those go
   * on a later install instead.
   */
  prune() {
    if (!this.split) return
    const keep = new Set(this.parts.map((p) => path.resolve(this.partDir(p))))
    const ids = new Set(this.parts.map((p) => p.id))
    const candidates = []
    for (const id of ids) {
      let names = []
      try { names = fs.readdirSync(path.join(this.dir, id)) } catch { continue }
      for (const n of names) candidates.push(path.resolve(this.dir, id, n))
    }
    // Legacy top-level version directories (anything but a part id or a download).
    try {
      for (const n of fs.readdirSync(this.dir)) {
        if (!ids.has(n) && !n.endsWith('.part')) candidates.push(path.resolve(this.dir, n))
      }
    } catch { /* no dir */ }
    for (const d of candidates) {
      if (keep.has(d)) continue
      try { if (fs.statSync(d).isDirectory()) fs.rmSync(d, { recursive: true, force: true }) } catch { /* in use; next time */ }
    }
  }

  /** Resumes a previous .part with a Range request when the server allows it. */
  async download(part, dest, base = 0) {
    let have = fs.existsSync(dest) ? fs.statSync(dest).size : 0
    if (have > part.bytes) { fs.rmSync(dest); have = 0 }
    if (have === part.bytes) { this.state.received = base + have; return }
    this.abort = new AbortController()
    const headers = { 'User-Agent': 'kvasir-wallet-desktop' }
    if (have) headers.Range = `bytes=${have}-`
    const res = await this.fetch(part.url, { headers, signal: this.abort.signal })
    if (res.status === 200 && have) { have = 0 }             // server ignored the range: start over
    else if (!(res.status === 200 || res.status === 206)) throw new Error(`${part.id} download answered ${res.status}`)
    const out = fs.createWriteStream(dest, { flags: have ? 'a' : 'w' })
    let got = have
    this.state.received = base + got
    try {
      for await (const chunk of res.body) {
        got += chunk.length
        this.state.received = base + got
        if (!out.write(chunk)) await new Promise((r) => out.once('drain', r))
      }
    } finally { await new Promise((r) => out.end(r)) }
    if (got !== part.bytes) throw new Error(`download ended at ${got} of ${part.bytes} bytes; try again to resume`)
  }
}

function sha256File(file) {
  return new Promise((resolve, reject) => {
    const h = crypto.createHash('sha256')
    fs.createReadStream(file).on('data', (d) => h.update(d)).on('error', reject).on('end', () => resolve(h.digest('hex')))
  })
}

module.exports = { CudaPack, PACKS, partsOf, eligibility, extractZip, readZipEntries, sha256File }
