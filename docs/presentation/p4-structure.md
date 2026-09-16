# P4 structure — for general developers

> Document status: **Structure explainer**. A Markdown edition with the same content as the slide deck `p4-architecture.html` and the generator `build-intro-pptx.js`.
> Performance figures are owned by separate, condition-bound measurement records. Current goals, status and ordering follow the [execution roadmap](../distributed-batching-roadmap.md);
> per-layer responsibilities and upstream isolation follow the [layer isolation contract](../layer-isolation-contract.md).

Code figures baseline: `b3a0d51ef` (2026-09-12). The facts in this document were checked against the code, not against other documents, and each section ends with the locations checked.

---

## 1. One-line summary

A model that does not fit on one GPU is split by layer and loaded across several machines; during inference only intermediate computation values are exchanged.
P4 is the layer responsible for that connection — it is not an inference engine.

| Figure | Value |
| --- | --- |
| Process types | 1 (agent) |
| ggml backends supported through llama.cpp | 18 |
| Rust | 114,154 lines · 419 files · 12 crates |
| Fixed llama.cpp pins | 10 (latest `451b89bae`, 27 patches) |

---

## 2. Why a layer like this is needed

When a model does not fit on one device, **how you split it determines performance.**

**Tensor parallelism** — splits a single layer across several devices. Every cut must reconcile the full activations,
so the bandwidth between devices becomes the limit, and it assumes the ultra-fast links inside a single machine.

**Pipeline parallelism** — cuts by layer range. Intermediate values cross only at the cut boundaries, and
weights and the KV cache stay on each machine. That is why several machines can be joined over an ordinary network.
This is what P4 implements.

It is not free. A request passes through the stages in turn, so latency accumulates with the number of stages and earlier stages sit idle for part of the time.
That is why batch composition, which overlaps many requests in the flow, is the core problem of this layer; sections 6 and 7 cover it.

What P4 **does take on** is which node on which machine serves which range, the order in which a request passes through that chain,
and what crosses over when. What it **does not take on** is matrix multiplication, kernels, quantization and sampling — those are the backend's job.

---

## 3. Layer structure — upper layers do not know the names of lower layers

| Layer | What it does | What it does not hold |
| --- | --- | --- |
| OUTER | decides what to load where and who receives what | direct changes to engine KV |
| `layers/protocol` | envelopes and frames — address, order, boundaries | content interpretation (opaque bytes) |
| `layers/agent` | processes, queues, workers, node lifetime, delivery and backpressure | backend names |
| `layers/adapters/adapter` | what a node requires of a backend — submission, completion, cancellation | types of a specific engine |
| Concrete adapters | `llamacpp-staged` · `llamacpp` · `vllm` · `sglang` · `mock` | state of other backends |
| backend | llama.cpp → ggml → CUDA · ROCm · Metal · CPU … | — |

Three properties this structure creates:

- **Delivery needs only the envelope.** Relay nodes do not open the content, so new message kinds do not make relaying heavier.
- **Registering a backend is one file.** `entrypoints/agent/src/adapters/mod.rs` — one name, one factory, one implementation.
- **mock is always included.** You can bring up a whole fleet without a GPU and verify batching and ordering.

The only surface an agent exposes to the outside is P4 over a socket. Whether an adapter talks to its backend over HTTP,
over a pipe, or calls it as a function in the same process is invisible from above.

---

## 4. Topology — one process per machine, several nodes inside it

There is no controller process. The path an agent uses to reach another agent and the path it uses to answer the outside are the same path.

- **agent** — one process per machine. It receives frames on a socket and puts them in a queue; workers look only at the address to decide whether a frame is theirs.
  If it is not, they pass it on whole.
- **node** — a logical execution unit inside an agent. It starts as just an id and becomes real when LOAD attaches an adapter.
  It has its own queue and holds long work by itself. It serves one layer range.
- **OUTER** — the side that makes requests from outside. It decides which node serves which range and which nodes form one session.

agent ↔ agent and agent ↔ OUTER use the same P4 frames.

Configurations run so far: 1 host with 8 stages, 2 hosts with 16 stages, 5 hosts with 6 stages.
The node count is set by model size, KV capacity and legal cut points, not by the number of cards.

---

## 5. Load and pipeline have separate lifetimes

**Step 1, LOAD — an independent command per node.** Each node reads only its own layer range from the GGUF and places it on the device.
The range is the half-open interval `stage_begin`/`stage_end`. The command carries no neighbours, no order and no session.

**Step 2, SESSION — install the order.** It tells every participating node the full list of nodes and that node's own index, in one go.

```text
stages: [ {agent, node, generation}, … ]   +   stage_index
```

Installation conditions are strict. `stages[stage_index]` must **actually be this worker's endpoint**, and
the envelope's target must match. If `load_generation` differs from the current load generation, it is rejected.
first/previous/next/terminal are derived **only from the installed order** — nodes do not decide them on their own.

