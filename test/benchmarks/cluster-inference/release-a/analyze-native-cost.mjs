import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';

const check = (v, message) => { if (!v) throw new Error(message); };
const sum = (xs, k) => xs.reduce((n, x) => n + x[k], 0);
// Graph times overlap native decode. Report both levels, never add them together.
export function analyzeNativeCost(text) {
  const calls = new Map(), contexts = new Map(), graphs = new Map(), bindings = new Set();
  const orphanGraphs = [];
  for (const line of text.split(/\r?\n/)) {
    if (!line.includes('P4_COST_')) continue;
    const match = line.match(/^P4_COST_([A-Z_]+) ((?:\w+=\S+ ?)+)$/);
    check(match, 'malformed cost record');
    const fields = {};
    for (const item of match[2].trim().split(' ')) {
      const [k, v] = item.split('=');
      check(!(k in fields), 'duplicate field');
      fields[k] = ['ctx', 'session'].includes(k) ? v : Number(v);
      if (!['ctx', 'session'].includes(k)) check(Number.isSafeInteger(fields[k]), 'invalid integer');
    }
    const f = fields, key = `${f.pid}:${f.call}`, gkey = `${f.pid}:${f.graph}`;
    check(f.pid > 0, 'missing process identity');
    const c = calls.get(key);
    switch (match[1]) {
      case 'CALL_BEGIN':
        check(f.call > 0 && f.input_bytes >= 0 && !c, 'duplicate or invalid call');
        calls.set(key, { ...f, bindings: [], natives: [], end: null }); break;
      case 'CONTEXT': {
        check(c && !c.end && f.ctx && f.rows > 0, 'unbound native context');
        const ck = `${f.pid}:${f.ctx}`;
        check(!contexts.has(ck), 'overlapping context');
        const n = { ...f, graphs: [], end: null };
        c.natives.push(n); contexts.set(ck, n); break;
      }
      case 'GRAPH_BEGIN': {
        check(f.graph > 0 && f.ctx && !graphs.has(gkey), 'duplicate graph');
        const g = { ...f, kv: [], end: null };
        graphs.set(gkey, g);
        const n = contexts.get(`${f.pid}:${f.ctx}`);
        (n ? n.graphs : orphanGraphs).push(g); break;
      }
      case 'NKV': {
        const g = graphs.get(gkey);
        check(g && !g.end && g.ctx === f.ctx, 'unbound KV observation');
        check(f.n_kv >= 0 && f.rows > 0 && f.sequences > 0 && [0, 1].includes(f.mask_allocated), 'invalid KV extent');
        g.kv.push(f); break;
      }
      case 'GRAPH_TIMES': {
        const g = graphs.get(gkey);
        check(g && !g.end && g.ctx === f.ctx, 'unbound graph completion');
        check(['setup_us', 'input_us', 'execute_us', 'nodes'].every(k => f[k] >= 0), 'invalid graph cost');
        g.end = f; break;
      }
      case 'NATIVE': {
        const ck = `${f.pid}:${f.ctx}`;
        let n = contexts.get(ck);
        // A rejected call may exit before begin_decode. Retain it as unknown.
        if (!n && c && !c.end && f.ok === 0) {
          n = { pid: f.pid, call: f.call, ctx: f.ctx, rows: null, graphs: [], end: null };
          c.natives.push(n);
        }
        check(c && n && n.call === f.call && !n.end, 'unbound native completion');
        if (f.ok === 1) {
          check(['setup_us', 'decode_us', 'capture_us', 'post_us', 'capture_bytes', 'total_us'].every(k => f[k] >= 0), 'invalid native cost');
          const parts = f.setup_us + f.decode_us + f.capture_us + f.post_us;
          check(f.total_us >= parts && f.total_us - parts <= 3, 'native cost overlap or missing interval');
        }
        n.end = f; contexts.delete(ck); break;
      }
      case 'BIND': {
        check(c && !c.end && f.execution > 0 && f.load > 0 && /^[0-9a-f]+$/.test(f.session) && f.rows > 0, 'invalid execution binding');
        const bk = `${f.pid}:${f.load}:${f.session}:${f.execution}`;
        check(!bindings.has(bk), 'duplicate execution binding');
        bindings.add(bk); c.bindings.push(f); break;
      }
      case 'CALL_END':
        check(c && !c.end && ['total_us', 'parse_us', 'match_us', 'sample_us', 'encode_us'].every(k => f[k] >= 0), 'invalid call completion');
        c.end = f; break;
      default: throw new Error('unknown cost record');
    }
  }
  const complete = c => c.end?.ok === 1 && c.bindings.length > 0 && c.natives.length > 0 &&
    c.natives.every(n => n.end?.ok === 1 && n.graphs.length > 0 &&
      n.graphs.every(g => g.end?.status === 0 && g.kv.length > 0));
  const observed = [...calls.values()];
  const accepted = observed.filter(complete);
  return { schema: 'p4.release-a.native-cost.v1', calls: observed,
    complete_calls: accepted.length, incomplete_calls: observed.length - accepted.length,
    orphan_graphs: orphanGraphs, diagnostic_conformance: accepted.length > 0 && accepted.length === observed.length,
    totals: { call_us: sum(accepted.map(c => c.end), 'total_us'),
      native_us: sum(accepted.flatMap(c => c.natives.map(n => n.end)), 'total_us'),
      graph_execute_us: sum(accepted.flatMap(c => c.natives.flatMap(n => n.graphs.map(g => g.end))), 'execute_us') },
    limits: { pure_kernel_us: null, cross_host_transit_us: null, release_acceptance: false,
      graph_execute_includes: 'backend dispatch, internal copies and explicit synchronization; overlaps native decode',
      incomplete_state: 'unknown; excluded from complete totals and retained above' } };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [input, output] = process.argv.slice(2);
  check(input && output, 'usage: node analyze-native-cost.mjs stderr.log fresh-output.json');
  const bytes = fs.readFileSync(input);
  fs.writeFileSync(output, JSON.stringify({ input: { path: path.resolve(input),
    sha256: crypto.createHash('sha256').update(bytes).digest('hex') },
    ...analyzeNativeCost(bytes.toString('utf8')) }, null, 2) + '\n', { flag: 'wx' });
}
