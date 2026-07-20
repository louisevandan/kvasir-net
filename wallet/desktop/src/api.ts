// Bridge to the Electron main process (window.linkcpp). When absent (plain
// browser / Playwright verification), a mock keeps the UI renderable with
// believable data so layout can be inspected without a chain connection.

import { makeBrowserWallet } from './browserWallet'

export type Network = 'devnet' | 'mainnet'
export interface Balances { sol: number; token: number | null; symbol: string }
export interface TxRef { signature: string; blockTime: number | null; failed: boolean }
export interface AppConfig { network: Network; stakingUrl: string; language: string | null }
export interface WalletState { exists: boolean; encrypted: boolean; locked: boolean }
export interface GatewayStatus { running: boolean; managed: boolean; port: number; hostUrl: string; ipUrl: string; configuredUrl: string; dir?: string; lastError?: string | null; keyPath?: string; keyExists?: boolean }

export interface LinkcppAPI {
  isElectron: boolean
  wallet: {
    has(): Promise<boolean>
    state(): Promise<WalletState>
    create(wordCount: number | undefined, passphrase: string): Promise<{ address: string; mnemonic: string }>
    preview(wordCount?: number): Promise<{ mnemonic: string }>
    commit(mnemonic: string, passphrase: string): Promise<{ address: string }>
    import(mnemonic: string, passphrase: string): Promise<{ address: string }>
    unlock(passphrase: string): Promise<{ address: string }>
    lock(): Promise<boolean>
    upgrade(passphrase: string): Promise<{ address: string }>
    changePassphrase(oldPass: string, newPass: string): Promise<{ address: string }>
    address(): Promise<string | null>
    mnemonic(): Promise<string | null>
    // Optional: detached ed25519 signature over a message (SIWS operator login).
    // Present in the browser wallet; may be absent on the Electron bridge.
    signMessage?(message: Uint8Array): Promise<Uint8Array>
    clear(): Promise<boolean>
  }
  config: { get(): Promise<AppConfig>; set(patch: Partial<AppConfig>): Promise<AppConfig> }
  solana: {
    balances(network: Network): Promise<Balances>
    history(network: Network): Promise<TxRef[]>
    sendSol(a: { to: string; amount: number; network: Network }): Promise<string>
    sendToken(a: { to: string; amount: number; network: Network }): Promise<string>
  }
  meta(): Promise<{ mint: string; symbol: string; decimals: number; treasuryOwner: string; defaultStakingUrl: string; os: string; arch: string; deviceKind: string }>
  gateway: {
    status(): Promise<GatewayStatus>
    start(): Promise<GatewayStatus>
    stop(): Promise<GatewayStatus>
    setDir(dir: string): Promise<GatewayStatus>
    setKeyFile(file: string): Promise<GatewayStatus>
    pickKey(): Promise<GatewayStatus>
    // Optional: /api/admin/* HTTP routed through the Electron main process,
    // which keeps the session cookie (the renderer is cross-origin to the
    // gateway, so browser cookies can't carry the admin session).
    adminFetch?(url: string, init?: { method?: string; body?: string }): Promise<AdminFetchResult>
  }
  models?: {
    list(): Promise<LocalModelInfo[]>
    remove(name: string): Promise<LocalModelInfo[]>
    dir(): Promise<string>
    generate(name: string, prompt: string, maxTokens?: number):
      Promise<{ ok: boolean; text?: string; error?: string; usage?: { completion_tokens?: number } }>
  }
  openExternal(url: string): Promise<void>
  revealPath(p: string): Promise<void>
}

export interface LocalModelInfo { name: string; sizeBytes: number }
export interface AdminFetchResult { status: number; ok: boolean; body: string }

// Cookie-session HTTP for the gateway's /api/admin/* surface. In Electron the
// call goes through main (cross-origin cookies don't work in the renderer);
// in the served web wallet a same-origin fetch carries the cookie itself.
export async function adminRequest(url: string, init: { method?: string; body?: string } = {}): Promise<AdminFetchResult> {
  const bridge = typeof window !== 'undefined' ? window.linkcpp?.gateway.adminFetch : undefined
  if (bridge) return bridge(url, init)
  const r = await fetch(url, {
    method: init.method || 'GET',
    credentials: 'include',
    headers: { 'Content-Type': 'application/json' },
    body: init.body,
  })
  return { status: r.status, ok: r.ok, body: await r.text() }
}

// Genesis gateway. Fallback when the app isn't served by a gateway (file:// preview);
// served web apps use their own origin, desktop uses constants. The public gateway
// is the default; set VITE_STAKING_URL (see .env.example) to point at a local/dev host.
const DEFAULT_STAKING_URL = import.meta.env.VITE_STAKING_URL || 'https://gate.kvasir-ai.net'
const DEMO_ADDR = 'JcVYk4PpP5m2kDhGzJAmYAtD8GF7svNgNK1V1BzTrRG'

// Homepage node-operator guide (per-OS desktop installers + mobile store links).
// The web app can't be a node itself, so it sends users here to get the native
// apps — must match kvasir-home LINKS.runNode and shared-spec nodeGuideUrl.
export const NODE_GUIDE_URL = 'https://kvasir-ai.net/run-node'

