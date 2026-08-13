import { ControllerInstance } from '../../client/controller-instance.mjs';
import { randomUUID } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { performance } from 'node:perf_hooks';
import { ringTopology } from '../../topology/pipeline-topology.mjs';
import { compatibleRingCapabilities } from '../../capability/pipeline-capabilities.mjs';
import { measuredModelLoadOptions } from '../../capability/model-load-policy.mjs';
import { distribution, writePlan, writeSummary, writeTrace } from '../../evidence/run-evidence.mjs';
import { startGpuTelemetry, summarizeGpuTelemetry } from '../../evidence/gpu-telemetry.mjs';
import { buildGroupStats } from '../../evidence/pipeline-group-stats.mjs';
import { executeSteadyArrivals, executeWindow, steadyArrivalPlan } from './steady-arrival-plan.mjs';

// The owned E2E can outlive an invoking terminal. Preserve its artifacts even
// when that terminal closes the inherited stdout pipe before the run ends.
process.stdout.on('error', (error) => {
  if (error.code !== 'EPIPE') throw error;
});

// Native ceiling measured; see ../../../../docs/runtime-evidence.md#2026-08-09-native-pipeline-capacity-ceiling-and-parallel256-proof.
const MAX_NATIVE_PIPELINE_PARALLEL = 256;
// Matches MAX_INFLIGHT_RANGE in layers/runtime/src/domain/agent/lifecycle/node_spec.
const MAX_AGENT_SLOT_WIDTH = 1024;
const [endpoint, nodeId, host, promptArgument, maxTokensArgument, parallelArgument, concurrentArgument, preserveFailedGroupArgument, traceFileArgument, planFileArgument, summaryFileArgument, reportFileArgument, benchmarkArgument] = process.argv.slice(2);
const prompt = promptArgument || 'Reply with one short Korean greeting.';
const maxTokens = Number(maxTokensArgument || 8);
if (!Number.isInteger(maxTokens) || maxTokens < 1 || maxTokens > 1024) throw new Error('maxTokens must be an integer from 1 to 1024');
const parallel = Number(parallelArgument || 1);
const concurrentRequests = Number(concurrentArgument || parallel);
const totalRequests = Number(process.env.P4_TOTAL_REQUESTS ?? concurrentRequests);
const initialRequests = Number(process.env.P4_INITIAL_REQUESTS ?? concurrentRequests);
const arrivalIntervalMs = Number(process.env.P4_ARRIVAL_INTERVAL_MS ?? 0);
const executionWindow = Number(process.env.P4_EXECUTION_WINDOW ?? totalRequests);
const nativeParallel = Number(process.env.P4_NATIVE_PARALLEL ?? parallel);
// Arrival admission, not plan width. A NodeSlot defaults to one permit, so a
// node_spec without this value serialises every stream at the Agent and the
// adapter never sees two requests to coalesce. Sized from concurrent arrivals;
// the adapter queue and its per-deployment gate remain the real bound.
const agentSlotWidth = Number(process.env.P4_AGENT_SLOT_WIDTH ?? Math.min(totalRequests, MAX_AGENT_SLOT_WIDTH));
const preserveFailedGroup = preserveFailedGroupArgument === '1';
const benchmark = benchmarkArgument === '1';
const benchmarkIgnoreEog = process.env.P4_PIPELINE_BENCHMARK_IGNORE_EOG === '1';
if (benchmarkIgnoreEog && !benchmark) {
  throw new Error('P4_PIPELINE_BENCHMARK_IGNORE_EOG requires benchmark mode');
}
if (!Number.isInteger(parallel) || parallel < 1 || parallel > 65_535) throw new Error('parallel must be an integer from 1 to 65535');
if (!Number.isInteger(concurrentRequests) || concurrentRequests < 1 || concurrentRequests > parallel) throw new Error('concurrentRequests must be an integer from 1 to parallel');
if (!Number.isInteger(totalRequests) || totalRequests < concurrentRequests || totalRequests > MAX_AGENT_SLOT_WIDTH) throw new Error(`P4_TOTAL_REQUESTS must be an integer from concurrentRequests to ${MAX_AGENT_SLOT_WIDTH}`);
if (!Number.isInteger(initialRequests) || initialRequests < 1 || initialRequests > concurrentRequests) throw new Error('P4_INITIAL_REQUESTS must be an integer from 1 to concurrentRequests');
if (!Number.isInteger(arrivalIntervalMs) || arrivalIntervalMs < 0) throw new Error('P4_ARRIVAL_INTERVAL_MS must be a non-negative integer');
if (totalRequests > initialRequests && arrivalIntervalMs === 0) throw new Error('P4_ARRIVAL_INTERVAL_MS must be positive when P4_TOTAL_REQUESTS exceeds P4_INITIAL_REQUESTS');
if (!Number.isInteger(executionWindow) || executionWindow < 1 || executionWindow > totalRequests) throw new Error('P4_EXECUTION_WINDOW must be an integer from 1 to P4_TOTAL_REQUESTS');
if (totalRequests > initialRequests && executionWindow < totalRequests) throw new Error('P4_EXECUTION_WINDOW must cover P4_TOTAL_REQUESTS for steady ingress; otherwise the client, not the Pipeline, queues requests');
if (!Number.isInteger(nativeParallel) || nativeParallel < 1 || nativeParallel > MAX_NATIVE_PIPELINE_PARALLEL) throw new Error(`P4_NATIVE_PARALLEL must be an integer from 1 to ${MAX_NATIVE_PIPELINE_PARALLEL}`);
if (!Number.isInteger(agentSlotWidth) || agentSlotWidth < 1 || agentSlotWidth > MAX_AGENT_SLOT_WIDTH) throw new Error(`P4_AGENT_SLOT_WIDTH must be an integer from 1 to ${MAX_AGENT_SLOT_WIDTH}`);
// Acceptance bound for a parallel run: aggregate throughput must hold the
// measured single-stream rate of the same placement.
const referenceTps = process.env.P4_REFERENCE_TPS === undefined ? undefined : Number(process.env.P4_REFERENCE_TPS);
if (referenceTps !== undefined && (!Number.isFinite(referenceTps) || referenceTps <= 0)) throw new Error('P4_REFERENCE_TPS must be a positive number');
const model = process.env.P4_PIPELINE_MODEL ?? 'Qwen2.5-1.5B-Instruct-Q8_0.gguf';
const adapterId = process.env.P4_PIPELINE_ADAPTER_ID ?? 'adapter-local';
const promptSet = await loadPromptSet(process.env.P4_PREFILL_PROMPT_FILE);
const promptOffset = Number(process.env.P4_PREFILL_PROMPT_OFFSET ?? 0);
if (!Number.isInteger(promptOffset) || promptOffset < 0) throw new Error('P4_PREFILL_PROMPT_OFFSET must be a non-negative integer');
const batch = Number(process.env.P4_PIPELINE_BATCH ?? 256);
const ubatch = Number(process.env.P4_PIPELINE_UBATCH ?? 128);
const rawNodeBatchLimits = process.env.P4_NODE_BATCH_LIMITS_JSON;
const firstStageLayers = process.env.P4_PIPELINE_FIRST_STAGE_LAYERS === undefined
  ? undefined
  : Number(process.env.P4_PIPELINE_FIRST_STAGE_LAYERS);
