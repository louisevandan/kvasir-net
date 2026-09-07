# The adapter contract, and everyone who implements it

> 문서 지위 (2026-09-06): **구성요소 안내**. 해당 경로의 API·구조 안내다. 과거 service 경로와 현재 event 경로는 실제 호출자로 구분한다.
> 현재 목표·상태·순서는 [실행 로드맵](../../docs/distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](../../docs/document-map.md)를 따른다.

[`adapter/`](adapter) is the contract: what a node asks of a backend. It
depends on nothing at all, not even the protocol — an adapter reaching for a P4
type is reaching past its own contract, and having no dependency to reach
through is what makes that a compile error rather than a convention.

Every other directory is a backend implementing it. Nothing above this layer
changes when one is added: a backend is a name registered in
[`entrypoints/agent/src/adapters`](../../entrypoints/agent/src/adapters) and an
`Adapter` implementation, and that is the whole cost.

The contract sits here rather than beside `agent/` and `protocol/` so that
`adapters/` is self-describing — the thing to implement and the things that
implement it, in one place. What it must not gain is a dependency on any of
them.

| Path | Backend | State |
| --- | --- | --- |
| `adapter/` | none — the contract | Zero dependencies. The one file every backend below is written against. |
| `mock/` | none — arithmetic | Implements the interface. Ships in every build, so a fleet can be loaded without hardware. |
| `llamacpp/served/` | llama.cpp, vLLM, SGLang | One implementation, registered under three names, because the three serve the same HTTP. Proved against a stock `llama-server` on Metal and on CUDA, and against wire-level servers behaving like each. |
| `llamacpp/staged/` | llama.cpp, split across machines | Rust adapter and staged server sources are present behind the staged compatibility boundary. The patch series and preparation script remain the replaceable upstream integration path; native transaction parity and long-run acceptance are separate gates. |

## What a new adapter owes

`Distribution` — `Staged` if we own the boundary between the pieces and a chain
of nodes spans the model, `Internal` if the backend spreads it itself and
presents one entry point. vLLM and SGLang are `Internal`; only a `Staged`
adapter can be one link of a chain.

`start(Work, &dyn EventSink)` — a procedure. It may block, since a real backend
waits on a device, and the node runs it on a blocking thread for exactly that
reason. Results arrive as events, never as a return value.

Three works: `Load` and `Unload` name a deployment and carry an opaque plan;
`Hop` carries a **window of sequences**, because batching is the node's decision
and the cohort is the shape the runtime works in. Hidden state never appears —
that transfer stays inside the backend under either distribution.

Only the end of a chain produces a token. A staged adapter that is not last
returns an outcome with no text and no stop, and the node hands the work on.

## Cache contract and implementation order

The mock adapter is the first conformance target for durable KV. A real llama
adapter may be incomplete while it preserves this boundary; P4 and the node
must not wait for a llama.cpp-specific cache API before testing policy.

[`Adapter`](adapter/src/lib.rs) is event-based: `start(Work, &dyn EventSink)`
returns immediately and reports `Cached`, `Failed`, or the other outcomes
through the sink. [`Work::Cache`](adapter/src/work/cache/mod.rs) is one
instruction for one `sequence`, one `stage_id`, one deployment `generation`,
and one `operation_id`. The adapter must not replace `sequence` with an
execution request id.

Cache mutations use this transaction contract:

```text
PreparePersist | PrepareRestore | PrepareDiscard
  → Commit or Abort
Reconcile      → receipt only; no mutation
```

`Prepare` is not visible as committed state. `Commit` is replay-safe,
`Abort` compensates the prepared mutation, and `Reconcile` exposes
`Absent`, `Prepared`, `Committed`, `Aborted`, or `Inconsistent` without
guessing. Restore must be bounded: unavailable capacity, a stale generation,
or a missing/corrupt durable record becomes an explicit refusal or failure.
The node admits no follow-up Hop until Restore has completed for every stage.

The mock must cover ordering, transaction compensation, idempotent Discard,
capacity refusal, generation/identity fencing, restart recovery, receipt
corruption, and multi-stage partial failure. Once those cases pass, the llama
adapter may be implemented later behind the same contract. Its remaining work
is backend-specific: materialising KV, selecting/evicting device slots, and
proving long-run native acceptance. The served adapter remains unsupported for
durable cache until it implements these operations.

## Backend HTTP contracts, for whoever writes these

The previous adapters spoke to their backends over HTTP. Their Rust is gone
(see `git log` before the v6 core landed) but the contracts they used are:

**Linker Pipeline runtime**, via the host supervisor:

| Purpose | Request |
| --- | --- |
| Capability | `GET /api/runtime` |
| Start a deployment | `POST /api/runtime-groups` with the stage plan as the body |
| Status | `GET /api/runtime-groups/{deployment}` — `phase=running` is terminal |
| Release | `DELETE /api/runtime-groups/{deployment}` |
| Batched inference | `POST /api/runtime-groups/{deployment}/v1/chat/completions/batch` |
| Streamed inference | Upgrade `/api/pipeline-inference-stream`, protocol `linker-pipeline-inference-stream-v1` |

**Stock llama-server**: `POST /v1/chat/completions` with
`Accept: text/event-stream`, answered as SSE.

That is one adapter's private business with one backend. It says nothing about
this layer: the agent speaks P4 over a socket and nothing else. Three backends
answering a similar wire is not a reason to give them one implementation —
they diverge as they move, and each is getting its own.

Reservation figures came from `"reserved_bytes"` in the native process logs.
Report them per stage: a model spread over layer ranges finishes when its
slowest piece does, and one total hid a non-final stage reserving the whole
model.
