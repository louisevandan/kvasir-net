import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { isDeepStrictEqual } from 'node:util';

export const contract = Object.freeze({
  schema: 'p4.release-a.manifest.v1', context_tokens: 102400, output_tokens: 2048,
  resident: 8, pending_count: 64, pending_bytes: 128 * 1024 ** 2,
  pending_tokens: 6553600, request_bytes: 2 * 1024 ** 2,
  ttft_ms: { short: 60000, medium: 300000, long: 900000 }, itl_ms: 250,
  deadline_ms: { short: 600000, medium: 1200000, long: 1800000 },
  arrival_ms: [0, 180000, 480000, 780000, 1080000, 1380000, 1680000, 1980000],
  load_ms: 3600000, quiescence_ms: 30000, cancel_issue_stop_ms: 1000,
  send_slip_ms: 1000, sample_interval_ms: 1000, sample_coverage: .95, clock_skew_ms: 10,
});
const fail = (condition, message) => { if (!condition) throw new Error(message); };
const integer = (n, name, min = 0) => fail(Number.isSafeInteger(n) && n >= min, `${name}: invalid integer`);
const digest = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const sha = x => typeof x === 'string' && /^[a-f0-9]{64}$/.test(x) && !/^([a-f0-9])\1+$/.test(x);

