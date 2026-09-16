# OUTER sessions and the KV lifecycle

> Document status (2026-09-06): **Area contract, distinct from implementation**. Read it as the owning area's contract and goals, not as a completed implementation. Where it conflicts with the current development order, follow the explicit hand-over recorded in the roadmap.
> Current goals, status and ordering follow the [execution roadmap](distributed-batching-roadmap.md); document authority and reading paths follow the [document map](document-map.md).

## Status and scope

This document defines the layer boundaries for OUTER disconnects, heartbeat, stopping inference, KV Persist/Restore/Discard,
and retention-period GC. P4 is a message broker, not a policy engine or a KV scheduler.
Policy that P4 does not interpret is decided by a policy component resident in the agent, and
actual KV slot, GPU and memory scheduling is decided by the concrete adapter.

The current node-bound worker uses `target:node` as the ordering key and uses the frame `route`
only as a handle. This change reduces contention between different routes arriving at the same node,
but it is applied after the main queue's Control/Response/Decode/Prefill lane selection.
Issue-order guarantees per `sequence_id` are therefore still a target contract,
not something the current implementation has completed.

## Responsibilities by layer

| Layer | Responsibilities | Does not do |
| --- | --- | --- |
| Agent-resident session policy | heartbeat, disconnect verdict, request ownership, issuing Persist/Restore/Discard, TTL GC | KV byte placement, GPU slot selection |
| P4 agent/protocol | message delivery, identifier preservation, per-session delivery order, carrying results, refusals and failures | sampling, decoding, eviction policy, deciding when to restore |
| Concrete adapter | inference boundaries, securing, evicting and restoring KV slots, actual concurrency and device scheduling | redefining the meaning of P4 messages |

The related implementation entry points are [agent (`Agent`)](../layers/agent/src/agent/mod.rs), [node runner (`Node`)](../layers/agent/src/node/runner/mod.rs),
[service message vocabulary (`ToAgent`/`ToNode`)](../layers/service/src/message/mod.rs),
and [cache coordinator (`CacheTransaction`)](../layers/service/src/cache.rs).

## Identifiers

Identifiers do not substitute for one another.

| Identifier | Meaning | Lifetime |
| --- | --- | --- |
| `return_channel` | OUTER's logical channel and ownership axis | kept as a logical channel across reconnects |
| `ingress_generation` | connection generation of that `return_channel` | incremented on every socket reconnect |
| `sequence_id` | the conversation session that KV preserves | kept after Persist |
| `request_id` or `route` | handle for an individual frame or execution | one message or one execution |
| `operation_id` | one Persist/Restore/Discard operation | for the duration of that operation |

The durable key for KV is `sequence_id`, not `request_id`. Multiple execution requests with the same
`sequence_id` continue one conversation state, and each execution may have its own
`request_id` and `operation_id`. Which session a later inference continues after Restore
is always decided by an explicit `sequence_id`.

## P4 ordering contract

The order P4 guarantees is delivery order, not policy.

> All messages with the same `sequence_id` are delivered to each target node in issue
> order. No order is guaranteed between different `sequence_id`s.

`sequence_id` is the ordering key, and `route` or a separate request handle must be the key that identifies
a frame. Using the same field for both causes the following problems.

- Restore and the following Hop cannot be tied into the same session order.
- They are spread across different workers and the order is lost.
- `claim`/`remove` on the node queue may mistake several frames for the same item.

Current node worker selection uses `target:node` in [agent dispatch](../layers/agent/src/agent/mod.rs),
and ordinary peer traffic uses `route`. This choice
serializes per-node ingress onto one worker, but it does not remove the lane priority of the [main queue](../layers/agent/src/queue/main/mod.rs).
A change that fully separates `ordering_key` from `frame_handle`
and guarantees issue order is therefore a separate acceptance condition.

Delivery order does not mean execution completion. A later inference in the same session cannot become runnable before Restore
completes, and where that boundary is enforced depends on the adapter's
capability.

## OUTER heartbeat and disconnect

