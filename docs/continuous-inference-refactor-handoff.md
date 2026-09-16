# Continuous distributed inference refactoring handoff

> Document status (2026-09-06): **Historical / old plan**. Preserves the plans and observations of the time. Do not use it for current status, execution order or promotion criteria.
> For the current goal, status and order, follow the [execution roadmap](distributed-batching-roadmap.md); for document authority and reading paths, follow the [document map](document-map.md).

This describes the code as of 2026-08-24, the generated run evidence and the not-yet-committed working tree
together. This document is not a change log that collects only successes. It is the handoff
baseline that fixes what was changed and why, how far each piece of evidence proves, and what the next
work must pass.

> 2026-08-25 update: this document is the historical pre-event-architecture
> audit baseline. The replacement implementation and its current proof order
> are authoritative in [P4 self-describing event contract](event-protocol-v2.md).
> The "current" conclusions below describe the preserved reference branch
> `codex/p4-pre-event-architecture-reference`, not the replacement branch.

## Current conclusion

Implementation and verification are finished up to the lossless cut-set and the ABI 18 native runtime. P4
handles only deployment submission, cancellation and result delivery; the llama.cpp adapter/native runtime
owns the physical UBATCH, Prefill/Decode mixing, stage-local state and sampling.

Gate A passed. Two stages on local 3090 + local 4080 produced the same UTF-8 response `cobalt-314159` as the
single-stage baseline, and native conformance also preserved the F32 cut-set's
descriptor and payload bytes unchanged. The whole product, however, is not complete.

- The canonical 40-request local wave succeeded with 40/40 terminals, mixed Prefill/Decode
  and the GPU ceiling respected, but only 6/40 passed the manual semantic check.
- Gate B is therefore **FAILED**. High TPS and 40/40 transport do not replace
  correctness, and per the rules we did not proceed to TUF, remote 3-stage, placement calibration
  or P4 parity.
- The Qwen3.6-27B Q6_K 2-stage candidate exceeded the local ceiling in loaded VRAM before any request was
  submitted. Native was reinforced to record stock llama.cpp's model/context/compute breakdown directly
  and to check each stage's `--vram-limit-mib` before ready, but
  this does not mean the GGUF-only planner predicts the exact compute graph size before load.
- The Event 41 power failure on the remote host with two 3090s and the damaged TUF SSH state remain as they were.

The current state is **an implementation midpoint whose correctness is proven up to Gate A**. The next approval
must first settle on a local acceptance model/workload that can meet 40/40 semantic correctness and on an exact
compute-buffer memory plan.

## Independent re-audit record — 2026-08-24

This record does not re-quote the earlier run figures above. It is the result of artifacts and runs newly
produced in the same dirty worktree
(`p4/adapter-boundary`, `ba46d6300ccb1bfead90f4f8f4a66d6a27cc4d07`).

- compat prepare verified official `3e3a7a416d` and patch-set
  `817426f81c890002708c50c0a5deb6905a3f4348b92198481984d58bf3e88626`.
  The upstream must be a pristine tree without Linker sources.
- `packages/llama_domain` was built first, then the `apps/llama` bundle and the CUDA
  runtime were rebuilt. The final ABI 18 `linker-node` SHA-256 is
  `a9c34e9f9465a011c902ae9fd1a47efa035df5875d1f5fdc830bc42bbc301a09`, and the
  build ID is
  `linker-pipeline-3e3a7a416d65-3e3a7a416d.817426f81c890002708c50c0a5deb6905a3f4348b92198481984d58bf3e88626-78b7fe9a18aa`.
  The server bundle SHA-256 is
  `02a28af41b93b9ff34e5e4c8c8c2ed6cc1d26aef8ee1fbdbf0d4c837da775187`, and
  the value reported by `/api/runtime` matched the actual file hash.
- A pack manifest defect in [`build-node-runtime.ps1`](../../llama/scripts/build-node-runtime.ps1), found during the build,
  was also fixed. With the default relative build directory, it truncated the absolute
  artifact path incorrectly, so `files[].path` was not runtime-relative.
  The runtime root is now normalized to an absolute path and a regression check was added; all 26 file paths
  in the new pack are runtime-relative. This fix is also still in the dirty worktree.