Why this separation pays off:

- Sessions can be set up many times on the same load. Changing the order does not need a reload.
- UNLOAD is accepted only when idle; once it passes, it clears sessions, requests, in-flight records and session keys, and resets the generation to 0.
  A SESSION carrying an old generation is then rejected as stale.
- The head owns settlement and output approval, and the terminal produces tokens. They are different nodes.

*Checked: `session`/`unload` in `v2/node/worker/control.rs`.*

---

## 6. How a node picks a backend

A node knows only the adapter contract. Everything below it is swappable.

| Registered name | Attached backend |
| --- | --- |
| `mock` | arithmetic only; no device |
| `mock-instant` | immediate response; for ordering tests |
| `llamacpp` | llama-server (HTTP) |
| `vllm` | vLLM server |
| `sglang` | SGLang server |
| `llamacpp-staged` | layer-split execution (exposed only on hosts with a prepared executable) |

llama.cpp comes in two shapes. With `llamacpp`, one process holds the whole model and answers over HTTP, so
it is a single node and the chain length is 1. `llamacpp-staged` cuts the model into layer ranges loaded on several nodes,
and the chain length equals the node count. Backends that lay out the model themselves, such as vLLM and SGLang, participate with chain length 1.

It matters in practice that `mock` is always in the build. Without a GPU or model files you can bring up the whole fleet
and run routing, ordering, batching and cancellation end to end, and because computation is replaced with arithmetic the results are deterministic.
If two runs give different results, the difference is in P4.

*Checked: `registry` in `entrypoints/agent/src/adapters/mod.rs`.*

---

## 7. Batching ① — what goes into a batch

**The order for filling one logical batch.** Decode goes in first at 1 row each, and prefill fills the remaining rows with a rotating water-fill.

- Attention models fill the logical batch up to `llama_n_batch`, and llama.cpp does the splitting at `n_ubatch`.
- Recurrent and hybrid models require the same width per sequence, so one call produces exactly one physical UBATCH.
- Verify and Replay are single indivisible transactions — they must sit inside one physical UBATCH.

**The width of a group is set by the population, not by "the free slots right now".** Ready + in flight + waiting is divided by the number of windows
to derive the cohort width. A group does not widen just because one request briefly came back, and a request that has emitted its whole prompt but is not yet
settled stays in its cohort without widening new groups.

**While generation is live, prefill uses only its share.** If even one decode is in progress, prefill rows are
limited to `mixed_prefill_rows`. Pure prefill uses the whole token budget — in the example fixed by the tests,
128 rows during generation and 512 rows for pure prefill. This is a non-preemptive unit of work, not preemption.

**`PREFILL_PATIENCE = 8`.** If a prompt is still waiting after decode has had 8 consecutive batches, the next batch is
the prompt's turn. It is a cap, not a share. The code comment says so itself — *8 is not a measured value. It is only the minimum needed for a bound
to exist, and it has never been judged against throughput on real hardware.*

A prepared selection does not consume fairness until it is accepted. A rejected or cancelled candidate does not use up its turn,
and plans that belong to someone else or are stale are rejected by name. The selection layer is pure — neither KV nor execution authority is committed here.

*Checked: `v2/scheduler.rs` (`Phase`·`Demand`·`PREFILL_PATIENCE`·`PreparedPlan`), `PipelinePolicy::select` in `v2/scheduler/pipeline.rs`.*

---

## 8. Batching ② — when to send

Sending a chosen batch immediately makes a queue form at the tail; always waiting loses depth.
So **the time things actually took is collected to predict the cost of the next piece of work.**

Observe → predict → project → decide. Per stage it accumulates Frame round-trip times as samples, estimates per-stage time for the candidate batch shape
from the profile, adds the not-yet-committed in-flight work across all stages as a FIFO, and then decides.

| Verdict | Meaning |
| --- | --- |
| `PurePrefill` | no generation — full width |
| `DecodeOnly` | no prefill rows |
| `Cold` | a batch this node did not send is open — no profile |
| `CalibrationWait` | not enough samples yet |
| `Admit` | fits within the budget |
| `DeferPrefill` | exceeds it — no prefill this time |
| `ProgressProbe` | sends one share even if the target is not met, to avoid starvation |

Giving milliseconds via `P4_STAGED_PREFILL_SERVICE_MS` sets a microsecond budget. If it is not given, this policy is off entirely.

The code nails this down itself — **this is a prediction policy, not execution, not KV authority, not transport credit, and not a response-time guarantee.**
The projection leaves out unmeasured transport and return latency, and it makes no promise about the inter-token gap the client experiences.
The delay for gathering decode only is capped at 2 ms.

