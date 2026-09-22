'use strict'
const { app, BrowserWindow, ipcMain, safeStorage, shell, dialog } = require('electron')
const path = require('node:path')
const fs = require('node:fs')
const os = require('node:os')
const http = require('node:http')
const crypto = require('node:crypto')
const { spawn } = require('node:child_process')
const { P4Node } = require('./p4node.cjs')
const { Participation } = require('./participation.cjs')
const { executors, capacityForBudget, nextSlotWindow, slotsThatFit, slotBytes, expertExecutor, setInstalledExecutor } = require('./executors.cjs')
const { CudaPack } = require('./cudaPack.cjs')
const { ExpertHost, ExpertPool } = require('./expertHost.cjs')
const { RelayTunnel } = require('./relay.cjs')
const { capability } = require('./hardware.cjs')
const bip39 = require('bip39')
const bs58 = require('bs58')
const nacl = require('tweetnacl')
const { derivePath } = require('ed25519-hd-key')
const {
  Connection, PublicKey, Keypair, Transaction, SystemProgram, LAMPORTS_PER_SOL,
} = require('@solana/web3.js')
const {
  getAssociatedTokenAddress, getAccount, createAssociatedTokenAccountInstruction,
  createTransferCheckedInstruction, TokenAccountNotFoundError,
} = require('@solana/spl-token')
const C = require('./constants.cjs')

const isDev = !!process.env.ELECTRON_DEV
// Test hook (Playwright E2E): isolate the profile so automated runs never touch
// the real wallet. Must be set before anything derives userData paths.
if (process.env.KVASIR_USER_DATA) app.setPath('userData', process.env.KVASIR_USER_DATA)
let win = null

// Auto-detect the host OS so this machine registers as the right node kind.
function osName() {
  switch (process.platform) {
    case 'darwin': return 'macos'
    case 'win32': return 'windows'
    case 'linux': return 'linux'
    default: return 'unknown'
  }
}

// ---- persistence: passphrase-encrypted mnemonic + plain config -------------
// The recovery phrase is sealed with a key derived from the user's passphrase
// (scrypt → AES-256-GCM). Without the passphrase the phrase is unrecoverable —
// even by this app — which also removes the old Linux plaintext fallback.
const walletFile = () => path.join(app.getPath('userData'), 'wallet.enc')
const configFile = () => path.join(app.getPath('userData'), 'config.json')

function readConfig() {
  try { return JSON.parse(fs.readFileSync(configFile(), 'utf8')) } catch { return {} }
}
function writeConfig(patch) {
  const cfg = { ...readConfig(), ...patch }
  fs.writeFileSync(configFile(), JSON.stringify(cfg, null, 2))
  return cfg
}

// scrypt params: 128*N*r bytes ≈ 16 MiB at N=16384 — strong yet snappy on unlock.
const SCRYPT = { N: 16384, r: 8, p: 1 }
const SCRYPT_MAXMEM = 64 * 1024 * 1024
function sealMnemonic(mnemonic, passphrase) {
  const salt = crypto.randomBytes(16)
  const iv = crypto.randomBytes(12)
  const key = crypto.scryptSync(String(passphrase), salt, 32, { ...SCRYPT, maxmem: SCRYPT_MAXMEM })
  const cipher = crypto.createCipheriv('aes-256-gcm', key, iv)
  const ct = Buffer.concat([cipher.update(mnemonic, 'utf8'), cipher.final()])
  return {
    kvasir: 1, kdf: 'scrypt', ...SCRYPT,
    salt: salt.toString('base64'), iv: iv.toString('base64'),
    ct: ct.toString('base64'), tag: cipher.getAuthTag().toString('base64'),
  }
}
function openMnemonic(env, passphrase) {
  const key = crypto.scryptSync(String(passphrase), Buffer.from(env.salt, 'base64'), 32,
    { N: env.N, r: env.r, p: env.p, maxmem: SCRYPT_MAXMEM })
  const decipher = crypto.createDecipheriv('aes-256-gcm', key, Buffer.from(env.iv, 'base64'))
  decipher.setAuthTag(Buffer.from(env.tag, 'base64'))
  return Buffer.concat([decipher.update(Buffer.from(env.ct, 'base64')), decipher.final()]).toString('utf8')
}

// New wallets are JSON envelopes; legacy files are raw safeStorage/plaintext.
function readEnvelope() {
  if (!fs.existsSync(walletFile())) return null
  try { const o = JSON.parse(fs.readFileSync(walletFile(), 'utf8')); if (o && o.kvasir === 1) return o } catch {}
  return null
}
function walletExists() { return fs.existsSync(walletFile()) }
function isEncrypted() { return !!readEnvelope() }

// Legacy reader — only used to migrate an old unencrypted wallet to a passphrase.
function loadLegacyMnemonic() {
  if (!walletExists() || isEncrypted()) return null
  const buf = fs.readFileSync(walletFile())
  try {
    return safeStorage.isEncryptionAvailable() ? safeStorage.decryptString(buf) : buf.toString('utf8')
  } catch { return buf.toString('utf8') }
}

// In-memory unlocked session. Cleared on lock / auto-lock / quit.
let session = null // { mnemonic, address }
function setSession(mnemonic) { session = { mnemonic, address: addressOf(mnemonic) }; return session.address }
function clearSession() { session = null }
function requireUnlocked() {
  if (!session) throw new Error('locked')
  return session.mnemonic
}

