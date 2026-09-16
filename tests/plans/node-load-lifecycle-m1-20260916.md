# LOAD/UNLOAD node lifecycle M1 deterministic execution plan

## Sealed scope

The baseline is `e1fc1c8ee4fda82ab87217af28a78d43eb1b9970`. Building on the common codec, M1 implements the typed adapter lifecycle
completion, the agent's asynchronous node supervisor, and real TCP LOAD/UNLOAD for a neutral fixture.
Migrating the real llama.cpp/HF workers is owned by M2, and the OUTER callers and removal of the old CREATE/DELETE by M3.

## Reuse of past lessons

- `L001`: no build or model run happens on this PC. On remote Spark, use the absolute Cargo path and at most 8 jobs.
- `L002`: the M1 neutral run uses only OS-assigned loopback ports. Before M2 models, the event preflight must pass.
- `L003`: real TCP tests check the entire OUTER→agent→node→agent-owned result→same OUTER path.
- `L004`: pin Spark Cargo to `/home/m42/.cargo/bin/cargo` first.
- `L005`: EventNode is the only consumer of ordinary completions. Code review rejects any implementation where the supervisor polls the same front or duplicates
  Events into an unbounded channel.
- `L006`: normalize patched existing documents back to their original CRLF and pass the full docs-lint before moving to code tests.
- `L007`: do not commit without confirming that line counts and numstat are within the intended scope. An already published bad commit is
  corrected with a forward commit that restores the original, not a force push that erases history.
- `L008`: apply the Rust formatter first and open remote round 1 only after the check passes.
- `L009`: remote mutations are not built with nested shell quoting; transfer a separate script and then check the diff.
- `L010`: confirm the remote `origin` and `FETCH_HEAD` object IDs, then create the independent worktree from that full ID.
- `L011`: for semantic mutations, count only a test assertion failure, after formatter/check pass, as detection evidence.
- `L012`: do not hide the absence of rustfmt on Spark behind assumptions; keep local baseline fmt separate from remote mutation `cargo check`.

## Ownership and call path review before round 1

| Value | Producer | Owner while waiting | Next owner on success | Owner on failure |
| --- | --- | --- | --- | --- |
| External lifecycle Event | OUTER | agent bounded input | retired after the terminal result is published | control remainder or explicit rejected result |
| Per-model LOAD/UNLOAD Event | agent supervisor | node bounded input/adapter | adapter completion | node failure remainder |
| adapter completion | adapter | adapter bounded completion/EventNode | bounded boundary for the agent supervisor | EventNode failure remainder |
| terminal lifecycle result | agent | bounded reply/transport | original OUTER | agent control remainder |

Agent control checks the common metadata, kind, ID occupancy and capacity before native, and registers `loading` atomically.
It does not await long adapter work. The snapshot string is not terminal authority. Removal happens only after confirming the typed completion and input/output
drain. The result source is the agent, and causation/return context match the original external request.

## Forbidden designs

- EventNode and the supervisor competing to consume the same adapter completion front.
- Preserving completions with an unbounded `mpsc` or an unaccounted clone of the original Event.
- Inferring LOAD completion from the adapter snapshot string.
- Removing the owner/route before the terminal result, keeping the admission fence on a busy UNLOAD, turning a failure into a success.
- Increasing queues/timeouts or shrinking normal inputs to make tests pass.

## Sealed test bundle and run rounds

The first bundle runs `NL04`, `NL02`, `NL10`, `NL07` and neutral `NL01` together. malformed/exact±1 and a disabled kind
must have no effect before factory/native. INSPECT responds during a slow LOAD, and duplicate LOADs start the adapter at most once.
The LOAD success result and the UNLOAD success result return to the same OUTER, and the last INSPECT shows nodes 0 and retained 0.

| Round | Condition to open the run | Allowed changes |
| --- | --- | --- |
| 1 | The ownership table above matches the code, rustfmt and docs checks pass, and the full bundle is written | One complete candidate |
| 2 | The single causal path of the round-1 failure, a new lesson ID and an automatic blocking test are recorded | The one change needed for that cause |
| 3 | The round-2 change and its removal mutation fail as expected, and source/inputs are re-sealed | Confirmation run only |

Environment setup mistakes and command typos are also pre-run review failures. Do not retry the same command as is. If a new design is needed after 3 rounds,
do not mark M1 complete; redesign.

## Execution environment and commands

- Development checkout: `F:\dev\p4`; runs only low-load rustfmt, docs-lint and Python preflight tests.
- Rust build/test: Spark `m42@192.168.0.26`, `/home/m42/.cargo/bin/cargo`, `CARGO_BUILD_JOBS<=8`,
  `RUST_TEST_THREADS<=8`, separate source copy and target.
- Order: targeted protocol/adapter/agent tests → real TCP neutral bundle → workspace feature off/on. They do not run concurrently.
- Each round records in the report the source digest, exact commands, exit codes, pass/fail/ignored, the first failure and the remaining attempts.

M1 completion requires the real TCP neutral result and the full regression. A state where only docs, codec and mock functions pass is still in progress.

## Run results

M1 was completed on 2026-09-16. The results for the real TCP neutral lifecycle path, preservation across OUTER disconnect, workspace feature off/on
and the independent owner-removal mutation are recorded in the
[M1 verification report](../reports/node-load-lifecycle/20260916_023059.md). Migration of the real llama.cpp/HF workers
proceeds in M2 as planned.
