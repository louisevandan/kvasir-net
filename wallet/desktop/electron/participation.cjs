'use strict'
/**
 * Joining the bridge's expert market.
 *
 * Until now this app ran a p4 agent on the machine and told the settlement
 * service it was online — and that was all. Nothing ever asked the machine to
 * serve anything, so `contributedUnits` stayed at 0 forever and the operator
 * saw an "online" node that earned nothing. p4node.cjs supervises the local
 * process; this file is the part that was missing, the one that makes the
 * bridge aware the machine exists and willing to give it work.
 *
 * The flow mirrors wallet/ios/.../BridgeAuthService.swift +
 * BridgeParticipation.swift and wallet/android/.../BridgeAuthService.kt, so the
 * three clients present themselves to the bridge identically:
 *
 *   1. POST /api/auth/challenge   {wallet}                  -> {nonce, message}
 *   2. ed25519-sign the message with the wallet key         -> base64
 *   3. POST /api/auth/node-token  {wallet,nonce,signature}  -> {node_token} (30d)
 *   4. Authorization: Bearer <node_token> from then on
 *   5. POST /api/expert-volunteer {model?, max_experts?}    -> an assignment
 *   6. POST /api/expert-coverage  {segments:[...]}          periodically
 *
 * Deliberately NOT here: downloading expert shards and relaying the actual
 * compute. The bridge side of both is still being written; without them a
 * machine can be a visible volunteer but not yet a worker, which is the state
 * this round is meant to reach.
 */
const CHALLENGE_PATH = '/api/auth/challenge'
const TOKEN_PATH = '/api/auth/node-token'
const VOLUNTEER_PATH = '/api/expert-volunteer'
const COVERAGE_PATH = '/api/expert-coverage'
const DEMAND_PATH = '/api/expert-demand'

// The bridge rejects anything longer; a base58 Solana address is 32-44.
const MAX_WALLET_LEN = 44

// The bridge drops a worker from the census after WORKER_STALE_MS (120s) and
// takes its relay port and session with it. The next tick is scheduled AFTER
// the current one finishes, so the real interval is pollMs + however long the
// tick took — at 60s two slow-or-failed ticks in a row already exceed 120s and
// the machine falls out of the market. 45s leaves room for that.
const DEFAULT_POLL_MS = 45_000
// A failed tick means the census entry is already ageing, so retry well before
// the next scheduled poll rather than letting the gap compound.
const RETRY_MS = 15_000
const REQUEST_TIMEOUT_MS = 20_000

class ParticipationError extends Error {
  constructor(message, { status = 0, code = '' } = {}) {
    super(message)
    this.name = 'ParticipationError'
    this.status = status
    this.code = code
  }
}

/**
 * @param {object} opts
 * @param {string} opts.baseUrl        gateway base, e.g. https://gate.kvasir-ai.net
 * @param {() => string} opts.wallet   base58 address of the unlocked wallet
 * @param {(bytes: Uint8Array) => Uint8Array} opts.sign
 *        detached ed25519 signature. Injected rather than taking the secret key
 *        so the mnemonic never leaves main.cjs's unlocked-session handling.
 * @param {{load:()=>({token:string,expiresAt:number}|null), save:(v:object|null)=>void}} [opts.store]
 *        persists the 30-day node token across restarts. Optional: without it
 *        the token simply lives for the process's lifetime.
 * @param {(line:string)=>void} [opts.log]
 */
class Participation {
  constructor({ baseUrl, wallet, sign, store = null, log = () => {}, workerId = null, host = null }) {
    this.base = String(baseUrl || '').replace(/\/+$/, '')
    this.walletFn = wallet
    // Same identity the settlement side already uses for this machine
    // (nodeSettings.tsx: `desktop-${address.slice(0,8)}`), so one machine is
    // one node in both the census and the reward ledger rather than two
    // half-visible ones.
    this.workerIdFn = workerId || (() => {
      const w = String(this.walletFn() || '')
      return w ? `desktop-${w.slice(0, 8)}` : ''
    })
    this.signFn = sign
    this.store = store
    this.log = log
    this.token = null
    this.tokenExpiresAt = 0
    this.assignment = null
    this.timer = null
    this.running = false
    this.lastError = null
    // What turns an assignment into something held (expertHost.cjs). Without
    // one this machine volunteers and reports nothing, which is the truth.
    this.host = host
  }

