# OUTER acceptance test three-turn plan

> Document status (2026-09-06): **Partial test plan**. This is the plan for the mock/OUTER path in question. It does not replace full real-hardware acceptance of very large models.
> Current goals, status and ordering follow the [execution roadmap](distributed-batching-roadmap.md); document authority and reading paths follow the [document map](document-map.md).

This document pins the order and verdict criteria of the acceptance tests for OUTER sessions, cancel, restore and the KV
lifecycle. Do not add test cases ad hoc or change their order. Only three turns in total
are allowed.

## Change and commit rules

| Turn | Allowed changes | Commit boundary | Test scope |
| --- | --- | --- | --- |
| 1st | After committing the plan document, fix only defects found by the acceptance tests | plan commit → defect fix commit | All core contracts and interactions |
| 2nd | Fix and re-verify 1st-turn defects. No new structures or new scenarios | 2nd-turn defect fix commit | Reproduce 1st-turn failures + full regression |
| 3rd | Only fine-tuning, removing flakiness and improving logs/verdicts | final fix commit | Final smoke and full gate |

In each turn, record `git status`, binary hashes, ports and the state root before testing.
Store test outputs in `target/outer-acceptance/<turn>-<run-id>/`.

## Isolated topology

To avoid colliding with the llama.cpp test session, the OUTER acceptance tests use separate processes and
state directories.

```text
OUTER driver :52102
        │
agent A :52100 ── agent B :52101
        │              │
   mock stage A   mock stage B
```

- adapter: `mock` or `mock-instant`
- agent: two `p4-agent.exe` processes
- OUTER: a separate `p4-drive.exe` process
- state: `target/outer-acceptance/<turn>-<run-id>/state-*`
- logs: agent stdout/stderr, drive stdout/stderr, process exit codes
- If a port is in use, do not stop the existing process and do not start another
  run. Record the collision and treat that run as failed.

## Parallel run bundles

Each bundle uses its own agents, ports, state root and deployment, so in the 1st and 2nd turns they
run concurrently. The order of steps inside a bundle is fixed.

| Bundle | Verification axis | Key evidence |
| --- | --- | --- |
| A | Connection, reconnect, generation | stale ACK/event rejection, new generation acceptance, journal replay |
| B | Request order, restore barrier | Restore order within the same sequence, Hop blocked until Restore completes, partial residency rejection |
| C | Cancel, terminal | queued cancel, active hop boundary, duplicate Cancel, terminal exactly once and replay |
| D | KV transaction | 2-stage/4-stage Persist/Restore/Discard, compensation on commit failure, restart recovery |
| E | Lifetime policy | heartbeat miss threshold, retain-until boundary, duplicate GC candidate removal |

Bundles A–E run in parallel, but no two bundles share the same agent or state
root. Parallel run results are merged into per-bundle manifests.

## 1st turn: confirm all contracts and logical relationships

### 1st-turn pre-gate

1. Record the commit id of the plan commit
2. `cargo fmt --all -- --check`
3. `cargo clippy --workspace --all-targets -- -D warnings`
4. `cargo test --workspace`
5. Record the hashes of the release `p4-agent.exe` and `p4-drive.exe`
6. Confirm that the reserved ports are free and that the existing llama.cpp session is alive

If the pre-gate fails, do not start the acceptance tests.

### 1st-turn cases

Each case checks the request ID, sequence ID, operation ID, route, return channel
and ingress generation against the logs.

| ID | Scenario | Relationship that must be confirmed |
| --- | --- | --- |
| A1 | Receive event and ACK after connecting | channel + stream + event sequence |
| A2 | Reconnect after a disconnect | ACKs/events of the previous generation do not intrude on the new connection |
| A3 | Two Restores on the same sequence | Publish order matches adapter execution order |
| A4 | Follow-up Hop during Restore | Hop does not run before all stages complete |
| A5 | Restore fails on one stage | A partial resident is not published as executable |
| A6 | Cancel a queued request | Only the target request is terminated |
| A7 | Resend the same Cancel | No additional terminal created, same result replayed |
| A8 | Cancel an active hop | Does not claim a forced stop; terminates after the boundary |
| A9 | 4-stage Persist → restart → Restore | Each shard and position preserved |
| A10 | One stage fails during commit | After abort/reconcile, does not pretend to be in a success state |
| A11 | GC just before and after the expiry boundary | `now < retain_until` retained, `now >= retain_until` discard candidate |
| A12 | Different sequences concurrently | Different sequences run in parallel, the same sequence keeps order |

### 1st-turn verdict

- If any of A1–A12 fails, stop the tests immediately and preserve the failure manifest and
  related logs.
- Fix the implementation after root-cause analysis. Limit the fix to code that directly explains the failure and
  the corresponding regression test.
- After the fix, create the `outer-acceptance-1-fix` commit.

## 2nd turn: reproduce 1st-turn defects and full regression

The 2nd turn first reproduces the cases that failed in the 1st turn with the same inputs. If they cannot be reproduced,
the fix is not judged complete. Then rerun all A–E parallel bundles and `cargo test --workspace`
.

Verdict criteria:

- 0 1st-turn failure cases
- 0 across all of A1–A12
- 0 cross-talk between parallel bundles
- 0 missing or duplicate terminals
- 0 leftover agents/drives/listeners after shutdown
- 0 regressions in existing tests not changed in the 1st turn

If a new failure is found, only a 2nd-turn fix commit is allowed. Do not widen the test scope or
change protocol semantics.

## 3rd turn: final fine-tuning

The 3rd turn keeps the contracts that passed in the 2nd turn and does only the following.

- Stabilize timing margins and polling
- Fill in missing fields in logs and manifests
- Deterministic ordering and removing causes of flakiness
- Final format, Clippy, diff and workspace test

If the 3rd turn needs a meaningful protocol, state or scheduling change, it is not accepted as the final version
and is reverted into a separate change.

## Final outputs

The final commit must include all of the following.

- 1st/2nd/3rd-turn manifests
- pass/fail and process exit codes per bundle A–E
- agent/drive log paths
- executed binary hashes
- list of ports and state roots
- the ignored real llama.cpp E2E and the reason
- implementation status update in `protocol-outer.md`

Real llama.cpp adapter tests do not replace this plan. The OUTER contract is settled first with the Mock
adapter, and the llama.cpp session verifies it in a separate acceptance that connects the same wire, cache and cancel inputs
to a real backend.
