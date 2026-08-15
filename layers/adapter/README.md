# Adapter interface

What a node asks of whatever executes its work. `layers/adapters/*` implement
it; nothing here names a backend.

Folders are cut by what changes them, not by size.

| Path | Purpose | Moves when |
| --- | --- | --- |
| `src/work/distribution/` | Whether a backend owns its own parallelism. | A new *kind* of backend appears — not a new backend. |
| `src/work/load/` | Materialising and releasing a share of a model. | A plan gains a key. |
| `src/work/hop/` | One pass over a window of sequences. | Throughput work changes the execution shape. |
| `src/event/report/` | The reporting vocabulary. | Observability needs grow. |
| `src/event/sink/` | How an event travels. | Ideally never. |
| `src/lib.rs` | The trait itself. | Rarely. |

The hop is the only execution unit. A node hands the adapter one hop and is
told when that hop ends; between hops the node drains its own queue. That is
where deadlines and cancellation are decided, because there is no way to
interrupt a hop in progress and no need for one — not starting the next hop is
the whole mechanism.

A hop carries a batch of sequences rather than one, because the cohort window
is the shape the workload actually has. Load reports progress per stage since a
model is distributed as layer ranges and the slowest stage decides completion.

This crate has no dependencies, including on `p4-protocol`. An adapter that
needs a P4 message type is reaching past its own contract.