- Unit re-verification: direct-pipeline 57/57, `llama_domain` 132/132,
  `apps/llama` server 96/96 + scripts 22/22. This is not evidence of GPU correctness.

### Gate A single request: passed with lossless ABI 18

The first Gate A run failed. A byte-level audit found a product path that lowered the generation cut-set from stock F32 to
F16 and then raised it back to F32, and the single-stage and
two-stage greedy token streams differed. This lossy conversion was removed, and the ABI was bumped from 17 to 18 so the tensor
descriptor and payload pass through unchanged.

The real receive path and native conformance now share the same decode helper.
Conformance checks the F32 descriptor, element count and payload byte equality, and
passed with `exact_physical_cutset=true`. The former `hidden_f16_*` observation keys were
renamed to `hidden_tensor_*`, with a legacy fallback kept only in the persisted history reader.

A fixed semantic oracle prompt was submitted once each to the latest single-stage and to local two-stage (`[0,18)`,
`[18,28)`). Both runs produced 9 tokens, `stop`, and UTF-8 bytes
`cobalt-314159`, and the server/native/dirty-diff hashes also matched. The latest two-stage
evidence SHA-256 is
`215ea62ee806ac9095fb2ca31a104b412bbe8205712f0abd8833e3225741d6cb`.

During the audit, a comparison that built the default even `[0,14)`, `[14,28)` split without a physical ceiling
produced `RESULT = 314159` and failed the semantic check. This run was not deleted either.
A run with a different cut-set is not a regression comparison; it proves that the split itself is a
correctness input. Local multi-stage preparation therefore now requires a physical VRAM ceiling for every stage,
and an unjustified even split fails closed before any request is created.

- [single-stage baseline report](../../../target/gate-a-lossless-abi18/baseline/report.json)
- [two-stage report](../../../target/gate-a-lossless-abi18/two-stage/report.json)
- [latest single-stage baseline](../../../target/final-gate-a-abi18/baseline/report.json)
- [latest proven 18/10 split](../../../target/final-gate-a-abi18/two-stage-proven-split/report.json)
- [preserved rejected 14/14 split](../../../target/final-gate-a-abi18/two-stage/report.json)

### Gate B local 40-request wave: structure passed, semantic correctness failed

The canonical 20 + 5 + 5 + 5 + 5 wave ran on Qwen3.5-9B Q5_K_S, `parallel=40`, `batch=ubatch=128`, local two-stage
(`[0,22)`, `[22,32)`).
40/40 ended with `stop`, with 40,600 tokens at an aggregate 92.111 token/s.
Every follow-up wave overlapped existing Decode, and both stages recorded 8 mixed samples.
Peak VRAM was 10,485 MiB on the 3090 and 9,370 MiB on the 4080, within the ceilings.

However, the hash-bound review that read all 40 full responses passed only 6/40.
Semantic errors recurred: forbidden invented figures, inverted offset commit results, a misreading of the autovacuum lock,
and double-counted headroom. The automatic format checks passed, but with the
manual review complete and the evidence hash matching, the result is `passed=false`.

- [Gate B report](../../../target/gate-b-local-abi18/inference/report.json)
- [Gate B complete responses](../../../target/gate-b-local-abi18/inference/evidence.md)
- [Gate B manual review](../../../target/gate-b-local-abi18/inference/manual-semantic-review.json)
- [Gate B semantic verdict](../../../target/gate-b-local-abi18/inference/semantic-review.json)

The larger Qwen3.6-27B Q6_K candidate, right after load with `batch=ubatch=768`, used 24,253 MiB on the 3090
and 15,511 MiB on the 4080, exceeding the 23,552/12,288 MiB ceilings respectively.
Not a single request was submitted, and the runtime was deleted. About 12.9 GiB of graph compute
buffer was reserved per stage, but the current planner only accounts for GGUF/KV persistent memory.
Until this defect is fixed, do not retry the same model with a reduced width.

## Self-audit: the gap between the early assurance and the actual result

