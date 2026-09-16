# 2026-09-09 — Defect: a failed run lost its own partial results

Kind: defect root cause confirmation, fix, real consumption-path tests and mutation verification. This is not performance evidence.
Started from baseline HEAD `352bdb092` with a clean working tree.
The current work order is owned by the [roadmap](../../../../../../../docs/distributed-batching-roadmap.md).

## Symptom

In the 2026-09-09 saturation experiment, **the 4 failed runs left not a single `artifact.json`.**
Those 4 runs are not of the same kind.

| Run | drive error | Classification |
| --- | --- | --- |
| `20260909T055821Z-151f5b02` | `node event error … node-0` | 2-stage **load** rejected |
| `20260909T065649Z-e6c5c4c4` | `node event error … node-0` | 2-stage **load** rejected |
| `20260909T060144Z-0a42617d` | `node already exists` | Duplicate node |
| `20260909T061603Z-6cadb520` | `inference observation evidence Missing { requests: 256, stage_executions: 8 }; receive failed: event stream ended mid-frame` | 4-stage **inference** aborted |

Only the last one aborted during inference, and **its drive error is `event stream ended mid-frame`.**
`os error 10054` is a **separate observation** in the same run's agent log (`P4_EVENT_CONNECTION_STOPPED`),
not something drive saw.

The artifact's `delivery.counted = 583` is also not the number of OUTPUTs received.
`counted` in [`delivery.mjs`](../../../../../../../test/benchmarks/p4-4node/delivery.mjs) is
the maximum running total of `P4_EVENT_OUTER_MISSING discarded=`, i.e. **the number of events discarded because they could not be delivered to OUTER**.
So **it is still unknown how far that run got in accepting results.** The discard count alone must not be used
to settle the first cause of the disconnection either.

That is exactly this defect. Because nothing accepted survives a failure after inference has started,
**there is no data to distinguish the first cause among missing observation evidence, stream abort and transport failure.**
A successful retry does not close the issue.

## Cause

`drive` in [`inference.rs`](../../../../../../../tools/event-drive/src/run/inference.rs)
collected requests, outputs, completions, releases, observations and stage spans in local variables, while sending every rejection and transport failure
inside the loop out through `return Err`/`?`. At that moment the local variables vanish entirely.
[`run/mod.rs`](../../../../../../../tools/event-drive/src/run/mod.rs) receives it with `inference::drive(..).await?`,
so it never reaches `assemble`, and the CLI exits without writing an artifact.

The earlier 2026-09-09 fix kept a **teardown failure** from erasing the run. Failures of inference itself were unchanged.

## Fix

A failure after inference has started is **a result, not the absence of a result.**

- The receive loop of `drive` is wrapped in an async block. The `?` and `return Err` inside the loop stay where the checks
  belong, and that error becomes **this run's first error**. If a node already reported an error,
  that one is kept (`get_or_insert_with`).
- Only the checks before submission (request count range, `InferenceIdentity::new`) still return `Err`. At that point nothing has been collected.
- The arithmetic on release receipt was moved after the check. An event that is going to be rejected no longer leaves the `released` count incremented.
- `RequestArtifact` gains `submission` (`delivered` / `uncertain`). A write failure may still have arrived,
  so it is **uncertain**, not "unsubmitted", and the identifier is consumed either way.
- `RunArtifact` gains `evidence_missing` and a `submissions` summary (configured/delivered/uncertain/
  unsubmitted/incomplete/unreleased). A run that stopped because evidence never arrived and a run that received everything
  and rejected something are different failures.
- **Attribution follows the evidence, not the verdict.** When the observation evidence is complete, `apply_counts` is applied
  whether the run failed or releases remain outstanding. If the evidence proves per-request row counts but they are reported as 0, that records a value nobody
  measured, and a 0 remains that needs explaining even though `evidence_missing` is `null`.
  So `evidence_missing` is the contract with the reader — if it is `null`, the row counts are attributed final values;
  if it has a value, they are unattributed, and a 0 means there is no evidence, not that no work happened.
  **What was valid is preserved, but nothing absent is fabricated.**

