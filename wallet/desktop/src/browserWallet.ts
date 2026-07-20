// Real non-custodial wallet for the hosted web app (served by the gateway).
// Mirrors the Electron main-process wallet, but runs in the browser: keys are
// derived and transactions signed here, and the recovery phrase is sealed at
// rest with WebCrypto (PBKDF2 -> AES-256-GCM) in localStorage. Login = unlock
// with the passphrase; logout = lock (drops the in-memory key).
//
// Derivation uses the SAME libraries as desktop/mobile (bip39 + ed25519-hd-key +
// @solana/web3.js, path m/44'/501'/0'/0'), so one recovery phrase yields the same
// address on every platform.

import * as bip39 from 'bip39'
import { derivePath } from 'ed25519-hd-key'
import {
  Connection, PublicKey, Keypair, Transaction, SystemProgram, LAMPORTS_PER_SOL,
} from '@solana/web3.js'
import {
  getAssociatedTokenAddress, getAccount, createAssociatedTokenAccountInstruction,
  createTransferCheckedInstruction, TokenAccountNotFoundError,
} from '@solana/spl-token'
import { pbkdf2 } from '@noble/hashes/pbkdf2.js'
import { sha256 } from '@noble/hashes/sha2.js'
import { randomBytes } from '@noble/hashes/utils.js'
import { gcm } from '@noble/ciphers/aes.js'
import nacl from 'tweetnacl'
import type { LinkcppAPI, AppConfig, Network, Balances, TxRef } from './api'

// Chain constants — must match wallet/shared-spec/token.devnet.json + wallet-constants.json.
const DERIVATION_PATH = "m/44'/501'/0'/0'"
const TOKEN = { symbol: 'KVR', mint: '6cuJAmqtMuGzJ7s7eWQSqfJvEFRUdTiYR3cuMmiNoCPQ', decimals: 6 }
const TREASURY_OWNER = '8uu2gDKFVtNS79yqYyztJeerEKAh4cnZGdQytCjsYNfF'
const RPC: Record<Network, string> = {
  devnet: 'https://api.devnet.solana.com',
  mainnet: 'https://api.mainnet-beta.solana.com',
}

const LS_WALLET = 'kvasir.wallet.v1'   // encrypted mnemonic envelope
const LS_ADDR = 'kvasir.wallet.addr'   // public address (shown while locked)
const LS_CFG = 'kvasir.config.v1'      // network / stakingUrl / language

// ---- base64 <-> bytes -----------------------------------------------------
const b64 = (u: Uint8Array) => btoa(String.fromCharCode(...u))
const ub64 = (s: string) => Uint8Array.from(atob(s), (c) => c.charCodeAt(0))
const te = new TextEncoder()
const td = new TextDecoder()

// ---- passphrase -> AES-256-GCM (pure JS) ----------------------------------
// Pure-JS PBKDF2 + AES-GCM via @noble, NOT WebCrypto: crypto.subtle is only
// exposed in a secure context (HTTPS or localhost), and the gateway is served
// over plain HTTP on a public IP. @noble works in any context.
interface Envelope { v: 1; salt: string; iv: string; ct: string }
function deriveKey(passphrase: string, salt: Uint8Array): Uint8Array {
  return pbkdf2(sha256, te.encode(passphrase), salt, { c: 200000, dkLen: 32 })
}
function seal(mnemonic: string, passphrase: string): Envelope {
  const salt = randomBytes(16), iv = randomBytes(12)
  const ct = gcm(deriveKey(passphrase, salt), iv).encrypt(te.encode(mnemonic))
  return { v: 1, salt: b64(salt), iv: b64(iv), ct: b64(ct) }
}
function open(env: Envelope, passphrase: string): string {
  const pt = gcm(deriveKey(passphrase, ub64(env.salt)), ub64(env.iv)).decrypt(ub64(env.ct)) // throws on wrong passphrase
  return td.decode(pt)
}

// ---- derivation (identical to desktop/mobile) -----------------------------
function keypairFromMnemonic(mnemonic: string): Keypair {
  const seed = bip39.mnemonicToSeedSync(mnemonic, '')
  const { key } = derivePath(DERIVATION_PATH, Buffer.from(seed).toString('hex'))
  return Keypair.fromSeed(key)
}

function osGuess(): string {
  const p = (navigator.platform || navigator.userAgent || '').toLowerCase()
  return p.includes('mac') ? 'macos' : p.includes('win') ? 'windows' : 'linux'
}

// The web app is served BY the gateway, which runs on the node's host machine —
// so the node's OS is the SERVER's real OS, not the browser's. Ask the gateway
// (/api/config → hostOs); fall back to the browser guess only if an older
// gateway doesn't report one.
async function hostOs(stakingUrl: string): Promise<string> {
  try {
    const r = await fetch(new URL('/api/config', stakingUrl).toString(), { cache: 'no-store' })
    if (r.ok) {
      const c = await r.json()
      if (c && typeof c.hostOs === 'string' && c.hostOs) return c.hostOs
    }
  } catch { /* offline / older gateway — fall back below */ }
  return osGuess()
}

