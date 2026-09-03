#!/usr/bin/env node
// Four-node acceptance run.
//
//   node test/benchmarks/p4-4node/run.mjs <scenario> [--out DIR]
//
// Starts one agent, samples every NVIDIA device, drives the scenario through
// p4-event-drive, then judges whether the answers mean anything. Structural
// success (40/40 delivered) is not acceptance; judge.mjs decides.

import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { judgeArtifact } from "./judge.mjs";
import {
  agreesWithExpected,
  beginRun,
  collectEvidence,
  currentCompatManifest,
  defaultRunsDir,
  preserveWorkingTree,
  newRunId,
  promoteRun,
} from "./evidence.mjs";
import { fencedRecords } from "./fence.mjs";
import { checkDelivery, recordChannelFailures } from "./delivery.mjs";
import { checkSessionKeys } from "./session-key.mjs";
import {
  fetchRemoteAgentLog,
  fetchRemoteRecord,
  remoteRecordLength,
  remoteRecordFailure,
  clearRemoteRecordFailure,
  openTunnel,
  proveTunnelIdentity,
  remoteImageDigests,
  remoteLauncher,
  remoteAgentLogLength,
  startRemoteGpuSampler,
} from "./remote.mjs";
import { scenario } from "./scenarios.mjs";
import { writeConfig } from "./spec.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");

function collect(child) {
  const state = { stdout: "", stderr: "" };
  child.stdout?.on("data", (chunk) => { state.stdout += chunk.toString("utf8"); });
  child.stderr?.on("data", (chunk) => { state.stderr += chunk.toString("utf8"); });
  return state;
}

const waitForExit = (child) => new Promise((resolve, reject) => {
  child.once("error", reject);
  child.once("exit", (code, signal) => resolve({ code, signal }));
});

async function waitForReady(child, state, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (state.stdout.includes("P4_EVENT_AGENT_READY")) return;
    if (child.exitCode !== null) {
      throw new Error(`agent exited before READY: ${state.stderr || state.stdout}`);
    }
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  throw new Error(`agent READY timeout: ${state.stderr || state.stdout}`);
}

/// Stops a child and says whether it actually stopped.
///
/// The timeout used to be silent, so a process still running after three
/// seconds looked the same as one that had exited - and the agent's record
/// is read straight after this returns. A record read from a process that
/// is still writing is the race the closing offset exists to close.
async function stopChild(child) {
  if (!child) return true;
  if (child.exitCode !== null) return true;
  child.kill();
  const exited = await Promise.race([
    waitForExit(child).then(() => true).catch(() => true),
    new Promise((resolve) => setTimeout(() => resolve(false), 5_000)),
  ]);
  return exited && child.exitCode !== null;
}

/// The share of the UBATCH a batch actually carried.
///
/// `rows_per_batch` alone cannot say whether the pipeline is full, because a
/// wide batch on a wide UBATCH is still mostly empty. The plan's own width is
/// the denominator, so this is the number the batching work is trying to move.
function fill(rows, ubatch) {
  if (!ubatch || rows.length === 0) return null;
  const total = rows.reduce((sum, value) => sum + value, 0);
  return Number(((100 * total) / rows.length / ubatch).toFixed(2));
}

/// Percentiles over a sorted copy, so a single slow batch cannot pass for the
/// common case and a common case cannot hide a stall.
function spread(values) {
  if (values.length === 0) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const at = (fraction) => sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * fraction))];
  const total = sorted.reduce((sum, value) => sum + value, 0);
  return {
    mean: Number((total / sorted.length).toFixed(1)),
    p50: at(0.5),
    p90: at(0.9),
    max: sorted[sorted.length - 1],
  };
}

