#!/usr/bin/env node
'use strict'
/**
 * kvasir-node — a headless expert node for Linux servers.
 *
 * The desktop app's node is three modules with no Electron in them:
 * participation.cjs (the bridge market), expertHost.cjs (shard, worker,
 * relay — as a pool of slots) and executors.cjs (which worker, what it costs).
 * This is those three, wired to a key file and a budget instead of a wallet
 * screen and a slider. They are required from wallet/desktop, not copied, so
 * the desktop and the server run the same code.
 *
 *   kvasir-node keygen --out ~/.config/kvasir/node-key.json
 *   kvasir-node run --key ~/.config/kvasir/node-key.json --budget 24 \
 *       --worker /opt/kvasir/linkcpp-expert-worker
 *
 * The key is a Solana CLI keypair file (a JSON array of 64 bytes, what
 * `solana-keygen new` writes). Rewards go to its address. It is read from a
 * file on purpose: a key or mnemonic on the command line ends up in shell
 * history and in `ps` for every user on the machine.
 */
const crypto = require('node:crypto')
const fs = require('node:fs')
const os = require('node:os')
const path = require('node:path')
const { execFileSync } = require('node:child_process')
const nacl = require('tweetnacl')
const WebSocket = require('ws')

// Two layouts, one resolution. In a checkout the market/shard/worker logic is
// required straight out of wallet/desktop/electron so a server and a desktop
// provably run the same code. A published tarball has no desktop app in it, so
// the packager copies those three files to lib/ beside this one. Checking for
// the packaged layout first means an installed node never reaches back toward a
// repo that is not there.
const PACKAGED = path.join(__dirname, 'lib')
const DESKTOP = fs.existsSync(path.join(PACKAGED, 'participation.cjs'))
  ? PACKAGED
  : path.join(__dirname, '..', 'wallet', 'desktop', 'electron')
const { Participation } = require(path.join(DESKTOP, 'participation.cjs'))
const { ExpertHost, ExpertPool } = require(path.join(DESKTOP, 'expertHost.cjs'))
const executorsMod = require(path.join(DESKTOP, 'executors.cjs'))

const GATEWAY = 'https://gate.kvasir-ai.net'
const GIB = 1024 ** 3

// ---- base58 (Solana addresses) ----------------------------------------------

const B58 = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz'
function base58(bytes) {
  let n = BigInt('0x' + (Buffer.from(bytes).toString('hex') || '0'))
  let out = ''
  while (n > 0n) { out = B58[Number(n % 58n)] + out; n /= 58n }
  for (const b of bytes) { if (b === 0) out = '1' + out; else break }
  return out
}

// ---- key file ------------------------------------------------------------------

/** A Solana CLI keypair file: JSON array of 64 bytes (secret || public). */
function loadKey(file) {
  const st = fs.statSync(file)
  if (process.platform !== 'win32' && (st.mode & 0o077)) {
    throw new Error(`${file} is readable by other users (mode ${(st.mode & 0o777).toString(8)}); chmod 600 it`)
  }
  const raw = JSON.parse(fs.readFileSync(file, 'utf8'))
  if (!Array.isArray(raw) || raw.length !== 64) throw new Error(`${file} is not a 64-byte keypair file`)
  const kp = nacl.sign.keyPair.fromSecretKey(Uint8Array.from(raw))
  return { secretKey: kp.secretKey, address: base58(kp.publicKey) }
}

function keygen(file) {
  if (fs.existsSync(file)) throw new Error(`${file} exists; refusing to overwrite a key`)
  fs.mkdirSync(path.dirname(file), { recursive: true, mode: 0o700 })
  const kp = nacl.sign.keyPair()
  fs.writeFileSync(file, JSON.stringify(Array.from(kp.secretKey)), { mode: 0o600 })
  return base58(kp.publicKey)
}

// ---- node token store ------------------------------------------------------

