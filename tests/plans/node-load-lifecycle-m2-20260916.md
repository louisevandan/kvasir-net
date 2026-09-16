# LOAD/UNLOAD node lifecycle M2 deterministic execution plan

## Sealed scope

The baseline is `094a97e84233cc428f32341e55ea1636c6b1b064`. M2 connects the M1 agent supervisor
to the real retained workers of llama.cpp and HF. Migrating OUTER callers and removing CREATE/DELETE is M3 scope,
and real-model and multi-computer acceptance is M4 scope.

## Advance verdicts

- An internal lifecycle Event has the agent as source and the node as target, and carries the original OUTER return route unchanged.
- The adapter does not change the logical OUTER owner of inference; it only sends the lifecycle terminal back to the same agent.
- A successful LOAD is `Present` after native/child preparation and identity binding; a successful UNLOAD is `Absent` after actual termination.
- A LOAD rejected before any native effect is `Rejected/Absent`; a busy UNLOAD is `Rejected/Present`.
- A cleanup failure or an unknown outcome after start is `Failed/Unknown`; it neither opens the route nor removes the owner.
- Typed completion is bound into the adapter result JSON/IPC body so that readiness, first error and cleanup error are preserved.
- Snapshot strings and inferred process/thread termination are not used as terminal authority.

## Reusing past lessons

- `L001`: no build/model run on this PC. On Spark, use the absolute Cargo path and at most 8 jobs.
- `L002`: real TCP ports use OS assignment or a sealed port outside the dynamic range.
- `L003`: cross-check the source, target, causation and generation of the lifecycle completion together with the original OUTER response.
- `L004`: pin the absolute paths of Cargo, Node and Python, and `HF_TEST_PYTHON`, all at once before running.
- `L005`: only EventNode removes items from the adapter completion front. Do not create a new unbounded channel.
- `L006`: mutations with a different source/binary/hash are recompiled in a separate worktree and target.
- `L007`: do not terminate existing listeners, agents or model processes; reclaim only the test PIDs.
- `L008`~`L013`: check shell quoting, test counts, remote synchronization and moves of owned values before building.
- `L014`: create remote patches with `git diff --output`, and check the LF/CRLF counts and the apply check first.
- `L015`: the remote runner uses `set -euo pipefail` and tools at confirmed absolute paths, and asserts before building that each of the two adapters has at least 1 targeted test.
- `L016`: the blocking first input and the subsequent drain must both use the common `handle_received_input`. Keep the test in which the first LOAD emits a terminal and the original input claim drops to 0.
- `L017`: queue saturation tests keep byte-profile headroom separately, and send the lifecycle input only after confirming output 1 and upstream claim0 for the preceding input.

## Ownership and state table

| Case | typed status | resource state | agent action |
| --- | --- | --- | --- |
| llama/HF normal LOAD | succeeded | present | Open the route after confirming the drain |
| opaque LOAD pre-rejection | rejected | absent | Remove the temporary route and owner |
| Failure after child/native start, cleanup succeeded | failed | absent | Remove the owner after the failure result |
| child/native cleanup failed or unknown | failed | unknown | Keep the owner and fence |
| llama/HF busy UNLOAD | rejected | present | Release the fence; existing work continues |
| llama/HF normal UNLOAD | succeeded | absent | OUTER result after removing the route and owner |

## Sealed tests and rounds

The first round runs together the typed decoders of both adapters, the agent-target terminal, regression of the existing OUTER path,
HF busy/cleanup failure, llama native fixture LOAD/UNLOAD and the retention census.
After that, run M1 real TCP and the workspace with the feature off and on. Timeouts, expected values and capacities are not relaxed after a failure.

| Round | Condition to open the run | Allowed changes |
| --- | --- | --- |
| 1 | `cargo check --tests` passes and the targeted test list has at least 1 test for each of the two adapters | Fix only deterministic defects in code/fixtures |
| 2 | The cause of the round-1 failure is classified as exactly one of code, test or runner, and recorded in the register | Minimal fix for that cause |
| 3 | Round-2 results and the source/target/retention logs match the expected state table | Confirmation run only |
| 4~5 | First add the reason the work did not finish within 3 rounds, plus an automatic preventive block | Exception confirmation within the scope the user allowed |

## Mutation

In an independent worktree, remove the agent-target binding of the real adapter completion, or the typed lifecycle fields.
The targeted real worker test or the M1 agent TCP test must fail. The baseline target is not shared.

## Run results

M2 was completed with the final candidate in round 5. It passed the 3 llama.cpp and 4 HF targeted tests, the workspace with the feature off and on
at 1,523 passed / 0 failed / 7 ignored each, and 3 independent mutations on the final source. `L014`~`L018` were recorded in the
[deterministic execution register](../../docs/deterministic-execution-register.md) together with their automatic blocking measures.
The exact source, rounds, command results and log hashes follow the
[M2 report](../reports/node-load-lifecycle/20260916_032621.md). The next phase is M3 caller migration.
