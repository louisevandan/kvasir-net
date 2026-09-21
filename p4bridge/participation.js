'use strict';
/**
 * The participation surface: how a phone earns.
 *
 * A node that wants to contribute asks what is under-covered, says what it has
 * taken on, and opens a relay so the ring can reach it through the NAT it sits
 * behind. The settlement gateway reads the resulting contribution ledger and
 * pays against it.
 *
 * This is the half that went missing. `controller/hub.py` served it until the
 * control plane was retired; both phones kept the client side and have been
 * calling into a 404 ever since — the connect button on the node settings
 * screen could only ever fail. The routes, field names and error shapes here
 * follow the recovered contract exactly, because the clients are shipped and
 * cannot be changed to match us.
 *
 * Two things the original got wrong, corrected here and called out so the
 * difference is deliberate rather than accidental:
 *
 *   - Every error body is `{"error": ...}`. The original emitted `{"detail":
 *     ...}` from handlers and `{"error": ...}` from middleware, and both mobile
 *     clients only ever read `error`, so an explained refusal reached the user
 *     as a bare "HTTP 403".
 *   - Participation is not operator eligibility. See nodeauth.js.
 */
const crypto = require('node:crypto');
const { pipeline } = require('node:stream');
const { timingSafeEqual } = require('./nodeauth');

/** How long a worker's coverage claim stands without a heartbeat. The clients
 *  beat every 15s; the margin is for a backgrounded phone, not for slack. */
const WORKER_STALE_MS = 120_000;
/** Replicas wanted per expert segment before it stops being under-covered. */
const TARGET_REPLICAS = Number(process.env.KVR_EXPERT_TARGET_REPLICAS ?? 2);
/** Contribution units per megabyte carried over a relay. */
const UNITS_PER_MB = Number(process.env.KVR_EXPERT_UNITS_PER_MB ?? 1);
/** Coordinator listen ports handed to relay sessions, sticky per worker. */
const PORT_BASE = 52_970;
const PORT_SPAN = 60;

/**
 * Bounds on what one caller can make the server hold or walk.
 *
 * Everything below `/api/auth` takes a node token, but a token costs only a
 * wallet and a signature — anyone can mint one. So these are not protection
 * against an outsider; they are protection against any single participant,
 * malicious or merely broken, turning its own bookkeeping into the host's
 * problem. `coverage()` walks every live worker's segments on every volunteer
 * poll, so an unbounded segment list is unbounded work for everyone else.
 */
const MAX_SEGMENTS_PER_WORKER = 256;
const MAX_WORKERS = 4_096;
/** A base58 Solana address is 32 bytes, so 44 characters at most. */
const MAX_WALLET_LENGTH = 64;
/** A relay session name is ours to shape; a caller may not make it a payload. */
const MAX_SESSION_LENGTH = 128;

const json = (res, status, body) => {
  const text = JSON.stringify(body);
  res.writeHead(status, {
    'content-type': 'application/json',
    'content-length': Buffer.byteLength(text),
  });
  res.end(text);
};
const fail = (res, status, message) => json(res, status, { error: message });

class Participation {
  /**
   * @param {object} options
   * @param {import('./nodeauth').NodeAuth} options.auth
   * @param {(nodeId: string, units: number, meta: object) => void} options.credit
   *        Fold contribution units into the ledger the gateway polls.
   * @param {() => Array<{id: string, name: string, nEmbd: number, nLayer: number, nExpert: number}>}
   *        options.models what this bridge can hand out work for.
   */
  constructor({ auth, credit, models, shard = null }) {
    this.auth = auth;
    this.credit = credit;
    this.models = models;
    /**
     * Where expert weights come from: a reader sitting beside the GGUF, on this
     * same host. Absent means shard download is simply not offered — a bridge
     * without the model file has nothing to serve, and saying so is better than
     * a relay that times out.
     */
    this.shard = shard;
    /** worker_id -> {model, nLayer, nExpert, segments, url, owner, ts} */
    this.workers = new Map();
    /** session -> {host, port, model, layer, experts, owner, nodeId, ts} */
    this.relayTargets = new Map();
    /** session -> {opened, closed, ws2tcp, tcp2ws, creditedBytes} */
    this.relayStats = new Map();
    /** worker_id -> port, so a reconnecting worker keeps its coordinator port */
    this.ports = new Map();
    this.flushTimer = setInterval(() => this.flushContributions(), 60_000);
    this.flushTimer.unref?.();
  }

