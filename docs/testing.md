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

`layers/agent/tests/topology.rs` tests the model rather than the wiring.
Everything the layer knows about the network is in the envelope — an absolute
address, a chain that travels whole, a reply address — so if that is really all
it needs, the shape is free. A row of agents that own nothing and relay; one
entry point in front of five workers; a chain that returns to a machine it
already visited; a machine that stops being reachable mid-flight, whose work
must be answered rather than left hanging; a partition that heals with nothing
restarted; and cut-and-heal cycles under continuous arrivals on a slow link.
The partitions assert they really partitioned — with the link up, the same work
would have finished long before the check.

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

## What the suite is, file by file

293 tests. The count matters less than the split: the levels catch different
things, and three defects in this layer survived every level but the fleet.

| Where | Tests | What it holds |
| --- | ---: | --- |
| `p4-protocol` | 46 | Round trips, and refusal of every truncation, trailing byte and unknown tag. Address parsing, advertised-address resolution, chain advance and restart. |
| `p4-agent-core` (unit) | 70 | The pure decisions — judge, window, outcome — plus the queue, the peer table and its retirement. |
| `p4-service` (unit) | 32 | Message encoding with explicit tags, the payload seam, the registry, the machine and status snapshots. |
| `p4-mock` | 19 | That the mock honours what it declares: widths, ceilings, per-position cost, the four faults. |
| `p4-adapter` | 9 | The contract's own small logic, including which id a fork leaves state under. |
| `p4-llamacpp` (unit) | 33 | HTTP framing, SSE, status refusal, chunk shapes including reasoning content, plan parsing, session behaviour. |
| `p4-link` | 7 | That a declared impairment is deterministic and irregular, and adds rather than replaces. |
| `p4-agent` | 6 | That the registry carries what this build claims and refuses what it does not. |
| `tests/simulation.rs` | 10 | Real agents on real sockets: chains of one, two and three stages; a crowd against a ceiling; batching; relay through an agent owning no node; ordering; concurrent chains; a failing backend. |
| `tests/lifecycle.rs` | 8 | Load reported per stage, a declared ceiling, unload, a failing load, deadlines, cancellation. |
| `tests/queues.rs` | 3 | The two-tier queue while the mock deliberately holds hops. Asserts **both** halves — node deep *and* lanes shallow — because node depth alone is equally satisfied by an agent that backed up with it. |
| `tests/network.rs` | 7 | The network as the slow thing: latency, jitter that must not reorder, stalls, a narrow link, one bad hop, and a slow link with a slow backend. One test exists only to guard the others — a relay that fell out of the path would leave them passing *faster*. |
| `tests/topology.rs` | 6 | The model rather than the wiring: a row of relaying agents, a star, a chain revisiting a machine, a partition answered by its deadline, a heal with nothing restarted. The partitions assert they really partitioned. |
| `tests/resilience.rs` | 6 | What a long-lived listener meets: garbage, truncation, wrong magic and version, an impossible length, a header claiming 900KB then nothing, 600 abandoned connections, frames for a node that does not exist, and a node replaced twenty-four times that must be released each time. |
| `tests/protocol.rs` | 7 | What OUTER can find out and do: a distributed load watched stage by stage, a stage whose load failed refusing to serve, an unload, locating a request while it runs, cancelling one without touching the rest, and collecting counters. |
| `tests/cache.rs` | 7 | Persist, restore, fork and discard by message, including a conversation spread over a chain and a stage that did not restore. |
| `tests/many_nodes.rs` | 2 | More than one node on one agent, created and loaded individually, and two chains sharing them. |
| `tests/against_a_server.rs` | 8 | The llama.cpp adapter against a wire-level stub: real socket, real chunked framing, real SSE. |
| `tests/stays_detached.rs` | 4 | That the llama.cpp adapter compiles against nothing of llama.cpp's — no build script, no `-sys`, no `llama.h`, two dependencies, two endpoint paths. |

## What each level could not catch

Worth recording, because it is the argument for keeping all of them.

**Unit tests missed** the three defects that needed sustained load across
processes: a starved node select, a starved lane, and a silent drop on the send
path.

**The simulation missed** two leaks, because a leaked thing is inert. A
replaced node stayed resident with its adapter and queue; the peer map was
append-only in every address ever seen. Both were found by a soak watching
memory, and are now pinned by a test that asks an adapter whether it was ever
dropped.

**The stub missed** two defects that only a real model has. A `503 Loading
model` read as a stream that ended having produced nothing — every request
completed, every verdict passed, no token generated. And a reasoning model
streams its thinking under a different key, so reading only `content` dropped
every token of a fourteen-second answer. The stub always answered 200 with
`content`; nothing about it was wrong, and nothing about it was enough.

**A test can race the thing it observes.** The cancellation test watched a
node until it was holding a route and then cancelled it — and lost that race
about one run in three, because a node can claim work between the observation
and the cancel. It now runs against a backend that never answers, so nothing
behind the first hop can be claimed and what is queued stays queued. Which of
the others remains is not asserted: routes are spread across workers by hash,
so the order they reach a node is not the order they were sent.

**The verdicts alone miss** an answer that produced nothing: four passes are
consistent with zero tokens. Any harness driving a real backend records the
token count beside them.
