# P4 request/response message pairs

This is the complete implemented P4B1 v5 request/response map. The source of truth is [`Message`](../layers/protocol/src/contract/message/mod.rs), its semantic [`catalog`](../layers/protocol/src/catalog/mod.rs), and its binary [`codec`](../layers/protocol/src/codec/mod.rs); the Node.js caller surface is [`ControllerInstance`](../tools/controller/client/controller-instance.mjs). `*` means zero or more frames; `ERROR` is the terminal alternative unless noted.

## Frame and correlation rules

Every P4 TCP frame has a 16-byte little-endian header: magic `P4B1`, version `5`, kind, reserved zeroes, payload byte length, reserved zeroes. The payload begins with `u32 route_id_length`, UTF-8 `route_id`, and `u64 deadline_unix_ms`, followed by the message fields below. A frame is at most 1 MiB; a string field is at most 256 KiB. `u32`, `u64`, and `f32` values are little-endian.

| Correlation field | Used by | Rule |
| --- | --- | --- |
| frame `route_id` | every message on a multiplexed transport | Unique transport stream identity. It is preserved end-to-end and is never inferred from a possibly duplicated business ID. |
| frame `deadline_unix_ms` | every routed task | Absolute deadline; zero disables it. Expired queued/peer work returns `ERROR`. |
| `request_id` | ingress, execute, health, error | Identifies one execution or probe. |
| `operation_id` | node/model lifecycle | Identifies one create/load/unload operation. |
| `ingress_id` | external ingress | Identifies an external submission before execution begins. |
| `session_id` | ingress, execute, token, done | Empty ingress value is replaced by the in-process controller role's controller-scoped session ID. |
| `(node_id, deployment_id, binding_id, runtime_generation)` | execute | Must name the currently ready binding; otherwise execution returns `ERROR`. |

## Complete control-plane pairs

| Request kind | Normal response sequence | Request fields (wire order) | Response fields (wire order) | Owner |
| --- | --- | --- | --- | --- |
| `INVENTORY_QUERY` (34) | `HARDWARE_REPORT` (35) | `controller_id`, `request_id` | `agent_id`, `report_id`, `snapshot` JSON | Agent |
| `ADAPTER_REGISTER` (36) | `ADAPTER_REGISTERED` (37) | `adapter_id`, `adapter_kind`, `endpoint`, `descriptor` JSON | `adapter_id`, `detail` | adapter → Agent |
| `NODE_CREATE` (38) | `NODE_CREATED` (39) | `controller_id`, `operation_id`, `node_id`, `adapter_id`, `node_spec` JSON | `operation_id`, `node_id`, `adapter_id`, `state`, `detail` | Agent forwards to selected adapter |
| `MODEL_LOAD` (40) | `LOAD_PROGRESS` (19)* → `DRAFT_REPORT` (20)* → `MODEL_BOUND` (41) | `controller_id`, `node_id`, `operation_id`, `deployment_id`, `binding_id`, `model`, `plan_revision`, `stage_plan` JSON | progress: `operation_id`, `node_id`, `percent`, `detail`; draft: `operation_id`, `node_id`, `model_bytes`, `kv_bytes`, `layer_bytes`, `ffn_bytes`, `detail`; bound: `operation_id`, `node_id`, `deployment_id`, `binding_id`, `runtime_generation`, `state`, `detail` | Agent forwards; adapter owns concrete load |
| `MODEL_UNLOAD` (42) | `MODEL_UNBOUND` (43) | `controller_id`, `node_id`, `operation_id`, `deployment_id`, `binding_id` | `operation_id`, `node_id`, `deployment_id`, `binding_id`, `detail` | Agent forwards; adapter owns concrete unload |
| `HEALTH_CHECK` (16) | `HEALTH` (17) | `controller_id`, `node_id`, `request_id` | `request_id`, `node_id`, `ready`, `detail` | Agent forwards to adapter |

`HARDWARE_REPORT.snapshot`, `ADAPTER_REGISTER.descriptor`, `NODE_CREATE.node_spec`, and `MODEL_LOAD.stage_plan` are opaque JSON at the P4 layer. Only the selected adapter interprets its backend configuration. The canonical `stage_plan.load_options` object is defined in [model-load.md](model-load.md). `NODE_CREATED(state=ready)` creates the Agent-owned model-free NodeSlot; `MODEL_BOUND(state=ready)` then creates or reuses concrete backend state and records a new `runtime_generation`.

## External inference pair

The external client never sends `EXECUTE`. The in-process controller role validates the target binding and acquires the Agent-owned NodeSlot execution credit, then sends the acknowledgement and transforms ingress into `EXECUTE` on the same client stream.

