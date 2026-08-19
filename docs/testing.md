# Testing

```bash
cargo test --workspace          # from apps/p4
cargo fmt --all -- --check
```

Three levels, and each catches what the one below cannot.

## Cache contract: mock first

복원 정책과 P4 경계는 실제 llama.cpp 없이 목 어댑터로 먼저 검증한다. 목은
`Adapter::start`가 즉시 반환하고 `EventSink`로 결과를 내는 계약을 지키며,
`Work::Cache`의 단일 `sequence`·`stage_id`·`generation`·`operation_id`를
그대로 기록해야 한다. 라마 어댑터는 아래 계약을 깨지 않는 동안 지연 가능하다.

| 시나리오 | 목 어댑터가 증명할 것 | 필수 결과 |
| --- | --- | --- |
| Persist → Commit | resident KV를 durable 상태로 바꾸고 receipt를 남김 | `Cached`/`Committed` |
| Restore → Commit | 같은 `sequence`를 복원하고 후속 Hop을 Restore 뒤에만 실행 | 복원 후 상태·순서 보존 |
| Prepare → Abort | 준비 상태와 resident/durable 원상복구 | 성공을 가장하지 않음 |
| Discard 재전달 | 이미 삭제된 durable 상태에 중복 Discard | idempotent `Cached` 또는 명시적 `Absent` |
| KV 슬롯 부족 | Restore를 무한 대기시키지 않음 | bounded `Refused`와 재시도 정보 |
| 없는 sequence / generation 불일치 | 다른 세션·배포에 복원하지 않음 | `Failed` |
| 손상·누락 receipt 또는 재시작 | 추측 복원하지 않고 상태를 드러냄 | `Inconsistent` 또는 `Failed` |
| 다단계 중 한 stage 실패 | partial residency를 실행 가능으로 공개하지 않음 | 전체 transaction 보상 또는 reconciliation |
| 동일 sequence의 Restore + Hop 동시 도착 | 큐 순서를 보존해 Hop을 앞세우지 않음 | Restore 완료 전 Hop 금지 |

구현 계약의 기준은 [`Adapter`](../layers/adapters/adapter/src/lib.rs)와
[`Work::Cache`](../layers/adapters/adapter/src/work/cache/mod.rs)이며,
목 구현의 수명주기 검증은 `p4-mock` 테스트에 둔다. 실제 라마 어댑터의
파일 포맷·GPU 슬롯·전송 성능 테스트는 이 표의 대체물이 아니라 후속
acceptance다.

## Pure

The parts that decide things are pure functions with no I/O, because they are
the easiest to get subtly wrong and the most expensive to debug once running.

`worker::judge` — an envelope and our address in, a verdict out. A test pins
that the verdict never depends on lane or chain: the moment it did, forwarding
would have to understand its traffic.

`node::window` — a ready lap goes before fresh prefill *once the deployment is
full*, and fresh prefill goes first while there is still room; a window never
mixes lanes; the ceiling is never exceeded; expired work is reported rather
than dropped. Admission fills the room in one window rather than trickling,
because a node letting one in per window reaches its ceiling no faster than the
sequences it is already carrying complete.

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

The fourth is the one the design rests on: ninety arrivals spread over time
against a ceiling of six, sampling after every one, asserting the node never
hands the adapter more than the ceiling. Three guards keep it from passing for
the wrong reason — the queue must actually have run past the ceiling, windows
must actually have been wider than one, and the agent's lanes must not be where
the backlog sat. Without the first, "never exceeded" is what an idle node also
looks like; without the second, it is what a node dispatching one at a time
looks like.

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

The counts below are the current inventory as of the latest focused run. The
count matters less than the split: the levels catch different
things, and three defects in this layer survived every level but the fleet.

