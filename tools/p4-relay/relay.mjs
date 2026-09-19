#!/usr/bin/env node
/**
 * A dialable address for a node that has not got one.
 *
 * p4 reaches a node by opening a TCP connection to the address in the envelope.
 * There is no route table, no rendezvous and no hole punching, and a connection
 * the agent opened itself will not carry work inbound — a `Data` frame on it is
 * answered with `peer_closed("unexpected frame on outbound hop")`. A laptop
 * behind NAT therefore cannot join a p4 pipeline at all, however willing it is.
 *
 * This is the way round that needs no change to the engine. The relay holds a
 * public address; the node keeps one outbound connection to the relay; when
 * somebody dials the public address the relay carries those bytes down the
 * connection the node already has. Both ends of p4 see an ordinary socket to an
 * ordinary address and neither learns the relay is there.
 *
 * ## What it refuses to be
 *
 * **It does not read p4.** Payloads are forwarded byte for byte and never
 * parsed. The relay cannot tell a LOAD from an INSPECT and must not be able to:
 * the moment it understands the traffic it becomes something that can alter it.
 *
 * **It is not a scheduler.** It does not choose placement, rank nodes, or know
 * what a stage is. A node is an address here and nothing more.
 *
 * **It is not a trust anchor for rewards.** It knows which wallet opened a
 * tunnel, which is worth recording, but bytes through a relay are not proof of
 * inference and this must never be presented as a contribution measure.
 *
 * ## Why it is also the authentication boundary
 *
 * p4 has no authentication of any kind — no TLS, no tokens, no allowlist. Any
 * host that can reach an agent's port may send NODE_LOAD, NODE_UNLOAD and
 * INSPECT. Opening such a port on a contributor's home machine would be
 * indefensible. Here the node never listens publicly at all: it holds one
 * outbound connection, proves a wallet before that connection carries anything,
 * and the relay decides who may dial in. The reachability problem and the
 * authentication problem have the same answer, which is the main argument for
 * solving it this way rather than with port forwarding.
 *
 * Usage:  kvasir-p4-relay            (configured by environment, see README)
 */
import net from 'node:net';
import { TYPE, encode, json, createParser, typeName } from './protocol.mjs';
import { newChallenge, verify } from './auth.mjs';

const CONTROL_PORT = Number(process.env.KVASIR_RELAY_PORT ?? 43000);
const BIND = process.env.KVASIR_RELAY_BIND ?? '0.0.0.0';
const ADVERTISED = process.env.KVASIR_RELAY_HOST ?? '127.0.0.1';
const PORT_FROM = Number(process.env.KVASIR_RELAY_PORT_FROM ?? 43100);
const PORT_TO = Number(process.env.KVASIR_RELAY_PORT_TO ?? 43199);
// Who may dial a node's public port. Empty means anyone, which is right for a
// devnet on a private network and wrong on the open internet — the README says
// so where an operator will read it.
const CALLERS = (process.env.KVASIR_RELAY_ALLOW_FROM ?? '').split(',').map((s) => s.trim()).filter(Boolean);
const IDLE_MS = Number(process.env.KVASIR_RELAY_IDLE_MS ?? 90_000);

const log = (...parts) => console.log(new Date().toISOString(), ...parts);

/** Public ports in use, so two nodes never land on one. */
const taken = new Set();
function claimPort() {
  for (let port = PORT_FROM; port <= PORT_TO; port += 1) {
    if (!taken.has(port)) { taken.add(port); return port; }
  }
  return null;
}

/** Everything the relay holds for one connected node. */
class Node {
  constructor(socket) {
    this.socket = socket;
    this.peer = `${socket.remoteAddress}:${socket.remotePort}`;
    this.nodeId = null;
    this.owner = null;
    this.publicPort = null;
    this.listener = null;
    this.streams = new Map();     // stream id -> the dialer's socket
    this.nextStream = 1;
    this.challenge = null;
    this.lastSeen = Date.now();
    this.openedStreams = 0;
    this.bytesIn = 0;
    this.bytesOut = 0;
  }

  send(type, stream, payload) {
    if (this.socket.destroyed) return false;
    return this.socket.write(encode(type, stream, payload));
  }

  /**
   * Take a dialer's socket and give it a stream on this node's tunnel.
   *
   * Backpressure runs in both directions and matters more than it looks: a
   * pipeline stage can push hidden states faster than a home uplink drains, and
   * without pausing, the relay buffers the difference until it dies.
   */
  attach(dialer) {
    const id = this.nextStream;
    this.nextStream = (this.nextStream + 1) >>> 0 || 1;
    this.streams.set(id, dialer);
    this.openedStreams += 1;
    this.send(TYPE.OPEN, id, Buffer.from(JSON.stringify({ from: `${dialer.remoteAddress}` }), 'utf8'));

    dialer.on('data', (chunk) => {
      this.bytesIn += chunk.length;
      if (!this.send(TYPE.DATA, id, chunk)) {
        dialer.pause();
        this.socket.once('drain', () => dialer.resume());
      }
    });
    const finish = (why) => {
      if (!this.streams.delete(id)) return;
      this.send(TYPE.CLOSE, id, Buffer.from(why, 'utf8'));
      dialer.destroy();
    };
    dialer.on('end', () => finish('dialer ended'));
    dialer.on('close', () => finish('dialer closed'));
    dialer.on('error', (error) => finish(`dialer error: ${error.message}`));
    return id;
  }

