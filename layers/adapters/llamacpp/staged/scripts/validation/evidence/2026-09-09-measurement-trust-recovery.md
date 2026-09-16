# 2026-09-09 — Three defects that made performance measurements unreadable, and a correction of retracted evidence

Kind: defect reproduction, fix and verification, plus a documentation correction. **This is not performance evidence.**
It contains no new inference measurement. The current work order is owned by the
[roadmap](../../../../../../../docs/distributed-batching-roadmap.md).

This document covers items 1, 2 and 4 of roadmap §0.7 P-2. Item 3 (the `pressure` re-verdict) is not here.

## Why these were fixed first

The three defects look unrelated, but they have the same effect: **they make the results of a performance experiment unreadable even when it runs.**

## 1. The cited correlation had already been retracted by its source

The issue-width comment in `worker/drive.rs` said that throughput moves together with stage overlap at r=0.891 and moves
against width at r=-0.357. The first version of roadmap §0.7 (`daae4c8fd`) cited this as the rationale for the width
policy.

The source data behind that comment, the [09-03 record](2026-09-03-load-and-batching.md), says right below that table
`Everything in the paragraph above is backwards. See the next section.`, and the next section is titled
`The correlation was reverse causation (2026-09-04)`.

| Controlled experiment preserved by the source | Value |
| --- | --- |
| Interleaved runs with width capped by `P4_STAGED_MAX_ISSUE_ROWS` | 8 runs (cap 0/24/0/24/0/12/0/48) |
| total row/s | 544.25 → 198.10 |
| Stage overlap at that time | 81.1% → **95.4%** (GPU utilization also highest) |
| Correlation after controlling for width | width **+0.898**, overlap **-0.060** |
| Fit | **tail step = 34.2 ms per batch + 1.051 ms per row** |

So the rise in overlap was an effect, not a cause. The busy stages were busy paying a fixed cost over and over.
A later section of the same record had already decomposed that fixed cost: at the tail, **sampling at 46.9 ms is larger**
than `llama_decode` at 40.1 ms, and per row it is 0.11 ms for the transformer versus 0.29 ms for the sampler.

**Action.** Replaced the comment in `drive.rs` with the 09-04 result (`2bc8f93cd`, comment-only change, `cargo check --lib`
passed). Rewrote roadmap §0.7 (`2bc8f93cd`). No retracted citation remains in the code.

Three other statements in §0.7 were corrected at the same time: calling a product of values from different sets an identity;
reading RPC span overlap (31.6%) as concurrent GPU computation; and treating the outcome of an intended policy (0 mixed
prefill/decode batches) as a defect and then computing 191 TPS as the expected gain on top of it. The grounds are,
respectively, the in-code comment at `run.mjs:118` ("These are service spans, not device time") and the
`plan_equal_ordinary` policy at `scheduler.rs:280`.

## 2. Cleanup failure was erasing the run's own failure

### Symptom

`pressure` (resident 256) ended on both hosts with UNLOAD `unload is busy; active_owners=224/256`. But **there was no
artifact file at all**, so it could not be judged whether this was a release leak or a correct rejection after inference
had already broken.

### Cause

The ERROR branch of `run/inference.rs` returns the failure as `Ok(InferenceResult{ error: Some(..) })`.
But `execute` then received the UNLOAD response with `receive_exact(...).await?`. After inference breaks, the node still
holds owners, so UNLOAD is rejected, and that `?` **discarded the entire `InferenceResult`.**
The first error, the requests, the output, the observations and the settlement record all disappear together. The caller
(`main.rs`) writes an artifact only on `Ok`, so no file is left.

### Fix

Split out `teardown` and made its return type **`Option<String>`, not `Result`**.
With no `Result` to propagate, the call site cannot use `?`.

- Writing the original defect back in is a **compile error**. Confirmed:
  `error[E0277]: the ? operator can only be used on Results, not Options`.