  // ---- token ---------------------------------------------------------------

  /**
   * A cached token is reused until it is close to expiry. `skew` keeps us from
   * starting a request with a token that dies mid-flight.
   */
  cachedToken() {
    if (!this.token && this.store) {
      const saved = this.store.load()
      if (saved && saved.token) {
        this.token = saved.token
        this.tokenExpiresAt = Number(saved.expiresAt) || 0
      }
    }
    const skew = 60_000
    if (this.token && (!this.tokenExpiresAt || Date.now() + skew < this.tokenExpiresAt)) {
      return this.token
    }
    return null
  }

  forgetToken() {
    this.token = null
    this.tokenExpiresAt = 0
    if (this.store) this.store.save(null)
  }

  /** Steps 1-3. Requires an unlocked wallet, because step 2 signs. */
  async mintToken() {
    const wallet = String(this.walletFn() || '')
    if (!wallet) throw new ParticipationError('wallet is locked', { code: 'wallet_locked' })
    if (wallet.length > MAX_WALLET_LEN) {
      throw new ParticipationError(`wallet address is ${wallet.length} chars; the bridge accepts at most ${MAX_WALLET_LEN}`,
        { code: 'wallet_too_long' })
    }

    const ch = await this.post(CHALLENGE_PATH, { wallet })
    const message = ch && ch.message
    const nonce = ch && ch.nonce
    if (!message || !nonce) {
      throw new ParticipationError('the bridge issued no challenge', { code: 'no_challenge' })
    }

    // Sign the message EXACTLY as given: it is a human-readable string the
    // bridge re-derives byte for byte. Re-encoding or trimming it here would
    // produce a signature that verifies against nothing.
    const sig = this.signFn(Buffer.from(message, 'utf8'))
    const signature = Buffer.from(sig).toString('base64')

    const resp = await this.post(TOKEN_PATH, { wallet, nonce, signature })
    const token = resp && resp.node_token
    if (!token) throw new ParticipationError('the bridge issued no node token', { code: 'no_token' })

    this.token = token
    // expires_in is seconds (30 days). Treat a missing value as "unknown" and
    // rely on the 401-retry path rather than inventing an expiry.
    const ttl = Number(resp.expires_in)
    this.tokenExpiresAt = Number.isFinite(ttl) && ttl > 0 ? Date.now() + ttl * 1000 : 0
    if (this.store) this.store.save({ token, expiresAt: this.tokenExpiresAt, wallet })
    this.log(`bridge: node token minted (${ttl ? Math.round(ttl / 86400) + 'd' : 'no stated expiry'})`)
    return token
  }

  async ensureToken() {
    return this.cachedToken() || (await this.mintToken())
  }

  /**
   * Run an authenticated call, re-minting once on 401. A node token outlives
   * the app, so the common failure is a token that expired or was revoked
   * while the app was closed; without this the node would sit there failing
   * every poll until someone restarted it.
   */
  async authed(fn) {
    let token = await this.ensureToken()
    try {
      return await fn(token)
    } catch (e) {
      if (e instanceof ParticipationError && e.status === 401) {
        this.log('bridge: node token rejected, re-authenticating')
        this.forgetToken()
        token = await this.mintToken()
        return await fn(token)
      }
      throw e
    }
  }

  // ---- market --------------------------------------------------------------