// ---- debug identity (DEVELOPMENT ONLY) --------------------------------------
// Lets automated testing of node participation, shard download and the relay
// run without a human unlocking the wallet.
//
// It deliberately does NOT unlock or bypass the real wallet. It substitutes a
// separate test keypair, and the real wallet's passphrase protection is left
// exactly as it was. A switch that opened the real wallet would be one leaked
// env var away from being a way into anybody's funds.
//
// Two gates, both required:
//   - !app.isPackaged — a packaged/release build ignores this entirely, so the
//     switch cannot ship to users even if the variable is set on their machine.
//   - KVASIR_DEBUG_WALLET=1
//
// None of the flows under test need the real wallet: the bridge accepts a
// zero-balance identity for everything participation does. Anything earned
// lands on the test identity, never on the operator's wallet.
const DEBUG_WALLET = !app.isPackaged && process.env.KVASIR_DEBUG_WALLET === '1'
const debugWalletFile = () => path.join(app.getPath('userData'), 'debug-wallet.json')
let debugKeypair = null
function debugIdentity() {
  if (!DEBUG_WALLET) return null
  if (!debugKeypair) {
    try {
      const raw = JSON.parse(fs.readFileSync(debugWalletFile(), 'utf8'))
      debugKeypair = Keypair.fromSecretKey(Uint8Array.from(bs58.decode(raw.secretKey)))
    } catch {
      // Stable across restarts so the node keeps one identity (and one node
      // token) instead of appearing as a new machine on every launch.
      debugKeypair = Keypair.generate()
      fs.writeFileSync(debugWalletFile(), JSON.stringify({
        secretKey: bs58.encode(debugKeypair.secretKey),
        note: 'DEBUG ONLY: a test identity for development. Not a real wallet — never fund it.',
      }, null, 2))
    }
    console.log(`[DEBUG WALLET] using test identity ${debugKeypair.publicKey.toBase58()} — NOT the real wallet`)
  }
  return debugKeypair
}
/** Address everything signs as: the test identity in debug mode, else the real wallet. */
function activeAddress() {
  const d = debugIdentity()
  return d ? d.publicKey.toBase58() : (readConfig().address || '')
}
/** Keypair to sign with. Throws 'locked' when the real wallet is locked. */
function activeKeypair() {
  return debugIdentity() || keypairFromMnemonic(requireUnlocked())
}
function saveEncrypted(mnemonic, passphrase) {
  if (!passphrase || String(passphrase).length < 8) throw new Error('passphrase must be at least 8 characters')
  fs.writeFileSync(walletFile(), JSON.stringify(sealMnemonic(mnemonic, passphrase)))
  const address = setSession(mnemonic)
  writeConfig({ address })
  return address
}

// ---- wallet derivation (matches iOS/Android: SLIP-0010 ed25519) ------------
// Besides BIP39 mnemonics, the desktop can import a raw ed25519 secret key
// (base58 string or solana-cli JSON array) — the genesis/treasury key predates
// BIP39 and has no mnemonic. A raw key is sealed exactly like a mnemonic
// (scrypt → AES-256-GCM under the passphrase), tagged with a `raw:` prefix.
// Desktop-only: the web/mobile wallets stay mnemonic-only.
const RAW_PREFIX = 'raw:'
function parseRawSecret(input) {
  const s = String(input || '').trim()
  try {
    if (s.startsWith('[')) {
      const arr = JSON.parse(s)
      if (Array.isArray(arr) && arr.length === 64) return RAW_PREFIX + bs58.encode(Buffer.from(arr))
    } else if (!/\s/.test(s) && s.length >= 80) { // one long token — never a word list
      if (Buffer.from(bs58.decode(s)).length === 64) return RAW_PREFIX + s
    }
  } catch { /* not a raw key — caller falls back to mnemonic validation */ }
  return null
}
function keypairFromMnemonic(mnemonic) {
  if (mnemonic.startsWith(RAW_PREFIX)) {
    return Keypair.fromSecretKey(Uint8Array.from(bs58.decode(mnemonic.slice(RAW_PREFIX.length))))
  }
  const seed = bip39.mnemonicToSeedSync(mnemonic, '')
  const { key } = derivePath(C.derivationPath, seed.toString('hex'))
  return Keypair.fromSeed(key)
}
function addressOf(mnemonic) { return keypairFromMnemonic(mnemonic).publicKey.toBase58() }
// The app's Network is 'devnet' | 'mainnet'; wallet-constants keys the clusters
// by their Solana names, where mainnet is 'mainnet-beta'. Indexing by the app
// value therefore missed on mainnet, and the old fallback missed too because
// there is no defaultCluster key — so this threw on .rpcUrl instead of picking
// an endpoint. Map it the same way api.ts does for explorer links.
const CLUSTER_KEY = { devnet: 'devnet', mainnet: 'mainnet-beta' }
function rpc(network) {
  const cluster = C.clusters[CLUSTER_KEY[network] ?? network] || C.clusters.devnet
  return cluster.rpcUrl
}
function conn(network) { return new Connection(rpc(network), 'confirmed') }

// ---- balances / history ---------------------------------------------------
async function balances(network) {
  const m = requireUnlocked()
  const owner = keypairFromMnemonic(m).publicKey
  const c = conn(network)
  const lamports = await c.getBalance(owner)
  let token = null
  try {
    const ata = await getAssociatedTokenAddress(new PublicKey(C.token.mint), owner)
    const acc = await getAccount(c, ata)
    token = Number(acc.amount) / 10 ** C.token.decimals
  } catch (e) {
    token = null // mint not held / not on this cluster
  }
  return { sol: lamports / LAMPORTS_PER_SOL, token, symbol: C.token.symbol }
}

async function history(network) {
  const m = requireUnlocked()
  const owner = keypairFromMnemonic(m).publicKey
  const sigs = await conn(network).getSignaturesForAddress(owner, { limit: 15 })
  return sigs.map((s) => ({ signature: s.signature, blockTime: s.blockTime, failed: !!s.err }))
}

async function freshBlockhash(c) {
  // Use finalized to avoid "Blockhash not found" during preflight (same fix as mobile).
  const { blockhash, lastValidBlockHeight } = await c.getLatestBlockhash('finalized')
  return { blockhash, lastValidBlockHeight }
}

async function sendSol({ to, amount, network }) {
  const m = requireUnlocked()
  const kp = keypairFromMnemonic(m); const c = conn(network)
  const tx = new Transaction().add(SystemProgram.transfer({
    fromPubkey: kp.publicKey, toPubkey: new PublicKey(to),
    lamports: Math.round(amount * LAMPORTS_PER_SOL),
  }))
  const { blockhash } = await freshBlockhash(c)
  tx.recentBlockhash = blockhash; tx.feePayer = kp.publicKey
  tx.sign(kp)
  const sig = await c.sendRawTransaction(tx.serialize())
  await c.confirmTransaction(sig, 'confirmed')
  return sig
}

