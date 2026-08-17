# What is implemented

A reference to the code as it stands: every crate, what it holds, what state it
keeps, and why it is separate from its neighbours. [`api.md`](api.md) is the
protocol, [`internals.md`](internals.md) the decisions and the defects behind
them, [`constraints.md`](constraints.md) the invariants. This is the map.

16,447 lines of Rust across nine crates, of which 8,977 are tests.

| Crate | Path | Source | Tests | Depends on |
| --- | --- | ---: | ---: | --- |
| `p4-protocol` | `layers/protocol` | 801 | 561 | nothing |
| `p4-adapter` | `layers/adapters/adapter` | 383 | 138 | nothing |
| `p4-agent-core` | `layers/agent` | 2,203 | 3,715 | adapter, protocol |
| `p4-service` | `layers/service` | 986 | 2,526 | adapter, agent, protocol |
| `p4-mock` | `layers/adapters/mock` | 483 | 349 | adapter |
| `p4-llamacpp-served` | `layers/adapters/llamacpp/served` | 1,151 | 1,413 | adapter, serde_json |
| `p4-agent` | `entrypoints/agent` | 181 | 74 | all of the above |
| `p4-drive` | `tools/drive` | 1,041 | 124 | agent, protocol, service |
| `p4-link` | `tools/link` | 333 | 77 | tokio |

Two crates depend on nothing, and that is load-bearing. The protocol cannot
learn a backend; the adapter contract cannot reach for a P4 type, because there
is no dependency to reach through. Both are compile errors rather than
conventions.

---

## `layers/protocol` — the wire

Everything a hop must understand, and nothing else. No backend, no message
vocabulary.

| Module | Holds |
| --- | --- |
| `frame` | The 16-byte header, encode and decode, and the length caps. `frame_len` reads a header alone, so a reader knows a whole frame without decoding it. |
| `envelope` | The seven fields every hop reads. |
| `envelope/address` | `tcp://host:port`, and the identity of an agent. Parses IPv6 by splitting from the right so a literal keeps its own colons. |
| `envelope/address/advertised` | What a process calls itself, resolved from an optional hint and the bound socket. Takes a host or a `HOST:PORT`; refuses what is neither. `is_local_only` says when an identity only its own machine can reach. |
| `envelope/chain` | The ordered links and a position. `advance` stops at the end; `restart` laps. Those are separate events because finishing a pass and continuing generation are different things. |
| `envelope/recipient` | `Agent` or `Node(id)`. Two variants; a third would mean an agent acquired state it should not have. |
| `envelope/wire` | The envelope's own encoding, separate from the frame's. |
| `lane` | `Control`, `Prefill`, `Decode`, `Response`. Carried rather than derived, which is what lets a socket reader place a frame without opening it. |
| `error` | One error type. |

## `layers/adapters/adapter` — the contract

What a node asks of a backend. Zero dependencies, including the protocol.

```rust
pub trait Adapter: Send + Sync {
    fn distribution(&self) -> Distribution;
    fn start(&self, work: Work, events: &dyn EventSink);
}
```

`start` is a procedure. It may block — a real backend waits on a device, and
the node runs it on a blocking thread for exactly that reason. Results arrive
as events, never as a return value.

| Module | Holds |
| --- | --- |
| `work/distribution` | `Staged` (P4 owns the boundary between pieces; can be one link of a chain) or `Internal` (the backend spreads the model itself; its chain is one link). |
| `work/load` | `Load { deployment, plan, artifact }` and `Unload`. The plan is opaque. |
| `work/hop` | `Hop { deployment, phase, sequences }` — a **window**, because batching is the node's decision and the cohort is the shape a runtime works in. `Sequence` carries the id, the position, an optional prompt, what remains, and opaque options. |
| `work/cache` | `Cache { deployment, sequence, action }` with `Persist`, `Restore`, `Fork { into }`, `Discard`. `subject()` names the id the state will live under — the new one after a fork, which is how the fork case avoids being reported against the wrong id. |
| `event/report` | `LoadProgress`, `Loaded { generation, allocations }`, `Unloaded`, `HopComplete { outcomes }`, `Cached { sequence, bytes, detail }`, `Failed`. |
| `event/sink` | `raise(Event)`, returning nothing. An adapter has nothing to wait on. |

Hidden state never crosses this boundary. That transfer stays inside a backend
under either distribution, which is why a chain over a self-contained backend
is one link rather than a special case.

## `layers/agent` — the core

The two-tier queue, the worker, the node, and the transport. Routes opaque
bodies; reads them only through the `Payload` seam.

### `agent/`

`Agent` holds the address, four traffic counters, the main queue, the peer
table, the node map, the continuation registry, the duties and the payload
seam. Counters are counted rather than inferred, because a frame that vanishes
leaves no other trace.

