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
- NodeSlot admission is controller-owned and permit-based: `INGRESS_ACCEPTED` means ownership and binding validation succeeded; the async relay takes one permit before adapter dispatch, while binding lifecycle takes every permit.
- Pipeline throughput has three independent axes: arrival is the E2E request window (`concurrent_requests`), bounded waiting belongs to the Agent/adapter queues, and GPU service capacity is the deployment's declared `max_sequences`. An E2E `parallel` value must not be copied into `node_spec.p4_max_inflight`.
- Independent does not mean unset. A NodeSlot defaults to one permit, so a `node_spec` that omits `p4_max_inflight` makes the Agent the narrowest tier and releases one execution at a time. The adapter then receives one request per batch, its coalescing window has nothing to merge, and the native scheduler reports `peak=1` against its full `limit`. A run in that state still completes every request with zero errors, which is why `throughput.verdict` exists.
- <a id="agent-local-native-transport"></a>At `MODEL_LOAD`, the Adapter derives the IPC domain of every local stage from its owning Agent address; the controller cannot claim local memory affinity. Native Pipeline selects shared memory only for adjacent stages with that common domain and otherwise retains TCP.
- `MODEL_LOAD.stage_plan.load_options` carries the controller-selected common load policy and its reproducible batch-limit calculation. Agent routing treats it as opaque JSON; the concrete adapter filters it. Unsupported process-start options fail explicitly instead of becoming hidden defaults. See [model-load.md](model-load.md).

## Rejection gate

- <a id="rejection-gate"></a>The Agent keeps node, controller and binding state until the binding is unloaded, and refuses any request that contradicts it. `authorization/` owns those rules; adding authentication changes that folder alone.
- Concrete adapters re-check binding generation, but none of them keys on `controller_id`. Ownership therefore exists only at this boundary and cannot be delegated downstream.
- A slot never caches its adapter handle. `registry::resolve` joins the slot with `adapters[adapter_id]` at use time, so adapter identity has exactly one source.
- Re-registering an adapter under a different endpoint is refused while NodeSlots are attached; those slots were authorised against the previously registered runtime.
- `MODEL_UNLOAD` removes a binding only when the request names the deployment that was recorded, so a mismatched unload cannot silently drop a live binding.

## Execution credit

- <a id="execution-credit"></a>`INGRESS_ACCEPTED` follows ownership and binding-generation validation, not raw socket receipt or GPU credit. It means the request is queued for adapter dispatch; a deadline can still fail while it waits.
- The async relay takes one slot permit immediately before adapter dispatch and holds it through the terminal response; `MODEL_LOAD` and `MODEL_UNLOAD` take every permit.
- Sizing and exclusivity are independent. Raising `p4_max_inflight` changes concurrency only; a lifecycle transition still waits for every permit, so a binding stays stable until it is explicitly unloaded.
- Ownership and binding readiness are checked before acceptance and again after relay credit acquisition, so a queued request cannot run against a reloaded binding.
- This is Agent policy over opaque NodeSlot capacity, not a CUDA, llama.cpp, or native Pipeline data-plane mechanism.

## Invariants

- `NODE_CREATED(ready)` precedes `MODEL_LOAD`.
- `MODEL_BOUND(ready)` precedes `EXECUTE`.
- `MODEL_UNBOUND` removes only that binding.
- A co-resident handler is invoked directly; a distinct process is encoded with P4B1/TCP. “Same address” is insufficient when a process boundary remains.
- Concrete workers do not participate in P4 neighbour/data-plane traffic.
- No NodeSlot execution may overlap its ModelLoad or ModelUnload lifecycle transition.