/**
 * The 30-day node token, as the desktop keeps it but without an OS keystore:
 * a 0600 file, dropped when it belongs to a different wallet (a new key must
 * never reuse a token minted for the old one).
 */
function tokenStore(file, address) {
  return {
    load() {
      try {
        const rec = JSON.parse(fs.readFileSync(file, 'utf8'))
        return rec && rec.wallet === address() ? rec : null
      } catch { return null }
    },
    save(rec) {
      if (!rec) { fs.rmSync(file, { force: true }); return }
      fs.mkdirSync(path.dirname(file), { recursive: true, mode: 0o700 })
      fs.writeFileSync(file, JSON.stringify(rec), { mode: 0o600 })
    },
  }
}

// ---- budget ------------------------------------------------------------------

/**
 * Bytes to lend. Explicit --budget wins. Otherwise half of what the GPU
 * reports — but a unified-memory machine (GB10, Apple) may report no card
 * total at all, and guessing there would lend the host's own RAM without
 * asking, so that case requires --budget.
 */
async function resolveBudget(opts, gpuReadiness = executorsMod.gpuReadiness) {
  if (opts.budget != null) {
    const gib = Number(opts.budget)
    if (!(gib > 0)) throw new Error(`--budget must be a positive number of GiB, got ${opts.budget}`)
    return Math.floor(gib * GIB)
  }
  const gpu = await gpuReadiness()
  const total = gpu && gpu.gpus && gpu.gpus[0] && gpu.gpus[0].totalBytes
  if (Number.isFinite(total) && total > 0) return Math.floor(total / 2)
  throw new Error('the GPU reports no memory size (unified memory?); say how much to lend with --budget <GiB>')
}

function slug(s) {
  return String(s).toLowerCase().replace(/[^a-z0-9-]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 24) || 'host'
}

// ---- run ---------------------------------------------------------------------

/**
 * Build the node. Split from main() so tests can drive it against a loopback
 * bridge with a fake worker.
 */
function createNode(opts) {
  const key = loadKey(opts.key)
  const dataDir = opts.data || path.join(os.homedir(), '.local', 'share', 'kvasir-node')
  const gateway = (opts.gateway || GATEWAY).replace(/\/+$/, '')
  // Distinct per machine: two servers on one wallet must not share an id, or
  // each would overwrite the other's census entry and relay session.
  const workerId = `server-${key.address.slice(0, 8)}-${slug(opts.name || os.hostname())}`
  const log = opts.log || ((line) => console.log(`${new Date().toISOString()} ${line}`))
  const { entry } = executorsMod.expertExecutor()
  const model = opts.memoryModel || (entry && entry.memoryModel) || null
  const workerBinary = () => {
    // In order: what the operator named, the environment, what `worker
    // --install` put on disk, and finally a build executors.cjs knows how to
    // find in a checkout. The installed copy comes before the checkout so a
    // server that was set up by the installer never depends on a repo.
    const installed = path.join(WORKER_DIR, 'linkcpp-expert-worker')
    return opts.worker
      || process.env.KVASIR_EXPERT_WORKER
      || (fs.existsSync(installed) ? installed : null)
      || executorsMod.expertExecutor().path
  }
  const budget = opts.budgetBytes
  let participation = null
  const pool = new ExpertPool({
    makeHost: () => new ExpertHost({
      baseUrl: gateway,
      token: () => participation.ensureToken(),
      workerBinary,
      workerArgs: opts.workerArgs || [],
      shardDir: path.join(dataDir, 'shards'),
      log,
      WebSocket,   // this package's ws, so the server needs no desktop node_modules
    }),
    workerId: () => workerId,
    // Unified memory has no separate "free VRAM"; the budget is the limit.
    nextWindow: (held) => executorsMod.nextSlotWindow(budget, model, null, held),
    fits: (held) => executorsMod.slotsThatFit(budget, model, null, held),
  })
  participation = new Participation({
    baseUrl: gateway,
    wallet: () => key.address,
    sign: (bytes) => nacl.sign.detached(Uint8Array.from(bytes), key.secretKey),
    store: tokenStore(path.join(dataDir, 'node-token.json'), () => key.address),
    workerId: () => workerId,
    log,
    host: pool,
    // Recorded in the ledger as a server, not guessed. A Mac runs the Metal
    // worker; everything else here is CUDA.
    platform: {
      os: { darwin: 'macos', linux: 'linux', win32: 'windows' }[process.platform] || process.platform,
      device_kind: 'server',
      accelerator: 'gpu',
      backend: process.platform === 'darwin' ? 'metal' : 'cuda',
    },
  })
  return { participation, pool, workerId, address: key.address, gateway, workerBinary, model, budget, log }
}

