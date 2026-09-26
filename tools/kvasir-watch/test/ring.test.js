import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, mkdtempSync, existsSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { ringProbes, parseGaugeLine, rateGauge, readGauge } from '../ring.mjs';
import { compactRing, diffRing, main as alertMain } from '../ring-alert.mjs';

const FX = path.join(path.dirname(new URL(import.meta.url).pathname), 'fixtures');
const load = (name) => JSON.parse(readFileSync(path.join(FX, name), 'utf8'));
const NOW = Date.parse('2026-09-26T00:12:00Z');   // 2 min after the fixture gauge line

const config = {
  bridge: {
    url: 'http://127.0.0.1:19000',
    tokenEnv: 'KVASIR_BRIDGE_TOKEN',
    catalog: path.join(FX, 'catalog.json'),
    agents: { 'tcp://192.168.100.1:42011': 'GB10-1', 'tcp://192.168.100.2:42011': 'GB10-2' },
    gauges: { 'GB10-1': path.join(FX, 'gauge.log') },
  },
};

/** A fetch that answers from fixtures and records what was asked. */
function fakeFetch({ health = load('health-healthy.json'), healthStatus = 200, runtime = load('runtime-healthy.json'), runtimeStatus = 200, requireToken = 't0k' } = {}) {
  const calls = [];
  const fn = async (url, init = {}) => {
    calls.push({ url, headers: init.headers ?? {} });
    const reply = (status, body) => ({ status, text: async () => JSON.stringify(body) });
    if (url.endsWith('/health')) return reply(healthStatus, health);
    if (url.endsWith('/api/runtime')) {
      if (requireToken && init.headers?.['x-kvasir-service-token'] !== requireToken) {
        return reply(401, { error: { message: 'service token required', type: 'unauthorized' } });
      }
      return reply(runtimeStatus, runtime);
    }
    return reply(404, {});
  };
  fn.calls = calls;
  return fn;
}
const env = { KVASIR_BRIDGE_TOKEN: 't0k' };
const by = (probes, name) => probes.find((p) => p.name === name);

test('healthy ring: bridge serving, both agents connected, stages loaded, gauge good', async () => {
  const fetch = fakeFetch();
  const probes = await ringProbes(config, { fetch, env, now: NOW });
  const bridge = by(probes, 'bridge');
  assert.equal(bridge.ok, true);
  assert.equal(bridge.value.serving, true);
  assert.equal(bridge.value.runtime, 'read');
  assert.match(bridge.value.catalog, /step37@1790317534470/);
  for (const label of ['GB10-1', 'GB10-2']) {
    assert.equal(by(probes, `host:${label}`).value.running, true, label);
    assert.equal(by(probes, `host:${label}`).value.uptime, null, 'uptime is not known through the bridge');
    const stages = by(probes, `stages:${label}`).value;
    assert.equal(stages.nodes.length, 1);
    assert.equal(stages.nodes[0].state, 'loaded');
    assert.equal(stages.nodes[0].generation, null, 'no node generation is invented');
    assert.equal(stages.nodes[0].catalogGeneration, 1790317534470);
    assert.equal(stages.vramBytes, 130596823040);
  }
  assert.equal(by(probes, 'host:GB10-1').value.recentErrors.join(), '1 preserved transport failure(s)');
  assert.equal(by(probes, 'gauge:GB10-1').value.level, 'good');
  // Only the bridge was asked, and only over HTTP.
  assert.deepEqual(fetch.calls.map((c) => c.url), ['http://127.0.0.1:19000/health', 'http://127.0.0.1:19000/api/runtime']);
});

test('control: /health 503 is a live bridge that is not serving, never "unreachable"', async () => {
  const fetch = fakeFetch({ healthStatus: 503, health: { status: 'unhealthy', serving_models: 0, inspect_error: null, dials_stopped: [], awaiting_probe: [] } });
  const probes = await ringProbes(config, { fetch, env, now: NOW });
  const bridge = by(probes, 'bridge');
  assert.equal(bridge.ok, true);
  assert.equal(bridge.value.serving, false);
  assert.equal(bridge.value.status, 503);
  assert.equal(compactRing({ tracks: { ring: probes } }).bridge, 'not serving (503)');
});