The early assurance that this goal could be implemented was inaccurate. It did not distinguish seeing a possible
structure in the source from proving correctness, performance and generality together on the current product path.
A new session must treat this document's conclusions as claims only, and re-judge the items below
with code and new runs.

| Early expectation or interim claim | Actual current state |
| --- | --- |
| P4 and the llama adapter can be fully separated | A new deployment relay and contract exist, but legacy Hop/window code remains, and operational parity has not proven that it can be removed. |
| The llama.cpp adapter performs continuous mixed batching efficiently | On the canonical 40 requests, structure, sustained mixing and transport passed, but semantic correctness was 6/40, so performance approval is blocked. |
| Several GGUF architectures are supported generically | Model-name branches are rejected, GGUF recurrent metadata is read, and inspect passed for several architectures. Actual generation has not been proven for every architecture. |
| Stage-local KV/recurrent memory is complete | The latest source/dist/native load and stock breakdown instrumentation passed. A real 2-stage run of the Qwen2 transformer passed, but real-hardware generation on recurrent architectures is not yet approved. |
| The 4-GPU success proves the current implementation | Some runs used a stale binary, and the 250 W run deliberately reused the earlier binary and a fixed plan to isolate the power cause. It is not evidence for the current dirty source. |
| 40/40 terminals means inference succeeded | The latest local Gate B manual semantic check was 6/40. This failure came from confusing transport completion with correct inference. |
| Remote preparation is a simple follow-up task | Direct preparation supports a dynamic GPU count and a 2+1 topology, but TUF SSH and actual supervisor-owned model load are still blocked. |

Repeated errors of judgement are preserved as well.

- Counter `0`, process exit, listener, configuration and file existence were mistaken for evidence of actual data-path
  execution.
- After source changes, the runner that selected a stale binary was not sealed first, so
  an unrelated run was reported as a success of the new code.
- Terminal/non-empty checks were over-interpreted as semantic correctness of the responses.
- We tried to reimplement in P4 batching that was already solved, before reading llama.cpp and the existing native scheduler
  thoroughly.
- Only VRAM was watched; the hardware power envelope was not treated as a separate safety condition.
- TUF's existing SSH key file was modified before it was backed up and identified, and a key
  append without a newline plus PowerShell scalar concatenation damaged even the existing access.

This work's outcome must therefore not be called "an ultra-high-performance distributed inference implementation".
What can be acknowledged now is a partial implementation of boundary/relay/native scheduler/planner/evidence
and the identification of several failure causes. A product success verdict is possible only after every item of the completion definition passes
with new artifacts.

## Final responsibility boundary

| Owner | Owns | Does not own |
| --- | --- | --- |
| P4 core | deployment discovery and identity, bounded relay, `Submit`/`Cancel`, routing of `Accepted`/`Rejected`/`Produced`/`Settled` | rank, layer, KV, Prefill, Decode, Hop, Window, UBATCH, per-model branches |
| llama deployment client | sustained multiplexed connection, generation fencing, reconnect/replay, cancel retention, bounded queue/ledger, `Full` deadline | physical batch composition, reinterpretation of tensor shapes |
| `apps/llama` | GGUF inspection, runtime pack and process lifecycle, placement candidates and validation, run statistics aggregation | P4 routing policy, execution branches based on model names |
| native `linker-node` | ready session, physical llama.cpp UBATCH, Prefill water-fill over the rows left after reserving Decode rows, per-stage KV/recurrent state, exact cut-set, terminal sampling | P4 admission and external deployment identity |
| stock llama.cpp | model architecture graph, tensor role/shape, actual physical ubatch, backend/RPC/GPU execution semantics | Linker/P4-specific protocol and state |
| benchmark harness | identical workload, binary/source hash, GPU/VRAM/power samples, full responses and manual semantic verdicts | runtime policy, workaround rules that interpret results as success |

The representative implementation of the current boundary is in
[`agent/relay.rs`](../layers/agent/src/agent/relay.rs),
[`deployment/client.rs`](../layers/adapters/llamacpp/deployment/src/client.rs) and
[`entrypoints deployment.rs`](../entrypoints/agent/src/adapters/deployment.rs).
The P4 relay is an alternative to the path that builds hops, and it does not classify phases. If the required
deployment address is missing, startup fails; this prevents silently falling back to the legacy path
while the new path is under test.

