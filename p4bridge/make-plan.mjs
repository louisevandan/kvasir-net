#!/usr/bin/env node
/**
 * Build the Step-3.7-Flash placement plan for the two MI250 hosts.
 *
 * The stage-plan token list follows tools/scripts/e2e/event-four-node-config.mjs
 * (the authoritative builder) with one correction: that file targets CUDA and
 * emits `--device CUDA0`. These hosts run the HIP build, whose ggml backend
 * names its devices `ROCm<n>` — `libggml-hip.so.0` carries the prefix `ROCm`.
 * A CUDA0 plan loads the whole model and only then fails to find the device.
 *
 * Agents advertise over the InfiniBand link (10.10.10.0/24), not the office
 * LAN and not loopback: a stage hands its successor to its own agent, which
 * dials the peer agent directly (agent/src/event_runtime/transport.rs:233).
 * With a loopback address in the plan, host 01 would dial itself.
 */
import { writeFileSync } from 'node:fs';

// A node's generation and the model's load generation are one number. The
// adapter checks the RELEASE receipt's source node generation against the
// receipt's load_generation and stops the node when they differ
// (v2/transport_owners.rs:1216) — so a ring with node generation 1 and a
// timestamp load generation serves exactly one request and then loses its
// head. A fresh value per plan also keeps a reload from colliding with a node
// registration the previous attempt left behind.
const GENERATION = Number(process.env.P4_GENERATION ?? Date.now());

const TOTAL_LAYERS = 45;          // step35.block_count
const CONTEXT = 4096;
const PARALLEL = 8;
const TOTAL_CONTEXT = CONTEXT * PARALLEL;
// The physical result bound below grows with N_BATCH * N_UBATCH, and the
// agent's retained stores are 256 MiB. At 2048/512 the bound is 34 GB and no
// load can ever be admitted. 128 rows is what the cluster-inference layout
// uses in production.
const N_BATCH = 128;
const N_UBATCH = 32;
const HIDDEN_SIZE = 4096;         // step35.embedding_length
const FLASH_ATTENTION = false;    // conservative for the first recovery load

// The v4 LOAD command carries the adapter's resource profile, and the adapter
// compares `max_physical_result_bytes` for exact equality against what the
// stage server reports at READY. That figure is derive_max_physical_result_bytes
// in server/src/runtime/stage_memory_plan.cpp, reproduced here so the plan does
// not depend on a remembered constant. A stage that forwards hidden state is
// bounded by its capsule payload; the tail, which returns tokens, by its
// outcome rows. Both forms below reproduce the figures these hosts reported at
// n_batch 2048, and the terminal form also reproduces the 3,696,012 recorded
// for the cluster-inference tail stage at n_batch 128.
const STRING_MAX = 2 + 4096;                            // wire::kMaxString
const OWNER_MAX = 56 + 5 * STRING_MAX;
const ROW_MAX = 16 + 4 + 1 + OWNER_MAX + PARALLEL * 4;
const CAPSULE_HEADER = 8 + 10 * 4;
const TENSOR_DESCRIPTOR_MAX = 8 + 4 * 8 + 4 * 8 + 8 + 8 + 4 + STRING_MAX + 8;
const TENSORS_PER_CAPSULE = 2;
const OUTCOME_MAX = 6 * 4 + (4 + 4 + 2 * STRING_MAX);

/** A stage that hands hidden state to the next one. */
const nonterminalBound = () => 12
  + N_BATCH * (CAPSULE_HEADER + TENSORS_PER_CAPSULE * TENSOR_DESCRIPTOR_MAX
    + TENSORS_PER_CAPSULE * N_UBATCH * HIDDEN_SIZE * 4)
  + N_BATCH * ROW_MAX;

/** The tail, which produces tokens rather than a capsule payload. */
const terminalBound = () => 12
  + N_BATCH * CAPSULE_HEADER
  + N_BATCH * (ROW_MAX + OUTCOME_MAX);

// The settlement gateway asks for 1024 output tokens per request — enough for
// a complete answer inside the mobile client's 60s timeout. A profile below
// that rejects the gateway's every call with "request exceeds the loaded
// resource profile", so the profile follows the client, not the other way.
const MAX_OUTPUT_TOKENS_PER_REQUEST = 1024;

