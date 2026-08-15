# API

## Wire contract

[`contract/message/mod.rs`](../layers/protocol/src/contract/message/mod.rs) and [`codec/frame/mod.rs`](../layers/protocol/src/codec/frame/mod.rs) own P4B1 v6: fixed 16-byte little-endian header, `P4B1` magic, version byte `6`, kind byte, u32 payload length, 1 MiB frame cap, 256 KiB text-field cap, and a 256-element cap on any repeated field. Every payload starts with a bounded `route_id` and absolute `deadline_unix_ms`; the remaining bytes are the message payload. v6 is intentionally incompatible with v5: `DRAFT_REPORT` no longer names transformer parts, and the version byte makes a stale peer fail instead of misreading the new payload.

| Subprotocol | Messages | Contract |
| --- | --- | --- |
| Agent inventory | `INVENTORY_QUERY`, `HARDWARE_REPORT` | Best-effort OS/CPU/GPU facts plus registered adapters and slots. |
| Adapter lifecycle | `ADAPTER_REGISTER`, `ADAPTER_REGISTERED` | Adapter self-registers ID, kind, endpoint, descriptor. |
| Node lifecycle | `NODE_CREATE`, `NODE_CREATED` | Creates a model-independent NodeSlot on a registered adapter. |
| Model binding | `MODEL_LOAD`, `LOAD_PROGRESS`, `DRAFT_REPORT`, `MODEL_BOUND`, `MODEL_UNLOAD`, `MODEL_UNBOUND` | Loads, measures, replaces, and unloads versioned model partitions without deleting the NodeSlot. |
| External ingress | `INGRESS_SUBMIT`, `INGRESS_ACCEPTED` | Agent forwards external work to ControllerProcessor; it issues missing session IDs. |
| Inference | `EXECUTE`, `TOKEN`, `DONE`, `ERROR` | Executes only a ready `(node_id, deployment_id, binding_id, runtime_generation)`. |

`ControllerInstance` exposes `inventory()`, `createNode()`, `loadModel()`, `unloadModel()`, `health()`, and streaming `infer()`. Every call accepts `signal` and `timeoutMs`; timeout writes the absolute wire deadline and sends route-local `CANCEL` without closing the pooled socket. `infer()` sends `INGRESS_SUBMIT`, not a direct node instruction.

Every request, stream, terminal response, field list, and `CANCEL` guarantee is mapped in [message-pairs.md](message-pairs.md).

Every message's semantic class, queue, terminal rule, correlation key, and allowed communication direction is defined once in [`catalog/mod.rs`](../layers/protocol/src/catalog/mod.rs). The Agent task envelope and generic worker contract are documented in [task-runtime.md](task-runtime.md).

`ModelLoad.stage_plan` and `NodeCreate.node_spec` are bounded opaque JSON objects. Only the selected adapter interprets them. `stage_plan.load_options` has a canonical backend-neutral schema for flash attention, mmap, KV cache, and measured dynamic batching; see [model-load.md](model-load.md). `EXECUTE.options` is likewise adapter-owned: stock llama.cpp passes all non-protected OpenAI-compatible values; Pipeline currently selects `max_tokens`, `temperature`, `top_p`, `top_k`, and `seed`.
