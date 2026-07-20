'use strict'
const { app, BrowserWindow, ipcMain, safeStorage, shell, dialog } = require('electron')
const path = require('node:path')
const fs = require('node:fs')
const os = require('node:os')
const http = require('node:http')
const crypto = require('node:crypto')
const { spawn } = require('node:child_process')
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
function rpc(network) { return (C.clusters[network] || C.clusters[C.defaultCluster]).rpcUrl }
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
function llamaServerBin() {
  const explicit = process.env.LINKCPP_LLAMA_SERVER
  if (explicit && fs.existsSync(explicit)) return explicit
  const repo = path.resolve(__dirname, '..', '..', '..')
  const candidates = [
    path.join(repo, 'build-node-darwin-metal', 'bin', 'llama-server'),
    path.join(repo, 'build-ring-darwin-metal', 'bin', 'llama-server'),
    path.join(repo, 'build', 'bin', 'llama-server'),
  ]
  return candidates.find((p) => fs.existsSync(p)) || null
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
  app.on('activate', () => { if (BrowserWindow.getAllWindows().length === 0) createWindow() })
})
app.on('before-quit', () => stopGateway())
app.on('window-all-closed', () => { if (process.platform !== 'darwin') app.quit() })
