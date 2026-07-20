// HTTP clients for the off-chain staking/rewards service and the inference
// gateway — identical JSON contract to the iOS/Android wallets.

import { adminRequest } from './api'

async function req<T>(base: string, path: string, method: 'GET' | 'POST', body?: unknown): Promise<T> {
  let status: number, ok: boolean, text: string
  if (path.startsWith('/api/admin/')) {
    // Admin surface needs the operator session cookie — see adminRequest.
    const r = await adminRequest(base.replace(/\/$/, '') + path, { method, body: body ? JSON.stringify(body) : undefined })
    ;({ status, ok, body: text } = r)
  } else {
    const res = await fetch(base.replace(/\/$/, '') + path, {
      method,
      headers: body ? { 'content-type': 'application/json' } : undefined,
      body: body ? JSON.stringify(body) : undefined,
    })
    ;({ status, ok } = res); text = await res.text()
  }
  if (!ok) {
    let msg = text
    try { msg = JSON.parse(text).error ?? text } catch {}
    throw new Error(msg || `HTTP ${status}`)
  }
  return text ? JSON.parse(text) : (undefined as T)
}

export interface PerfTier { tier: string; minTps: number; mult: number }
// Genesis gateway config (/api/config). publicUrl is the canonical URL the gateway
// advertises for itself; clients on the shipped default auto-adopt it (genesis discovery).
export interface StakingConfig { cluster: string; symbol: string; aprPercent: number; rewardPerUnit: number; perfTiers?: PerfTier[]; publicUrl?: string; gatewayBonus?: number; rpcUrl?: string; mint?: string; vaultOwner?: string; decimals?: number }
export interface StakePosition { owner: string; principal: number; rewards: number; total: number; aprPercent: number }
export interface NodeStatusItem {
  nodeId: string; status: string; os?: string; deviceKind?: string; accelerator?: string; label?: string
  perfScore?: number; backend?: string; mode?: string; tier?: string; perfMultiplier?: number
  contributedUnits: number; effectiveUnits?: number; pendingRewards: number; claimedTotal: number; lastReport?: number | null
}
export interface NodeStatusTotals { nodes: number; online: number; contributedUnits: number; effectiveUnits?: number; pending: number; claimedTotal: number; lifetimeRewards: number }
export interface NodeStatus { owner: string; totals: NodeStatusTotals; nodes: NodeStatusItem[] }

export class Staking {
  constructor(private base: string) {}
  config() { return req<StakingConfig>(this.base, '/api/config', 'GET') }
  position(owner: string) { return req<StakePosition>(this.base, `/api/positions/${owner}`, 'GET') }
  stake(owner: string, amount: number, signature: string) { return req<StakePosition>(this.base, '/api/stake', 'POST', { owner, amount, signature }) }
  unstake(owner: string, amount?: number) { return req(this.base, '/api/unstake', 'POST', amount != null ? { owner, amount } : { owner }) }
  nodeStatus(owner: string) { return req<NodeStatus>(this.base, `/api/node/status/${owner}`, 'GET') }
  claim(owner: string) { return req<{ signature: string; claimed: number }>(this.base, '/api/node/claim', 'POST', { owner }) }
  registerNode(b: { nodeId: string; owner: string; os?: string; deviceKind?: string; accelerator?: string; label?: string; perfScore?: number; backend?: string; mode?: string; hostsGateway?: boolean }) {
    return req(this.base, '/api/node/register', 'POST', b)
  }
  heartbeat(nodeId: string, hostsGateway?: boolean) { return req(this.base, '/api/node/heartbeat', 'POST', { nodeId, hostsGateway }) }
  removeNode(nodeId: string, owner: string) { return req<{ removed: string }>(this.base, '/api/node/remove', 'POST', { nodeId, owner }) }
  allNodes() { return req<GlobalNodeStatus>(this.base, '/api/node/all', 'GET') }
}

export interface GlobalNode {
  nodeId: string; status: string; os: string; deviceKind?: string; accelerator?: string; backend?: string | null
  label: string; perfScore: number; tier: string; perfMultiplier: number; effectiveUnits: number; pendingRewards: number; ownerShort: string
}
export interface GlobalNodeStatus {
  totals: { nodes: number; online: number; owners: number; effectiveUnits: number; byTier: Record<string, number>; byOs: Record<string, number> }
  nodes: GlobalNode[]
}

export interface PayModel { id: string; name: string }
export interface PaymentQuote { requestId: string; model: string; priceToken: number; recipient: string; mint: string; symbol: string; estimated?: boolean; estPromptTokens?: number; estCompletionTokens?: number; estTotalTokens?: number }
export interface TokenUsage { promptTokens: number; completionTokens: number; totalTokens: number; costToken?: number }
export interface InferenceResult { requestId: string; paid: boolean; signature?: string; model?: string; priceToken?: number; result: string; usage?: TokenUsage }

