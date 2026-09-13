import test from 'node:test';
import assert from 'node:assert/strict';
import { analyzeNativeCost } from './analyze-native-cost.mjs';

const trace = `P4_COST_CALL_BEGIN pid=1 call=1 input_bytes=40
P4_COST_CONTEXT pid=1 call=1 ctx=abc rows=8
P4_COST_GRAPH_BEGIN pid=1 ctx=abc graph=1
P4_COST_NKV pid=1 ctx=abc graph=1 n_kv=256 rows=8 sequences=2 mask_allocated=1 mask_nkv=256
P4_COST_GRAPH_TIMES pid=1 ctx=abc graph=1 status=0 setup_us=3 input_us=4 execute_us=20 reused=0 nodes=10
P4_COST_NATIVE pid=1 call=1 ctx=abc ok=1 total_us=42 setup_us=2 decode_us=27 capture_us=10 post_us=2 capture_bytes=400
P4_COST_BIND pid=1 call=1 execution=1 load=1 session=6162 rows=8
P4_COST_CALL_END pid=1 call=1 ok=1 total_us=50 parse_us=1 match_us=1 sample_us=2 encode_us=2`;

test('cost consumer keeps overlapping levels separate and preserves real KV extent', () => {
  const a = analyzeNativeCost(trace);
  assert.equal(a.diagnostic_conformance, true);
  assert.deepEqual(a.totals, { call_us: 50, native_us: 42, graph_execute_us: 20 });
  assert.equal(a.calls[0].natives[0].graphs[0].kv[0].n_kv, 256);
  assert.equal(a.limits.pure_kernel_us, null);
  assert.equal(a.limits.release_acceptance, false);
});

test('missing completion, KV, or binding stays incomplete with raw observations retained', () => {
  for (const marker of ['GRAPH_TIMES', 'NKV', 'BIND', 'CALL_END', 'NATIVE']) {
    const a = analyzeNativeCost(trace.split('\n').filter(l => !l.startsWith(`P4_COST_${marker} `)).join('\n'));
    assert.equal(a.diagnostic_conformance, false, marker);
    assert.equal(a.incomplete_calls, 1);
    assert.equal(a.calls.length, 1);
    assert.equal(a.totals.call_us, 0);
  }
});

test('duplicate authority, context mismatch, malformed records and overlapping costs reject', () => {
  const bind = trace.split('\n').find(l => l.startsWith('P4_COST_BIND'));
  assert.throws(() => analyzeNativeCost(trace.replace(bind, `${bind}\n${bind}`)), /duplicate execution/);
  assert.throws(() => analyzeNativeCost(trace.replace('ctx=abc graph=1 status', 'ctx=def graph=1 status')), /unbound graph/);
  assert.throws(() => analyzeNativeCost(trace.replace('decode_us=27', 'decode_us=38')), /overlap/);
  assert.throws(() => analyzeNativeCost(trace + '\nP4_COST_BROKEN'), /malformed/);
});

test('warmup graphs are preserved but cannot count as a bound execution', () => {
  const a = analyzeNativeCost(trace.split('\n').filter(l => /COST_(GRAPH|NKV)/.test(l)).join('\n'));
  assert.equal(a.orphan_graphs.length, 1);
  assert.equal(a.complete_calls, 0);
  assert.equal(a.diagnostic_conformance, false);
});