## What was changed and why

### 1. Shrink P4's backend-shaped boundary to deployment submission

Early P4 had llama.cpp execution knowledge, such as Prefill/Decode lanes, Hop/Window, and position and remaining tokens,
pushed all the way up into the core. In that structure, every new model and new
batch algorithm also changed the P4 protocol and scheduler, and P4 ended up
re-deciding physical batching with less information than the adapter.

This was replaced with the deployment-scoped `Submit`/`Cancel` and
`Accepted`/`Rejected`/`Produced`/`Settled` contract. The P4 payload carries the
prompt, the maximum output length and opaque options; the OpenAI/llama request shape is built
in the adapter. `Full` is also handled as a typed refusal, not a string error.

The purpose of this change is not to make P4 useless. It keeps only the distributed
control-plane properties that P4 must know, and moves GPU execution knowledge down to the
adapter, which has more information.

### 2. Harden the relay into an operable bounded stream

The sustained connection and reconnect paths had the following defects.

- a blocked socket write blocked the submit call itself
- a submission could be sent 0 times between reconnect's snapshot and install
- control and data shared the same bounded queue, so `Cancel` and generation advance
  were silently lost
- conversely, making the channel unbounded to eliminate control loss let memory
  grow without bound
- cancel intent was not in the ledger, so it was lost on reconnect after a write failure
- stale reader events had no connection epoch and were handled as events of the new connection
- `Full` retries could continue forever without a deadline
- P4 invented the generation with a default of `1`, although the backend must issue it

A single-owner pump, submission permits, bounded inbound/ledger, cancel intent replay,
connection epochs, a backend-owned generation handshake, absorption of duplicate reconnect prefixes,
and `Full` retries with a deadline were introduced. In particular, after reconnect an already received
`Produced` prefix is dropped as a duplicate, and only a forward gap is treated as an error.

This path was not sealed on green unit tests alone. The properties were confirmed with mutations that fail when the
corresponding guards for reconnect, cancel and the concurrency ceiling are removed, and with real-socket tests.

### 3. Fix lifecycle/identity defects first and keep them as a legacy checkpoint

There were EOS slot leaks, index re-derivation from position, missing terminal accounting, and a late close
during sequence reuse that cancelled a new session. Response event
ordinals, session epochs, terminal accounting, close acknowledgement and bounded
tombstones were introduced to block the lifecycle defects of the path at the time.

This work was not a batching implementation; it was a capacity/lifecycle recovery of the existing path.
`b0bc5b06f` is the checkpoint that preserves that fact. In the final deployment submission
contract, `Settled` is the submission terminal, so much of this checkpoint's
`SessionClose/session_epoch` must not be mistaken for the final ownership.

### 4. Move physical batching behind the llama.cpp adapter

The old P4 window did not mix lanes, but the native path already put Prefill and Decode together
into one physical UBATCH. P4 cannot know what batch optimization needs: the ready sessions,
KV residency, the remaining prompt, and the microbatches llama.cpp actually split.
Optimization was therefore moved into the native adapter's ownership.

The current [`session.inc`](../../llama/native/linker-node/inference/session.inc) treats
one scheduler window as one physical llama.cpp UBATCH. It first reserves one row for each active
Decode sequence and assigns the remaining rows to Prefill.
[`prefill-plan.inc`](../../llama/native/linker-node/inference/prefill-plan.inc)
water-fills the residual rows so that one long prompt does not take all of them.
[`physical-window.inc`](../../llama/native/linker-node/inference/physical-window.inc)
maps the physical microbatches llama.cpp actually built to their logical owners, and
[`batch-forward.inc`](../../llama/native/linker-node/pipeline/batch-forward.inc)
makes every downstream stage execute the same physical capsule.

As a result, a single request is handled as a normal case of a less-full batch, and with multiple requests the
remaining GPU width can be filled with Prefill without starving Decode. This is not
a policy that fixes "Prefill-only nodes" and "Decode-only nodes". Each window composes
both phases together according to the actual ready state.

### 5. Correct to stage-local KV and recurrent memory

