# P4

The communication layer for distributed inference. Agents carry work between
machines; a concrete adapter runs it.

One process type: the agent. There is no controller and no node process — a
node lives inside an agent, and an agent reaching another agent is the same
path as an agent answering the outside.

```
OUTER ──▶ entry agent ──┬──▶ agent ──▶ node ──▶ adapter ──▶ backend
                        ├──▶ agent ──▶ node ──▶ adapter ──▶ backend
                        └──▶ agent ──▶ node ──▶ adapter ──▶ backend
```

## Attaching a backend

This is the whole of it:

```rust
// entrypoints/agent/src/adapters/mod.rs
registry.register_fn("llamacpp", |node| Arc::new(LlamaCpp::new(node)));
```

plus one implementation of [`p4_adapter::Adapter`](layers/adapter). Nothing
else changes — not the wire, not the queue, not the worker, not the node, not
the chain. See [layers/adapters/README.md](layers/adapters/README.md) for what
an adapter owes and for the backend HTTP contracts.

## Running it

```bash
p4-agent 0.0.0.0:19311            # one per machine
p4-drive 0.0.0.0:19310 HOST:19311,HOST:19312 1000 64 mock-instant
```

`P4_AGENT_STATS=1` makes an agent print, once a second, its lane depths, its
node depths, and counts for every step at which a frame could go missing. Two
of those numbers answer the question this layer exists to make answerable: a
shallow agent queue beside a deep node queue puts a slowdown below the adapter,
and the reverse puts it here.

## Layout

| Path | What it is |
| --- | --- |
| [`layers/protocol`](layers/protocol) | The wire. An envelope every hop reads and a body only its destination does. |
| [`layers/adapter`](layers/adapter) | What a node asks of a backend. No dependencies, no backend names. |
| [`layers/agent`](layers/agent) | The core: one queue, workers, nodes, chains. |
| [`layers/service`](layers/service) | Body vocabulary, the agent's own duties, and the adapter registry. |
| [`layers/adapters`](layers/adapters) | Concrete adapters. `mock` ships in every build. |
| [`entrypoints/agent`](entrypoints/agent) | The process. |
| [`tools/drive`](tools/drive) | Drives a fleet and reports a verdict. |

## Documents

| Goal | File |
| --- | --- |
| What the layer is and why it is shaped this way | [docs/overview.md](docs/overview.md) |
| The wire | [docs/api.md](docs/api.md) |
| How a message moves through an agent | [docs/architecture.md](docs/architecture.md) |
| Invariants and what breaks if they go | [docs/constraints.md](docs/constraints.md) |
| Decisions, and the defects behind them | [docs/internals.md](docs/internals.md) |
| Running it and driving a fleet | [docs/usage.md](docs/usage.md) |
| Testing | [docs/testing.md](docs/testing.md) |
| Measured behaviour of the backend below | [docs/runtime-evidence.md](docs/runtime-evidence.md) |
| The revision that produced all this | [P4_REVISION_PLAN.md](P4_REVISION_PLAN.md) |
