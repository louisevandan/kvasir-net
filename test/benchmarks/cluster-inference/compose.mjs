import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { dossier } from './workloads/engineering-dossier.mjs';

export const sha256 = value => crypto.createHash('sha256').update(value).digest('hex');
const integer = (value, name, minimum = 0) => {
  if (!Number.isSafeInteger(value) || value < minimum) throw Error(`${name} must be an integer >= ${minimum}`);
  return value;
};

/** Compose one OUTER input; cluster placement and runtime identity remain explicit inputs. */
export function compose({ cluster, model, policy, workload, runtime, runId, generation, tokenCounts }) {
  if (!/^[a-zA-Z0-9][a-zA-Z0-9._-]+$/.test(runId ?? '')) throw Error('invalid runId');
  integer(generation, 'generation', 1);
  if (!cluster.nodes?.length) throw Error('cluster needs nodes');
  if (!runtime?.sources?.length || runtime.sources.some(x => !/^[0-9a-f]{40}$/.test(x))) throw Error('full runtime source commits required');
  if (!runtime.binding_file_sha256?.match(/^[0-9a-f]{64}$/)) throw Error('runtime binding file SHA256 required');
  const config = structuredClone(cluster);
  const maxTokens = integer(workload.max_output_tokens, 'max output', 1);
  const minimum = integer(workload.minimum_output_tokens, 'minimum output', 1);
  if (minimum > maxTokens) throw Error('minimum output exceeds maximum');
  if (workload.kind !== 'engineering-dossier') throw Error('unsupported workload kind');
  if (!workload.record_counts?.length) throw Error('workload has no requests');
  const template = model.template;
  for (const key of ['system_prefix', 'system_suffix', 'user_prefix', 'assistant_prefix']) {
    if (typeof template?.[key] !== 'string') throw Error(`model template missing ${key}`);
  }
  if (!Array.isArray(model.stop) || model.stop.some(x => typeof x !== 'string' || !x)) throw Error('model stop strings required');
  const requests = workload.record_counts.map((records, index) => {
    integer(records, 'records', 4);
    const item = dossier(records, index);
    const prompt = template.system_prefix + item.messages[0].content + template.system_suffix
      + template.user_prefix + item.messages[1].content + template.assistant_prefix;
    return { index, prompt, prompt_sha256: sha256(prompt), prompt_bytes: Buffer.byteLength(prompt), expected: item.expected };
  });
  const capacity = Math.min(...config.nodes.map(n => integer(n.sequence_capacity, 'sequence_capacity', 1)));
  const context = Math.min(...config.nodes.map(n => integer(n.context_size, 'context_size', 1)));
  if (maxTokens >= context) throw Error('output leaves no prompt context');
  const env = {};
  for (const [name, value] of Object.entries(policy.environment ?? {})) {
    if (!/^P4_STAGED_(MIN_BATCH_ROWS|MAX_OPEN_BATCHES|MAX_ISSUE_ROWS|PREFILL_FRAGMENTS|DECODE_MEMBERS|PREFILL_MEMBERS|PREFILL_ROWS|PREFILL_ROWS_PER_REQUEST|PIPELINE_BATCHING|MIXED_PREFILL_ROWS|MIXED_BATCH_ROWS)$/.test(name)) throw Error(`unsupported policy field ${name}`);
    integer(value, name, name === 'P4_STAGED_PREFILL_FRAGMENTS' ? 1 : 0);
    env[name] = String(value);
  }
  if (Number(env.P4_STAGED_DECODE_MEMBERS ?? 0) > capacity) throw Error('decode cap exceeds resident capacity');
  if (Number(env.P4_STAGED_PREFILL_MEMBERS ?? 0) > capacity) throw Error('prefill cap exceeds resident capacity');
  if (Number(env.P4_STAGED_PIPELINE_BATCHING ?? 0) > 1) throw Error('pipeline batching must be 0 or 1');
  if (env.P4_STAGED_MIXED_BATCH_ROWS !== undefined &&
      (env.P4_STAGED_PIPELINE_BATCHING !== '1' || Number(env.P4_STAGED_MIXED_BATCH_ROWS) === 0)) {
    throw Error('profiled mixed token budget requires pipeline policy and positive tokens');
  }
  if (env.P4_STAGED_PIPELINE_BATCHING === '1' && (Number(env.P4_STAGED_MAX_OPEN_BATCHES ?? 0) === 0
    || Number(env.P4_STAGED_MIXED_PREFILL_ROWS ?? 128) === 0
    || Number(env.P4_STAGED_PREFILL_FRAGMENTS ?? 1) !== 1)) {
    throw Error('pipeline policy needs finite open window, positive mixed quantum and fragment limit one');
  }
  const waves = workload.waves ?? [{ after_ms: 0, count: requests.length }];
  let last = -1, total = 0;
  for (const wave of waves) {
    integer(wave.after_ms, 'wave time'); integer(wave.count, 'wave count', 1);
    if (wave.after_ms < last) throw Error('waves must be ordered');
    last = wave.after_ms; total += wave.count;
  }
  if (total !== requests.length) throw Error('wave/request count mismatch');
  let tokenizerVerified = false;
  if (tokenCounts !== undefined) {
    if (tokenCounts.length !== requests.length) throw Error('token count length mismatch');
    requests.forEach((request, i) => {
      const count = tokenCounts[i];
      if (count.prompt_sha256 !== request.prompt_sha256) throw Error('token count belongs to another prompt');
      integer(count.prompt_tokens, 'prompt tokens', 1);
      if (count.prompt_tokens + maxTokens > context) throw Error('prompt and output exceed context');
      request.prompt_tokens = count.prompt_tokens;
    });
    tokenizerVerified = true;
  }
  config.nodes.forEach((node, i) => {
    node.node = `${runId}-n${i}`; node.generation = generation;
    if (policy.native_threads !== undefined) {
      const threads = integer(policy.native_threads, 'native threads', 1);
      if (/--threads(?:-batch)?\b/.test(node.plan)) throw Error('base plan already owns native threads; remove duplication explicitly');
      node.plan += ` --threads ${threads} --threads-batch ${threads}`;
    }
  });
  Object.assign(config, {
    channel: runId, session_id: runId, connection_generation: generation, load_generation: generation,
    prompt: '', prompts: requests.map(r => r.prompt), session_key_template: '', max_tokens: maxTokens, waves,
    pre_inference_hold_ms: 0,
    options: JSON.stringify({ temperature: 0, seed: 7, top_p: .9, top_k: 20, ignore_eos: false, stop: model.stop }),
    acceptance: { minimum_generated_tokens: minimum, allowed_stop_reasons: ['eos'], responses: requests.map(r => ({
      required_substrings: [...r.expected.map(x => x.id), '검증 완료: 압력과 발열의 인과관계는 미확인입니다.'],
    })) },
  });
  return { config, manifest: {
    schema_version: 1, run_id: runId, model: model.id, policy: policy.id, workload: workload.id,
    runtime, context_per_session: context, resident: capacity, agent_environment: env,
    tokenizer_verified: tokenizerVerified, config_sha256: sha256(JSON.stringify(config, null, 2)),
    requests: requests.map(({ prompt, ...request }) => request),
    scope: 'Configuration and identity binding only. Tokenizer checks, lifecycle preflight, GPU execution and prose acceptance are separate gates.',
  } };
}