async function sendToken({ to, amount, network }) {
  const m = requireUnlocked()
  const kp = keypairFromMnemonic(m); const c = conn(network)
  const mint = new PublicKey(C.token.mint)
  const dest = new PublicKey(to)
  const srcAta = await getAssociatedTokenAddress(mint, kp.publicKey)
  const dstAta = await getAssociatedTokenAddress(mint, dest)
  const ixs = []
  try { await getAccount(c, dstAta) } catch (e) {
    if (e instanceof TokenAccountNotFoundError || String(e).includes('could not find account')) {
      ixs.push(createAssociatedTokenAccountInstruction(kp.publicKey, dstAta, dest, mint))
    } else throw e
  }
  ixs.push(createTransferCheckedInstruction(
    srcAta, mint, dstAta, kp.publicKey,
    Math.round(amount * 10 ** C.token.decimals), C.token.decimals,
  ))
  const tx = new Transaction().add(...ixs)
  const { blockhash } = await freshBlockhash(c)
  tx.recentBlockhash = blockhash; tx.feePayer = kp.publicKey
  tx.sign(kp)
  const sig = await c.sendRawTransaction(tx.serialize())
  await c.confirmTransaction(sig, 'confirmed')
  return sig
}

// ---- IPC -------------------------------------------------------------------
// ---- gateway (staking-service) process control ----------------------------
// The desktop can host the network's gateway/settlement service directly, so the
// operator needs no separate terminal. The hosting node earns a reward bonus.
let gwProc = null
let gwLastError = null
const GW_PORT = (() => { try { return Number(new URL(C.stakingServiceUrl).port) || 8791 } catch { return 8791 } })()
// The gateway's CODE (server.js + node_modules + token spec) is bundled into the
// app under Resources/ — it is non-secret and lives on the internal disk, which a
// spawned process can always read (an external-volume repo cannot be). The only
// secret, the settlement admin key, is NEVER bundled: the operator sets its file
// and the main process passes it inline (see startGateway). A config override +
// dev-repo fallback keep `npm run dev` and custom builds working.
const bundledGatewayDir = () => path.join(process.resourcesPath, 'gateway')
const bundledSpecPath = () => path.join(process.resourcesPath, 'gateway-spec', 'token.devnet.json')
const gatewayDir = () => {
  const configured = readConfig().gatewayDir
  if (configured) return configured
  if (process.env.KVR_GATEWAY_DIR) return process.env.KVR_GATEWAY_DIR
  if (app.isPackaged) return bundledGatewayDir()
  return path.join(__dirname, '..', '..', '..', 'solana', 'staking-service')
}
function lanIp() {
  const ifs = os.networkInterfaces()
  for (const name of Object.keys(ifs)) for (const i of ifs[name] || []) if (i.family === 'IPv4' && !i.internal) return i.address
  return '127.0.0.1'
}
function localHost() { const h = os.hostname(); return h.includes('.') ? h : `${h}.local` }
function gatewayHealth() {
  return new Promise((resolve) => {
    const req = http.get(`http://127.0.0.1:${GW_PORT}/health`, { timeout: 1500 }, (res) => { res.resume(); resolve(res.statusCode === 200) })
    req.on('error', () => resolve(false))
    req.on('timeout', () => { req.destroy(); resolve(false) })
  })
}
// The settlement private key file the operator points the app at. It is NEVER
// bundled or copied — the main process reads it at start and passes it inline to
// the gateway (see startGateway), so the operator keeps sole custody of the file.
function settlementKeyFile() {
  return process.env.KVR_ADMIN_KEY_FILE || readConfig().adminKeyFile || ''
}
async function gatewayStatus() {
  const keyPath = settlementKeyFile()
  return {
    running: await gatewayHealth(), managed: !!gwProc, port: GW_PORT,
    hostUrl: `http://${localHost()}:${GW_PORT}`, ipUrl: `http://${lanIp()}:${GW_PORT}`,
    configuredUrl: C.stakingServiceUrl,
    dir: readConfig().gatewayDir || '', lastError: gwLastError,
    keyPath, keyExists: !!keyPath && fs.existsSync(keyPath),
  }
}
function startGateway() {
  if (gwProc) return
  gwLastError = null
  const dir = gatewayDir()
  if (!fs.existsSync(path.join(dir, 'server.js'))) {
    throw new Error(`gateway code not found at "${dir}".`)
  }
  const env = { ...process.env }
  // Settlement key: read the operator's key file HERE (main process) and pass it
  // INLINE via KVR_ADMIN_KEY, so the spawned gateway never opens the file itself —
  // the app never copies or bundles the key, and the operator keeps sole custody.
  const keyFile = settlementKeyFile()
  if (!env.KVR_ADMIN_KEY) {
    if (!keyFile) throw new Error('Set your settlement key file below (the private key is never bundled in the app).')
    try {
      env.KVR_ADMIN_KEY = fs.readFileSync(keyFile, 'utf8').trim()
    } catch (e) {
      throw new Error(`Can't read the settlement key at "${keyFile}": ${(e && e.message) || e}`)
    }
  }
  // Point the bundled gateway at the bundled token spec (its own __dirname lookup
  // resolves to the repo layout, which isn't present in the packaged app).
  if (!env.KVR_TOKEN_SPEC && app.isPackaged) env.KVR_TOKEN_SPEC = bundledSpecPath()
  // The hosted gateway needs an admin allowlist or its operator surface
  // (SIWS sign-in, pricing governance) stays disabled. Default to this app's
  // wallet + the genesis wallet; anything exported in the user's shell wins.
  if (!env.KVR_ADMIN_WALLETS) {
    const admins = [readConfig().address, C.treasuryOwner].filter(Boolean)
    if (admins.length) env.KVR_ADMIN_WALLETS = admins.join(',')
  }
  // Run the gateway with the user's system Node (via a login shell so PATH/nvm
  // resolve), from a safe working directory (userData) with an ABSOLUTE server.js
  // path. Two macOS constraints shaped this:
  //   1. The gateway folder must be on an INTERNAL disk. A subprocess spawned by an
  //      unsigned GUI app cannot read a repo on an external/removable volume even
  //      after the app is granted Full Disk Access (TCC is per-binary and doesn't
  //      propagate the grant to children), so hosting reads EPERM off such volumes.
  //   2. We use the system Node, not the app's bundled one (ELECTRON_RUN_AS_NODE):
  //      Electron 32's Node (v20.18) rejects the gateway deps' require() of an ESM
  //      module (rpc-websockets → uuid) with ERR_REQUIRE_ESM; the system Node ≥20.19
  //      accepts it.
  // A userData cwd sidesteps the login shell's `brew shellenv`/getcwd tripping on a
  // protected cwd; server.js resolves its spec, admin key and DB via __dirname + env
  // (never cwd), and the DB is pinned to userData.
  const serverJs = path.join(dir, 'server.js')
  const safeCwd = app.getPath('userData')
  if (!env.KVR_DATA_DIR) env.KVR_DATA_DIR = path.join(app.getPath('userData'), 'gateway-data')
  const stdio = ['ignore', 'ignore', 'pipe']
  if (process.platform === 'win32') {
    gwProc = spawn('node', [serverJs], { cwd: safeCwd, stdio, shell: true, windowsHide: true, env })
  } else {
    const quoted = `'${serverJs.replace(/'/g, "'\\''")}'`
    gwProc = spawn(process.env.SHELL || '/bin/zsh', ['-lc', `exec node ${quoted}`], { cwd: safeCwd, stdio, env })
  }
  let errBuf = ''
  gwProc.stderr?.on('data', (d) => { errBuf = (errBuf + d.toString()).slice(-4000) })
  gwProc.on('error', (e) => { gwLastError = String((e && e.message) || e); gwProc = null })
  gwProc.on('exit', (code) => { if (code) gwLastError = errBuf.trim() || `gateway exited (code ${code})`; gwProc = null })
}
function stopGateway() { if (gwProc) { try { gwProc.kill('SIGTERM') } catch {} gwProc = null } }

