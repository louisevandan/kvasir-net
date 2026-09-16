# P4 development handoff rules

A session that is new to this repository still starts in the following order. No conversation history is needed.

1. Read the [current execution roadmap](docs/distributed-batching-roadmap.md) to the end.
2. Read the [verification and real-hardware acceptance conventions](docs/distributed-batching-verification.md) to the end.
3. Read the [layer isolation contract](docs/layer-isolation-contract.md) and confirm the responsibilities of P4, the adapters, native, llama and the backends.
4. In the [document map](docs/document-map.md), read the document that owns the contract for your work target.
5. Check HEAD, the working tree and the execution path. If they differ from a document's baseline commit, audit the difference first.

## Top-priority goal

Load very large models onto distributed nodes across several physical computers, and serve heavy continuous request waves
with normal prompts and normal responses while maximizing useful generation TPS and GPU utilization.
Unit tests, simulators, several processes on one computer and small models are not proof of the final result.
The final result is approved only by the multi-computer real-hardware wave evidence defined by the roadmap and the verification conventions.

## Mandatory rules for changes and completion

- The roadmap is the sole owner of the current execution order. Do not turn the past U/P series back into serial prerequisites.
- Do not read a document's plans or target contracts as implementation facts. For the execution path, trace the event runtime.
- Every functional fix leaves a failing counterexample, a test of the real consuming path, and a mutation check that fails when the fix is removed.
- Safety, documentation and unit-test stages are `enabling`. Until the real service envelope of the current final source passes,
  they do not count as product integrity completion or as performance progress. First close the integrity baseline for single requests, bounded sustained inflow,
  overload/cancel/drain, distribution and soak; only after that, open performance candidates.
- Every model run records, in the same analysis window, for both single requests and sustained waves: TTFT, prefill rows/s, useful generation TPS,
  per-phase batch width/fill, runnable/blocked, GPU sample coverage/util/memory/power, normal responses, settlement and reclaim.
  If data is missing, do not fill it with 0 or with numbers from a different past model; judge it as not measured.
- A performance change compares a baseline and a candidate under the same model, artifact, topology, resident, KV, corpus, arrival sequence and output conditions.
  At minimum, rerun both a single request and a sustained wave. Without an improvement figure, it does not count as progress
  in the performance phase. A fast candidate that breaks integrity is an immediate regression.
- Before starting a new stage, search past reports and the
  [deterministic execution register](docs/deterministic-execution-register.md) for the same boundary, error string and resource kind. Link reusable failures
  to the test IDs and automatic blocking mechanisms of the current plan, and write down why any non-applicable item does not apply. Do not start the first run
  after only reading past failures without referencing them in the new execution plan.
- Before the next run, turn a failed run's reproduction conditions into an invariant and pin it as an automatic preflight, an assertion or a test.
  Do not just write a lesson in the report and leave the same failure kind to human judgement again. Do not start the next model run
  without a new deterministic check that blocks the same failure kind.
- For the three runs per stage: the first run takes a complete candidate whose source, callers, return path, state and resource boundaries have all been reviewed;
  the second fixes exactly one difference identified from evidence; the third only confirms that fix. After the third,
  do not repeat the same run until code, contract or preflight has changed and been reviewed independently.
- For each run failure, record a stable lesson ID, the refuted hypothesis, the proven cause, the new automatic blocking path, and the
  next stage/test that will reuse this lesson. If a lesson ID has no blocking mechanism yet, the state is under analysis, not ready to run.
- Before committing, check `git diff --numstat` and `git diff --cached --stat`. If there are unintended whole-file line-ending changes,
  mass deletion and re-addition, or generated artifacts, do not commit; recover forward from the original.
- Prove remote event runtime connectivity with a per-agent round trip in both directions that uses the advertised address and return route
  exactly as written in the configuration. SSH, `nc` or a one-way socket connection do not substitute for this evidence.
- For native listener ports, query the actual dynamic/ephemeral range on each participating OS and use only values outside that range or explicitly reserved.
  An automatic check before model load rejects collisions.