Separately, if enough batches are already in flight, the head holds the plan for a moment. The basis for that is also left in a comment, with numbers —
batches that arrived while the tail was busy waited p50 128 ms for the batch ahead, and 63% of all batches did. One batch spends a fixed cost of about 55 ms
from the tail back to the first layer. If there is a slot, it sends immediately even if thin — an earlier experiment that waited for width
regardless of slots lost 26%.

**All of these knobs are off by default.** `DECODE_MEMBERS` · `PREFILL_MEMBERS` · `PREFILL_ROWS` ·
`PREFILL_ROWS_PER_REQUEST` · `MAX_OPEN_BATCHES` · `MAX_ISSUE_ROWS` · `MIN_BATCH_ROWS` ·
`PREFILL_FRAGMENTS` · `PIPELINE_BATCHING` · `MIXED_BATCH_ROWS` · `MIXED_PREFILL_ROWS` · `PREFILL_SERVICE_MS`.
If they are not turned on, the existing path runs unchanged. The figures above are the basis of the hypothesis, not a promotion result.

*Checked: `v2/scheduler/service.rs` (`ServiceSample`·`ServiceVerdict`·`ServiceBudget::decide`), `v2/node/worker/service.rs`, `v2/node/worker/drive.rs`, defaults in `v2/node/state.rs`.*

---

## 9. What a load looks like — each node holds weights and KV for its own range

Example of an 80-layer model split across four nodes:

| Node | Layers | Device | What that node holds |
| --- | --- | --- | --- |
| node 0 | `[0, 20)` | `CUDA0` | weights · KV · compute buffer — for this range only |
| node 1 | `[20, 40)` | `CUDA1` | 〃 |
| node 2 | `[40, 60)` | `ROCm0` | 〃 |
| node 3 | `[60, 80)` | `MTL0` | 〃 |

Even with the same `n_ctx`, KV cost differs by node. In a past observation it was 173.5 / 63.3 / 157.7 / 126.1 MB —
because the layer makeup differs per range, and the pipeline's limit is set by the most expensive node.

There is a mechanism to confirm that plan and reality match. Declaring `--expect-layer-device begin:end:name`
checks that the ranges cover the whole cut with no gaps and no overlaps, and queries the device placement twice, before load (PLAN) and after load (LOAD),
and compares them. Matching total memory alone does not pass. Ranges to be left on the CPU are also declared explicitly.

"Node = one GPU" is not a rule. One device can hold several stages, one stage can use several devices,
and some ranges can be placed on the CPU.

*Checked: `stage_begin`/`stage_end` in `staged/adapter/src/config.inc.rs`, `server/src/runtime/stage_memory_plan.hpp`, `docs/llamacpp-stage-memory.md`.*

---

## 10. What crosses the network during inference

**Neither weights, nor the KV cache, nor the compute buffer leave the node.**
What crosses in one step is the bundle of tensors at the cut boundary — the in-flight batch.

| What | Size | Movement |
| --- | --- | --- |
| Weights | tens to hundreds of GB | loaded once, never moved afterwards |
| KV cache | tens to hundreds of MB per node and request | never moved — so a request is bound to the set of nodes holding its KV |
| Per-step transfer | a few boundary tensors | 31·27·23 for gemma-4, 81 transfers per step. 1 for the Qwen family |

Tokens produced by the terminal return to the head and go out **only after approval**. The head compares the authority fingerprint of the original submission
with the issuance evidence returned by the tail, and then produces the output.

So the bandwidth required between nodes is far smaller than for tensor parallelism. There is a price instead — latency
accumulates as stages are added, and if only one request flows at a time the earlier stages sit idle. Filling that idle time is the subject of sections 7 and 8.

*Checked: `PhysicalCapsule`/`Tensor` in `v2/capsule.rs`, the `prepare_outputs` call site (head) in `v2/node/worker/release.rs`, the observation table in `docs/adapter-batching-layers.md`.*

---

## 11. KV cache persistence

Each stage holds the KV for its own layer range, so save and restore happen separately on each node.
llama.cpp's public state API is used as is.

1. **Persist** — `llama_state_seq_get_size_ext` / `llama_state_seq_get_data_ext`
2. Write the manifest, the state bytes and a checksum together to `<kv-root>/<key>.lkv`
3. **Cell reclaim** — after saving, clear the KV cells with `llama_memory_seq_rm`
4. **Restore** — read the file and put it back with `llama_state_seq_set_data_ext`; the next decode is not allowed until that upload finishes

Eligibility to restore is decided by six manifest items: `build_identity`, `runtime_identity`, `context_identity`,
`kv_format` (K=type;V=type;flags), `token_position`, `checksum`/`bytes`.
**If any one differs, it is rejected** — state from a different build, a different context or a different KV type is not restored.

- If `--kv-root` is not given, the feature is reported as off. There is no path that silently keeps state only in memory.
- A runtime whose memory is dirty because a decode failed rejects **all** of save, restore and delete.
  llama.cpp does not say which sequence was damaged, so possibly torn state is never written to a file.
