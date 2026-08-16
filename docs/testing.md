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
mocks behind the adapter boundary. Eighteen scenarios: chains of one, two and
three stages; forty arrivals against a ceiling of four; batching actually
happening; a slow backend showing as node depth rather than agent depth; relay
through an agent owning no node; sustained arrivals; per-route ordering;
concurrent chains sharing agents; a failing backend; a failing load; deadlines;
cancellation.

`layers/service/tests/two_process.rs` does the same through the message
vocabulary: nodes created, a model loaded, an inference chained, all by frame.

## Fleet

The level that found three defects the other two could not — a starved node
select, a starved lane, and a silent drop on the send path. Unit tests cannot
reach them because each needs sustained load across processes.

```bash
p4-agent 0.0.0.0:19311            # one per machine
p4-drive 0.0.0.0:19310 HOST:19311,HOST:19312,HOST:19313 1000 64 mock-instant
```

Four claims, printed as a verdict: every request answered, none failed, every
stream in order, one terminal per route. Run each chain shape several times
against long-lived agents — a defect that only appears on the second run
against the same process is exactly the kind this level exists for.

Run with `P4_AGENT_STATS=1` when something is wrong. Lane depth beside node
depth says which side of the adapter boundary is slow; the per-node counts say
at which step a frame stopped existing.
