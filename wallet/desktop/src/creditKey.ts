// Credit API key: minting, caching, and recovery.
//
// The key is what inference credit is spent against. It is minted by signing a
// gateway nonce with the wallet key, and cached per wallet on this device —
// only its hash is kept server-side, so a lost cache cannot be recovered by
// asking for it back, it has to be reminted.
//
// That makes the cache a failure mode: if the gateway's ledger is replaced or
// the key is revoked, the stored key keeps being sent and every call answers
// 401 `invalid API key`, with nothing in the UI to clear it. `reissueApiKey`
// is the way out, and Settings exposes it.

import { api } from './api'
import { Credit } from './services'

const cacheKey = (addr: string) => `kvasir.credit.apikey.${addr}`
const b64 = (u: Uint8Array) => btoa(String.fromCharCode(...u))

/** Cached key for this wallet, if any. Does not contact the gateway. */
export function cachedApiKey(addr: string): string | null {
  try { return localStorage.getItem(cacheKey(addr)) } catch { return null }
}

export function clearApiKey(addr: string): void {
  try { localStorage.removeItem(cacheKey(addr)) } catch { /* ignore */ }
}

/**
 * Register the wallet (idempotent) and mint a fresh key, replacing any cached
 * one. Both steps sign a gateway nonce, so the wallet must be unlocked.
 *
 * Nonces are short-lived and held in the gateway's memory, so a restart between
 * the challenge and the signature invalidates them — a failure here is worth
 * retrying once before treating it as real.
 */
export async function mintApiKey(credit: Credit, addr: string, label: string): Promise<string> {
  if (!api.wallet.signMessage) throw new Error('wallet locked')
  const sign = async (message: string) =>
    b64(await api.wallet.signMessage!(new TextEncoder().encode(message)))

  const rc = await credit.registerChallenge(addr)
  await credit.register(addr, rc.nonce, await sign(rc.message))
  const ac = await credit.apikeyChallenge(addr)
  const { apiKey } = await credit.apikey(addr, ac.nonce, await sign(ac.message), label)
  try { localStorage.setItem(cacheKey(addr), apiKey) } catch { /* ignore */ }
  return apiKey
}

/** Cached key if present, otherwise mint one. */
export async function ensureApiKey(credit: Credit, addr: string, label: string): Promise<string> {
  return cachedApiKey(addr) ?? await mintApiKey(credit, addr, label)
}

/** Drop the cached key and mint a new one. The recovery path for a key the
 *  gateway no longer accepts. Credit balance is held against the wallet, not
 *  the key, so nothing is lost by reissuing. */
export async function reissueApiKey(credit: Credit, addr: string, label: string): Promise<string> {
  clearApiKey(addr)
  return await mintApiKey(credit, addr, label)
}