A heartbeat must be an application-level message, not a TCP liveness check.
The following is the target wire/policy; it does not mean it is implemented in the current P4 code.

```text
Agent → Outer: Ping(nonce, issued_at)
Outer → Agent: Pong(nonce, ingress_generation)
```

When implemented, the agent must validate the nonce and `ingress_generation`, and transition to `Disconnected` only when the configured
miss threshold is exceeded, not on a single failure. A reconnect registers as a new connection generation,
and responses and ACKs from the previous connection must not intrude on the new session's ownership.

## Disconnect and KV flow

On disconnect, the policy component must find all executions owned by that `return_channel` and `ingress_generation`.
The current implementation has no primitive to enumerate this set of executions or to cancel it
as a set, so this is an acceptance condition. A running hop may continue up to the hop boundary,
according to P4's current cancellation semantics,
and the next hop does not start.

```text
Connected
  └─ heartbeat miss threshold exceeded
       └─ Disconnected
            ├─ request cancel/stop of related inference
            ├─ attribute progress state to sequence_id
            └─ issue Persist(sequence_id)
```

Once Persist completes, the session is `Detached`. Because durable KV exists, not running Restore right now
does not mean the session is lost.

## Restore is a message request

Restore is not a special P4 scheduling command; it is one request that goes into the queue.

```text
Detached
  └─ Restore(sequence_id)
       └─ handled by the adapter in its own scheduler
            ├─ wait until the current inference boundary
            ├─ secure or evict a KV slot
            ├─ restore the KV
            └─ allow later inference after restore completes
```

P4 does not slot Restore into a decode window, and it does not define capacity reservation, victim selection or the GPU
transfer method. An adapter may overlap Restores of different sessions,
but it must keep the order between a Restore and later inference for the same `sequence_id`.

The default adapter capability is the safe mode that handles Restore and inference at the same node
barrier. Only an adapter that advertises a separate capability may allow cross-session overlap.
The session is not exposed as runnable until Restore has completed on every
stage.

### Mock adapter first, and the line for deferring llama

The restore policy of protocol, agent and service is fixed first with the mock adapter. For the real llama
adapter, the KV file format, GPU slot reclaim and device transfer optimization may be deferred as long as the following conditions
hold.

- The mock adapter passes the asynchronous event boundary of `Adapter::start` and the `Work::Cache`
  lifecycle.
- `sequence` is used as the durable key, and `operation_id` and `generation` are never mixed up or
  guessed.
- `PreparePersist/Restore/Discard` is always followed by `Commit` or `Abort`,
  and `Reconcile` does not change state.
- A later Hop cannot succeed before Restore completes, and capacity, generation and integrity states that cannot be handled
  are reported as `Refused` or `Failed`.

As long as this line is not crossed, P4's ordering, disconnect, retry and GC policy tests can be completed even if the llama adapter
does not yet actually store and restore KV. Judging the llama adapter complete
requires a separate acceptance that connects this contract to real KV and device slots.

## Results and retries

So that the policy component can decide whether to retry, cache results must distinguish at least
the following.

| Result | Meaning | Policy |
| --- | --- | --- |
| `Cached` | cache operation completed (`bytes` included) | next inference or GC state update |
| `Accepted` | an ordinary command was received | wait for that command's later result |
| `Done` | inference generation finished | proceed to the next inference |
| `Refused` | cannot be handled now, but durable state is preserved | retry at the time the adapter suggests |
| `Failed` | permanent failure such as corruption or no session | automatic retry forbidden |
| `Inconsistent` | the receipt and actual state cannot be determined | fail-close, reconciliation |

The current `CacheStatus.state` can mechanically carry `Inconsistent` on the reconciliation path.
By contrast, `CacheFailed.detail` is free text, so the failure path cannot distinguish
`Refused` from a permanent `Failed`. Failure codes and an optional retry-time field
remain as additional contract. This distinction is not about putting eviction policy into P4;
it is a contract for safely carrying the adapter's backpressure and the policy layer's retries.

## 30-day retention and GC