Allocating a pipeline-sized KV cache on every node, or having the adapter special-case models by
name, are both wrong. Each stage must hold only the mutable state of the
layers it owns. The authority on tensor roles and sizes must be the GGUF and the
stock llama.cpp architecture graph.

KV is allocated to match each stage's contiguous layer range, and recurrent models were generalized to compute
the exact R/S width from the GGUF's SSM convolution/state/inner/group metadata. If the metadata is incomplete,
loading fails closed rather than passing on a model-name guess.
The detailed contract is in [llama.cpp stage memory](llamacpp-stage-memory.md) and
[`apps/llama` internals](../../llama/docs/internals.md).

The current source, `packages/llama_domain/dist`, the `apps/llama` server bundle and the ABI 18
native runtime were rebuilt. The latest local 2-stage load of the Qwen2.5 transformer and a
single request passed. The recurrent R/S width was verified up through parser/unit/compat source,
but the latest stage generation for recurrent models lies past the Gate B stop line, so it is
not yet product approval evidence.

### 6. Switch placement from VRAM ratios to measurable cost

The change moves away from fixed rules such as a `12:23:23:23` VRAM ratio or "the 4080 always goes first".
Actual placement must weigh the following costs together.

- stage weights and exact KV/recurrent cache obtained from the GGUF
- fixed costs such as the leading embedding and the trailing norm/LM head
- boundary activation and transport cost
- actual free VRAM per device and the safety ceiling
- per-stage `prefill_compute_us`, `decode_compute_us`

The planner produces candidates, and after ready it must re-verify the ceiling against the actual increase reported by `nvidia-smi`.
The current verification topology is local 3090 23 GiB, local display-owning
4080 12 GiB, and TUF 4070 Laptop at `min(8,192 MiB, dedicated VRAM reported by the device)`.
TUF's display is driven by the integrated AMD GPU, so the full dedicated VRAM that the 4070 reports
can be used as the inference ceiling. Simple
capacity proportionality is only a first candidate; the final split and rank order are decided by balancing per-stage compute time,
edge fixed costs and remote boundary transfer costs together.

It was confirmed that `S:\models` is mounted in TUF's interactive user session, but the drive visibility of an SSH
session is not a runtime authority. The central PC's current non-interactive
SSH key was rejected by TUF. The next preparation task is not to restore the access channel and then inspect
`S:` over SSH; it is to pass the same
`S:\models\...` argument to TUF's actual supervisor/native process and record that it becomes model-ready.

### 7. Change testing from "the process ran" to actual evidence

In one case, a binary on an old fixed path was selected and
40/40 was reported without running the new code at all. Responses with no output file, or with only a URL, could also pass
on an empty-body check alone. The harness therefore gained the following.

- source HEAD, dirty paths, dirty diff hash, untracked source hash, harness files,
  and the server bundle, native hash and build id actually executed
- the binary path, mtime and hash actually selected
- a different long prompt per request, deterministic sampling and full responses
- per-session terminal/tokens/body, wave overlap, per-phase compute time
- per-GPU utilization, VRAM and power samples
- a semantic review in which a person reads every output text and judges it

The formal target workload is defined in
[`direct-pipeline/README.md`](../../../test/benchmarks/direct-pipeline/README.md):
20 simultaneous starts, then 5 every 60 seconds, 4 times, for 40 requests total. Input is about 480 tokens,
the output ceiling is 2048 tokens, and the prompt encourages sufficient output without forcing
its length. This contract exists to create the situation where a new Prefill wave arrives while existing Decode
is in progress.

## Run evidence so far

| Run | Result | What it proves | What it does not prove |
| --- | --- | --- | --- |
| 5 concurrent requests, Qwen3.6-27B Q6_K, 4 GPUs | 5/5, 7,103 generated tokens, 594.777 s, 11.942 aggregate tok/s, mean first-token 5.369 s | 4-stage execution, 5-session physical batching, `mixedSamples=1` on stages 0/1, max sampled UBATCH 682 | 40-request wave, sustained saturation, latest uncommitted source |
| 10 concurrent requests, remote 3090 350 W | 10/10 error, 0 generated; remote Windows Event 41, bugcheck 0 | the host abnormally lost power/reset under load | the exact cause among PSU, cables and GPU |
| same 10 requests, remote 3090 250 W | 10/10, 13,228 generated tokens, 522.821 s, 25.301 aggregate tok/s | the same binary, plan and workload completed at the lowered power limit; all 4 GPUs reached peak util of 83--100% | that 250 W is a permanent fix, 40-request wave, sustained mixed phase, latest source |

