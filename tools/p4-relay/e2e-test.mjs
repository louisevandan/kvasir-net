#!/usr/bin/env node
/**
 * The relay carries bytes, unaltered, for several callers at once.
 *
 * This is the only property that matters. p4's own framing rides inside these
 * payloads, so a single byte reordered or dropped is a corrupted event, and the
 * failure would surface far away as a protocol error nobody traces back here.
 * So the test compares hashes of a megabyte of random data, not a greeting.
 */
import net from 'node:net';
import crypto from 'node:crypto';
import { spawn } from 'node:child_process';
import nacl from 'tweetnacl';
import { connectTunnel } from './tunnel.mjs';

const ALPHA = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';
const b58 = (bytes) => {
  const digits = [0];
  for (const byte of bytes) {
    let carry = byte;
    for (let i = 0; i < digits.length; i++) { carry += digits[i] << 8; digits[i] = carry % 58; carry = (carry / 58) | 0; }
    while (carry > 0) { digits.push(carry % 58); carry = (carry / 58) | 0; }
  }
  let prefix = '';
  for (const byte of bytes) { if (byte === 0) prefix += '1'; else break; }
  return prefix + digits.reverse().map((d) => ALPHA[d]).join('');
};

const sha = (buf) => crypto.createHash('sha256').update(buf).digest('hex').slice(0, 16);
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
let failures = 0;
const check = (name, ok, detail = '') => {
  console.log(`  ${ok ? 'ok  ' : 'FAIL'} ${name}${detail ? ` — ${detail}` : ''}`);
  if (!ok) failures += 1;
};

// A stand-in for the p4 agent: echoes every byte back, unchanged.
const agent = net.createServer((socket) => socket.pipe(socket));
await new Promise((r) => agent.listen(45001, '127.0.0.1', r));

const relay = spawn(process.execPath, [new URL('./relay.mjs', import.meta.url).pathname], {
  env: {
    ...process.env,
    KVASIR_RELAY_PORT: '43000', KVASIR_RELAY_BIND: '127.0.0.1',
    KVASIR_RELAY_HOST: '127.0.0.1',
    KVASIR_RELAY_PORT_FROM: '43100', KVASIR_RELAY_PORT_TO: '43101',
  },
  stdio: ['ignore', 'pipe', 'pipe'],
});
relay.stdout.on('data', (d) => process.stdout.write(`    relay| ${d}`));
relay.stderr.on('data', (d) => process.stdout.write(`    relay! ${d}`));
await wait(600);

const keys = nacl.sign.keyPair();
const owner = b58(keys.publicKey);
let advertise = null;
const tunnel = connectTunnel({
  relayHost: '127.0.0.1', relayPort: 43000,
  nodeId: 'desktop-test-1', owner,
  sign: async (text) => Buffer.from(nacl.sign.detached(Buffer.from(text, 'utf8'), keys.secretKey)).toString('base64'),
  agentPort: 45001,
  onEvent: (e) => { if (e.kind === 'registered') advertise = e.advertise; },
});

for (let i = 0; i < 40 && !advertise; i++) await wait(100);
check('the node registers and is given a public address', Boolean(advertise), advertise ?? 'never registered');
const port = advertise ? Number(new URL(advertise.replace('tcp://', 'http://')).port) : 0;

/** Dial the public port, send `payload`, resolve with everything echoed back. */
function roundTrip(payload) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    const dialer = net.connect({ host: '127.0.0.1', port }, () => dialer.write(payload));
    dialer.on('data', (chunk) => {
      chunks.push(chunk);
      const total = chunks.reduce((n, c) => n + c.length, 0);
      if (total >= payload.length) { dialer.end(); resolve(Buffer.concat(chunks)); }
    });
    dialer.on('error', reject);
    setTimeout(() => reject(new Error('timed out')), 20_000);
  });
}

if (port) {
  const small = Buffer.from('P4E3 pretend frame');
  check('a short message survives', sha(await roundTrip(small)) === sha(small));

  const big = crypto.randomBytes(1024 * 1024);
  const back = await roundTrip(big);
  check('a megabyte of random bytes survives byte for byte',
    back.length === big.length && sha(back) === sha(big),
    `sent ${big.length} sha ${sha(big)}, got ${back.length} sha ${sha(back)}`);

  const payloads = [crypto.randomBytes(300_000), crypto.randomBytes(300_000), crypto.randomBytes(300_000)];
  const results = await Promise.all(payloads.map(roundTrip));
  check('three concurrent callers do not cross streams',
    results.every((got, i) => sha(got) === sha(payloads[i])),
    results.map((got, i) => `${sha(payloads[i])}->${sha(got)}`).join(' '));
}

// An unsigned registration must not get an address.
const refused = await new Promise((resolve) => {
  const socket = net.connect({ host: '127.0.0.1', port: 43000 });
  let answer = 'no reply';
  socket.on('data', (chunk) => {
    const text = chunk.toString('utf8');
    if (text.includes('nonce') && !text.includes('error')) {
      socket.write(Buffer.concat([
        Buffer.from('KVR1'), Buffer.from([1]),
        (() => { const b = Buffer.alloc(8); b.writeUInt32BE(0, 0); b.writeUInt32BE(Buffer.from('{"nodeId":"x","owner":"x","signature":"x"}').length, 4); return b; })(),
        Buffer.from('{"nodeId":"x","owner":"x","signature":"x"}'),
      ]));
    } else if (text.includes('error')) { answer = text.slice(text.indexOf('{')); socket.destroy(); resolve(answer); }
  });
  socket.on('close', () => resolve(answer));
  setTimeout(() => { socket.destroy(); resolve(answer); }, 4000);
});
check('an unsigned registration is refused', String(refused).includes('error'), String(refused).slice(0, 90));

tunnel.stop();
relay.kill();
agent.close();
await wait(200);
console.log(failures ? `\n  ${failures} failure(s)` : '\n  all checks passed');
process.exit(failures ? 1 : 0);