30-day retention is executed by a policy component resident in the agent, not by OUTER,
because the retention period must keep running after OUTER is gone.

- When Persist completes, `retain_until` is recorded in the durable manifest/receipt.
- The default policy is `retain_until = persisted_at + 30 days`.
- A successful Restore, or use under the policy, may renew the retention period.
- The agent scheduler queries the expiry list once a day and issues `Discard(sequence_id)`.
- Discard must be idempotent, and the local list entry is not deleted before the completion receipt is confirmed.
- So that GC works after an agent restart, the adapter must be able to enumerate its cache list.

The recommended management message has the following form.

```text
ListCaches(deployment_id)
  → sequence_id, retain_until, bytes, receipt_state
```

P4 does not interpret the retention periods in the list; it only carries the request and response. Having the adapter that holds the actual KV
provide the list and receipts reduces mismatches between the coordinator journal and the actual
KV.

## Current implementation and acceptance conditions

Foundations confirmed in the current implementation:

- [ToNode](../layers/service/src/message/mod.rs) carries Persist/Restore/Discard and
  Prepare/Commit/Abort.
- [CacheTransaction](../layers/service/src/cache.rs) coordinates cache receipts from multiple stages
  per operation.
- The node runner runs Cache as lifecycle work without mixing it with hops.
- Cancel validates `request_id`, `stream_id`, `return_channel` and `ingress_generation`
  together. Duplicate Cancels are accepted idempotently, and no new terminal is added to a request that is already
  terminal. Forced interruption of an already started hop is not guaranteed;
  cooperative handling up to the hop boundary is stated explicitly.
- A cancelled request receives, once per request, a replayable terminal `Failed` on the original `return_channel`,
  and additional queued carriers in the chain are discarded. The number of extra discards
  must be counted as a separate observation; it is not in the current status contract.

The required failure scenarios for the mock-adapter-first implementation follow the `Cache contract` table in [testing.md](testing.md).
Once that table passes, deferring the llama adapter does not block the protocol
implementation.

Claiming completion of this document requires separately verifying the following.

| Condition | Verification level |
| --- | --- |
| `sequence_id` becomes an independent durable wire identifier, not derived from `request_id` | wire/adapter contract |
| separation of the `sequence_id` ordering key from the frame handle | Pure: queue tests; Simulated: `layers/agent/tests/network.rs` |
| delivery order before and after Restore for the same sequence | Pure: queue tests; Simulated: `layers/service/tests/cache_in_a_deployment.rs` |
| blocking later inference before Restore completes | Simulated: `layers/service/tests/cache_in_a_deployment.rs`; Fleet: `tools/drive` |
| heartbeat miss threshold and `return_channel`/`ingress_generation` fencing | Pure: `layers/service/src/outer_policy.rs`; transport reconnect integration remains |
| enumeration of the per-connection execution set and cancellation as a set | Simulated: `layers/service/tests/protocol_in_flight.rs`; Fleet: reconnect run |
| 1 replayable terminal `Failed` per request and a count of extra carrier discards | Simulated: `layers/service/tests/protocol_in_flight.rs`; Fleet: status snapshot |
| the adapter refuses Restore in a bounded way and passes a retry time or failure code | Simulated: adapter tests; Fleet: `tools/drive` |
| delivery of the adapter's `Refused`/`Failed`/`Inconsistent` results | Pure: wire tests; Simulated: cache tests |
| cache enumeration after restart and 30-day GC | Pure retention policy: `layers/service/src/outer_policy.rs`; adapter enumeration/scheduler integration remains |
| partial residency during a multi-stage Restore is not exposed as runnable | Simulated: `layers/service/src/cache.rs`, `layers/service/tests/cache_barrier.rs`, mock recovery tests |

These items do not mean that P4 interprets KV policy. They mean that P4, as a broker, must not lose ordering,
identifiers or result delivery, so that the adapter and the agent policy can each carry out their own
responsibilities.

Related documents: [protocol.md](protocol.md), [architecture.md](architecture.md),
[api.md](api.md), [testing.md](testing.md).
