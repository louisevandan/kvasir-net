'use strict';
/**
 * OUTER client for the P4 event protocol (P4E3 envelopes inside P4H1
 * acknowledged frames), in plain Node with no dependencies.
 *
 * Ported field for field from the engine's own client,
 * `layers/adapters/hf/python/p4hfadapter/integration/transport/__init__.py`,
 * and checked against the encoder in `layers/protocol/src/event/wire.rs`
 * and the hop frames in `layers/protocol/src/event/hop.rs`.
 *
 * What the engine gives us and what it does not:
 *   - Delivery is acknowledged per frame (Hello → Data → Receipt → ReceiptAck).
 *     A refused receipt is a hard error; there is no retry queue here.
 *   - An OUTER endpoint has no dialable address. The reply target is the
 *     socket we already opened, identified by (ingress agent, channel,
 *     connection generation), so one client owns one connection.
 *   - There is no cancel. A submitted request runs until it finishes or its
 *     deadline passes; closing the socket only stops us from reading it.
 */
const net = require('node:net');
const crypto = require('node:crypto');

const EVENT_MAGIC = Buffer.from('P4E3', 'ascii');
const HOP_MAGIC = Buffer.from('P4H1', 'ascii');
const PROTOCOL_VERSION = 3;
const HOP_VERSION = 1;
const MAX_FRAME = 64 * 1024 * 1024;

const CLASS = { control: 0, data: 1, output: 2, telemetry: 3 };
const CLASS_NAME = ['control', 'data', 'output', 'telemetry'];
const HOP = { hello: 1, helloAck: 2, data: 3, receipt: 4, receiptAck: 5, query: 6, queryResult: 7 };
const RECEIPT = { 1: 'accepted_exact', 2: 'rejected', 3: 'conflict', 4: 'unknown' };

/* ---- encoding ---------------------------------------------------------- */

function text(value) {
  const body = Buffer.from(String(value), 'utf8');
  const head = Buffer.alloc(4);
  head.writeUInt32LE(body.length, 0);
  return Buffer.concat([head, body]);
}

function u16(value) { const b = Buffer.alloc(2); b.writeUInt16LE(value, 0); return b; }
function u32(value) { const b = Buffer.alloc(4); b.writeUInt32LE(value, 0); return b; }
function u64(value) { const b = Buffer.alloc(8); b.writeBigUInt64LE(BigInt(value), 0); return b; }

/** Endpoints are values: agent address, node on an agent, or an OUTER socket. */
function agentEndpoint(address) { return { kind: 0, address }; }
function nodeEndpoint(agent, node, generation) { return { kind: 1, agent, node, generation }; }
function outerEndpoint(ingressAgent, channel, connectionGeneration) {
  return { kind: 2, ingressAgent, channel, connectionGeneration };
}

function encodeEndpoint(endpoint) {
  if (endpoint.kind === 0) return Buffer.concat([Buffer.from([0]), text(endpoint.address)]);
  if (endpoint.kind === 1) {
    return Buffer.concat([Buffer.from([1]), text(endpoint.agent), text(endpoint.node), u64(endpoint.generation)]);
  }
  return Buffer.concat([
    Buffer.from([2]), text(endpoint.ingressAgent), text(endpoint.channel), u64(endpoint.connectionGeneration),
  ]);
}

/** The return route is an OUTER endpoint written without its kind byte. */
function encodeOuterBody(endpoint) {
  return Buffer.concat([
    text(endpoint.ingressAgent), text(endpoint.channel), u64(endpoint.connectionGeneration),
  ]);
}

