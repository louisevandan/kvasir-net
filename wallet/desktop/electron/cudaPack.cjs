'use strict'
/**
 * The CUDA expert-worker pack: downloaded on demand, never bundled.
 *
 * The Windows CUDA worker needs cuBLAS beside it — about 770 MB of NVIDIA
 * DLLs. Bundling them would make every installer ~9x larger, including for
 * the people with no NVIDIA GPU and the people who never host experts. So the
 * worker and its runtime ship as one versioned pack, fetched only when this
 * machine has a GPU that can run it and the operator asks for it.
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
    version: '2026.09.22-4171aef7',
    url: null,
    sha256: 'bcdb1bcbe39f598065680b014b1fd0e826e6928c2a7f82c63a6318666145de98',
    bytes: 731_114_203,
    worker: 'linkcpp-expert-worker.exe',
    // ggml's CUDA arch list for this build starts at sm_50 (as PTX).
    minComputeCapability: 5.0,
    minCudaMajor: 12,
  },
}

const run = (cmd, args) => new Promise((resolve) => {
  execFile(cmd, args, { timeout: 5000, windowsHide: true }, (err, stdout) => resolve(err ? null : String(stdout)))
})

/**
 * Can this machine run the pack at all? Checked before anything is
 * downloaded: 731 MB for a GPU that cannot load the kernels is the worst way
 * to find out.
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
    this.fetch = fetch || globalThis.fetch
    this.log = log
    this.check = check
    this.state = { phase: 'idle', received: 0, total: pack ? pack.bytes : 0, error: null }
    this.abort = null
  }

  installDir() { return this.pack ? path.join(this.dir, this.pack.version) : null }

  /** The worker binary, if this version is fully installed. */
  workerPath() {
    const d = this.installDir()
    if (!d) return null
    const p = path.join(d, this.pack.worker)
    return fs.existsSync(p) ? p : null
  }

  status() {
    return {
      available: Boolean(this.pack && this.pack.url),
      version: this.pack ? this.pack.version : null,
      bytes: this.pack ? this.pack.bytes : 0,
      installed: Boolean(this.workerPath()),
      ...this.state,
    }
  }

  cancel() { if (this.abort) this.abort.abort() }

  async install() {
    const pack = this.pack
    if (!pack) throw new Error(`no CUDA pack for ${process.platform}`)
    if (!pack.url) throw new Error('the CUDA pack has not been published yet')
    if (this.workerPath()) return this.workerPath()
    if (this.state.phase === 'downloading' || this.state.phase === 'installing') throw new Error('already installing')
    const elig = await this.check(pack)
    if (!elig.ok) { this.state = { ...this.state, phase: 'failed', error: elig.reason }; throw new Error(elig.reason) }
    fs.mkdirSync(this.dir, { recursive: true })
    const zip = path.join(this.dir, `${pack.version}.zip.part`)
    const tmp = path.join(this.dir, `${pack.version}.tmp`)
    try {
      this.state = { phase: 'downloading', received: 0, total: pack.bytes, error: null }
      await this.download(pack, zip)
      this.state.phase = 'verifying'
      const digest = await sha256File(zip)
      if (digest !== pack.sha256) {
        fs.rmSync(zip, { force: true })   // a bad archive must not be resumed from
        throw new Error(`the downloaded pack does not match its expected hash (${digest.slice(0, 12)}…)`)
      }
      this.state.phase = 'installing'
      fs.rmSync(tmp, { recursive: true, force: true })
      await extractZip(zip, tmp)
      if (!fs.existsSync(path.join(tmp, pack.worker))) throw new Error('the pack has no worker binary')
      fs.rmSync(this.installDir(), { recursive: true, force: true })
      fs.renameSync(tmp, this.installDir())
      fs.rmSync(zip, { force: true })
      this.state = { phase: 'installed', received: pack.bytes, total: pack.bytes, error: null }
      this.log(`cuda pack: installed ${pack.version}`)
      return this.workerPath()
    } catch (e) {
      fs.rmSync(tmp, { recursive: true, force: true })
      this.state = { ...this.state, phase: 'failed', error: e.name === 'AbortError' ? 'cancelled' : e.message }
      this.log(`cuda pack: ${this.state.error}`)
      throw e
    } finally { this.abort = null }
  }

  /** Resumes a previous .part with a Range request when the server allows it. */
  async download(pack, dest) {
    let have = fs.existsSync(dest) ? fs.statSync(dest).size : 0
    if (have > pack.bytes) { fs.rmSync(dest); have = 0 }
    if (have === pack.bytes) { this.state.received = have; return }
    this.abort = new AbortController()
    const headers = { 'User-Agent': 'kvasir-wallet-desktop' }
    if (have) headers.Range = `bytes=${have}-`
    const res = await this.fetch(pack.url, { headers, signal: this.abort.signal })
    if (res.status === 200 && have) { have = 0 }             // server ignored the range: start over
    else if (!(res.status === 200 || res.status === 206)) throw new Error(`pack download answered ${res.status}`)
    const out = fs.createWriteStream(dest, { flags: have ? 'a' : 'w' })
    this.state.received = have
    try {
      for await (const chunk of res.body) {
        this.state.received += chunk.length
        if (!out.write(chunk)) await new Promise((r) => out.once('drain', r))
      }
    } finally { await new Promise((r) => out.end(r)) }
    if (this.state.received !== pack.bytes) throw new Error(`download ended at ${this.state.received} of ${pack.bytes} bytes — try again to resume`)
  }
}

function sha256File(file) {
  return new Promise((resolve, reject) => {
    const h = crypto.createHash('sha256')
    fs.createReadStream(file).on('data', (d) => h.update(d)).on('error', reject).on('end', () => resolve(h.digest('hex')))
  })
}

module.exports = { CudaPack, PACKS, eligibility, extractZip, readZipEntries, sha256File }