/** Resolve reusable profiles relative to the experiment file, never a branch/worktree name. */
export function compileFile(specFile, outputDirectory) {
  const spec = JSON.parse(fs.readFileSync(specFile, 'utf8'));
  const read = name => JSON.parse(fs.readFileSync(path.resolve(path.dirname(specFile), spec[name]), 'utf8'));
  const input = { ...spec, cluster: read('cluster'), model: read('model'), policy: read('policy'), workload: read('workload'), runtime: read('runtime') };
  if (spec.tokenCounts) input.tokenCounts = read('tokenCounts');
  const result = compose(input);
  // Exclusive creation: a measured arm must never be overwritten.
  fs.mkdirSync(outputDirectory);
  fs.writeFileSync(path.join(outputDirectory, 'config.json'), JSON.stringify(result.config, null, 2));
  fs.writeFileSync(path.join(outputDirectory, 'manifest.json'), JSON.stringify(result.manifest, null, 2));
  return result;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  if (process.argv.length !== 4) throw Error('usage: node compose.mjs experiment.json NEW_OUTPUT_DIRECTORY');
  const result = compileFile(path.resolve(process.argv[2]), path.resolve(process.argv[3]));
  console.log(JSON.stringify({ run_id: result.manifest.run_id, tokenizer_verified: result.manifest.tokenizer_verified, requests: result.config.prompts.length }));
}
