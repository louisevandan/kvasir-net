> Historical record: requirements, status and measurements from the time of the standalone repository. The current layout and usage follow the [HF guide](../../../README.md). The complete original is in the Git bundle preserved during the migration.

> 2026-09-14: P4 integration is being implemented under the user's §0 instruction. For the current status of the earlier unconnected/read-only descriptions, the [integration specification](../../integration/README.md) and its acceptance report take precedence.

# Architecture and P4 wiring

Status: design proposal. P4's current observations are kept separate from the new implementation plan.

## Planned execution structure

```text
OUTER: model, topology, SLO, request and deployment selection
  -> P4 event transport / broker / node lifecycle
    -> this project's Rust adapter bridge (RetainedNodeAdapter implementation planned)
      -> bounded node-local IPC
        -> Python model worker
          -> per-model partial forward + per-request KV/recurrent state
            -> Transformers / PyTorch / quantization libraries / device kernels
```

The first design carries inter-node boundary tensors on the P4 event delivery path as adapter-owned payloads.
Python workers do not send directly to other nodes on their own and bypass P4 backpressure.
Splitting and reassembly of large payloads, byte reservations and buffer lifetimes are bound in the adapter codec and the bridge.
If a separate data transfer path is needed later, it is handled as a separate contract that verifies the same ownership and capacity preservation.

## Responsibilities

The Python executor is specific to the selected model. Each model can have a different implementation, state structure, boundary tensors and quantization path,
and no common base class or automatic model discovery is required.
What is shared as a contract is the P4 wiring, event delivery and execution identity, and resource and output ownership.
Code inside the Rust bridge can also be reused, but building a framework for many models is not a prerequisite.

| Component | Owns | Outside its boundary |
| --- | --- | --- |
| OUTER | model/quantization combination, placement, request goals, deployment, operating policy | Directly manipulating engine state |
| P4 common core | opaque events, routing, node lifecycle, generic delivery/capacity ownership | Interpreting model layers, tokens, KV or quantization formats |
| Rust bridge and adapter ledger | request/execution identity, reservations, batch publication, settlement, output approval, P4 and IPC wiring | Exposing PyTorch tensors or private engine types to P4 |
| Python model executor | partial load/compute, physical KV/state, device completion, kernel selection, measurement | Emitting tokens to OUTER directly before settlement |
| Per-model module | forward semantics, legal cuts, tensor/state schema, special ops, sampling inputs | Guessing the cache structure of other models |

Python computes the actual tokens and state, and the adapter ledger approves the request attribution of results and external output.
The ledger commit and the effect intent are bound together, but network/device calls themselves are not put inside a pure transaction.
If an error leaves it unclear whether execution happened, it stays fenced/uncertain, and the same work is not unconditionally re-run.

## Model partitioning

A simple example for a 60-layer decoder is A=embedding+0..19, B=20..39, C=40..59+norm/lm_head.
This is not a claim that it is a legal cut or a balanced placement for any particular model.
Each stage holds the weights of its assigned layers and the per-request state, and the boundary tensors the model requires are sent between stages.
The tail's next token goes back as the next decode input after ledger approval.

Even a pure dense model must preserve position/RoPE/mask/global layer index/tied weights.
With shared KV, cross-layer state, recurrent state or MoE, do not assume a single hidden state is all that is passed.
Group dependent layers into the same stage or define additional boundary state, and reject illegal cuts before loading.

Between hosts, PP (layer-wise partitioning) is considered first. TP across GPUs on the same fast host is a later option.
TP is verified separately, including quantization packing/group boundaries and collective constraints. `device_map="auto"` is not a multi-host executor.

## Batching

Transformers continuous batching, paged KV and chunked prefill are candidates for reuse.
A structure where each Python stage independently runs a full `generate_batch` does not guarantee distributed step consistency.
The request membership/positions/cancellation of the whole pipeline are defined by a single adapter scheduling contract, and stages consume approved executions.
The L0–L5 separation of the existing llama.cpp adapter is a design reference, not approval to reuse its source wholesale.

## Current P4 wiring points and future compilation

The 2026-09-13 reference HEAD is `d122125bafeaa6d32790761669f1bfa5868d8078`.
`RetainedNodeAdapter` is in `layers/adapters/adapter/src/node_adapter/mod.rs`, and
create in `entrypoints/agent/src/event_runtime/control.rs` currently builds only `llamacpp`.
`entrypoints/agent/Cargo.toml` assembles the concrete adapters. Dynamic Python plugin discovery is not a currently observed feature.

The recommended future wiring is for this repository to provide an independent Rust bridge crate and a Python worker package,
with the P4 composition root registering the new adapter kind and compiling them together.
Python code is not natively compiled into the Rust binary automatically, so a separate worker deployment/environment lock is needed.

| When | What to do | P4 changes |
| --- | --- | --- |
| Initialization complete | Docs and separate Git setup | none |
| Standalone implementation | Implement bridge/worker/test host in this repository; reference neutral P4 crates read-only if needed | none |
| Explicit integration work | P4 entrypoint dependency, kind registration, configuration, deployment bundle, neutrality tests | Only after a separate user instruction |

A sibling path dependency is a candidate during development. Reproducible deployment is decided as either a Git dependency with both revisions pinned or
a deployment package. Check the dependency graph so that, within one build, the P4 protocol/adapter crates are not
duplicated from different path/Git sources and split Rust type identity.
The target, lock, environment and outputs of the standalone build stay in this repository. No workspace member or submodule is added to P4 now.