// Flags that stand alone. Everything else takes a value, and a missing one is
// an error rather than a silent `true` — `--budget --name x` should not quietly
// lend an undefined amount of memory.
const BARE_FLAGS = new Set(['install', 'help'])

function parseArgs(argv) {
  const [cmd, ...rest] = argv
  const opts = {}
  for (let i = 0; i < rest.length; i++) {
    const a = rest[i]
    if (!a.startsWith('--')) throw new Error(`unexpected argument ${a}`)
    const k = a.slice(2)
    if (BARE_FLAGS.has(k)) { opts[k] = true; continue }
    const v = rest[i + 1]
    if (v == null || v.startsWith('--')) throw new Error(`--${k} needs a value`)
    opts[k] = v
    i++
  }
  return { cmd, opts }
}

const USAGE = `usage:
  kvasir-node keygen --out <file>
  kvasir-node address --key <file>
  kvasir-node worker --install [--dir <dir>]
  kvasir-node run --key <file> [--budget <GiB>] [--worker <path>] [--name <id>]
                  [--gateway <url>] [--data <dir>] [--poll <seconds>]

  --key      Solana CLI keypair file (JSON, 64 bytes), mode 600. Rewards go here.
  --budget   GPU memory to lend, in GiB. Required on unified-memory machines.
  --worker   linkcpp-expert-worker binary (or KVASIR_EXPERT_WORKER).
  --name     this machine in the node id (default: hostname).
`

/** Where a downloaded worker lives, and where `run` looks for one. */
const WORKER_DIR = path.join(os.homedir(), '.local', 'share', 'kvasir-node', 'worker')

const DOWNLOAD_BASE = (process.env.KVASIR_DOWNLOAD_BASE
  || 'https://pub-3fa7c08233cd497dbd39f89a9093c965.r2.dev/node').replace(/\/+$/, '')

function fetchJson(url) {
  return fetch(url, { redirect: 'follow' }).then((r) => {
    if (!r.ok) throw new Error(`${url} answered ${r.status}`)
    return r.json()
  })
}

/**
 * Fetch the expert worker for this machine.
 *
 * It is a separate download from the node itself for the same reason the
 * desktop app downloads its CUDA pack rather than shipping it: the compute
 * binary is specific to the GPU vendor and the instruction set, and carrying
 * every combination would make a 1 MB install a several-hundred-MB one for
 * machines that will use one of them.
 *
 * The hash is checked. This writes an executable that will be handed model
 * weights and run on the operator's GPU.
 */