const parseNodeBatchLimits = (raw) => {
  if (raw === undefined || !raw.trim()) return undefined;
  let parsed;
  try {
    parsed = JSON.parse(raw);
  } catch (error) {
    throw new Error(`P4_NODE_BATCH_LIMITS_JSON invalid JSON: ${String(error)}`);
  }
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
    throw new Error('P4_NODE_BATCH_LIMITS_JSON must be a JSON object map');
  }
  return parsed;
};
const backendSamplingProfile = (() => {
  const raw = process.env.P4_PIPELINE_BACKEND_SAMPLING_PROFILE;
  if (raw === undefined) return undefined;
  const [temperature, topP, topK, seed, extra] = raw.split(':');
  const parsed = {
    temperature: Number(temperature),
    top_p: Number(topP),
    top_k: Number(topK),
    seed: Number(seed)
  };
  if (extra !== undefined || !Number.isFinite(parsed.temperature) || parsed.temperature < 0
    || !Number.isFinite(parsed.top_p) || parsed.top_p <= 0 || parsed.top_p > 1
    || !Number.isInteger(parsed.top_k) || parsed.top_k < 0
    || !Number.isInteger(parsed.seed) || parsed.seed < 0) {
    throw new Error('P4_PIPELINE_BACKEND_SAMPLING_PROFILE must be temperature:top_p:top_k:seed');
  }
  return parsed;
})();
if (!Number.isInteger(batch) || !Number.isInteger(ubatch) || batch < 1 || ubatch < 1 || ubatch > batch) {
  throw new Error(`Invalid P4 pipeline batch configuration: batch=${batch} ubatch=${ubatch}`);
}
// Independent controller processes can start in the same millisecond.  The
// runtime registry and session allocator require this root to be unique across
// those processes, not merely within one Node.js event loop.
const runIdentity = `${process.env.P4_E2E_RUN_ID ?? 'run'}-${randomUUID()}`;
const controllerId = `p4-adapter-e2e-${runIdentity}`;
const peerControllerId = `${controllerId}-peer`;
const deploymentId = `p4-adapter-deployment-${runIdentity}`;
const bindingId = `p4-adapter-binding-${runIdentity}`;
const peerNodeId = `pipeline-peer-marker-${runIdentity}`;
const contextPerRequestTokens = Math.max(
  1024,
  maxTokens + 512,
  maxTokens + promptSet.targetTokens + 128,
  promptSet.contextPerRequestTokens
);
const contextTokens = nativeParallel * contextPerRequestTokens;
const stagePortBase = Number(process.env.P4_PIPELINE_STAGE_PORT_BASE ?? 52221);
if (!Number.isInteger(stagePortBase) || stagePortBase < 1024 || stagePortBase > 65534) {
  throw new Error(`P4_PIPELINE_STAGE_PORT_BASE must be an integer from 1024 to 65534; received ${stagePortBase}`);
}
const topology = ringTopology(stagePortBase);
const { nodes, plannerNodes } = topology;
const nodeBatchLimits = parseNodeBatchLimits(rawNodeBatchLimits);
const loadOptions = await measuredModelLoadOptions({
  model,
  nodes,
  parallel: nativeParallel,
  batch,
  ubatch,
  nodeBatchLimits
});
// A stage with no measured profile is capped at one sequence, which collapses
// the whole group without producing a single error. Say so before loading.
if (loadOptions.batching.unverified_nodes.length) {
  console.log(
    `P4_UNVERIFIED_STAGES nodes=${loadOptions.batching.unverified_nodes.join(',')} `
      + `model=${model} max_sequences=${loadOptions.batching.max_sequences} `
      + 'detail=no measured batch profile matched; the group runs one sequence at a time. '
      + 'Add a profile to tools/controller/capability/measured-batch-profiles.json '
      + 'or set P4_NODE_BATCH_LIMITS_JSON.',
  );
}
console.log(
  `P4_BATCH_POLICY max_sequences=${loadOptions.batching.max_sequences} `
    + `per_node=${loadOptions.batching.node_limits.map((n) => `${n.node_id}:${n.max_sequences}`).join(',')}`,
);
const stagePorts = nodes.map((node) => node.node_port);
const forcedStageNodeOrder = process.env.P4_PIPELINE_STAGE_NODE_ORDER === undefined
  ? undefined
  : process.env.P4_PIPELINE_STAGE_NODE_ORDER.split(',').map((nodeId) => nodeId.trim()).filter(Boolean);
