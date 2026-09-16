# P4 distributed mock test plan

> Document status (2026-09-06): **Partial test plan**. This is the plan for the mock/OUTER path in question. It does not replace full real-hardware acceptance of very large models.
> Current goals, status and ordering follow the [execution roadmap](distributed-batching-roadmap.md); document authority and reading paths follow the [document map](document-map.md).

Goal: on the central PC and the RTX 3090×2 Windows 11 environment of `m42-server2`, without using real
llama.cpp and with `p4-mock` as the adapter, verify the protocol scenarios from discovery through
distributed load, multiple inference, queue/backpressure, monitoring, cache, and failure
return.

Scope:

- Central PC: `192.168.0.6`, responsible for the P4 source and builds
- Remote PC: `192.168.0.29`, SSH `42mob@m42-server2`
- Remote builds: forbidden. Copy only the identical Windows x64 binaries built on the central PC
- Adapters: `mock`, `mock-instant`, and `p4-link` if needed
- Experiment precondition: check the remote host's existing processes, GPU jobs and P4 ports, and stop if there is a
  conflict. Do not stop services that are in use.

## 1. Run artifacts and evidence

The checked-in local runner writes a manifest and
per-worker logs to `target/parallel-mock-e2e/<run-id>/`. Remote cross-host runs produce manual artifacts and are not
collected into this path automatically.

| Artifact | What it verifies |
| --- | --- |
| `manifest.json` | run id, worker/request/token inputs, per-worker verdict and working set |
| `worker-*/agent-*.log` | local agent stdout/stderr |
| `worker-*/drive.log` | request/stream verdicts and queue peak |
| `worker-*/drive.err.log` | driver errors |

Before copying, compare the central and remote SHA-256 of the binaries. The remote host is not required to have the source or to
run `cargo`.

## 2. Shared topology

```text
OUTER/controller
      |
  local ingress agent :52001
      |
  local mock stage :52002 ---- TCP link/relay ---- remote agent :52003
                                                   |
                                             remote mock stage :52004
      <--------------- reply/status ----------------┘
```

The actual tests use both of the following modes.

1. `local-only`: put all agents and mock stages on the central PC to separate protocol errors
   from the remote network.
2. `cross-host`: separate the local ingress/stage from the remote stage to verify frame routing,
   return anchor, peer queue, reconnect and link impairment.

## 3. Parallel run bundle

The following four worker scenarios use different ports and `run-id`s and
can run at the same time. They do not share a deployment.

| Worker | Scenario | Key evidence |
| --- | --- | --- |
| W1 | discovery/model profile | `Inspect`, `InspectModel`, artifact/profile round trip, unsupported adapter refusal |
| W2 | sustained pipeline | continuous prefill/decode, stage overlap, FIFO, ceiling, bounded queues |
| W3 | return/monitoring | multiple OUTER routes, ingress return, status correlation, disconnect/reconnect |
| W4 | lifecycle/cache/failure | load/unload, persist/restore/fork/discard, deadline, failed hop |

W1 smoke-tests the new `InspectModel` wire and the mock profile first. W2~W4
fan out in parallel after W1's binary smoke passes. The test runner itself
runs each worker as a separate process so that one worker's CPU spin or exit does not hide another
worker's result.

The repeatable run command is [`tools/scripts/e2e/run-distributed-mock.ps1`](../tools/scripts/e2e/run-distributed-mock.ps1).
The defaults are 4 workers, 2 stages per worker, 128 requests and 16 tokens, and each
worker uses its own ports, logs and deployment.

## 4. Scenarios and verdict criteria

### D-01 discovery contract

OUTER requests `InspectModel` with an artifact reference and an adapter name.
The agent returns the mock profile, and P4 does not interpret or
rewrite the profile string.

A drive with `P4_DRIVE_DISCOVER=1` runs this preflight against every selected agent
before creating nodes. It compares the artifact and opaque profile of each response,
stores the per-agent capability snapshot ID/expiry, and passes it to the same agent's `Load`.
If a response is missing, an artifact does not match, or a snapshot is empty, it does not
start create/load/inference.

Verdict:

- Request/reply correlation is maintained.
- Artifact, adapter and profile round-trip byte-for-byte.
- An unregistered adapter and an empty artifact produce an explicit `Failed`.
- The existing `Inspect` returns only the machine snapshot and does not pretend to be a
  model profile.

### D-02 distributed placement input

After collecting the model profile and capability snapshot from each of the local and remote agents,
OUTER builds one placement plan and sends an opaque `Load`
to each node.

Verdict:

- Every stage uses the same model fingerprint/profile.
- `Load` is not sent before the profile has been obtained.
- An `Internal` adapter is not used as a middle node of a staged chain.
- A snapshot mismatch or an unsupported distribution results in a load refusal.

The mock profile is not a substitute for a parser that reads real GGUF files, but it is a synthetic profile with llama-family
architecture/layer/embedding/head/KV-head/context/quantization/fingerprint and
stage/boundary bytes. The mock adapter also receives and records the opaque
load plan and sampling-shaped JSON options, rejects non-object options
per request, and returns a llama-compatible backend report. D-02 therefore
verifies discovery and the adapter boundary, but it does not prove CUDA allocator or real
llama.cpp kernel behaviour.