| Where | Tests | What it holds |
| --- | ---: | --- |
| `p4-protocol` | 50 | Round trips, and refusal of every truncation, trailing byte and unknown tag. Address parsing, advertised-address resolution, chain advance and restart, and the route home a reply falls back on: the chain's first link, never the target that just failed and never this agent. |
| `p4-agent-core` (unit) | 98 | The pure decisions — judge, window, outcome — plus the queue, the peer table and its retirement, and the relay: an undeliverable frame arrives at the chain's first link, and a relay that fails is the end of it rather than the start of a loop. |
| `p4-service` (unit) | 32 | Message encoding with explicit tags, the payload seam, the registry, the machine and status snapshots. |
| `p4-mock` | 19 | That the mock honours what it declares: widths, ceilings, per-position cost, the four faults. |
| `p4-adapter` | 9 | The contract's own small logic, including which id a fork leaves state under. |
| `p4-llamacpp-served` (unit) | 51 | HTTP framing, SSE, status refusal, chunk shapes including reasoning content, plan parsing, session behaviour, and which of the three backends insists on being told what it serves. Plus what a placement becomes on llama.cpp's command line, that a backend this build cannot start refuses rather than guessing, and that a load's patience is a different number from a token's. |
| `tests/owns_its_backend.rs` | 4 | That a node owns the process behind it: a share is ready once it has not exited, one that exits while settling is a failure, a backend that gives up is reported rather than waited out through a ten-minute patience, and letting go of one kills it — the last confirmed against the operating system's own process table rather than against our record of it. |
| `p4-link` | 7 | That a declared impairment is deterministic and irregular, and adds rather than replaces. |
| `p4-agent` | 6 | That the registry carries what this build claims and refuses what it does not. |
| `p4-drive` | 19 | Which stages a chain visits: all by default, a held share left out, and refusal of a stage outside the deployment, an empty set, a chain that descends or repeats, and one ending anywhere but the tail. Plus reading a status snapshot: the three figures that matter, peaks that only rise, the deepest of several nodes, and a route named after a field not being read as one. Plus the fleet grammar: one chain reads and names its nodes as it always did, replicas are named apart because they can share an agent, replicas of different shapes are refused, and a plan falls back from replica-and-stage to stage to the default. |
| `tests/simulation.rs` | 10 | Real agents on real sockets: chains of one, two and three stages; a crowd against a ceiling; batching; relay through an agent owning no node; ordering; concurrent chains; a failing backend. |
| `tests/lifecycle.rs` | 10 current (8 historical) | Load reported per stage, a declared ceiling, unload, a failing load, deadlines, cancellation. |
| `tests/queues.rs` | 5 current (4 historical) | The two-tier queue while the mock deliberately holds hops. Asserts **both** halves — node deep *and* lanes shallow — because node depth alone is equally satisfied by an agent that backed up with it. The fifth holds the ceiling under spread arrivals, guarded three ways against passing for the wrong reason. |
| `tests/network.rs` | 7 | The network as the slow thing: latency, jitter that must not reorder, stalls, a narrow link, one bad hop, and a slow link with a slow backend. One test exists only to guard the others — a relay that fell out of the path would leave them passing *faster*. |
| `tests/topology.rs` | 6 | The model rather than the wiring: a row of relaying agents, a star, a chain revisiting a machine, a partition answered by its deadline, a heal with nothing restarted. The partitions assert they really partitioned. |
| `tests/resilience.rs` | 7 | What a long-lived listener meets: garbage, truncation, wrong magic and version, an impossible length, a header claiming 900KB then nothing, 600 abandoned connections, frames for a node that does not exist, and a node replaced twenty-four times that must be released each time. |
| `tests/protocol.rs` | 3 | What a load makes visible: a distributed load watched stage by stage, a stage whose load failed refusing to serve, and an unload. |
| `tests/protocol_in_flight.rs` | 5 | What can be watched and steered while work runs: locating a request, cancelling one without touching the rest, receiving its replayable terminal failure, being told there was nothing left to stop, and collecting counters from another machine. |
| `tests/cache.rs` | 4 | The four verbs on one node: persist and restore continuing where it left off, a fork that copies rather than renames, a discard that cannot be repeated, and refusal of what was never persisted. |
| `tests/cache_in_a_deployment.rs` | 3 | What a cache verb does around itself: the deployment stays bound and serving, and a conversation spread over a chain is persisted and restored on every stage. |
| `tests/many_nodes.rs` | 2 | More than one node on one agent, created and loaded individually, and two chains sharing them. |
| `tests/against_a_server.rs` | 8 | The adapter against a wire-level stub: real socket, real chunked framing, real SSE. |
| `tests/three_backends.rs` | 4 | The one place the three names differ, against a server that behaves like vLLM: a load that finds out what is served, a named model left alone, a server listing nothing refused at the load rather than at the first request, and the lenient two not charged for the strict one's rule. |
| `tests/lets_go_of_a_stream.rs` | 1 | That dropping a sequence ends its connection instead of leaving it to a fifteen-minute read timeout. |
| `tests/stays_neutral.rs` | 3 | That the core has not learned a backend: no source names one, the dependencies are the three chosen, and the window composer's inputs are still lanes and a ceiling. |
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
consistent with zero tokens. The driver now keeps one stream's text and prints
it beside them, because the token count alone is also consistent with every
token being an empty string.

**A harness can invent a defect.** Two did, and both only appeared once runs
got long. The driver waited a fixed number of polls, which was ample for
sixty-four tokens and ran out at four thousand six hundred of five thousand —
it reported a stall at the exact token its own patience expired on, while the
backend went on to finish normally. Waiting is now bounded by silence rather
than duration, and when the driver does stop it says so above the verdicts.
Then route names were reused across runs (`q0`, `q1`, …); a driver that had
walked away left its inference still generating into the same address, and the
next run's stream took those tokens too — an ordering failure reported against
a layer that had ordered them correctly. Route names now carry the run.

**Nothing below the fleet could have found** that the front's worker probe was
wrong. Opening a TCP connection to a declared worker passes against a stub, a
mock, and an idle port, and fails against the real deployment: llama.cpp's RPC
worker refuses further connections while it is serving, and on Windows that is
byte-identical to nothing listening. The test that would have caught it is the
one that ran second — a probe passing on an empty port and failing on a working
one is only visible once something is actually working.