const post = async (path, body) => {
  const response = await fetch(`${host}${path}`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) });
  if (!response.ok) throw new Error(`${path} HTTP ${response.status}: ${await response.text()}`);
  return response.json();
};
const groupStats = async (groupHost = host) => {
  const response = await fetch(`${groupHost}/api/runtime-groups/${deploymentId}`);
  if (!response.ok) throw new Error(`runtime group stats HTTP ${response.status}: ${await response.text()}`);
  return buildGroupStats(await response.json(), nodes);
};
const controller = new ControllerInstance({ controllerId, endpoint });
const peerController = new ControllerInstance({ controllerId: peerControllerId, endpoint });
const remoteController = topology.remoteAgentEndpoint ? new ControllerInstance({ controllerId, endpoint: topology.remoteAgentEndpoint }) : undefined;
const localNodeId = nodeId || 'pipeline-2gpu';
const remoteNodeId = `${localNodeId}-remote`;
let generation;
let remoteGeneration;
let completed = false;
const latency = {};
let parallelTraces = [];
let activeInferenceStreams = 0;
const inferRequest = (requestId, maxTokens, requestPrompt = prompt) => controller.infer({ nodeId: localNodeId, deploymentId, bindingId, runtimeGeneration: generation, prompt: requestPrompt, sessionId: '', requestId, maxTokens, temperature: 0.2, options: { top_p: 0.9, top_k: 20, seed: 7 } });
const executeTraced = async (request) => {
  const { request_id: requestId, prompt: requestPrompt } = request;
  const requestStarted = performance.now();
  const trace = { request: { type: 'INGRESS_SUBMIT', ...request }, controller_wait_ms: Number((requestStarted - request.scheduled_at).toFixed(3)), responses: [] };
  let accepted = false; let tokens = 0; let done = false; let text = ''; let streamActive = false;
  try {
    for await (const event of inferRequest(requestId, maxTokens, requestPrompt)) {
      const elapsed_ms = Number((performance.now() - requestStarted).toFixed(3));
      if (event.type === 'accepted') {
        accepted = true;
        trace.responses.push({ type: 'INGRESS_ACCEPTED', ingress_id: event.ingressId, request_id: event.requestId, session_id: event.sessionId, elapsed_ms });
      } else if (event.type === 'token') {
        if (!streamActive) {
          streamActive = true;
          activeInferenceStreams += 1;
        }
        tokens += 1; text += event.text;
        trace.responses.push({ type: 'TOKEN', request_id: event.requestId, session_id: event.sessionId, phase: event.phase, position: event.position, index: event.index, text: event.text, elapsed_ms });
      } else if (event.type === 'done') {
        done = true;
        trace.responses.push({ type: 'DONE', request_id: event.requestId, session_id: event.sessionId, reason: event.reason, generated_tokens: event.generatedTokens, elapsed_ms });
      }
    }
  } catch (error) {
    trace.responses.push({ type: 'ERROR', request_id: requestId, detail: String(error), elapsed_ms: Number((performance.now() - requestStarted).toFixed(3)) });
  } finally {
    if (streamActive) activeInferenceStreams -= 1;
  }
  trace.final_text = text;
  trace.completed = accepted && done && tokens > 0;
  return { accepted, tokens, done, trace };
};