function encodeEvent({ eventId, correlationId, source, target, returnRoute, eventClass, sequence, deadlineUnixMs, adapterKind, contentType, payload }) {
  const parts = [
    u16(PROTOCOL_VERSION), text(eventId), text(correlationId), Buffer.from([0]),
    encodeEndpoint(source), encodeEndpoint(target),
  ];
  parts.push(returnRoute ? Buffer.concat([Buffer.from([1]), encodeOuterBody(returnRoute)]) : Buffer.from([0]));
  parts.push(Buffer.from([CLASS[eventClass] ?? 0]));
  parts.push(u64(sequence));
  parts.push(deadlineUnixMs ? Buffer.concat([Buffer.from([1]), u64(deadlineUnixMs)]) : Buffer.from([0]));
  parts.push(adapterKind ? Buffer.concat([Buffer.from([1]), text(adapterKind)]) : Buffer.from([0]));
  parts.push(text(contentType));
  const envelope = Buffer.concat(parts);
  const body = payload ?? Buffer.alloc(0);
  return Buffer.concat([EVENT_MAGIC, u32(envelope.length), u32(body.length), envelope, body]);
}

/* ---- decoding ---------------------------------------------------------- */

class Cursor {
  constructor(buffer) { this.buffer = buffer; this.offset = 0; }
  take(n) {
    if (this.offset + n > this.buffer.length) throw new Error('truncated P4 envelope');
    const slice = this.buffer.subarray(this.offset, this.offset + n);
    this.offset += n;
    return slice;
  }
  byte() { return this.take(1)[0]; }
  u16() { return this.take(2).readUInt16LE(0); }
  u32() { return this.take(4).readUInt32LE(0); }
  u64() { return Number(this.take(8).readBigUInt64LE(0)); }
  text() { return this.take(this.u32()).toString('utf8'); }
  optional(read) {
    const flag = this.byte();
    if (flag !== 0 && flag !== 1) throw new Error('invalid optional flag');
    return flag ? read() : null;
  }
  endpoint() {
    const kind = this.byte();
    if (kind === 0) return { kind, address: this.text() };
    if (kind === 1) return { kind, agent: this.text(), node: this.text(), generation: this.u64() };
    if (kind === 2) return { kind, ingressAgent: this.text(), channel: this.text(), connectionGeneration: this.u64() };
    throw new Error('invalid endpoint kind');
  }
  get finished() { return this.offset === this.buffer.length; }
}

function decodeEvent(frame) {
  if (frame.length < 12 || !frame.subarray(0, 4).equals(EVENT_MAGIC)) throw new Error('invalid P4 event magic');
  const envelopeLen = frame.readUInt32LE(4);
  const payloadLen = frame.readUInt32LE(8);
  if (frame.length !== 12 + envelopeLen + payloadLen) throw new Error('P4 event frame length mismatch');
  const cursor = new Cursor(frame.subarray(12, 12 + envelopeLen));
  if (cursor.u16() !== PROTOCOL_VERSION) throw new Error('unsupported P4 event version');
  const meta = {
    eventId: cursor.text(),
    correlationId: cursor.text(),
    causationId: cursor.optional(() => cursor.text()),
    source: cursor.endpoint(),
    target: cursor.endpoint(),
    returnRoute: cursor.optional(() => ({
      kind: 2, ingressAgent: cursor.text(), channel: cursor.text(), connectionGeneration: cursor.u64(),
    })),
    eventClass: CLASS_NAME[cursor.byte()],
    sequence: cursor.u64(),
    deadlineUnixMs: cursor.optional(() => cursor.u64()),
    adapterKind: cursor.optional(() => cursor.text()),
    contentType: cursor.text(),
  };
  if (!cursor.finished) throw new Error('trailing P4 envelope bytes');
  return { meta, payload: frame.subarray(12 + envelopeLen) };
}

/* ---- hop frames -------------------------------------------------------- */

function hopHeader(kind) {
  return Buffer.concat([HOP_MAGIC, u16(HOP_VERSION), u16(0), Buffer.from([kind])]);
}

function hopHello(senderId, generation, maxOutstanding, maxReceiptBytes) {
  return Buffer.concat([
    hopHeader(HOP.hello), text(senderId), u64(generation), u32(maxOutstanding), u64(maxReceiptBytes),
  ]);
}