  /**
   * Step 5. Returns the assignment, or null when the bridge wants nobody.
   *
   * An assignment WITHOUT `n_embd` is refused. The dimension decides how the
   * expert tensors are read; guessing it does not fail loudly, it silently
   * computes garbage and reports it as work. iOS does the same
   * (BridgeParticipation.swift: "bridge did not supply n_embd … skipping").
   */
  async volunteer({ model = null, maxExperts = null } = {}) {
    const body = {}
    if (model) body.model = model
    if (maxExperts != null) body.max_experts = maxExperts

    const resp = await this.authed((token) => this.post(VOLUNTEER_PATH, body, token))
    if (!resp || resp.assigned !== true) {
      this.assignment = null
      return { assigned: false, reason: (resp && resp.reason) || 'no assignment' }
    }
    if (resp.n_embd == null) {
      this.assignment = null
      throw new ParticipationError(
        `the bridge assigned '${resp.model}' without n_embd — refusing to serve rather than compute garbage`,
        { code: 'missing_n_embd' })
    }
    this.assignment = resp
    const [begin, end] = Array.isArray(resp.experts) ? resp.experts : [null, null]
    this.log(`bridge: assigned ${resp.model} layer ${resp.layer} experts ${begin}-${end} (n_embd=${resp.n_embd})`)
    return resp
  }

  /**
   * Step 6. Announce this machine to the census and report what it HOLDS.
   *
   * Two things about the contract are easy to get wrong, and both fail
   * quietly (p4bridge/participation.js:298):
   *
   *   - A segment is a THREE-ELEMENT ARRAY [layer, begin, end]. The handler
   *     filters with `Array.isArray(s) && s.length === 3`, so objects are
   *     dropped one by one and the response is still {ok:true} — work that
   *     was never registered, reported as success.
   *   - worker_id and model are TOP-LEVEL and required; without them the
   *     post is a 400.
   *
   * Posting with no segments is correct and useful: liveWorkers() filters on
   * heartbeat freshness alone, so an empty report still puts this machine in
   * the census as an available volunteer. Claiming segments it does not hold
   * would be worse than useless — coverage() counts a replica per reported
   * expert, and expert-volunteer then reads that range as covered and stops
   * recruiting for it. That is a gap marked full: silently wrong.
   */
  async reportCoverage(segments, { model = null, workerId = null, nLayer = 0, nExpert = 0, url = '' } = {}) {
    const a = this.assignment
    const modelId = model || (a && a.model)
    const worker = workerId || this.workerIdFn()
    // Without these the bridge answers 400; there is nothing to salvage.
    if (!modelId || !worker) return null
    const body = {
      worker_id: worker,
      model: modelId,
      segments: Array.isArray(segments) ? segments : [],
      n_layer: nLayer || (a && a.n_layer) || 0,
      n_expert: nExpert || (a && a.n_expert) || 0,
      owner: this.walletFn() || '',
    }
    // Relay wiring only happens for a "relay:<session>" url, and an empty one
    // leaves this worker unwired — correct until there is something to serve.
    if (url) body.url = url
    return this.authed((token) => this.post(COVERAGE_PATH, body, token))
  }

  /** Step 7, read-only: what the bridge is currently recruiting for. */
  async demand() {
    return this.authed((token) => this.get(DEMAND_PATH, token))
  }

  // ---- loop ----------------------------------------------------------------

  /**
   * Poll the market until stopped. Errors are logged and retried rather than
   * thrown: a node that stops volunteering because the bridge blipped is worse
   * than one that keeps asking.
   */
  /**
   * @param {object} [opts]
   * @param {number|(() => number|null)} [opts.maxExperts] a number, or a function
   *        read on every tick so a budget the operator changes (the VRAM slider)
   *        applies on the next poll without restarting the loop. null/undefined
   *        means "no cap — let the bridge decide".
   */
  start({ pollMs = DEFAULT_POLL_MS, model = null, maxExperts = null } = {}) {
    if (this.running) return
    this.running = true
    this.pollMs = pollMs
    this.model = model
    this.maxExpertsFn = typeof maxExperts === 'function' ? maxExperts : () => maxExperts
    this.tick()
  }