try {
  let started = performance.now();
  const plan = await post('/api/plans', { model, nodes: plannerNodes, ctx: contextTokens, parallel: nativeParallel, reserve_mib: 128, cache_type_k: 'q8_0', cache_type_v: 'q8_0', loading_strategy: 'pipeline-stage-vram-only' });
  latency.plan_ms = performance.now() - started;
  if (!plan.feasible || plan.nodes_used !== nodes.length) throw new Error(`${nodes.length}-stage pipeline plan was not feasible`);
  if (firstStageLayers !== undefined) {
    const placements = [...plan.placement].sort((left, right) => left.stage_index - right.stage_index);
    const totalLayers = plan.model.n_layer;
    if (!Number.isInteger(firstStageLayers) || firstStageLayers < 1 || firstStageLayers >= totalLayers || placements.length !== 2) {
      throw new Error(`Invalid P4_PIPELINE_FIRST_STAGE_LAYERS=${firstStageLayers} for ${totalLayers}-layer two-stage plan`);
    }
    const ranges = [[0, firstStageLayers], [firstStageLayers, totalLayers]];
    for (const [index, placement] of placements.entries()) {
      const previousLayers = placement.n_layers;
      const nextLayers = ranges[index][1] - ranges[index][0];
      const layerVramPerLayer = placement.layer_body_vram_gib / previousLayers;
      const kvVramPerLayer = placement.kv_vram_gib / previousLayers;
      placement.layers = ranges[index];
      placement.n_layers = nextLayers;
      placement.kv_gpu_layer_start = ranges[index][0];
      placement.kv_gpu_layer_end = ranges[index][1];
      placement.layer_body_vram_gib = layerVramPerLayer * nextLayers;
      placement.kv_vram_gib = kvVramPerLayer * nextLayers;
      placement.vram_used_gib = placement.boundary_vram_gib + placement.layer_body_vram_gib + placement.kv_vram_gib;
    }
    plan.tensor_split = ranges.map(([start, end]) => end - start);
  }
  if (forcedStageNodeOrder !== undefined) {
    const placements = [...plan.placement].sort((left, right) => left.stage_index - right.stage_index);
    if (forcedStageNodeOrder.length !== placements.length
      || new Set(forcedStageNodeOrder).size !== forcedStageNodeOrder.length) {
      throw new Error(`P4_PIPELINE_STAGE_NODE_ORDER must name each of ${placements.length} stages exactly once`);
    }
    for (const [stageIndex, placement] of placements.entries()) {
      const nodeId = forcedStageNodeOrder[stageIndex];
      const nodeIndex = nodes.findIndex((node) => node.id === nodeId);
      if (nodeIndex < 0) throw new Error(`P4_PIPELINE_STAGE_NODE_ORDER references unknown node ${nodeId}`);
      placement.node = nodeIndex;
      placement.node_id = nodeId;
    }
  }
  started = performance.now();
  const inventories = await Promise.all([
    controller.inventory().then((inventory) => ({ endpoint, ...inventory })),
    remoteController ? remoteController.inventory().then((inventory) => ({ endpoint: topology.remoteAgentEndpoint, ...inventory })) : Promise.resolve(undefined)
  ]);
  latency.inventory_ms = performance.now() - started;
  const usableInventories = inventories.filter(Boolean);
  for (const inventory of usableInventories) console.log(`P4_INVENTORY agent=${inventory.agentId} endpoint=${inventory.endpoint} adapters=${inventory.snapshot.adapters.length} gpus=${inventory.snapshot.gpus.length}`);
  const nativeRingCapabilities = compatibleRingCapabilities(usableInventories, adapterId);
  console.log(`P4_PIPELINE_CAPABILITY ${JSON.stringify(nativeRingCapabilities)}`);
  started = performance.now();
  const [node, peerNode] = await Promise.all([
    controller.createNode({ nodeId: localNodeId, adapterId, nodeSpec: { resource_policy: 'adapter-owned', topology: topology.remoteIds.length ? 'four-stage-cross-host' : 'two-stage', p4_max_inflight: agentSlotWidth } }),
    peerController.createNode({ nodeId: peerNodeId, adapterId, nodeSpec: { resource_policy: 'adapter-owned', topology: 'marker-only' } }),
    remoteController ? remoteController.createNode({ nodeId: remoteNodeId, adapterId, nodeSpec: { resource_policy: 'adapter-owned', topology: 'remote-stage-owner', p4_max_inflight: agentSlotWidth } }) : Promise.resolve(undefined)
  ]);
  latency.node_create_ms = performance.now() - started;
  if (node.state !== 'ready') throw new Error(`P4 pipeline node is not ready: ${node.detail}`);
  if (peerNode.state !== 'ready') throw new Error(`P4 peer node is not ready: ${peerNode.detail}`);
  console.log(`P4_NODE state=${node.state} id=${node.nodeId}`);
  console.log(`P4_AGENT_ADMISSION slot_width=${agentSlotWidth} concurrent_requests=${concurrentRequests} total_requests=${totalRequests} source=${process.env.P4_AGENT_SLOT_WIDTH ? 'P4_AGENT_SLOT_WIDTH' : 'total_requests'} axis=arrival`);
  console.log(`P4_MULTI_CONTROLLER_PASS controllers=2 nodes=2 peer_node=${peerNode.nodeId}`);
  let sawDraft = false;
  const adapterRequest = {
    nodes, plan, ctx: contextTokens, parallel: nativeParallel, master_port: stagePorts[0], batch, ubatch,
    ...(backendSamplingProfile ? { backend_sampling_profile: backendSamplingProfile } : {}),
    ...(benchmarkIgnoreEog ? { benchmark_ignore_eog: true } : {})
  };
  const load = async (client, targetNodeId, ownedIds, label) => {
    let bound;
    for await (const event of client.loadModel({ nodeId: targetNodeId, deploymentId, bindingId, model, planRevision: 'pipeline-v1', stagePlan: topology.agentStagePlan(ownedIds, adapterRequest), loadOptions })) {
      if (event.type === 'load-progress') console.log(`P4_LOAD agent=${label} percent=${event.percent} detail=${event.detail}`);
      if (event.type === 'draft-report') { sawDraft = true; console.log(`P4_DRAFT agent=${label} model=${event.modelBytes} kv=${event.kvBytes} layer=${event.layerBytes} ffn=${event.ffnBytes}`); }
      if (event.type === 'model-bound') bound = event.runtimeGeneration;
    }
    return bound;
  };
  started = performance.now();
  [generation, remoteGeneration] = await Promise.all([load(controller, localNodeId, topology.localIds, 'local'), remoteController ? load(remoteController, remoteNodeId, topology.remoteIds, 'remote') : Promise.resolve(undefined)]);
  latency.model_load_ms = performance.now() - started;
  if (!sawDraft || !generation) throw new Error('P4 did not receive a ready model binding');
  started = performance.now();
  const health = await controller.health({ nodeId: localNodeId });
  latency.health_ms = performance.now() - started;
  if (!health.ready) throw new Error(`P4 pipeline health is not ready: ${health.detail}`);
  console.log(`P4_HEALTH ready=${health.ready} detail=${health.detail}`);
  const arrivals = steadyArrivalPlan(totalRequests, initialRequests, arrivalIntervalMs);
  const requests = promptsFor(totalRequests).map((requestPrompt, index) => ({
    index,
    request_id: `${controllerId}-arrival-${index}`,
    arrival: arrivals[index],
    prompt: requestPrompt,
    max_tokens: maxTokens,
    temperature: 0.2,
    options: { top_p: 0.9, top_k: 20, seed: 7 }
  }));
  await writePlan(planFileArgument, requests);
  started = performance.now();
  const gpuMonitor = process.env.P4_GPU_TELEMETRY === '1' ? startGpuTelemetry() : undefined;
  let results;
  let gpuSamples = [];
  try {
    results = totalRequests === initialRequests
      ? await executeWindow(requests, executionWindow, executeTraced)
      : await executeSteadyArrivals(
        requests, executeTraced, () => activeInferenceStreams > 0);
  } finally {
    if (gpuMonitor) gpuSamples = await gpuMonitor.stop();
  }
  parallelTraces = results.map((result) => result.trace);
  await writeTrace(traceFileArgument, parallelTraces);
  if (!results.every((result) => result.accepted && result.done && result.tokens > 0)
    || !parallelTraces.every((trace) => {
      const done = trace.responses.find((response) => response.type === 'DONE');
      return done
        && done.generated_tokens <= trace.request.max_tokens
        && (!benchmarkIgnoreEog || done.generated_tokens === trace.request.max_tokens);
    })) throw new Error('P4 parallel requests did not all stream and finish within max_tokens');
  latency.parallel_requests_ms = performance.now() - started;
  latency.parallel_completed = results.length;
  latency.parallel_streamed_events = results.reduce((sum, result) => sum + result.tokens, 0);
  console.log(`P4_ARRIVAL_PASS parallel=${parallel} initial=${initialRequests} steady=${totalRequests - initialRequests} interval_ms=${arrivalIntervalMs} overlap=${totalRequests > initialRequests ? 'verified' : 'not_requested'} requests=${results.length} unique_prompts=${new Set(requests.map((request) => request.prompt)).size} streamed_events=${latency.parallel_streamed_events} elapsed_ms=${latency.parallel_requests_ms.toFixed(3)}`);
  const tokens = latency.parallel_streamed_events;
  const text = parallelTraces.map((trace) => trace.final_text).join('');
  if (!tokens || !text) throw new Error('P4 pipeline did not stream a token');
  const ringStats = await groupStats();
  const remoteRingStats = topology.remoteGroupHost ? await groupStats(topology.remoteGroupHost) : undefined;
  if (process.env.P4_PIPELINE_EXPECT_SHARED_MEMORY === '1') {
    const sent = ringStats.stages.reduce((total, stage) => total + stage.shared_memory.sent_frames, 0);
    const received = ringStats.stages.reduce((total, stage) => total + stage.shared_memory.received_frames, 0);
    if (!sent || !received) throw new Error(`P4 shared-memory transport was not observed: sent=${sent} received=${received}`);
    console.log(`P4_SHARED_MEMORY_PASS sent=${sent} received=${received}`);
  }
  const gpuTraceFile = summaryFileArgument?.replace(/\.json$/i, '-gpu.jsonl');
  if (gpuTraceFile && gpuSamples.length) await writeFile(gpuTraceFile, `${gpuSamples.map((sample) => JSON.stringify(sample)).join('\n')}\n`, 'utf8');
  const runStats = { prompt, prompt_fixture: promptSet.source, prompt_offset: promptOffset, prompt_target_tokens: promptSet.targetTokens, benchmark, benchmark_ignore_eog: benchmarkIgnoreEog, requested_max_tokens: maxTokens, parallel, concurrent_requests: concurrentRequests, total_requests: totalRequests, initial_requests: initialRequests, steady_arrivals: totalRequests - initialRequests, arrival_interval_ms: arrivalIntervalMs, steady_overlap_verified: totalRequests > initialRequests, execution_window: executionWindow, native_parallel: nativeParallel, agent_slot_width: agentSlotWidth, ...(referenceTps === undefined ? {} : { reference_tps: referenceTps }), batch, ubatch, model_load_options: loadOptions, stage_layers: adapterRequest.plan.placement.map((placement) => placement.layers), stage_nodes: adapterRequest.plan.placement.map((placement) => placement.node_id ?? nodes[placement.node]?.id), context_tokens: contextTokens, context_per_request_tokens: contextPerRequestTokens, generated_token_events: tokens, response_chars: text.length, p4_latency_ms: latency, ...(gpuSamples.length ? { gpu_telemetry: { trace_file: gpuTraceFile, summary: summarizeGpuTelemetry(gpuSamples, distribution) } } : {}), ...ringStats, ...(remoteRingStats ? { remote_group: remoteRingStats } : {}) };
  await writeSummary(summaryFileArgument, reportFileArgument, runStats, parallelTraces);
  console.log(`P4_PIPELINE_STATS ${JSON.stringify(runStats)}`);
  completed = true;
  console.log('P4_PIPELINE_E2E_CLIENT_PASS');
} finally {
  if (generation) await controller.unloadModel({ nodeId: localNodeId, deploymentId, bindingId }).catch(() => undefined);
  if (remoteGeneration) await remoteController?.unloadModel({ nodeId: remoteNodeId, deploymentId, bindingId }).catch(() => undefined);
  if (completed || !preserveFailedGroup) await fetch(`${host}/api/runtime-groups/${deploymentId}`, { method: 'DELETE' }).catch(() => undefined);
}

