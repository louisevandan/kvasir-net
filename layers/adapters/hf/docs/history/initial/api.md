> Historical record: requirements, status and measurements from the time of the standalone repository. The current layout and usage follow the [HF guide](../../../README.md). The complete original is in the Git bundle preserved during the migration.

> 2026-09-14: P4 integration is being implemented under the user's §0 instruction. For the current status of the earlier unconnected/read-only descriptions, the [integration specification](../../integration/README.md) and its acceptance report take precedence.

# Planned API and event contract

Status: design draft. The operation/field/state names below are not a currently runnable API or a finalized wire schema.

The implemented lower transport contract is [local IPC framing](../../transport/framing/README.md).
The standalone Qwen worker's step/release/shutdown and plan format are in the [Qwen scripts](../../models/qwen3_5_0_8b/README.md).
The full set of semantic operations of the P4 bridge below and the product payload schema are still planned.

This contract is an execution and ownership contract between the bridge and the worker dedicated to the selected model.
It does not mean a general-purpose Python API that every model must implement. The payload and boundary/state schemas
are defined for the selected model, and capabilities are used only to state the supported scope of that implementation.

## External P4 boundary

The current reference boundary is `p4_adapter::node_adapter::RetainedNodeAdapter`.
The main surface is `try_offer_retained`, `peek_retained_completion`, `try_take_retained_matching`,
`poll_take_retained`, `snapshot` and `completion_storage_snapshot`.
Exact signatures are rechecked against the pinned P4 revision, and the Rust trait is not exposed to Python as is.

Full is a temporary inability to accept that returns ownership of the original event and buffers/reservations. It is distinct from Closed.
Successful acceptance is not execution completion; completion goes out as a separate event.
The adapter kind candidate is `hf-transformers`, and content-type names and versions are finalized during implementation.
New fields and tensor formats go in the concrete adapter payload; the common P4 envelope is not extended per model.

## bridge ↔ worker semantic operations

| Operation | Input | Result/completion condition |
| --- | --- | --- |
| Inspect/Capabilities | worker/runtime identity, model recipe | Actually supported operations, dtypes, kernels and state/cut constraints |
| Load | model/artifact manifest, stage range, device, budget, generation | Assigned weights loaded, kernel warmup and measured memory, or an explicit failure |
| BindSession | load identity, session incarnation, required path/boundary schema | Agreement on state/boundaries; loading and path binding are kept separate |
| ExecuteStep | issue ID, immutable membership, per-request position/length, tensor bundle | Actual device completion and state progress, boundary tensor/tail result |
| Cancel/Quiesce | request incarnation, cutoff issue | Evidence of execution stop/termination and the state stop point |
| Release | same identity, settled state range | Confirmation that the actual KV/state and reservations were returned |
| Unload | load generation, session/flight cleanup conditions | Worker resource reclaim; success is forbidden if uncertain or not cleaned up |

Persist/Restore and speculative operations are outside the first scope. Features not in capabilities are rejected before execution.
A timeout does not guarantee an immediate interrupt of a GPU kernel. Receiving a cancel is kept separate from the physical stop and return.

## Required identity

Every execution, completion and return binds the model artifact identity, load generation, stage ID, session incarnation,
request ID and issue/operation ID. The meanings of request ID, KV sequence ID and operation ID are kept distinct.
A late completion/RELEASE from a previous generation must not be able to change the state of a new request.
Redelivery of the same identity is handled with a defined replay result, and a network retry is not turned directly into a GPU re-execution.

## Boundary tensor bundle

It is planned to include the version, model boundary schema, tensor names/roles, dtype, shape, layout, byte length,
request/row mapping, position/mask-related metadata and issue identity.
The element count product, byte ranges, limits and unknown dtype/schema are validated. pickle and raw GPU pointers are not used as wire formats.
A process-local handle is an IPC-internal value with an owner, a lifetime and a release, and is not passed around like a remote address.
Returning a transfer buffer as an ack and the KV/state being stopped and released are different pieces of evidence.

## Planned state transitions

```text
load: unloaded -> loading -> ready -> draining -> unloaded
request: queued -> admitted -> executing -> settled -> releasing -> released
error: unknown whether executed -> uncertain/fenced -> reconcile or explicit failure cleanup
```

The actual ledger must distinguish per-issue accepted/settled and effect pending/sent/uncertain.
Even if the tail computes a token, it is not sent to OUTER before settlement.
A partial stage failure, a stale receipt or a wrong membership must not damage approved output, reservations or state.
Durable exactly-once after a process restart is not provided by this state diagram alone.

## Required model/run manifest items

- model ID/revision, original/quantized tensor hashes, tokenizer/chat-template hash, manufacturer code revision.
- Python/Transformers/PyTorch/quantization library/kernel and P4/bridge revisions.
- Per-module bit/group/symmetry/scale/packing/exclusion rules, calibration dataset digest, actual execution dtype.
- Stage layer ranges, shared modules, boundary/state schema, device, physical host, memory pool.
- context, request count, prefill token budget, decode/flight limits, KV/state/workspace/transfer budgets.
- workload, sampling, stop/EOS, quality/SLO tolerances, supported capabilities, fallback policy.

The manifest syntax/schema and command-line options have not been created yet. They will be finalized at the first implementation, in JSON/TOML or similar,
preserving the required semantics above, and written together with a validator and counterexamples.
