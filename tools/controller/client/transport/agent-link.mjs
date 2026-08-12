import net from 'node:net';
import { randomUUID } from 'node:crypto';

const MAGIC = Buffer.from('P4B1');
const VERSION = 5;
const CANCEL = 5;
const TERMINAL = new Set([3, 4, 17, 35, 37, 39, 41, 43]);
const links = new Map();

export function agentLink(endpoint) {
  let link = links.get(endpoint);
  if (!link) {
    link = new AgentLink(endpoint);
    links.set(endpoint, link);
  }
  return link;
}

export function closeAgentLinks() {
  for (const link of links.values()) link.close();
  links.clear();
}

class AgentLink {
  #endpoint;
  #socket;
  #connecting;
  #decoder = new FrameDecoder();
  #pending = new Map();

  constructor(endpoint) {
    this.#endpoint = endpoint;
  }

  close() {
    const socket = this.#socket;
    this.#socket = undefined;
    socket?.destroy();
    this.#fail(new Error('P4 Agent link closed'));
  }

  async *exchange(packet, { signal, timeoutMs = 0 } = {}) {
    const routeId = randomUUID();
    const deadline = timeoutMs > 0 ? Date.now() + timeoutMs : 0;
    const queue = new AsyncQueue();
    this.#pending.set(routeId, queue);
    const cancel = (detail, error) => {
      this.#sendCancel(routeId, packet.requestId, detail);
      this.#pending.delete(routeId);
      queue.fail(error);
    };
    const abort = () => cancel('caller aborted', signal?.reason instanceof Error ? signal.reason : new Error('P4 request aborted'));
    const timer = timeoutMs > 0
      ? setTimeout(() => cancel('deadline exceeded', new Error(`P4 request exceeded ${timeoutMs}ms deadline`)), timeoutMs)
      : undefined;
    if (signal) {
      if (signal.aborted) abort();
      else signal.addEventListener('abort', abort, { once: true });
    }
    try {
      if (!signal?.aborted) {
        const socket = await this.#connect();
        socket.ref();
        await write(socket, routedFrame(routeId, deadline, packet.kind, packet.payload));
      }
      for await (const frame of queue) yield frame;
    } finally {
      if (signal) signal.removeEventListener('abort', abort);
      if (timer) clearTimeout(timer);
      this.#pending.delete(routeId);
      if (this.#pending.size === 0) this.#socket?.unref();
    }
  }

  async #connect() {
    if (this.#socket && !this.#socket.destroyed) return this.#socket;
    if (this.#connecting) return this.#connecting;
    this.#connecting = connect(this.#endpoint).then(socket => {
      this.#socket = socket;
      this.#decoder = new FrameDecoder();
      socket.on('data', chunk => this.#onData(chunk));
      socket.on('error', error => this.#fail(error));
      socket.on('close', () => this.#fail(new Error('P4 Agent connection closed')));
      return socket;
    }).finally(() => { this.#connecting = undefined; });
    return this.#connecting;
  }

  #onData(chunk) {
    try {
      for (const frame of this.#decoder.push(chunk)) {
        const queue = this.#pending.get(frame.routeId);
        if (!queue) continue;
        queue.push({ kind: frame.kind, payload: frame.payload });
        if (TERMINAL.has(frame.kind)) {
          this.#pending.delete(frame.routeId);
          queue.close();
          if (this.#pending.size === 0) this.#socket?.unref();
        }
      }
    } catch (error) {
      this.#socket?.destroy();
      this.#fail(error);
    }
  }

  #sendCancel(routeId, requestId, reason) {
    const socket = this.#socket;
    if (!socket || socket.destroyed || !requestId) return;
    void write(socket, routedFrame(routeId, 0, CANCEL, Buffer.concat([
      text(requestId), text(reason),
    ]))).catch(() => {});
  }

  #fail(error) {
    this.#socket = undefined;
    const pending = [...this.#pending.values()];
    this.#pending.clear();
    for (const queue of pending) queue.fail(error);
  }
}

function connect(endpoint) {
  const at = endpoint.lastIndexOf(':');
  const host = endpoint.slice(0, at);
  const port = Number(endpoint.slice(at + 1));
  if (at < 1 || !Number.isInteger(port)) throw new Error('endpoint must be host:port');
  return new Promise((resolve, reject) => {
    const socket = net.createConnection({ host, port, noDelay: true });
    socket.once('connect', () => resolve(socket));
    socket.once('error', reject);
  });
}

function write(socket, bytes) {
  return new Promise((resolve, reject) => socket.write(bytes, error => error ? reject(error) : resolve()));
}

function routedFrame(routeId, deadline, kind, messagePayload) {
  const route = Buffer.from(routeId, 'utf8');
  const payload = Buffer.allocUnsafe(12 + route.length + messagePayload.length);
  payload.writeUInt32LE(route.length, 0);
  route.copy(payload, 4);
  payload.writeBigUInt64LE(BigInt(deadline), 4 + route.length);
  messagePayload.copy(payload, 12 + route.length);
  const header = Buffer.alloc(16);
  MAGIC.copy(header);
  header[4] = VERSION;
  header[5] = kind;
  header.writeUInt32LE(payload.length, 8);
  return Buffer.concat([header, payload]);
}

function text(value) {
  const data = Buffer.from(String(value), 'utf8');
  const length = Buffer.allocUnsafe(4);
  length.writeUInt32LE(data.length);
  return Buffer.concat([length, data]);
}

class FrameDecoder {
  #pending = Buffer.alloc(0);
  push(chunk) {
    this.#pending = Buffer.concat([this.#pending, chunk]);
    const frames = [];
    while (this.#pending.length >= 16) {
      if (!this.#pending.subarray(0, 4).equals(MAGIC) || this.#pending[4] !== VERSION) throw new Error('unsupported P4 frame');
      const length = this.#pending.readUInt32LE(8);
      if (length > 1024 * 1024) throw new Error('P4 frame too large');
      if (this.#pending.length < 16 + length) break;
      const routed = this.#pending.subarray(16, 16 + length);
      const routeLength = routed.readUInt32LE(0);
      if (!routeLength || routed.length < 12 + routeLength) throw new Error('invalid P4 route envelope');
      frames.push({
        kind: this.#pending[5],
        routeId: routed.subarray(4, 4 + routeLength).toString('utf8'),
        payload: routed.subarray(12 + routeLength),
      });
      this.#pending = this.#pending.subarray(16 + length);
    }
    return frames;
  }
}

class AsyncQueue {
  #values = []; #waiters = []; #closed = false; #error;
  push(value) { const waiter = this.#waiters.shift(); if (waiter) waiter.resolve({ value, done: false }); else this.#values.push(value); }
  close() { this.#closed = true; while (this.#waiters.length) this.#waiters.shift().resolve({ done: true }); }
  fail(error) { if (this.#closed) return; this.#error = error; this.#closed = true; while (this.#waiters.length) this.#waiters.shift().reject(error); }
  [Symbol.asyncIterator]() { return this; }
  next() { if (this.#error) return Promise.reject(this.#error); if (this.#values.length) return Promise.resolve({ value: this.#values.shift(), done: false }); if (this.#closed) return Promise.resolve({ done: true }); return new Promise((resolve, reject) => this.#waiters.push({ resolve, reject })); }
}