function promptsFor(count) {
  if (promptSet.prompts.length >= promptOffset + count) return promptSet.prompts.slice(promptOffset, promptOffset + count);
  if (promptSet.prompts.length) throw new Error(`prompt fixture ${promptSet.source} supplies ${promptSet.prompts.length} prompts, but offset=${promptOffset} count=${count} are required`);
  return languagePrompts(count, promptOffset);
}

async function loadPromptSet(promptFile) {
  if (!promptFile) return { source: 'built-in-language-prompts', targetTokens: 0, contextPerRequestTokens: 0, prompts: [] };
  const fixture = JSON.parse(await readFile(promptFile, 'utf8'));
  if (fixture.format !== 'p4-prefill-prompt-set/v1' || !Array.isArray(fixture.prompts)) throw new Error(`invalid P4 prompt fixture: ${promptFile}`);
  const prompts = fixture.prompts.map((entry, index) => {
    if (!entry || typeof entry.id !== 'string' || typeof entry.text !== 'string' || entry.text.trim().length < 550) throw new Error(`invalid meaningful prompt at index ${index} in ${promptFile}`);
    return entry.text;
  });
  if (new Set(prompts).size !== prompts.length) throw new Error(`prompt fixture contains duplicate prompts: ${promptFile}`);
  const targetTokens = Number(fixture.target_tokens);
  const contextPerRequestTokens = Number(fixture.context_per_request_tokens);
  if (!Number.isInteger(targetTokens) || targetTokens < 1 || !Number.isInteger(contextPerRequestTokens) || contextPerRequestTokens < targetTokens) throw new Error(`invalid token budget in prompt fixture: ${promptFile}`);
  return { source: promptFile, targetTokens, contextPerRequestTokens, prompts };
}