export function makeBrowserWallet(defaultStakingUrl: string): LinkcppAPI {
  // In-memory unlocked session — cleared on lock / logout / reload.
  let session: { mnemonic: string; kp: Keypair; address: string } | null = null

  const readEnvelope = (): Envelope | null => {
    try { const o = JSON.parse(localStorage.getItem(LS_WALLET) || 'null'); return o && o.v === 1 ? o : null } catch { return null }
  }
  const exists = () => !!localStorage.getItem(LS_WALLET)
  const requireSession = () => { if (!session) throw new Error('locked'); return session }

  async function saveEncrypted(mnemonic: string, passphrase: string): Promise<string> {
    if (!passphrase || passphrase.length < 8) throw new Error('passphrase must be at least 8 characters')
    const env = seal(mnemonic, passphrase)
    localStorage.setItem(LS_WALLET, JSON.stringify(env))
    const kp = keypairFromMnemonic(mnemonic)
    session = { mnemonic, kp, address: kp.publicKey.toBase58() }
    localStorage.setItem(LS_ADDR, session.address)
    return session.address
  }

  const readCfg = (): Partial<AppConfig> => { try { return JSON.parse(localStorage.getItem(LS_CFG) || '{}') } catch { return {} } }
  const writeCfg = (patch: Partial<AppConfig>) => { const c = { ...readCfg(), ...patch }; localStorage.setItem(LS_CFG, JSON.stringify(c)); return c }
  const conn = (network: Network) => new Connection(RPC[network] || RPC.devnet, 'confirmed')

  async function balances(network: Network): Promise<Balances> {
    const { kp } = requireSession()
    const c = conn(network)
    const lamports = await c.getBalance(kp.publicKey)
    let token: number | null = null
    try {
      const ata = await getAssociatedTokenAddress(new PublicKey(TOKEN.mint), kp.publicKey)
      const acc = await getAccount(c, ata)
      token = Number(acc.amount) / 10 ** TOKEN.decimals
    } catch { token = null }
    return { sol: lamports / LAMPORTS_PER_SOL, token, symbol: TOKEN.symbol }
  }

  async function history(network: Network): Promise<TxRef[]> {
    const { kp } = requireSession()
    const sigs = await conn(network).getSignaturesForAddress(kp.publicKey, { limit: 15 })
    return sigs.map((s) => ({ signature: s.signature, blockTime: s.blockTime ?? null, failed: !!s.err }))
  }

  async function freshBlockhash(c: Connection) {
    const { blockhash } = await c.getLatestBlockhash('finalized')
    return blockhash
  }

  async function sendSol({ to, amount, network }: { to: string; amount: number; network: Network }): Promise<string> {
    const { kp } = requireSession(); const c = conn(network)
    const tx = new Transaction().add(SystemProgram.transfer({
      fromPubkey: kp.publicKey, toPubkey: new PublicKey(to), lamports: Math.round(amount * LAMPORTS_PER_SOL),
    }))
    tx.recentBlockhash = await freshBlockhash(c); tx.feePayer = kp.publicKey; tx.sign(kp)
    const sig = await c.sendRawTransaction(tx.serialize())
    await c.confirmTransaction(sig, 'confirmed')
    return sig
  }

  async function sendToken({ to, amount, network }: { to: string; amount: number; network: Network }): Promise<string> {
    const { kp } = requireSession(); const c = conn(network)
    const mint = new PublicKey(TOKEN.mint); const dest = new PublicKey(to)
    const srcAta = await getAssociatedTokenAddress(mint, kp.publicKey)
    const dstAta = await getAssociatedTokenAddress(mint, dest)
    const ixs = []
    try { await getAccount(c, dstAta) } catch (e) {
      if (e instanceof TokenAccountNotFoundError || String(e).includes('could not find account')) {
        ixs.push(createAssociatedTokenAccountInstruction(kp.publicKey, dstAta, dest, mint))
      } else throw e
    }
    ixs.push(createTransferCheckedInstruction(srcAta, mint, dstAta, kp.publicKey, Math.round(amount * 10 ** TOKEN.decimals), TOKEN.decimals))
    const tx = new Transaction().add(...ixs)
    tx.recentBlockhash = await freshBlockhash(c); tx.feePayer = kp.publicKey; tx.sign(kp)
    const sig = await c.sendRawTransaction(tx.serialize())
    await c.confirmTransaction(sig, 'confirmed')
    return sig
  }

  return {
    isElectron: false,
    wallet: {
      has: async () => exists(),
      state: async () => ({ exists: exists(), encrypted: exists(), locked: !session }),
      create: async (wordCount, passphrase) => {
        const mnemonic = bip39.generateMnemonic(wordCount === 24 ? 256 : 128)
        return { address: await saveEncrypted(mnemonic, passphrase), mnemonic }
      },
      preview: async (wordCount) => ({ mnemonic: bip39.generateMnemonic(wordCount === 24 ? 256 : 128) }),
      commit: async (mnemonic, passphrase) => {
        if (!bip39.validateMnemonic(mnemonic)) throw new Error('invalid recovery phrase')
        return { address: await saveEncrypted(mnemonic, passphrase) }
      },
      import: async (mnemonic, passphrase) => {
        const m = String(mnemonic || '').trim().replace(/\s+/g, ' ')
        if (!bip39.validateMnemonic(m)) throw new Error('invalid recovery phrase')
        return { address: await saveEncrypted(m, passphrase) }
      },
      unlock: async (passphrase) => {
        const env = readEnvelope(); if (!env) throw new Error('no wallet')
        let m: string; try { m = open(env, passphrase) } catch { throw new Error('invalid passphrase') }
        const kp = keypairFromMnemonic(m)
        session = { mnemonic: m, kp, address: kp.publicKey.toBase58() }
        localStorage.setItem(LS_ADDR, session.address)
        return { address: session.address }
      },
      lock: async () => { session = null; return true },
      // Browser wallets are always born encrypted — no legacy upgrade path.
      upgrade: async () => { throw new Error('not supported') },
      changePassphrase: async (oldPass, newPass) => {
        const env = readEnvelope(); if (!env) throw new Error('no wallet')
        let m: string; try { m = open(env, oldPass) } catch { throw new Error('invalid passphrase') }
        return { address: await saveEncrypted(m, newPass) }
      },
      address: async () => (session ? session.address : (localStorage.getItem(LS_ADDR) || null)),
      mnemonic: async () => requireSession().mnemonic,
      // Detached ed25519 signature over an arbitrary message (Sign-In With Solana:
      // proves wallet ownership to the gateway without exposing the key).
      signMessage: async (message: Uint8Array) => nacl.sign.detached(message, requireSession().kp.secretKey),
      clear: async () => { session = null; localStorage.removeItem(LS_WALLET); localStorage.removeItem(LS_ADDR); return true },
    },
    config: {
      get: async () => {
        const c = readCfg()
        return { network: (c.network as Network) || 'devnet', stakingUrl: c.stakingUrl || defaultStakingUrl, language: c.language ?? null }
      },
      set: async (patch) => {
        const c = writeCfg(patch)
        return { network: (c.network as Network) || 'devnet', stakingUrl: c.stakingUrl || defaultStakingUrl, language: c.language ?? null }
      },
    },
    solana: { balances, history, sendSol, sendToken },
    meta: async () => ({
      mint: TOKEN.mint, symbol: TOKEN.symbol, decimals: TOKEN.decimals, treasuryOwner: TREASURY_OWNER,
      defaultStakingUrl, os: await hostOs(defaultStakingUrl), arch: 'x64', deviceKind: 'web',
    }),
    // The web app is served BY the gateway; it can't manage that process from the
    // browser, so hosting is reported as unmanaged (no host-node reward bonus).
    gateway: {
      status: async () => ({ running: true, managed: false, port: Number(new URL(defaultStakingUrl).port) || 0, hostUrl: defaultStakingUrl, ipUrl: defaultStakingUrl, configuredUrl: defaultStakingUrl }),
      start: async () => ({ running: true, managed: false, port: Number(new URL(defaultStakingUrl).port) || 0, hostUrl: defaultStakingUrl, ipUrl: defaultStakingUrl, configuredUrl: defaultStakingUrl }),
      stop: async () => ({ running: true, managed: false, port: Number(new URL(defaultStakingUrl).port) || 0, hostUrl: defaultStakingUrl, ipUrl: defaultStakingUrl, configuredUrl: defaultStakingUrl }),
      setDir: async () => ({ running: true, managed: false, port: Number(new URL(defaultStakingUrl).port) || 0, hostUrl: defaultStakingUrl, ipUrl: defaultStakingUrl, configuredUrl: defaultStakingUrl }),
      setKeyFile: async () => ({ running: true, managed: false, port: Number(new URL(defaultStakingUrl).port) || 0, hostUrl: defaultStakingUrl, ipUrl: defaultStakingUrl, configuredUrl: defaultStakingUrl }),
      pickKey: async () => ({ running: true, managed: false, port: Number(new URL(defaultStakingUrl).port) || 0, hostUrl: defaultStakingUrl, ipUrl: defaultStakingUrl, configuredUrl: defaultStakingUrl }),
    },
    openExternal: async (url) => { window.open(url, '_blank') },
    revealPath: async () => {},
  }
}