The 250 W run's compute instrumentation recorded prompt 3,482 tokens, Prefill 3,472 tokens,
`prefill_compute_tps=1758.916`, Decode 13,228 tokens and
`decode_compute_tps=55.582`. GPU peak VRAM was 9,701 MiB on the local 3090,
12,061 MiB on the local 4080, and 10,804 MiB on each remote 3090. The 4080 stayed under the 12 GiB
ceiling, but with only about 227 MiB of headroom, so a larger plan must not be
allowed without measurement after ready.

This run was a burst that sent the first 10 requests at once, so every sampled batch on stages 0/1 was
Prefill, and `mixedSamples=0`. The 5-request run's `mixedSamples=1`, by contrast, only proves that
the mixing capability exists. Neither is performance evidence that the mixing ratio and GPU saturation
hold under continuous waves.

Response delivery succeeded 10/10, but the manual semantic verdict was 4/10. There were cases where sentences were cut off at 2048 tokens,
where figures and attempt counts the prompt forbade were invented, and where the answer contradicted the residual
Prefill rule. This run is therefore a success of pipeline
transport and compute, not a success of inference quality.

Locally generated evidence:

- [5-request report](../../../target/direct-pipeline-distributed/qwen36-27b-wave-p5/inference/report.json)
- [10-request 250 W report](../../../target/direct-pipeline-distributed/qwen36-27b-wave-p10-power250/inference/report.json)
- [10-request full responses](../../../target/direct-pipeline-distributed/qwen36-27b-wave-p10-power250/inference/evidence.md)
- [10-request manual semantic verdict](../../../target/direct-pipeline-distributed/qwen36-27b-wave-p10-power250/inference/manual-semantic-review.json)
- [350 W host failure diagnosis](../../../target/direct-pipeline-distributed/qwen36-27b-wave-p10/failure-diagnosis.json)

`target/` evidence is locally generated output, not evidence preserved in Git. The next approval run
must copy the same content to a repository-owned evidence location and link it by hash.

## What the change history means

| Scope | Representative checkpoint | Meaning |
| --- | --- | --- |
| generic boundary and event accounting | `b6e16941f`--`b94156408` | removed backend shape and silent laps from the P4 contract |
| terminal/capacity recovery | `096c9c2b7`, `b0bc5b06f` | sealed the leak and reuse fences of the legacy staged path |
| deployment relay | `d8f603e7c`--`f663dff7b` | actual operational path, boundedness, reconnect, generation, deadline |
| separation of responsibilities | `848619ea3`, `e29db45d8` | deployment-owned inference and adapter-owned mixed batching |
| physical UBATCH | `ff870d7ec`, `9172729cb` | residual Prefill water-fill and continuous staged execution checkpoint |
| model-agnostic memory | `71b6d7307`--`c94acf2e4` | stage-local mutable state, rejection of model-name branches |
| reproducible evidence | `97eb79f08`--`ba46d6300` | sampling, phase TPS, accelerator, opaque remote model root |

## Resume checkpoint — 2026-08-24

A new session starts from this section. All of the existing dirty worktree belongs to the current work, so
do not reset, check out or run a broad clean.

- branch: `p4/adapter-boundary`
- base HEAD: `ba46d6300ccb1bfead90f4f8f4a66d6a27cc4d07`
- status: the docs, source, compat patch and benchmark harness are all in the uncommitted dirty
  worktree. In a different worktree or a clean clone, this checkpoint cannot be
  reproduced.

### Currently green

| Check | Result |
| --- | --- |
| `node --test test/benchmarks/direct-pipeline/*.test.mjs` | 57/57 |
| `npm test --workspace packages/llama_domain` | 132/132 |
| `npm test --workspace apps/llama` | server 96/96 + scripts 22/22 |
| docs graph `--include-apps` | violations 0 |

