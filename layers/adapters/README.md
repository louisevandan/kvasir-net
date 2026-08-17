# The adapter contract, and everyone who implements it

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
| `llamacpp/staged/` | llama.cpp, split across machines | Patch series and preparation script present. Nothing else: it is **two** artifacts still to be written, a C++ server holding one layer range and a Rust adapter that owns that process, and it needs a cut-set path opened in `adapter/` first. [STAGED.md](../../../../STAGED.md) is the plan. |

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
`Accept: text/event-stream`, answered as SSE. vLLM and SGLang serve the same
OpenAI-compatible surface, which is why one self-contained adapter shape covers
all three.

Reservation figures came from `"reserved_bytes"` in the native process logs.
Report them per stage: a model spread over layer ranges finishes when its
slowest piece does, and one total hid a non-final stage reserving the whole
model.