// ---- gateway admin HTTP (cookie-session calls routed through main) ----------
// The renderer is cross-origin to every gateway (file:// in prod, the Vite host
// in dev), so browser cookies can't carry the admin session. Admin calls go
// through main instead: no CORS here, and the session cookie lives in this
// in-memory jar — it never touches disk and drops on quit.
const adminCookies = new Map() // origin -> "name=value; name2=value2"
async function adminFetch(url, init = {}) {
  const u = new URL(url)
  if (!/^https?:$/.test(u.protocol) || !u.pathname.startsWith('/api/admin/')) {
    throw new Error('adminFetch is limited to gateway /api/admin/ endpoints')
  }
  const headers = { 'Content-Type': 'application/json' }
  const cookie = adminCookies.get(u.origin)
  if (cookie) headers.Cookie = cookie
  const res = await fetch(url, { method: init.method || 'GET', headers, body: init.body })
  const set = typeof res.headers.getSetCookie === 'function' ? res.headers.getSetCookie() : []
  if (set.length) {
    const jar = new Map((adminCookies.get(u.origin) || '').split('; ').filter(Boolean)
      .map((p) => [p.slice(0, p.indexOf('=')), p]))
    for (const c of set) { const kv = c.split(';')[0]; jar.set(kv.slice(0, kv.indexOf('=')).trim(), kv.trim()) }
    adminCookies.set(u.origin, [...jar.values()].join('; '))
  }
  return { status: res.status, ok: res.ok, body: await res.text() }
}

// ---- on-device models -------------------------------------------------------
// The hub stages GGUFs here (same /control/download flow as the phones); the
// user manages them and can run them locally through a bundled/discovered
// llama-server binary.
function modelsDir() {
  const dir = path.join(app.getPath('userData'), 'models')
  try { fs.mkdirSync(dir, { recursive: true }) } catch {}
  return dir
}
function listModels() {
  try {
    return fs.readdirSync(modelsDir())
      .filter((f) => f.endsWith('.gguf'))
      .map((name) => {
        let size = 0
        try { size = fs.statSync(path.join(modelsDir(), name)).size } catch {}
        return { name, sizeBytes: size }
      })
      .sort((a, b) => a.name.localeCompare(b.name))
  } catch { return [] }
}
function deleteModel(name) {
  const safe = path.basename(String(name || ''))
  try { fs.unlinkSync(path.join(modelsDir(), safe)) } catch {}
}
// Resolve a llama-server binary: explicit env, then common repo build dirs.
/**
 * The local inference runtime.
 *
 * Only repository build directories were looked at, which meant the feature
 * worked for whoever had just compiled llama.cpp in this tree and for nobody
 * else — the same shape of gap the p4 agent had. A packaged app carries its own
 * under `resources/llama`, and a developer's installed copy is now found too.
 */