`create_node`, `delete_node`, `cancel`, `node_status`, `node_counts`,
`node_depth_total`, `enqueue`, `peers`, `continuations`.

`NodeStatus { node, depth, running, waiting }` — `waiting` is the part a count
cannot give: which requests are on this node now, in the order it will take
them. `running` is how many sequences are inside the adapter at this moment,
which is the ceiling being kept, observable. It was a bool — "is something
running" — and that could not answer the question it existed for: equally true
at one and at a hundred, so an operator watching a backlog could not tell from
it whether the excess was being held here or handed to the backend.

### `queue/`

`main` — four lanes and the taking of them. `offer` (try, for socket readers,
so a full lane comes back as a refusal rather than blocking the reader) and
`send` (await, for nodes, so backpressure reaches the thing producing work).
Preference is bounded: every sixteenth take is fair, because strict priority is
a veto rather than a preference.

`lane` — `Lanes` (per-lane depth) and `Budget` (connections, in-flight and
depth, sized apart because one constant doing all three narrowed the others
silently).

### `worker/judge`

A pure function: an envelope and our address in, a verdict out.

```rust
if !envelope.is_mine(own) { Forward(target) }
else { match recipient { Agent => Agent, Node(id) => Node(id) } }
```

The verdict never depends on lane or chain. The moment it did, forwarding would
have to understand its traffic.

### `node/`

| Module | Holds |
| --- | --- |
| `runner` | The event loop, and what it hands a backend. Advances on two events and no others: work arriving, and a hop ending. No timer — a node's pace is the backend's pace. Holds the ceiling, the in-flight map keyed by sequence id, the lifecycle slot and the outbox. |
| `runner/events` | The other direction: the channel an adapter raises on, and what an answer means — a token routed on, a hop ended, a load bound or refused. Apart from the loop because scheduling moves when batching does and this moves when the adapter's event vocabulary does. |
| `runner/handle` | What the agent holds and what an operator can read: depth, how many are inside the adapter, which routes are waiting, and the counters. Moves when a question needs answering. |
| `runner/bound` | The load transaction, and nothing else. |
| `queue` | The node's own queue: push, claim, remove, the waiting list, and how many are inside the adapter. Claiming a window sets that count and finishing clears it, in the same call as the queue removal so the two cannot drift. |
| `window` | `compose` — a ready lap before fresh prefill, never mixing lanes, never past the ceiling, expired work reported rather than dropped. |
| `outcome` | Pure routing: `Hop`, `Finish`, `Lap`, `Unheard`, and the token count that bounds a ring a backend never stops. |
| `payload` | The seam. `sequence`, `lifecycle`, `ceiling`, and the reply shapes. The core learns no message catalog. |

`Bound` is the transaction: `Never` (serves anything — a backend needing no
load is legitimate), `At(generation)` (serves that generation only), `Refused`
(a load failed here; serves nothing until one succeeds).

The run loop returns when its work channel closes. Written as a disabled
`Some(...)` branch it parked forever instead, because the node owns its own
event sender — every replaced or deleted node stayed resident with its adapter,
queue and in-flight map.

### `transport/`

`inbox` — accepts, bounded by a semaphore, and does nothing but enqueue. A
malformed frame closes the connection; a refused frame is counted and reported,
because there is nowhere in band to answer on a one-way connection.

`outbound` — one connection per peer, kept open. Sending waits rather than
refuses: a frame here is usually already a reply, and a reply has no reply
address, so a refusal would vanish. A peer is checked for death before use,
because a dead socket accepts a write into its buffer. A silent peer retires
after an idle window and removes its own entry under the map's lock — without
that the map was append-only in every address ever seen.

A frame that still cannot be written goes home through the chain rather than to
stderr. Its first link is the stage the caller sent the work to, so it is an
agent the caller was connected to; the envelope is untouched, so the agent it
lands on judges it as a frame for somewhere else and forwards it, which is what
it already does for anything not addressed to it. Nothing is taught to the
receiving side. This is the reporting topology stated plainly: a node reports to
its agent, and an agent the caller is not connected to hands the answer to one
that is.

Once only. A flag travels beside the frame inside the transport — never on the
wire — because a relay that failed would name the same first link again and go
round for as long as the process lives. Two guards on top of that: the route
home is never the target that just failed, and never this agent itself.

`pump` returns a boxed future rather than being an `async fn`, and that is not
style. It and `connection` each reach the other, so their opaque `impl Future`
types have auto traits depending on one another; the compiler cannot resolve
that and reports only that the pump is not `Send`. Naming the type is what
breaks the loop.

### `continuation`

`register`, `resolve`, `forget`, `outstanding`. A response path pinned to a call
stack dies when that frame returns; a requester registers a continuation
instead. Nothing in the shipped core registers — it is the seam a deployment
uses — so `outstanding` is a leak indicator rather than a working count.