function metrics(artifact, ubatch) {
  const rows = [];
  let batches = 0;
  let mixed = 0;
  const stage = [];
  const idle = [];
  const gated = [];
  const readyGap = [];
  const readySequences = [];
  for (const observation of artifact.batch_observations ?? []) {
    stage.push(observation.stage_ms ?? 0);
    idle.push(observation.idle_ms ?? 0);
    gated.push(observation.idle_gated ?? 0);
    readyGap.push((observation.ready_rows ?? 0) - (observation.logical_rows ?? 0));
    readySequences.push(observation.ready_sequences ?? 0);
    for (const batch of observation.physical_batches ?? []) {
      batches += 1;
      rows.push(batch.rows);
      if (batch.prefill_rows > 0 && batch.decode_rows > 0) mixed += 1;
    }
  }
  const seconds = (artifact.elapsed_ms ?? 0) / 1000;
  const decode = artifact.requests.reduce((sum, r) => sum + (r.decode_rows ?? 0), 0);
  const prefill = artifact.requests.reduce((sum, r) => sum + (r.prefill_rows ?? 0), 0);
  const totalRows = rows.reduce((sum, value) => sum + value, 0);
  return {
    wall_s: Number(seconds.toFixed(2)),
    prefill_rows: prefill,
    decode_rows: decode,
    generation_tps: seconds > 0 ? Number((decode / seconds).toFixed(2)) : null,
    total_tps: seconds > 0 ? Number(((decode + prefill) / seconds).toFixed(2)) : null,
    physical_batches: batches,
    rows_per_batch: batches > 0 ? Number((totalRows / batches).toFixed(2)) : null,
    ms_per_batch: batches > 0 ? Number(((seconds * 1000) / batches).toFixed(1)) : null,
    mixed_batches: mixed,
    ubatch_fill_pct: fill(rows, ubatch),
    // The first node: how long its own stage held each batch, how long it
    // stood still before planning the next, and whether the coalescing
    // threshold or an empty ready set is what it was standing still for.
    first_node_stage_ms: spread(stage),
    first_node_idle_ms: spread(idle),
    first_node_gate_refusals: gated.reduce((sum, value) => sum + value, 0),
    // Token rows the ready set held minus the rows the batch carried: a
    // positive number is the scheduler leaving work on the floor. An earlier
    // version of this subtracted from the ready *request* count and read the
    // never-positive result as a clean sweep; it proved nothing.
    ready_rows_left: spread(readyGap),
    ready_sequences: spread(readySequences),
  };
}

/// The local agent's record file, in the shape the remote read returns.
///
/// A local run writes its own file for the reason a remote one does - the
/// stage servers' stderr is inherited here too - so it is read from disk
/// rather than scraped out of that shared stream. The file is created fresh
/// per run directory, so the whole of it belongs to this run and there is no
/// range to cut.
/// Whether the local agent marked its record channel as failed.
///
/// The same independent path the remote run reads, for the same reason: the
/// announcement must not travel on the stderr the stage servers share.
function readLocalRecordFailure(file) {
  const marker = `${file}.failed`;
  if (!fs.existsSync(marker)) return null;
  return `P4_RECORD_FAILED ${fs.readFileSync(marker, "utf8").trim()}`;
}

function readLocalRecord(file) {
  if (!fs.existsSync(file)) return { text: "", endsOnRecord: true };
  const text = fs.readFileSync(file, "utf8");
  return { text, endsOnRecord: text === "" || text.endsWith("\n") };
}