function llamaServerBin() {
  const exe = process.platform === 'win32' ? 'llama-server.exe' : 'llama-server'
  const explicit = process.env.LINKCPP_LLAMA_SERVER
  if (explicit && fs.existsSync(explicit)) return explicit
  const candidates = []
  if (process.resourcesPath) candidates.push(path.join(process.resourcesPath, 'llama', exe))
  const repo = path.resolve(__dirname, '..', '..', '..')
  candidates.push(
    path.join(repo, 'build-node-darwin-metal', 'bin', exe),
    path.join(repo, 'build-ring-darwin-metal', 'bin', exe),
    path.join(repo, 'build', 'bin', exe),
    // Installed by a package manager: Homebrew on either Mac architecture, or
    // anywhere on PATH. A developer who has llama.cpp should not have to build
    // it again inside this tree.
    '/opt/homebrew/bin/' + exe,
    '/usr/local/bin/' + exe,
  )
  const found = candidates.find((candidate) => fs.existsSync(candidate))
  if (found) return found
  try {
    const which = require('node:child_process')
      .execFileSync(process.platform === 'win32' ? 'where' : 'which', [exe], { encoding: 'utf8' })
      .split('\n')[0].trim()
    if (which && fs.existsSync(which)) return which
  } catch { /* not on PATH either */ }
  return null
}
let localProc = null
async function localGenerate(sender, name, prompt, maxTokens) {
  const bin = llamaServerBin()
  const safe = path.basename(String(name || ''))
  const model = path.join(modelsDir(), safe)
  if (!bin) return { ok: false, error: 'no local llama-server binary found (set LINKCPP_LLAMA_SERVER)' }
  if (!fs.existsSync(model)) return { ok: false, error: 'model not found' }
  const port = 8123
  if (!localProc) {
    const libDir = path.dirname(bin)
    localProc = spawn(bin, ['-m', model, '-ngl', '999', '--host', '127.0.0.1',
      '--port', String(port), '-c', '2048', '--no-mmap'],
      { stdio: 'ignore', env: { ...process.env, DYLD_LIBRARY_PATH: libDir } })
    localProc.on('exit', () => { localProc = null })
  }
  // Wait for the server, then stream a chat completion.
  for (let i = 0; i < 60; i++) {
    await new Promise((r) => setTimeout(r, 1000))
    try { const h = await fetch(`http://127.0.0.1:${port}/health`); if (h.ok) break } catch {}
  }
  try {
    const res = await fetch(`http://127.0.0.1:${port}/v1/chat/completions`, {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ messages: [{ role: 'user', content: prompt }],
        max_tokens: maxTokens, stream: false }),
    })
    const data = await res.json()
    return { ok: true, text: data.choices?.[0]?.message?.content ?? '',
      usage: data.usage || {} }
  } catch (e) {
    return { ok: false, error: String(e) }
  }
}

// ---- this machine as a p4 node ----------------------------------------------
// The agent is a real process holding real accelerators. `measured` is the only
// throughput number the app is allowed to report: a decode this machine actually
// ran. Until one exists it stays null, and the settlement service scores the node
// on its floor tier rather than on a number the app made up.
const p4node = new P4Node()

// ---- bridge participation --------------------------------------------------
// The p4 agent above owns this machine's accelerators; this is what tells the
// bridge the machine exists and asks it for work. Without it the node registers
// with settlement, reports "online", and is never given anything to serve —
// contributedUnits stays 0 forever, which is exactly what operators saw.
//
// The node token is a 30-day bearer credential, so it gets the same treatment
// as the mnemonic: encrypted with the OS keystore, never written in the clear.
// config.json sits unprotected in userData and would hand a 30-day identity to
// anyone who reads that folder.
const nodeTokenFile = () => path.join(app.getPath('userData'), 'node-token.enc')
const nodeTokenStore = {
  load() {
    try {
      const buf = fs.readFileSync(nodeTokenFile())
      if (!safeStorage.isEncryptionAvailable()) return null
      const rec = JSON.parse(safeStorage.decryptString(buf))
      // A token minted for a different wallet is useless: the bridge scopes it
      // to the signer. Silently dropping it makes switching wallets just work.
      return rec && rec.wallet === activeAddress() ? rec : null
    } catch { return null }
  },
  save(rec) {
    try {
      if (rec == null) { fs.unlinkSync(nodeTokenFile()); return }
      if (!safeStorage.isEncryptionAvailable()) {
        // Refuse rather than fall back to plaintext: a 30-day bearer identity
        // on disk in the clear is worse than re-signing on every start, which
        // is all this costs (the wallet is unlocked by then anyway).
        console.log('node token: OS keystore unavailable — not persisting; will re-authenticate each start')
        return
      }
      fs.writeFileSync(nodeTokenFile(), safeStorage.encryptString(JSON.stringify(rec)))
    } catch { /* best effort: a lost token is re-minted on the next poll */ }
  },
}

// ---- GPU memory the operator lends to the network ---------------------------
// The GPU on a desktop is usually shared — an image generator or a game can
// want most of it. So the operator chooses how much the node may use, and the
// app turns that into something the bridge understands: how many experts to
// accept.
//
// The conversion is the executor's memory model, not a fixed factor:
//
//     usable = min(budget, free VRAM now + what our own worker holds - reserve)
//     N      = floor((usable - F - S - H) / R), clamped to [0, 64]
//
// R (resident bytes per expert), F (fixed cost), S (scratch at the largest
// batch served) and H (headroom) come from the executor — they are measured
// per backend in executors.cjs. A flat "expert size x 1.25" over-asks when the
// budget is small (F alone is ~100 MiB) and would be badly wrong for an
// executor that expands the weights, e.g. fp16 at 3.3x the served size.
const VRAM_STEP = 256 * 1024 * 1024
// Kept free for the desktop itself (compositor, browser) whatever the budget.
const SYSTEM_VRAM_RESERVE = 512 * 1024 * 1024
// From the most recent probe. The default budget is derived from the total and
// the usable part from free, so both refresh whenever the node screen polls.
let lastGpuTotalBytes = null
let lastGpuFreeBytes = null
// VRAM our own expert worker holds right now. It shows up as "used" in the
// probe, and must be added back or the node would shrink its own offer every
// time it took work. Estimated from the memory model, not measured: WDDM does
// not report per-process GPU memory.
function ownWorkerGpuBytes() {
  const m = expertMemoryModel()
  if (!m || !expertPool) return 0
  return expertPool.slots.filter((s) => s.phase === 'serving')
    .reduce((sum, s) => sum + slotBytes(m, s.heldExperts()), 0)
}

// This platform's expert executor — CUDA on Windows/Linux, Metal on a Mac —
// and its measured memory model. Not a fixed id: the Mac has its own entry.
function expertMemoryModel() {
  const { entry } = expertExecutor()
  return entry && entry.memoryModel ? entry.memoryModel : null
}