- Failure reclaim is complete only when INSPECT nodes 0, task-owned native children 0 and task-owned listeners 0 are confirmed together with
  preservation and settlement of the transport failure. Do not claim native reclaim from agent exit alone.
- On rejection, check that not only the request but also the ledger, reservations, credit and output effects are preserved.
- Distinguish between a model and a simulator calling the same function and having actually tested the real path.
- Do not make a test pass by relaxing expected values, goldens, judges or limits to match the implementation, or by deleting failing inputs.
- Compute overall totals from the final exit of `cargo test --workspace --no-fail-fast` and all of its summaries.
  Not-run, feature-excluded, ignored and failed tests are counted separately. Do not hide new required tests behind a feature.
- Run mutations only in an independent copy or a verification worktree. Do not restore the user's working tree with checkout/reset.
- Confirm that the mutation was actually recompiled and that source and binary are bound. Exclude from evidence any run where a shared build cache reused the baseline.
- After the source, binary, model and workload of a measurement arm are sealed, do not modify them. Do other work in a different checkout.
- Do not approve performance or normal responses from GPU utilization, RPC depth, mixed batch count or row count alone.
- Do not confuse the meanings of request, settlement, edge credit and KV completion. A returned transport credit is not evidence of a KV stop point.
- Node count is a constraint from the model, KV capacity, legal cuts and deployment. Do not make a one-node-per-card limit a general rule.
- Do not put private llama.cpp types into policy or the ledger. A change to the backend-neutral core comes with a neutrality test.
- Upstream adaptation happens inside the modules the isolation contract allows. Check not only direct includes but also transitive includes/links,
  forward declarations, public signatures and imported relinks. If an upper layer needs a change, approve and verify the contract change separately.
- Distinguish the llama.cpp abstraction layer from the concrete ggml/CUDA, CPU and Metal backends. A clean pin replay is not proof of semantic compatibility.
- Verify three conditions separately: dependency isolation, state-change authority and semantic compatibility. An upstream adaptation with the same semantics
  is finished inside the modules the isolation contract allows; if a pure ledger/policy change is needed, first review it as a boundary leak or a contract change.
- Do not misread "owned by P4" as "owned by the common core". Batch/flight/KV/shape/stage codecs are owned by the adapter, and
  the common core forwards opaque payloads. Impl/internal accessors and raw ordinals in the compatibility layer are also subject to isolation checks.
- Do not emit token/native effects ahead of settlement approval. Bind the ledger commit to the effect intent, and
  preserve external-effect failures and unknown outcomes as a separate progress state. Check compiler isolation and state-authority isolation together.
- Risky experimental options are disabled by default until proven. The parallel sampler is serial by default until shared-context safety is verified.
- At the end of each stage, record the code baseline, test IDs/commands/results, failure evidence, the remaining stages and the first next action.
  If the final source differs from the verified source, do not claim completion.
- Commit at every recoverable intermediate point, such as an implementation or a pinned regression. Before committing, stop other writers and review all
  non-ignored changes so that source, tests and docs are all included. Manage temporary outputs and local secrets with ignore rules,
  and do not leave required files partially staged. Right after committing, confirm 0 non-ignored dirty/untracked files.
  An intermediate failing state states WIP, the exact failure and the next action, and is not dressed up as green/complete. Push is a separate permission.
- A review request is read-only. A development request also does not widen permission for unrelated changes, stopping remote processes, deployment or push.

## Honesty in documents and verification

A new document needs a README index entry and a document map registration. Keep line endings consistent within a file.
docs-lint is a string and index check; it does not guarantee semantic accuracy or that unimplemented tests have run.
Without the required hardware or access rights, finish the local safety work and leave the real-hardware gate as BLOCKED.
When the user has asked for development to be completed, continue if a next safe step remains within the approved scope.
However, if external resources, permissions or a final model choice are needed, state the reason and the required input, and stop.
