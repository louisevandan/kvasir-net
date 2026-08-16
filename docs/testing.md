# Testing

```bash
cargo test --workspace          # from apps/p4
cargo fmt --all -- --check
```

Three levels, and each catches what the one below cannot.

## Pure

The parts that decide things are pure functions with no I/O, because they are
the easiest to get subtly wrong and the most expensive to debug once running.

`worker::judge` — an envelope and our address in, a verdict out. A test pins
that the verdict never depends on lane or chain: the moment it did, forwarding
would have to understand its traffic.

`node::window` — a ready lap goes before fresh prefill, a window never mixes
lanes, the ceiling is never exceeded, expired work is reported rather than
dropped.

`node::outcome` — the three cases that are the whole routing behaviour of an
inference. A middle node hands work on even having produced no text; the end
either finishes once or reports a token and starts a lap; a token is enqueued
before the lap it precedes.

`envelope`, `frame`, `message::wire` — round trips, and refusal of every
truncation, every trailing byte, every unknown tag.

## Simulated

`layers/agent/tests/simulation.rs` stands real agents on real sockets with
mocks behind the adapter boundary: chains of one, two and three stages; forty
arrivals against a ceiling of four; batching actually happening; relay through
an agent owning no node; sustained arrivals; per-route ordering; concurrent
chains sharing agents; a failing backend.

`layers/agent/tests/lifecycle.rs` takes the other half — a load reported per
stage, a declared ceiling, an unload, a failing load, deadlines, cancellation.

`layers/agent/tests/network.rs` makes the network the slow thing. Each agent
sits behind a relay carrying a declared latency, jitter, width or stall, and
nothing in P4 is told it is there. A chain across slow links, jitter that must
not become reordering, a link that seizes for a tenth of a second, a narrow
link the lanes must not become the buffer for, one bad hop that must not
serialise the rest, and a slow link with a slow backend at once. One test
exists only to guard the others: a relay that fell out of the path would leave
them passing *faster*, so it asserts the crossing really took the time.

`layers/agent/tests/resilience.rs` is what a listener meets over weeks. Garbage,
a header cut in half, a body cut short, wrong magic, wrong version, an
impossible length, a header claiming 900KB followed by nothing; 600 connections
opened and abandoned three ways against a ceiling of 256; frames for a node
that does not exist. Every one of them ends by checking the agent still serves,
and the long mixed run ends by checking every queue drained, no reply is still
expected, and no node lost or orphaned anything.

`layers/agent/tests/queues.rs` watches the two-tier queue while the mock is
deliberately holding hops, which is the only time the interesting state exists.
Concurrent arrivals against one slow node, a chain whose middle stage is the
slow one, and decode still being dispatched while prefill is queued. Each
asserts both halves of the attribution: the node's queue deep *and* the
agent's lanes shallow. Node depth alone would also be satisfied by an agent
that had backed up with it.

`layers/service/tests/two_process.rs` does the same through the message
vocabulary: nodes created, a model loaded, an inference chained, all by frame.

`layers/service/tests/many_nodes.rs` gives each agent more than one. Everything
else here places one node per agent, which hides whether the nodes are separate
queues and lifecycles behind a single address. Four nodes over two agents, each
created and loaded on its own, then a chain that visits each machine twice; and
two chains sharing those nodes at once, where the claim is only that no route
takes another's tokens.

## Fleet

The level that found three defects the other two could not — a starved node
select, a starved lane, and a silent drop on the send path. Unit tests cannot
reach them because each needs sustained load across processes.

```bash
p4-agent 0.0.0.0:52001 THIS_HOST          # one per machine
p4-drive 0.0.0.0:52003 HOST_A:52001,HOST_B:52001 1000 64 mock-instant THIS_HOST:52003
```

Agents bind `52001-52008`, which the fleet's hosts already admit. Name each
agent and the driver by an address the others can reach: a run where everything
calls itself `127.0.0.1` completes its prefill and then stops after one token,
because the lap resolves to whichever machine is holding the frame.

Four claims, printed as a verdict: every request answered, none failed, every
stream in order, one terminal per route. Run each chain shape several times
against long-lived agents — a defect that only appears on the second run
against the same process is exactly the kind this level exists for.

Run with `P4_AGENT_STATS=1` when something is wrong. Lane depth beside node
depth says which side of the adapter boundary is slow; the per-node counts say
at which step a frame stopped existing.