/** Validate a fully materialized specification; missing measured bounds never become defaults. */
export function validateManifest(m) {
  fail(m.schema === contract.schema && ['cold', 'sustained'].includes(m.mode), 'unsupported manifest');
  fail(typeof m.run_id === 'string' && /^[a-zA-Z0-9][a-zA-Z0-9._-]+$/.test(m.run_id), 'invalid run identity');
  integer(m.epoch, 'epoch', 1);
  fail(isDeepStrictEqual(m.contract, contract), 'Release A contract differs');
  fail(Array.isArray(m.artifacts) && m.artifacts.length > 0, 'missing artifacts');
  const artifacts = new Map();
  for (const a of m.artifacts) {
    fail(typeof a.id === 'string' && a.id && !artifacts.has(a.id), 'duplicate or empty artifact id');
    fail(typeof a.path === 'string' && a.path && sha(a.sha256), 'invalid artifact path/hash');
    integer(a.bytes, 'artifact bytes', 1); artifacts.set(a.id, a);
  }
  const artifact = id => { fail(artifacts.has(id), `unbound artifact ${id}`); return artifacts.get(id); };
  fail(Array.isArray(m.sources) && m.sources.length > 0 && m.sources.every(x => /^[a-f0-9]{40}$/.test(x)), 'missing sources');
  fail(m.model?.shards?.length === 10 && new Set(m.model.shards).size === 10, 'model needs ten unique shards');
  m.model.shards.forEach(artifact);
  artifact(m.model.metadata); artifact(m.model.template); artifact(m.tokenizer?.binary);
  fail(m.tokenizer.add_special === true && m.tokenizer.parse_special === true, 'wrong tokenizer mode');
  fail(m.trust_boundary === 'approved-private-lan', 'unsupported trust boundary');
  fail(m.profile?.spec === 'none' && m.profile.prefill_fragments === 1 &&
    m.profile.service_controller === false && m.profile.prefix_reuse === false, 'unsupported experimental profile');
  artifact(m.profile.artifact);
  fail(m.hosts?.length === 7 && new Set(m.hosts.map(h => h.identity)).size === 7, 'seven unique physical hosts required');
  const hosts = new Map(), pools = new Map();
  for (const h of m.hosts) {
    fail(sha(h.identity), 'host identity must be a measured digest, not an IP');
    artifact(h.inspect); integer(h.inspected_unix_ms, 'inspect time', 1);
    hosts.set(h.identity, h);
    for (const p of h.pools ?? []) {
      const key = h.identity + ':' + p.id;
      fail(p.id && !pools.has(key), 'duplicate physical memory pool');
      integer(p.available_bytes, 'available bytes', 1); integer(p.reserve_bytes, 'host reserve');
      fail(p.reserve_bytes <= p.available_bytes, 'reserve exceeds available memory');
      pools.set(key, { available: p.available_bytes - p.reserve_bytes, reserved: 0 });
    }
  }
  fail(m.stages?.length === 8, 'eight stages required');
  let end = 0;
  for (const s of m.stages) {
    fail(hosts.has(s.host) && s.layer_begin === end, 'unbound host or discontinuous cut');
    integer(s.layer_end, 'layer end', end + 1); end = s.layer_end;
    artifact(s.plan); artifact(s.native); artifact(s.agent); artifact(s.allocation_conformance);
    fail(sha(s.state_abi) && sha(s.wire_abi), 'missing state/wire ABI binding');
    fail(s.n_batch === 128 && s.n_ubatch === 64 && s.kv_unified === true &&
      s.k_type === 'f16' && s.v_type === 'f16' && s.flash_attention === true, 'unsupported execution shape');
    fail(s.allocations?.length > 0, 'missing PLAN pool ownership');
    for (const a of s.allocations) {
      const pool = pools.get(s.host + ':' + a.pool);
      fail(pool, 'allocation references unknown physical pool');
      for (const key of ['weight_bytes', 'state_bytes', 'scratch_bytes']) integer(a[key], key);
      const bytes = a.weight_bytes + a.state_bytes + a.scratch_bytes;
      integer(bytes, 'allocation sum', 1); pool.reserved += bytes;
      integer(pool.reserved, 'shared pool sum'); fail(pool.reserved <= pool.available, 'shared pool overcommitted');
    }
    for (const key of ['physical_result_bytes', 'retained_output_count', 'retained_output_bytes',
      'receipt_count', 'receipt_each_bytes', 'receipt_bytes', 'edge_count', 'edge_each_bytes', 'edge_bytes']) {
      integer(s.bounds?.[key], key, 1);
    }
    const b = s.bounds;
    for (const [count, each, bytes] of [[b.retained_output_count, b.physical_result_bytes, b.retained_output_bytes],
      [b.receipt_count, b.receipt_each_bytes, b.receipt_bytes], [b.edge_count, b.edge_each_bytes, b.edge_bytes]]) {
      integer(count * each, 'response ownership product', 1);
      fail(count * each <= bytes, 'response ownership under-reserved');
    }
    // Separate storage owners are additive even when payload contents are equal.
    for (const [name, bytes] of [[s.output_pool, b.retained_output_bytes],
      [s.receipt_pool, b.receipt_bytes], [s.edge_pool, b.edge_bytes]]) {
      const pool = pools.get(s.host + ':' + name);
      fail(pool, 'response ownership references unknown pool');
      pool.reserved += bytes; integer(pool.reserved, 'pool plus response sum');
      fail(pool.reserved <= pool.available, 'shared pool overcommitted');
    }
  }
  fail(end === 108, 'model layer count mismatch');
  const waves = m.mode === 'cold' ? [0] : contract.arrival_ms;
  fail(m.requests?.length === waves.length * 8, 'request/wave count mismatch');
  fail(new Set(m.requests.map(r => r.id)).size === m.requests.length, 'duplicate request identity');
  for (let w = 0; w < waves.length; w++) {
    const counts = { short: 0, medium: 0, long: 0 };
    for (const r of m.requests.slice(w * 8, w * 8 + 8)) {
      fail(r.id && Object.hasOwn(counts, r.class) && r.after_ms === waves[w], 'wrong class or arrival');
      counts[r.class]++;
      artifact(r.prompt); artifact(r.token_ids); artifact(r.oracle); artifact(r.source_facts);
      integer(r.input_tokens, 'input tokens', 1); integer(r.serialized_bytes, 'serialized request bytes', 1);
      fail(r.serialized_bytes <= contract.request_bytes && r.input_tokens + contract.output_tokens <= contract.context_tokens,
        'request capacity exceeded');
      fail(r.class === 'short' ? r.input_tokens >= 2000 && r.input_tokens <= 8000 :
        r.class === 'medium' ? r.input_tokens === 32000 : r.input_tokens === 100038, 'wrong class token count');
      fail(artifact(r.token_ids).bytes === r.input_tokens * 4, 'token ID byte count mismatch');
    }
    fail(counts.short === 4 && counts.medium === 2 && counts.long === 2, 'wrong wave composition');
  }
  artifact(m.semantic_review); artifact(m.legacy_regression_manifest);
  return { valid: true, requests: m.requests.length, timeout_ms: waves.at(-1) + contract.deadline_ms.long,
    runtime_acceptance: false };
}

/** Bind every exact file before emitting a validated result; this is not a LOAD authorization. */
export function verifyFiles(m, root) {
  const result = validateManifest(m);
  for (const a of m.artifacts) {
    const bytes = fs.readFileSync(path.resolve(root, a.path));
    fail(bytes.length === a.bytes && digest(bytes) === a.sha256, `artifact changed: ${a.id}`);
  }
  return result;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [input] = process.argv.slice(2);
  fail(input, 'usage: node manifest.mjs materialized-manifest.json');
  console.log(JSON.stringify(verifyFiles(JSON.parse(fs.readFileSync(input)), path.dirname(path.resolve(input)))));
}