function hopData(attempt, event) {
  const digest = crypto.createHash('sha256').update(event).digest();
  return {
    digest,
    frame: Buffer.concat([hopHeader(HOP.data), u64(attempt), digest, u32(event.length), event]),
  };
}

function hopReceipt(kind, attempt, digest, status = 1, detail = '') {
  return Buffer.concat([
    hopHeader(kind === 'receipt' ? HOP.receipt : HOP.queryResult),
    u64(attempt), digest, Buffer.from([status]), text(detail),
  ]);
}

function hopReceiptAck(senderId, generation, attempt, digest) {
  return Buffer.concat([hopHeader(HOP.receiptAck), text(senderId), u64(generation), u64(attempt), digest]);
}

function decodeHop(frame) {
  if (frame.length < 9 || !frame.subarray(0, 4).equals(HOP_MAGIC)) throw new Error('invalid P4 hop magic');
  if (frame.readUInt16LE(4) !== HOP_VERSION || frame.readUInt16LE(6) !== 0) throw new Error('unsupported P4 hop version');
  const kind = frame[8];
  const cursor = new Cursor(frame.subarray(9));
  let value;
  if (kind === HOP.helloAck) {
    value = {
      kind: 'hello_ack',
      acceptedGeneration: cursor.u64(),
      senderId: cursor.text(),
      generation: cursor.u64(),
      maxOutstanding: cursor.u32(),
      maxReceiptBytes: cursor.u64(),
    };
  } else if (kind === HOP.data) {
    const attempt = cursor.u64();
    const digest = Buffer.from(cursor.take(32));
    const event = Buffer.from(cursor.take(cursor.u32()));
    if (!crypto.createHash('sha256').update(event).digest().equals(digest)) throw new Error('P4 hop data digest mismatch');
    value = { kind: 'data', attempt, digest, event };
  } else if (kind === HOP.receipt || kind === HOP.queryResult) {
    value = {
      kind: kind === HOP.receipt ? 'receipt' : 'query_result',
      attempt: cursor.u64(),
      digest: Buffer.from(cursor.take(32)),
      status: cursor.byte(),
      detail: cursor.text(),
    };
    if (!RECEIPT[value.status]) throw new Error('invalid P4 hop receipt status');
  } else if (kind === HOP.receiptAck || kind === HOP.query) {
    value = {
      kind: kind === HOP.receiptAck ? 'receipt_ack' : 'query',
      senderId: cursor.text(),
      generation: cursor.u64(),
      attempt: cursor.u64(),
      digest: Buffer.from(cursor.take(32)),
    };
  } else {
    throw new Error(`unexpected P4 hop frame kind ${kind}`);
  }
  if (!cursor.finished) throw new Error('trailing P4 hop bytes');
  return value;
}

/* ---- client ------------------------------------------------------------ */

/**
 * One OUTER connection. Events arrive out of band, so callers either await a
 * correlated reply (`exchange`) or subscribe to the stream (`onEvent`).
 */
class OuterClient {
  constructor({ host, port, address, channel, connectionGeneration = 1, maxOutstanding = 256, deadlineMs = 120_000, connectTimeoutMs = 0, hop = true }) {
    this.host = host;
    this.port = port;
    this.address = address ?? `tcp://${host}:${port}`;
    this.channel = channel ?? crypto.randomBytes(16).toString('hex');
    this.outer = outerEndpoint(this.address, this.channel, connectionGeneration);
    this.deadlineMs = deadlineMs;
    // Not the same clock as deadlineMs, which rides on events. This one bounds
    // the TCP handshake, which otherwise has no bound at all.
    this.connectTimeoutMs = connectTimeoutMs;
    // Agents built before the acknowledged-delivery frames landed accept bare
    // P4E3 events and close the socket on a P4H1 hello. `hop: false` speaks to
    // those; `connect()` falls back to it automatically.
    this.hop = hop;
    this.sequence = 0;
    this.attempt = 1;
    this.maxOutstanding = maxOutstanding;
    this.senderId = `${this.address}#${this.channel}`;
    this.outstanding = new Map();     // attempt -> {digest, resolve, reject}
    this.received = new Map();        // attempt -> digest
    this.waiters = new Map();         // correlationId -> {resolve, reject}
    this.subscriptions = new Map();   // correlationId -> handler(event)
    this.listeners = new Set();
    this.buffer = Buffer.alloc(0);
    this.socket = null;
    this.closed = false;
    this.finishing = false;
    this.sentBytes = 0;
    this.receivedBytes = 0;
  }

