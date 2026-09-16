# P4 product release development plan and investment rationale

Surveyed on 2026-09-13. The implications of the 15-slide [external architecture analysis deck](https://docs.google.com/presentation/d/1Ycq1kuAR1BX1zO6kJHvF18WaRLsB318hSVwPx93ZpPc/edit), especially slides 10–15, were checked against the latest official model and engine materials and against the actual P4 code and real-hardware records. **The earlier proposal to treat R1–R8 as the list of planned releases is withdrawn.** The mere fact that something can be finished as a product does not make it worth development investment.

This document is the **single development plan** that owns investment judgement, product scope, implementation scope and handoff. It does not depend on separate per-version plan files or earlier conversations. Release priority follows the [roadmap](distributed-batching-roadmap.md#current-status), test and performance verdicts follow the [verification protocol](distributed-batching-verification.md), and the layer that owns a change follows the [isolation contract](layer-isolation-contract.md). The plan is currently finalized; no product development, remote retest or deployment has been executed.

**Priority change on 2026-09-14: the first development target is [external HF adapter acceptance](#hf-integration).** After that integration, the long-context, cancellation, cleanup and next-wave work of [Release A](#release-a) proceeds. On 2026-09-15, per user instruction, the reference model changed to Qwen3.5-122B-A10B. Read this together with the [adoption scope of batching G1–G6](#batch-decisions), the [acceleration judgement including DFlash/DSpark](#speculation-decision) and the [fresh-session start procedure](#fresh-session). A/S/B/C are product contract identifiers, not the old R1–R8 phase numbers or actual Git tags.

<a id="hf-integration"></a>

> The standalone-repository layout in HF §0 was discarded on 2026-09-14 per user instruction. Current ownership, build and migration follow the [HF guide](hf-integration.md). The kickoff audit below is the record from that time; the feature plans after Release A still stand.

## 0. Top priority — accept p4hfadapter into the real P4 event path

Scheduled ahead of the existing A/S/B/C per user instruction on 2026-09-14. **`layers/adapters/hf` owns the per-model Python development and the Rust concrete adapter,
and P4 statically links the external crate and consumes its creation, discovery and lifecycle contracts.**
The deliverable is a P4 integrated distribution that can use an external HF execution configuration. This is not work that only registers the adapter and defers execution
until after A. The real LOAD→request→cancel/release→UNLOAD→DELETE and re-creation are verified together.
Qwen3.5-0.8B is the conformance model for this connection; it does not stand in for very-large-model performance or the final H0–H7 promotion.

HF-0~3 acceptance completed on 2026-09-14: the [consumption configuration](hf-integration.md) and the [acceptance report](../layers/adapters/hf/tests/reports/p4-integration/20260914_023000.md) record the real two-host run, cancel/re-acceptance, Python replacement, reproducible build and the existing llama.cpp on/off verification. For the follow-up transfers R1–R9 on 2026-09-15 and the re-verification of both real adapters on the final source, see the [Release A acceptance report](../tests/reports/release-a/20260915_183158.md). The gap and audit descriptions below reflect the state at kickoff; the next step of Release A is the Qwen122B A-PLAN.

### 0.1 Current code on both sides and investment rationale

- P4 audit HEAD: `6bd01d7e12f2711cb27e93b3b45533703e3caf9b`, clean at start. The common boundary is
  [RetainedNodeAdapter](../layers/adapters/adapter/src/node_adapter/mod.rs), and the actual assembly is
  [p4-agent Cargo](../entrypoints/agent/Cargo.toml) and [event create/remove](../entrypoints/agent/src/event_runtime/control.rs).
  The old service `adapters::registry()` is not a connection target.
- HF audit HEAD: `df4f81b7c774e60cba78d71ad868c515e619cb15`, clean at start.
  The [HF handoff](../layers/adapters/hf/docs/history/initial/HANDOFF.md) and the [Qwen run report](../layers/adapters/hf/tests/reports/qwen3_5_0_8b/20260913_220709.md) were checked
  against the current Python code. **There is no Rust bridge crate yet.** There are Python framing, the Qwen-specific loader/forward/state/worker
  and a local controller. The reported 8 combinations and 47-step comparison are records from one physical computer, not a fresh model run for this review.
- The first connection target is Qwen3.5-0.8B revision `2fc06364715b967f1860aea9cf38778875588b17`, dense FP32 split.
  The logits threshold overrun in the RTX 4080+3090 BF16 split is kept as unresolved. Quantization, physical batching, multiple physical hosts,
  full element-wise cache comparison and P4 retained acceptance are not proven yet.
- The goal is a path to modify, verify and deploy the vendors' Python implementations of the latest models independently.
  It is not an alternative that pulls memory classes or draft features missing from llama.cpp into the common core. Testing whether the same P4 binary
  can swap in compatible Python distributions proves that the model development cycle is independent.

### 0.2 Ownership and limits on P4 changes

| Location | Responsibilities and required outputs | Responsibilities not included |
| --- | --- | --- |
| P4 entrypoint / root build | External Rust crate dependency, source mapping and lock; `hf-transformers` creation and advertising of the kinds actually supported; integration and existing-path regression tests | Registering model names or Qwen classes, installing Python, downloading model weights, per-model tensor interpretation |
| HF `layers/adapters/hf/adapter/` | Role folders construction/retained/process/ipc/lifecycle. Trait implementation, bounded input/completion, Python supervision and IPC, identity, reservation, delivery/output ownership, error/exit evidence | Re-implementing a Rust scheduler that understands model layers/KV |
| HF per-model Python | Loader, partial forward, KV/recurrent, model batching/selection policy, sampling, quantization, tensor codec. Roles stay separated even inside each model directory | A common model interface that every model must inherit; node-to-node transfer that bypasses the P4 broker |
| HF execution spec / tools | Worker environment lock, entry/argv/env, model/split/device, IPC/capability identity, deployment, integration scenario/fixture/report | Adding Qwen fields to the P4 common envelope; forcing reuse of the llama-specific PLAN/LOAD wire |

**“Only the dependency and the creation branch” is a boundary on product logic, not a claim that only two files change.**
The ADAPTERS in the current [INSPECT](../entrypoints/agent/src/event_runtime/control/inspection/mod.rs) is also a `llamacpp` constant.
Bind a small static creation/support list in the entrypoint so that the kinds that compile match the kinds that actual CREATE accepts.
Advertising that the Python environment/model is ready is a separate LOAD readiness verification. Do not build a general dynamic plugin loader.
If a change to the common P4 core/trait/model wire is judged necessary, review it with an actual counterexample under the [isolation contract](layer-isolation-contract.md#external-hf-boundary)
and record it as a separate scope. Do not redesign the llama staged protocol path to make HF fit into it.

There is exactly one model scheduler, and it lives in Python. Rust checks the membership, position and issue chosen by the head/controller
against the pre-delivery reservation/authority contract and binds them immutably. Rust's judgement of whether something can be delivered and Python's
model batch selection are different responsibilities. A budget that cannot be given to Python does not approve publication, and Python finishing a token
computation does not by itself approve P4 output. This decision gives the latest user instruction priority over the earlier Rust batch-publication description in the HF docs.

### 0.3 Actual gaps to connect first

1. **Switch the local test controller to the P4 path:** the current HF `routing.Pipeline` allows only `host=local` and
   calls `Peer.exchange` in sequence. This code is kept as a reference. On the integration path, Python stage results
   go through Rust retained completion → P4 broker/next node → next Python worker. The HF execution spec fixes where the tail result and next step live and
   whether the head or the model controller owns scheduling. A configuration where a separate controller holds every worker's
   stdin/stdout directly and P4 only creates shell nodes fails.
2. **Execution spec and handshake:** CREATE creates only the Rust side and the bounded mailbox; LOAD runs the specified Python.
   The current worker's `ready/run_id` alone does not prove bridge/worker compatibility. Both sides confirm the IPC version, worker distribution hash,
   model/tokenizer/plan, stage, load generation, dtype/boundary, supported operations and byte bounds with each other.
   stdout is for framing only; stderr is collected separately under a bound. Children and reservations left by LOAD failure, duplicate LOAD or partial initialization are reclaimed.
3. **retained ownership:** on Full/Closed, return the original allocation/claim; peek does not consume. A matching take
   checks the full front identity and implements wake registration/deregistration and capacity resumption. Queued, held, in-flight and pending output
   are counted all the way through. Do not turn `completion_storage_snapshot=None` into empty. Input bytes, IPC copies/scratch
   and output/receipt reservations are computed separately. The 32MiB frame limit is not the total heap limit.
4. **Settlement, cancellation and shutdown:** bind the current Python `release` and step-boundary cancellation to the P4 request incarnation/issue/cutoff.
   Partial writes, worker death and timeouts are preserved as execution-unknown, and the same issue is never re-run automatically. Distinguish settlement of token/native effects
   from actual external delivery. Mark unloaded only after confirming worker exit, state reclaim and zero held output.
   Current P4 DELETE checks `snapshot()` for empty/unloaded/closed, an authoritative retained count of 0 and a healthy node task.
   Do not bypass this check just because Python has exited. If worker death immediately ends the whole adapter completion stream and thereby the node task, current DELETE refuses, so recoverable errors are kept separate from the lifetime of the façade that settles control and results. Provide a procedure that cleans up only the control paths that can still proceed and the children the adapter owns.
5. **Re-acceptance on the same load:** the current `StageSessions` compares `len(active)+len(retired)` with `max_requests`, so
   released IDs still count against the limit. Putting the next 8 requests into the same worker after the default 8 is not supported today.
   The integration separates the active request limit from the lifetime of the duplicate-prevention records. For example, an explicit session epoch switch, made after confirming settlement and state resolution on every stage,
   can reclaim the bounded retired records and reject the old epoch. Do not work around this with a plain `retired.clear()` or
   a higher cap. Keep the cumulative limit contract and counterexamples of the existing v1 standalone scripts, and version the new long-running semantics.
   Always verify that a late step/result/release after slot/epoch reuse does not change a new request.
6. **Independent deployment:** if the Rust bridge or P4 ABI changes, rebuild P4; a compatible Python implementation swap is LOADed
   into the existing P4 binary under a new worker bundle identity. Do not overwrite a running worker; switch after UNLOAD. Incompatible bundles are refused before LOAD.
   The HF repository owns the HF worker/environment preparation commands and failure diagnostics, and P4 docs point to their exact revision.

### 0.4 Cargo coupling and reproducible deployment

Development may start with a path link to `layers/adapters/hf/adapter` inside P4. Before the crate exists, do not register an empty crate or
an always-succeeding stub in P4. The standalone HF crate depends only on P4's `p4-adapter`/`p4-protocol` as the required
public boundary and does not depend on P4 agent or llama private implementations. Absorbing P4 and HF into each other as workspace members
or copying P4 source to create a separate trait is excluded.

Verify that one build has **exactly one package ID/source/version each** for `p4-adapter`/`p4-protocol`, using Cargo metadata and
the actual constructor's `Arc<dyn RetainedNodeAdapter>` conversion. If Git sources are unified onto a path, the `[patch]` of the consumer,
the P4 root, must apply; a patch in an HF sub-crate alone does not count as a fix.
Follow the [official Cargo override rules](https://doc.rust-lang.org/cargo/reference/overriding-dependencies.html).
Also distinguish the packages that reference both repositories and the role of each side's lockfile. The final P4 build is defined by the P4 root lock and the source seals of both sides.

If optional features are used, wire them default off and test creation, INSPECT and regression with the feature both enabled and disabled.
**Do not assume that existing P4 builds without an HF checkout just because the path dependency is optional.** Cargo lock resolution
also takes optional dependencies into account; see the [official resolver description](https://doc.rust-lang.org/cargo/reference/resolver.html#features).
If the development path remains the shipping contract, a build bundle/script that restores the exact commits of both repos must be delivered.
To choose a Git revision or crate version for distribution, first verify an actually reachable source and a reproducible build. Do not guess a remote URL or
release version that does not exist yet, and this plan does not create or push remotes automatically. The worker bundle is distributed separately from the Rust crate source.

### 0.5 Integration completion conditions and work handoff

| Implementation bundle (not a separate release) | Owning repository and exit evidence |
| --- | --- |
| HF-0 fix the boundary and spec | Fix HEAD, dirty state, trait, kind, submission/result/error/exit wire, budgets, source mapping and test fixtures on both sides. Write a difference table against the current HF worker |
| HF-1 standalone Rust bridge | In the HF repository, test refusal, saturation, partial I/O, death, cancellation, unreclaimed output and exit with the real retained mailbox and a fixture worker. Keep model-free contract checks separate from the real Qwen comparison |
| HF-2 P4 event connection | Verify dependency, creation, INSPECT and regression/neutrality in the P4 entrypoint. Actually create the external crate's concrete type and run the worker from an event request |
| HF-3 execution and deployment acceptance | Compare an independent baseline for the same model/precision/scenario with the result consumed through P4. Verify real stage transfer between two physical hosts, cancel/release/next request/UNLOAD/DELETE, Python swap and reproducible build |

The [HF integration verification contract](distributed-batching-verification.md#hf-integration-contract) owns the required tests and concrete counterexamples.
Passing each bundle is interim development evidence. **HF acceptance is complete only when HF-3 works and deployment, operation and reproduction commands exist.**
A successful connection with a small Qwen is not described as a performance promotion for very large models, and H0–H7 are still required separately by the existing per-product goals.
Do not hide the existing llama timer RED. If fixing it is a regression requirement of the HF/core change, fix it first; if it is unrelated, keep it as an open item of A
and do not claim the whole workspace is GREEN. Do not pull the whole batching improvement of A back in as a prerequisite of the HF integration.

A new session first reads HF's HANDOFF/AGENTS/model docs and checks whether the Rust crate has been created yet. The HF repository's
“P4 read-only” is the existing scope of that work; this plan schedules the follow-up acceptance work on the P4 side. This document change did not
send development messages to other sessions or change HF sources, docs or remote environments. In actual development, external crate/worker
changes are verified and committed in the HF checkout, and the creation wiring in the P4 checkout. Never stage dirty changes from both repositories together.

## 1. Direction and investment principles

**The proposed product direction is to let the heterogeneous LAN equipment a user already owns run the latest large open-weight models that are worth choosing for real work, in sustained use on normal long-context tasks.** The number of supported models, backends and memory classes is excluded from the target metrics. Extensions that serve only pre-2024 models are not adopted without concrete evidence that actual users need those models. An old algorithm is still considered if it solves a bottleneck of current models.

There are three focus areas.

1. **Practicality of the currently loaded model:** finish the long-context requests that could not be processed before and keep accepting the next requests. Distinguish getting the model onto the server from finishing the user's work.
2. **Removing waste in repeated work:** if recomputing the same code, document or conversation context every time is actually expensive, reduce it.
3. **Model choice relative to investment:** pay the support cost only when a new model delivers better work results or less time/memory than the existing model.

The primary metrics are normal task completions per unit time, useful generation TPS under a fixed load, user-facing TTFT/ITL, and unusable time after a failure. GPU utilization, total parameter count and the number of supported classes are explanatory variables. In scenarios that require API/tool calls, the verdict covers the final answer, format and tool arguments; producing many reasoning tokens does not count as task completion.

Code analysis, long technical document queries and several rounds of follow-up questions are **the practical workload proposed by this review**. The actual customer usage mix has not been obtained. Prefix hit rate and MTP profitability are therefore not assumed as if they were real usage statistics. Having the user buy bigger GPUs or other equipment is not the default alternative for improving P4 either.

**Adoption threshold:** the target model/task, current loss, possible gain, cheaper alternative, cost and stop condition must all be explained. Items whose gain is unconfirmed get only a short feasibility study and are not reserved as a release.

## 2. Starting point according to current code and measurements

### 2.1 Baseline update

- The first code audit was at 6b175fb8535898ec4120297f1aadcd623879766d. The final audited code HEAD is **245d6b785c96ec770dc6041ded35457c2ef97260**. After the previous 484b856ee, 532deee47/2ff8cc703 changed the OUTER load policy and its verification. The follow-up 245d6b785 moved the load module to `tools/model-loading/` and kept the core policy logic. There are no Rust/native changes in this range. A new session re-audits the HEAD difference after the documentation commit.
- The reviewed llama.cpp pin is 451b89bae, with 27 active patches. The Nemotron real-hardware runs below used a separately sealed binary / upstream 434ddbbc0. They are not reused as results for the latest pin.
- [placement-policy](../tools/model-loading/src/placement-policy.ts) removed the exhaustive subset search and the 20-device limit. [model-loading-policy](../tools/model-loading/src/model-loading-policy.ts) and [model-loading-planner](../tools/model-loading/src/model-loading-planner.ts) connect a typed fleet, per-model layer demand and measured service time. The earlier proposal “there is no planner; implement 20-device expansion first” is therefore already redundant. Whether the function results match native PLAN→LOAD on real hardware is a separate question.

### 2.2 Confirmed losses

| Evidence | Currently confirmed result | Meaning for the investment decision |
| --- | --- | --- |
| V1.1 [sealed real-hardware record](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md) | On one MI250 host, raw generation-only TPS improved, but the 100k arms failed on deadline and on normal-response acceptance. The shutdown watchdog also exceeded its limit | Rather than adding one more batching algorithm, isolate where native cost, return and cleanup get stuck on long context |
| Nemotron [load-summary](../target/nemotron550-all-fleet/load-summary.json) | LOAD and SESSION 8/8 across 7 physical hosts / 8 stages, LOAD about 2,114 s | The load path already exists. A plain LOAD success cannot be treated as launching a new model service |
| Nemotron [test-progress](../target/nemotron550-all-fleet/test-progress.json) | The long arm with 8 requests of 100,038 input tokens ended with completed/released 0/0 after about 7,203 s. Deadline, missing observations and UNLOAD busy were recorded | Direct evidence of what blocks the practical value of a supported recent model. Observations are also missing, so the failure cannot be pinned on a pure compute shortage or on the scheduler |
| Nemotron [mixed progress](../target/nemotron550-mixed-waves/progress.json) | The follow-up mixed test refused to start with old native work remains | The cleanup problem also costs the next workload and development verification time. There is no new mixed performance result |
| [M3 MSA rejection record](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-13-minimax-m3-msa-distributed-rejection.md) | A normal MSA GGUF is refused for stage residency. The incomplete static flag patch was withdrawn | Independently of the value of opening a recent model, the semantic implementation and kernel cost must be estimated first |
| OUTER update | 532deee47 fixed available, the unified host limit, legal cuts and the batching objective comparison. Input with multiple unified accelerators whose shared topology cannot be represented is refused. The [6,678-case comparison record](../tools/model-loading/README.md) shows agreement between synthetic demand and the reference policy; it is not a real-hardware run | The old low-level capacity 100/total 120 counterexample is not reused as a defect of the current upper-level planner. Binding to native PLAN/LOAD and the current INSPECT is what remains |
| Batch selection test re-review | Unlike the 45/45 in the first document, the rerun gave scheduler 33/33, bounded 9/10, service 2/2. The timer test also fails when run alone | Currently **44 passed / 1 failed**, cause not confirmed. It is the first regression task of Release A, and 45 GREEN is not handed off |

The Nemotron progress files were read locally this time; this is neither a new real-hardware run nor a full log re-audit. The end states in the files are newer information than the roadmap's earlier “in progress”, but the cause and the normal goodput are not judged yet. The measurement arms were not modified.

## 3. Evaluating unsupported features against current models

The figures below are structural descriptions from official model cards. Do not rank speed or quality across different models by active parameter count alone. The maximum context is also not P4's supported length or practical processing length.

| Model/family | Latest official evidence and practical relevance | Judgement from the P4 perspective |
| --- | --- | --- |
| Nemotron 3 Ultra 550B-A55B | Released 2026-06; Mamba-2/attention/LatentMoE with MTP, up to 1M context. The official BF16 deployment examples assume large datacenter GPUs. [NVIDIA model card](https://huggingface.co/nvidia/NVIDIA-Nemotron-3-Ultra-550B-A55B-BF16) | There is already evidence of a 7-host load, so it is the reference candidate for testing practical improvements at the lowest additional model support cost. The quality and cost of the current Q5 GGUF over LAN are measured separately |
| Qwen3.5-397B-A17B | Gated DeltaNet+attention hybrid, native 262,144 context and MTP. Relevant to code, tool and long-context use. [Qwen model card](https://huggingface.co/Qwen/Qwen3.5-397B-A17B) | The pin factory routes Qwen3.5 to the general hybrid branch. Concluding that Qwen3.5 is unsupported because hybrid-idx is unsupported is wrong. It is a candidate for a cost/quality comparison on the existing hybrid path, not an approval of current P4 real-hardware support |
| MiniMax M3, about 428B / 23B active | 2026-06 MSA paper and public model; long code/document and multimodal work. [Model card](https://huggingface.co/MiniMaxAI/MiniMax-M3), [MSA paper](https://arxiv.org/abs/2606.13392) | This is not legacy-model compatibility work. Still, a large port is on hold until the model's own quality and its efficiency on current equipment are proven. vision/video support is not pulled in along with it |
| DeepSeek V4-Flash 284B / 13B active | Released in 2026, CSA/HCA and 1M context. Pro is 1.6T/49B. [Official model card](https://huggingface.co/deepseek-ai/DeepSeek-V4-Flash) | The judgement to prioritize it because of its weight size is withdrawn. The compression/indexer/MTP state and backend correctness risks must be assessed first. There are also reports of response corruption on GB10/RPC, so distributed expansion waits until native quality is verified on that combination. Loading the 1.6T Pro is not scheduled as a current goal |
| DeepSeek V3.2/DSA | Officially released 2025-12. [Official announcement](https://api-docs.deepseek.com/news/news251201/) | This is not pre-2024 support either. Still, full DSA support is not a separate goal without evidence that its work quality or equipment fit beats V4-Flash and the models that already work |

There is an important cost difference. The current [official MiniMax MSA kernel](https://github.com/MiniMax-AI/MSA) requires NVIDIA SM100 and Linux x86_64 and states that aarch64 is untested. The paper's H800 measurements must also be kept apart from what today's kernel repository targets. The [KTransformers M3 path](https://github.com/kvcache-ai/ktransformers/blob/main/doc/en/kt-kernel/MiniMax-M3-Tutorial.md) offers a CPU expert offload alternative, but the supported GPU in that guide is SM90 and the upstream SGLang path is SM100. Do not count these as drop-in replacements for 3090/Metal/MI250. Getting the benefits of the MSA algorithm on P4's ggml path may require a separate backend implementation and verification.

### 3.1 How many of the 10 classes are supported is not an investment metric

The llama_model::create_memory branch of the reviewed pin was checked directly. Composite models can take different paths depending on metadata, so the table below is not a “one name = guaranteed support” table.

| memory path | Connection confirmed in code | Decision |
| --- | --- | --- |
| Plain KV / iSWA / recurrent / hybrid | Existing opt-in. Includes the hybrid path for Nemotron and Qwen3.5 | Focus on real service and combination verification of the target models |
| MSA | MINIMAX_M3 | Implement only when M3's task quality and per-device sparse cost clear the investment threshold |
| DSV4 | DEEPSEEK4, the special cache in trunk and a separate MTP path | First compare Flash's practical value and its kernel/state fit |
| DSA | DEEPSEEK32, GLM_DSA, some HY_V4 checkpoints | Fix the scope only when an actually selected model needs this path |
| DSA-ISWA | DOTS3NOTE | The official [vLLM implementation](https://docs.vllm.ai/en/latest/api/vllm/models/dots3_note/nvidia/model/) was confirmed, but this survey found no unique value for P4's target work. On hold |
| hybrid-idx | QWEN4EXP, the public [Qwen3.8-Flash-Next](https://huggingface.co/Qwen/Qwen3.8-Flash-Next) | Tied to a currently public model, so not treated as an enum without demand. Classified as a sparse-stabilization feasibility candidate because the QSA computation is incomplete and the recurrent/indexer state is risky |
| hybrid-ISWA | Hybrid models with SWA metadata | Not scheduled as a general implementation goal unless a specific selected checkpoint needs it |

### 3.2 Sparse attention: do not mistake the existence of an execution path for stabilization

This reflects the later correction in the analysis the user provided. Sparse support in this context is not sparse-vector storage for retrieval but **execution and state support for models that use an indexer to choose attention targets**. Six conditions are kept separate: artifact preservation → model computation → state consistency → actual backend compute → P4 stage execution → multi-computer service acceptance. Success on an earlier condition does not approve a later one.

The upstream re-check snapshot is 002a12ad25503a93501b2e188c360029830a241a (2026-09-13). It is separate from the P4 pin, and the latest upstream was not applied to P4.

| Item | Directly confirmed facts / limits of the reports | P4 investment judgement |
| --- | --- | --- |
| M3 MSA | The [model code](https://github.com/ggml-org/llama.cpp/blob/002a12ad25503a93501b2e188c360029830a241a/src/models/minimax-m3.cpp#L222) enables MSA under the flash attention and stream conditions. With multiple sequences + unified KV it falls back to dense. Having a decode gather implementation does not make that combination a sparse service | The current P4 [compat check](../layers/adapters/llamacpp/staged/server/src/compat/p4_llama_compat.cpp) explicitly refuses this combination. Stage residency is not approved either. Simply adding an opt-in flag is excluded from the development plan |
| Qwen3.8 QSA computation | The [model code](https://github.com/ggml-org/llama.cpp/blob/002a12ad25503a93501b2e188c360029830a241a/src/models/qwen4exp.cpp#L747) still has a TODO for sparse activation and a call that uses the full range | The span where the indexer cost is paid without any sparse gain is an investment candidate. The implementation scope must be estimated including backend shape, pooling and state |
| Earlier Qwen two-session collision | [#27994](https://github.com/ggml-org/llama.cpp/issues/27994) is closed, and [#27941](https://github.com/ggml-org/llama.cpp/pull/27941), named as the fix, merged on 2026-09-01 | The claim “the bug is still there as is” is excluded. The regression input that mixes the same logical position across sequences must be kept |
| Qwen rollback | [#28019](https://github.com/ggml-org/llama.cpp/issues/28019) is a public reproduction report that multi-seq replay failed when the rollback excluded by default was forcibly allowed | Do not conclude that the default path causes the same corruption. It is a counterexample showing that MTP/rollback support cannot be declared by extending an allowlist alone |
| The two DSA caches, MLA and LID | The [cache implementation](https://github.com/ggml-org/llama.cpp/blob/002a12ad25503a93501b2e188c360029830a241a/src/llama-kv-cache-dsa.cpp#L61) forwards remove/copy/shift/state calls to both | This structure alone neither confirms a current corruption bug nor proves atomic consistency. The physical mapping and actual results must be checked after allocation failure, transformation and restore |
| Unneeded indexer V-cache | [#28296](https://github.com/ggml-org/llama.cpp/issues/28296) is closed. The linked [#28330](https://github.com/ggml-org/llama.cpp/pull/28330) merged on 2026-09-10 and removes the V allocation of the Qwen3.8 indexer | The premise “every recent indexer allocates an unneeded V” is withdrawn. It also does not mean the actual allocation of other classes such as DSA was fixed. Decide the investment from per-model PLAN/LOAD bytes |
| V4 response quality by device | [#28132](https://github.com/ggml-org/llama.cpp/issues/28132) reports character/response corruption with GB10 2-node RPC and specific quants/builds | Not proof that the model as a whole or unified KV is the cause. For candidates on that equipment, check native reference responses first, and do not raise their rank on load size alone |

The roughly 4× improvement in [QSA #28734](https://github.com/ggml-org/llama.cpp/issues/28734) is a report under the author's separate patch, 5×3090, F16 KV and long context. It is neither upstream default performance nor a figure reproduced in P4, and it does not verify full normal responses or sustained service. It is used only as grounds to investigate a potentially large improvement. The V4 draft/prefix detail issues in the attachment could not be independently confirmed down to cause and version this time, so they were left out of the list of confirmed defects.

The SM100 condition of the official MSA repository is **a condition of that repository's kernels**. Do not use it as grounds that a separate llama.cpp ggml implementation is fundamentally impossible on a 3090. Conversely, a particular CUDA success does not guarantee the same sparse path and quality on Metal/ROCm.

#### Concrete verification units for state stabilization

The key identity is not a single sequence ID but **model/state generation + sequence membership + logical position → block members → current physical cell**. With a shared prefix, one cell can belong to several sequences, so a simple one-to-one `(seq_id, position)` implementation is not imposed. This structure is not put into the P4 common ledger; it is handled as a state contract of the llama.cpp adapter/native side.

| Proposed test ID | Input that exposes the failure | Observation / pass criterion |
| --- | --- | --- |
| SP-ARTIFACT | Original and indexer-stripped artifacts with the same name, missing metadata, tensor shape mismatch | Check the actual GGUF header, tensor list and shard hashes. Missing required components are refused before LOAD and leave no reservation or output effects. The HF architecture label is auxiliary information |
| SP-SEQUENCE | A and B with different correct answers start at the same position; interleave on both sides of a block boundary; cancel A and reuse its slot for C | B's result does not depend on the mixing order or on C's content. Compare selected cells, state, logits and normal-task results against independent runs; the tolerance is fixed in advance from the backend baseline |
| SP-LIFECYCLE | Prefix copy/share, suffix remove, rewind, shift, slot move/compaction, save/restore, prepare failure | Combination tests on the real path for **only the operations declared as supported**. The identity/coverage of main KV, indexer and recurrent state match, and no stale cell is accessed. Refused input preserves all related state and reservations. Unsupported operations are explicitly refused before the request |
| SP-SPEC | Partial accept/reject, replay, splitting across multiple sequences, re-request after termination | Matches the non-spec baseline, including all auxiliary state. A passing single-sequence restore does not substitute for multi-sequence evidence |
| SP-KERNEL | Contexts such as 4k/32k/100k that cross block-count boundaries, prefill vs decode, target KV dtype and device | Record the execution graph/kernel, the KV range read, indexer/attention time and total response time together. An op with a sparse name, or top-k generation, does not by itself approve a compute saving |
| SP-STAGED | Placement across multiple physical hosts with an approved cut, continuous arrivals, slot reuse, cancellation and a following normal wave | Check per-stage residency of main/auxiliary state, PLAN/LOAD bytes, cleanup and final responses. Do not dress up a silent switch to dense on an unsupported combination as a sparse improvement |

This is **a proposal for new tests, not execution results.** Implementation changes come with real consumer-path counterexamples and fix-removal mutations, and the verification protocol applies. Support for every combination is not required. For example, a non-unified-only product can also be complete if the number of concurrent users, memory cost, refusal behavior and repeated waves meet the promise. A single non-unified setting line is still not treated as proof of consistency.

## 4. Per-feature survey and investment decisions

### 4.1 Problem to adopt now: the real bottleneck in long-context compute cost, cleanup and return

The [current vLLM documentation](https://docs.vllm.ai/en/latest/configuration/optimization/) already describes decode-first chunked prefill and a total token budget as a standard method, and states the trade-off between ITL with a small budget and TTFT with a large budget. P4 already implements this structure. “Introduce chunked prefill” or more knobs is therefore not counted as new value.

What P4 lacks is a cost breakdown into actual n_kv/context length, CPU mask, attention, CPU experts, boundary copy/transfer and return stall, bound to the selection policy. Do not guess that the 100k failure is either the scheduler or the GPU and rewrite everything. If the cause is CPU offload or per-device native cost, fix that adapter/backend path or change the verified cut.

**Why invest:** on modern models that are already owned and loaded, this problem currently blocks both user work and the next tests. Whatever reduces it also carries over to new models, caching and MTP.

**Cheaper alternatives:** first compare limited corrections to the current settings/legal cut, fixing the OS connection problem on the return path, and removing unnecessary CPU paths. The roadmap's existing input cap, independent cohort and receipt accounting fixes are not re-implemented. Full durable recovery and authentication are not built alongside.

**Stop condition:** if a short cost breakdown still does not isolate the bottleneck, do not start a large implementation. If a measurable compute/bandwidth lower bound already exceeds the target time, discard the scheduler-only plan and escalate it as a separate decision about the model, placement or product load. Do not turn a failure into success by lowering the existing failing workload or SLO.

### 4.2 High potential value, usage pattern still to be confirmed: reuse of repeated context

[vLLM APC](https://docs.vllm.ai/en/latest/features/automatic_prefix_caching/) states that it reduces prefill recomputation for repeated queries on the same document and in multi-turn conversations but does not reduce decode time. [SGLang HiCache](https://www.lmsys.org/blog/2025-09-10-sglang-hicache/) presents reuse cases with long coding-agent contexts. These cases support feasibility; their reported improvement rates are not carried over as P4 estimates.

**Revision of the earlier plan:** the order that first builds SSD save/restore into a finished product and pushes automatic reuse far back had weak grounds. If the real work involves repeated context, first analyze the token prefixes that user requests actually share, and review in-memory reuse that fits the model state. A disk tier is added for the cases where the reuse interval causes eviction from memory.

In hybrids such as Nemotron/Qwen, the recurrent state cannot be truncated back to an arbitrary prefix length. The [Marconi study](https://arxiv.org/abs/2411.19379) and the current vLLM description of Mamba checkpoint boundaries support this extra cost. An effort estimate of the form “there is a file save API, so attach a general radix cache” is wrong.

**Value calculation:** the expected saving per repetition is h×T_prefill − T_lookup − h×T_restore − amortized write cost − eviction recomputation cost. h is the hit ratio measured by actual token prefix / valid state, not the ratio of similar sentences. Include per-tier snapshot bytes and the resident capacity that other requests lose.

**Adoption condition:** on reproducible multi-turn document/code queries, the net saving above must be positive and account for a significant share of total work time. Also check how much cold/no-hit requests degrade. Put it on hold if repetition is low or generation dominates. A durable SSD store is not itself a release goal.

<a id="speculation-decision"></a>

### 4.3 Do not close the acceleration plan with native MTP alone

**Revised conclusion:** the default comparison group is non-spec, the existing MTP is a low-cost candidate, and DFlash/DSpark are active comparison targets to be checked all the way down to the artifacts for current large models. The earlier judgement that put all external drafts on hold as “someday” is withdrawn. Instead of implementing every algorithm, finish the one method that shows a gain on the selected model as product C. A is complete as a spec-off normal service on its own and does not wait for C.

The fixed references are [the speculative implementation at 451b89bae0c4b1dd612eb503ceace906c01ddcc9](https://github.com/ggml-org/llama.cpp/blob/451b89bae0c4b1dd612eb503ceace906c01ddcc9/common/speculative.cpp) and [the guide at that pin](https://github.com/ggml-org/llama.cpp/blob/451b89bae0c4b1dd612eb503ceace906c01ddcc9/docs/speculative.md). **This pin also contains DFlash/DSpark.** On the other hand, `LlamaPlan::requests_unsupported_speculative` in P4 [compat](../layers/adapters/llamacpp/staged/server/src/compat/p4_llama_compat.cpp) refuses external drafts and any type other than NONE/DRAFT_MTP. The required work is therefore connecting the distributed execution contract and the state consumption path, rather than writing new upstream features.

| Method | Inputs, extra cost, current evidence | Adoption judgement |
| --- | --- | --- |
| Native MTP | A next-token head inside the target plus Verify/Replay. Some models need no separate external draft checkpoint. A P4 physical MTP path exists | Check the actual tensors and the rollback-capable range of the current Nemotron/Qwen3.5, and keep it as the cheapest comparison group. The conclusion “it already exists, so it is enough” is forbidden |
| `draft-simple` | A small independent model proposes tokens sequentially. Separate weights/KV, tokenizer compatibility and draft compute cost | A candidate only if a small draft matching the target model exists and there is evidence that it is cheaper than hidden-state transfer. Having two small models loaded is not a benefit |
| EAGLE3 | Hidden states from selected target layers, plus a draft and vocabulary mapping trained for the target | Has the same feature collection/identity problem as DFlash/DSpark. Not implemented separately unless the selected target has an artifact or a performance advantage |
| DFlash | A diffusion draft that takes target hidden states and generates block candidates in parallel. A [paper](https://arxiv.org/abs/2602.06036), [official code](https://github.com/z-lab/dflash) and a pin implementation exist | May amortize a long target cycle. When the draft block grows, evaluate verify rows/KV together with the waiting time of other requests |
| DSpark | DFlash-style block computation with a low-dimensional Markov correction from previous tokens and, on supported checkpoints, confidence-based selection of the valid prefix. A [paper](https://arxiv.org/abs/2607.05147), [DeepSpec](https://github.com/deepseek-ai/DeepSpec) and a pin implementation exist | Not merely an option with a different name. Verify the trained block/anchor/confidence head/vocabulary and target combination, and compare whether its total cost per committed token is lower than DFlash's |
| N-gram family | Takes candidates from existing token repetition and verifies them without extra draft weights. Several upstream variants exist | A cheap control for code-editing and repeated-phrase tasks. Does not promise general inference acceleration. The P4 Verify/Replay and cancellation binding costs still apply |

#### Actual model pairs to review for investment now

- **Qwen3.5-397B-A17B + [Z-Lab DFlash draft](https://huggingface.co/z-lab/Qwen3.5-397B-A17B-DFlash):** the official card names the target pair. It published results at concurrency 1 and 32 on 8×B200, BF16, SGLang, greedy/thinking and 5 repetitions, compared with the built-in MTP. This is grounds to also review DFlash under high load. The values reported for that runtime and equipment are not reused as P4 Q5/LAN gains. GGUF conversion and the pin's draft graph, selected layers and hybrid replay for this pair are separately unverified.
- **DeepSeek-V4-Flash-0731 + [ggml-org DSpark GGUF](https://huggingface.co/ggml-org/DeepSeek-V4-Flash-0731-GGUF):** a DSpark artifact is published in the same distribution. Its listed size is about 10.8–10.9GB, so the label “small draft” alone does not mean it fits for free into spare VRAM. The pin's `src/models/dflash.cpp` also has a DSV4 graph branch. However, the DSV4 stage/state of the P4 target itself is a prerequisite that is not yet approved. Do not arbitrarily mix the earlier Flash variant with the 0731 target.
- **Qwen3.5-122B-A10B + native MTP:** a comparison candidate on the same target as the revised A. Containing MTP tensors does not by itself approve Verify/Replay support. The compatibility of dedicated DFlash/DSpark artifacts is checked separately. Do not quietly fold an in-house training project into C.

DeepSpec's public small checkpoints state that they were trained on non-thinking data for their target. This alone does not predict acceptance for a large target, for thinking, or for our code tasks. Training and data preparation are a large separate cost, so a model pair without a suitable artifact is put on hold. The pin's actual model graph and artifact metadata take precedence over the outdated “only Qwen3 is supported” description in the docs, and even that is not extended into full backend support.

#### Concrete implementation and counterexamples that remain in P4

1. **Feature transfer:** the pin's `common/speculative.cpp` collects the target's layer input embeddings via `target_layer_ids` in the draft metadata. In P4 those layers can sit on different stages. Deliver the required tap, position, membership, generation and dtype in an adapter-owned capsule, and reserve the bytes, lifetime and rollback retention before sending. This cannot be replaced by the tail's single last hidden state or by a context pointer from another process.
2. **Batching and placement:** reflect target/draft tensors, KV, scratch and feature transfer fully in the native PLAN. Compare only the finite feasible placements among same GPU / separate GPU / CPU. If the draft reduces the resident count, include that loss in the comparison. Assuming that upstream `ctx_other` and tensor sharing cross process or backend boundaries is forbidden.
3. **State and settlement:** bind partial accept 0/1/k/all, rejected-suffix removal, bonus token/EOS/stop, retries, late results and cancellation on every participating stage. Never output unapproved tokens first. Compare not only the target KV but also recurrent/indexer/draft state with the non-spec baseline. Unsupported rewinds are refused before publication.
4. **Fence scope:** the current [Worker drive](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/drive.rs) blocks all head publication in `verify_fenced()`. Measure how much an independent request B is delayed by A's verification. Narrow the fence only after proving immutable membership and resource/settlement independence.
5. **Adoption policy:** limit the draft block to the trained cap and confirm the head that confidence use requires. Refuse unsupported options early. Compare spec-off, MTP and any feasible external draft at low concurrency, R saturation and mixed waves, and ship a static mode with a safe off path. Adaptive switching is not added without its own counterexamples.

For n committed tokens, compare `n × baseline decode cycle` with `draft + feature copy/network + target verify + replay + settle`, but the final objective function is total task completion time and useful TPS. Acceptance rate, draft TPS or GPU busy alone does not approve a method. The synthetic acceptance option does not preserve the model distribution, so it is forbidden in performance/quality acceptance and kept only as a diagnostic arm. C's release tests follow [§6.5](#later-releases) and the verification protocol.

### 4.4 Adopt narrowly: verified placement specs and resource planning

Multiple computers are an important usage condition for P4. Still, providing **verified finite placement specs** for the target models and native PLAN→LOAD agreement is cheaper right now than a product that automatically optimizes every device combination.

[llama.cpp RPC](https://raw.githubusercontent.com/ggml-org/llama.cpp/master/tools/rpc/README.md) is a candidate for a direct distributed-execution comparison, but its own official docs describe it as a proof-of-concept and fragile. Neither “RPC exists, so P4 is unnecessary” nor “P4 is always faster” has grounds. Compare deployment cost, normal load and cleanup only where that is possible on the same model/quantization/equipment. If vLLM/SGLang or KTransformers already meet the requirement on supported equipment, record using that engine as the alternative instead of building a new P4 adapter.

**Adopted scope:** without duplicating the current OUTER implementation, connect the shared host pool, legal cuts, availability that changed over time and model/runtime identity verification to the actual deployment input. A new global optimizer, automatic autoscaling and staged ports of new engines are excluded. If the goal is correctness and preventing failed placements, do not force a TPS improvement figure.

### 4.5 New model support: prove replacement value before the name

The five conditions a new candidate must pass are a public, reproducible checkpoint; quality on the target tasks; fleet fit of the actual quantized file and runtime memory; a native kernel path on the target backend; and work value better than the existing alternatives. Local deployment permission terms are also checked on that artifact, but this document does not give legal interpretations.

**Revised proposal:** separately from improving the practicality of Nemotron/Qwen3.5 on the existing path, compare the artifact/state/kernel risks of Qwen3.8 QSA, M3 MSA, V4 and DSA in writing. Only the one model that is most promising in work value and target-equipment fit enters a small execution feasibility study. Do not prioritize Flash on size alone or read M3's gather implementation as stability. The support schedule for a new family is not fixed before this study.

**Stop condition:** defer support if the required backend effectively needs a new kernel stack, or if it improves none of quality, resources or time in practice compared with the existing hybrid models. An outcome that spends tens of days of effort just to get “one fewer unsupported class” is not adopted. The existing closing verdict on M3 stands.

### 4.6 Investments excluded from this release

| Item | Judgement from the survey | Condition for reconsideration |
| --- | --- | --- |
| P/D disaggregation | [vLLM](https://docs.vllm.ai/en/latest/features/disagg_prefill/) describes its main purpose as tuning TTFT and ITL independently and does not claim a throughput improvement from the feature itself. It is also a different structure from P4's layer pipeline. An approach that adds model replica capacity and KV transfer is not introduced ahead of need on the current LAN | When there are resources for duplicate models and bandwidth for KV transfer, and an explicit tail SLO that chunked prefill could not meet |
| A full PKI/authorization product inside P4 | The [vLLM security guidance](https://docs.vllm.ai/en/latest/usage/security/) also restricts internal distributed paths to a trusted network. The current target is a trusted LAN; first judge whether a verified network isolation/authentication layer can be used | When mutually untrusted parties are admitted into the same fleet, or external control exposure is an actual requirement. An API key alone is not considered to protect internal TCP |
| Durable request replay / non-stop crash continuation | [Ray Serve](https://docs.ray.io/en/latest/serve/architecture.html) also distinguishes replica/controller restarts from the loss of volatile request state. Recovering P4's CPU/GPU/KV and external output costs far more than a watchdog/cold restart | When actual failure frequency × cost of lost work exceeds the implementation and operation cost. Do not rush to promise exactly-once token delivery |
| Support for every backend, model and memory combination | The mere existence of a name or enum does not prove demand | A combination essential to user tasks on the selected model/equipment |
| Sampler parallelization or zero-copy without profile evidence | Could matter, but if it is a small part of total time, the effect is small for the effort | When that span is a large share of the actual critical path and can be removed or parallelized safely |

If a span accounts for a fraction f of total time, making only that span infinitely fast caps the theoretical speedup at 1/(1−f). For example, removing a 5% span completely gives about 1.053×. This calculation is not a P4 measurement; it is an arithmetic guard against large investments in small bottlenecks.

<a id="batch-decisions"></a>

## 5. Adoption decisions that connect batching, architecture and operations issues

The [batching code review](batching-code-review.md) owns the detailed paths and selector figures for G1–G6. “In A” below means **responsibility for satisfying the required contract**, not an order to write every candidate algorithm unconditionally. If an existing path passes the real consumption tests, reuse it; fix the minimal path whose failure is confirmed. Effort-day estimates and the uniform 20% investment threshold are removed because they lack measured grounds. Performance promotion figures follow the existing H5, and large changes first submit a cost breakdown and counterexamples so that the scope can be re-estimated.

| Candidate | Actual problem and loss hypothesis | Product decision / change boundary | Proof / stop condition |
| --- | --- | --- | --- |
| G1 P progress in the default batcher | The ordinary selector does not choose P once D fills the capacity. Re-offering cap 8, D8/P1 32 times gives D256/P0; bounded gives D224/P32. This is not a measurement of starvation in a real wave | **Required in A.** A finite P wait/progress contract on the general path as well. Minimal change, compared with the existing bounded policy. The hybrid equal-width and verify/replay invariants are kept | A-BATCH: D demand below, equal to and above the cap, cap 1/8, repeated admit/refuse. On the real Worker, not just the selector: P progress, D delay and state preservation on refusal |
| G2 UBATCH sent in bulk after decode completes | Native collects the callback output and returns it as a PhysicalResult after `llama_decode` finishes. A small n_ubatch does not guarantee that the next stage starts early | **A verifies the overlap achievable with independent logical issues.** True partial-UBATCH transfer is conditionally included in C or in a later product whose separate value is proven | Confirm the critical-path share with a native compute/capture/return/forward timeline. It also needs a commit/abort and credit contract for when upstream fails after downstream consumed part of the result. An approach that only opens the callback is excluded |
| G3 pipeline D width and window | Width is computed from the active population/window. It is neither a fixed cohort identity nor a cost-optimal policy, and narrowing D width can raise RPC/launch fixed costs | **In A.** Selection from approved finite profiles, and limited experiments on D width, window and P budget. Unbounded online search and global optimization are excluded | Compare window 1/2/4 and legal neighboring row caps on the same cut/memory. Fix the number of profiles and the selection criteria before A/B. Check selection bias with a final holdout |
| G4 actual shape of the service cost | Max position is not the actual n_kv, and it can miss other sequences' unified KV, CPU/offload and mask/graph costs | **Required cost observation in A.** The adapter/native side produces actual n_kv, phase, rows, membership and placement. An online controller without consumption evidence stays off | Compare the same logical position/rows under different KV occupancy and CPU/GPU placement. Link profile error, synchronization and transfer to actual TTFT/ITL. If adding the cost function does not reduce net cost, keep the static profile |
| G5 multiple P fragments for one request | The current pipeline refuses prefill_fragments≠1. Even with smaller chunks, one request cannot fill several flights | **A keeps 1.** The service is completed through overlap of multiple independent requests. Multiple fragments are a separate experiment, only when the bubble of a long single request dominates the total work loss | Judge plain KV and recurrent/hybrid separately. Promote only model combinations that pass the prefix frontier, reversed/duplicate order, cancellation, partial failure and verify/replay counterexamples. Deleting the limit number is forbidden |
| G6 per-request waiting and session selection | Demand lacks age, deadline and actual KV, and the stage service RPC is not user ITL. `first_session_with_work` picks the first eligible session and does not rotate | **Required in A.** Per-request and per-session blocked reason and duration, and progress accounting based on accepted issues. If needed, choose the simplest policy among round-robin/age/deficit | Test both different sessions A/B and multiple requests within the same session. A refusal does not consume a turn or age budget, and overload is an explicit refusal. Do not report an ordering risk as confirmed starvation on real hardware |

### 5.1 Register of gaps from the external deck and earlier discussions

C1–C9 are topic IDs assigned in this audit, not the names of already implemented tests. Implementation ownership of each requirement follows the [isolation contract](layer-isolation-contract.md).

| Topic / deck | Current implementation or missing contract | Final assignment |
| --- | --- | --- |
| C1 operational Cancel/Drain, slides 10 and 12 | The event Worker has no consumption path for operational Cancel/Drain requests. Teardown and edge credit return are not KV quiescence on every stage | A-LIFE: stop new publication, classify and settle in-flight results, reuse slots after cleanup. A host whose responses are unknown is isolated by epoch and then cold-recovered; remote KV 0 is not assumed |
| C2 resources/receipts, slides 9–11 | The input request budget differs from the native output/return/receipt limits. The event broker ledger retains mostly by count. The existing ControlBudget RELEASE/SETTLE fix is in place | A-BYTES: advance reservation by lifetime for pending wire, retained output, duplicate-judgement receipts and scratch, with no effect on refusal. Bind all the way to the actual consumer instead of rewriting the existing accounting |
| C3 scheduling, slides 10–11 | The deck's patience description does not prove the whole ordinary default batcher. The controller is off by default, and service estimates are not TTFT/ITL | A-BATCH/A-COST and G1/G3/G4/G6. G2/G5 are conditional |
| C4 placement/load planning, slides 1–8 | OUTER's latest available/legal-cut/unified verification is already implemented and synthetically tested. Native PLAN→LOAD, actual pool separation and device placement verification remain | A-PLAN. No duplicate development against the old 20-device limit or the upper-level shared pool defect |
| C5 special architectures, slide 14 | MSA/DSV4/DSA/hybrid-idx involve not only tensor shapes but also auxiliary state, aliasing and legal cut issues | All SP gates of S. The Qwen3.5 hybrid in A also gets checks for recurrent consistency, actual KV bytes and cleanup |
| C6 reuse/persistence, slide 12 | Even though native save/restore exists, Persist/Restore consumption is not wired into event v2. Do not misread the existing `adapter/cache_transactions.inc.rs` as the current path | B completes everything from the actual user request through reuse. If the first scope is in-memory, SSD is explicitly unsupported |
| C7 acceleration, slide 13 | P4 native MTP support and upstream DFlash/DSpark support sit at different layers. The full head verify fence, hidden-state transfer and auxiliary-state rollback are the cost/consistency issues | C: complete one adopted method from the §4.3 target pairs end to end. Ending the review with MTP alone is forbidden |
| C8 isolation/extension, slides 1–8 and 14–15 | Impl accessors behind the public facade, raw ordinals and transitive include/link are also boundaries. A clean patch replay is not semantic compatibility | All products: an evidence matrix of PLAN/LOAD/physical/KV/spec per adapter×memory×backend; verify compiler, state-change authority and semantic conformance separately |
| C9 security/recovery, slide 15 | Trusted-LAN operation; not a TLS/source authentication or durable replay product | The A operations spec includes bind/firewall, allowed principals and the cold recovery procedure. External exposure and multiple untrusted tenants are out of scope. Automatic re-execution of earlier requests after a restart is forbidden |

The separate storage risks of C6 also stay on record. [native KV](../layers/adapters/llamacpp/staged/server/src/runtime/llama_stage_runtime_kv.cpp) uses a path when there is no model identity, the build identity depends on upstream/system information, and there are issues with the 128MiB storage cap and with checking the remove result. If SSD goes into product B, the K gate must prove shard/tokenizer/template, patch/state ABI and KV layout identity, bounded chunk I/O, separate states for save completion and memory reclaim failure, and partial restore/restart. Even if in-memory B is chosen, do not record it as “SSD operation already supported”.

### 5.2 Combinations to solve together per model

| Execution structure | Batching and stage boundaries | State added by reuse/acceleration | Judgement |
| --- | --- | --- | --- |
| Nemotron Mamba-2/attention hybrid | Recurrent equal-width and ordering constraints, CPU expert and actual n_kv cost. A cut that only fits weight capacity is not guaranteed to be optimal | Valid checkpoints instead of arbitrary prefix rewind. MTP partial replay includes the recurrent state | In A, service acceptance with the existing non-spec / 1 fragment. In B/C, approve only the added operations |
| Qwen3.5 GDN hybrid | Distinguish general hybrid from hybrid-idx. DFlash selected layers can sit on several stages | Linear-attention state, draft features, block verify. Tokenizer agreement alone is not state compatibility | Priority C comparison candidate among large modern models; first needs non-spec native/distributed reference quality |
| M3 MSA | Two costs, indexer and attention, and the multi-seq + unified limitation. Stage-local residency is not approved | Handle indexer cells and main KV together across copy/remove/restore/slot reuse | S; keep the current fail-closed behavior. Publishing dense fallback results as sparse gains is forbidden |
| DSA/GLM-DSA | Binding of the MLA/LID caches. GLM-DSA has a graph path where later layers share the top-k of an earlier full layer, so a cut can break that dependency | Physical cell mapping, shared top-k lifetime, composite cache prepare failure | No support expansion before SP and the legal cut / forward payload are proven |
| DSV4/DSpark | Compressed/indexed cache and draft graph, an extra artifact of about 11GB, target variant binding | Cannot assume that the target auxiliary cache, MTP/DSpark and prefix are each stabilized independently | Ships only when both the S and C contracts pass on the same selected model; no interim “load-only edition” release |
| Qwen3.8 QSA/hybrid-idx | Sparse computation TODO and the actual kernel path, recurrent+indexer composite state | Sequence membership/block key, rollback boundary | Keep the latest bug fixes and estimate cost only for the computation/semantics that do not exist yet. Do not treat CUDA/Metal/ROCm evidence as interchangeable |

<a id="release-a"></a>

## 6. Release A after HF acceptance — a distributed service that sustains long-context work

### 6.1 User promise and shipping scope

**After loading Qwen3.5-122B-A10B once on an approved fleet, the user can submit multiple long and short tasks, receive progress status and normal responses, and run new tasks even after request cancellation or overload.** Long document analysis is offered as asynchronous jobs, and short follow-up queries as streaming. Maintenance performs admission stop → drain → UNLOAD. When a failure is unknown, the system does not pretend success; it provides explicit failure, isolation and cold recovery. A ships only if all of this works without B's prefix cache or C's acceleration.

The existing CLI/event path is the product entry point. A new web UI, an OpenAI-compatible API or a general authentication server is not an A requirement. However, **documented OUTER commands and event consumption paths** through which the user can actually submit, query progress, cancel and shut down are required. Do not write command names or protocol fields into the docs ahead of time as if they were already implemented APIs. Ship the CLI help, examples and error codes fixed at the implementation checkpoint together.

The support targets are fixed as follows. This does not mean current real-hardware approval.

- Reference model (changed by the user on 2026-09-15): `unsloth/Qwen3.5-122B-A10B-MTP-GGUF`, 3 shards starting at `Qwen3.5-122B-A10B-UD-Q5_K_S-00001-of-00003.gguf`. The local original is at `S:/models/unsloth/Qwen3.5-122B-A10B-MTP-GGUF/`. GGUF `qwen35moe.block_count=49` and `nextn_predict_layers=1` were confirmed. Local native PLAN confirmed n_layer48/n_layer_all49; the full set of legal cuts and the allocation are verified separately. Even though the file includes MTP, speculative execution is disabled in A. Fix the actual hash, metadata and tokenizer/template in the new Qwen spec, and treat a replacement with the same file name as a new artifact.
- Reference fleet: real distributed execution on at least 2 physical hosts from the existing equipment. The exact host/device/cut/stage count is decided by Qwen's new INSPECT, native PLAN, shared pool budget and cost comparison, and sealed before the run. The earlier Qwen 5host/6stage cut is a starting candidate for the search, not acceptance evidence for the resident 8/context conditions. Do not reuse the 550B table below for Qwen. Do not use IPs or device ordinals as hardware identity.
- resident 8, context 102,400 per sequence, total 819,200, F16 K/V, flash attention on, unified KV, native batch/ubatch 128/64, spec none. Additional models beyond the target and new backends are outside the release scope.
- The default public mode is a single approved static batch profile. Do not change the global pipeline/controller defaults before real-hardware runs. Candidates are selected by a finite comparison of the existing ordinary and bounded/pipeline modes. The automatic service controller, multiple prefill fragments, external drafts and automatic prefix reuse are disabled in A.

| Old 550B host suffix (192.168.0.x), not applied to the new target | stage / layer range [begin,end) | backend note |
| --- | --- | --- |
| .29 | 0 [0,24), 1 [24,48) | CUDA0/1, two stages on one host |
| .26 | 2 [48,70) | CUDA |
| .20 / .21 | 3 [70,78), 4 [78,88) | Metal |
| .17 / .19 / .6 | 5 [88,94), 6 [94,98), 7 [98,108) | CUDA; for the middle .6, confirm the approved device identity |

The current OUTER DDR profile represents whole-layer CPU stages and does not optimize GPU expert offload or contention among several stages on the same device. A may **state the existing CPU expert placement explicitly as a finite approved layout** and verify it with native PLAN, actual allocation and calibration. Do not present a placement the planner cannot represent as if the planner had approved it automatically. Adding a general expert optimizer is not a requirement of A.

The original 550B [config](../target/nemotron550-all-fleet/config.json) and its failure results are preserved as historical evidence, and rerunning them is not a prerequisite for the new Qwen target. Fix Qwen's model, tensor overrides, actual metadata, tokenizer/template and device/cut anew. Do not reuse old binaries, channels, sessions or ports as they are. A new run is sealed with a new namespace/epoch and the current build artifacts. Layout improvement is decided first as a separate experimental axis, and its gain is not added to the batch-policy improvement rate. If the fleet is unavailable or the artifact cannot be accessed, finish local development and leave H6 BLOCKED. A small conformance model other than the approved Qwen122B does not substitute for completing A.

### 6.2 Implementation units — each is an interim commit, not a separate release

| Unit | Implementation entry point and completion output | Counterexample / consumer path |
| --- | --- | --- |
| A0 reproduce and fix the spec | Isolate the cause of the existing timer RED; preserve the existing Nemotron failure and break down compute/return/cleanup in the new Qwen trace. Review the acceptance verdict in `tools/event-drive/src/run` and the actual commands. Write the tracked corpus, benchmark manifest and reproduction commands | A-RED/A-COST. Before adding instrumentation, confirm that the existing failure reproduces. Do not guess whether a missing stage is still computing or whether return/observation was lost |
| A1 exact refusal before deployment | Compare the public input/result of `tools/model-loading/index.ts` → native PLAN → LOAD → actual allocation. On a pool, artifact or capability mismatch, fail the whole deployment and reclaim the resources already created | A-PLAN. Fresh available, integrated pool, legal cut, actual CPU expert placement, shard/ABI mismatch. Rewriting the planner algorithm is forbidden |
| A2 finite task lifetime | OUTER/event adapter dispatch → Worker publication/settlement → native quiescence → cleanup on all stages → terminal result. Actual user commands for Cancel/Drain, status query and admission | A-LIFE. Distinguish output committed before the cancel from output forbidden after it. Preserve unknown-result states, the first error and cleanup errors. Keep completion/cancel/failure separate from resource cleanup status |
| A3 byte acceptance and control progress | Bind the limits across `layers/agent/src/event_broker/ledger.rs`, the existing control ownership and the adapter input/output/return paths. Account for duplicate ownership of output receipts/payloads | A-BYTES. On refusal, zero effect on ledger, reservations, credit and tokens; exactly one effect even with duplicate/late replies. Cancel/settle control keeps progressing during data saturation |
| A4 batch progress and profiles | `v2/scheduler.rs`, `scheduler/pipeline.rs`, `node/state.rs`, `worker/drive.rs`, `worker/service.rs`, native physical capture/return. Minimal changes for the G1/G3/G4/G6 contracts | A-BATCH/A-COST. Distinguish request fairness from session fairness. Keep the KV/hybrid invariants within the verified range. The next wave must progress even without expanding G2/G5 |
| A5 product acceptance and operations package | Current executable and companion DLLs, profile, support/refusal matrix, CLI examples, normal/cancel/drain/cold-recovery procedures, test runner/report. A new session reproduces with the same commands | A-SERVICE and H0–H7. Source/binary binding and the final full test. Record deployment/performance promotion only after the corresponding real-hardware run passes |

If in A0 the measured lower bound of long-context compute already exceeds the product SLO, first estimate the scale of the required kernel/placement changes. Do not push ahead on the assumption that a small scheduler change will be enough. If the smallest feasible fix exceeds the scope, report **A not complete, with the specific bottleneck and required resources**. This user model change is managed as a new target/spec and does not turn the old 550B failure into a pass. Do not relax Qwen's 100k input, correct answers or SLO after the run in order to ship. During development, commit at each regression fix, feature connection and verification completion, and record the remaining work in the roadmap.

### 6.3 Test and operations contract that decides the release

The [Release A verification contract](distributed-batching-verification.md#release-a-contract) owns the concrete inputs, limit values and verdicts. A-RED/PLAN/BYTES/LIFE/BATCH/COST/SERVICE are **planned test IDs defined here**, not existing test functions. Developers must connect them to actual runners/tests and leave expected failures and fix-removal mutations.

The required outputs are as follows.

1. **Support spec:** model shards/hash, runtime/patch/state ABI, device identity, cut, KV, batch/profile, trust boundary and supported operations. Separate columns for PLAN success, LOAD success, native conformance, distributed normal service and spec. Anything out of scope is refused up front.
2. **Fixed run spec:** normal corpus and oracle, tokenization, seed, arrival schedule, deadline/SLO, resident/queue/byte bounds and A/B/holdout order. The loader must refuse placeholders, missing caps and unsupported combinations.
3. **Automatic verdict and evidence:** full text / normal stop of every request, settlement/cleanup, late results, omissions/errors, per-host allocation/compute/transfer data, source/binary/library hashes. Do not make `target/` the only handoff location; keep replayable manifests, corpus, run commands and result summaries on tracked paths. For large raw data, record the hash and access path.
4. **User operation:** reproduce install, submission, status, cancel, new request, drain, UNLOAD and recovery after a communication failure on the same shipping configuration. Do not reuse a slot immediately as though the state of a disconnected host were known. Cold-restart only after confirming that the previous epoch is blocked and that the owning process has exited or a new load is in place.

If the baseline has 0 normal completions, do not report the improvement rate as infinite. A's first achievement is going from failure to normal service and cleanup. Optimization within the range where both normal arms hold is proven with H5. Having only the observation work done, or only a local GREEN, is not an A release.

### 6.4 Decisions on defaults, failures and compatibility

- Invalid profile/artifact/backend combinations and byte-limit overruns are refused explicitly before publication, and the caller must be able to see the reason. Distinguish ordinary request failures during processing from a whole-server poison.
- Receiving a cancel does not mean force-stopping native GPU work immediately. Stop new publication and settle already-published work up to its boundary, or move to bounded fault recovery. Do not label unknown work as a successful drain.
- Protocol/identity changes required for safety bump the version and refuse mixed runs with older peers before start. Do not expose backend-specific private types through the common protocol/ledger.
- An experimental candidate that does not clear the gain/safety bar is not included in the default profile. Accept that the existing A service is complete without that option. If an earlier feature failed, do not just turn the option off and report the existing regression as solved.

<a id="later-releases"></a>

### 6.5 Follow-up products — finished products with adoption conditions, not feature lists

The roadmap owns the execution priority of S/B/C. No development schedule is reserved before each candidate's feasibility verdict. Having no selected target means investment is on hold; it is not a demand to implement every model now.

| Product | User value delivered on its own | Scope to fix and required implementation | Release proof / investment stop |
| --- | --- | --- | --- |
| S: long-context service on a selected modern sparse model | Solves the target tasks better than A's model, or delivers the same quality with fewer resources or less time | Select one target/artifact/quant/backend and concurrency/context from the §3 candidates. Includes main/auxiliary cache, legal cuts, the actual sparse kernel, admission, cancel/cleanup, deployment and operations | Quality/work time/memory comparison with the existing model on **the same tasks**, all the relevant SP gates, H0–H7. Native sparse not executed, dense fallback, or a multi-user service that passed with only one sequence counts as failure. On hold if only the cost of a new kernel stack remains without practical benefit |
| B: reuse-based conversations over repeated documents/code | Reduces the waiting across a whole task that asks about the same document several times | Valid prefix/checkpoint of the selected model, identity, admission, eviction, cold fallback, prevention of cross-user contamination, cancel/cleanup. The first scope is in-memory; SSD only when the actual eviction interval requires it | 4 follow-up queries on 4k/32k/100k targets, common prefix 0/50/90%, a mix of cold/hit/no-hit and different users. Measure total time and the loss to other requests. Arbitrary hybrid rewind is forbidden; per-feature K/H gates, contamination counterexamples, mutations. On hold if the hit rate is high but there is no net saving |
| C: speculative response service matched to the target | Answers faster in the stated concurrency range while keeping the same target and quality | First select a model pair from §4.3. Adopt one method from a comparison of non-spec, MTP, feasible DFlash/DSpark and low-cost draftless methods. Feature collection, draft placement, verify/replay, fence, off mode and cancel/cleanup go into one product | Compare total cost, tail, useful TPS and draft memory at concurrency 1/2/4/R and under mixed/overload conditions. Partial reject/slot reuse, target/draft identity mismatch, output distribution/quality tests and H5. The off path must be accepted in the saturated range, and a method with no gain is not added just to raise the support count |

The detailed specs of B/C also seal the target artifact, SLO and resource caps before implementation. Greedy uses the non-spec token/position and logits baseline of the same approved layout; stochastic sampling checks the rejection/sampling math and verification against a fixed distribution, together with normal tasks. Do not demand bit-identical tokens across different sampling paths just because the seed is the same. If the recurrent/indexer operations that MTP/DFlash/DSpark use are not approved, C must also satisfy S's state contract, and acceleration on an unstable target is not released first.

<a id="fresh-session"></a>

## 7. Starting and handing off a new session without context

The current first request is “implement the p4hfadapter acceptance work in §0 of the development plan”. After HF acceptance is complete, it continues with “implement Release A”. A new session reads in this order: AGENTS → the full roadmap → the full verification protocol → the isolation contract → the documentation map → this document and the batching code review. Do not treat past in-progress states mentioned in the docs as current process state.

### 7.1 Confirm the baselines on both sides first, resume A after HF

At HF kickoff, check `git rev-parse HEAD`/`git status --short` in both P4 and HF, and
use `Test-Path F:/dev/p4/layers/adapters/hf/adapter/Cargo.toml` rather than presuming a crate that does not exist yet.
The current fast standalone HF test is `python -B tools/testing/run.py` in that repository. Actual model/mutation commands follow the HF model docs.
HF build/integration commands are finalized after the crate/feature is implemented; planned options are not run.

The following commands are **the existing regression commands for resuming A after HF is complete**, not an instruction to run all of A before the HF implementation.
These commands exist today. Run them in PowerShell at the repository root. They are not normal real-hardware or remote deployment commands.

```powershell
Get-Location
git rev-parse HEAD
git status --short
git diff 245d6b785c96ec770dc6041ded35457c2ef97260 -- layers tools test
cargo test -p p4-llamacpp-staged-adapter --lib v2::node::worker::loop_tests::bounded_strategy::phase_pacing_actual_loop_expires_decode_wait_without_another_tail_or_input -- --exact --nocapture
cargo test -p p4-llamacpp-staged-adapter --lib v2::scheduler -- --nocapture
cargo test -p p4-llamacpp-staged-adapter --lib v2::node::worker::loop_tests::bounded_strategy -- --nocapture
cargo test -p p4-llamacpp-staged-adapter --lib v2::node::worker::loop_tests::service_budget -- --nocapture
npm run test:model-loading
node --test test/benchmarks/cluster-inference/*.test.mjs
```

First determine from the event trace whether the timer failure is a functional defect or a test synchronization issue in which a normal RELEASE arrived between observation snapshots. The current reproduction is `bounded_strategy.rs:283`, `next_event 57→58`, `free_sequences []→[1]`. Do not arbitrarily relax the expected values or timeouts. If the test needs fixing, keep the original oracle that the timer does not change state, and prove that an independent mutation injecting a wrong timer state change fails.

The implementation entry point of the new acceptance runner is `tools/event-drive/src/run/{config,acceptance,inference_evidence,teardown_preserves_failure_tests}.rs`. The current CLI takes the form `cargo run -p p4-event-drive -- CONFIG.json ARTIFACT.json`, but the string conditions and minimum token count of the current acceptance cannot judge all of H1/H5. **Writing the A runner/corpus/manifest gate is also part of the development scope.** Do not run profiles that are yet to be created as if they were existing files, and do not pass unimplemented cancel options to the current CLI.

### 7.2 Precise records of development stops and completion

- If the executed source differs, first update the related consumer paths and tests, and record the difference from the documented baseline. Do not overwrite sealed arms or user changes. Mutations are bound through an independent checkout and an actual recompilation/hash.
- When resuming A after HF acceptance, proceed in this order: resolve the timer RED → A1–A4 real-path counterexamples/mutations → related native/backend conformance → A5 real-hardware runs. The internal order of A1–A4 can follow their dependencies, and they are not released separately. Tally the exit of the final `cargo test --workspace --no-fail-fast` and all summaries.
- On the native side, check the current pin/27-patch manifest and build the relevant backend. For Windows runs, ship the verified companion DLLs together and check their hash/timestamp. Do not mix an old server binary with new Rust results.
- Numeric budgets are made concrete from the A contract of the verification protocol and the native PLAN/INSPECT results, and sealed before the run. If the limits do not fit, record INVALID/BLOCKED/FAIL. Do not fill in unmeasured performance as approval.
- Run reports record the code hash, commands, test IDs, passed/failed/ignored/not run, the native vs multi-host distinction, the first error / cleanup errors, remaining work and the first next action. Keep local safety completion separate from product release.
- Remote resources are used within the approved scope of the development request. That approval does not extend to killing unrelated processes, modifying existing sealed arms or pushing. If resources are unavailable, finish the local work that can be done and record the required hosts/artifacts and H gates concretely.

## 8. Evidence preservation and current verification status

The HF scheduling audit on 2026-09-14 read both repositories and modified only the P4 plan documents. The 09-13 tests below are records from that time, not HF integration results.

- The full text and rendering of all 15 slides of the external deck were reviewed. The original text and images are preserved in `target/external-slide-review-20260913/`. The attached sparse analysis was verified separately against the actual pin and the latest upstream state, and cause claims that could not be confirmed were left out of §3.
- The latest upstream review snapshot is `002a12ad25503a93501b2e188c360029830a241a`, and the P4 pin is `451b89bae0c4b1dd612eb503ceace906c01ddcc9`. The five latest source files are in `target/external-slide-review-20260913/upstream-002a12ad/`. The DFlash/DSpark facts were checked against `common/speculative.cpp` and `src/models/dflash.cpp` at the P4 pin and against official model materials. Descriptions from the remote master were not used as P4 execution results.
- The Nemotron `test-progress.json` SHA-256 is `1FEF255A5A2DFCE443B4BF56B8CA052A167D300E0C83589ADDF7115ACA5AF60A`, and that of the mixed progress file is `9B14E2DCC5F2FC400843C186A91FC14629B53A05194903E61502E731C82C12D6`. The 100k failure and the never-started mixed run are preserved. The current remote state was not re-checked.
- The current tally of the batching re-review is **44/1/0**, not a whole-workspace tally. The standalone timer failure log is `target/external-slide-review-20260913/review-20260913/phase-pacing-alone.log`, SHA-256 `4d3e2a4979ac853b91998c3832d45603af615188b726d63d9a1a472cadb471ec`. The original author's 45/45 is kept only as a historical record. See the [batching document correction](batching-code-review.md#review-correction).
- The low-level shared pool probe is kept separate from the upper-level planner's latest fixes and the 6,678 synthetic comparisons. Synthetic KV/runtime demand does not substitute for actual native tensor/allocation/cut conformance.
- This result is **a completed development plan**. New A/SP/C test implementations, new model downloads, GPU/remote real-hardware runs and performance improvements were not run. The source/link/EOL/docs-lint checks are document checks, not proof of product acceptance.

Document close-out re-check: after the OUTER move, `npm run test:model-loading` passed 34/34, and the standalone timer test reproduced RED at 0/1.
Product sources and existing tests were not modified. Document links/anchors and the docs-lint regression suite 12/12 were confirmed.

Change verification on 2026-09-14: docs-lint clean on 100 P4 documents, docs-lint regression 12/12, file/anchor links in the modified documents and `git diff --check` passed. HF HEAD/dirty state was identical before and after reading, and no Rust/Python/Cargo implementation or model real-hardware run was changed or executed.