  /** Bytes coming back up the tunnel, headed for whoever dialled. */
  toDialer(id, payload) {
    const dialer = this.streams.get(id);
    if (!dialer || dialer.destroyed) return;
    this.bytesOut += payload.length;
    if (!dialer.write(payload)) {
      this.socket.pause();
      dialer.once('drain', () => this.socket.resume());
    }
  }

  closeStream(id, why) {
    const dialer = this.streams.get(id);
    if (!dialer) return;
    this.streams.delete(id);
    dialer.end();
  }

  teardown(why) {
    for (const [id] of this.streams) this.closeStream(id, why);
    if (this.listener) { this.listener.close(); this.listener = null; }
    if (this.publicPort) { taken.delete(this.publicPort); }
    if (!this.socket.destroyed) this.socket.destroy();
    if (this.nodeId) {
      log(`node ${this.nodeId} gone (${why}) — ${this.openedStreams} stream(s), ` +
        `${this.bytesIn} B in, ${this.bytesOut} B out`);
      nodes.delete(this.nodeId);
    }
  }
}

const nodes = new Map();          // nodeId -> Node

/** Open this node's public port and start accepting dials for it. */
function openPublicPort(node) {
  return new Promise((resolve) => {
    const port = claimPort();
    if (port == null) return resolve({ error: 'the relay has no free ports' });
    const listener = net.createServer((dialer) => {
      const from = dialer.remoteAddress ?? '';
      if (CALLERS.length && !CALLERS.some((allowed) => from.endsWith(allowed))) {
        log(`refused a dial to ${node.nodeId} from ${from}: not in KVASIR_RELAY_ALLOW_FROM`);
        dialer.destroy();
        return;
      }
      dialer.setNoDelay(true);
      node.attach(dialer);
    });
    listener.on('error', (error) => {
      taken.delete(port);
      resolve({ error: `cannot open ${port}: ${error.message}` });
    });
    listener.listen(port, BIND, () => {
      node.publicPort = port;
      node.listener = listener;
      resolve({ port });
    });
  });
}

const control = net.createServer((socket) => {
  socket.setNoDelay(true);
  const node = new Node(socket);
  node.challenge = newChallenge(ADVERTISED);
  node.send(TYPE.CHALLENGE, 0, Buffer.from(JSON.stringify({
    nonce: node.challenge.nonce, relayHost: ADVERTISED,
  }), 'utf8'));

  const reject = (why) => {
    log(`refused ${node.peer}: ${why}`);
    node.send(TYPE.WELCOME, 0, Buffer.from(JSON.stringify({ error: why }), 'utf8'));
    setTimeout(() => node.teardown(why), 50);
  };

  const onFrame = async (type, stream, payload) => {
    node.lastSeen = Date.now();
    switch (type) {
      case TYPE.HELLO: {
        if (node.nodeId) return reject('already registered on this connection');
        let hello;
        try { hello = JSON.parse(payload.toString('utf8')); } catch { return reject('hello is not json'); }
        const checked = verify({
          relayHost: ADVERTISED, nodeId: hello.nodeId, owner: hello.owner,
          nonce: node.challenge.nonce, signature: hello.signature,
        });
        if (!checked.ok) return reject(checked.error);
        // One tunnel per node id. A second is far more likely a stale process
        // than a second machine, and letting it through would hand a live
        // node's traffic to whichever socket registered last.
        if (nodes.has(hello.nodeId)) return reject('that node id already has a tunnel');

        node.nodeId = String(hello.nodeId);
        node.owner = checked.owner;
        const opened = await openPublicPort(node);
        if (opened.error) return reject(opened.error);
        nodes.set(node.nodeId, node);
        node.send(TYPE.WELCOME, 0, Buffer.from(JSON.stringify({
          ok: true, publicHost: ADVERTISED, publicPort: opened.port,
          advertise: `tcp://${ADVERTISED}:${opened.port}`,
        }), 'utf8'));
        log(`node ${node.nodeId} (${node.owner.slice(0, 8)}…) is reachable at ${ADVERTISED}:${opened.port}`);
        return;
      }
      case TYPE.DATA:
        if (!node.nodeId) return reject('data before hello');
        return node.toDialer(stream, payload);
      case TYPE.CLOSE:
        return node.closeStream(stream, payload.toString('utf8') || 'node closed');
      case TYPE.PING:
        return node.send(TYPE.PONG, 0, payload);
      case TYPE.PONG:
        return;
      default:
        return reject(`unexpected ${typeName(type)} from a node`);
    }
  };

  socket.on('data', createParser(
    (type, stream, payload) => { onFrame(type, stream, payload).catch?.((e) => reject(e.message)); },
    (error) => reject(error.message),
  ));
  socket.on('error', (error) => node.teardown(`socket error: ${error.message}`));
  socket.on('close', () => node.teardown('socket closed'));
});

// A tunnel that has gone quiet is usually a machine that slept or a network
// that moved. Dropping it frees the port and lets the node register again
// cleanly, which is better than holding an address nothing answers on.
setInterval(() => {
  const now = Date.now();
  for (const node of nodes.values()) {
    if (now - node.lastSeen > IDLE_MS) node.teardown('idle');
    else node.send(TYPE.PING, 0);
  }
}, Math.max(5_000, Math.floor(IDLE_MS / 3))).unref();

control.listen(CONTROL_PORT, BIND, () => {
  log(`relay control on ${BIND}:${CONTROL_PORT}, advertising ${ADVERTISED}, ` +
    `public ports ${PORT_FROM}-${PORT_TO}` +
    (CALLERS.length ? `, dials allowed from ${CALLERS.join(', ')}` : ', dials allowed from ANYWHERE'));
});
