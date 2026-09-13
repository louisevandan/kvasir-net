import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import readline from 'node:readline';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { corpusCase } from './corpus.mjs';

const sha = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const put = (file, value) => fs.writeFileSync(file, value, { flag: 'wx' });

// Alternate records preserve all facts. Subset selection changes prose format,
// never expected answers; every final prompt is re-tokenized by the real probe.
export function compactSubset(deltas, excess) {
  const reachable = new Map([[0, []]]);
  for (let i = 0; i < deltas.length; i++) {
    if (!Number.isSafeInteger(deltas[i]) || deltas[i] <= 0) continue;
    for (const [sum, ids] of [...reachable]) {
      const next = sum + deltas[i];
      if (next <= excess && !reachable.has(next)) reachable.set(next, [...ids, i]);
    }
    if (reachable.has(excess)) return reachable.get(excess);
  }
  return null;
}

async function main(specPath, output) {
  const spec = JSON.parse(fs.readFileSync(specPath));
  if (!spec.tokenizer || !spec.runtime_directory || !spec.model || !Number.isSafeInteger(spec.cases) ||
      spec.cases < 8 || spec.cases > 64 || spec.cases % 8) throw Error('invalid corpus probe inputs');
  fs.mkdirSync(output, { recursive: false });
  const probe = spawn(spec.tokenizer, [spec.model], {
    env: { ...process.env, PATH: spec.runtime_directory + path.delimiter + process.env.PATH },
    stdio: ['pipe', 'pipe', fs.openSync(path.join(output, 'tokenizer.stderr.log'), 'wx')] });
  let pending, failure;
  const exited = new Promise(resolve => probe.once('exit', code => {
    failure = Error(`tokenizer exited ${code}`); if (pending) pending.reject(failure); resolve(code);
  }));
  probe.on('error', error => { failure = error; if (pending) pending.reject(error); });
  const lines = readline.createInterface({ input: probe.stdout });
  lines.on('line', line => {
    const entry = pending; pending = null;
    if (!entry) { failure = Error('unsolicited tokenizer response'); return; }
    if (!/^[1-9][0-9]*$/.test(line)) entry.reject(Error('invalid tokenizer count'));
    else entry.resolve(Number(line));
  });
  const scratch = path.join(output, 'probe-input.txt');
  const count = async (text, tokenFile) => {
    if (failure) throw failure;
    fs.writeFileSync(scratch, text);
    const response = new Promise((resolve, reject) => { pending = { resolve, reject }; });
    probe.stdin.write(scratch + (tokenFile ? '\t' + tokenFile : '') + '\n');
    const timer = setTimeout(() => { if (pending) pending.reject(Error('tokenizer timeout')); }, 60000);
    try { return await response; } finally { clearTimeout(timer); }
  };
  const requests = [];
  let finished = false;
  try {
    for (let variant = 0; variant < spec.cases; variant++) {
      const kind = variant % 8 < 4 ? 'short' : variant % 8 < 6 ? 'medium' : 'long';
      const target = kind === 'short' ? null : kind === 'medium' ? 32000 : 100038;
      let item = corpusCase(kind, variant, kind === 'short' ? 32 : 8);
      if (target) {
        let low = 8, high = 1500;
        if (await count(corpusCase(kind, variant, high).prompt) < target) throw Error('corpus range insufficient');
        while (low < high) {
          const mid = Math.floor((low + high) / 2);
          if (await count(corpusCase(kind, variant, mid).prompt) < target) low = mid + 1;
          else high = mid;
        }
        let matched = false;
        // A fixed finite search over equivalent prose, never response-based retries.
        for (let records = low; records < low + 8 && !matched; records++) {
          const base = corpusCase(kind, variant, records);
          const excess = await count(base.prompt) - target;
          const deltas = [];
          for (const r of base.record_alternatives.slice(0, 128)) {
            deltas.push(await count('\n' + r.full + '\n') - await count('\n' + r.compact + '\n'));
          }
          const compact = compactSubset(deltas, excess);
          if (compact !== null) {
            const candidate = corpusCase(kind, variant, records, compact);
            if (await count(candidate.prompt) === target) { item = candidate; matched = true; }
          }
        }
        if (!matched) throw Error(`no exact semantic corpus sizing for ${kind}/${variant}`);
      }
      const id = `case-${String(variant).padStart(2,'0')}`;
      const tokenFile = path.join(output, id + '.tokens.bin');
      const tokens = await count(item.prompt, tokenFile);
      if (target ? tokens !== target : tokens < 2000 || tokens > 8000) throw Error('final token count mismatch');
      const tokenBytes = fs.readFileSync(tokenFile);
      if (tokenBytes.length !== tokens * 4) throw Error('token ID extent mismatch');
      put(path.join(output, id + '.prompt.txt'), item.prompt);
      put(path.join(output, id + '.oracle.json'), JSON.stringify(item.expected, null, 2) + '\n');
      put(path.join(output, id + '.facts.json'), JSON.stringify(item.source_facts, null, 2) + '\n');
      requests.push({ id, class: kind, variant, records: item.records, compact_records: item.compact_records,
        input_tokens: tokens, prompt_bytes: Buffer.byteLength(item.prompt), prompt_sha256: sha(item.prompt),
        token_ids_sha256: sha(tokenBytes), oracle_sha256: sha(fs.readFileSync(path.join(output,id+'.oracle.json'))),
        after_ms: [0,180000,480000,780000,1080000,1380000,1680000,1980000][Math.floor(variant/8)] });
      console.log(`${id} ${kind} ${tokens} tokens ${item.records} records`);
    }
    const sources = {};
    for (const name of ['corpus.mjs', 'materialize-corpus.mjs', 'tokenize.cpp']) sources[name] = sha(fs.readFileSync(new URL(name, import.meta.url)));
    const libraries = fs.readdirSync(spec.runtime_directory).filter(n => /\.(dll|so|dylib)$/.test(n)).map(name => ({
      name, sha256: sha(fs.readFileSync(path.join(spec.runtime_directory,name))) }));
    put(path.join(output,'corpus.json'), JSON.stringify({ schema: 'p4.release-a.corpus.v1', sources, requests,
      tokenizer: { binary: spec.tokenizer, sha256: sha(fs.readFileSync(spec.tokenizer)), libraries,
        source_commit: spec.tokenizer_source_commit, model: spec.model, add_special: true, parse_special: true },
      semantic_review: 'pending', current_runtime_token_equivalence: 'pending', runtime_acceptance: false }, null, 2) + '\n');
    finished = true;
  } catch (error) {
    put(path.join(output,'failure.json'), JSON.stringify({ error: String(error), completed_cases: requests }, null, 2));
    throw error;
  } finally {
    probe.stdin.end();
    if (!finished) probe.kill();
    await exited;
    lines.close();
  }
  const exit = await exited;
  if (exit !== 0) throw Error(`tokenizer exit ${exit}`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [spec, output] = process.argv.slice(2);
  if (!spec || !output) throw Error('usage: node materialize-corpus.mjs probe-spec.json fresh-output-directory');
  await main(spec, path.resolve(output));
}
