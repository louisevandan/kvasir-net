# 2026-09-09 — Defect: the control response budget blocked release and settlement

Kind: defect root cause confirmation, fix, consumption-path tests, mutation verification and an on-hardware `pressure` re-verdict. **This is not performance acceptance evidence.**
Started from baseline HEAD `249ff9d7a83` with a clean working tree.
The current work order is owned by the [roadmap](../../../../../../../docs/distributed-batching-roadmap.md).
The previous verdict and run artifacts are in the [measurement trust recovery record](2026-09-09-measurement-trust-recovery.md).

## Symptom

`pressure` (512 requests, resident 256) ran twice on 3090×2 (`m42-server2`), and both ended with the same first error.
Completion was 96/512 and 64/512, and `released_count` was 0 in both.

```
LLAMA_ADAPTER_EVENT_REJECTED / stage control batch total receipt budget is exhausted
```

## Cause

The budget check in `ownership.rs` **reserved `MAX_CONTROL_BYTES` (1 MiB) per item for every new control**.
The cumulative cap `MAX_RECEIPT_BYTES` is 64 MiB, so a control batch is rejected once its width reaches 64, regardless of
the actual response size. The two product callers (`worker/release.rs`, `worker/settlement.rs`) check all sequences of an
event as one batch, so at sequence capacity 256, full-width release and settlement could structurally never pass.

The actual responses are not that large.

| Command | Response contract | Bound |
| --- | --- | ---: |
| `PhysicalRelease` | Must be **byte-for-byte identical** to the request body, otherwise it is fenced (`worker/release.rs`) | request length = 67–70 B |
| `PhysicalSettle` | identity prefix echo + `u32` count + one `i32` per physical row (`control_identity::settlement_reply`, `frontier::validate_continuation_width`) | `physical_capacity × 4 + prefix + 4` |

### The rejected width — confirming the earlier record's “about 64”

The earlier record wrote “about 64 rows” as a prediction computed from the constants and left it unconfirmed. Measured
with real wire commands, the cap accounting **allows up to 63 items and rejects from 64.** This follows from the two
unchanged constants and the unchanged encoder, not from the budget code.

`control_identity::prefix` is `4 + 2 + 2 + 8×3 + 4 + (4+session) + (4+key)` bytes. With session `session` (7 B)
and key `session\0request-N` (17–18 B), a request is 68–69 B. Counting the 1 MiB reservation as well:

- 63 items: `63 × 1,048,576 + 4,337 = 66,064,625 B ≤ 67,108,864 B` — pass
- 64 items: `64 × 1,048,576 + 4,406 = 67,113,270 B > 67,108,864 B` — reject

The test `a_full_width_control_batch_fits_when_each_command_reserves_its_own_bound` confirms the same boundary (63 items
pass, rejection from 64) by execution.

**This calculation is the boundary for this command set when no receipts are retained.** A stage with a longer request
body or with receipts already held is rejected at a smaller count. And **the command kind and batch width actually
rejected in the 09-09 run were not recorded in that artifact.** This value is the width the cap accounting allows, not the
width of that run.

## Fix

Each command declares **the response bound its own contract proves**, and the same value is used in both the per-item
check and the batch-total check.

- `ownership.rs` gains `ControlBudget { identity, operation_id, request, response_bound }`, and
  `validate_control_batch` takes a list of these. `check_control` also gains a `response_bound` argument.
- `release.rs` passes `response_bound = body.len()`. Right below, it fences if response ≠ request, so
  this bound is enforced on the spot.
- `settlement.rs` passes the `physical_capacity × 4 + prefix + 4` it already computed through to the batch check.
  Previously it was used only in the per-item `validate_control_sizes`, and the total check used 1 MiB.
- `commit_control` settles against **the actual length of the arrived response**. The bound is used only for the
  reservation before native execution. A response longer than the bound is fenced before commit (release by echo
  comparison, settle by `settlement_reply` and `validate_continuation_width`).