The rejections themselves were not relaxed. Invalid events are still rejected and the run still ends as a failure,
and `main.rs` exits with code 1 on `passed=false`.

## Tests

Faults are injected into the real consumption path of `consumer_budget_boundary_tests` (a real `EventWire`, and a peer that verifies
real PREFILL submissions). The injection point was chosen as **after all OUTPUTs are delivered and before the first RELEASE**,
because it has something to preserve and lets the three faults be compared at the same place.
**There is no evidence that the 09-09 on-hardware failure broke at the same point** — as noted above, how far that run got in
accepting results is unknown because no artifact survived. These are valid synthetic counterexamples, not historical reproductions.

| Test | Injection | What it pins down |
| --- | --- | --- |
| `a_cut_connection_after_the_outputs_keeps_them` | The peer cuts the connection | `receive failed` is the first error, and 2 completions, 0 releases, the responses and the observations survive |
| `an_expired_deadline_after_the_outputs_keeps_them` | The peer stalls and the run's own deadline expires | Reported as `overall deadline expired`, and the same things survive |
| `an_invalid_event_after_the_outputs_keeps_them_without_accepting_it` | The peer sends an unknown content type | The rejection is the first error, and **the rejected event is not accepted** (less evidence than a normal run, spans 0) |
| `a_refused_unload_after_a_broken_run_reports_both_and_still_fails` | UNLOAD rejection on top of the cut run above | `error` and `cleanup_error` each survive, the artifact is produced, `submissions` is exact, and `passed=false` |
| `complete_evidence_attributes_rows_even_when_the_run_failed_with_a_release_outstanding` | The existing case where release is rejected but evidence is complete | With complete evidence, row counts are attributed regardless of failure or outstanding release; they stay 0 only when evidence is missing |
| `a_wave_that_fails_mid_write_separates_delivered_uncertain_and_unsubmitted` | Write failure mid-wave | Splits into delivered 1 / uncertain 1 / unsubmitted 2 |
| `a_run_that_breaks_and_then_fails_teardown_still_writes_its_artifact_and_exits_nonzero` | Local TCP peer + **the real CLI binary** | `execute → teardown → JSON file written → exit code 1` actually happens |

Overall `cargo test --workspace --no-fail-fast --locked`: **1,374 passed / 0 failed / 7 ignored**
(7 new tests over the previous 1,367).

The 89 existing boundary and output-contract tests were updated for the contract change to read rejections from `run.error`.
The rejection targets and messages are unchanged and were not relaxed.

## Mutation verification

Performed in a separate detached worktree (`F:/dev/p4-mutation-partial`, with its own `target/`), and each round's
recompilation was confirmed by sha256.

| Round | Mutation | Test binary sha256 | Result |
| --- | --- | --- | --- |
| baseline | none | `5470617d301f82c8…8d833751` | 89 passed / 0 failed |
| M1 | `outcome?;` — propagate the loop's error as before the fix | `73e4c7ad9cffd87c…ef4dbc25` | 84 passed / **5 failed** |
| M2 | Move `apply_counts` back to its old place (inside the full-release gate) | `689eaccdcd25f898…1f23e480` | 90 passed / **1 failed** |
| M3 | Remove `apply_counts` entirely | `d65a2a5660265453…7ece85d2` | 78 passed / **13 failed** |

M1 failures: all 4 of the new tests above and
`a_future_wave_cannot_extend_the_overall_evidence_deadline_or_send_after_it`.

M2 fails **only one** new attribution test — the old placement leaves the tally of successful runs intact and leaves 0 only in failed or
unreleased runs, so that test catches exactly that regression. M3 removes attribution altogether and brings down 12 existing
tests along with it.

## Remaining

- **The first cause of the 09-09 10054 is unconfirmed.** This fix only ensures that, if the same thing happens again, data to judge it
  survives. It is not marked resolved on the strength of a successful retry.
- The CLI exit code is a single branch in which `main.rs` exits with 1 on `passed=false`, and these tests pin down only up to `passed=false`.
  There is not yet a test that launches the process and checks the exit code.
- Next is the staged server defect in which capacity shrinks because of the recurrent allocation used for planning.
  [Evidence](2026-09-09-saturation-and-utilisation.md)