/** Free GPU memory the node may still use, or null where it is not knowable (unified memory). */
function availableGpuBytes() {
  return lastGpuFreeBytes == null ? null : Math.max(0, lastGpuFreeBytes + ownWorkerGpuBytes() - SYSTEM_VRAM_RESERVE)
}

/**
 * The budget in effect. An operator who never touched the slider still gets a
 * budget — half the card — and it is the same number the slider displays.
 *
 * Returning "unlimited" when unset was the first version, and it asked the
 * bridge for everything: the very first poll came back with all 288 experts of
 * a layer, while the slider on screen showed 4 GiB. A control that displays a
 * value other than the one in force is worse than no control.
 */
function vramBudgetBytes() {
  const v = Number(readConfig().vramBudgetBytes)
  if (Number.isFinite(v) && v >= 0 && readConfig().vramBudgetBytes != null) return v
  if (lastGpuTotalBytes) return Math.floor(lastGpuTotalBytes / 2 / VRAM_STEP) * VRAM_STEP
  return null
}

/**
 * Experts this machine can hold in total at the current budget, across slots
 * of at most one shard each. This is what the slider shows. With no GPU probe
 * yet it is one shard's worth, not "no cap" — asking for a whole layer before
 * we even know the card size is how the unlimited request happened.
 */
function maxExpertsForBudget(budget = vramBudgetBytes()) {
  return capacityForBudget(budget, expertMemoryModel(), availableGpuBytes()).experts
}
async function refreshGpuTotal() {
  try {
    const c = await executors()
    const gpu = c.gpu && c.gpu.gpus && c.gpu.gpus[0]
    if (gpu && gpu.totalBytes) lastGpuTotalBytes = gpu.totalBytes
    if (gpu && Number.isFinite(gpu.freeBytes)) lastGpuFreeBytes = gpu.freeBytes
    return c
  } catch { return null }
}

// The Windows CUDA worker and its cuBLAS DLLs, fetched on demand rather than
// bundled (see cudaPack.cjs). Once installed, the executor probe finds it.
const cudaPack = new CudaPack({ dir: path.join(app.getPath('userData'), 'cuda-pack'), log: (line) => console.log(line) })
setInstalledExecutor('linkcpp-expert-worker', () => cudaPack.workerPath())

// Turns an assignment into a served segment: shard download, the worker, the
// relay. Declared before participation, which drives it.
let participation = null
const expertPool = new ExpertPool({
  makeHost: () => new ExpertHost({
    baseUrl: readConfig().stakingUrl || C.stakingServiceUrl,
    token: () => participation.ensureToken(),
    workerBinary: () => expertExecutor().path,
    shardDir: path.join(app.getPath('userData'), 'shards'),
    log: (line) => console.log(line),
  }),
  workerId: () => participation.workerIdFn(),
  nextWindow: (held) => nextSlotWindow(vramBudgetBytes(), expertMemoryModel(), availableGpuBytes(), held),
  fits: (held) => slotsThatFit(vramBudgetBytes(), expertMemoryModel(), availableGpuBytes(), held),
})
participation = new Participation({
  baseUrl: readConfig().stakingUrl || C.stakingServiceUrl,
  wallet: () => activeAddress(),
  // Signing needs the unlocked mnemonic (or the debug identity). Throwing here
  // rather than returning an empty signature is what surfaces "wallet is
  // locked" as a real reason.
  sign: (bytes) => nacl.sign.detached(Uint8Array.from(bytes), activeKeypair().secretKey),
  store: nodeTokenStore,
  log: (line) => console.log(line),
  host: expertPool,
})
const relay = new RelayTunnel()
let measured = null   // { tps, tokens, elapsedMs, model, at }

async function benchmark(sender, maxTokens = 64) {
  const models = listModels()
  if (!models.length) return { ok: false, error: 'no local model to measure with — add a GGUF first' }
  const smallest = models.slice().sort((a, b) => a.sizeBytes - b.sizeBytes)[0]
  const started = Date.now()
  const result = await localGenerate(sender, smallest.name, 'Write one sentence about distributed systems.', maxTokens)
  if (!result.ok) return { ok: false, error: result.error }
  const elapsedMs = Date.now() - started
  const tokens = Number(result.usage?.completion_tokens ?? 0)
  if (!tokens || elapsedMs <= 0) return { ok: false, error: 'the run reported no token count' }
  measured = { tps: tokens / (elapsedMs / 1000), tokens, elapsedMs, model: smallest.name, at: Date.now() }
  return { ok: true, ...measured }
}

async function nodeStatus({ inspect = false } = {}) {
  if (inspect) await p4node.inspect().catch(() => {})
  // `participation` is what the operator needs to tell "the agent is running"
  // from "the bridge has actually given this machine work" — the two looked
  // identical before, which is why an earning-nothing node read as healthy.
  return {
    ...p4node.status(), capability: await capability(), measured,
    relay: relay.status(), participation: participation.status(),
    // Surfaced so a test identity can never be mistaken for the real wallet.
    debugWallet: DEBUG_WALLET ? activeAddress() : null,
    // What can actually compute here, and live GPU memory (not cached — the
    // GPU is shared, so free VRAM moves while the app runs).
    compute: await refreshGpuTotal(),
    vramBudgetBytes: vramBudgetBytes(),
    maxExperts: maxExpertsForBudget(),
    // Held across slots (one shard, one worker each) vs what the budget allows.
    expertSlots: capacityForBudget(vramBudgetBytes(), expertMemoryModel(), availableGpuBytes()).slots,
    heldExperts: expertPool.heldExperts(),
    // So the slider's live preview uses the same conversion as the offer.
    expertMemoryModel: expertMemoryModel(),
    vramReserveBytes: SYSTEM_VRAM_RESERVE,
    ownWorkerGpuBytes: ownWorkerGpuBytes(),
    cudaPack: cudaPack.status(),
  }
}