### P-01 pipeline feed-ahead

Give each stage a different hop cost and inject 64~256 requests
continuously. Observe whether prefill blocks decode and whether stages 0/1 process earlier and
later requests overlapping.

Verdict:

- No adapter hop exceeds the node `ceiling`.
- Per-stage busy time overlaps, and the idle gap does not keep growing
  while a queue exists.
- Token/event order is FIFO per request, and different requests do not mix.
- Both the ingress lane and the node queue are bounded, and the overflow policy is stated.

### Q-01 long-running arrivals and memory pressure

Keep the production rate above the mock processing rate and run for 10 minutes or more.
`run-distributed-mock.ps1` samples each running worker agent every 1 second
and records queue depth and process working set
min/peak/delta in the manifest together with the request/frame results.

Verdict:

- Queue depth does not exceed the configured cap.
- Readers/workers do not block indefinitely; one of an explicit reject, deadline or spill
  is observed.
- The working set does not grow without bound in proportion to the request count.
- No route/continuation/KV state remains for completed requests.

### R-01 return anchor and multiple OUTERs

Connect two logical OUTER channels to one ingress agent and send, at the same time, requests that reuse the same
route string. Then drop the ingress connection
and run the reconnect policy.

Verdict:

- Tokens and Done arrive only on the original ingress/return channel.
- Route reuse does not consume another channel's responses.
- Reconnect ends with exactly one declared policy: buffer, rebind or cancel.
- Late, duplicate or post-terminal events do not contaminate new request state.

### M-01 monitoring transparency

Collect status separately for idle, during load, prefill, decode, blocked link, failed hop, and right after
unload.

Verdict:

- Agent/node/adapter identity and request/stream/sequence/hop correlation are
  linked to status and events.
- Lane depth, peer queue, in-flight, phase, last-progress and deadline are
  distinguished.
- GPU utilization is not claimed from queue depth alone; mock busy/idle and the
  adapter report are recorded together.
- Stale status can be rejected using the snapshot sequence and generated time.

### K-01 cache lifecycle

On each stage, persist, restore, fork and discard the same sequence, and inject a restore
failure and a deployment generation change separately.

Verdict:

- Every stage reports the same operation/sequence identity.
- A state where only some stages were restored is not exposed to inference.
- Fork preserves the original and changes only the new sequence, independently.
- If the model fingerprint, deployment generation or cache format does not match, it is
  rejected explicitly.

## 5. Run order

1. Record `git rev-parse HEAD`, `cargo test --workspace` and the binary hashes in the
   manifest.
2. Run local-only W1 to check the discovery codec and the mock hook.
3. Build the release binaries on the central PC and copy them to the remote Windows path.
4. On the remote host, check read-only the hostname, OS/architecture, binary hash, P4 ports in use and
   existing P4/backend processes.
5. Run the cross-host W1 smoke on separate, non-conflicting ports.
6. After W1 passes, run W2~W4 in parallel with separate run-ids.
7. Run Q-01 as a separate long run, sampling the Task Manager or PowerShell
   process working set together with P4 status.
8. Compare all processes and listeners against the run manifest to confirm nothing remains after shutdown.
   Existing user processes are excluded from shutdown.

## 6. Limits of the current implementation and next implementation steps

- `InspectModel` uses the P4 wire and the adapter hook, and the mock profile
  deliberately does not imitate GGUF facts.
- The served concrete adapter validates relative
  artifact references under `P4_MODEL_DIR` or `LLAMA_MODEL_DIR` and returns the real GGUF metadata/tensor index profile and
  fingerprint.
- The parser smoke on a real model file passed with
  `Qwen2.5-1.5B-Instruct-Q8_0.gguf`.
- The discovery response carries the snapshot ID/generated/expiry, and when OUTER binds it
  to Load, the agent rejects an expired snapshot before calling the adapter.
  Still remaining: verification that the ID actually matches in a snapshot registry, a hardware resource snapshot, and adapter
  distribution-mode validation.
- The agent lane, node ingress, adapter event channel and node outbox are all
  bounded RAM paths based on `Budget.depth`. Excess node work does not wait;
  it is returned as an explicit `Failed`, and a slow downstream propagates backpressure through the bounded outbox
  up to the event loop. Disk spill, FIFO paging and
  retry quota policies are not implemented yet.
- There is no real staged GPU adapter, so the mock overlap results of this plan are not GPU
  utilization evidence.

Related contracts:

