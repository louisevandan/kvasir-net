'use strict';
/**
 * The one-shot tools must hand their connection back.
 *
 * An agent releases a connection's slot only on FINISH; a socket that dies
 * leaves the return route and the permit in place (transport.rs:1076-1078).
 * Both tools send events, so before this each run cost the agent a slot for
 * good -- which is where "one INSPECT costs one slot" came from. It was never
 * the INSPECT.
 *
 * This stands up a socket that speaks just enough of the wire to answer an
 * INSPECT, and watches what the tool does with the connection afterwards.
 */
const test = require('node:test');
const assert = require('node:assert/strict');
const net = require('node:net');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { execFile } = require('node:child_process');
const { encodeEvent, decodeEvent, agentEndpoint, outerEndpoint } = require('../wire.js');

const frame = (buf) => {
  const len = Buffer.alloc(4);
  len.writeUInt32LE(buf.length, 0);
  return Buffer.concat([len, buf]);
};

test('inspect.mjs sends FINISH instead of dropping the socket', async (t) => {
  const seen = { finish: false, destroyed: false, frames: 0 };
  let address;

  const agent = net.createServer((socket) => {
    let buffer = Buffer.alloc(0);
    socket.on('close', () => { if (!seen.finish) seen.destroyed = true; });
    socket.on('data', (chunk) => {
      buffer = Buffer.concat([buffer, chunk]);
      for (;;) {
        if (buffer.length < 4) return;
        const size = buffer.readUInt32LE(0);
        if (size === 0) {                       // FINISH
          buffer = buffer.subarray(4);
          seen.finish = true;
          socket.write(Buffer.alloc(4));        // acknowledge it
          continue;
        }
        if (buffer.length < 4 + size) return;
        const body = buffer.subarray(4, 4 + size);
        buffer = buffer.subarray(4 + size);
        let event;
        try {
          event = decodeEvent(body);
        } catch {
          // Not a bare P4E3 event: it is the hop hello. Older agents answer
          // that by closing, and connect() falls back to direct -- which is the
          // mode the bridge runs in and the only one where FINISH is known to
          // release the permit. Forcing the fallback keeps this test on that
          // path; see the note below about what it therefore does NOT cover.
          socket.destroy();
          return;
        }
        seen.frames += 1;
        socket.write(frame(encodeEvent({
          eventId: 'agent:1',
          correlationId: event.meta.correlationId,
          source: agentEndpoint(address),
          target: outerEndpoint(address, 'reply', 1),
          eventClass: 'control',
          sequence: 1,
          contentType: 'application/vnd.p4.agent.snapshot-v1+json',
          payload: Buffer.from(JSON.stringify({ nodes: [] }), 'utf8'),
        })));
      }
    });
  });

  const port = await new Promise((r) => agent.listen(0, '127.0.0.1', () => r(agent.address().port)));
  address = `tcp://127.0.0.1:${port}`;
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'p4tool-'));
  const plan = path.join(dir, 'plan.json');
  fs.writeFileSync(plan, JSON.stringify({ stages: [{ agent: address, node: 'n0', generation: 1 }] }));
  t.after(() => { agent.close(); fs.rmSync(dir, { recursive: true, force: true }); });

  await new Promise((resolve, reject) => {
    execFile(process.execPath, [path.join(__dirname, '..', 'inspect.mjs'), plan],
      { timeout: 20_000 }, (error, stdout, stderr) => (error ? reject(new Error(`${error.message}\n${stderr}`)) : resolve(stdout)));
  });

  assert.equal(seen.frames, 1, 'the tool should have sent exactly one INSPECT');
  assert.equal(seen.finish, true, 'the tool dropped the socket instead of sending FINISH -- that is a slot gone for good');

  // ⚠ What this does NOT cover: the tools do not pass `hop`, so against the
  // current agent they connect in HOP mode, not the direct mode this test
  // forces. FINISH in direct mode is proved -- the bridge's shutdown released
  // both agents' permits in production on 2026-09-25. Hop-mode FINISH handling
  // in the agent's serve_hop is unread. Until it is, this test shows the tool
  // asks correctly; it does not show the agent answers.
});
