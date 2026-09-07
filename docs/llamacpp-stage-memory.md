# llama.cpp stage memory contract

> 문서 지위 (2026-09-06): **분야 계약·구현과 구별**. 소유 분야의 계약/목표를 읽되 구현 완료로 간주하지 않는다. 현재 개발 순서와 충돌하면 로드맵의 명시적 이관을 따른다.
> 현재 목표·상태·순서는 [실행 로드맵](distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](document-map.md)를 따른다.

The adapter must preserve stock llama.cpp memory semantics while making each
mutable storage region physically resident on exactly one pipeline stage.

Status: design authority for the staged llama.cpp compatibility layer. Plain
attention KV now separates nonresident metadata from resident buffers. Every
other memory implementation rejects a partial stage until it explicitly
declares and implements the same guarantees; an unsplit `[0, n_layer)` model
remains compatible with upstream behavior.

## Ownership boundary

| Owner | Owns | Must not own |
| --- | --- | --- |
| P4 | deployment identity, submission delivery, routing, cancellation, deadlines, typed capacity feedback | Prefill/Decode classification, UBATCH composition, model layers, KV/state layout, graph cuts, sampling |
| llama.cpp adapter | GGUF interpretation, memory topology, stage placement, physical UBATCH scheduling, rollback, sampling, stage protocol | P4 routes or request identity policy |
| official llama.cpp | architecture graph and memory semantics | Linker/P4 transport or fleet policy |

P4 submits independent requests through the
[deployment contract](deployment-adapter-contract.md). It never sends a batch
plan. The native adapter turns ready submissions into one exact physical
`llama_ubatch` and forwards that same row/position/sequence/output description
through every stage.

## Upstream authority

The compatibility layer derives behavior from these official paths; it must
not recreate them with model-name branches:

Preparation enforces this boundary. Compatibility diffs may not modify
`src/models/` or add `LLM_ARCH_*`/model-implementation casts, and adapter
runtime sources may not include llama.cpp private model/context/graph/memory
headers. A model-format workaround is an upstream update or contribution, not
an adapter branch.

| Upstream source | Contract consumed by the adapter |
| --- | --- |
| `src/llama-model.cpp#llama_model::create_memory` | Selects plain KV, iSWA, MSA, DSA/DSV4, recurrent, hybrid, reuse, and cross-context share semantics. |
| `src/llama-memory.h#llama_memory_i` | Defines batch preparation, update, sequence operations, state I/O, and memory accounting for every memory implementation. |
| `src/llama-batch.h#llama_ubatch` | One physical batch carries tokens, positions, sequence membership, and per-row output flags; Prefill and Decode are not separate llama.cpp batch types. |
| `src/llama-context.cpp#llama_context::decode` | `init_batch` chooses physical UBATCHes; no-slot is retryable status `1`; failed graph execution rolls back the affected memory positions. |
| `src/llama-model.cpp#llama_model::dev_layer` | Stock backend/RPC placement makes a layer's weights and memory use that layer's backend. |

The pinned, pristine source is
`layers/adapters/llamacpp/upstream`. Linker changes remain only in the
versioned `staged/compat/<sha>/` series.

## Normalized memory topology

The adapter needs one model-independent description derived while
`create_memory` builds the official memory object:

```text
MemoryRegion {
  id                 stable storage-root identity
  kind               upstream memory implementation and role
  logical_layers     every layer that reads or mutates this root
  canonical_layer    upstream reuse/share target, if any
  tensors            type, shape, strides, and byte count
  update_semantics   shift/copy/rollback/state operations touching the root
  resident_stage     the single authoritative stage
}
```

`kind` is diagnostic and capability data, not scheduling policy. The source of
truth remains the object created by upstream. New upstream memory kinds are
therefore detected as undeclared capability and rejected, never silently
treated as ordinary KV.

## Residency rules

1. Semantic existence and physical residency are separate. Every stage keeps
   enough tensor metadata to build the stock full graph. Only its resident
   regions own real backend buffers.
2. Sequence/cell/position allocation metadata is replicated. Every stage sees
   the same physical UBATCH and therefore makes the same metadata transition.
   K/V/R/S/compressed payload bytes are not replicated.
