> Historical record: requirements, status and measurements from the time of the standalone repository. The current layout and usage follow the [HF guide](../../../README.md). The complete original is in the Git bundle preserved during the migration.

> 2026-09-14: P4 integration is being implemented under the user's §0 instruction. For the current status of the earlier unconnected/read-only descriptions, the [integration specification](../../integration/README.md) and its acceptance report take precedence.

# Verification plan

Status: the full product acceptance tests below are not complete. Verification of the standalone Qwen model scripts follows
the [Qwen plan](../../../tests/plans/qwen3_5_0_8b-20260913.md) and the [run report](../../../tests/reports/qwen3_5_0_8b/20260913_220709.md).
The lower-level local IPC framing tests were also run.
Their scope and results are in the [plan](../../../tests/plans/framing-20260913.md) and the
[report](../../../tests/reports/framing/20260913_174516.md). They are not counted as a full WIRE-01 pass.
For the initial document checks, see the [initialization record](bootstrap-evidence.md).

## Separating comparison baselines

| Baseline | What to check |
| --- | --- |
| Manufacturer high-precision run | Original semantics and normal-response quality |
| Non-distributed run of the same quantization | Error, quality, kernels and memory of the quantization itself |
| Single-process stage split of the same quantization | Partial forward and cache/index/cut errors |
| Multi-process/multi-host run of the same artifact | Additional impact of codec, ordering, settlement and device combination |
| Equivalent comparison arm on existing llama.cpp | Real alternative comparison; state format differences and keep it separate from distributed error tests |

If the model is too large for a single-device baseline run, state the offload/multi-device run of the verified reference explicitly.
Do not pass with a small model only and then approve correctness for the target very large model.
Pin the allowed logits error, quality drop and performance/SLO criteria in the model manifest before testing.
Small numerical differences in sampling can make tokens diverge, so use logits/state comparison together with semantic evaluation of responses.

## Planned test register

| ID | Real consuming path and counterexample | Required evidence |
| --- | --- | --- |
| REF-01 | Baseline run with the same tokenizer/template/stop as the manufacturer's example | inputs, outputs, logits, version/hash |
| Q-01 | Compression applied to all target modules, and excluded modules | Actual module/kernel, weight/metadata bytes |
| Q-02 | Detect a fallback that fully restores at the first forward | load/first run/long-context peak, actual dtype, explicit rejection |
| Q-03 | Calibration and independent evaluation, quality versus original | data digest, full response text, pinned judge, error |
| Q-04 | Absence of non-assigned weights/state in partial stage load | tensor/allocator list, peak, rejection of missing metadata |
| MOD-01 | prefill/decode parity of a single-process split | Multiple lengths, cache position, logits/state comparison |
| MOD-02 | Illegal cuts across shared KV/recurrent/tied/MoE | Rejection before any effect, and reference parity for legal cuts |
| BR-01 | Real consumption of retained event Full/Closed/peek/take | Original/reservation ownership preserved, no double consumption |
| BR-02 | worker crash/IPC timeout/partial tensor | First error, uncertain, leftover state, cleanup result |
| WIRE-01 | dtype/shape/length/version/identity corruption | 0 effects on native execution, output and state |
| LIFE-01 | Full LOAD/SESSION/request/release/UNLOAD flow | Actual state/memory return and a following normal request |
| LIFE-02 | Late completion/RELEASE after cancel, EOS, limit or timeout | New incarnation protected, reservations preserved/returned |
| SET-01 | Wrong membership/receipt, output send failure | 0 output before ledger approval, effect pending/uncertain preserved |
| BATCH-01 | Different lengths, mixed generation/prefill, mid-flight joins | Same membership, order, progress and limits per stage |
| DIST-01 | Target model run on at least two real physical computers | host identity, stage hash, normal responses, return |
| HET-01 | Connecting stages with different devices/quantization recipes | boundary dtype, overall quality, actual kernels, memory |
| WAVE-01 | Repeated heavy continuous waves, long/short prompts, long context | Normal responses, TTFT/ITL, useful TPS, fairness, resource limits |
| INT-01 | Future real P4 entrypoint consumption and existing adapter regression | Pinned revisions on both sides, build, neutrality, real event tests |

Tests for unsupported model features state an N/A reason. Required tests are not hidden behind a feature/ignored to claim a pass.
HET-01 is required when claiming heterogeneous support. INT-01 runs after P4 change permission and the integration stage are reached.

## Failing counterexamples and mutations

Before a functional fix, obtain a failing counterexample and reproduce it on the real worker/bridge consuming path.
Mutations that remove the fix or a key check run only in an independent copy or a verification checkout.
Record source/binary/environment hashes and whether recompilation happened, to exclude baseline reuse from a shared cache.
Do not relax inputs, the judge, tolerances or limits after the fact to make a failure pass.
Python import/bytecode and the worker execution path are also bound to the mutation source.

## Performance formula and records

- Only generated tokens of requests that passed the normal quality and completion conditions in the fixed measurement window count in the useful TPS numerator.
- The denominator is the declared total service measurement wall time. Idle/wait time between waves is not arbitrarily removed either.
- Keep raw TPS and useful TPS separate, and report every failed, incomplete, length-stopped, quality-rejected and unclassifiable request.
- TTFT distinguishes scheduled send, actual send, acceptance and first-token times, and ITL is computed from actual consecutive output intervals.
- Seal the model, quantization, kernel, host/cut, batch/context, sampling, workload and warmup/measurement boundaries.
- Align the GPU observation window with the measurement arm, and look at transfer/compute/wait and the actual batch population together.
- The best result is "the best among tested conditions"; no optimality is claimed outside the explored range.

## Evidence preservation for failed runs

On timeout/crash, clean up the owned workers and resources within a stated limit and preserve whatever partial results are possible.
Distinguish the request text, full output text, token count, terminal/release, first error, cleanup error and evidence_missing.
Do not let an artifact copy failure delay run cleanup indefinitely or overwrite the original failure record.
The final report includes the run commands, exit codes, full summary, not-run/ignored/failed tests and the source baseline.
