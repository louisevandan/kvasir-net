# LOAD/UNLOAD node lifecycle M3 deterministic execution plan

## Sealed scope

The baseline is `2ee9b4905c7971356f6088876caf346f2ab1acb6`. M3 migrates Rust event-drive,
the Qwen HF OUTER controller and the current lifecycle verification scripts to Agent-target NODE_LOAD/NODE_UNLOAD.
It removes the legacy CREATE/DELETE acceptance branch from the agent event runtime. The separate
`Agent::create_node/delete_node` in the service layer and the dated historical artifacts are not changed in this stage.

## Pre-run verdict

- External NODE_LOAD/NODE_UNLOAD target the Agent and preserve the original OUTER source/return route.
- Only the opaque bytes after the common little-endian lifecycle metadata are in each adapter's existing LOAD/UNLOAD format.
- The result source is the requested target Agent. Verify correlation/causation first, then check the lifecycle metadata's
  node/generation/adapter/operation/status/resource state.
- Build readiness is read only from the opaque adapter result of a successful LOAD. A transport ACK or acceptance is not success.
- If a LOAD in a wave is rejected, OUTER individually sends NODE_UNLOAD to the nodes that already succeeded in the same wave and to the successful nodes of earlier waves.
  P4 does not automatically reclaim another agent's nodes.
- A normal shutdown confirms removal of the node and its native resources from the NODE_UNLOAD result alone. No DELETE is appended.
- Legacy CREATE/DELETE and node-target lifecycle are rejected with no side effects.

## Reuse of past lessons

- Apply all of `L001`–`L020`. No build/model run happens on this PC; runs go on Spark with at most 8 jobs.
  Patches are moved after LF/hash/apply checks.
- `L019`: do not infer the stage from the Agent source of a lifecycle result; bind causation ID→requested node, then
  check the metadata identity again.
- `L020`: both the normal HF UNLOAD and the failure-reclaim abort must go through the Agent lifecycle, and after cleanup
  `owned` and `ready` must be cleared to match the actual result.
- `L021`: all remote preparation, tests and mutations transfer a reviewable LF runner, check its SHA, then run it.
  Do not nest remote `$()` or pipes inside PowerShell.
- `L022`: for new files, compare the diff SHA with the same paths marked `git add -N` both before generating the local patch and after applying it remotely.
  Do not allow untracked files to drop out of the identity check.
- `L023`: generate the diff SHA with `--binary --full-index` on both sides to remove the effect of Git object ID abbreviation
  settings.
- `L024`: expected-failure tests use an explicit `match` instead of `unwrap_err`, which relies on `Debug` of the success type, and
  move on to run tests only after `cargo check --tests` passes.
- `L025`: for unknown-node bypass requests, compare the actual ingress log with the snapshot's `rejected_remote`, and
  confirm the absence of node effects independently with `nodes=[]`.
- `L026`: preserve the unknown cause of an HF worker EOF verbatim as a string, and confirm the success/absent of the later supervisor abort
  in a separate terminal. Do not reduce error observations to a boolean.
- `L027`: the lifecycle controller's import/codec tests must be collected even on standard Python without the model package.
  safetensors is required only in the actual tensor step, and the runner confirms the collected test count before running.
- The test peer interprets the actual request target/source/causation and the lifecycle metadata. Do not claim use of the new path
  by counting content type strings alone.
- Python and Rust do not each implement the same literal fixture independently. Pin the wire prefix, schema, identity and
  status/resource state as a mutually compatible fixture.
- The partial rollback test does not close the socket on the first rejection. It must observe the UNLOAD that OUTER sent to the successful nodes and
  its completion.

## Required tests

| ID | Real path | Verdict |
| --- | --- | --- |
| M3-01 | Rust event-drive LOAD builder/response consumer | Agent target, common codec, typed succeeded/present, uses opaque readiness |
| M3-02 | Rust: one of two agent LOADs rejected/absent | OUTER UNLOAD once to the successful node, CREATE/DELETE 0 |
| M3-03 | event-drive CLI socket fixture with inference failure + teardown | Uses only the new LOAD/UNLOAD and preserves the existing failure artifact |
| M3-04 | HF Pipeline real Client codec | CREATE/DELETE 0, Agent lifecycle result verified, owned/ready 0 after normal shutdown |
| M3-05 | HF lifecycle failure/cleanup fixture | Does not hide failed/unknown or cleanup errors, and records the recovery command |
| M3-06 | Agent real TCP legacy CREATE/DELETE and node-target lifecycle | Response is a rejection, INSPECT nodes/retention/native unchanged |
| M3-07 | Full workspace feature off/on and HF Python suite | Existing llama.cpp/HF regressions on both sides, failed/ignored counted separately |

## Rounds

Before the first complete candidate, fix the Rust/Python codec, all current callers and the agent legacy branch together, and check the formatter,
the docs check and the targeted test discovery counts. At most 3 rounds; round 1 includes compile + M3-01–06 + regression.
On failure, add the cause and an automatic block to the deterministic execution register before running the same command again. Round 2 fixes only that
single cause, and round 3 performs a clean confirmation with no functional change plus independent mutations.

For independent mutations, pick at least two that the real consuming tests detect, from: reverting the Rust lifecycle target to node, putting DELETE back into the Python shutdown, and restoring the agent's
legacy CREATE branch; recompile them in a separate worktree and
target from the final source.
