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

plus one implementation of [`p4_adapter::Adapter`](layers/adapters/adapter). Nothing
else changes — not the wire, not the queue, not the worker, not the node, not
the chain. See [layers/adapters/README.md](layers/adapters/README.md) for what
an adapter owes and for the backend HTTP contracts.

## Running it

```bash
p4-agent 0.0.0.0:52001 THIS_HOST  # one per machine
p4-drive 0.0.0.0:52003 HOST_A:52001,HOST_B:52001 1000 64 mock-instant THIS_HOST:52003
```

The second argument is what a process calls itself, and every reply is
addressed to it — across machines it has to be an address the others can reach.

`P4_AGENT_STATS=1` makes an agent print, once a second, its lane depths, its
node depths, and counts for every step at which a frame could go missing. Two
of those numbers answer the question this layer exists to make answerable: a
shallow agent queue beside a deep node queue puts a slowdown below the adapter,
and the reverse puts it here.

## Layout

| Path | What it is |
| --- | --- |
| [`layers/protocol`](layers/protocol) | The wire. An envelope every hop reads and a body only its destination does. |
| [`layers/agent`](layers/agent) | The core: one queue, workers, nodes, chains. |
| [`layers/service`](layers/service) | Body vocabulary, the agent's own duties, and the adapter registry. |
| [`layers/adapters`](layers/adapters) | The contract and everyone who implements it: [`adapter/`](layers/adapters/adapter) is what a node asks of a backend, with no dependencies and no backend names; the rest are backends. `mock` ships in every build. |
| [`entrypoints/agent`](entrypoints/agent) | The process. |
| [`tools/drive`](tools/drive) | Drives a fleet and reports a verdict. |

## Documents

| Goal | File |
| --- | --- |
| What the layer is and why it is shaped this way | [docs/overview.md](docs/overview.md) |
| Every crate, what it holds, and what is not built | [docs/implementation.md](docs/implementation.md) |
| The wire and the message vocabulary | [docs/api.md](docs/api.md) |
| Protocol audit, return routing, pipeline, options, and KV open decisions | [docs/protocol.md](docs/protocol.md) |
| OUTER sessions, heartbeat, and KV lifecycle boundary | [docs/protocol-outer.md](docs/protocol-outer.md) |
| Sealed staged MTP decision and implementation gate | [docs/protocol-mtp.md](docs/protocol-mtp.md) |
| How a message moves through an agent | [docs/architecture.md](docs/architecture.md) |
| Invariants and what breaks if they go | [docs/constraints.md](docs/constraints.md) |
| Decisions, and the defects behind them | [docs/internals.md](docs/internals.md) |
| Running it and driving a fleet | [docs/usage.md](docs/usage.md) |
| Testing | [docs/testing.md](docs/testing.md) |
| Distributed mock test plan | [docs/distributed-mock-test-plan.md](docs/distributed-mock-test-plan.md) |
| Measured behaviour of the backend below | [docs/runtime-evidence.md](docs/runtime-evidence.md) |
| The revision that produced all this | [P4_REVISION_PLAN.md](P4_REVISION_PLAN.md) |