async function installWorker(opts) {
  const arch = { x64: 'x64', arm64: 'arm64' }[process.arch]
  if (process.platform !== 'linux' || !arch) {
    throw new Error(`no worker build for ${process.platform}-${process.arch}`)
  }
  const index = await fetchJson(`${DOWNLOAD_BASE}/latest.json`)
  const key = `worker-linux-${arch}`
  const name = index[key]
  const want = index[`${key}-sha256`]
  if (!name) {
    throw new Error(
      `no expert worker has been published for linux-${arch} yet.\n`
      + 'Until one is, build it from apps/linkcpp-expert-worker in the repository\n'
      + 'and point the node at it with --worker <path>. The node runs fine that way;\n'
      + 'this command only saves you the build.')
  }
  if (!want) throw new Error(`the index has no checksum for ${name}; refusing to install it`)

  const dir = opts.dir || WORKER_DIR
  fs.mkdirSync(dir, { recursive: true })
  const url = `${DOWNLOAD_BASE}/${name}`
  console.log(`downloading ${url}`)
  const reply = await fetch(url, { redirect: 'follow' })
  if (!reply.ok) throw new Error(`${url} answered ${reply.status}`)
  const bytes = Buffer.from(await reply.arrayBuffer())
  const got = crypto.createHash('sha256').update(bytes).digest('hex')
  if (got !== want) throw new Error(`checksum mismatch: expected ${want}, got ${got}`)

  const archive = path.join(dir, name.split('/').pop())
  fs.writeFileSync(archive, bytes)
  execFileSync('tar', ['-xzf', archive, '-C', dir], { stdio: 'inherit' })
  fs.rmSync(archive, { force: true })
  const bin = path.join(dir, 'linkcpp-expert-worker')
  if (!fs.existsSync(bin)) throw new Error(`${name} unpacked without linkcpp-expert-worker in it`)
  fs.chmodSync(bin, 0o755)
  console.log(`\ninstalled ${bin}`)
  console.log('`run` finds it here on its own; --worker is only for a build of your own.')
}

async function main(argv) {
  const { cmd, opts } = parseArgs(argv)
  if (cmd === 'keygen') {
    if (!opts.out) throw new Error('keygen needs --out <file>')
    console.log(`wrote ${opts.out}\naddress ${keygen(opts.out)}`)
    return
  }
  if (cmd === 'address') {
    if (!opts.key) throw new Error('address needs --key <file>')
    console.log(loadKey(opts.key).address)
    return
  }
  if (cmd === 'worker') {
    if (!('install' in opts)) { process.stdout.write(USAGE); process.exitCode = 2; return }
    await installWorker(opts)
    return
  }
  if (cmd !== 'run') { process.stdout.write(USAGE); process.exitCode = cmd ? 2 : 0; return }
  if (!opts.key) throw new Error('run needs --key <file>')
  loadKey(opts.key)   // a bad key file is the first thing to hear about, not the budget
  opts.budgetBytes = await resolveBudget(opts)
  const node = createNode(opts)
  if (!node.workerBinary()) throw new Error('no expert worker binary: pass --worker <path> or set KVASIR_EXPERT_WORKER')
  if (!node.model) throw new Error('no memory model for this platform\'s expert executor')
  const cap = executorsMod.capacityForBudget(node.budget, node.model, null)
  node.log(`kvasir-node ${node.workerId} · wallet ${node.address} · gateway ${node.gateway}`)
  node.log(`worker ${node.workerBinary()} · lending ${(node.budget / GIB).toFixed(1)} GiB → up to ${cap.experts} experts in ${cap.slots} slot(s)`)
  node.participation.start({ pollMs: (Number(opts.poll) || 45) * 1000 })
  const summary = setInterval(() => {
    const s = node.pool.status()
    node.log(`status: ${s.servingSlots} slot(s) serving, ${s.heldExperts} experts held`)
  }, 5 * 60_000)
  const stop = (sig) => {
    node.log(`${sig}: releasing slots and leaving the market`)
    clearInterval(summary)
    node.participation.stop()   // releases every slot's worker and relay
    setTimeout(() => process.exit(0), 200)
  }
  process.on('SIGTERM', () => stop('SIGTERM'))
  process.on('SIGINT', () => stop('SIGINT'))
}

module.exports = { loadKey, keygen, tokenStore, resolveBudget, createNode, parseArgs, base58, slug }

if (require.main === module) {
  main(process.argv.slice(2)).catch((e) => { console.error(`kvasir-node: ${e.message}`); process.exit(1) })
}
