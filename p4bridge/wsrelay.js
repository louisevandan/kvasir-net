'use strict';
/**
 * A WebSocket ⟷ TCP byte bridge, hand-written so the bridge keeps no
 * dependencies.
 *
 * This exists because a phone cannot be dialled. Carrier NAT and Cloudflare's
 * 80/443-only edge mean neither side can open a socket to the other, so both
 * dial out to here and this splices them. What crosses is opaque: ring stage
 * traffic in one case, expert dispatch in the other. Nothing here parses it.
 *
 * Two details that are easy to get wrong and expensive to debug:
 *
 *   - Server frames are never masked, client frames always are. Sending a
 *     masked frame to a browser or to URLSession fails the connection with no
 *     useful message.
 *   - Only binary frames. The iOS client decodes a text frame as UTF-8 and the
 *     Android one ignores the opcode, so a stray text frame corrupts the
 *     stream in one client and vanishes in the other.
 *
 * The first byte a ring peer sends is its role preamble ('P' predecessor, 'N'
 * successor). It is payload here, not protocol: it must not be buffered,
 * inspected or reordered.
 */
const crypto = require('node:crypto');
const net = require('node:net');

const GUID = '258EAFA5-E914-47DA-95CA-C5AB0DC85B11';
const OP_CONTINUATION = 0x0;
const OP_TEXT = 0x1;
const OP_BINARY = 0x2;
const OP_CLOSE = 0x8;
const OP_PING = 0x9;
const OP_PONG = 0xa;

function acceptKey(key) {
  return crypto.createHash('sha1').update(key + GUID).digest('base64');
}

/** Encode one unmasked server frame. */
function encodeFrame(opcode, payload = Buffer.alloc(0)) {
  const length = payload.length;
  let header;
  if (length < 126) {
    header = Buffer.alloc(2);
    header[1] = length;
  } else if (length < 65536) {
    header = Buffer.alloc(4);
    header[1] = 126;
    header.writeUInt16BE(length, 2);
  } else {
    header = Buffer.alloc(10);
    header[1] = 127;
    header.writeBigUInt64BE(BigInt(length), 2);
  }
  header[0] = 0x80 | opcode;
  return Buffer.concat([header, payload]);
}

function closeFrame(code, reason = '') {
  const body = Buffer.alloc(2 + Buffer.byteLength(reason));
  body.writeUInt16BE(code, 0);
  body.write(reason, 2);
  return encodeFrame(OP_CLOSE, body);
}

/**
 * Incremental decoder for masked client frames.
 * Calls `onData` for each payload chunk of a data frame, in order.
 */
class FrameDecoder {
  constructor({ onData, onPing, onClose }) {
    this.buffer = Buffer.alloc(0);
    this.onData = onData;
    this.onPing = onPing;
    this.onClose = onClose;
  }

  push(chunk) {
    this.buffer = this.buffer.length ? Buffer.concat([this.buffer, chunk]) : chunk;
    for (;;) {
      if (this.buffer.length < 2) return;
      const first = this.buffer[0];
      const second = this.buffer[1];
      const opcode = first & 0x0f;
      const masked = (second & 0x80) !== 0;
      let length = second & 0x7f;
      let offset = 2;
      if (length === 126) {
        if (this.buffer.length < offset + 2) return;
        length = this.buffer.readUInt16BE(offset);
        offset += 2;
      } else if (length === 127) {
        if (this.buffer.length < offset + 8) return;
        const big = this.buffer.readBigUInt64BE(offset);
        if (big > BigInt(Number.MAX_SAFE_INTEGER)) { this.onClose(1009, 'frame too large'); return; }
        length = Number(big);
        offset += 8;
      }
      let mask = null;
      if (masked) {
        if (this.buffer.length < offset + 4) return;
        mask = this.buffer.subarray(offset, offset + 4);
        offset += 4;
      }
      if (this.buffer.length < offset + length) return;
      const payload = Buffer.from(this.buffer.subarray(offset, offset + length));
      this.buffer = this.buffer.subarray(offset + length);
      if (mask) {
        for (let i = 0; i < payload.length; i += 1) payload[i] ^= mask[i & 3];
      }
      if (opcode === OP_BINARY || opcode === OP_CONTINUATION || opcode === OP_TEXT) {
        if (payload.length) this.onData(payload);
      } else if (opcode === OP_PING) {
        this.onPing(payload);
      } else if (opcode === OP_CLOSE) {
        this.onClose(1000, 'peer closed');
        return;
      }
      // OP_PONG needs no answer.
    }
  }
}