function languagePrompts(count, offset = 0) {
  const languages = [
    'Rust', 'C', 'C++', 'C#', 'Java', 'Kotlin', 'Scala', 'Go', 'Python', 'Ruby', 'JavaScript', 'TypeScript', 'PHP', 'Swift', 'Objective-C', 'Dart', 'R', 'Julia', 'Haskell', 'OCaml', 'F#', 'Elixir', 'Erlang', 'Clojure', 'Common Lisp', 'Scheme', 'Prolog', 'Fortran', 'COBOL', 'Ada', 'Pascal', 'Delphi', 'Lua', 'Perl', 'Groovy', 'Visual Basic', 'Zig', 'Nim', 'V', 'Solidity', 'SQL', 'Bash', 'PowerShell', 'MATLAB', 'Assembly', 'Verilog', 'VHDL', 'Smalltalk', 'ABAP', 'Racket',
    '한국어', '영어', '일본어', '중국어', '스페인어', '프랑스어', '독일어', '이탈리아어', '포르투갈어', '러시아어', '아랍어', '힌디어', '벵골어', '우르두어', '인도네시아어', '베트남어', '태국어', '터키어', '페르시아어', '스와힐리어', '네덜란드어', '스웨덴어', '노르웨이어', '덴마크어', '핀란드어', '폴란드어', '체코어', '헝가리어', '루마니아어', '그리스어', '히브리어', '라틴어', '에스페란토', '타밀어', '텔루구어', '마라티어', '펀자브어', '말레이어', '필리핀어', '우크라이나어', '카탈루냐어', '웨일스어', '아일랜드어', '마오리어', '하와이어', '몽골어', '티베트어', '조지아어', '아르메니아어'
  ];
  const styles = ['핵심 특징을 한 문장으로 포함해.', '주요 사용처를 덧붙여.', '초보자 관점에서 써.', '역사적 배경은 짧게만 언급해.', '강점과 한계를 간단히 말해.', '대표적인 활용 분야를 포함해.', '다른 언어와의 차이는 생략해.', '친절한 한국어 문체로 답해.', '전문 용어는 필요할 때만 사용해.', '두 문장을 넘기지 마.'];
  return Array.from({ length: count }, (_, index) => {
    const position = index + offset;
    return `${languages[position % languages.length]} 언어를 한국어로 간단히 설명해. ${styles[Math.floor(position / languages.length) % styles.length]}`;
  });
}
