import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { validateBenchmarkSpec, verifyFiles, verifySourceEol } from './benchmark-spec.mjs';


const directory = path.dirname(fileURLToPath(import.meta.url));
const repository = path.resolve(directory, '../../../..');
const specPath = path.join(directory, 'benchmark-spec-qwen122b-h0-v8.json');
const read = () => JSON.parse(fs.readFileSync(specPath));
const hash = value => crypto.createHash('sha256').update(value).digest('hex');
const reseal = component => {
  component.version = hash(Buffer.from(component.files
    .map(item => `${item.path}\0${item.bytes}\0${item.sha256}\n`).join('')));
};


test('sealed H0 spec binds the real model, three hosts, bounded workloads, and LOAD authorization only', () => {
  const spec = read();
  assert.deepEqual(validateBenchmarkSpec(spec), {
    valid: true,
    h0_status: 'sealed',
    load_authorized: true,
    runtime_acceptance: false,
    hosts: 3,
    stages: 3,
    corpus_requests: 64,
  });
  assert.deepEqual(verifyFiles(spec, directory, repository), validateBenchmarkSpec(spec));
});


test('H0 rejects missing identity, old lifecycle, unsafe remote shell, unbounded resources, and weakened gates', () => {
  const original = read();
  const mutations = [
    spec => { spec.schema = 'p4.release-a.benchmark-spec.v1'; },
    spec => { spec.spec_id = 'qwen3_5_122b_a10b_h0_20260916_v3'; },
    spec => { spec.source.source_commit = '0'.repeat(40); },
    spec => { spec.source.source_bundle.sha256 = hash('other source bundle'); },
    spec => { spec.source.compatibility.patch_digest = hash('other compatibility patch'); },
    spec => { spec.source.runtime_kind = 'direct'; },
    spec => { spec.source.components[0].files[0].sha256 = hash('different scheduler'); },
    spec => {
      spec.source.components[0].files = spec.source.components[0].files
        .filter(item => item.path !== 'tools/event-drive/src/run/inference.rs');
      reseal(spec.source.components[0]);
    },
    spec => {
      spec.source.components[0].files = spec.source.components[0].files
        .filter(item => item.path !== 'tools/event-drive/src/run/source_grounded.rs');
      reseal(spec.source.components[0]);
    },
    spec => {
      spec.source.components[1].files = spec.source.components[1].files
        .filter(item => item.path !== 'test/benchmarks/cluster-inference/release-a/judge-h1-quality.py');
      reseal(spec.source.components[1]);
    },
    spec => {
      spec.source.components[1].files = spec.source.components[1].files
        .filter(item => item.path !== 'test/benchmarks/cluster-inference/release-a/judge-reference-capability.py');
      reseal(spec.source.components[1]);
    },
    spec => {
      spec.source.components[1].files = spec.source.components[1].files
        .filter(item => item.path !== '.gitattributes');
      reseal(spec.source.components[1]);
    },
    spec => {
      spec.source.components[1].files = spec.source.components[1].files
        .filter(item => item.path !== 'test/benchmarks/cluster-inference/release-a/judge-integrity-i0.py');
      reseal(spec.source.components[1]);
    },
    spec => {
      spec.source.components[1].files = spec.source.components[1].files
        .filter(item => item.path !== 'test/benchmarks/cluster-inference/release-a/build-integrity-i0-evidence.py');
      reseal(spec.source.components[1]);
    },
    spec => {
      spec.source.components[1].files = spec.source.components[1].files
        .filter(item => item.path !== 'test/benchmarks/cluster-inference/release-a/inspect-i0-active-host.py');
      reseal(spec.source.components[1]);
    },
    spec => { spec.lifecycle.load_content_type = 'application/vnd.p4.node.create-v1'; },
    spec => { spec.lifecycle.separate_create_delete_allowed = true; },
    spec => { spec.remote_execution[0].argv[1] = '$HOME/inspect.py'; },
    spec => { spec.remote_execution[0].argv.pop(); },
    spec => { delete spec.hosts[0].device.identifier; },
    spec => { spec.hosts[1].device.power_cap.reason = ''; },
    spec => { spec.hosts[2].links[0].speed_mbps = null; },
    spec => { spec.execution_layout.stages[1].layer_begin++; },
    spec => { spec.execution_layout.stages[0].pool.headroom_bytes++; },
    spec => { spec.execution_layout.stages[0].native_sha256 = hash('other native'); },
    spec => { spec.execution_layout.stages[0].agent_port = 42000; },
    spec => { spec.capacity.pending.count--; },
    spec => { spec.artifacts = spec.artifacts.filter(artifact => artifact.id !== 'h1_materializer'); },
    spec => { spec.artifacts = spec.artifacts.filter(artifact => artifact.id !== 'h1_judge'); },
    spec => { spec.artifacts = spec.artifacts.filter(artifact => artifact.id !== 'h0_verifier'); },
    spec => { spec.artifacts = spec.artifacts.filter(artifact => artifact.id !== 'spec_validator'); },
    spec => { spec.artifacts = spec.artifacts.filter(artifact => artifact.id !== 'event_preflight'); },
    spec => { spec.artifacts = spec.artifacts.filter(artifact => artifact.id !== 'integrity_spec'); },
    spec => { spec.artifacts = spec.artifacts.filter(artifact => artifact.id !== 'i0_materializer'); },
    spec => { spec.artifacts = spec.artifacts.filter(artifact => artifact.id !== 'i0_evidence_builder'); },
    spec => { spec.artifacts = spec.artifacts.filter(artifact => artifact.id !== 'i0_active_preflight'); },
    spec => { spec.artifacts = spec.artifacts.filter(artifact => artifact.id !== 'i0_host_observer'); },
    spec => { spec.artifacts = spec.artifacts.filter(artifact => artifact.id !== 'i0_route_inspector'); },
    spec => { spec.artifacts = spec.artifacts.filter(artifact => artifact.id !== 'i0_runner'); },
    spec => { spec.artifacts = spec.artifacts.filter(artifact => artifact.id !== 'i0_cleanup'); },
    spec => { spec.artifacts = spec.artifacts.filter(artifact => artifact.id !== 'i0_judge'); },
    spec => { spec.artifacts = spec.artifacts.filter(artifact => artifact.id !== 'reference_capability_judge'); },
    spec => { spec.artifacts = spec.artifacts.filter(artifact => artifact.id !== 'i1_runner'); },
    spec => { spec.artifacts = spec.artifacts.filter(artifact => artifact.id !== 'i1_evidence_builder'); },
    spec => { spec.artifacts = spec.artifacts.filter(artifact => artifact.id !== 'i1_judge'); },
    spec => { spec.artifacts = spec.artifacts.filter(artifact => artifact.id !== 'i1_path_tests'); },
    spec => { spec.integrity.execution_order = ['I0', 'P0']; },
    spec => { spec.integrity.integrity_baseline = true; },
    spec => { spec.integrity.performance_improvement_claimed = true; },
    spec => { spec.workload.modes.quality.max_in_flight = 8; },
    spec => { spec.workload.modes.quality.open_loop = true; },
    spec => { delete spec.workload.modes.quality.request_deadline_ms_by_class.long; },
    spec => { spec.workload.modes.quality.timeout_ms = 1800000; },
    spec => { delete spec.workload.modes.quality.pre_inference_hold_ms; },
    spec => { spec.workload.modes.quality.post_inference_hold_ms = 0; },
    spec => { delete spec.workload.modes.recovery; },
    spec => { spec.workload.modes.sustained.timeout_ms--; },
    spec => { spec.workload.modes.overload.rejected_min--; },
    spec => { spec.workload.modes.soak.minimum_duration_ms = 3599999; },
    spec => { spec.workload.fault_arms.pop(); },
    spec => { spec.ab.paired_repetitions = 7; spec.ab.paired_order.pop(); },
    spec => { spec.telemetry.sample_interval_ms = 0; },
    spec => { spec.slo.itl_ms = 251; },
    spec => { spec.execution_safety.local_desktop_model_run = true; },
    spec => { spec.execution_safety.remote_build_jobs_by_role.spark = 15; },
    spec => { spec.model.parameters.active_per_token++; },
    spec => { spec.model.shards[0].sha256 = '0'.repeat(64); },
    spec => { spec.runtime_acceptance = true; },
  ];
  for (const mutate of mutations) {
    const bad = structuredClone(original);
    mutate(bad);
    assert.throws(() => validateBenchmarkSpec(bad));
  }
});


test('file verification detects a valid-looking replacement before LOAD authorization', () => {
  const spec = read();
  assert.throws(() => verifySourceEol(spec, repository, item => `${item.path}: eol: unspecified`),
    /source component is not pinned to LF/);
  const inspector = spec.artifacts.find(artifact => artifact.id === 'gguf_inspector');
  inspector.sha256 = hash('valid-looking replacement');
  assert.doesNotThrow(() => validateBenchmarkSpec(spec));
  assert.throws(() => verifyFiles(spec, directory, repository), /artifact changed: gguf_inspector/);
});


test('CLI performs the same file-bound H0 gate', () => {
  const cli = fileURLToPath(new URL('./benchmark-spec.mjs', import.meta.url));
  const run = spawnSync(process.execPath, [cli, specPath], { encoding: 'utf8' });
  assert.equal(run.status, 0, run.stderr);
  assert.deepEqual(JSON.parse(run.stdout), validateBenchmarkSpec(read()));
});