const resourceProfile = (physicalResultBytes) => ({
  version: 1,
  max_requests: PARALLEL,
  max_request_retained_bytes: PARALLEL * 2 * 1024 * 1024,
  max_input_tokens: TOTAL_CONTEXT,
  max_request_bytes: 2 * 1024 * 1024,
  max_output_tokens_per_request: MAX_OUTPUT_TOKENS_PER_REQUEST,
  max_output_tokens: PARALLEL * MAX_OUTPUT_TOKENS_PER_REQUEST,
  max_physical_result_bytes: physicalResultBytes,
  // Completion and edge must cover one physical result and stay inside the
  // agent's 256 MiB retained stores.
  max_completion_payload_bytes: 192 * 1024 * 1024,
  max_completion_retained_bytes: 192 * 1024 * 1024,
  max_edge_retained_bytes: 192 * 1024 * 1024,
  max_receipt_retained_bytes: 1024 * 1024,
});

const HOSTS = {
  h1: {
    agent: 'tcp://127.0.0.1:42011',
    model: '/home/banya/models/step37-merged/Step-3.7-Flash-Q4_K_XL.gguf',
    binary: '/home/banya/p4-native-build/hip/p4_staged_server',
  },
  h2: {
    agent: 'tcp://127.0.0.1:42012',
    model: '/home/banya/models/step37-merged/Step-3.7-Flash-Q4_K_XL.gguf',
    binary: '/home/banya/p4-native-build/hip/p4_staged_server',
  },
};

// Two stages per host, each on a GCD of a different MI250 package (0 and 2),
// so the pair does not share one package's memory bandwidth while the pipeline
// keeps both busy at once.
// Two stages, both on the host that still holds the model. A pipeline needs at
// least two; this is the shortest ring the engine will accept, and the fewest
// hops a token can cross.
const PLACEMENT = (process.env.P4_PLACEMENT ?? '3x1').split(',').includes('4x1')
  ? [
    { node: 'step37-s0', host: 'h1', begin: 0,  end: 12, gcd: 0, port: 42100 },
    { node: 'step37-s1', host: 'h1', begin: 12, end: 23, gcd: 2, port: 42101 },
    { node: 'step37-s2', host: 'h1', begin: 23, end: 34, gcd: 4, port: 42102 },
    { node: 'step37-s3', host: 'h1', begin: 34, end: 45, gcd: 6, port: 42103 },
  ]
  : process.env.P4_PLACEMENT === '4x2agents'
  ? [
    // Four stages, two agents, ONE host. This is the slow four-stage shape with
    // the physical host boundary removed and nothing else changed — the one
    // variable the earlier comparison moved twice at once.
    { node: 'step37-s0', host: 'h1', begin: 0,  end: 12, gcd: 0, port: 42100 },
    { node: 'step37-s1', host: 'h2', begin: 12, end: 23, gcd: 2, port: 42101 },
    { node: 'step37-s2', host: 'h1', begin: 23, end: 34, gcd: 4, port: 42102 },
    { node: 'step37-s3', host: 'h2', begin: 34, end: 45, gcd: 6, port: 42103 },
  ]
  : process.env.P4_PLACEMENT === '2x1'
  ? [
    { node: 'step37-s0', host: 'h1', begin: 0,  end: 23, gcd: 0, port: 42100 },
    { node: 'step37-s1', host: 'h1', begin: 23, end: 45, gcd: 2, port: 42101 },
  ]
  : [
    { node: 'step37-s0', host: 'h1', begin: 0,  end: 15, gcd: 0, port: 42100 },
    { node: 'step37-s1', host: 'h1', begin: 15, end: 30, gcd: 2, port: 42101 },
    { node: 'step37-s2', host: 'h1', begin: 30, end: 45, gcd: 4, port: 42102 },
  ];

const quote = (value) => {
  if (value.includes('"')) throw new Error('plan values cannot contain a double quote');
  return `"${value}"`;
};