3. Each mutable storage root has one writer and one physical owner. A stage
   graph may read or write only roots resident on that stage.
4. Reuse and share form connected components. A layer boundary may not split a
   component. The planner co-locates the component or the adapter rejects the
   plan before context allocation.
5. Cross-context share is unsupported until both contexts can prove one common
   owner and lifecycle. It must fail closed rather than allocate independent
   copies.
6. Memory-update graphs and state I/O operate on resident regions only.
   Replicated metadata is serialized once per stage fragment only where the
   upstream format requires it.
7. A pruned graph retaining an unbuffered nonresident memory/weight tensor is a
   load/decode error, not an optional diagnostic.

These rules reproduce stock RPC ownership without copying its control plane:
RPC has one logical graph and one backend owner per storage root; P4 has
independent processes, so the adapter explicitly transports immutable graph
cut tensors and keeps mutable roots stage-local.

## Legal cut

A requested boundary `b` is legal only when all of the following hold:

```text
for every mutable region R:
  all R.logical_layers lie on one side of b

for every graph edge u -> v crossing b:
  u is immutable after the producing stage completes the UBATCH

for every final stage graph tensor t:
  t has a real buffer, or t is an allocated stage input/compute tensor
```

The first rule makes upstream `reuse` and `share` authoritative. It replaces
model-specific exclusions such as "Gemma cannot be split here" with a general
storage-alias constraint.

## Execution equivalence

For every submitted physical UBATCH, distributed execution is valid only if:

1. every rank receives identical token/embedding mode, positions, sequence
   membership, and output flags;
2. every stock graph operation executes exactly once across the ordered stage
   partition;
3. every crossing immutable tensor is transmitted once with exact type, shape,
   strides, alias, and bytes;
4. every memory metadata transition equals stock llama.cpp;
5. every mutable byte is read/written only by its authoritative stage;
6. terminal logits, sampling, stop reason, and generated text equal the stock
   or one-stage adapter path for deterministic sampling;
7. retryable capacity and failure rollback preserve the upstream return-code
   meaning.

## Current implementation gap

| Surface | Current state | Required change |
| --- | --- | --- |
| Weights | Nonresident weights are metadata-only and the GGUF bytes are not loaded. | Keep; the reachability audit is now mandatory. |
| Plain KV | Nonresident K/V tensors are metadata-only; allocation, copy, shift, state I/O, and byte accounting visit resident layers only. | Prove stock/one-stage/multi-stage output equivalence with a real GGUF. |
| iSWA/MSA/DSA/DSV4/recurrent/hybrid | Partial stages are default-denied by the upstream-selected memory implementation, while an unsplit model remains valid. | Implement the same region contract per upstream memory composition before opting a type in. |
| Reuse/share | A boundary splitting one reuse component and partial-stage cross-context share are rejected before backend-buffer allocation. | Move the same alias-component result into planner preflight for earlier diagnostics. |
| Graph cut | Nonresident weight/KV reachability is fatal, but layer provenance still partly falls back to tensor-name parsing. | Consume explicit memory-region provenance. |
| Planner accounting | Can charge KV by owning layer. | Accept layer-local evidence only after runtime reports resident memory regions. |
| Frontend utilities | The stage runtime still includes llama.cpp `common.h`, `sampling.h`, and `speculative.h`. This is model-agnostic but remains an upstream-update surface. | Hide these behind one versioned generic bridge; never replace them with adapter-owned model or sampler branches. |

## Integrated implementation gate

The work is one correctness change with three simultaneous outputs, not a
sequence of speculative model fixes:

- Compatibility layer: plain-KV metadata/residency separation, mandatory graph
  reachability check, and a default-deny stage-residency capability on
  `llama_memory_i`.
- Adapter/planner: reject undeclared memory kinds and illegal alias cuts before
  allocating a context; report actual resident bytes by memory kind and layer.
- Oracles: stock vs one-stage vs multi-stage deterministic logits, zero real
  bytes for nonresident regions, resident-only update/state behavior, and a
  mutation that re-admits one nonresident root and must fail.

No throughput or GPU-utilization result is accepted until one meaningful
single-session response passes these equivalence gates. Mixed Prefill/Decode
load and GPU saturation are performance gates after correctness, using the
adapter-owned continuous-window path rather than P4 batching.