```text
external ControllerInstance → Agent: INGRESS_SUBMIT
Agent/controller role: validate binding + acquire NodeSlot credit
Agent/controller role → external client: INGRESS_ACCEPTED
controller role → NodeProcessor/Agent → adapter: EXECUTE
adapter → Agent/controller role → external client: TOKEN* → DONE
                                      └─────────────── ERROR
```

| Request / response | Fields (wire order) | Meaning |
| --- | --- | --- |
| `INGRESS_SUBMIT` (32) | `controller_id`, `ingress_id`, `request_id`, `session_id`, `node_id`, `deployment_id`, `binding_id`, `runtime_generation`, `max_tokens`, `temperature`, `prompt`, `options` JSON | External prompt and all adapter-selectable sampling options. Blank `session_id` is allowed. |
| `INGRESS_ACCEPTED` (33) | `ingress_id`, `request_id`, `session_id` | Sent only after a valid binding and one NodeSlot execution credit are acquired. Its session ID is either the supplied value or the newly issued value. |
| internal `EXECUTE` (1) | `controller_id`, `node_id`, `deployment_id`, `binding_id`, `request_id`, `session_id`, `runtime_generation`, `phase`, `position`, `max_tokens`, `temperature`, `prompt`, `options` JSON | The ControllerProcessor creates it with `phase=PREFILL`, `position=0`; the adapter receives it. |
| `TOKEN` (2)* | `controller_id`, `node_id`, `request_id`, `session_id`, `phase`, `position`, `index`, `text` | A non-empty text chunk. It is a P4 stream event, not a claim of tokenizer-token equivalence. |
| `DONE` (3) | `controller_id`, `node_id`, `request_id`, `session_id`, `reason`, `generated_tokens` | Terminal successful execution result. |

For a continuing generation, the current adapter may internally use `phase=DECODE`, a later `position`, and retained backend KV state. P4 does not expose a separate externally callable `PREFILL` or `GENERATE` message; the phase is carried by `EXECUTE` and `TOKEN`.

## Error and cancellation behavior

| Input / condition | Implemented terminal response | Notes |
| --- | --- | --- |
| Any rejected control or inference operation | `ERROR` (4): `request_id`, `detail` | `request_id` is the original request/operation where available. |
| unknown adapter, wrong controller ownership, no ready binding, stale generation, or NodeSlot admission full | `ERROR` | Agent rejects before `INGRESS_ACCEPTED`/forwarding; admission is immediate rejection, not an application prefill queue. |
| adapter-native load or inference failure | `ERROR` | Adapter converts its own failure to a P4 terminal frame. |
| `CANCEL` (5): `request_id`, `reason` on the original `route_id` | `ERROR(request_id, detail="cancelled: …")` from Agent | Agent aborts the route relay and removes prepared/routing state. The stock llama.cpp adapter shuts down an active HTTP socket; Pipeline detaches route output but already-issued native compute may finish. |

## Concrete backend and node-to-node traffic

The following are real execution exchanges but **not P4B1 message pairs**; they must not be mistaken for missing P4 frame kinds.

| Boundary | Concrete exchange | P4 translation / ownership |
| --- | --- | --- |
| Adapter ↔ host native supervisor | model group start/status/delete and native process lifecycle | Triggered by `MODEL_LOAD`/`MODEL_UNLOAD`; only terminal P4 lifecycle frames are returned. |
| Adapter ↔ native Pipeline HTTP endpoint | completion request with selected prompt/sampling fields → chunked/SSE text | Adapter emits `TOKEN*` then `DONE` or `ERROR`; backend request shape is adapter-private. |
| native Pipeline stage 0 ↔ stage 1 | hidden-state tensors forward; sampled token returns to the first stage | Native data plane, not Agent-routed P4. The controller remains outside this exchange after ingress acceptance. |
| stock llama.cpp adapter ↔ llama-server | OpenAI-compatible completion request → SSE chunks | Adapter filters P4 `options`, owns its backend connection, then emits P4 `TOKEN*` and `DONE`/`ERROR`. |

## Observed 256-session path

The successful 256-way E2E used the following pair sequence independently for every request: `INGRESS_SUBMIT → INGRESS_ACCEPTED → EXECUTE → TOKEN* → DONE`. It also used one shared setup sequence: `INVENTORY_QUERY → HARDWARE_REPORT`, `NODE_CREATE → NODE_CREATED`, `MODEL_LOAD → LOAD_PROGRESS* → DRAFT_REPORT → MODEL_BOUND`, and teardown `MODEL_UNLOAD → MODEL_UNBOUND`. The complete 500-token-cap, 256-session request/response plan, trace, and report are in [runtime-evidence.md](runtime-evidence.md#2026-08-09-scripted-256x500-requestresponse-proof).