function buildPlan({ model, begin, end }) {
  const unowned = [];
  for (let layer = 0; layer < TOTAL_LAYERS; layer += 1) {
    if (layer < begin || layer >= end) unowned.push(layer);
  }
  const tokens = [
    '--model', quote(model),
    '--memory-topology', 'discrete',
    '--layer-begin', `${begin}`,
    '--layer-end', `${end}`,
    '--kv-layer-begin', `${begin}`,
    '--kv-layer-end', `${end}`,
    '--n-seq-max', `${PARALLEL}`,
    '--spec-type', 'none',
    '--kv-unified',
    '--batch-size', `${N_BATCH}`,
    '--ubatch-size', `${N_UBATCH}`,
    '--ctx-size', `${TOTAL_CONTEXT}`,
    '--n-gpu-layers', `${TOTAL_LAYERS - begin}`,
    '--device', 'ROCm0',
    '--flash-attn', FLASH_ATTENTION ? 'on' : 'off',
    '--no-mmap',
    '--cache-type-k', 'q8_0',
    '--cache-type-v', FLASH_ATTENTION ? 'q8_0' : 'f16',
  ];
  if (unowned.length) {
    tokens.push('--override-tensor', quote(`blk\\.(${unowned.join('|')})\\..*=CPU`));
  }
  return tokens.join(' ');
}

const stages = PLACEMENT.map((stage) => {
  const host = HOSTS[stage.host];
  return {
    agent: host.agent,
    node: stage.node,
    generation: GENERATION,
    binary: host.binary,
    endpoint: `127.0.0.1:${stage.port}`,
    plan: buildPlan({ model: host.model, begin: stage.begin, end: stage.end }),
    args: [],
    environment: [
      ['PATH', '/opt/rocm/bin:/usr/local/bin:/usr/bin:/bin'],
      // HIP_VISIBLE_DEVICES is authoritative for the HIP runtime; the CUDA
      // name is set to the same value so either resolution masks one GCD.
      ['HIP_VISIBLE_DEVICES', `${stage.gcd}`],
      ['CUDA_VISIBLE_DEVICES', `${stage.gcd}`],
      // All the arithmetic runs on the GCD; the ggml CPU pool exists only to
      // drive it. libgomp's default wait policy spins those 48 threads while
      // idle, so two stages peg all 96 logical cores and the agent's per-event
      // work — which is what actually paces the ring — runs on scraps. Measured:
      // the stage servers burned ~790 CPU-seconds each per 100-token run.
      ['OMP_WAIT_POLICY', 'PASSIVE'],
      ['GOMP_SPINCOUNT', '0'],
      ['P4_STAGED_TRACE_HELLO', '1'],
      // Per-step and per-hop timing, so a stall can be attributed to a hop
      // rather than guessed at.
      ['P4_STAGED_TRACE_STEP', '1'],
      ['P4_STAGED_TRACE_HOP', '1'],

    ],
    n_batch: N_BATCH,
    n_ubatch: N_UBATCH,
    context_size: CONTEXT,
    total_context_size: TOTAL_CONTEXT,
    sequence_capacity: PARALLEL,
    resource_profile: resourceProfile(
      stage.end === TOTAL_LAYERS ? terminalBound() : nonterminalBound(),
    ),
    ready_timeout_ms: 1_800_000,
    io_timeout_ms: 120_000,
  };
});

const plan = {
  ingress_agent: HOSTS.h1.agent,
  load_generation: GENERATION,
  model: {
    id: 'step-3.7-flash',
    name: 'Step-3.7-Flash (428B MoE)',
    context_size: CONTEXT,
    max_tokens: 1024,
    // From the GGUF's own tokenizer.chat_template: ChatML turns, ending the
    // assistant turn opener with a <think> block, and <|im_end|> (128007) as
    // the end-of-turn token. p4 applies no template, so OUTER renders this.
    prompt_format: 'chatml',
    reasoning: true,
  },
  stages,
};

const out = new URL('./load-plan.step37.json', import.meta.url).pathname;
writeFileSync(out, JSON.stringify(plan, null, 2) + '\n');
console.log(`wrote ${out} — ${stages.length} stages at generation ${GENERATION}`);
for (const [index, stage] of stages.entries()) {
  const p = PLACEMENT[index];
  console.log(`  ${stage.node}  layers [${p.begin}, ${p.end})  GCD ${p.gcd}  agent ${stage.agent}  stage ${stage.endpoint}`);
}