  /**
   * One poll, never two at once. poke() can land while a tick is still awaiting
   * the bridge (a slot finishing its download does exactly that); running a
   * second tick alongside would double the coverage posts and could volunteer
   * twice for one free slot. So a poke during a tick is remembered and run as
   * soon as the current one ends.
   */
  async tick() {
    if (!this.running) return
    if (this.inTick) { this.pokeWanted = true; return }
    this.inTick = true
    try { await this.tickOnce() } finally { this.inTick = false }
    if (this.pokeWanted) { this.pokeWanted = false; this.poke() }
  }

  async tickOnce() {
      if (!this.running) return
      let failed = true
      try {
        if (this.host && typeof this.host.reports === 'function') {
          await this.tickPool(this.host)
          this.lastError = null
          failed = false
          if (this.running) this.timer = setTimeout(() => this.tick(), this.pollMs)
          return
        }
        const cap = this.maxExpertsFn ? this.maxExpertsFn() : null
        const host = this.host
        if (host && host.busy()) {
          // Holding (or fetching) an assignment: do not volunteer again. Once
          // our segment is reported, the bridge sees that range as covered and
          // would hand out a different one every poll — a node that swapped
          // shards each minute would never serve anything.
          if (cap != null && host.heldExperts() > cap) {
            host.release('the VRAM budget no longer fits it')
            this.assignment = null
          }
        } else if (cap === 0 || (host && !host.canHost())) {
          // No GPU memory lent, or nothing here that could compute an expert.
          // Asking for work we cannot hold would only earn an assignment to refuse.
          this.assignment = null
        } else {
          await this.volunteer({ model: this.model, maxExperts: cap })
          if (this.assignment && host) {
            host.provision(this.assignment, { workerId: this.workerIdFn() })
              // Report as soon as it serves, not a poll later: the coverage
              // post is what wires the relay session the worker dials.
              .then(() => this.poke())
              .catch(() => { /* logged by the host; the next tick volunteers again */ })
          }
        }
        // Report every tick, assignment or not: the census keys on heartbeat
        // freshness, so staying silent drops this machine out of the market.
        const url = host ? host.coverageUrl() : ''
        const resp = await this.reportCoverage(this.heldSegments(), {
          model: this.model || (host && host.held && host.held.model),
          url,
        })
        if (url && resp && resp.wired && typeof host.wired === 'function') host.wired()
        this.lastError = null
        failed = false
      } catch (e) {
        this.lastError = e.message
        // A locked wallet is the expected state after a restart, not a fault.
        this.log(e.code === 'wallet_locked'
          ? 'bridge: waiting for the wallet to be unlocked'
          : `bridge: ${e.message}`)
      }
      if (this.running) {
        this.timer = setTimeout(() => this.tick(), failed ? RETRY_MS : this.pollMs)
      }
  }

  /**
   * One tick with an ExpertPool: trim to the budget, grow by at most one slot,
   * then report every slot that has something to say.
   *
   * Growth is one slot per tick and only while no slot is still being set up,
   * so a big machine fills over a few polls rather than asking for eight
   * shards at once — and each request's size is what fits after the slots
   * already held, not a flat 64.
   */
  async tickPool(pool) {
    pool.trim()
    const window = pool.canHost() ? pool.nextWindow() : 0
    if (window > 0) {
      await this.volunteer({ model: this.model, maxExperts: window })
      if (this.assignment) {
        const a = this.assignment
        pool.provision(a)
          .then(() => this.poke())
          .catch(() => { /* logged by the host; a later tick volunteers again */ })
      }
    } else if (!pool.busySlots().length) {
      this.assignment = null
    }
    for (const r of pool.reports()) {
      const resp = await this.reportCoverage(r.segments, {
        model: this.model || r.model || (this.assignment && this.assignment.model),
        workerId: r.workerId,
        url: r.url,
      })
      // The relay session exists only once the bridge has taken this report.
      if (r.url && resp && resp.wired && typeof pool.wired === 'function') pool.wired(r.workerId)
    }
  }

