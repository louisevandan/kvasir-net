# Adapter interface

> Document status (2026-09-06): **Component guide**. This is an API and structure guide for this path. The legacy service path and the current event path are distinguished by their actual callers.
> Current goals, status and ordering follow the [execution roadmap](../../../docs/distributed-batching-roadmap.md); document authority and reading paths follow the [document map](../../../docs/document-map.md).

What a node asks of whatever executes its work. `layers/adapters/*` implement
it; nothing here names a backend.

## Current event boundary

`src/node_adapter/` owns the backend-neutral `NodeAdapter` and completion mailbox.
`try_offer` / `try_take` / `try_publish` move opaque P4 events without blocking;
Full returns the undelivered event and Closed is a distinct outcome. The adapter,
not this crate, interprets model-specific content types and completion meaning.

The mailbox's capacity listener is a bounded, RAII notification registration.
Register before attempting publication, keep the registration while waiting,
and retry the actual operation after a wake. A wake reserves no slot and proves
neither delivery nor KV completion. Combining this notification with input and
shutdown is the concrete actor's responsibility; adding the API alone does not
remove a worker's blocking wait. Callback code runs synchronously outside mailbox
locks and must be short, nonblocking and nonpanicking.

One logical asynchronous reader is supported. Sender closure wakes that reader
after disconnection; buffered events remain readable before Closed. Receiver
closure wakes capacity waiters after disconnection. These are local mailbox
lifetime rules, not a distributed graceful-drain protocol.

This crate depends on `p4-protocol`, `serde`, and `serde_json`; it has no llama.cpp
or device-backend dependency. Adding opaque transport notification does not give
P4's common layer authority over the adapter's flight ledger or batch policy.

## Service Work API (not the event worker)

Folders are cut by what changes them, not by size.

| Path | Purpose | Moves when |
| --- | --- | --- |
| `src/work/distribution/` | Whether a backend owns its own parallelism. | A new *kind* of backend appears — not a new backend. |
| `src/work/load/` | Materialising and releasing a share of a model. | A plan gains a key. |
| `src/work/hop/` | One pass over a window of sequences. | Throughput work changes the execution shape. |
| `src/event/report/` | The reporting vocabulary. | Observability needs grow. |
| `src/event/sink/` | How an event travels. | Ideally never. |
| `src/lib.rs` | The trait itself. | Rarely. |

In this service API, the hop is the execution unit. A node hands the adapter one hop and is
told when that hop ends; between hops the node drains its own queue. That is
where deadlines and cancellation are decided, because there is no way to
interrupt a hop in progress and no need for one — not starting the next hop is
the whole mechanism.

A hop carries a batch of sequences rather than one, because the cohort window
is the shape the workload actually has. Load reports progress per stage since a
model is distributed as layer ranges and the slowest stage decides completion.

The service hop description is not evidence that the current event worker has
the same deadline, cancellation, queue-drain or completion behavior. Follow the
actual caller and the roadmap's current verification record.