Direct preparation's request, placement, runtime, `nvidia-smi` parser and host
bootstrap each accept one or more real GPUs on both local and remote hosts and build the stage count dynamically.
A 2 local + 1 remote topology and host boundary preservation are also pinned by tests.
The VRAM/reserved/compute arrays must have exactly as many entries as the physical GPUs discovered.

[`run-ssh-forwarded-real-four-node.ps1`](../tools/scripts/e2e/run-ssh-forwarded-real-four-node.ps1) is
an entry point for past E2E records, with its name and verdict fixed to four processes. It is not used as the current TUF 2+1
acceptance path, and it is not a product runner that bypasses
the new direct preparation.

### Current stop point

1. The Gate B canonical local wave structurally completed 40/40, but the hash-bound manual semantic
   verdict was 6/40, so it is **FAILED**. This failure takes precedence over remote and performance approval.
2. The latest source/dist/server/native provenance matches Gate A. However, the 27B
   candidate's actual VRAM after load, including the exact compute graph, exceeded the ceiling, and
   the 9B candidate did not pass the semantic gate.
3. The 10-request run used to isolate the power cause used the earlier, exactly pinned binary/plan.
   It is not product proof of the current dirty source.
4. TUF `admin@192.168.0.17` is reachable on TCP 22, but it currently rejects the central PC's
   `BatchMode` public-key authentication. TUF's effective `sshd_config`
   reads `C:\ProgramData\ssh\administrators_authorized_keys`. The cause is that in this file
   the existing key and the added key were joined without a newline and parsed as one key's comment.
   The fix is to split the key records into one line per `ssh-ed25519`, remove duplicates and then
   restart `sshd`. This recovery is not a prerequisite for the local gate.

### What to resume immediately

Do not start with remote generalization or SSH recovery. Gate A is finished, and Gate B correctness
is the current stop line.

1. First judge the 6/40 failed responses and the model/workload fit. Do not relax thresholds or add automatic
   retries that turn transport success into success of the model's capability.
2. Distinguish the GGUF persistent plan before load from stock llama.cpp's
   model/context/compute breakdown after load. If any stage's actual breakdown exceeds its physical
   ceiling, fail before ready.
3. After deciding on a local-fit model/workload that can meet 40/40 semantic correctness, run the same trace
   once. Do not add model-name branches or arbitrary splits.
4. Only after local Gate B passes, proceed to TUF SSH, supervisor-owned 3-stage model load and
   the TCP boundary.

### First audit order for a new session

1. Do not quote this document's figures; start by re-collecting `git status`, the diff and the current binary
   hash.
2. Check that the patch hash in [`manifest.json`](../layers/adapters/llamacpp/staged/compat/3e3a7a416/manifest.json)
   matches the actual prepared tree. The upstream must contain
   no Linker changes.
3. Rerun the three tests below. Green proves only unit properties and is not
   reported as GPU success.

   ```powershell
   node --test test/benchmarks/direct-pipeline/*.test.mjs
   npm test --workspace packages/llama_domain
   npm test --workspace apps/llama
   ```

4. Record the actual output location, hash and `--runtime-info` for `npm run build --workspace apps/llama`
   and for the current CUDA native build. Do not rely only on a message saying the build script
   succeeded.
5. Check that the full input and output of the latest Gate A and the source/dist/server/native/diff hashes are all
   bound together. While Gate B is failing, do not do remote/TUF work or calibration.

The first question of a new session must not be "what code should we build next" but "does the current dirty source
actually become one binary that produces one correct response".

## Next work: three integration gates

Use only the following three gates, so that intermediate steps do not multiply into independent goals. Each gate
has one runnable artifact and an explicit stop condition.

### Gate A — deterministic match of the latest artifacts and single-request correctness

1. Build the current GGUF/recurrent/planner/source first, and from that source build the
   TypeScript `dist`, the server bundle and native `linker-node`, in that order.
2. Put the source HEAD + dirty diff hash, dist hash, native/server hash and build id into the report,
   and fail before running if any artifact is older than the source.