- Rejection messages carry the values needed for reproduction. The batch check records `member(s)`, `new`, `retained`,
  `reserve`, `need` and `limit`; the per-item check records `slot`, `retained`, `reclaimed`, `request`,
  `response bound`, `need` and `limit`. Callers prefix the command kind and width (`release of N sequence(s) for session S: …`,
  `settlement of N sequence(s) at physical capacity C: …`).

The caps themselves (1 MiB, 64 MiB) were not raised, and resident was not lowered. The common core's budget structure was
not touched either. What changed is **how the size** the adapter reserves is computed.

## Correction — what `released_count=0` means

The earlier record used `released_count=0` to state that “release never ran”. **That assertion was wrong.**
The head **actually performs its own stage's release** with `release_stage_sequence` in `CommittedEffect::Release` in
`worker/effects.rs` and then forwards to the next stage. That path uses a per-item check for a single sequence, so it does
not hit the batch-total check. What got blocked was the batch path of the later stages.

The correct statement is therefore **“release completion across all stages was not confirmed”**,
not “release did not start on any stage”. The work snapshot carried by the UNLOAD rejection is also
the state of that one node only. That artifact cannot show how far each stage progressed.

## Tests

All three new tests start from a failing counterexample.

| Test | Location | What it pins down |
| --- | --- | --- |
| `a_full_width_control_batch_fits_when_each_command_reserves_its_own_bound` | `ownership.rs` | With the production caps (1 MiB/64 MiB) and real wire commands, full-width release and settlement at sequence capacity 256 pass, and measuring the same batch with the cap accounting admits only up to 63 items |
| `a_release_batch_its_bounds_afford_is_admitted_at_the_consumption_path` | `stage_tests.rs` | On the real consumption path (`fixture.handle`), with a budget that covers each command's bound but not the 1 MiB accounting, every member executes natively and a retransmission is replayed |
| `a_settlement_batch_its_bounds_afford_is_admitted_at_the_consumption_path` | `stage_tests.rs` | Same as above, for the settlement path |

Two existing tests were updated together because the contract changed.

- `aggregate_..._receipt_budget_refuses_before_the_first_native_effect` (two: release and settlement):
  the rejection verdict is rewritten against **the sum of each command's contract bound** instead of 1 MiB. The rejection
  itself and the “consumes nothing” check are unchanged.
- `total_receipt_budget_is_checked_before_new_native_controls`: pins down the new division of labor, where the gate
  before native execution rejects on the bound and `commit_control` rejects only on the arrived response length.