  stop() { clearInterval(this.flushTimer); }

  liveWorkers() {
    const cutoff = Date.now() - WORKER_STALE_MS;
    const live = [];
    for (const [id, w] of this.workers) {
      if (w.ts >= cutoff) { live.push([id, w]); continue; }
      // Everything a worker held goes with it. The port span is sixty wide, so
      // a port that is never released means sixty coverage posts — free to make,
      // a node token costs a keypair and a signature — permanently stop every
      // other device in the fleet from being wired for a relay.
      this.workers.delete(id);
      this.ports.delete(id);
      for (const [session, target] of this.relayTargets) {
        if (target.nodeId === id) {
          this.relayTargets.delete(session);
          this.relayStats.delete(session);
        }
      }
    }
    return live;
  }

  modelInfo(ref) {
    const base = String(ref ?? '').split('/').pop();
    return this.models().find((m) => m.id === ref || m.id === base || m.name === ref) ?? null;
  }

  /**
   * Per-layer replica counts for a model, folded into runs of equal coverage.
   * A run is what a volunteer is handed, so that a node takes a contiguous
   * expert window rather than a scattered set it would have to fetch piecemeal.
   */
  coverage(model) {
    const info = this.modelInfo(model);
    if (!info) return [];
    // Only the layers that actually hold routed experts. `nLayer` is the block
    // count, which is not the same thing: this model's first three blocks are
    // dense, and a GGUF may also place MoE every n-th layer. Counting from zero
    // to nLayer offered a volunteer layer 0 of Step 3.7 — a window with no
    // tensors behind it, which nothing would have caught until the device asked
    // for the shard and the file had nothing to give.
    //
    // A model whose topology was never read has no expert layers here, so it is
    // offered nothing. That is deliberate: refusing to recruit is recoverable,
    // and handing out windows that cannot be served is not.
    const expertLayers = Array.isArray(info.expertLayers) ? info.expertLayers : [];
    const replicasFor = new Map(expertLayers.map((l) => [l, new Array(info.nExpert).fill(0)]));
    for (const [, w] of this.liveWorkers()) {
      if (w.model !== info.id && w.model !== info.name) continue;
      for (const [layer, begin, end] of w.segments) {
        const replicas = replicasFor.get(layer);
        if (!replicas) continue;            // a layer this model does not shard
        for (let e = Math.max(0, begin); e < Math.min(end, info.nExpert); e += 1) {
          replicas[e] += 1;
        }
      }
    }
    return [...replicasFor.entries()].map(([layer, replicas]) => {
      const segments = [];
      let start = 0;
      for (let e = 1; e <= replicas.length; e += 1) {
        if (e === replicas.length || replicas[e] !== replicas[start]) {
          const count = replicas[start];
          segments.push({
            experts: [start, e],
            replicas: count,
            target: TARGET_REPLICAS,
            scarcity: Math.round(
              Math.max(0, (TARGET_REPLICAS - count) / TARGET_REPLICAS) * 1000) / 1000,
          });
          start = e;
        }
      }
      return { layer, segments };
    });
  }

  portFor(workerId) {
    if (this.ports.has(workerId)) return this.ports.get(workerId);
    const taken = new Set(this.ports.values());
    for (let i = 0; i < PORT_SPAN; i += 1) {
      const port = PORT_BASE + i;
      if (!taken.has(port)) { this.ports.set(workerId, port); return port; }
    }
    return null;
  }

  /** Fold relay bytes into the ledger. Charged on the larger direction: a ring
   *  hop carries hidden state one way and receipts the other, and paying for
   *  both would count the same work twice. */
  flushContributions() {
    for (const [session, stats] of this.relayStats) {
      const target = this.relayTargets.get(session);
      if (!target) continue;
      const carried = Math.max(stats.ws2tcp, stats.tcp2ws);
      const fresh = carried - stats.creditedBytes;
      if (fresh <= 0) continue;
      stats.creditedBytes = carried;
      this.credit(target.nodeId ?? session, (fresh / 1e6) * UNITS_PER_MB, {
        owner: target.owner ?? '',
        model: target.model ?? '',
      });
    }
  }