3. Inspect several GGUF architectures in local `S:\models` to confirm that the adapter has no model-name
   branches and that the required
   metadata is complete or explicitly unsupported.
4. First, on two stages (local 3090 + local 4080), check that the planner computes the exact stage
   weight/cache and that the actual VRAM increase after ready stays within the 23/12 GiB
   ceilings.
5. Run a long prompt at parallel 1 and have a person read the input, the full output and the Prefill/Decode TPS.
   If the meaning is wrong or a sentence is cut off at the token ceiling, it fails.

Do not run performance sweeps before Gate A is finished. Repeating a wrong artifact under a larger
load does not add evidence.

### Gate B — continuous mixed batching and node consistency

Gate B is also run first on the two local stages. The remote host with two 3090s that caused the power problem
is used only after the user turns it back on and explicitly approves its power safety.
After the local wave passes, add the one TUF laptop that is the current remote target, and verify access,
3-stage request/placement, the TCP boundary and runtime-ready provenance.

1. Run the canonical 40-request workload as is: 20 concurrent, then 5 every 60 seconds for
   4 waves, with about a 1K input+output budget per request.
2. Every wave must overlap the Decode of earlier requests, and each stage must show the same
   physical execution id, row ownership and terminal count.
3. Do not look only at `mixedSamples > 0`. Store Prefill/Decode tokens and compute time,
   sampled occupancy, queue depth, GPU utilization, VRAM and power per interval for the whole
   run.
4. A person reads all 40 full responses. Correctness succeeds only when 40/40 terminals and 40/40 semantic verdicts
   pass together.
5. If Event 41, a stage pipe close or a VRAM ceiling breach occurs even once on the remote, stop
   immediately. Do not cover up failures with automatic retries or power limit changes.

### Gate C — measurement-driven optimization and P4 path approval

With Gate B's exact workload and seed fixed, A/B only placement, rank order,
`batch/ubatch` and pipeline window depth, one variable at a time. The objective is to maximize aggregate Prefill+Decode TPS,
with response correctness, VRAM and power safety as constraints.

- Choose the split that shortens the longest stage in per-stage Prefill/Decode compute time.
- Do not just raise average GPU utilization. Look at per-wave idle gaps, batch occupancy,
  aggregate token/s and tail latency together.
- Feed the same trace into the direct adapter path and the P4 relay path, and compare response/event
  parity. The P4 queue must reflect only bounded transport pressure and must
  contain no phase scheduling policy.
- Only after parity and the absence of performance degradation are confirmed, remove or isolate the legacy Hop/window inference
  path.

## Definition of done

Declare this refactoring complete only when all of the following are true.

- The P4 core's operational path has no Prefill/Decode/UBATCH/rank/KV/model-name policy.
- The llama.cpp adapter handles a single request and a continuous 40-request wave with the same
  scheduler.
- Every stage executes exactly the same physical plan, with no missing or duplicate terminals.
- Even for different GGUF architectures, stage memory is obtained from stock llama.cpp metadata and
  graph without adapter changes, or loading explicitly fails closed.
- The latest source/dist/native provenance matches.
- All 40/40 responses pass every check: terminal, non-empty, prompt conformance and semantic
  correctness.
- The local 3090 23 GiB, local 4080 12 GiB and TUF 4070 8 GiB ceilings and the approved
  power envelope are respected.
- Direct and P4 relay results are identical, and P4 is neither a performance bottleneck nor the owner of phase
  policy.
- Aggregate Prefill/Decode TPS, per-session TPS, latency and GPU utilization are reproducible from the same
  report.

## Related documents

| Purpose | Document |
| --- | --- |
| Detailed reasoning on why the boundary changed | [adapter boundary](adapter-boundary.md) |
| Per-stage tensor/KV/recurrent ownership | [llama.cpp stage memory](llamacpp-stage-memory.md) |
| Current native runtime decisions | [`apps/llama` internals](../../llama/docs/internals.md) |
| Canonical workload and evidence contract | [direct pipeline benchmark](../../../test/benchmarks/direct-pipeline/README.md) |
| Full P4 implementation map | [implementation](implementation.md) |