- [protocol.md](protocol.md#9-model-discovery-and-distributed-loading)
- [testing.md](testing.md)
- [service message](../layers/service/src/message/mod.rs)
- [service wire](../layers/service/src/message/wire.rs)
- [adapter contract](../layers/adapters/adapter/src/lib.rs)
- [mock adapter](../layers/adapters/mock/src/lib.rs)

## 7. Run results

On 2026-08-18, a cross-host mock smoke was run on the central PC and `192.168.0.29`
using only release binaries.

| Item | Result |
| --- | --- |
| binary | after the central release build, only `p4-agent.exe` and `p4-drive.exe` copied to the remote host |
| hash | for this fix release, central and remote match: `p4-agent.exe` `2F7172B7578FCBA6E4ACADA3A04849EC411396ED7A9DAA15AEDF558EC5B22BC9`, `p4-drive.exe` `A0B88A6BD7686304AD733D18F0FCD2C88D9C1811FAB86AD79AC910EAF3EE4A37` |
| topology | local stage + SSH-forwarded remote stage, 2 stages |
| load | nodes 2, mock, ceiling 8 |
| inference | 32 requests × 8 tokens |
| discovery | 2 profiles collected (local/remote), profile bytes match |
| result | completed 32, failed 0, unanswered 0, tokens 256 |
| timing | 385 ms, 748 frames/s |
| queue | peak node 31, peak adapter 8, peak main lane 0 |
| ordering | every stream in order, one terminal per route |

On the direct `192.168.0.29:52001` path the remote agent started normally, but the central PC could not connect to
the remote TCP listener, and a 30-second node creation timeout occurred.
Without changing the firewall, the run was repeated with bidirectional SSH forwarding `-L 52101`, `-R 52003`
and passed. On the first tunnel attempt the remote agent disappeared when the SSH session ended,
so the agent was kept alive in a PTY session. This result is evidence that when operating remote
Windows hosts, the firewall, process lifetime and forward/reply paths must all be recorded in the
manifest.

The latest release added the mock discovery preflight. The remote
agent bound to `52001` and advertised `127.0.0.1:52101`, and the driver used
`P4_DRIVE_DISCOVER=1` to collect both agents' profiles before loading. After both local and
remote profiles were collected and the profile bytes matched, it ran 32 requests × 8 tokens
with `completed=32`, `failed=0`, `unanswered=0`, stream order passing,
`tokens=256`, `elapsed_ms=385`, `frames_per_second=748`. This result
proves the cross-host path discovery→opaque Load→pipeline inference, but
it does not prove a real GGUF file fingerprint or GPU memory usage.

On the final llama-shaped mock release, `run-distributed-mock.ps1` ran 4 workers
in parallel. Each worker handled 1024 requests ×
32 tokens on an independent 2-stage deployment, and all four workers recorded `completed=1024`, `failed=0`,
`unanswered=0`, `tokens=32768`, stream order passing. Per-worker peak
node queue was 753, 747, 746, 729; peak in-adapter was 16; peak main lane was
9~15; working-set peak was about 25~26 MB. This result verifies that per-request
FIFO and terminal correlation hold even with several agents/drivers running at once.

After re-copying the bounded release to the remote host, the 2026-08-18 cross-host tunnel smoke was
also rerun. On the 2-stage topology with a central stage and a remote stage, 32 requests ×
8 tokens finished with `completed=32`, `failed=0`, `unanswered=0`, stream order passing,
with peak node queue 31, peak in-adapter 8, peak main lane 0. This
run also passed the discovery preflight and delivery of generation options/opaque plan,
and after the run all central and remote P4 listeners were cleaned up.

A 6000 requests × 1 token overflow run also finished with `completed=6000`, `failed=0`,
`unanswered=0`, `tokens=6000`, stream order passing, elapsed 55,659 ms,
peak node queue 24, peak in-adapter 16, peak main lane 14. The runner collected 55
working-set samples and recorded min 15,138,816 B, peak 21,798,912 B, delta
6,660,096 B in the manifest. In other words, producers were throttled at lane admission and the
node queue/event/outbox did not grow without bound. These figures are mock adapter
results and do not replace verification of real GPU memory/allocator limits.
The earlier 60000-request record took 595622 ms, shorter than the 10-minute condition, so it is not
claimed as a 10-minute pass. The arrival-paced local run of this fix release processed 6000 requests
× 1 token over 56470 ms and recorded `completed=6000`, `failed=0`,
`unanswered=0`, `tokens=6000`, peak node queue 23, peak in-adapter 16,
peak main lane 9, 12002 samples. This is bounded mock queue evidence and
does not replace verification of real GPU memory/allocator limits.

### Latest correlation/retry verification

After the response-correlation, bounded emergency retry, blocking-admission, and
file-backed mock-cache changes, the release binaries were rerun on 2026-08-18.
The local discovery run collected two profiles and completed 16 requests × 8
tokens with all stream/terminal verdicts passing.

The latest SSH cross-host run used local agent `192.168.0.6:52301`, remote
agent advertised as `127.0.0.1:52311`, and bidirectional `-L 52311`/`-R 52303`
forwarding. Discovery collected both profiles; 32 requests × 8 tokens completed
with `failed=0`, `unanswered=0`, `tokens=256`, `elapsed_ms=323`, and all four
request/order/terminal verdicts passing. This remains mock and transport
evidence, not real GPU or production OUTER subscription evidence.