## `layers/service` — the vocabulary

One implementation of everything the core leaves open.

| Module | Holds |
| --- | --- |
| `message` | `ToAgent`, `ToNode`, `Reply`. |
| `message/wire` | Tag-and-length encoding, explicit tag constants, and refusal of every truncation, trailing byte and unknown tag. |
| `duties` | What an agent does with a message addressed to itself. Every branch is a procedure whose only output is a frame on the queue. |
| `payload` | The `Payload` implementation: reads a body into `Work`, and formats replies. |
| `registry` | Name to factory. The single attach point for a backend. |
| `machine` | The `Inspect` snapshot. |
| `status` | The `Status` snapshot: traffic, lanes, peers, replies outstanding, and per node its depth, whether a hop is inside the backend, and the routes it holds. Node ids and routes are escaped, because they come from a caller and a newline would make one node look like two. |

## `layers/adapters/mock` — a backend that is not one

Implements the whole interface against arithmetic. It is the second
implementation that keeps the contract honest: what it can implement is the
interface, and that it finishes without a backend concept is the evidence the
boundary is clean.

`Profile` declares the shape measurement found — cost by chain position,
prefill dearer than a lap, a load spread over stages, bytes reserved per stage
— plus four faults (`Load`, `Hop`, `Silence`, none) that are otherwise hard to
reach without breaking something real. Nothing is computed and nothing is
random: the same profile produces the same timings everywhere, so a difference
between two fleet runs is a difference in P4.

It tracks per sequence a `turn` and a `lifetime`. The turn decides when a
request stops; the lifetime is how much state there is and therefore what a
persisted copy costs. Both were modelling errors found by building the cache
protocol: it used to drop a sequence's state the moment the turn finished, and
to record residency only on the terminal stage — but every stage holds the KV
for its layer range, and only the last has logits.

## `layers/adapters/llamacpp/served` — stock `llama-server`, and the two that copied its surface

llama.cpp's adapter for the shape llama.cpp already supports: one process
holding the whole model behind one completions endpoint. `../staged/` is the
other shape — a model split across machines with P4 owning the boundary — and
shares nothing with this but the interface. Two arrangements of one backend,
which is why they are folders under it.

vLLM and SGLang are registered against this same implementation because all
three copied one HTTP from OpenAI: a model list and a streamed chat completion.
That is the whole coupling, so it is one adapter under three names rather than
three copies of one file diverging, and the names are real registrations rather
than a claim in a document.

They are not equal tenants, and the folder says so. It sat at
`adapters/openai/` for a while on the strength of the shared surface, which
read as though the agent offered an OpenAI-compatible API — it does not, and a
service API is OUTER's concern rather than this layer's. Two things differ per
backend and both point the same way: vLLM refuses a model name it does not
serve, and only llama.cpp can be *started* here, because `launch` composes
`llama-server` and `ggml-rpc-server` flags and refuses to guess at anything
else's.

| Module | Holds |
| --- | --- |
| `endpoint` | The least HTTP that reaches a backend, on the standard library: request framing, chunked and length-delimited bodies, SSE lines, and a status check. A non-2xx is a failure carrying the body's message — read as a stream instead, a `503 Loading model` looked like a request that completed having produced nothing. |
| `chat` | The OpenAI-compatible surface: building a request, and reading a chunk. Reads `delta.content`, then `reasoning_content` when content is null, then the non-streamed shapes. A reasoning model streams its thinking under the second key, and reading only the first dropped every token of an answer. |
| `plan` | What a load's plan means here: endpoint, model name, patience, and — for a model split across devices — the role this node holds, its device, what it claims of it, and the shares held elsewhere. Records whether the plan actually named a model, because a default cannot be told from a choice and one backend checks. Opaque everywhere else. |
| `flavour` | Which of the three servers is behind the surface, and the one thing that follows: vLLM matches a request's `model` against what it serves and answers 404 to anything else, so its load asks and the other two are not charged the round trip. A field here that could be a plan key would be the adapter knowing a backend for no reason. |
| `session` | The impedance mismatch, and the reason it has its own file. P4 generates by lapping — a hop reports one token and the request comes round again — while the backend streams a whole completion down one connection. Asking for one token per hop would re-prefill on every lap. So the completion is requested once, read by a thread into a channel, and each hop takes the next token. The session counts what it has delivered, because the backend is the only thing that knows how far a sequence has got. |
| `launch` | What a placement becomes on one server's command line, and the process that results. `arguments` is a pure function of a plan and can be checked exhaustively without starting anything — which is the half that goes quietly wrong, since a batch too small to hold one prompt makes a server admit prefills one at a time and nothing about that looks like a mistake from outside. `process` waits, kills, and starts windowless. |
| `load` | Everything a load has to get right and an unload has to let go of, separated because it changes for different reasons than driving a completion: a backend gaining a flag, a machine gaining a card, or somebody changing their mind about who owns a running server. Every failure arrives at one place, which is where a started backend gets killed. |

