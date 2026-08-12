import assert from 'node:assert/strict';
import net from 'node:net';
import test from 'node:test';
import { ControllerInstance } from './controller-instance.mjs';
import { closeAgentLinks } from './transport/agent-link.mjs';

test('P4B1 v5 multiplexes two controllers over one Agent socket', { timeout: 5_000 }, async () => {
  let connections = 0;
  const server = net.createServer(socket => {
    connections += 1;
    const decoder = new Decoder();
    socket.on('data', chunk => {
      for (const request of decoder.push(chunk)) {
        assert.equal(request.kind, 16);
        const fields = strings(request.payload);
        const requestId = fields[2];
        socket.write(frame(request.routeId, 17, Buffer.concat([
          text(requestId), text('node-a'), Buffer.from([1]), text('ready'),
        ])));
      }
    });
  });
  await listen(server);
  const address = server.address();
  const endpoint = `127.0.0.1:${address.port}`;
  const first = new ControllerInstance({ controllerId: 'controller-a', endpoint });
  const second = new ControllerInstance({ controllerId: 'controller-b', endpoint });
  try {
    const [one, two] = await Promise.all([
      first.health({ nodeId: 'node-a', requestId: 'same-business-id' }),
      second.health({ nodeId: 'node-a', requestId: 'same-business-id' }),
    ]);
    assert.equal(one.requestId, 'same-business-id');
    assert.equal(two.requestId, 'same-business-id');
    assert.equal(connections, 1);
  } finally {
    closeAgentLinks();
    server.closeAllConnections?.();
    await close(server);
  }
});

test('a route deadline sends CANCEL without closing the shared socket', { timeout: 5_000 }, async () => {
  let connections = 0;
  let timedRoute;
  let cancelObserved = false;
  const server = net.createServer(socket => {
    connections += 1;
    const decoder = new Decoder();
    socket.on('data', chunk => {
      for (const request of decoder.push(chunk)) {
        if (request.kind === 16) {
          const requestId = strings(request.payload)[2];
          if (requestId === 'times-out') timedRoute = request.routeId;
          else socket.write(frame(request.routeId, 17, Buffer.concat([
            text(requestId), text('node-a'), Buffer.from([1]), text('ready'),
          ])));
        } else if (request.kind === 5) {
          assert.equal(request.routeId, timedRoute);
          assert.equal(strings(request.payload)[0], 'times-out');
          cancelObserved = true;
        }
      }
    });
  });
  await listen(server);
  const endpoint = `127.0.0.1:${server.address().port}`;
  const controller = new ControllerInstance({ controllerId: 'controller-timeout', endpoint });
  try {
    await assert.rejects(
      controller.health({ nodeId: 'node-a', requestId: 'times-out', timeoutMs: 20 }),
      /exceeded 20ms deadline/,
    );
    const health = await controller.health({ nodeId: 'node-a', requestId: 'after-timeout' });
    assert.equal(health.ready, true);
    assert.equal(cancelObserved, true);
    assert.equal(connections, 1);
  } finally {
    closeAgentLinks();
    server.closeAllConnections?.();
    await close(server);
  }
});

test('model load carries canonical load options inside the bounded stage plan', { timeout: 5_000 }, async () => {
  let receivedPlan;
  const server = net.createServer(socket => {
    const decoder = new Decoder();
    socket.on('data', chunk => {
      for (const request of decoder.push(chunk)) {
        assert.equal(request.kind, 40);
        const fields = strings(request.payload);
        receivedPlan = JSON.parse(fields[7]);
        socket.write(frame(request.routeId, 41, Buffer.concat([
          text(fields[2]), text(fields[1]), text(fields[3]), text(fields[4]),
          u64(1), text('ready'), text('bound'),
        ])));
      }
    });
  });
  await listen(server);
  const controller = new ControllerInstance({
    controllerId: 'controller-load-options',
    endpoint: `127.0.0.1:${server.address().port}`,
  });
  const loadOptions = {
    flash_attention: 'enabled',
    mmap: false,
    kv_cache: { type_k: 'q8_0', type_v: 'q8_0', offload: true },
    batching: {
      strategy: 'ready-queue-dynamic',
      max_sequences: 16,
      context_batch_tokens: 50,
      context_ubatch_tokens: 50,
      calculation: {
        method: 'minimum',
        terms: [{ name: 'verified_stage:p4-gpu-4080', value: 16, source: 'measured' }],
        result: 16,
      },
    },
    adapter_options: {},
  };
  try {
    const events = [];
    for await (const event of controller.loadModel({
      nodeId: 'node-a', deploymentId: 'deployment-a', bindingId: 'binding-a',
      model: 'Ornith-1.0-35B.gguf', planRevision: 'measured-v1',
      stagePlan: { placement: 'four-stage' }, loadOptions,
    })) events.push(event);
    assert.equal(events.at(-1)?.type, 'model-bound');
    assert.equal(receivedPlan.placement, 'four-stage');
    assert.deepEqual(receivedPlan.load_options, loadOptions);
  } finally {
    closeAgentLinks();
    server.closeAllConnections?.();
    await close(server);
  }
});

class Decoder {
  pending = Buffer.alloc(0);
  push(chunk) {
    this.pending = Buffer.concat([this.pending, chunk]);
    const frames = [];
    while (this.pending.length >= 16) {
      assert.equal(this.pending.subarray(0, 4).toString(), 'P4B1');
      assert.equal(this.pending[4], 5);
      const length = this.pending.readUInt32LE(8);
      if (this.pending.length < 16 + length) break;
      const payload = this.pending.subarray(16, 16 + length);
      const routeLength = payload.readUInt32LE(0);
      frames.push({
        kind: this.pending[5],
        routeId: payload.subarray(4, 4 + routeLength).toString(),
        payload: payload.subarray(12 + routeLength),
      });
      this.pending = this.pending.subarray(16 + length);
    }
    return frames;
  }
}

function frame(routeId, kind, messagePayload) {
  const route = Buffer.from(routeId);
  const payload = Buffer.alloc(12 + route.length + messagePayload.length);
  payload.writeUInt32LE(route.length, 0);
  route.copy(payload, 4);
  payload.writeBigUInt64LE(0n, 4 + route.length);
  messagePayload.copy(payload, 12 + route.length);
  const header = Buffer.alloc(16);
  header.write('P4B1');
  header[4] = 5;
  header[5] = kind;
  header.writeUInt32LE(payload.length, 8);
  return Buffer.concat([header, payload]);
}

function strings(payload) {
  const values = [];
  let offset = 0;
  while (offset < payload.length) {
    const length = payload.readUInt32LE(offset);
    offset += 4;
    values.push(payload.subarray(offset, offset + length).toString());
    offset += length;
  }
  return values;
}

function text(value) {
  const bytes = Buffer.from(value);
  const length = Buffer.alloc(4);
  length.writeUInt32LE(bytes.length);
  return Buffer.concat([length, bytes]);
}

function u64(value) {
  const bytes = Buffer.alloc(8);
  bytes.writeBigUInt64LE(BigInt(value));
  return bytes;
}

function listen(server) {
  return new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
}

function close(server) {
  return new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
}
