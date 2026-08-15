# Adapter interface

What a node asks of whatever executes its work. `layers/adapters/*` implement
it; nothing here names a backend.

| Path | Purpose |
| --- | --- |
| `src/work/` | The unit a node hands over: a load, an unload, or one hop. |
| `src/event/` | What comes back, and it comes back as an event, never a return value. |
| `src/lib.rs` | The trait itself. |

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