Five tests enforce that the crate stays detached: no build script, no `-sys` or
bindgen dependency, no `llama.h` or `ggml.h` in code, exactly two dependencies,
exactly two endpoint paths, and no `unsafe` anywhere — the last catching the
category rather than the spellings, since calling a foreign function requires
it. That property is why the adapter is worth having in this form, so it is
checked rather than intended.

## Node-owned backend lifetime

A plan may carry a `start`, and then the process behind the node exists because
that load exists. Without it a plan can claim eleven gibibytes of a card while
the server on it holds fifteen, and nothing in the protocol can tell — the plan
is the only record of intent and nothing checks it against a process.

Three things this had to get right, each found by getting it wrong first:

- **Readiness differs by role.** A front answers `/v1/models` once it holds the
  weights. A share speaks llama.cpp's RPC protocol and serves no HTTP at all, so
  asking it the same question waits out the whole patience and then reports a
  healthy worker as a failure. `Ready::WhenItHasNotExited` is all a share can
  offer; the front reaching across it is what proves it is held.
- **A load's patience is not a token's patience.** One is minutes because
  seventy gibibytes take minutes to read; the other is seconds. `start` carries
  its own `patience_ms` and requires it, because a guessed load timeout is the
  adapter deciding how long an operator will wait for weights it knows nothing
  about.
- **A process that exited is reported, not waited out.** Checked every poll, so
  a missing weights file fails in a second rather than after the ten minutes a
  large model is allowed.

Reload kills the previous backend before starting the next, because the card is
what is scarce and llama.cpp reports a card still held as a memory error. The
report carries `backend=attached|owned|released`, the pid, and started/stopped
counts — the pid because it is the only thing joining a node that claims a card
to a process that holds one.

Cache verbs are refused by name: llama.cpp can save sequence state, but not
through this surface, and a caller must be able to tell "not here" from "done".

### A model larger than one card

llama.cpp splits a model across devices itself, over its own RPC backend and
with no patch to it. P4 gives each share a node so the placement is stated: a
`worker` plan names a device and what it claims of it; a `front` plan names the
same for itself and lists the shares held elsewhere. Both must bind before the
deployment is executable, and only the front serves — a hop sent to a worker is
refused by name, because a caller that could not tell would wait for tokens that
were never coming.

A worker is not probed. The one question TCP could answer it cannot answer
usefully: a worker already serving a front refuses further connections, and that
refusal is indistinguishable from an empty port. The front is the proof —
llama.cpp will not start against an RPC device it cannot reach, nor answer
across one that died.

## `entrypoints/agent` — the process

176 lines. Binds, resolves what to call itself, builds the registry, starts the
inbox and the run loop, and prints what it can serve. `P4_AGENT_STATS=1` adds a
line a second: lane depths beside node depths, traffic, peers held, and replies
outstanding.

`src/adapters/mod.rs` is the whole of what adding a backend costs — a name, a
factory, and an `Adapter` implementation.

## `tools`

`drive` — OUTER. Creates nodes, loads them, runs inferences, prints four
verdicts: every request answered, none failed, every stream in order, one
terminal per route. Built on the same core as an agent, which is the point.
`session/replies.rs` holds what came back — it changes with the reply
vocabulary, and it is the only part another thread touches.

Two of its rules exist because breaking them made it report defects that were
not there. Waiting is bounded by silence rather than by a count of polls, since
an answer takes as long as it takes. And route names carry the run that made
them, because an agent outlives a driver: an inference the driver walked away
from kept generating into the same address, and the next run's stream collected
its leftovers.

`link` — a relay that carries frames badly on purpose: latency, jitter,
bandwidth, periodic stalls, and a cut. Deterministic, including jitter. Used as
a library by the tests and as a binary between machines.

---

## What is not implemented

- **The staged llama.cpp adapter.** The patch series and the preparation script
  are there; the `Adapter` implementation is not. Splitting one model across
  machines with P4 owning the boundary therefore has no code path yet.
- **vLLM and SGLang.** They serve the same surface `served/` speaks, so each is
  a registration and an implementation once there is a process to point at.
  Writing one without a backend to prove it against would be code without
  evidence.
- **TLS, authentication, authorisation.** A self-describing address is trusted
  because the network is. Use inside a trusted network only.
- **Durable state.** An agent that restarts has no nodes; OUTER holds the
  record of what it should have.
- **A field for the kind of a token.** Reasoning and answer text are merged,
  because nothing above the adapter can express the difference. Separating them
  is a protocol change worth making deliberately.
- **Windows ARM64.** The one platform in the matrix never exercised; that
  machine refuses key authentication.
