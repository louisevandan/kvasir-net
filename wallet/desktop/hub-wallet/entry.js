// Built-in non-custodial wallet for the linkcpp HUB admin UI, bundled to a single
// browser IIFE (window.LinkcppHubWallet). Uses the SAME libraries + derivation as
// the Kvasir wallet (wallet/desktop/src/browserWallet.ts), so a given mnemonic
// yields the SAME Solana address across web/desktop/mobile and the hub UI.
import * as bip39 from 'bip39'
import { derivePath } from 'ed25519-hd-key'
import { Keypair } from '@solana/web3.js'
import nacl from 'tweetnacl'
import { pbkdf2 } from '@noble/hashes/pbkdf2.js'
import { sha256 } from '@noble/hashes/sha2.js'
import { randomBytes } from '@noble/hashes/utils.js'
import { gcm } from '@noble/ciphers/aes.js'

const DERIVATION_PATH = "m/44'/501'/0'/0'" // identical to browserWallet.ts

function keypairFromMnemonic(mnemonic) {
  const seed = bip39.mnemonicToSeedSync(mnemonic, '')
  const { key } = derivePath(DERIVATION_PATH, Buffer.from(seed).toString('hex'))
  return Keypair.fromSeed(key)
}

export function generateMnemonic() { return bip39.generateMnemonic(128) } // 12 words
export function validateMnemonic(m) { return bip39.validateMnemonic(String(m || '').trim()) }
export function addressFromMnemonic(m) { return keypairFromMnemonic(String(m || '').trim()).publicKey.toBase58() }

// Sign an arbitrary message (Uint8Array) for SIWS. Returns a 64-byte signature.
export function signMessage(mnemonic, messageBytes) {
  const kp = keypairFromMnemonic(String(mnemonic || '').trim())
  return nacl.sign.detached(messageBytes, kp.secretKey)
}

// ---- encrypted at-rest storage (PBKDF2 + AES-GCM via @noble, works over plain
// HTTP where crypto.subtle is unavailable) ----------------------------------
const te = new TextEncoder(); const td = new TextDecoder()
const b64 = (u8) => btoa(String.fromCharCode(...u8))
const ub64 = (s) => Uint8Array.from(atob(s), (c) => c.charCodeAt(0))
function deriveKey(passphrase, salt) { return pbkdf2(sha256, te.encode(passphrase), salt, { c: 200000, dkLen: 32 }) }

export function seal(mnemonic, passphrase) {
  const salt = randomBytes(16), iv = randomBytes(12)
  const ct = gcm(deriveKey(passphrase, salt), iv).encrypt(te.encode(mnemonic))
  return { v: 1, salt: b64(salt), iv: b64(iv), ct: b64(ct) }
}
export function open(env, passphrase) {
  const pt = gcm(deriveKey(passphrase, ub64(env.salt)), ub64(env.iv)).decrypt(ub64(env.ct)) // throws on wrong passphrase
  return td.decode(pt)
}