- `error` (the run's own first failure) and `cleanup_error` (UNLOAD/DELETE failure) are now separate,
  and the artifact is always assembled.
- **The UNLOAD guard was not relaxed.** A cleanup failure still fails the run.

### Verification

`tools/event-drive/src/run/teardown_preserves_failure_tests.rs`. It drives the real consumption path
(`inference::drive`) up to a node-reported ERROR and then assembles the artifact. It is not a native, model or transport
fixture.

| Mutation | Result |
| --- | --- |
| Merge the two error fields (`cleanup_error.or(run.error)`) | 2 tests fail |
| Remove `cleanup_error.is_none()` from the verdict | 1 test fails (the one with a positive control) |
| Put `?` back at the call site | **No test catches it. That is why it is blocked by the type** |

The last row is the point of this fix. A test can pin down what `assemble` does, but it cannot pin down
**that `assemble` is reached**.

## 3. A child that exited by signal was judged a stop failure

`run.mjs::stopChild` asked `child.exitCode !== null` after `child.kill()`. A process killed by a signal has
`exitCode` null and `signalCode` set. Reproduced in this environment:

```text
exit event: code=null signal=SIGTERM
child.exitCode=null child.signalCode=SIGTERM
old predicate (exitCode !== null) -> false
```

So **every normal stop was reported as a stop failure.** Because `run.mjs:622` fails the whole run on `!agentStopped`,
runs that otherwise succeeded were flipped to failure at the end. This is why the 2026-09-07 record read `agent_stopped`
as "a stopped agent", and that interpretation was wrong.

The fix treats either an exit or a signal as stopped. 4 tests
(`test/benchmarks/p4-4node/stop-child.test.mjs`, using real child processes). The first test catches a mutation that
restores the old predicate.

## 4. Test compilation restored

`issue_witness_tests.rs:409` assigned through `RequestState`, which implements only `Deref`, and failed with E0594.
That broke the whole lib test target, so `cargo test --workspace` exited 101 with 0 tests run.
The copy is now explicit via `input_mut_for_test()`. There is still no `DerefMut`, and the product path is unchanged.

## 5. `pressure` re-verdict — the undetermined cause is now confirmed

Right after the fix, the same scenario ran on 3090×2 (`m42-server2`), and **the fix immediately produced the answer.**

Run `20260909T024542Z-58fbac79` (`target/pressure-remote-20260909/`).
Build `0eadefebd3` + patch set `961bd89cd119` (including 0025). The stage server hash matches the local build, and the
agent was replaced with the HEAD build and its hash checked. 4 stages, sequence capacity 256, 512 requests, 173.9 s.

### This time the artifact survived

Previously the UNLOAD rejection propagated and there was no artifact file at all. This time the two errors were recorded
separately.

| Field | Value |
| --- | --- |
| `error` | `LLAMA_ADAPTER_EVENT_REJECTED` / **`stage control batch total receipt budget is exhausted`** |
| `cleanup_error` | `unload is busy;work={...}` |
| `request_count` / `completed_count` / `released_count` | 512 / 96 / **0** |
| Requests that received `release_member` / requests actually released | 96 / **0** |

The work snapshot carried by the UNLOAD rejection is decisive.

```json
{"requests":0,"pending":0,"pending_releases":0,"pending_settlements":0,
 "prepared_issue":null,"verify_fenced":false,"flight_batches":0,"flight_executions":0,
 "open_batch_view":0,"effects":0,"active_publications":0,"held_input":false,
 "active_owners":224,"active_frontiers":224}
```

**No work is in flight.** Only 224 owners and frontiers remain.
And this 224 is **the same value** as the `active_owners=224/256` left by the preserved 2026-09-07 run.
Those runs could not record their first error, but they end in the same state on the same hardware.

### Reproduced twice

It ran twice on the same host. One run copied the weights to the remote local disk; the other **used the scenario's
original `S:` path unchanged**. The latter matches the baseline configuration.

| | `20260909T024542Z-58fbac79` (D: staging) | `20260909T024957Z-da3b12c7` (original `S:` path) |
| --- | --- | --- |
| `error` | `stage control batch total receipt budget is exhausted` | same |
| `released_count` | 0 | 0 |
| Requests that received `release_member` | 96 | 64 |
| `completed_count` / `request_count` | 96 / 512 | 64 / 512 |
| `active_owners` at UNLOAD | **224** | **256** |
| Other work counters | all 0 | all 0 |
| Elapsed | 173.9 s | 144.1 s |

Completion count and owner count differ between runs. **What is invariant is the first error, `released_count` 0, and
that owners and frontiers remain while all in-flight work is 0.** The `active_owners=224/256` left by the preserved
2026-09-07 run falls in the same range as these two values.

### Causal chain

1. The product callers of `validate_control_batch` are **only two**: `worker/release.rs:324` and
   `worker/settlement.rs:246`. So what hit the budget was **the release and settlement path itself**.
2. That is why 96 requests received `release_member` and `issued_work` but **0** were `released`.
   **[2026-09-09 correction]** This line originally said "release never ran". That assertion was wrong.
   The head actually performs its own stage's release in `CommittedEffect::Release` in `worker/effects.rs` before
   forwarding, and that path uses a per-item check for a single sequence, so it does not hit the batch-total check.
   The correct statement is that **release completion across all stages was not confirmed**. This artifact cannot show
   how far each stage progressed.
3. For every release that did not complete, owner and frontier slots stay occupied.
4. `require_idle_unload` rejects when owners are nonzero. As that function's own comment says, it is
   **the preflight for an "explicit, healthy-worker UNLOAD"**; failure cleanup is a separate path.

**Verdict: the UNLOAD rejection is an effect, not the cause.** It is not abandoned state leaking;
**release itself was blocked by a capacity limit and never got started.**

### That capacity limit

Two constants in `ownership.rs`.

| Constant | Value | Use |
| --- | ---: | --- |
| `MAX_CONTROL_BYTES` | 1 MiB | Reserves the **maximum response** for one control item before native execution |
| `MAX_RECEIPT_BYTES` | 64 MiB | Cap on total accumulated receipts |

Because each row reserves the worst-case 1 MiB response rather than its actual size, **a control batch reaches the cap at
about 64 rows even with zero accumulated receipts.** At sequence capacity 256, full-width release and settlement
structurally exceed this limit.

Per-row rejection itself is intended design and has tests (`ownership.rs:763`, `stage_tests.rs:1608`: it rejects before
any native effect and consumes nothing). The problem is not the rejection behavior but that
**the combination of the two constants is incompatible with resident 256**.

### Not yet confirmed

- The **actual width** of the rejected release batch is not recorded in this artifact. The 64-row cap is a prediction
  computed from the constants, not confirmed against an observed width.
  **[2026-09-09 update]** The cap accounting was measured with real wire commands and confirmed to **allow up to 63
  items and reject from 64** (the boundary when no receipts are retained). The rejection message now carries the member
  count and the required amount.
  **Even so, the command kind and width rejected in this run remain unconfirmed** — the post-fix run passed, so they
  simply could not be checked; it can be reproduced by adding diagnostic instrumentation to an independent copy of the
  pre-fix source.
  The cap accounting itself has been removed.
  [Fix evidence](2026-09-09-control-receipt-budget.md)
- This run stopped with only 96 of 512 requests complete. **No throughput figure is cited.**
- An owner count in the same range as the preserved run means it ended in the same state; it does not prove it took the
  same path. Why completion count and owner count vary between runs is also not yet explained.

### Correction about the remote host

While preparing this run, the remote was observed to be logged out (`explorer` 0, `LogonUI` 1), and that was written down
as the reason SSH could not see `S:`. **That causal claim was wrong.** Checked again while logged on,
`Test-Path 'S:\models'` in an SSH session is still False. Drive mappings are separate per logon session, so logon state does not
matter. This is exactly why the harness uses an interactive scheduled task,
and the existing comment in `remote-agent.mjs` already said so.

## Overall results

| Item | Value |
| --- | ---: |
| `cargo test --workspace --no-fail-fast --locked` | **1,364 passed / 0 failed / 7 ignored** |
| Of which staged adapter lib | 512 |
| Of which `p4-event-drive` | 85 |
| Harness `*.test.mjs` (run per file) | 76 |

The harness was run per file. With Node 26 on this machine, `node --test <directory>` resolves the directory as a module
and fails with `MODULE_NOT_FOUND`. This is unrelated to this change and was the same before the fix.

## Remaining

- The `pressure` re-verdict was performed on 3090×2 in section 5 above. What remains is the actual width of the rejected
  release batch.
- This document makes no new throughput or GPU utilization claim. All cited values such as 34.2 ms and 46.9 ms
  come from the 09-03/09-04 records and from 2B runs. The fixed-cost breakdown for 35B has not been measured yet.
- No new instrumentation is needed. The timer already exists at `server_physical.cpp:34`, and the switch is
  `--step-trace` at `remote-agent.mjs:75`.
