# Internals

## Decisions

- NodeSlot and concrete runtime are separate: vLLM may create an engine during ModelLoad while llama.cpp may reuse one; P4 records only the stable slot and binding result.
- Adapter endpoint registration is adapter-to-agent, never controller-to-agent; this prevents arbitrary controller route injection and survives repeated model loads.
- `binding_id + runtime_generation` is part of execution identity; reloading the same slot cannot run a stale session on a replacement runtime.
- ControllerProcessor owns missing-session issuance after `INGRESS_SUBMIT`; ingress transports remain interchangeable.
- `layers/runtime/src/foundation/transport` owns no hardware, lifecycle, or backend policy; `layers/runtime/src/domain/agent` owns mutable agent state.
- <a id="transport-neutral-dispatch"></a>`P4Handler` owns command semantics and `ResponseSink` owns stream emission; `InMemoryTransport` and `TcpTransport` differ only in delivery. This prevents controller, agent, and node routes from reimplementing lifecycle validation for a local fast path.
- Pipeline uses deployment ID as host runtime-group identity; a model unload deletes that concrete group but preserves the NodeSlot.
- Agent routing is a non-blocking `TaskHandler` above shared competing control/prefill/decode/response workers. Remote I/O and compatibility adapter calls execute outside those workers. Ordered output is a causal chain: final delivery of one response releases registration of the next response task.
- Requests and responses are separate `TaskEnvelope` instances linked by `causation_id`; no handler waits for its request's response.
- Equal source/target Agent IDs select the common in-memory route. Transport selection does not change message validation, response ordering, or handler code.
- NodeSlot admission is controller-owned and permit-based: execution takes one permit, binding lifecycle takes every permit, and saturation is rejection rather than hidden queueing.
- <a id="agent-local-native-transport"></a>At `MODEL_LOAD`, the Adapter derives the IPC domain of every local stage from its owning Agent address; the controller cannot claim local memory affinity. Native Pipeline selects shared memory only for adjacent stages with that common domain and otherwise retains TCP.
- `MODEL_LOAD.stage_plan.load_options` carries the controller-selected common load policy and its reproducible batch-limit calculation. Agent routing treats it as opaque JSON; the concrete adapter filters it. Unsupported process-start options fail explicitly instead of becoming hidden defaults. See [model-load.md](model-load.md).

## Execution credit

- `INGRESS_ACCEPTED` follows NodeSlot credit acquisition, not raw socket receipt; a controller can treat it as the point at which a selected binding may begin execution.
- The credit is held until the adapter sends a terminal P4 response; `MODEL_LOAD` and `MODEL_UNLOAD` acquire the entire slot so they cannot replace a live binding.
- This is Agent policy over opaque NodeSlot capacity, not a CUDA, llama.cpp, or native Pipeline data-plane mechanism.

## Invariants

- `NODE_CREATED(ready)` precedes `MODEL_LOAD`.
- `MODEL_BOUND(ready)` precedes `EXECUTE`.
- `MODEL_UNBOUND` removes only that binding.
- A co-resident handler is invoked directly; a distinct process is encoded with P4B1/TCP. “Same address” is insufficient when a process boundary remains.
- Concrete workers do not participate in P4 neighbour/data-plane traffic.
- No NodeSlot execution may overlap its ModelLoad or ModelUnload lifecycle transition.