- State for a single sequence larger than 128 MiB is rejected.

This is storage for continuing conversations, not a failure-recovery mechanism.

*Checked: `save`/`restore` in `server/src/runtime/llama_stage_runtime_kv.cpp`, manifest comparison in `state_store.cpp`, capability settings in `main.cpp`.*

---

## 12. MTP and other speculative methods — not automatic

**"If llama.cpp supports it, it is supported automatically" does not hold on this path.**
On the stage-split path, proposal, verification and rollback are state that crosses node boundaries, so the design is deliberately the opposite:
new methods added upstream are not switched on automatically.

- **Implemented**: only `COMMON_SPECULATIVE_TYPE_DRAFT_MTP` — the method where the model itself proposes the next tokens.
- **Rejected**: requiring a separate draft model fails **LOAD** with `CAPABILITY_UNAVAILABLE: draft_context_and_proposal_state_not_in_hop`,
  and any other speculative method fails it with `CAPABILITY_UNAVAILABLE: proposal_accept_rollback_state_not_in_hop`.

A comment explains why the support list is written down in one place — if call sites scanned the upstream enum,
a new enumerator would be silently misclassified as supported.

Things that exist specifically for MTP: `Verify` and `Replay` are first-class phases in the scheduler, and both are atomic transactions.
The tail stage produces proposals and manages proposal state per sequence. The memory plan measures the draft context's
usage together with everything else before load. The compat patches include the MTP tail stage and the speculative sequence lifetime
as separate items.

**The place that really is automatic is elsewhere — the device backend.** The stage runtime calls `ggml_backend_load_all()`
and that is all; it has no CUDA, Vulkan, HIP, Metal or OpenCL branches at all. Model architectures, quantization, samplers and grammars
likewise use llama.cpp's own implementation as is. This is where the line falls between what comes along for free and what needs explicit implementation
because it is state that crosses boundaries.

*Checked: `LlamaPlan::requests_unsupported_speculative` in `compat/p4_llama_compat.cpp`, `StageRuntime::load` in `runtime/llama_stage_runtime.cpp`.*

---

## 13. Models that hold more than KV — only what is declared can be split

llama.cpp memory is not just one plain KV. The pinned upstream has 10 implementations:

| Stage residency declaration | Implementations |
| --- | --- |
| Declared (4) | `llama-kv-cache` · `llama-kv-cache-iswa` · `llama-memory-recurrent` · `llama-memory-hybrid` |
| Not declared (6) | `llama-kv-cache-dsa` · `llama-kv-cache-dsa-iswa` · `llama-kv-cache-dsv4` · `llama-kv-cache-msa` · `llama-memory-hybrid-idx` · `llama-memory-hybrid-iswa` |

The gate has two layers. The default is **reject** (`linkcpp_stage_residency_supported = false`), and
an implementation must explicitly override it with `true` before it can be split.

1. Once in the factory, as a compile-time constant — for a partial stage without a declaration, memory is not created at all.
2. Again at context creation, through a virtual call — it throws with "stage residency not declared".

iSWA has no storage of its own and delegates to two caches, base and swa. Boundaries that would split a reused KV region
are rejected by the constructors of those two.

Before load, memory is computed per device, split into `model` (weights) · `context` (KV and other state) · `compute` (execution buffers).
The sum of the three is compared with the device's free memory, and if it does not fit, LOAD fails **before allocating**.

So "models that hold extra cache are handled too" is only half true. Having extra storage captured in the plan and resident on a stage is
limited to the 4 declared kinds; sparse-attention families such as DSA, DSV4 and MSA have no declaration yet and are rejected when split.
On the path that loads a whole model onto one node, they behave exactly as upstream.

*Checked: the list of memory implementations in `upstream/src`, patches `staged/compat/451b89bae/0016·0017·0022`, `server/src/runtime/stage_memory_plan.hpp`.*

---

## 14. Summary — what this gives developers

1. **Run large models by adding nodes.** The limits of one GPU and one machine stop being the limit on model size.
   Because the split is by layer range, adding nodes only adds communication at one boundary.
2. **Swap backends.** A node knows only the adapter contract. Register one name and one implementation, and
   not a single line above it changes.
3. **Borrow platform support.** llama.cpp already handles device support, and the impact of upstream changes stays inside the pin directory.
4. **Pinpoint the slow part.** The agent's queue depth and the number running inside a node are reported separately, so
   when things slow down, the observations show whether P4 or the backend is holding the work.

To be clear about the boundaries — **there is no TLS, authentication or authorization.** The design trusts addresses to identify themselves,
so use it only inside a trusted network. **There is no durable state either.** When an agent restarts its nodes disappear,
and the record of what should exist is held by OUTER.