Overall `cargo test --workspace --no-fail-fast --locked`:
**1,367 passed / 0 failed / 7 ignored** (3 new tests over the previous record's 1,364; still 0 failures).

`cargo clippy --all-targets --locked` emits no warnings for the four files changed here.
`rustfmt --edition 2024` was applied to `ownership.rs`. The repository still has unformatted files unrelated to this
change, so `cargo fmt --all` was not run.

## Mutation verification

Performed in a separate detached worktree (`F:/dev/p4-mutation-receipt`, with its own `target/`), not the user's working
tree, and each round's actual recompilation was confirmed by sha256. The mutations restore the pre-fix accounting.

| Round | Mutation | Test binary sha256 | Result |
| --- | --- | --- | --- |
| baseline | none | `d3400351f7c2486c…5546655c` | 515 passed / 0 failed |
| M1 | Reserve 1 MiB in the per-item check only | `143c17b303c60677…d9efe45a7` | 513 passed / **2 failed** |
| M2 | Reserve 1 MiB in the batch total only | `2906595fee880ada…eb9a173ccb` | 512 passed / **3 failed** |
| M3 | Both — the accounting of `249ff9d7a83` | `e0dafaa81e0ba318…3405c9ec79` | 511 passed / **4 failed** |

- M1 failures: `a_settlement_batch_..._consumption_path` (the first sequence executes natively, then the second is
  rejected) and `total_receipt_budget_is_checked_before_new_native_controls`.
  The release arm passes M1. The per-item check alone does not reach the cap at a width of 2 items,
  which is why a separate batch-total check is needed.
- M2 failures: all 3 new tests. The rejection message for the full-width batch is
  `256 member(s), 256 new, retained 0 B, reserve 268453266 B, need 268453266 B, limit 67108864 B`;
  the requirement is exactly 4 times the cap.
- M3 failures: the 4 above.

## On-hardware re-verdict — 3090×2 `pressure`

Only `p4-agent.exe` built from fix commit `76d9bc3e3` (sha256 `eaa153f4231385ce…eaad291cd5b60`) was redeployed
to the remote, and the same scenario ran twice. **The staged server, DLLs and launcher have the same hashes as in the
failing run.** So the only thing that changed is the adapter binary.

| | Failing control `20260909T024957Z-da3b12c7` | A `20260909T034439Z-4ce8e2b1` | B `20260909T035149Z-9014d441` |
| --- | --- | --- | --- |
| Commit | `71fea12e2` (clean) | `76d9bc3e3` (clean) | `76d9bc3e3` (clean) |
| `p4-agent.exe` sha256 | `7da7a265f2e43749…` | `eaa153f4231385ce…` | `eaa153f4231385ce…` |
| `p4_staged_server.exe` sha256 | `b66beffb479afa6d…` | same | same |
| launcher sha256 | `2cdfc22d16614452…` | same | same |
| `request/completed/released` | 512 / 64 / **0** | 512 / 512 / **512** | 512 / 512 / **512** |
| `error` | `stage control batch total receipt budget is exhausted` | `null` | `null` |
| `cleanup_error` (idle UNLOAD) | `unload is busy; active_owners=256` | `null` | `null` |
| Elapsed | 144.1 s (aborted) | 267.959 s | 254.886 s |

Both passing runs have `passed=true`, structural 512/512/512, session key match 512/512, 0 rejections and
`agent_stopped=true`.

**Slot reuse is also in this artifact.** A's 512 `release_member` entries use **256** physical slots 0–255,
exactly **2** per slot, with incarnations 1–512 that never overlap. So slots released by an earlier wave were taken again
by a later wave under a new incarnation, and all 512 requests completed through release.

All three invariants of the two pre-fix runs (the first error, `released_count=0`, and owners and frontiers remaining
while in-flight work is 0) are gone.

### What this run verified — RELEASE and SETTLE are distinguished

In this `pressure`, **Verify/Replay rows are 0 in both runs** (the per-request sum of `verify_rows` and `replay_rows` is 0,
`spec-type none`), and the artifact has no settlement record. Therefore:

- **RELEASE**: full-width completion, slot reuse and idle UNLOAD are proven on hardware.
- **SETTLE**: this run did not exercise that path. The evidence stays at the level of the consumption-path test above
  (`a_settlement_batch_..._consumption_path`) and mutations M1 and M2.
  An on-hardware verdict in a scenario where speculative execution produces settlements is still pending.

### What this run does not show

- **This is not performance acceptance.** A/B `generation_tps` is 382.15 and 401.75, physical batches are 1,367 and
  1,281, and rows per batch are 81.65 and 87.13. The pre-fix `pressure` never completed, so **there is no baseline to
  compare against.** These values are not cited as an improvement. The scenario also splits gemma-4-E2B across 4 nodes and
  offloads most layers to CPU, so it cannot be placed in the same column as the 35B baseline.
- `meaning` 512/512 is the `judge.mjs` heuristic, not semantic acceptance (a limitation owned by roadmap §0.0).
- This is not multi-physical-computer acceptance evidence. The two 3090 cards are in one host.
- The previous agent on the remote was backed up as `p4-agent.exe.20260909-pre-receipt-fix`.

## Remaining

- The command kind and batch width actually rejected in the 09-09 failing run. They are not in that artifact; the new
  message records them from the next rejection on. **They could not be confirmed in the post-fix runs** — because those
  runs passed, not because the values are unobtainable. Further reproduction is possible by adding diagnostic
  instrumentation to an independent copy of the pre-fix source.
- On-hardware verdict for the SETTLE batch path. See §"What this run verified" above.
- Pending count, byte and token budgets on the acceptance path (B2/B3) are outside the scope of this change. The
  acceptance code in `worker.rs` still states that reserving request storage is future work. Raising resident comes after
  that budget.
- The template mismatch in `prefill_mix_35b_2stage` and the sustained 8R condition are not the subject of this document.