async function main() {
  const [name, ...rest] = process.argv.slice(2);
  if (!name) throw new Error("usage: run.mjs <scenario> [--target local|remote] [--out DIR]");
  const outIndex = rest.indexOf("--out");
  const targetIndex = rest.indexOf("--target");
  const target = targetIndex >= 0 ? rest[targetIndex + 1] : "local";
  const spec = scenario(name, target);
  const runId = newRunId();
  // --out chooses where the run's directory lives, not whether it gets one.
  // Pointing it at a bare path used to make working and final the same
  // directory, which skipped both the refusal to reuse a directory and the
  // MANIFEST that makes the result checkable - so the runs most likely to be
  // pointed somewhere specific were the ones with no evidence contract.
  const run = outIndex >= 0
    ? beginRun(path.resolve(rest[outIndex + 1]), runId)
    : beginRun(defaultRunsDir(root), runId);
  const outDir = run.working;
  fs.mkdirSync(outDir, { recursive: true });

  const { file: configPath } = writeConfig(spec, outDir, { run_id: runId });
  const artifactPath = path.join(outDir, "artifact.json");
  const ingress = new URL(spec.ingress);

  // A remote target already has its agent running as an interactive scheduled
  // task (remote-agent.mjs), because only a process owned by the logged-on
  // user sees the mapped drive the model lives on.
  const agent = spec.target === "remote" ? null
    : spawn(path.join(root, "target", "release", "p4-agent.exe"),
      [`${ingress.hostname}:${ingress.port}`], {
        cwd: root,
        windowsHide: true,
        stdio: ["ignore", "pipe", "pipe"],
        env: {
          ...process.env,
          P4_AGENT_STATS: "1",
          P4_STAGED_LLAMA_INHERIT_STDERR: "1",
          P4_STAGED_TRACE_SESSION_KEY: "1",
          // Its own file, for the reason the remote agent has one: the
          // stage servers' stderr is inherited here too, and a record
          // read back from a shared channel has already been torn once.
          P4_RECORD_FILE: path.join(outDir, "agent.record.log"),
        },
      });
  const agentOutput = agent ? collect(agent) : { stdout: "", stderr: "" };
  let tunnel = null;
  let identity = null;
  let evidence = null;
  let sampler;
  let samplerOutput = { stdout: "" };
  let driveOutput = { stdout: "", stderr: "" };
  let agentLog = "";
  let record = { text: "", endsOnRecord: true };
  let recordFrom = 0;
  let recordTo = 0;
  let channelFailure = null;
  let agentStopped = true;
  let agentLogFrom = 0;
  let failure;

  try {
    if (agent) await waitForReady(agent, agentOutput, 20_000);
    evidence = collectEvidence({
      root,
      runId,
      spec,
      compatManifest: currentCompatManifest(root),
    });
    // A hash says a diff existed; the diff says which one.
    evidence.dirty_evidence = preserveWorkingTree(evidence.working_tree, outDir);
    if (spec.tunnel) {
      // Opening the forward is not proof it is ours: a bind failure leaves a
      // previous forward holding the port and the run would still look
      // remote. The identity probe makes the far side confirm it.
      ({ child: tunnel } = await openTunnel(spec.tunnel));
      identity = proveTunnelIdentity(spec.tunnel.host, spec.tunnel.remotePort);
      // Marks where this run's share of the appended agent log starts.
      agentLogFrom = remoteAgentLogLength(spec.tunnel);
      // Marks this run in the record file instead of computing an offset:
      // an offset still returns the whole file when the run wrote nothing.
      // This run's channel status is its own, not the last run's.
      clearRemoteRecordFailure(spec.tunnel);
      recordFrom = remoteRecordLength(spec.tunnel);
      // Hashed on the far side, so the evidence names what ran rather than
      // what this machine happens to have built, and carries the launcher
      // the agent's policy knobs live in.
      evidence.remote = {
        host: spec.tunnel.host,
        ...identity,
        images: remoteImageDigests(spec.tunnel),
        launcher: remoteLauncher(spec.tunnel),
      };
    }
    sampler = spec.target === "remote"
      ? startRemoteGpuSampler(spec.tunnel.host)
      : spawn("nvidia-smi", [
      "--query-gpu=timestamp,index,name,utilization.gpu,memory.used,power.draw",
      "--format=csv,noheader,nounits", "-lms", "250",
    ], { windowsHide: true, stdio: ["ignore", "pipe", "pipe"] });
    if (sampler && spec.target !== "remote") samplerOutput = collect(sampler);

    const drive = spawn(path.join(root, "target", "release", "p4-event-drive.exe"),
      [configPath, artifactPath], { cwd: root, windowsHide: true, stdio: ["ignore", "pipe", "pipe"] });
    driveOutput = collect(drive);
    const result = await waitForExit(drive);
    if (result.code !== 0) failure = `drive exited ${result.code}`;
  } catch (error) {
    failure = error.message;
  } finally {
    if (spec.target === "remote" && sampler) samplerOutput = { stdout: await sampler.stop() };
    else await stopChild(sampler);
    fs.writeFileSync(path.join(outDir, "gpu.csv"), samplerOutput.stdout, "utf8");
    // A remote run has no local agent, so its agent log has to be pulled from
    // the far side to land in the same file a local run writes.
    // The agent is stopped before its record is read, so what is read is
    // everything it was going to write. Reading first and stopping after
    // leaves a race between the last record and the closing length.
    agentStopped = await stopChild(agent);
    agentLog = spec.target === "remote" && spec.tunnel
      ? fetchRemoteAgentLog({ ...spec.tunnel, fromByte: agentLogFrom })
      : agentOutput.stderr;
    // Evidence comes from the record file, which has one writer. The agent
    // log is kept for what it is good for - reading what happened - and is
    // not what the verdict rests on.
    if (spec.target === "remote" && spec.tunnel) {
      recordTo = remoteRecordLength(spec.tunnel);
      channelFailure = remoteRecordFailure(spec.tunnel);
      record = fetchRemoteRecord({ ...spec.tunnel, fromByte: recordFrom, toByte: recordTo });
    } else {
      // The same two questions, asked of the local file: a run's own record,
      // and whether the channel that wrote it stayed alive. Asking only
      // remotely is how the local path kept reading its verdict off the
      // stderr four stage servers share.
      const localRecord = path.join(outDir, "agent.record.log");
      record = readLocalRecord(localRecord);
      recordFrom = 0;
      recordTo = record.text.length;
      channelFailure = readLocalRecordFailure(localRecord);
    }
    fs.writeFileSync(path.join(outDir, "agent.stderr.log"), agentLog, "utf8");
    fs.writeFileSync(path.join(outDir, "drive.stderr.log"), driveOutput.stderr, "utf8");
    await stopChild(tunnel);
  }

  if (failure) {
    // The relay's discard count belongs in the failure too: a drive that
    // stopped on a position gap is usually reporting a delivery loss, and
    // the two records read very differently.
    const lost = checkDelivery(record.text);
    fs.writeFileSync(path.join(outDir, "failure.json"),
      `${JSON.stringify({ run_id: runId, failure, delivery: lost, evidence }, null, 2)}\n`, "utf8");
    const discarded = lost.passed
      ? ""
      : `relay discarded ${lost.counted + lost.uncounted} event(s) to the OUTER\n`;
    // A failure is evidence too, and the failures are the runs most likely to
    // be argued over later. Promoting them gives them the same MANIFEST and
    // the same refusal to be edited in place that a pass gets.
    const promotedFailure = run.working === run.final ? run : promoteRun(run);
    process.stderr.write(`P4_4NODE_FAILED ${failure}\nrun ${runId} at ${promotedFailure.directory ?? run.final}\n${discarded}`
      + `${driveOutput.stderr.trim().slice(-3000)}\n`);
    process.exitCode = 1;
    return;
  }

  const artifact = JSON.parse(fs.readFileSync(artifactPath, "utf8"));
  const verdict = judgeArtifact(artifact);
  // The conversation key is one-way traffic, so it is proved against the
  // adapter's own trace rather than against anything in the reply.
  // Tokens the relay dropped were generated and paid for; a run that lost
  // them did not do what it reports.
  // Four stages of the same wrong build agree with each other perfectly, so
  // their agreement is checked against what the checkout pins, not only
  // against each other.
  const build = agreesWithExpected(evidence?.compat, artifact.build);

  // Only what this run wrote counts, and the same check decides that either
  // way. The local branch used to return ok unconditionally, so a record cut
  // in half read as a clean slice - the boundary was computed and discarded.
  const fence = fencedRecords(record, recordFrom, recordTo);
  const mine = fence.records.join("\n");
  const delivery = checkDelivery(mine);
  // A dead record channel makes every count above zero for the wrong reason,
  // and it says so on the one channel still working.
  // Two paths to the same fact, and the run needs only one of them to speak.
  // The marker file is independent of the stage servers' shared stderr; the
  // stderr line is what a person watching a terminal sees.
  const channelFailures = [
    ...(channelFailure === null ? [] : [channelFailure]),
    ...recordChannelFailures(agentLog),
  ];
  const sessionKeys = checkSessionKeys(
    JSON.parse(fs.readFileSync(configPath, "utf8")),
    artifact.requests.map((request) => request.request_id),
    mine,
  );
  const report = {
    run_id: runId,
    scenario: spec.name,
    target: spec.target,
    // Which machine actually served the run, taken from the far side rather
    // than from the scenario name.
    host: identity,
    // Which llama.cpp every stage reported, taken from the stages rather
    // than from what this machine happens to have built.
    build: { ...artifact.build, matches_pin: build.ok, mismatch: build.reason },
    description: spec.description,
    structural: {
      passed: artifact.passed,
      requests: artifact.request_count,
      completed: artifact.completed_count,
      released: artifact.released_count,
    },
    meaning: { passed: verdict.passed, meaningful: verdict.meaningful, total: verdict.total },
    session_keys: sessionKeys,
    delivery,
    record_channel_failures: channelFailures,
    agent_stopped: agentStopped,
    records: { fenced: fence.ok, reason: fence.reason, lines: fence.records.length },
    metrics: metrics(artifact, spec.nUbatch),
    sample_answer: artifact.requests[0]?.response?.slice(0, 400) ?? "",
    rejected: verdict.results.filter((r) => !r.meaningful).slice(0, 5),
  };
  fs.writeFileSync(path.join(outDir, "evidence.json"),
    `${JSON.stringify({ ...evidence, expected_build: evidence?.compat ?? null, observed_build: artifact.build, finished_at: new Date().toISOString() }, null, 2)}\n`, "utf8");
  fs.writeFileSync(path.join(outDir, "report.json"), `${JSON.stringify(report, null, 2)}\n`, "utf8");
  const promoted = run.working === run.final ? run : promoteRun(run);
  process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
  process.stdout.write(`evidence: ${promoted.directory ?? run.final}\n`);

  if (!artifact.passed || !verdict.passed || !sessionKeys.passed
    || !delivery.passed || !build.ok || !fence.ok || channelFailures.length > 0 || !agentStopped) {
    process.stderr.write("P4_4NODE_NOT_ACCEPTED\n");
    process.exitCode = 1;
  }
}

main().catch((error) => {
  process.stderr.write(`P4_4NODE_FAILED ${error.message}\n`);
  process.exitCode = 1;
});