  connect() {
    return new Promise((resolve, reject) => {
      const socket = net.createConnection({ host: this.host, port: this.port });
      socket.setNoDelay(true);
      const onError = (error) => { socket.destroy(); reject(error); };
      socket.once('error', onError);
      // net.createConnection carries no connect deadline of its own: a host
      // that drops SYNs silently leaves this pending until the OS gives up,
      // which is about two minutes on Linux. The bridge dials on a 15 s timer,
      // so without a bound here the attempts stack. Cleared once connected --
      // an established connection that sits idle between INSPECTs is normal
      // and must not be torn down for being quiet.
      if (this.connectTimeoutMs) {
        socket.setTimeout(this.connectTimeoutMs, () => {
          socket.destroy(new Error(`P4 connect timed out after ${this.connectTimeoutMs} ms`));
        });
      }
      socket.once('connect', () => {
        socket.setTimeout(0);
        socket.off('error', onError);
        this.socket = socket;
        socket.on('data', (chunk) => this._onData(chunk));
        socket.on('error', (error) => this._fail(error));
        socket.on('close', () => this._fail(new Error('P4 connection closed')));
        if (!this.hop) { resolve(this); return; }
        this._helloResolve = resolve;
        this._helloReject = reject;
        this._sendFrame(hopHello(this.senderId, this.outer.connectionGeneration, this.maxOutstanding, 64 * 1024 * 1024));
      });
    });
  }

  onEvent(listener) { this.listeners.add(listener); return () => this.listeners.delete(listener); }

  /**
   * Route every event that carries this correlation id to one handler. A
   * request's outputs, observations and terminal all share its id, so a
   * one-shot waiter cannot carry them.
   */
  subscribe(correlationId, handler) {
    this.subscriptions.set(correlationId, handler);
    return () => this.subscriptions.delete(correlationId);
  }

  _sendFrame(frame) {
    if (!this.socket) throw new Error('P4 connection is not open');
    this.socket.write(Buffer.concat([u32(frame.length), frame]));
    this.sentBytes += frame.length + 4;
  }

  _fail(error) {
    if (this.closed) return;
    this.closed = true;
    for (const waiter of this.waiters.values()) waiter.reject(error);
    this.waiters.clear();
    for (const pending of this.outstanding.values()) pending.reject?.(error);
    this.outstanding.clear();
    if (this._helloReject) { this._helloReject(error); this._helloReject = null; this._helloResolve = null; }
    for (const listener of this.listeners) listener({ error });
  }

  _onData(chunk) {
    this.buffer = this.buffer.length ? Buffer.concat([this.buffer, chunk]) : chunk;
    for (;;) {
      if (this.buffer.length < 4) return;
      const size = this.buffer.readUInt32LE(0);
      if (size === 0) {                       // FINISH acknowledgement
        this.buffer = this.buffer.subarray(4);
        this._finishAck?.();
        continue;
      }
      if (size > MAX_FRAME) { this._fail(new Error(`P4 frame exceeds bound: ${size}`)); return; }
      if (this.buffer.length < 4 + size) return;
      const frame = this.buffer.subarray(4, 4 + size);
      this.buffer = this.buffer.subarray(4 + size);
      this.receivedBytes += size + 4;
      try {
        if (this.hop) this._onFrame(frame);
        else this._deliver(decodeEvent(frame));
      } catch (error) {
        this._fail(error);
        return;
      }
    }
  }