export class Gateway {
  constructor(private base: string) {}
  models() { return req<{ recipient: string; symbol: string; models: PayModel[] }>(this.base, '/api/pay/models', 'GET') }
  quote(model: string, prompt: string) { return req<PaymentQuote>(this.base, '/api/pay/quote', 'POST', { model, prompt }) }
  infer(requestId: string, signature: string) { return req<InferenceResult>(this.base, '/api/inference', 'POST', { requestId, signature }) }
  // Pricing governance (genesis wallet + fresh TOTP, desktop-local only).
  getPricing() { return req<PricingView>(this.base, '/api/admin/pricing', 'GET') }
  setPricing(body: { modelId?: string; basePrice: number; perToken: number; estOut: number; totp: string }) {
    return req<{ ok: boolean; target: string; pricing: ModelPricing; updatedAt: number }>(this.base, '/api/admin/pricing', 'POST', body)
  }
  pricingAudit() { return req<{ audit: PricingAudit[] }>(this.base, '/api/admin/pricing/audit', 'GET') }
  // Propagation relays — public gateways this (genesis source) gateway pushes
  // pricing to. Managed here because the desktop app carries genesis auth.
  getRelays() { return req<{ isSource: boolean; relays: PricingRelay[] }>(this.base, '/api/admin/pricing/relays', 'GET') }
  addRelay(body: { url: string; secret?: string; label?: string; totp: string }) {
    return req<{ ok: boolean; relays: PricingRelay[] }>(this.base, '/api/admin/pricing/relays', 'POST', body)
  }
  removeRelay(url: string, totp: string) {
    return req<{ ok: boolean; relays: PricingRelay[] }>(this.base, '/api/admin/pricing/relays/remove', 'POST', { url, totp })
  }
}

export interface CreditBalance { wallet: string; balance: number; spent: number; symbol: string }
export type ChatStreamEvent = { type: 'token'; text: string } | { type: 'usage'; usage: TokenUsage }

// Prepaid-credit gateway client: SIWS self-registration + API-key minting, KVR
// credit deposit, balance, and streaming OpenAI-compatible chat.
//
// Streaming is why this exists: the non-streaming /api/inference route is cut off
// by Cloudflare's fixed 100s origin timeout on slow models (M3 ~1 tok/s) => 524.
// /v1/chat/completions with stream:true flows SSE from the first token, so the
// connection never idles out. Credits are debited per completion's usage.
export class Credit {
  constructor(private base: string) {}
  private url(p: string) { return this.base.replace(/\/$/, '') + p }

  registerChallenge(wallet: string) { return req<{ nonce: string; message: string }>(this.base, '/api/credits/register/challenge', 'POST', { wallet }) }
  register(wallet: string, nonce: string, signature: string) { return req(this.base, '/api/credits/register', 'POST', { wallet, nonce, signature }) }
  apikeyChallenge(wallet: string) { return req<{ nonce: string; message: string }>(this.base, '/api/credits/challenge', 'POST', { wallet }) }
  apikey(wallet: string, nonce: string, signature: string, label: string) { return req<{ apiKey: string }>(this.base, '/api/credits/apikey', 'POST', { wallet, nonce, signature, label }) }
  deposit(wallet: string, amount: number, signature: string) { return req<{ ok: boolean; balance: number }>(this.base, '/api/credits/deposit', 'POST', { wallet, amount, signature }) }

  async balance(apiKey: string): Promise<CreditBalance> {
    const res = await fetch(this.url('/api/credits/balance'), { headers: { authorization: `Bearer ${apiKey}` } })
    const text = await res.text()
    if (!res.ok) { let m = text; try { m = JSON.parse(text).error ?? text } catch { /* non-JSON */ } throw new Error(m || `HTTP ${res.status}`) }
    return JSON.parse(text)
  }

  // Stream an OpenAI-compatible completion; onEvent fires per content delta (and a
  // final usage chunk) so the UI appends incrementally and Cloudflare sees data flow.
  async streamChat(
    apiKey: string, model: string, messages: { role: string; content: string }[],
    onEvent: (e: ChatStreamEvent) => void, maxTokens = 1024,
  ): Promise<void> {
    const res = await fetch(this.url('/v1/chat/completions'), {
      method: 'POST',
      headers: { 'content-type': 'application/json', accept: 'text/event-stream', authorization: `Bearer ${apiKey}` },
      body: JSON.stringify({ model, messages, stream: true, max_tokens: maxTokens }),
    })
    if (!res.ok || !res.body) {
      const text = await res.text().catch(() => '')
      let m = text; try { const o = JSON.parse(text); m = o?.error?.message ?? o?.error ?? text } catch { /* non-JSON */ }
      throw new Error(m || `HTTP ${res.status}`)
    }
    const reader = res.body.getReader()
    const dec = new TextDecoder()
    let buf = ''
    for (;;) {
      const { done, value } = await reader.read()
      if (done) break
      buf += dec.decode(value, { stream: true })
      let nl: number
      while ((nl = buf.indexOf('\n')) >= 0) {
        const line = buf.slice(0, nl).trim(); buf = buf.slice(nl + 1)
        if (!line.startsWith('data:')) continue
        const payload = line.slice(5).trim()
        if (payload === '[DONE]') return
        let obj: any; try { obj = JSON.parse(payload) } catch { continue }
        const delta = obj?.choices?.[0]?.delta
        if (typeof delta?.content === 'string' && delta.content.length) onEvent({ type: 'token', text: delta.content })
        const u = obj?.usage
        if (u) onEvent({ type: 'usage', usage: { promptTokens: u.prompt_tokens ?? 0, completionTokens: u.completion_tokens ?? 0, totalTokens: u.total_tokens ?? 0 } })
      }
    }
  }
}

export interface PricingRelay { url: string; label: string; hasSecret: boolean; addedAt: number; addedBy: string }

export interface ModelPricing { basePrice: number; perToken: number; estOut: number }
export interface PricingView {
  default: ModelPricing
  perModel: Record<string, ModelPricing>
  updatedAt: number
  updatedBy: string | null
  genesis: string
  isGenesis: boolean
  symbol: string
  isSource?: boolean
  writeLocked?: boolean
}
export interface PricingAudit { at: number; by: string; target: string; prev: ModelPricing; next: ModelPricing }