  /**
   * Run the next poll now instead of waiting out the interval. Called when the
   * wallet is unlocked: the loop was almost certainly parked on "wallet is
   * locked", and making the operator wait a further minute to see their node
   * join is the kind of delay that reads as broken.
   */
  poke() {
    if (!this.running) return
    if (this.timer) { clearTimeout(this.timer); this.timer = null }
    this.timer = setTimeout(() => this.tick(), 0)
  }

  stop() {
    this.running = false
    if (this.timer) { clearTimeout(this.timer); this.timer = null }
    if (this.host) this.host.release('node stopped')
  }

  /**
   * What this machine actually HOLDS, as [layer, begin, end] triples.
   *
   * Only what the host's worker is serving right now. An accepted assignment
   * is not a held segment, and neither is a shard still downloading:
   * reporting either would inflate the bridge's replica count for a range
   * nothing can serve, and expert-volunteer would stop recruiting for it. The
   * machine still appears as a volunteer with an empty report, which is
   * exactly the truth — "here, holding nothing yet".
   */
  heldSegments() {
    return this.host ? this.host.heldSegments() : []
  }

  /**
   * What the UI shows. "the agent is running" and "the bridge has given this
   * machine work" looked identical before, which is how a node that earned
   * nothing read as healthy. `phase` is the one-word answer.
   */
  status() {
    const hasToken = !!this.cachedToken()
    let phase = 'stopped'
    if (this.running) {
      if (this.lastError === 'wallet is locked') phase = 'awaiting_unlock'
      else if (!hasToken) phase = 'authenticating'
      else if (this.host && (this.host.phase ?? this.host.status().phase) === 'serving') phase = 'serving'
      else if (this.assignment) phase = 'assigned'
      else phase = 'volunteering'
    }
    return {
      running: this.running,
      phase,
      hasToken,
      // True once the signature is done: later restarts reuse the 30-day
      // token, so the wallet only has to be unlocked for the first one.
      workerId: this.workerIdFn(),
      assignment: this.assignment,
      host: this.host ? this.host.status() : null,
      lastError: this.lastError,
    }
  }

  // ---- transport -----------------------------------------------------------

  post(path, body, token = null) { return this.request('POST', path, body, token) }
  get(path, token = null) { return this.request('GET', path, null, token) }

  async request(method, path, body, token) {
    const url = this.base + path
    // Cloudflare fronts the gateway, and its Browser Integrity Check answers a
    // client it classes as a bot with a bare 403 ("error code: 1010") that is
    // indistinguishable from an auth failure. Python's default agent is refused
    // outright today; Node's passes, but only by policy. Name the client so a
    // Cloudflare rule change cannot silently turn every poll into a "401".
    const headers = { Accept: 'application/json', 'User-Agent': 'kvasir-wallet-desktop' }
    if (body != null) headers['Content-Type'] = 'application/json'
    if (token) headers.Authorization = `Bearer ${token}`

    const ctl = new AbortController()
    const timer = setTimeout(() => ctl.abort(), REQUEST_TIMEOUT_MS)
    let resp
    try {
      resp = await fetch(url, {
        method, headers,
        body: body == null ? undefined : JSON.stringify(body),
        signal: ctl.signal,
      })
    } catch (e) {
      throw new ParticipationError(
        e.name === 'AbortError' ? `${method} ${path} timed out` : `${method} ${path}: ${e.message}`,
        { code: 'network' })
    } finally {
      clearTimeout(timer)
    }

    const text = await resp.text().catch(() => '')
    let data = null
    try { data = text ? JSON.parse(text) : null } catch { /* non-JSON body */ }

    if (!resp.ok) {
      const msg = (data && (data.error || (data.error && data.error.message))) || text || `HTTP ${resp.status}`
      throw new ParticipationError(String(msg).slice(0, 300), { status: resp.status })
    }
    return data
  }
}

module.exports = { Participation, ParticipationError, MAX_WALLET_LEN }
