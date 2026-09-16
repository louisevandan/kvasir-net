> Historical record: requirements, status and measurements from the standalone-repository period. For the current layout and usage, follow the [HF guide](../../../README.md). The full original is in the Git bundle preserved during the migration.

# Initial development execution plan

Written: 2026-09-13. Status: breakdown of implementation work. Deliverables not separately marked as implemented are planned.
Phase status is owned by the [roadmap](roadmap.md), test definitions by the [verification plan](testing.md), and whether items are settled by
the [decision record](decisions.md). Completion of each phase is judged by real test evidence.

Follow-up after development started: the standalone IPC of S4a was implemented first, and after the user specified the model a [Qwen-specific script](../../models/qwen3_5_0_8b/README.md) was added.
Node planning, partial load, stage forward and the local worker are implemented; no generic model framework is built.
All implementation follows the [per-role folder rules](../../structure/README.md).

## Confirmed starting point

- HEAD of this repository: `2ddd508a19f58921cfcbd6956d192565877eaa55`; no dirty state before the plan was written.
- When the plan was written, the only tracked files were documents and Git settings; there were no Python/Rust packages, CLI, tests or model implementations.
- P4 re-check HEAD: `484b856ee7e53aea5b850b654c45da53cb0724a6`; pre-existing dirty state present.
- In that P4 commit, `RetainedNodeAdapter`, node create and the adapter/agent Cargo files have
  no diff against the initial reference `d122125bafeaa6d32790761669f1bfa5868d8078`.
- node create creates only `llamacpp`. The actual retained consumption implementation is
  `layers/agent/src/event_node/retained.rs`, re-exported from `mod.rs`.
- This plan is based on local document and code observation. Upstream library support, versions and device compatibility are re-verified in S1.

## First development goal and order

Writing dedicated Python code for each model is the center of development. For the first model, the loader, forward, cache/state,
splitting and quantization are implemented directly; a generic processor, model registry or common model interface is not built first.
Keeping the P4 connection and the event/resource ownership contract is a separate matter from generalizing model execution.

The goal of the first development bundle is **a single-process runner that loads only the assigned layers of a chosen real model, runs prefill and repeated decode,
and can compare logits and state against a non-distributed baseline with the same quantization** (S1~S3).
Quality against the original and real compressed execution are proven separately. First pin down whether the model can run partially, then
extend to the Rust bridge, IPC and multiple hosts.

`S1 fix combination → S2 baseline/partial load → S3 partial forward → S4 local bridge/worker → S5 physical distribution/continuous requests → S6 P4 integration`

For S1~S3 the default proposal is contiguous-layer PP, one finite execution step at a time, and an explicit state owner.
4bit, a specific model, a specific OS and the IPC transport are not treated as values the user has confirmed.

## Decisions needed to start

| Input | Required content | What is possible without it |
| --- | --- | --- |
| Target model | exact HF ID, desired weights or the original, text/required feature scope | manifest fields and P4 boundary audit |
| Execution devices | physical hosts to use, GPU/memory, OS, RAM, network, scope of access and execution permission | standalone local contract design |
| Target workload | context, concurrent requests, max output, correct-response criteria, whether quality/latency/throughput comes first | design of evaluation items and record format |

Model revision, vendor code revision, kernel/package compatibility combinations, legal cuts and concrete resource budgets are
investigated by the developer from these inputs. The user does not need to decide every technical detail in advance.
Tolerance, quality and SLO numbers are fixed before the first measurement, and a run with values still undecided is not labelled an acceptance test.
The target model is never substituted arbitrarily, and P4's fleet is not automatically used as this project's test devices.

## S1 — Fix the execution combination and verification contract

1. Following the vendor forward/generation code of the target revision, write a module and state map.
   Trace embedding, position/RoPE/mask, layer index, cache mutation, norm/lm_head and sampling.
   Determine legal and illegal cuts for the structures that actually exist, such as shared KV, recurrent state, tied weights and MoE.
2. Survey public quantized artifacts first and compare them with conversion candidates based on the original.
   Record module coverage, auxiliary tensors, execution kernels, device constraints and how to obtain the original reference in a table.
3. Compute per-device weight/metadata/KV/state/activation/workspace/IPC buffer sizes and the load peak.
   For the reference run of a large model, specify a verifiable offload/multi-device configuration.
4. Pin the Python/PyTorch/Transformers/quantization dependency combination confirmed from official material and the chosen sources.
   Also record the P4 reference revision for the Rust bridge and the single-crate-source principle.
5. Write the fixed execution spec and minimal verification code for the first model. Distinguish settings under investigation from the sealed manifest, and
   check for missing identity, unsupported combinations, budgets and cuts. Do not build a generic schema engine that expresses many models.
   Model tensor/state checks are implemented in S3, and frame/shape/byte checks of external input in S4, on the actual consumption path.

Planned deliverables: the first model's execution spec in `manifests/`, audit results in `docs/models/<model>.md`,
minimal execution settings and verification code in the first model's Python package, and a Python dependency lock.
Exit condition: model/device/kernel candidates have source evidence, and revision, cut, evaluation inputs, tolerances and execution limits are fixed.
Support verdicts in this phase are static compatibility evidence; approval of real compressed execution happens in S2.