  // ---- HTTP ---------------------------------------------------------------

  /** Returns true when the request was handled. */
  async handle(req, res, path, query, body) {
    const who = this.auth.identify(req);
    const needsNode = () => {
      if (who.kind === 'service' || who.kind === 'node') return true;
      fail(res, 401, 'authentication required');
      return false;
    };

    if (req.method === 'POST' && path === '/api/auth/challenge') {
      const wallet = String(body?.wallet ?? '').trim();
      if (!wallet) return fail(res, 400, 'wallet is required'), true;
      // The nonce cap bounds how many challenges are held, not how large each
      // one is, and this string is what gets held. A Solana address is at most
      // 44 base58 characters; without this, ten thousand outstanding challenges
      // carrying a two-megabyte "wallet" each is twenty gigabytes of the host's
      // memory, bought for twenty gigabytes of upload.
      if (wallet.length > MAX_WALLET_LENGTH) {
        return fail(res, 400, 'that is not a wallet address'), true;
      }
      if (!await this.auth.allows(wallet)) {
        return fail(res, 403, 'this wallet is not allowed to participate'), true;
      }
      try {
        return json(res, 200, this.auth.newChallenge(wallet)), true;
      } catch (error) {
        return fail(res, error.status ?? 500, error.message), true;
      }
    }

    if (req.method === 'POST' && path === '/api/auth/node-token') {
      const wallet = String(body?.wallet ?? '').trim();
      const nonce = String(body?.nonce ?? '').trim();
      const signature = String(body?.signature ?? '');
      if (!wallet || !nonce || !signature) {
        return fail(res, 400, 'wallet, nonce and signature are required'), true;
      }
      if (!await this.auth.allows(wallet)) {
        return fail(res, 403, 'this wallet is not allowed to participate'), true;
      }
      const message = this.auth.consumeChallenge(wallet, nonce);
      if (!message) {
        return fail(res, 401, 'that challenge is unknown, expired, or already used'), true;
      }
      const { verifyWalletSignature } = require('./nodeauth');
      if (!verifyWalletSignature(wallet, message, signature)) {
        return fail(res, 401, 'the signature does not match that wallet'), true;
      }
      const { token, expiresIn } = this.auth.makeToken(wallet);
      return json(res, 200, { node_token: token, wallet, expires_in: expiresIn }), true;
    }

    if (req.method === 'GET' && path === '/api/expert-demand') {
      if (!needsNode()) return true;
      const want = String(query.get('model') ?? '');
      const models = this.models()
        .filter((m) => !want || m.id === want || m.name === want || m.id.endsWith(want))
        .map((m) => ({
          model: m.id,
          n_layer: m.nLayer,
          n_expert: m.nExpert,
          target_replicas: TARGET_REPLICAS,
          recruiting: true,
          layers: this.coverage(m.id),
        }));
      return json(res, 200, { target_replicas: TARGET_REPLICAS, recruiting: true, models }), true;
    }

    if (req.method === 'POST' && path === '/api/expert-volunteer') {
      if (!needsNode()) return true;
      const want = String(body?.model ?? '');
      const maxExperts = Number(body?.max_experts ?? 0) || 0;
      let best = null;
      for (const m of this.models()) {
        if (want && m.id !== want && m.name !== want && !m.id.endsWith(want)) continue;
        for (const { layer, segments } of this.coverage(m.id)) {
          for (const seg of segments) {
            if (seg.scarcity <= 0) continue;
            const width = seg.experts[1] - seg.experts[0];
            if (!best || seg.scarcity > best.seg.scarcity
              || (seg.scarcity === best.seg.scarcity && width > best.width)) {
              best = { model: m, layer, seg, width };
            }
          }
        }
      }
      if (!best) {
        return json(res, 200, { assigned: false, reason: 'expert coverage at target' }), true;
      }
      const [begin, endFull] = best.seg.experts;
      const end = maxExperts > 0 ? Math.min(endFull, begin + maxExperts) : endFull;
      // n_embd is not decoration: both clients abort the assignment when it is
      // missing rather than guess a hidden size and produce silent garbage.
      return json(res, 200, {
        assigned: true,
        model: best.model.id,
        layer: best.layer,
        experts: [begin, end],
        scarcity: best.seg.scarcity,
        replicas: best.seg.replicas,
        target: TARGET_REPLICAS,
        n_embd: best.model.nEmbd,
        n_layer: best.model.nLayer,
        n_expert: best.model.nExpert,
      }), true;
    }

    if (req.method === 'POST' && path === '/api/expert-coverage') {
      if (!needsNode()) return true;
      const workerId = String(body?.worker_id ?? '').trim();
      const model = String(body?.model ?? '').trim();
      if (!workerId || !model) {
        return fail(res, 400, 'worker_id and model are required'), true;
      }
      const segments = (Array.isArray(body?.segments) ? body.segments : [])
        .filter((s) => Array.isArray(s) && s.length === 3)
        .slice(0, MAX_SEGMENTS_PER_WORKER)
        .map((s) => s.map(Number))
        .filter((s) => s.every(Number.isFinite));
      const url = String(body?.url ?? '');
      const previous = this.workers.get(workerId);
      if (!previous && this.liveWorkers().length >= MAX_WORKERS) {
        // The census is already at capacity with workers that are still beating.
        // Admitting more would slow every other participant's poll.
        return fail(res, 503, 'the coverage census is full; try again shortly'), true;
      }
      // A heartbeat must not clobber an owner the relay dial has since adopted,
      // or the node stops being paid halfway through its own session.
      const owner = String(body?.owner ?? '') || previous?.owner || '';
      this.workers.set(workerId, {
        model: model.split('/').pop(),
        nLayer: Number(body?.n_layer ?? 0),
        nExpert: Number(body?.n_expert ?? 0),
        segments,
        url,
        owner,
        ts: Date.now(),
      });

      let wired = false;
      let listenPort = null;
      let session = null;
      if (url.startsWith('relay:')) {
        session = (url.slice('relay:'.length) || `expert-${workerId}`)
          .slice(0, MAX_SESSION_LENGTH);
        listenPort = this.portFor(workerId);
        if (listenPort) {
          const prior = this.relayTargets.get(session);
          this.relayTargets.set(session, {
            host: '127.0.0.1',
            port: listenPort,
            model: model.split('/').pop(),
            layer: segments[0]?.[0] ?? 0,
            experts: segments[0] ? [segments[0][1], segments[0][2]] : [0, 0],
            owner: owner || prior?.owner || '',
            nodeId: workerId,
            ts: Date.now(),
          });
          wired = true;
        }
      }
      return json(res, 200, {
        ok: true,
        workers: this.liveWorkers().length,
        wired,
        listen_port: listenPort,
        session,
      }), true;
    }

    const shardPath = /^\/api\/proxy\/models\/([^/]+)\/expert-shard$/.exec(path);
    if (req.method === 'GET' && shardPath) {
      if (!needsNode()) return true;
      if (!this.shard) {
        return fail(res, 503, 'this bridge serves no expert shards'), true;
      }
      // The request is semantic on the way in and semantic on the way out: the
      // reader beside the file decides what a layer and an expert range mean,
      // and refuses anything it cannot map. Nothing here interprets the bytes,
      // and nothing here can be talked into reading an arbitrary offset.
      //
      // Any node token opens this. The weights are a quantisation of a
      // published model, so what a token buys is not secrecy but a name to
      // attribute bandwidth to; the reader caps how much one request may pull.
      const target = new URL(`${this.shard.origin}/shard`);
      target.searchParams.set('model', decodeURIComponent(shardPath[1]));
      for (const key of ['layer', 'expert_begin', 'expert_end']) {
        const value = query.get(key);
        if (value === null) return fail(res, 400, `${key} is required`), true;
        target.searchParams.set(key, value);
      }
      // Node's own HTTP client, not fetch.
      //
      // fetch is undici, and undici asserts `assert(!this.paused)` when the
      // upstream socket ends while its parser is paused. A relay pauses its
      // source constantly — that is what backpressure is — so the assertion
      // fires whenever the reader finishes while the bridge still has bytes it
      // has not drained to a slower client. It is an uncaught exception, and
      // this process also serves the ring, so a shard download was taking
      // inference down with it. Aborting on client disconnect fixed only the
      // half of it where the client left first.
      //
      // There is nothing here that wants a Response object. This is a pipe.
      const shardUrl = new URL(target);
      const http = require(shardUrl.protocol === 'https:' ? 'node:https' : 'node:http');
      await new Promise((resolve) => {
        const upstream = http.request(shardUrl, {
          headers: { 'x-kvasir-service-token': this.shard.token },
        }, (reply) => {
          const headers = { 'content-type': reply.headers['content-type'] ?? 'application/octet-stream' };
          if (reply.headers['content-length']) headers['content-length'] = reply.headers['content-length'];
          // Which checkpoint the weights came from, so a worker can refuse a
          // shard that does not belong with the ones it already holds.
          if (reply.headers['x-kvasir-shard-digest']) {
            headers['x-kvasir-shard-digest'] = reply.headers['x-kvasir-shard-digest'];
          }
          res.writeHead(reply.statusCode, headers);
          // Streamed, not buffered: one window is hundreds of megabytes.
          // `pipeline` rather than `pipe` so a failure on either side is
          // handled instead of thrown at the process.
          pipeline(reply, res, (error) => {
            if (error && error.code !== 'ERR_STREAM_PREMATURE_CLOSE') {
              this.log?.(`shard relay ended early: ${error.message}`);
            }
            resolve();
          });
        });
        upstream.on('error', (error) => {
          if (!res.headersSent) fail(res, 502, `the shard reader is unreachable: ${error.message}`);
          else res.destroy();
          resolve();
        });
        // The client walking away must tear down the upstream read, not leave
        // it filling a buffer nobody will drain.
        res.on('close', () => upstream.destroy());
        upstream.end();
      });
      return true;
    }

    return false;
  }