/**
 * Give this machine an address the network can dial.
 *
 * The agent binds loopback, which is right — nothing here should be listening
 * on a public port, least of all a protocol with no authentication. The tunnel
 * is what makes it reachable anyway: one outbound connection, and work dialled
 * at the relay's address arrives down it.
 *
 * It needs the wallet, because the relay will not hand out an address to
 * someone who cannot prove which operator they are. A locked wallet therefore
 * means an agent that runs but cannot be given work, and the status says so
 * rather than the app retrying into a wall.
 */
function startRelay() {
  const cfg = readConfig()
  if (cfg.relayEnabled === false) return { skipped: 'turned off in settings' }
  const owner = debugIdentity() ? activeAddress() : (session ? session.address : (cfg.address || null))
  if (!owner) return { skipped: 'no wallet on this machine' }
  if (!session && !debugIdentity()) return { skipped: 'wallet is locked' }
  const { host, port } = cfg.relay || C.relay
  relay.start({
    relayHost: host,
    relayPort: port,
    // The same identity the settlement service and this app's node screen use.
    nodeId: `desktop-${owner.slice(0, 8)}`,
    owner,
    agentPort: p4node.port,
    sign: async (text) => {
      const kp = activeKeypair()
      return Buffer.from(nacl.sign.detached(Buffer.from(text, 'utf8'), kp.secretKey)).toString('base64')
    },
  })
  return { started: true }
}

function register() {
  ipcMain.handle('wallet:has', () => walletExists())
  // exists: any wallet on disk · encrypted: new passphrase format · locked: no live session
  ipcMain.handle('wallet:state', () => ({ exists: walletExists(), encrypted: isEncrypted(), locked: !session }))
  ipcMain.handle('wallet:create', (_e, wordCount = 12, passphrase) => {
    const strength = wordCount === 24 ? 256 : 128
    const mnemonic = bip39.generateMnemonic(strength)
    return { address: saveEncrypted(mnemonic, passphrase), mnemonic }
  })
  ipcMain.handle('wallet:preview', (_e, wordCount = 12) => {
    const strength = wordCount === 24 ? 256 : 128
    return { mnemonic: bip39.generateMnemonic(strength) }
  })
  ipcMain.handle('wallet:commit', (_e, mnemonic, passphrase) => {
    if (!bip39.validateMnemonic(mnemonic)) throw new Error('invalid recovery phrase')
    return { address: saveEncrypted(mnemonic, passphrase) }
  })
  ipcMain.handle('wallet:import', (_e, mnemonic, passphrase) => {
    const raw = parseRawSecret(mnemonic)
    if (raw) {
      keypairFromMnemonic(raw) // fromSecretKey validates the pub/priv pair before sealing
      return { address: saveEncrypted(raw, passphrase) }
    }
    const m = String(mnemonic || '').trim().replace(/\s+/g, ' ')
    if (!bip39.validateMnemonic(m)) throw new Error('invalid recovery phrase or secret key')
    return { address: saveEncrypted(m, passphrase) }
  })
  ipcMain.handle('wallet:unlock', (_e, passphrase) => {
    const env = readEnvelope(); if (!env) throw new Error('no encrypted wallet')
    let m; try { m = openMnemonic(env, passphrase) } catch { throw new Error('invalid passphrase') }
    const address = setSession(m); writeConfig({ address })
    // Resume market participation the operator already asked for. Signing on
    // unlock is only acceptable as "carry on with what you turned on" — never
    // as a side effect of unlocking, which is why this is gated on the node
    // being started rather than firing for every unlock.
    if (participation.running) {
      participation.poke()
    }
    return { address }
  })
  ipcMain.handle('wallet:lock', () => { clearSession(); return true })
  // One-time migration of a pre-lock (unencrypted) wallet onto a passphrase.
  ipcMain.handle('wallet:upgrade', (_e, passphrase) => {
    const legacy = loadLegacyMnemonic(); if (!legacy) throw new Error('no wallet to upgrade')
    const m = String(legacy).trim().replace(/\s+/g, ' ')
    if (!bip39.validateMnemonic(m)) throw new Error('stored phrase invalid')
    return { address: saveEncrypted(m, passphrase) }
  })
  ipcMain.handle('wallet:changePassphrase', (_e, oldPass, newPass) => {
    const env = readEnvelope(); if (!env) throw new Error('no encrypted wallet')
    let m; try { m = openMnemonic(env, oldPass) } catch { throw new Error('invalid passphrase') }
    return { address: saveEncrypted(m, newPass) }
  })
  // Address is public — available even while locked (from config) for the lock screen.
  ipcMain.handle('wallet:address', () => (session ? session.address : (readConfig().address || null)))
  ipcMain.handle('wallet:mnemonic', () => requireUnlocked())
  // Detached ed25519 signature over a message (SIWS operator login). The key
  // never leaves main — the renderer only hands over the bytes to sign.
  ipcMain.handle('wallet:signMessage', (_e, message) => {
    const kp = keypairFromMnemonic(requireUnlocked())
    return Buffer.from(nacl.sign.detached(Uint8Array.from(message), kp.secretKey))
  })
  ipcMain.handle('wallet:clear', () => { try { fs.unlinkSync(walletFile()) } catch {} ; clearSession(); writeConfig({ address: null }); return true })

  ipcMain.handle('config:get', () => ({
    network: readConfig().network || C.defaultCluster,
    stakingUrl: readConfig().stakingUrl || C.stakingServiceUrl,
    language: readConfig().language || null,
  }))
  ipcMain.handle('config:set', (_e, patch) => writeConfig(patch))

  ipcMain.handle('solana:balances', (_e, network) => balances(network))
  ipcMain.handle('solana:history', (_e, network) => history(network))
  ipcMain.handle('solana:sendSol', (_e, args) => sendSol(args))
  ipcMain.handle('solana:sendToken', (_e, args) => sendToken(args))
  ipcMain.handle('shell:open', (_e, url) => shell.openExternal(url))
  ipcMain.handle('shell:reveal', (_e, p) => { if (p && fs.existsSync(p)) shell.showItemInFolder(p) })

  ipcMain.handle('meta', () => ({
    mint: C.token.mint, symbol: C.token.symbol, decimals: C.token.decimals,
    treasuryOwner: C.treasuryOwner, defaultStakingUrl: C.stakingServiceUrl,
    os: osName(), arch: process.arch, deviceKind: 'desktop',
  }))

  ipcMain.handle('gateway:status', () => gatewayStatus())
  ipcMain.handle('gateway:start', async () => {
    if (!(await gatewayHealth())) {
      try { startGateway() } catch (e) { gwLastError = String((e && e.message) || e) }
    }
    for (let i = 0; i < 16; i++) { await new Promise((r) => setTimeout(r, 500)); if (await gatewayHealth()) break }
    return gatewayStatus()
  })
  ipcMain.handle('gateway:stop', async () => { stopGateway(); return gatewayStatus() })
  ipcMain.handle('gateway:setDir', (_e, dir) => {
    writeConfig({ gatewayDir: (dir || '').trim() || undefined })
    gwLastError = null
    return gatewayStatus()
  })
  ipcMain.handle('gateway:setKeyFile', (_e, file) => {
    writeConfig({ adminKeyFile: (file || '').trim() || undefined })
    gwLastError = null
    return gatewayStatus()
  })
  ipcMain.handle('gateway:pickKey', async () => {
    const win = BrowserWindow.getFocusedWindow()
    const r = await dialog.showOpenDialog(win, {
      title: 'Select the settlement admin key (admin.json)',
      properties: ['openFile'],
      filters: [{ name: 'Key', extensions: ['json'] }],
    })
    if (!r.canceled && r.filePaths[0]) writeConfig({ adminKeyFile: r.filePaths[0] })
    return gatewayStatus()
  })
  ipcMain.handle('gateway:adminFetch', (_e, url, init) => adminFetch(url, init || {}))

  // ---- on-device models: list, delete, run locally (parallels the mobile app) --
  ipcMain.handle('models:list', () => listModels())
  ipcMain.handle('models:delete', (_e, name) => { deleteModel(name); return listModels() })
  ipcMain.handle('models:dir', () => modelsDir())
  ipcMain.handle('models:generate', (e, { name, prompt, maxTokens }) =>
    localGenerate(e.sender, name, prompt, maxTokens || 512))

  // ---- node: run a p4 agent on this machine ---------------------------------
  ipcMain.handle('node:status', (_e, opts) => nodeStatus(opts || {}))
  ipcMain.handle('node:start', async () => {
    p4node.start()
    // Volunteer beside the agent. It polls, so a locked wallet or an
    // unreachable bridge is a logged retry rather than a failed start — the
    // local agent is useful on its own. Size the card first so the first
    // request carries a budget rather than asking for a whole layer.
    await refreshGpuTotal()
    participation.start({ pollMs: 45_000, maxExperts: () => maxExpertsForBudget() })
    // The tunnel comes up beside the agent, not after it is proven: the relay
    // only needs the port to exist by the time somebody dials, and a stream
    // that arrives early closes itself and says the agent is not up yet.
    startRelay()
    await new Promise((r) => setTimeout(r, 1200))
    return nodeStatus({ inspect: true })
  })
  ipcMain.handle('node:stop', async () => { participation.stop(); relay.stop(); await p4node.stop(); return nodeStatus() })
  ipcMain.handle('node:capability', (_e, refresh) => capability({ refresh: !!refresh }))
  ipcMain.handle('node:executors', () => executors())
  // Starts the pack install and returns at once; progress arrives through
  // node:status (cudaPack), which the node screen already polls.
  ipcMain.handle('node:installCudaPack', () => {
    cudaPack.install()
      .then(() => { if (participation.running) participation.poke() })
      .catch(() => { /* the reason is in cudaPack.status() */ })
    return cudaPack.status()
  })
  ipcMain.handle('node:cancelCudaPack', () => { cudaPack.cancel(); return cudaPack.status() })
  ipcMain.handle('node:setVramBudget', (_e, bytes) => {
    const v = Number(bytes)
    if (!Number.isFinite(v) || v < 0) throw new Error('VRAM budget must be a non-negative number of bytes')
    writeConfig({ vramBudgetBytes: Math.round(v) })
    // Takes effect on the next volunteer poll; no restart needed.
    return { vramBudgetBytes: vramBudgetBytes(), maxExperts: maxExpertsForBudget() }
  })
  ipcMain.handle('node:benchmark', (e, maxTokens) => benchmark(e.sender, maxTokens || 64))
}