test('control: bridge down → bridge and every agent probe fail with a reason; gauge still read', async () => {
  const fetch = async () => { throw new Error('connect ECONNREFUSED 127.0.0.1:19000'); };
  const probes = await ringProbes(config, { fetch, env, now: NOW });
  assert.equal(by(probes, 'bridge').ok, false);
  assert.match(by(probes, 'bridge').error, /ECONNREFUSED/);
  assert.equal(by(probes, 'host:GB10-1').ok, false);
  assert.equal(by(probes, 'stages:GB10-2').error, 'bridge unreachable');
  assert.equal(by(probes, 'gauge:GB10-1').ok, true);
  assert.equal(compactRing({ tracks: { ring: probes } }).bridge, 'unreachable');
});

test('no token: /health still read, agent probes explain the 401 instead of guessing', async () => {
  const probes = await ringProbes(config, { fetch: fakeFetch(), env: {}, now: NOW });
  assert.equal(by(probes, 'bridge').ok, true);
  assert.match(by(probes, 'bridge').value.runtime, /service token required.*is not set/);
  assert.equal(by(probes, 'host:GB10-1').ok, false);
  assert.match(by(probes, 'host:GB10-1').error, /KVASIR_BRIDGE_TOKEN is not set/);
});

test('an agent the bridge stopped dialling reads as not running, with the reason', async () => {
  const runtime = load('runtime-healthy.json');
  runtime.dial_ledger[1] = { ...runtime.dial_ledger[1], connected: false, stopped: 'lifetime' };
  runtime.machines[1].nodes = [];
  const probes = await ringProbes(config, { fetch: fakeFetch({ runtime }), env, now: NOW });
  const host = by(probes, 'host:GB10-2').value;
  assert.equal(host.running, false);
  assert.match(host.recentErrors[0], /dials stopped: lifetime/);
  assert.deepEqual(by(probes, 'stages:GB10-2').value.nodes, []);
  const facts = compactRing({ tracks: { ring: probes } });
  assert.equal(facts['agent GB10-2'], 'stopped (lifetime)');
  assert.equal(facts['stages GB10-2'], 'none');
});

test('an agent the config does not name still appears, labelled by address', async () => {
  const runtime = load('runtime-healthy.json');
  runtime.dial_ledger.push({ agent: 'tcp://192.168.100.3:42011', connected: true, stopped: null });
  const probes = await ringProbes(config, { fetch: fakeFetch({ runtime }), env, now: NOW });
  assert.ok(by(probes, 'host:192.168.100.3:42011'));
});

test('gauge: v5 line parsed, v4 line ignored, thresholds and staleness applied', () => {
  const lines = readFileSync(path.join(FX, 'gauge.log'), 'utf8').trim().split('\n');
  assert.equal(parseGaugeLine(lines[0]), null, 'a line without unique= is not a reading');
  const v5 = parseGaugeLine(lines[1]);
  assert.deepEqual([v5.at, v5.agent_sockfd, v5.unique, v5.hidden, v5.bridge], ['2026-09-26T00:09:40Z', 13, 11, 0, 'ok']);
  assert.equal(rateGauge(v5, NOW).level, 'good');
  assert.equal(rateGauge(v5, NOW + 20 * 60_000).level, 'stale');
  assert.equal(rateGauge({ ...v5, hidden: 1 }, NOW).level, 'warn');
  assert.equal(rateGauge({ ...v5, agent_sockfd: 128 }, NOW).level, 'warn');
  assert.equal(rateGauge({ ...v5, agent_sockfd: 200 }, NOW).level, 'critical');
  assert.equal(parseGaugeLine('2026-09-26T00:14:40Z agent_sockfd=na unique=na hidden=na sockfd_level=na').agent_sockfd, null);
  assert.equal(rateGauge(parseGaugeLine('2026-09-26T00:14:40Z agent_sockfd=na unique=na hidden=na'), NOW).level, 'bad');
  // Gauge v6 bridge words: a 503 is a live bridge, and never changes the slot rating.
  const v6 = (word) => parseGaugeLine(lines[1].replace('bridge=ok', `bridge=${word}`));
  assert.equal(v6('not_serving').bridgeDead, false);
  assert.equal(v6('http_502').bridgeDead, false);
  assert.equal(v6('unreachable(URLError)').bridgeDead, true);
  assert.equal(rateGauge(v6('not_serving'), NOW).level, 'good', 'slot rating is about slots, not the bridge');
  // The whole file: picks the last v5 line, not the last line if that were v4.
  assert.equal(readGauge(path.join(FX, 'gauge.log'), { now: NOW }).hidden, 0);
  assert.throws(() => readGauge('/nonexistent/gauge.log'), /not found/);
});