/**
 * Accept an upgrade and splice it to a TCP endpoint.
 *
 * @param {import('node:http').IncomingMessage} req
 * @param {import('node:net').Socket} socket
 * @param {Buffer} head bytes already read past the request headers
 * @param {{host: string, port: number}} target
 * @param {{onBytes?: (direction: 'ws2tcp'|'tcp2ws', bytes: number) => void,
 *          onClose?: () => void, log?: (line: string) => void}} hooks
 */
function bridge(req, socket, head, target, hooks = {}) {
  const log = hooks.log ?? (() => {});
  const key = req.headers['sec-websocket-key'];
  if (!key) { socket.destroy(); return; }

  socket.write(
    'HTTP/1.1 101 Switching Protocols\r\n'
    + 'Upgrade: websocket\r\n'
    + 'Connection: Upgrade\r\n'
    + `Sec-WebSocket-Accept: ${acceptKey(key)}\r\n\r\n`);
  socket.setNoDelay(true);

  const upstream = net.connect({ host: target.host, port: target.port });
  upstream.setNoDelay(true);

  let closed = false;
  const teardown = (why) => {
    if (closed) return;
    closed = true;
    log(`relay closed: ${why}`);
    try { socket.end(closeFrame(1000, 'bridge closed')); } catch { /* already gone */ }
    socket.destroy();
    upstream.destroy();
    hooks.onClose?.();
  };

  upstream.on('connect', () => log(`relay open -> ${target.host}:${target.port}`));
  upstream.on('error', (error) => {
    // Nothing is listening on the far side yet, or it went away mid-session.
    // 1011 rather than a silent close, so the client logs a cause.
    if (!closed) {
      try { socket.end(closeFrame(1011, 'upstream unavailable')); } catch { /* ignore */ }
    }
    teardown(`upstream error: ${error.message}`);
  });
  upstream.on('close', () => teardown('upstream closed'));

  upstream.on('data', (chunk) => {
    hooks.onBytes?.('tcp2ws', chunk.length);
    socket.write(encodeFrame(OP_BINARY, chunk));
  });

  const decoder = new FrameDecoder({
    onData: (payload) => {
      hooks.onBytes?.('ws2tcp', payload.length);
      upstream.write(payload);
    },
    onPing: (payload) => socket.write(encodeFrame(OP_PONG, payload)),
    onClose: () => teardown('client closed'),
  });

  if (head && head.length) decoder.push(head);
  socket.on('data', (chunk) => decoder.push(chunk));
  socket.on('error', (error) => teardown(`socket error: ${error.message}`));
  socket.on('close', () => teardown('socket closed'));
}

/** Refuse an upgrade with a WebSocket close code, after accepting the upgrade
 *  so the client sees the code rather than a bare TCP reset. */
function refuse(req, socket, code, reason) {
  const key = req.headers['sec-websocket-key'];
  if (key) {
    socket.write(
      'HTTP/1.1 101 Switching Protocols\r\n'
      + 'Upgrade: websocket\r\n'
      + 'Connection: Upgrade\r\n'
      + `Sec-WebSocket-Accept: ${acceptKey(key)}\r\n\r\n`);
    socket.end(closeFrame(code, reason));
  }
  socket.destroy();
}

module.exports = { bridge, refuse, encodeFrame, closeFrame, FrameDecoder, acceptKey };