function createWindow() {
  win = new BrowserWindow({
    width: 1440, height: 900, minWidth: 1080, minHeight: 720,
    backgroundColor: '#0f1420',
    title: 'Kvasir Wallet',
    webPreferences: {
      preload: path.join(__dirname, 'preload.cjs'),
      contextIsolation: true, nodeIntegration: false,
    },
  })
  win.maximize()
  win.webContents.on('did-finish-load', () => console.log('[main] renderer loaded ·', osName(), process.arch))
  win.webContents.on('did-fail-load', (_e, code, desc) => console.error('[main] load failed', code, desc))
  if (isDev) win.loadURL('http://localhost:5173')
  else win.loadFile(path.join(__dirname, '..', 'dist', 'index.html'))
}

app.whenReady().then(() => {
  register()
  createWindow()
  // Debug mode exists so participation can be tested without a human at the
  // lock screen — which is also what stands between the operator and the
  // "start node" button. Start the market loop directly; the local p4 agent is
  // left to the UI, since a dev checkout may not have an agent binary at all.
  if (DEBUG_WALLET) {
    debugIdentity()
    // Size the card first, so the first volunteer request already carries a
    // budget instead of asking for an entire layer.
    refreshGpuTotal().finally(() => {
      participation.start({ pollMs: 45_000, maxExperts: () => maxExpertsForBudget() })
    })
  }
  app.on('activate', () => { if (BrowserWindow.getAllWindows().length === 0) createWindow() })
})
app.on('before-quit', () => { stopGateway(); p4node.stop().catch(() => {}); expertPool.release('quitting') })
app.on('window-all-closed', () => { if (process.platform !== 'darwin') app.quit() })
