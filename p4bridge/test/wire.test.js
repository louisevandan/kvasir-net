'use strict';
/**
 * The wire is the part a port gets silently wrong: a field in the wrong order
 * still encodes, and the agent answers by closing the socket. These tests pin
 * the byte layout against the engine's own encoder and the reference client.
 */
const test = require('node:test');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const {
  encodeEvent, decodeEvent, decodeHop,
  agentEndpoint, nodeEndpoint, outerEndpoint,
} = require('../wire');

const outer = outerEndpoint('tcp://127.0.0.1:42011', 'deadbeefdeadbeef', 1);

test('an event round-trips through encode and decode', () => {
  const payload = Buffer.from('{"hello":"world"}', 'utf8');
  const frame = encodeEvent({
    eventId: 'channel:1',
    correlationId: 'req-1',
    source: outer,
    target: nodeEndpoint('tcp://127.0.0.1:42011', 'step37-s0', 3),
    returnRoute: outer,
    eventClass: 'data',
    sequence: 7,
    deadlineUnixMs: 1_789_000_000_000,
    adapterKind: 'llamacpp',
    contentType: 'application/vnd.p4.llamacpp.prefill-v3+json',
    payload,
  });
  const { meta, payload: body } = decodeEvent(frame);
  assert.equal(meta.eventId, 'channel:1');
  assert.equal(meta.correlationId, 'req-1');
  assert.equal(meta.eventClass, 'data');
  assert.equal(meta.sequence, 7);
  assert.equal(meta.deadlineUnixMs, 1_789_000_000_000);
  assert.equal(meta.adapterKind, 'llamacpp');
  assert.equal(meta.contentType, 'application/vnd.p4.llamacpp.prefill-v3+json');
  assert.deepEqual(meta.target, { kind: 1, agent: 'tcp://127.0.0.1:42011', node: 'step37-s0', generation: 3 });
  assert.deepEqual(meta.returnRoute, { kind: 2, ingressAgent: 'tcp://127.0.0.1:42011', channel: 'deadbeefdeadbeef', connectionGeneration: 1 });
  assert.equal(body.toString('utf8'), '{"hello":"world"}');
});

test('the frame header is P4E3 with little-endian envelope and payload lengths', () => {
  const frame = encodeEvent({
    eventId: 'a', correlationId: 'a', source: outer, target: agentEndpoint('tcp://127.0.0.1:42011'),
    returnRoute: outer, eventClass: 'control', sequence: 1, deadlineUnixMs: null,
    adapterKind: null, contentType: 'application/vnd.p4.agent.inspect-v1+json', payload: Buffer.from('{}'),
  });
  assert.equal(frame.subarray(0, 4).toString('ascii'), 'P4E3');
  const envelopeLen = frame.readUInt32LE(4);
  const payloadLen = frame.readUInt32LE(8);
  assert.equal(payloadLen, 2);
  assert.equal(frame.length, 12 + envelopeLen + payloadLen);
  assert.equal(frame.readUInt16LE(12), 3, 'protocol version 3 leads the envelope');
});

test('the envelope matches the reference client byte for byte', () => {
  // Built the way p4hfadapter's Python client builds it, for one fixed input.
  const text = (value) => {
    const body = Buffer.from(value, 'utf8');
    const head = Buffer.alloc(4);
    head.writeUInt32LE(body.length, 0);
    return Buffer.concat([head, body]);
  };
  const u16 = (v) => { const b = Buffer.alloc(2); b.writeUInt16LE(v, 0); return b; };
  const u64 = (v) => { const b = Buffer.alloc(8); b.writeBigUInt64LE(BigInt(v), 0); return b; };
  const outerBytes = Buffer.concat([
    Buffer.from([2]), text(outer.ingressAgent), text(outer.channel), u64(outer.connectionGeneration),
  ]);
  const target = agentEndpoint('tcp://127.0.0.1:42011');
  const expectedEnvelope = Buffer.concat([
    u16(3), text('deadbeefdeadbeef:1'), text('deadbeefdeadbeef:1'), Buffer.from([0]),
    outerBytes, Buffer.concat([Buffer.from([0]), text(target.address)]),
    Buffer.from([1]), outerBytes.subarray(1),
    Buffer.from([0]),                              // class control
    u64(1),
    Buffer.from([1]), u64(1_789_000_000_000),
    Buffer.from([0]),                              // no adapter kind
    text('application/vnd.p4.agent.inspect-v1+json'),
  ]);
  const frame = encodeEvent({
    eventId: 'deadbeefdeadbeef:1', correlationId: 'deadbeefdeadbeef:1',
    source: outer, target, returnRoute: outer, eventClass: 'control', sequence: 1,
    deadlineUnixMs: 1_789_000_000_000, adapterKind: null,
    contentType: 'application/vnd.p4.agent.inspect-v1+json', payload: Buffer.from('{}'),
  });
  assert.deepEqual(frame.subarray(12, 12 + expectedEnvelope.length), expectedEnvelope);
});

test('a hop hello acknowledgement decodes', () => {
  const text = (value) => {
    const body = Buffer.from(value, 'utf8');
    const head = Buffer.alloc(4);
    head.writeUInt32LE(body.length, 0);
    return Buffer.concat([head, body]);
  };
  const u16 = (v) => { const b = Buffer.alloc(2); b.writeUInt16LE(v, 0); return b; };
  const u32 = (v) => { const b = Buffer.alloc(4); b.writeUInt32LE(v, 0); return b; };
  const u64 = (v) => { const b = Buffer.alloc(8); b.writeBigUInt64LE(BigInt(v), 0); return b; };
  const frame = Buffer.concat([
    Buffer.from('P4H1'), u16(1), u16(0), Buffer.from([2]),
    u64(1), text('agent#7'), u64(9), u32(128), u64(64 * 1024 * 1024),
  ]);
  const hop = decodeHop(frame);
  assert.equal(hop.kind, 'hello_ack');
  assert.equal(hop.acceptedGeneration, 1);
  assert.equal(hop.senderId, 'agent#7');
  assert.equal(hop.generation, 9);
  assert.equal(hop.maxOutstanding, 128);
});

test('a hop data frame keeps its digest honest', () => {
  const event = crypto.randomBytes(64);
  const digest = crypto.createHash('sha256').update(event).digest();
  const u16 = (v) => { const b = Buffer.alloc(2); b.writeUInt16LE(v, 0); return b; };
  const u32 = (v) => { const b = Buffer.alloc(4); b.writeUInt32LE(v, 0); return b; };
  const u64 = (v) => { const b = Buffer.alloc(8); b.writeBigUInt64LE(BigInt(v), 0); return b; };
  const good = Buffer.concat([Buffer.from('P4H1'), u16(1), u16(0), Buffer.from([3]), u64(4), digest, u32(event.length), event]);
  assert.equal(decodeHop(good).attempt, 4);
  const tampered = Buffer.from(good);
  tampered[tampered.length - 1] ^= 0xff;
  assert.throws(() => decodeHop(tampered), /digest mismatch/);
});

test('a truncated envelope is refused, not guessed', () => {
  const frame = encodeEvent({
    eventId: 'a', correlationId: 'a', source: outer, target: agentEndpoint('tcp://127.0.0.1:42011'),
    returnRoute: outer, eventClass: 'control', sequence: 1, deadlineUnixMs: null,
    adapterKind: null, contentType: 'application/vnd.p4.agent.inspect-v1+json', payload: Buffer.alloc(0),
  });
  assert.throws(() => decodeEvent(frame.subarray(0, frame.length - 3)), /length mismatch/);
});