  _onFrame(frame) {
    const hop = decodeHop(frame);
    if (hop.kind === 'hello_ack') {
      if (hop.acceptedGeneration !== this.outer.connectionGeneration) throw new Error('P4 hop hello acknowledgement mismatch');
      this.peerSenderId = hop.senderId;
      this.peerGeneration = hop.generation;
      this.maxOutstanding = Math.min(this.maxOutstanding, hop.maxOutstanding);
      const resolve = this._helloResolve;
      this._helloResolve = null; this._helloReject = null;
      resolve?.(this);
      return;
    }
    if (hop.kind === 'receipt') {
      const pending = this.outstanding.get(hop.attempt);
      if (!pending || !pending.digest.equals(hop.digest)) throw new Error('P4 hop receipt conflicts with outstanding request');
      this.outstanding.delete(hop.attempt);
      if (hop.status !== 1) {
        const error = new Error(`P4 refused the request: ${RECEIPT[hop.status]} ${hop.detail}`.trim());
        const waiter = this.waiters.get(pending.correlationId);
        if (waiter) { this.waiters.delete(pending.correlationId); waiter.reject(error); }
        // A refusal for a request nobody is waiting on is not a protocol
        // violation, and throwing here reaches _onData's catch and tears the
        // connection down -- taking every other request on it, and an agent
        // slot with it (transport.rs:900-902). That became a NORMAL situation
        // the day the bridge started keeping connections through an INSPECT
        // timeout: the waiter is gone by design, and a late refusal would then
        // cost the slot the timeout was meant to save. Subscribed requests
        // (pipeline.js) were always in this position -- they own no waiter at
        // all, so a refusal for one used to kill the connection outright.
        // Hand it to the listeners and keep the socket. Found by GB10 #1 in
        // wire.js, 2026-09-25. Reachable only with P4_BRIDGE_HOP=1.
        else for (const listener of this.listeners) listener({ error, correlationId: pending.correlationId });
        return;
      }
      this._sendFrame(hopReceiptAck(this.senderId, this.outer.connectionGeneration, hop.attempt, hop.digest));
      return;
    }
    if (hop.kind === 'data') {
      const seen = this.received.get(hop.attempt);
      if (seen) {
        const same = seen.equals(hop.digest);
        this._sendFrame(hopReceipt('receipt', hop.attempt, hop.digest, same ? 1 : 3, same ? '' : 'attempt digest differs'));
        return;
      }
      this.received.set(hop.attempt, hop.digest);
      this._sendFrame(hopReceipt('receipt', hop.attempt, hop.digest));
      this._deliver(decodeEvent(hop.event));
      return;
    }
    if (hop.kind === 'receipt_ack') {
      const seen = this.received.get(hop.attempt);
      if (hop.senderId === this.peerSenderId && hop.generation === this.peerGeneration && seen?.equals(hop.digest)) {
        this.received.delete(hop.attempt);
      }
      return;
    }
    if (hop.kind === 'query') {
      const seen = this.received.get(hop.attempt);
      const status = !seen ? 4 : (seen.equals(hop.digest) ? 1 : 3);
      const detail = status === 4 ? 'receipt is not pinned' : (status === 3 ? 'attempt digest differs' : '');
      this._sendFrame(hopReceipt('query_result', hop.attempt, hop.digest, status, detail));
      return;
    }
    throw new Error(`unexpected P4 hop frame ${hop.kind}`);
  }

  _deliver(event) {
    const subscription = this.subscriptions.get(event.meta.correlationId);
    if (subscription) { subscription(event); return; }
    const waiter = this.waiters.get(event.meta.correlationId);
    if (waiter) {
      this.waiters.delete(event.meta.correlationId);
      waiter.resolve(event);
      return;
    }
    for (const listener of this.listeners) listener(event);
  }