  // ---- WebSocket relays ---------------------------------------------------

  /**
   * Resolve an upgrade to the TCP endpoint it bridges to.
   *
   * Both relays are byte pipes with no protocol of their own. In particular the
   * ring's one-byte role preamble — 'P' for "I am your predecessor", 'N' for
   * "I am your successor" — rides inside the first binary frame and must pass
   * through untouched: it is how a NAT'd stage that dials both neighbours tells
   * each one which file descriptor it is.
   */
  resolveUpgrade(path, query) {
    const token = query.get('token') ?? '';
    const wallet = this.auth.verifyToken(token);
    // Constant-time, like identify() on the HTTP path. A plain === here leaks
    // the secret one byte at a time to anyone who can time the refusal.
    const service = Boolean(this.auth.serviceToken)
      && timingSafeEqual(token, this.auth.serviceToken);
    if (!wallet && !service) return { code: 4401, reason: 'authentication required' };

    if (path === '/api/expert-relay') {
      const session = query.get('session') ?? '';
      const target = this.relayTargets.get(session);
      if (!target) return { code: 4404, reason: 'unknown relay session' };
      // The coverage POST creates the target before the worker dials, and it
      // has no owner of its own to give — so the dialing wallet is who gets
      // paid for what crosses this socket.
      if (!target.owner && wallet) target.owner = wallet;
      return { session, host: target.host, port: target.port };
    }

    if (path === '/api/ring-relay') {
      const controllerId = query.get('controller_id') ?? '';
      const target = this.relayTargets.get(`ring-${controllerId}`);
      if (!target) return { code: 4404, reason: 'unknown ring controller' };
      if (!target.owner && wallet) target.owner = wallet;
      return { session: `ring-${controllerId}`, host: target.host, port: target.port };
    }
    return null;
  }

  noteRelayBytes(session, direction, bytes) {
    let stats = this.relayStats.get(session);
    if (!stats) {
      stats = { opened: Date.now(), closed: 0, ws2tcp: 0, tcp2ws: 0, creditedBytes: 0 };
      this.relayStats.set(session, stats);
    }
    stats[direction] += bytes;
  }

  noteRelayClosed(session) {
    const stats = this.relayStats.get(session);
    if (stats) stats.closed = Date.now();
    this.flushContributions();
  }
}

module.exports = { Participation, TARGET_REPLICAS, WORKER_STALE_MS };