// When the wallet is served BY the gateway (the hosted web app), the gateway is
// this same origin — use it so node register / staking / inference reach the
// right machine instead of a stale default host. Falls back for file:// previews.
function servedStakingUrl(): string {
  try {
    if (typeof window !== 'undefined' && /^https?:$/.test(window.location.protocol)) return window.location.origin
  } catch { /* no window (SSR/tests) */ }
  return DEFAULT_STAKING_URL
}

function makeMock(): LinkcppAPI {
  let cfg: AppConfig = { network: 'devnet', stakingUrl: servedStakingUrl(), language: null }
  const DEMO_MNEMONIC = 'demo demo demo demo demo demo demo demo demo demo demo demo'
  let has = true
  let locked = false
  let pass = 'demodemo'
  return {
    isElectron: false,
    wallet: {
      has: async () => has,
      state: async () => ({ exists: has, encrypted: has, locked: has && locked }),
      create: async (_w, p) => { has = true; locked = false; pass = p || pass; return { address: DEMO_ADDR, mnemonic: DEMO_MNEMONIC } },
      preview: async () => ({ mnemonic: 'ripple lunar orbit velvet cabin pledge amber signal harbor tunnel modest kingdom' }),
      commit: async (_m, p) => { has = true; locked = false; pass = p || pass; return { address: DEMO_ADDR } },
      import: async (_m, p) => { has = true; locked = false; pass = p || pass; return { address: DEMO_ADDR } },
      unlock: async (p) => { if (p !== pass) throw new Error('invalid passphrase'); locked = false; return { address: DEMO_ADDR } },
      lock: async () => { locked = true; return true },
      upgrade: async (p) => { pass = p; locked = false; return { address: DEMO_ADDR } },
      changePassphrase: async (o, n) => { if (o !== pass) throw new Error('invalid passphrase'); pass = n; return { address: DEMO_ADDR } },
      address: async () => (has ? DEMO_ADDR : null),
      mnemonic: async () => { if (locked) throw new Error('locked'); return DEMO_MNEMONIC },
      clear: async () => { has = false; locked = false; return true },
    },
    config: { get: async () => cfg, set: async (p) => { cfg = { ...cfg, ...p }; return cfg } },
    solana: {
      balances: async () => ({ sol: 0.099995, token: 499.44, symbol: 'KVR' }),
      history: async () => ([
        { signature: '5Kd3…demo1zR', blockTime: 1783700000, failed: false },
        { signature: '2Xy9…demo2aB', blockTime: 1783600000, failed: false },
      ]),
      sendSol: async () => 'DemoSig1111', sendToken: async () => 'DemoSig2222',
    },
    meta: async () => ({ mint: '6cuJAmqtMuGzJ7s7eWQSqfJvEFRUdTiYR3cuMmiNoCPQ', symbol: 'KVR', decimals: 6, treasuryOwner: '8uu2gDKFVtNS79yqYyztJeerEKAh4cnZGdQytCjsYNfF', defaultStakingUrl: servedStakingUrl(), os: navigator.platform.toLowerCase().includes('mac') ? 'macos' : navigator.platform.toLowerCase().includes('win') ? 'windows' : 'linux', arch: 'x64', deviceKind: 'desktop' }),
    gateway: (() => {
      let running = false
      const snap = (): GatewayStatus => ({ running, managed: running, port: 8791, hostUrl: 'http://localhost:8791', ipUrl: 'http://127.0.0.1:8791', configuredUrl: DEFAULT_STAKING_URL, dir: '', lastError: null })
      return {
        status: async () => snap(),
        start: async () => { running = true; return snap() },
        stop: async () => { running = false; return snap() },
        setDir: async () => snap(),
        setKeyFile: async () => snap(),
        pickKey: async () => snap(),
      }
    })(),
    openExternal: async (url) => { window.open(url, '_blank') },
    revealPath: async () => {},
  }
}

declare global { interface Window { linkcpp?: LinkcppAPI } }

// Provider selection:
//  - Electron bridge (window.linkcpp) → keys live in the main process.
//  - Served web app (http/https) → real in-browser non-custodial wallet.
//  - Otherwise (file:// preview / tests) → mock with demo data.
function pickProvider(): { api: LinkcppAPI; kind: 'electron' | 'browser' | 'mock' } {
  if (typeof window !== 'undefined' && window.linkcpp) return { api: window.linkcpp, kind: 'electron' }
  try {
    if (typeof window !== 'undefined' && /^https?:$/.test(window.location.protocol)) {
      return { api: makeBrowserWallet(servedStakingUrl()), kind: 'browser' }
    }
  } catch { /* no window */ }
  return { api: makeMock(), kind: 'mock' }
}
const picked = pickProvider()
export const api: LinkcppAPI = picked.api
export const walletKind = picked.kind
export const isElectron = picked.kind === 'electron'
// Providers with a real persistent encrypted key store enforce the passphrase
// lock (a reload can't bypass it). The mock does not.
export const lockCapable = picked.kind !== 'mock'

export function explorerTxUrl(sig: string, network: Network): string {
  const cluster = network === 'mainnet' ? 'mainnet-beta' : 'devnet'
  return `https://explorer.solana.com/tx/${sig}?cluster=${cluster}`
}
export function shorten(s: string, head = 6, tail = 6): string {
  return s.length > head + tail + 1 ? `${s.slice(0, head)}…${s.slice(-tail)}` : s
}
export function fmt(v: number | null | undefined): string {
  if (v == null) return '—'
  return Number(v.toFixed(6)).toLocaleString(undefined, { maximumFractionDigits: 6 })
}
