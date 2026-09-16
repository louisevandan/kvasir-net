import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { isDeepStrictEqual } from 'node:util';

const EXPECTED_COMMIT = '25edd33cf24b46964e432a7cd6d89772417673db';
const EXPECTED_SOURCE_BUNDLE = Object.freeze({
  bytes: 5126427,
  sha256: '4de5d0147fe3ac281ab735ad02f3fbfb46c5874a3b434e33237cdfdf1e314cc6',
});
const EXPECTED_COMPAT_PATCH = 'd8018fa8f7f44d61d23cd68496024fa296d2571c860fda988cef91a28b2572a9';
const EXPECTED_HOST_ROLES = Object.freeze(['spark', 'mac20', 'mac21']);
const EXPECTED_MODES = Object.freeze(['quality', 'cold', 'sustained', 'recovery', 'overload', 'soak']);
const EXPECTED_FAULTS = Object.freeze(['cancel', 'slow_edge', 'disconnected_edge', 'node_restart', 'late_return']);
const EXPECTED_ARRIVALS = Object.freeze([0, 180000, 480000, 780000, 1080000, 1380000, 1680000, 1980000]);
const H1_DEADLINES = Object.freeze({ short: 600000, medium: 1200000, long: 1800000 });
const H1_TIMEOUT_MS = 32 * H1_DEADLINES.short + 16 * H1_DEADLINES.medium +
  16 * H1_DEADLINES.long + 300000;