test('ring.mjs opens no socket to an agent', () => {
  const src = readFileSync(new URL('../ring.mjs', import.meta.url), 'utf8');
  for (const forbidden of ['kvasir-p4-bridge', 'createConnection', 'node:net', "'ssh'", '42011)']) {
    assert.ok(!src.includes(forbidden), `ring.mjs must not contain ${forbidden}`);
  }
});

test('alert: baseline is silent, no change is silent, a change speaks and exits 10', async () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'kvasir-ring-'));
  const stateFile = path.join(dir, 'ring.json');
  const healthy = { tracks: { ring: await ringProbes(config, { fetch: fakeFetch(), env, now: NOW }) } };
  const file = path.join(dir, 'c.json');
  writeFileSync(file, JSON.stringify(healthy));
  const out = [];
  const write = process.stdout.write.bind(process.stdout);
  process.stdout.write = (s) => { out.push(String(s)); return true; };
  try {
    assert.equal(alertMain(['node', 'ring-alert.mjs', file], { stateFile }), 0);
    assert.ok(existsSync(stateFile));
    assert.equal(alertMain(['node', 'ring-alert.mjs', file], { stateFile }), 0, 'same facts: silent');
    assert.match(out.join(''), /baseline/);
    out.length = 0;
    const runtime = load('runtime-healthy.json');
    runtime.dial_ledger[0].connected = false;
    runtime.machines[0].nodes[0].state = 'failed';
    const broken = { tracks: { ring: await ringProbes(config, { fetch: fakeFetch({ runtime }), env, now: NOW }) } };
    writeFileSync(file, JSON.stringify(broken));
    assert.equal(alertMain(['node', 'ring-alert.mjs', file], { stateFile }), 10);
    const msg = out.join('');
    assert.match(msg, /agent GB10-1: connected → disconnected/);
    assert.match(msg, /stage step37-s0: loaded → failed/);
    assert.ok(!/GB10-2/.test(msg), 'the unchanged agent is not mentioned');
    out.length = 0;
    // Control: a collection with no bridge probe at all (old collector, or a
    // broken config) must read as a change, not as "fine".
    writeFileSync(file, JSON.stringify({ tracks: { ring: [] } }));
    assert.equal(alertMain(['node', 'ring-alert.mjs', file], { stateFile }), 10);
    assert.match(out.join(''), /bridge: .* → not probed/);
  } finally { process.stdout.write = write; }
});

test('diffRing names additions, removals and changes', () => {
  assert.deepEqual(diffRing({ a: 1, b: 2 }, { a: 1, b: 3, c: 4 }), ['b: 2 → 3', 'c: 4 (new)']);
  assert.deepEqual(diffRing({ a: 1 }, {}), ['a: 1 → gone']);
  assert.deepEqual(diffRing(null, { a: 1 }), ['a: 1 (new)']);
});