  /** Send one event. Resolves with its event id once the hop receipt lands. */
  send(target, contentType, payload, { adapterKind = null, eventClass = 'control', deadlineMs = this.deadlineMs, correlationId = null } = {}) {
    if (this.finishing) throw new Error('P4 connection is finishing');
    if (this.closed) throw new Error('P4 connection is closed');
    this.sequence += 1;
    const eventId = `${this.channel}:${this.sequence}`;
    const event = encodeEvent({
      eventId,
      correlationId: correlationId ?? eventId,
      source: this.outer,
      target,
      returnRoute: this.outer,
      eventClass,
      sequence: this.sequence,
      deadlineUnixMs: Date.now() + deadlineMs,
      adapterKind,
      contentType,
      payload: Buffer.isBuffer(payload) ? payload : Buffer.from(payload ?? '', 'utf8'),
    });
    if (!this.hop) {
      this._sendFrame(event);
      return eventId;
    }
    const attempt = this.attempt++;
    const { digest, frame } = hopData(attempt, event);
    this.outstanding.set(attempt, { digest, correlationId: correlationId ?? eventId });
    this._sendFrame(frame);
    return eventId;
  }

  /** Send and await the correlated reply. */
  exchange(target, contentType, payload, options = {}) {
    const timeoutMs = options.timeoutMs ?? this.deadlineMs;
    return new Promise((resolve, reject) => {
      let key;
      const timer = setTimeout(() => {
        if (key) this.waiters.delete(key);
        reject(new Error(`P4 request timed out after ${timeoutMs} ms (${contentType})`));
      }, timeoutMs);
      try {
        const eventId = this.send(target, contentType, payload, options);
        key = options.correlationId ?? eventId;
      } catch (error) {
        clearTimeout(timer);
        reject(error);
        return;
      }
      this.waiters.set(key, {
        resolve: (event) => { clearTimeout(timer); resolve(event); },
        reject: (error) => { clearTimeout(timer); reject(error); },
      });
    });
  }

  /** Ask an agent for its snapshot: nodes, capability, occupancy, transport. */
  async inspect(agentAddress, options = {}) {
    const event = await this.exchange(agentEndpoint(agentAddress), 'application/vnd.p4.agent.inspect-v1+json', '{}', options);
    if (event.meta.contentType !== 'application/vnd.p4.agent.snapshot-v1+json') {
      throw new Error(`unexpected INSPECT reply: ${event.meta.contentType}`);
    }
    return JSON.parse(event.payload.toString('utf8'));
  }

  /** Retire the socket. Not a settlement: outstanding work is unaffected. */
  finish(timeoutMs = 10_000) {
    if (this.finishing) return Promise.resolve();
    this.finishing = true;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error('P4 finish timed out')), timeoutMs);
      this._finishAck = () => { clearTimeout(timer); this.close(); resolve(); };
      try {
        this._sendFrame(Buffer.alloc(0));
      } catch (error) {
        clearTimeout(timer);
        reject(error);
      }
    });
  }

  close() {
    this.closed = true;
    this.socket?.destroy();
    this.socket = null;
  }
}

/**
 * Open a connection, falling back to bare events when the agent refuses the
 * acknowledged handshake (older builds close the socket instead of answering).
 */
async function connect(options) {
  const wanted = options.hop ?? true;
  try {
    const client = new OuterClient(options);
    await client.connect();
    return client;
  } catch (error) {
    if (!wanted || options.hop === false) throw error;
    const client = new OuterClient({ ...options, hop: false });
    await client.connect();
    return client;
  }
}

module.exports = {
  OuterClient, connect,
  agentEndpoint, nodeEndpoint, outerEndpoint,
  encodeEvent, decodeEvent, decodeHop,
  CLASS, RECEIPT,
};