## S2 — Baseline and real partial load

1. Build a reference runner that uses the same tokenizer/template/position/stop as the vendor run (REF-01).
2. For the same input, record logits, full output text, state, kernels and memory for the high-precision run and the quantized run (Q-01~03).
3. Implement a loader that reads only the weights and the scale/zero point/group/packing metadata the stage needs (Q-04).
   Do not load the whole model onto the GPU first and then delete the other layers.
4. After the first forward and long-text/max-batch runs, confirm that compression is preserved and check the memory peak.
   If execution falls back to a full dense restore instead of the required kernel, reject LOAD for performance use.

Planned deliverables: `models/<model>/loading/`, `quantization/`, `reference/`, and results in `artifacts/<run-id>/`.
Exit condition: REF-01, Q-01~04 and the quality criteria against the original are met. If the kernel or partial load does not work,
fix the S1 recipe and re-verify with a new manifest. Do not push the problem on to the distributed phases.

## S3 — Single-process partial forward

1. Separate, per model, the first stage's input preparation, the middle stages' operations and the last stage's norm/lm_head.
2. Implement per-request cache/state, the global layer index, position and the boundary tensor schema.
3. Feed the same input tokens to the non-distributed run and to the stage-split run, and compare logits and state for prefill and repeated decode.
   Then also evaluate free-generation responses to separate numeric error from the effect of token divergence.
4. Verify several lengths, padding, cache progress and the max-context boundary, as well as legal and illegal cuts (MOD-01~02).

Planned deliverables: `models/<model>/forward/`, `state/`, `boundary/`, per-role parity tests and a cut verification report.
Exit condition: parity with the same-quantization baseline within the fixed tolerance, absence of weights/state not assigned to the stage,
and rejection of illegal cuts before execution. This is the completion point of the first development bundle.

## S4 — Standalone Rust bridge and Python worker

| Order | Implementation bundle | Completion evidence |
| --- | --- | --- |
| S4a | worker Inspect/Load/BindSession/ExecuteStep/Release/Unload, bounded IPC and tensor codec | round trip with a real separate Python process, WIRE-01, normal LIFE-01 |
| S4b | `crates/p4-hf-adapter/` retained trait, ledger, output approval, standalone event test host | BR-01 and SET-01 on the P4 neutral node/broker consumption path |
| S4c | Cancel/Quiesce, generation fencing, crash/timeout/partial frame, cleanup and re-acceptance | BR-02, LIFE-02, LIFE-01 after a failure, fix-removal/mutation verification |

Choose one IPC method based on the OSes supported in S1 and the tensor sizes from S3, and define length limits and queue/byte reservations first.
Keep stdout logs separate from wire frames, and do not put pickle or raw GPU pointers on the wire.
The initial worker starts with serial device execution and records acceptance, device completion, settlement, output and state return separately.
A late completion/RELEASE must have no effect on a new incarnation.

The standalone test host lives in this repository. P4's neutral crates are referenced at a pinned revision, and
we confirm that mixing path and Git sources in one build does not duplicate types. This repository owns the target, lock and environment.
Mocks help with error injection but do not replace verification of the real worker/model consumption path.

## S5~S6 — Distributed acceptance and product integration

| Order | Work | Entry/exit condition |
| --- | --- | --- |
| S5a | fixed stages, single-request PP on at least two real physical hosts | device/execution permission secured, DIST-01, target model quality and return |
| S5b | different lengths, mid-way joins, prefill/decode mix, identical membership across all stages | BATCH-01, limits respected, re-acceptance after cancellation |
| S5c | long and short texts, long context, strong continuous waves, the heterogeneous recipe combinations to be claimed | WAVE-01, HET-01 where applicable, fixed quality/SLO and resource limits |
| S6 | kind registration, dependencies and worker deployment wiring in the P4 composition root | after a separate integration instruction: INT-01, existing adapter regression, real product consumption |

In S5, inter-node tensors pass through the P4 event delivery and backpressure boundary as adapter payloads.
The Python worker does not bypass this through a separate direct send path. Optimizations are chosen from measurements after correctness passes.
Performance follows the whole-wall-time denominator and the correct-completion token criteria of the [verification plan](testing.md).
Include failures, incomplete runs, length stops, quality drops and unclassifiable results, and record GPU/transfer/wait over the actual measurement interval.

## Change units and resume records

- For each implementation bundle, leave evidence in this order: related failing counterexample → implementation/verification on the real consumption path → mutation on an independent copy.
- The first commit bundles are separated in the order S1 combination/manifest, S2 reference/loader, S3 stage parity,
  and each is split into small restorable units. The same principle applies later to S4a~c and S5a~c.
- At each restore point, leave the source/environment/artifact hashes, the actual commands, exit codes and test results, the first error, cleanup errors,
  evidence_missing, open items and the first next action. Raw artifacts go to ignored paths; reproduction contracts and summaries go into Git.
- Phase pass status is updated only in the roadmap; HANDOFF links the next run to the relevant records.
- No schedule or performance numbers are promised before the target model/devices are fixed. After S1's model audit and reference resource estimate,
  estimate the S2~S3 effort and the device acquisition schedule.

First next action: secure the three inputs above and start with S1's table of model operations, state and quantization coverage.
