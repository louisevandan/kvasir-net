/**
 * The frames a node and the relay exchange.
 *
 * p4 has no notion of a node that cannot be dialled. `deliver_outbound` opens a
 * TCP connection to the address in the envelope and writes; there is no route
 * table, no rendezvous, no hole punching. Worse for our purposes, a connection
 * an agent opened itself is receipts-only — a `Data` frame arriving on it is
 * answered with `peer_closed("unexpected frame on outbound hop")`. So a laptop
 * cannot open a tunnel outward and be given work down it. Not with p4 as it is.
 *
 * The relay is the way round that does not require changing the engine. It owns
 * a public address, the node keeps one outbound connection to it, and when
 * somebody dials the public address the relay carries those bytes down the
 * connection the node already has open. p4 on both sides sees an ordinary TCP
 * socket to an ordinary address, and neither end learns the relay exists.
 *
 * ## What this framing has to do
 *
 * Carry several p4 connections at once over one socket. An agent in a pipeline
 * is dialled by its ingress and by its neighbours, all at the same time, so a
 * single-stream tunnel would serialise the pipeline it is meant to join.
 *
 * Be byte-exact. p4's own framing (`P4E3`, `P4H1`) is layered inside this one
 * and must arrive unaltered; this layer never inspects or reorders a payload.
 *
 * Fail loudly per stream. One p4 connection dying must close that stream and
 * nothing else — the node stays registered and its other work continues.
 *
 * ## Shape
 *
 *   magic 'KVR1' | u8 type | u32 stream | u32 length | payload
 *
 * ## The handshake
 *
 *   relay -> node   CHALLENGE { nonce, message }
 *   node  -> relay  HELLO     { nodeId, owner, signature, agentPort, ... }
 *   relay -> node   WELCOME   { publicHost, publicPort } | { error }
 *
 * The node signs the relay's message with the operator wallet — the same
 * ed25519-over-base58 scheme the gateway already uses for admin sign-in, so a
 * node proves the same identity the rewards ledger is keyed on.
 *
 * Big-endian, because this is our wire and a reader in any language should be
 * able to pick it apart without wondering. p4's own frames are little-endian
 * and stay that way inside the payload, untouched.
 */

export const MAGIC = Buffer.from('KVR1', 'ascii');
export const HEADER_BYTES = 4 + 1 + 4 + 4;

/** The most a single frame may carry. p4 allows a 2 GiB socket frame; we do not
 *  need to, and an attacker should not be able to make us allocate one. */
export const MAX_PAYLOAD = 4 * 1024 * 1024;

export const TYPE = {
  // relay -> node, first frame. payload: JSON { nonce, message }.
  // A server-issued nonce rather than a client timestamp: a signature the node
  // made earlier, for a different relay, or for anything else, must not open a
  // tunnel here. One extra round trip on a connection that then lives for hours.
  CHALLENGE: 8,
  // node -> relay, answering the challenge. payload: JSON registration + signature.
  HELLO: 1,
  // relay -> node. payload: JSON { ok, publicHost, publicPort, ... } or { error }.
  WELCOME: 2,
  // relay -> node: somebody dialled the public port, here is a new stream.
  OPEN: 3,
  // either way: bytes belonging to a stream, verbatim.
  DATA: 4,
  // either way: that stream is finished. payload: short reason, may be empty.
  CLOSE: 5,
  // either way, stream 0. Keeps NAT tables and idle-timeout proxies awake.
  PING: 6,
  PONG: 7,
};

export const typeName = (value) =>
  Object.keys(TYPE).find((key) => TYPE[key] === value) ?? `unknown(${value})`;

/** One frame, ready to write. */
export function encode(type, stream, payload = Buffer.alloc(0)) {
  const body = Buffer.isBuffer(payload) ? payload : Buffer.from(String(payload), 'utf8');
  if (body.length > MAX_PAYLOAD) throw new Error(`frame payload ${body.length} exceeds ${MAX_PAYLOAD}`);
  const frame = Buffer.allocUnsafe(HEADER_BYTES + body.length);
  MAGIC.copy(frame, 0);
  frame.writeUInt8(type, 4);
  frame.writeUInt32BE(stream, 5);
  frame.writeUInt32BE(body.length, 9);
  body.copy(frame, HEADER_BYTES);
  return frame;
}

export const json = (type, stream, value) => encode(type, stream, Buffer.from(JSON.stringify(value), 'utf8'));

/**
 * Turn a byte stream back into frames.
 *
 * TCP gives no message boundaries, so a frame arrives in pieces or several
 * arrive together, and both happen constantly under load. This buffers until a
 * whole frame is present and hands it over; it never partially delivers one.
 *
 * A bad magic is fatal rather than resynchronised. Once the framing is lost
 * there is no safe way to guess where the next frame starts, and guessing would
 * feed a mangled payload into p4, which is far worse than dropping a connection.
 */
export function createParser(onFrame, onError) {
  let buffer = Buffer.alloc(0);
  let broken = false;
  return (chunk) => {
    if (broken) return;
    buffer = buffer.length ? Buffer.concat([buffer, chunk]) : chunk;
    for (;;) {
      if (buffer.length < HEADER_BYTES) return;
      if (!buffer.subarray(0, 4).equals(MAGIC)) {
        broken = true;
        onError(new Error('framing lost: bad magic'));
        return;
      }
      const length = buffer.readUInt32BE(9);
      if (length > MAX_PAYLOAD) {
        broken = true;
        onError(new Error(`framing lost: payload ${length} exceeds ${MAX_PAYLOAD}`));
        return;
      }
      if (buffer.length < HEADER_BYTES + length) return;
      const type = buffer.readUInt8(4);
      const stream = buffer.readUInt32BE(5);
      const payload = buffer.subarray(HEADER_BYTES, HEADER_BYTES + length);
      buffer = buffer.subarray(HEADER_BYTES + length);
      onFrame(type, stream, payload);
    }
  };
}
