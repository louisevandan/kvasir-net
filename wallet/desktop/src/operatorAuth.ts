// Operator (gateway admin) sign-in: extends the wallet login with Sign-In With
// Solana + TOTP 2FA, gated to admin wallets. The unlocked wallet signs the SIWS
// challenge, and if the wallet enrolled 2FA the app prompts for the code before
// granting the operator session. Backs onto the gateway /api/admin/* endpoints.
//
// Works wherever the wallet can sign (`api.wallet.signMessage`): the served web
// wallet talks to its own origin directly, while the Electron desktop routes
// admin HTTP through the main process (adminRequest), whose in-memory cookie jar
// stands in for browser cookies that can't cross origins. On desktop this is
// used against the LOCAL gateway (pricing governance), not the public domain.

import { adminRequest, api } from './api'

export type OperatorPhase = 'disabled' | 'not-admin' | 'need-2fa' | 'signed-in'
export interface OperatorState {
  enabled: boolean          // gateway has an admin allowlist configured
  phase: OperatorPhase
  twofa: boolean            // this wallet has 2FA enrolled
  preAuth?: string          // short-lived token bridging verify -> 2fa/login
}

const base = (stakingUrl: string) => stakingUrl.replace(/\/+$/, '')

async function j(url: string, opts: { method?: string; body?: string } = {}): Promise<any> {
  const r = await adminRequest(url, opts)
  let d: any = {}
  try { d = JSON.parse(r.body) } catch { /* non-JSON error body */ }
  if (!r.ok) { const e: any = new Error(d.error || `HTTP ${r.status}`); e.status = r.status; throw e }
  return d
}

const b64 = (u: Uint8Array) => btoa(String.fromCharCode(...u))

export interface AdminStatus { enabled: boolean; authenticated: boolean; wallet: string | null; twofa: boolean }
export function adminStatus(stakingUrl: string): Promise<AdminStatus> {
  return j(`${base(stakingUrl)}/api/admin/status`)
}

// Establish (or confirm) the operator session for the unlocked wallet.
export async function operatorSignIn(stakingUrl: string, address: string): Promise<OperatorState> {
  if (!api.wallet.signMessage) return { enabled: false, phase: 'disabled', twofa: false } // mock preview can't sign
  const b = base(stakingUrl)
  const st: AdminStatus = await adminStatus(stakingUrl)
  if (!st.enabled) return { enabled: false, phase: 'disabled', twofa: false }
  if (st.authenticated && st.wallet === address) return { enabled: true, phase: 'signed-in', twofa: !!st.twofa }
  // Not signed in yet — prove ownership by signing a challenge.
  let ch: { nonce: string; message: string }
  try {
    ch = await j(`${b}/api/admin/challenge`, { method: 'POST', body: JSON.stringify({ wallet: address }) })
  } catch (e: any) {
    if (e.status === 403) return { enabled: true, phase: 'not-admin', twofa: false } // wallet isn't an operator
    throw e
  }
  const sig = await api.wallet.signMessage!(new TextEncoder().encode(ch.message))
  const res = await j(`${b}/api/admin/verify`, {
    method: 'POST', body: JSON.stringify({ wallet: address, nonce: ch.nonce, signature: b64(sig) }),
  })
  if (res.twofa_required) return { enabled: true, phase: 'need-2fa', twofa: true, preAuth: res.pre_auth }
  return { enabled: true, phase: 'signed-in', twofa: false }
}

export function operator2faLogin(stakingUrl: string, preAuth: string, code: string): Promise<any> {
  return j(`${base(stakingUrl)}/api/admin/2fa/login`, { method: 'POST', body: JSON.stringify({ pre_auth: preAuth, code }) })
}
export function operatorLogout(stakingUrl: string): Promise<any> {
  return j(`${base(stakingUrl)}/api/admin/logout`, { method: 'POST', body: '{}' })
}
export interface EnrollInfo { secret: string; otpauth_uri: string; backup_codes: string[] }
export function operatorEnroll(stakingUrl: string): Promise<EnrollInfo> {
  return j(`${base(stakingUrl)}/api/admin/2fa/enroll`, { method: 'POST', body: '{}' })
}
export function operatorConfirm(stakingUrl: string, code: string): Promise<any> {
  return j(`${base(stakingUrl)}/api/admin/2fa/confirm`, { method: 'POST', body: JSON.stringify({ code }) })
}
export function operatorDisable(stakingUrl: string, code: string): Promise<any> {
  return j(`${base(stakingUrl)}/api/admin/2fa/disable`, { method: 'POST', body: JSON.stringify({ code }) })
}