const META = /[$|&;<>()`"'\r\n]/;

const fail = (condition, message) => { if (!condition) throw new Error(message); };
const integer = (value, name, minimum = 0) =>
  fail(Number.isSafeInteger(value) && value >= minimum, `${name}: invalid integer`);
const finite = (value, name, minimum = 0) =>
  fail(Number.isFinite(value) && value >= minimum, `${name}: invalid number`);
const sha = value => typeof value === 'string' && /^[a-f0-9]{64}$/.test(value) && !/^([a-f0-9])\1+$/.test(value);
const commit = value => typeof value === 'string' && /^[a-f0-9]{40}$/.test(value);
const digest = bytes => crypto.createHash('sha256').update(bytes).digest('hex');

function recordDigest(records) {
  const payload = records.map(item => `${item.path}\0${item.bytes}\0${item.sha256}\n`).join('');
  return digest(Buffer.from(payload));
}

function validateFileRecord(item, name) {
  fail(item && typeof item.path === 'string' && item.path && !path.isAbsolute(item.path), `${name}: invalid path`);
  integer(item.bytes, `${name} bytes`, 1);
  fail(sha(item.sha256), `${name}: invalid SHA-256`);
}

function validatePowerCap(cap, name) {
  fail(cap && ['measured', 'unavailable'].includes(cap.status), `${name}: invalid power-cap status`);
  if (cap.status === 'measured') {
    fail(Array.isArray(cap.watts) && cap.watts.length > 0, `${name}: measured power cap lacks values`);
    cap.watts.forEach((value, index) => finite(value, `${name} watts ${index}`, 1));
  } else {
    fail(typeof cap.reason === 'string' && cap.reason.length >= 8, `${name}: unavailable power cap lacks reason`);
  }
}

function validateMode(mode, name) {
  integer(mode.requests, `${name} requests`, 1);
  integer(mode.timeout_ms, `${name} timeout`, 1);
  if (mode.waves) {
    fail(Array.isArray(mode.waves) && mode.waves.length > 0, `${name}: missing waves`);
    let prior = -1;
    let requests = 0;
    for (const [index, wave] of mode.waves.entries()) {
      integer(wave.after_ms, `${name} wave ${index} arrival`);
      integer(wave.count, `${name} wave ${index} count`, 1);
      fail(wave.after_ms >= prior, `${name}: arrivals are not monotonic`);
      prior = wave.after_ms;
      requests += wave.count;
    }
    fail(requests === mode.requests, `${name}: wave/request total differs`);
  }
}

export function validateBenchmarkSpec(spec) {
  fail(spec?.schema === 'p4.release-a.benchmark-spec.v2' && spec.h0_status === 'sealed', 'unsupported or unsealed H0 spec');
  fail(typeof spec.spec_id === 'string' && /^[a-z0-9_]+$/.test(spec.spec_id), 'invalid spec identity');

  const source = spec.source;
  fail(source?.source_commit === EXPECTED_COMMIT && commit(source.source_commit), 'runtime source commit differs');
  fail(source.runtime_kind === 'event', 'runtime must use the event path');
  fail(isDeepStrictEqual(source.source_bundle, EXPECTED_SOURCE_BUNDLE), 'source bundle binding differs');
  fail(source.compatibility?.pipeline === 'physical-wire-v4' && commit(source.compatibility.upstream_commit) &&
    source.compatibility.patch_digest === EXPECTED_COMPAT_PATCH, 'compatibility binding missing');
  fail(Array.isArray(source.components) && source.components.length === 3, 'source components missing');
  const components = new Map();
  for (const component of source.components) {
    fail(['scheduler', 'judge', 'summary'].includes(component.id) && !components.has(component.id), 'duplicate or unknown source component');
    fail(Array.isArray(component.files) && component.files.length > 0, `${component.id}: missing files`);
    component.files.forEach((item, index) => validateFileRecord(item, `${component.id} file ${index}`));
    fail(component.version === recordDigest(component.files), `${component.id}: version does not bind files`);
    components.set(component.id, component);
  }
  fail(components.size === 3, 'scheduler/judge/summary versions are incomplete');
  const componentPaths = id => new Set(components.get(id).files.map(item => item.path));
  for (const required of ['tools/event-drive/src/run/config.rs', 'tools/event-drive/src/run/inference.rs'])
    fail(componentPaths('scheduler').has(required), `scheduler omits H1 execution authority: ${required}`);
  for (const required of [
    'test/benchmarks/cluster-inference/release-a/prepare-h1-quality.py',
    'test/benchmarks/cluster-inference/release-a/judge-h1-quality.py',
  ]) fail(componentPaths('judge').has(required), `judge omits H1 sealed tool: ${required}`);

  const lifecycle = spec.lifecycle;
  fail(lifecycle?.schema === 1 && lifecycle.load_content_type === 'application/vnd.p4.node.load-v1' &&
    lifecycle.unload_content_type === 'application/vnd.p4.node.unload-v1' &&
    lifecycle.adapter_load_content_type === 'application/vnd.p4.llamacpp.load-v4+json' &&
    lifecycle.adapter_unload_content_type === 'application/vnd.p4.llamacpp.unload-v3+json' &&
    lifecycle.node_created_by_load === true && lifecycle.node_removed_by_unload === true &&
    lifecycle.separate_create_delete_allowed === false, 'lifecycle is not the sealed LOAD/UNLOAD contract');

  fail(Array.isArray(spec.artifacts) && spec.artifacts.length >= 13, 'local artifacts missing');
  const artifacts = new Map();
  for (const artifact of spec.artifacts) {
    fail(typeof artifact.id === 'string' && artifact.id && !artifacts.has(artifact.id), 'duplicate artifact id');
    validateFileRecord(artifact, `artifact ${artifact.id}`);
    artifacts.set(artifact.id, artifact);
  }
  for (const id of ['corpus', 'native_actual', 'native_actual_result', 'host_inspector', 'gguf_inspector',
    'h1_materializer', 'h1_judge', 'h0_verifier', 'spec_validator', 'spec_tests',
    'host_inspector_tests', 'event_preflight', 'event_preflight_tests'])
    fail(artifacts.has(id), `missing artifact ${id}`);
  fail(artifacts.get('event_preflight').path === '../../../../tools/validate_event_runtime_preflight.py' &&
    artifacts.get('event_preflight_tests').path === '../../../../tools/tests/test_validate_event_runtime_preflight.py',
  'event preflight artifact path differs');

  fail(Array.isArray(spec.remote_execution) && spec.remote_execution.length === 3, 'remote execution bindings missing');
  const remoteRoles = new Set();
  for (const remote of spec.remote_execution) {
    fail(EXPECTED_HOST_ROLES.includes(remote.role) && !remoteRoles.has(remote.role), 'duplicate remote role');
    remoteRoles.add(remote.role);
    fail(remote.runner === 'host_inspector' && remote.runner_sha256 === artifacts.get('host_inspector').sha256,
      'remote runner is not bound to the tracked inspector');
    fail(typeof remote.remote_path === 'string' && remote.remote_path.endsWith(`${remote.runner_sha256}.py`) &&
      !META.test(remote.remote_path), 'remote runner path is unsafe');
    fail(Array.isArray(remote.argv) && isDeepStrictEqual(remote.argv,
      ['python3', remote.remote_path, '--role', remote.role, '--output', `/tmp/p4-h0-v2-host-${remote.role}.json`]),
    'remote invocation is not argv-only');
    fail(remote.argv.every(value => typeof value === 'string' && value && !META.test(value)), 'remote argv has shell metacharacters');
    fail(remote.local_remote_hash_equal === true, 'remote runner hash was not matched');
  }

  const model = spec.model;
  fail(model?.id === 'qwen3_5_122b_a10b_ud_q5_k_s_v1' && model.architecture === 'qwen35moe' &&
    model.variant === 'UD-Q5_K_S', 'wrong Release A model');
  fail(model.quantization?.label === 'UD-Q5_K_S' && model.quantization.gguf_version === 3 &&
    model.quantization.quantization_version === 2 && model.quantization.file_type === 16, 'quantization differs');
  const parameters = model.parameters;
  for (const key of ['total', 'expert', 'nonexpert', 'active_per_token', 'experts', 'experts_active'])
    integer(parameters?.[key], `model parameter ${key}`, 1);
  fail(parameters.total === parameters.expert + parameters.nonexpert &&
    parameters.active_per_token === parameters.nonexpert + parameters.expert * parameters.experts_active / parameters.experts &&
    parameters.total === 124635206144 && parameters.active_per_token === 9954546176,
  'total/active parameter accounting differs');
  fail(model.layers?.gguf_blocks === 49 && model.layers.trunk === 48 && model.layers.mtp === 1 &&
    model.layers.mtp_execution === 'disabled', 'layer/MTP contract differs');
  fail(model.context?.model_max_tokens === 262144 && model.context.per_sequence_tokens === 102400 &&
    model.context.resident === 8 && model.context.total_tokens === 819200 &&
    model.context.per_sequence_tokens * model.context.resident === model.context.total_tokens, 'context contract differs');
  fail(isDeepStrictEqual(model.kv, { k: 'f16', v: 'f16', unified: true, flash_attention: true }), 'KV contract differs');
  fail(sha(model.chat_template_sha256), 'chat template hash missing');
  fail(model.model_manifest?.kind === 'inline_h0_binding' && model.model_manifest.inspector === 'gguf_inspector' &&
    model.model_manifest.tensor_count === 899, 'model metadata is not bound');
  validateFileRecord(model.model_manifest.snapshot, 'model metadata snapshot');
  fail(model.model_manifest.snapshot.path.startsWith('target/release-a-h0-'), 'model snapshot must remain ignored run evidence');
  fail(Array.isArray(model.shards) && model.shards.length === 3, 'model needs three shards');
  let shardBytes = 0;
  const shardHashes = new Set();
  for (const shard of model.shards) {
    integer(shard.bytes, 'model shard bytes', 1); shardBytes += shard.bytes;
    fail(sha(shard.sha256) && !shardHashes.has(shard.sha256), 'invalid or duplicate model shard');
    shardHashes.add(shard.sha256);
    fail(['windows', 'spark', 'mac'].every(key => typeof shard.locations?.[key] === 'string' && shard.locations[key]),
      'model shard locations missing');
  }
  fail(shardBytes === 88310156320, 'model artifact size differs');
  fail(sha(model.tokenizer?.sha256) && commit(model.tokenizer.source_commit) && model.tokenizer.add_special === true &&
    model.tokenizer.parse_special === true, 'tokenizer binding differs');
  const sampling = model.sampling;
  fail(isDeepStrictEqual(sampling, { temperature: 0, top_p: 1, top_k: 0, min_p: 0, repeat_penalty: 1,
    seed: 20260916, max_output_tokens: 2048, speculative: 'none' }), 'sampling/seed contract differs');
  fail(model.distribution_reason?.coverage_goal === 'multi_host_final' &&
    model.distribution_reason.split_required_for_approved_layout === true &&
    typeof model.distribution_reason.reason === 'string' && model.distribution_reason.reason.length > 40,
  'multi-host distribution reason missing');

  fail(isDeepStrictEqual(spec.coverage, { resource_tier: 'vram_only', coverage: 'multi_host_final', physical_host_count: 3,
    unified_memory_devices: ['spark', 'mac20', 'mac21'], cpu_offload_of_owned_compute: false }), 'coverage/resource tier differs');
  fail(Array.isArray(spec.hosts) && spec.hosts.length === 3, 'three physical hosts are required');
  const hosts = new Map();
  for (const host of spec.hosts) {
    fail(EXPECTED_HOST_ROLES.includes(host.role) && !hosts.has(host.role), 'duplicate or unknown host role');
    fail(sha(host.identity) && typeof host.address === 'string' && typeof host.hostname === 'string', 'host identity missing');
    validateFileRecord(host.inventory_snapshot, `${host.role} inventory snapshot`);
    fail(host.inventory_snapshot.path.startsWith('target/release-a-h0-'), 'host snapshot must remain ignored run evidence');
    integer(host.captured_unix_ms, 'host capture time', 1);
    integer(host.logical_cpu_count, 'host logical CPUs', 1);
    integer(host.physical_memory_bytes, 'host physical memory', 1);
    integer(host.disk_free_bytes, 'host disk free', 1);
    integer(host.dynamic_port_range?.first, 'dynamic port first', 1);
    integer(host.dynamic_port_range?.last, 'dynamic port last', host.dynamic_port_range.first);
    fail(host.dynamic_port_range.last <= 65535, 'dynamic port range exceeds port space');
    fail(['CUDA', 'Metal'].includes(host.device?.backend) && typeof host.device.plugin === 'string' &&
      typeof host.device.driver_or_os_build === 'string', 'host backend/driver binding missing');
    if (host.device.backend === 'CUDA') {
      fail(host.device.identifier?.kind === 'uuid' && /^GPU-[0-9a-f-]+$/i.test(host.device.identifier.value), 'CUDA UUID missing');
    } else {
      fail(host.device.identifier?.kind === 'metal_registry_id' && /^\d+$/.test(host.device.identifier.value) &&
        typeof host.device.identifier.uuid_unavailable_reason === 'string', 'Metal registry identity missing');
    }
    validatePowerCap(host.device.power_cap, `${host.role} power cap`);
    for (const binary of ['agent', 'native']) {
      integer(host.binaries?.[binary]?.bytes, `${host.role} ${binary} bytes`, 1);
      fail(sha(host.binaries?.[binary]?.sha256) && typeof host.binaries[binary].path === 'string', `${host.role} ${binary} binding missing`);
    }
    if (host.role === 'spark') fail(sha(host.binaries.event_drive?.sha256), 'Spark event-drive binding missing');
    integer(host.runtime_libraries?.count, `${host.role} library count`, 1);
    fail(sha(host.runtime_libraries?.digest) && typeof host.runtime_libraries.binding === 'string' &&
      Array.isArray(host.runtime_libraries.files) && host.runtime_libraries.files.length === host.runtime_libraries.count,
    'library digest/files missing');
    for (const [index, library] of host.runtime_libraries.files.entries()) {
      fail(typeof library.path === 'string' && library.path, `${host.role} library ${index}: path missing`);
      integer(library.bytes, `${host.role} library ${index} bytes`, 1);
      fail(sha(library.sha256), `${host.role} library ${index}: hash missing`);
    }
    fail(recordDigest(host.runtime_libraries.files) === host.runtime_libraries.digest, `${host.role}: library digest differs`);
    fail(Array.isArray(host.links) && host.links.length === 2, `${host.role}: two peer links required`);
    const peers = new Set();
    for (const link of host.links) {
      fail(EXPECTED_HOST_ROLES.includes(link.peer_role) && link.peer_role !== host.role && !peers.has(link.peer_role), 'invalid peer link');
      peers.add(link.peer_role);
      integer(link.speed_mbps, 'link speed', 1);
      finite(link.min_ms, 'link min latency'); finite(link.avg_ms, 'link average latency');
      finite(link.max_ms, 'link maximum latency'); finite(link.stddev_ms, 'link latency deviation');
      finite(link.packet_loss_percent, 'link packet loss'); integer(link.samples, 'link samples', 5);
      fail(link.packet_loss_percent === 0 && link.min_ms <= link.avg_ms && link.avg_ms <= link.max_ms, 'invalid link measurement');
    }
    fail(host.telemetry?.primary && typeof host.telemetry.available === 'boolean', `${host.role}: telemetry capability missing`);
    if (!host.telemetry.available) fail(host.telemetry.fallback_available === true && host.telemetry.unavailable_reason,
      `${host.role}: unavailable primary telemetry lacks fallback/reason`);
    fail(host.protected_agent?.port === 52005 && host.protected_agent.present === true &&
      host.protected_agent.must_not_terminate === true, 'protected existing agent contract differs');
    fail(isDeepStrictEqual(host.preflight_state, { task_native_workers: 0, hf_workers: 0, protected_agent_present: true }),
      `${host.role}: pre-H0 process state differs`);
    hosts.set(host.role, host);
  }
  fail(hosts.size === 3 && new Set([...hosts.values()].map(host => host.identity)).size === 3, 'physical host identities are not unique');

  const layout = spec.execution_layout;
  fail(layout?.artifact === 'native_actual' && layout.allocation_conformance === 'native_actual_result' &&
    isDeepStrictEqual(layout.cuts, [0, 24, 36, 48]), 'execution layout binding differs');
  fail(isDeepStrictEqual(layout.shape, { n_batch: 128, n_ubatch: 64, resident: 8, context_per_sequence: 102400,
    total_context: 819200, sequence_capacity: 8, kv_unified: true, k_type: 'f16', v_type: 'f16',
    flash_attention: true, prefill_fragments: 1, speculative: 'none' }), 'execution shape differs');
  fail(Array.isArray(layout.stages) && layout.stages.length === 3, 'three stages are required');
  let priorLayer = 0;
  const stageRoles = new Set();
  for (const stage of layout.stages) {
    fail(hosts.has(stage.role) && stage.host === hosts.get(stage.role).identity && !stageRoles.has(stage.role), 'stage/host binding differs');
    stageRoles.add(stage.role);
    fail(stage.layer_begin === priorLayer && stage.kv_layer_begin === stage.layer_begin && stage.kv_layer_end === stage.layer_end,
      'stage cut is discontinuous');
    integer(stage.layer_end, 'stage layer end', stage.layer_begin + 1); priorLayer = stage.layer_end;
    fail(stage.owned_layer_compute === 'device' && stage.owned_layer_weights === 'device' &&
      stage.nonowned_cpu_tensors_computed === false && stage.mtp_layer_48_enabled === false, 'stage placement is not device-resident trunk execution');
    fail(Array.isArray(stage.allocation_entries) && stage.allocation_entries.length >= 1, 'stage allocations missing');
    let required = 0;
    for (const entry of stage.allocation_entries) {
      for (const key of ['model_bytes', 'kv_bytes', 'compute_bytes', 'required_bytes']) integer(entry[key], `stage ${stage.id} ${key}`);
      fail(entry.required_bytes === entry.model_bytes + entry.kv_bytes + entry.compute_bytes, 'allocation component sum differs');
      required += entry.required_bytes;
    }
    const pool = stage.pool;
    for (const key of ['available_bytes', 'reserve_bytes', 'usable_bytes', 'required_bytes', 'headroom_bytes']) integer(pool?.[key], `${stage.id} pool ${key}`);
    fail(pool.usable_bytes === pool.available_bytes - pool.reserve_bytes && pool.required_bytes === required &&
      pool.headroom_bytes === pool.usable_bytes - pool.required_bytes && pool.headroom_bytes > 0,
    'stage pool arithmetic differs');
    fail(stage.actual_allocation_conformant === true && stage.agent_sha256 === hosts.get(stage.role).binaries.agent.sha256 &&
      stage.native_sha256 === hosts.get(stage.role).binaries.native.sha256, 'stage binary/allocation binding differs');
    for (const [label, port] of [['agent', stage.agent_port], ['native', stage.native_port]]) {
      integer(port, `${stage.id} ${label} port`, 1);
      const range = hosts.get(stage.role).dynamic_port_range;
      fail(port < range.first || port > range.last, `${stage.id} ${label} port overlaps dynamic range`);
    }
    const bounds = stage.result_bounds;
    integer(bounds?.max_rows, 'result row bound', 1);
    integer(bounds?.max_physical_result_bytes, 'physical result bound', 1);
    fail(bounds.max_rows === 128 && bounds.max_completion_payload_bytes === bounds.max_physical_result_bytes,
      'stage result/edge bounds differ');
  }
  fail(priorLayer === 48 && stageRoles.size === 3, 'stage cuts do not cover the trunk');

  const capacity = spec.capacity;
  fail(capacity?.resident === 8 && capacity.pending.count === 64 && capacity.pending.bytes === 64 * 2097152 &&
    capacity.pending.input_tokens === 64 * 102400, 'pending budget differs');
  fail(isDeepStrictEqual(capacity.request, { max_bytes: 2097152, max_input_tokens: 102400, max_output_tokens: 2048 }),
    'per-request budget differs');
  fail(capacity.admission.max_requests === 72 && capacity.admission.retained_request_bytes === 72 * 2097152 &&
    capacity.admission.input_tokens === 72 * 102400 && capacity.admission.output_tokens === 72 * 2048,
  'aggregate admission budget differs');
  fail(isDeepStrictEqual(capacity.stores, { completion: { count: 65536, bytes: 268435456 },
    edge: { count: 65536, bytes: 268435456 }, receipt: { count: 65536, bytes: 67108864 },
    hop_outstanding: 256 }), 'retained store bounds differ');
  for (const stage of layout.stages) {
    fail(stage.result_bounds.max_physical_result_bytes <= capacity.stores.completion.bytes &&
      stage.result_bounds.max_physical_result_bytes <= capacity.stores.edge.bytes &&
      stage.result_bounds.max_simultaneous_retained_groups ===
        Math.floor(capacity.stores.completion.bytes / stage.result_bounds.max_physical_result_bytes),
    'stage result bound exceeds or misstates retained capacity');
  }

  const workload = spec.workload;
  fail(workload?.corpus === 'corpus' && workload.workload_digest === artifacts.get('corpus').sha256 &&
    workload.corpus_requests === 64 && isDeepStrictEqual(workload.wave_class_order,
      ['short', 'short', 'short', 'short', 'medium', 'medium', 'long', 'long']), 'corpus/workload binding differs');
  fail(isDeepStrictEqual(workload.class_tokens, { short: { min: 2000, max: 8000 }, medium: { exact: 32000 },
    long: { exact: 100038 } }), 'class token bounds differ');
  fail(workload.modes && isDeepStrictEqual(Object.keys(workload.modes), EXPECTED_MODES), 'workload modes missing or reordered');
  for (const name of EXPECTED_MODES) validateMode(workload.modes[name], name);
  const quality = workload.modes.quality;
  fail(quality.requests === 64 && isDeepStrictEqual(quality.waves, [{ after_ms: 0, count: 64 }]) &&
    quality.submission === 'release_closed_loop' && quality.max_in_flight === 1 && quality.open_loop === false &&
    isDeepStrictEqual(quality.request_deadline_ms_by_class, H1_DEADLINES) && quality.overall_grace_ms === 300000 &&
    quality.timeout_ms === H1_TIMEOUT_MS && quality.normal === true, 'H1 quality authority is not sealed closed-loop execution');
  fail(workload.modes.cold.requests === 8 &&
    isDeepStrictEqual(workload.modes.sustained.waves.map(wave => wave.after_ms), EXPECTED_ARRIVALS) &&
    workload.modes.sustained.waves.every(wave => wave.count === 8) && workload.modes.sustained.open_loop === true &&
    workload.modes.sustained.timeout_ms >= EXPECTED_ARRIVALS.at(-1) + 1800000, 'normal wave contract differs');
  const recovery = workload.modes.recovery;
  fail(recovery.blocks >= 3 && recovery.requests === recovery.blocks * recovery.requests_per_block && recovery.same_load === true &&
    recovery.timeout_ms >= recovery.blocks * recovery.block_timeout_ms + (recovery.blocks - 1) * recovery.quiescence_ms,
  'recovery waves are not bounded under one load');
  const overload = workload.modes.overload;
  fail(overload.requests >= 2 * capacity.resident && overload.accepted_max === capacity.admission.max_requests &&
    overload.rejected_min === overload.requests - overload.accepted_max && overload.submit_window_ms <= 1000 &&
    overload.rejection_deadline_ms <= 5000, 'overload contract differs');
  const soak = workload.modes.soak;
  fail(soak.requests >= 32 * capacity.resident && soak.minimum_duration_ms >= 3600000 && soak.same_load === true,
    'soak does not meet H7 duration/request minimum');
  fail(Array.isArray(workload.fault_arms) && workload.fault_arms.map(arm => arm.id).join(',') === EXPECTED_FAULTS.join(','),
    'fault arms missing or reordered');
  workload.fault_arms.forEach(arm => { integer(arm.requests, `${arm.id} requests`, 1); integer(arm.timeout_ms, `${arm.id} timeout`, 1); });

  fail(isDeepStrictEqual(spec.slo, { ttft_ms: { short: 60000, medium: 300000, long: 900000 }, itl_ms: 250,
    send_slip_ms: 1000, clock_skew_ms: 10, normal_response_pass_rate: 1, request_terminal_pass_rate: 1 }),
  'SLO contract differs');
  const ab = spec.ab;
  fail(ab?.one_policy_variable === 'pipeline_mixed_batching_v1' && ab.paired_repetitions >= 8 && ab.holdout_pairs >= 4 &&
    ab.paired_order.length === ab.paired_repetitions && ab.holdout_order.length === ab.holdout_pairs,
  'A/B repetitions are insufficient');
  for (const order of [ab.paired_order, ab.holdout_order]) {
    fail(order.every(value => ['AB', 'BA'].includes(value)) &&
      Math.abs(order.filter(value => value === 'AB').length - order.filter(value => value === 'BA').length) <= 1,
    'A/B order is not balanced');
  }
  integer(ab.holdout_seed, 'holdout seed', 1);
  fail(isDeepStrictEqual(ab.arms?.A?.environment, {}) && isDeepStrictEqual(ab.arms?.B?.environment, {
    P4_STAGED_PIPELINE_BATCHING: '1', P4_STAGED_MIXED_BATCH_ROWS: '128', P4_STAGED_MIXED_PREFILL_ROWS: '128',
  }), 'A/B arms change the wrong policy');
  fail(ab.promotion?.median_useful_tps_improvement_percent === 5 && ab.promotion.ci95_lower_bound_gt === 0 &&
    ab.promotion.holdout_direction_positive === true && ab.promotion.ttft_p95_ratio_max === 1.10 &&
    ab.promotion.itl_p95_ratio_max === 1.05, 'A/B promotion threshold differs');

  const telemetry = spec.telemetry;
  fail(telemetry?.sample_interval_ms === 1000 && telemetry.warmup_ms === 60000 && telemetry.minimum_coverage === 0.95 &&
    telemetry.retain_raw_samples === true && telemetry.unsupported_metrics_are_null_with_reason === true &&
    Array.isArray(telemetry.required) && telemetry.required.length >= 10, 'telemetry contract differs');
  const safety = spec.execution_safety;
  fail(safety?.local_desktop_build === false && safety.local_desktop_model_run === false &&
    safety.remote_build_max_jobs > 0 && safety.remote_build_max_jobs <= 8 && safety.load_requires_validated_h0 === true &&
    safety.inspect_and_unload_task_owned_nodes_before_run === true && safety.protected_agents_must_remain === true &&
    safety.assigned_agent_port === 22150 && isDeepStrictEqual(safety.assigned_native_ports, [23150, 23151, 23152]),
  'execution safety contract differs');
  fail(spec.evidence_bundle?.required_files?.length === 9 &&
    spec.evidence_bundle.failure_file_required_on_failure === 'failure.json', 'evidence bundle contract differs');
  fail(spec.runtime_acceptance === false, 'H0 cannot claim runtime acceptance');
  return { valid: true, h0_status: 'sealed', load_authorized: true, runtime_acceptance: false,
    hosts: 3, stages: 3, corpus_requests: 64 };
}

export function verifyFiles(spec, specDirectory, repositoryRoot) {
  const result = validateBenchmarkSpec(spec);
  const artifacts = new Map(spec.artifacts.map(item => [item.id, item]));
  for (const item of spec.artifacts) {
    const bytes = fs.readFileSync(path.resolve(specDirectory, item.path));
    fail(bytes.length === item.bytes && digest(bytes) === item.sha256, `artifact changed: ${item.id}`);
  }
  for (const component of spec.source.components) {
    for (const item of component.files) {
      const bytes = fs.readFileSync(path.resolve(repositoryRoot, item.path));
      fail(bytes.length === item.bytes && digest(bytes) === item.sha256, `source component changed: ${item.path}`);
    }
  }
  const corpus = JSON.parse(fs.readFileSync(path.resolve(specDirectory, artifacts.get('corpus').path)));
  fail(corpus.schema === 'p4.release-a.corpus.v1' && corpus.target_id === spec.model.id &&
    corpus.requests?.length === spec.workload.corpus_requests && corpus.tokenizer?.sha256 === spec.model.tokenizer.sha256,
  'corpus identity differs');
  for (let index = 0; index < corpus.requests.length; index += 8) {
    fail(isDeepStrictEqual(corpus.requests.slice(index, index + 8).map(request => request.class), spec.workload.wave_class_order),
      `corpus wave composition differs at ${index / 8}`);
  }
  const actual = JSON.parse(fs.readFileSync(path.resolve(specDirectory, artifacts.get('native_actual').path)));
  const allocation = JSON.parse(fs.readFileSync(path.resolve(specDirectory, artifacts.get('native_actual_result').path)));
  fail(isDeepStrictEqual(actual.stages.map(stage => [stage.layerBegin, stage.layerEnd]), [[0, 24], [24, 36], [36, 48]]) &&
    allocation.actualAllocationConformant === true && allocation.loadAuthorized === true, 'native layout evidence differs');
  return result;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [input] = process.argv.slice(2);
  fail(input, 'usage: node benchmark-spec.mjs benchmark-spec.json');
  const absolute = path.resolve(input);
  const specDirectory = path.dirname(absolute);
  const repositoryRoot = path.resolve(specDirectory, '../../../..');
  const spec = JSON.parse(fs.readFileSync(absolute));
  console.log(JSON.stringify(verifyFiles(spec, specDirectory, repositoryRoot)));
}
