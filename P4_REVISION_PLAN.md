# P4 revision plan

> Document status (2026-09-06): **historical / superseded plan**. It preserves the plans and observations of the time. Do not use it for current status, execution order or promotion criteria.
> Current goals, status and order follow the [execution roadmap](docs/distributed-batching-roadmap.md); document authority and reading paths follow the [documentation map](docs/document-map.md).

> **Status: completed history. Not a kickoff guide.**
>
> This document planned the reorganization from v5 to v6, and that reorganization is finished. The reference implementation
> it lists, `layers/runtime` and `tools/controller`, no longer exists — removing them was
> the outcome of this plan. The `Q` decisions here record what was chosen **at the time** and why;
> they are not instructions to follow now.
>
> Current status is in [STATUS.md](../../STATUS.md); future work and the norms that bind it are in
> [STAGED.md](../../STAGED.md). If those two disagree with this document, those two win.
>
> **In particular, Q-22 was reversed.** This document decided that "hidden state stays native; tensors do not go into
> frames". That premise assumed a native data plane that would move the tensors.
> The staged structure has none — both stage servers are ours, they sit on different
> machines, and P4 is the only thing connecting them. So the cut-set travels in the frame body.
> The rationale is in "How the cut-set crosses between nodes" in STAGED.md.
>
> Reference implementation: P4B1 v5 (`layers/protocol`, `layers/runtime`, `layers/adapters`, `tools/controller`) — all reorganized
> Target: **P4B1 v6** (§90) — reached
> First written: 2026-08-13 · Completed: 2026-08-14

## Summary

| | |
|---|---|
| Premises | 12 (§1.1) |
| Topics | 14, A~N |
| Confirmed defects | `D-1`~`D-84` (including 4 withdrawn or resolved) |
| Remediation designs | `P-1`~`P-64` (including 2 withdrawn) |
| Open | Of `Q-1`~`Q-72`, **53 open**; 19 decided, withdrawn or resolved |
| Parallel session | Measured conclusions are reflected in §92. Only `Q-49` is in progress |
| Build phases | 0~6, sequential (§91) |

> **Direction fixed on 2026-08-15:** the first implementation target is **the agent core**, not the details of individual messages — §1.4. The controller is discarded, and the agent's only internal entity is the node.

**The three heaviest items** — everything else builds on them.
1. **CPS violations are baked into the foundational contract** (`D-26`). The whole control plane is return-value based, and the function is even named `compatibility`. The CPS spec in §1.4 is its replacement
2. **Reorganizing the address and identifier scheme** (`P-2`·`P-34`). The self-describing address becomes the agent identity, and relaying becomes a default queue behavior
3. **The chain lives outside P4** (`D-35`). The core of the inference path is not expressed in the protocol

**Adapter boundary audit result** — the dependency direction is correct, and `layers/protocol` contains 0 backend strings. The leaks are in **contracts and docs**: KV/FFN in `DRAFT_REPORT` (`D-62`), `docs/model-load.md` codifying llama.cpp knobs as the formal schema (`D-63`), and no adapter interface as an explicit artifact (`D-64`).

**The first step is phase 0** — `P-40`. Removing the Pipeline adapter's 5-item whitelist alone lifts the state in which structured output is impossible (`D-56`). It needs no wire change and depends on no other phase.

## 0. Conventions of this document

### Notation

| Notation | Meaning |
|---|---|
| `D-n` | Confirmed defect. Comes with the source file and line |
| `Q-n` | Open decision. Sub-designs are not finalized before it is answered |
| `P-n` | Remediation proposal. Promoted to spec once its `Q` is resolved |
| (draft) | Source cross-check done, but not yet agreed |
| (decided) | Confirmed in a meeting. The rationale is recorded with it |
| (withdrawn) | Reversed in later discussion. Kept rather than deleted, to preserve the reason |

Only defects confirmed in the source are listed. Guesses are filed as `Q`.

### Revision method

Do not describe everything at once. Hold meetings per topic and fill in only what has been agreed.
**When a contradiction with existing text is found, fix that section instead of appending a new one.** `D`/`P`/`Q` numbers are globally sequential and never reused — withdrawn items keep their number and stay marked `(withdrawn)`. Section numbers can be rearranged as topics grow, so references use `D`/`P`/`Q` numbers. The revision history is recorded in the last section.

**Section numbering convention:** topic sections grow sequentially from §2, and synthesis sections not tied to a topic (open items, order, throughput, impact, remaining items, history) are **fixed at §89 and above**. Adding a topic does not shift the synthesis section numbers.

Because numbers are globally sequential and never rearranged, **item numbers within a topic do not appear in order.** This is intentional: some items were added later or moved over from other topics. Examples: `P-39` in topic A, `P-33` in topic G.

### Reading order for this document

| Purpose | Section |
|---|---|
| What are we building | §1.2 target architecture, §1.3 identifier ownership |
| What is wrong | The "Defects" sections of topics A~I |
| What will we do | The "Remediation design" sections of topics A~I |
| What must be decided | §89 open decisions |
| Rewrite or revise | §91.0 decision record |
| In what order | §91 build order — **units of work** |
| How it relates to throughput | §92 — **P4 does not own throughput** |

## 1. Background premises and target architecture

### 1.0 Terms

**OUTER** — the external requesting party. It holds orchestration authority, instructs agents and receives results. The party this document has so far called "external" is OUTER.

**Agent** — a socket program. It has one main message queue and tokio workers. As internal entities it owns **only nodes**.

**Node** — an id shell inside the agent. At load time it is bound to a concrete adapter that is instantiated for it.

**There is no controller.** It was discarded on 2026-08-15 — the rationale is in §1.2.

### 1.1 Premises

This is the direction confirmed in meetings.

1. State assets live outside the agent. External records are authoritative for which nodes exist and which models are where.
   **OUTER owns infrastructure facts.** It already knows the access addresses of all agents. Trying to discover infrastructure facts through the protocol only creates a "who comes first" ordering contradiction, so **distinguish what must be learned through the protocol from what OUTER already knows.** Infrastructure facts such as addresses and placement authority are the latter and are not something P4 queries.
2. Aim for a functional form. State passed as arguments is treated as living outside.
3. OUTER instructs node creation, model load and release. The entry agent **does not own these instructions; it only relays them** — it neither interprets nor verifies them, and it holds no state. (Relaying is not owning)
4. Model loading is reorganized as an OUTER request. OUTER sets the load policy and injects it through the protocol, and its expressiveness must cover what the concrete runtime actually offers.
5. Load progress, completion and failure return to the OUTER that issued the instruction. The path goes through the entry agent, but that agent only passes it along.
6. P4 does not interpret load options. They pass through as strings, and the concrete adapter interprets them.
7. Load and release depend on the node's prior state. A command that violates a precondition becomes a failure message. When a release succeeds, the node's concrete instance is completely torn down.
8. CPS. A response is not a return value; it **becomes a message that calls the requester.** Anything once emitted therefore cannot be taken back.
9. **No protocol handling has a return value.** All a worker can do is its own job and putting messages on the queue. Responses and forwards alike go through the queue. Every handling path that does not follow this must be revised.
10. **A node knows as little as possible about its own load structure.** Keep the abstraction layer. Judgements about placement structure, such as layer numbers, are excluded from the node's checks; OUTER, as the orchestrating party, is responsible for them.
12. **P4 does not interpret inference options either.** This mirrors premise 6 (load options). Every spec that must reach the concrete runtime as an actual argument must be supported, and adapters do not pick and choose arbitrarily. **Silently dropping unsupported options is forbidden.**
11. **Network reachability is a constraint.** A typical deployment has OUTER outside the firewall with only one tunnel open to the internal network. There are exactly two constraints.
    - OUTER can reach **exactly one agent**. That agent, which handles the VPC's ingress/egress, is called the **entry agent**
    - The entry agent **can reach every other agent**

    The entry agent therefore has no reason to host any particular node. When every message — node create/delete, model load/release, inference and so on — is sent to the entry agent, it either consumes the message itself or forwards it to another agent to carry out. **This constraint is the only reason the entry agent exists.**

### 1.2 Target architecture flow

This is the destination that the individual defect fixes aim for.

#### Controller discarded (decided 2026-08-15)

**The controller does not exist.** For a long time this document drew the controller as an independent participant between OUTER and the entry agent, and the role table said it "knows the node list and order for this request". **Both were wrong.**

- **It is not an inference front end that knows the node list.** Per premise 1, the node list is already OUTER's external state. The controller only **has it injected through inference and load commands**, and having something injected is not knowing it
- **Its only reason to exist was the firewall.** "Controller" meant the **entry point of the agent** that handles VPC ingress/egress

But the entry point is the agent itself. Everything the controller did **requires only the agent's knowledge.** No unique state or judgement remains for a separate object to own. So it is removed from the participants.

**Result: the agent owns only nodes as internal entities.** This reduces the worker's judgement to the dichotomy `internal node | external address` (§1.4).

#### Topology — reachability decides the structure (premise 11)

```text
       firewall
          │                          ┌──▶ Agent A ──▶ Node
 OUTER ───┼──▶ Entry Agent ──────────┼──▶ Agent B ──▶ Node
          │    (VPC ingress/egress)  └──▶ Agent C ──▶ Node
               │
               └── may also host nodes itself
```

- OUTER can reach **exactly one** agent
- That entry agent **reaches every other agent**
- The entry agent **has no reason to host any particular node.** It is a pure entry point unrelated to node placement; it may simply have nodes of its own if needed

So every message, whatever its kind, passes through the same gateway. **Distinguishing relaying from owning is the core of this design** — the entry agent carries messages but neither interprets nor owns them.

#### There are only two recipients — the agent itself, or a node

The envelope's address decides which agent consumes a message. Once that is decided, **the recipient inside that agent is one of two.**

| Recipient | Message | Why here |
|---|---|---|
| **The agent itself** | Node create/delete | The agent owns the node registry |
| | Inference request intake | Requests come to the agent, not to a node |
| | Hardware spec survey | A fact about the machine, not about a node |
| **Node** | Model load/release | The node is what gets instantiated |
| | Prefill/decode | The node is what executes |

**Inference splits into two steps.** The agent receives the request, and the agent **forwards a prefill message to the node**. The fact that intake and execution have different owners shapes this path.

#### Control path — through the entry agent, without ownership

```text
OUTER ──hardware/capability───▶ Entry ──▶ (consumed itself, or target Agent)
OUTER ──node create/delete────▶ Entry ──▶ Agent itself
OUTER ──model load/unload─────▶ Entry ──▶ Agent ──▶ Node ──▶ Adapter
OUTER ◀──progress/done/failed── Entry ◀── Agent
```

**OUTER is still the party that directs** orchestration, load and release; the entry agent only passes things through. It neither verifies nor holds state. (Premises 3·4·5·11)

If the entry agent is itself the target agent, it consumes the message on the spot without forwarding.

#### Inference path — the agent takes the request, the node executes it

```text
OUTER ──inference(node chain)──▶ Entry Agent
                                    │ after intake, one PREFILL carrying the chain goes to the node
                                    ▼
                              Node[0] ──▶ Node[1] ──▶ … ──▶ Node[n]
                                     (each node reads the chain in the message and forwards to the next itself)
                                                                  │
                                    ┌────────generated token──────┘
                                    ▼
OUTER ◀────────token stream──── Entry Agent
```

**The agent does not intervene at every hop.** The chain is carried inside the prefill message sent to node 1, and each node learns the next node from that message on its own. This is source routing.

Decode cycles under node control. After the first prefill, the generation loop runs through **node-to-node communication**.

```text
        ┌─────────────── next token request ──────────────┐
        ▼                                                 │
   Node[0] ──▶ Node[1] ──▶ … ──▶ Node[n] ─────────────────┘
                                    │ token / end of generation
                                    ▼
                              Entry Agent ──▶ OUTER
                        (on the spot if it is itself the entry agent)
```

**The last node cannot report directly to OUTER** (premise 11). It sends decoded tokens and end of generation to the **entry agent**, which passes them on to OUTER. If the reporting node already belongs to the entry agent, there is no relay hop.

Node placement is free. Neither the first node nor the last node has any reason to be on the entry agent.

- OUTER **sends the node list along with the request.** The agent only has it injected and does not keep it, and it knows neither the concrete state nor the structure of each node
- The agent's only active involvement per request is **intake and sending one prefill**. After that it only receives reports and passes things through
- **Prefill is also a state-check step.** Each node reports entry and completion, so this is when the node's actual state first becomes known (topic G)
- **The last node gives generated tokens to the entry agent, not to OUTER.** This leaves room for later filtering or additional work; currently it only passes them through to OUTER
- **There is no stream/non-stream mode.** Inference in this system is always streaming

#### Role summary

| Party | Knows | Does not know | Topology |
|---|---|---|---|
| OUTER | Hardware capability, node orchestration, load plan, chain composition, all business identifiers | Runtime state | Outside the firewall |
| Entry Agent | Its own nodes (if any) + relaying | Orchestration intent, the content it carries | The **only** agent OUTER reaches. Unrelated to node placement |
| Agent | Node ids and capacity on its own machine | Orchestration intent, the chain | Internal network. **Every agent can relay** |
| Node | Its own binding and adapter | Its own load structure (premise 10), the chain as a whole | The agent's **only** internal entity |
| Adapter | The concrete substance — loaded layers, runtime | P4's upper-level semantics | Internal network. An external transport target with an address |

### 1.3 Identifier ownership

**Principle: OUTER issues identifiers.** The only exceptions are the instance generation and transport/CPS-internal IDs. This follows directly from premise 1 — if the authority over state is external, the names that point to that state must also be decided externally.

| Identifier | Issued by | Lifetime | Notes |
|---|---|---|---|
| **Access address** | OUTER (infrastructure fact) | Until the placement changes | **The agent's identity** (P-2) |
| `node_id` | OUTER | create ~ delete | Already issued externally |
| `deployment_id` | OUTER | Logical deployment unit | |
| `binding_id` | OUTER | load ~ unload | |
| `plan_revision` | OUTER | Plan revision | Version of the external intent (P-8) |
| `request_id` | **OUTER** | One inference | Assigned separately for each inference |
| `session_id` | **OUTER** | Conversation/execution session | Not issued by the agent (D-54) |
| `operation_id` | OUTER | One lifecycle operation | |
| `runtime_generation` | **Adapter** | Instance generation | The only business ID not issued by OUTER (P-8) |
| `route_id` | Sender | One exchange | Transport-layer correlation |
| `task_id` / `causation_id` | Runtime internal | One Task | CPS internal |
| ~~`agent_id`~~ | **Abolished** | — | Replaced by the access address (P-2) |
| `ingress_id` | OUTER | submission ~ promotion | Whether it duplicates `request_id` is Q-43 |

#### 4-tuple rule

An executable concrete instance is identified exactly by the following.

```text
(agent access address, node_id, binding_id, runtime_generation)
```

The first three are names issued by OUTER; only the last is an instance generation issued by the adapter. If any one of the four is missing, execution may land on a stale instance. Chain entries (P-25) and execution requests share this tuple.

### 1.4 Agent core — first implementation target (confirmed 2026-08-15)

**This comes first.** The details of individual messages come after. Most defects in topics A~N are symptoms of this core being absent, and once the core stands, `D-26`·`D-27`·`D-30`·`D-66`~`D-69` disappear together.

1. Each agent is a **multi-platform socket program**
2. It is implemented with the **tokio worker-thread pattern + CPS pattern**
3. Each agent has one **main message queue**, and workers **drain its messages promptly**
4. There are strictly **only three kinds** of messages
5. Those for an object inside the agent, those for another agent, and those for OUTER
6. The latter two are **fundamentally the same external-send message**. The worker sends it over the socket **as is, to that address**
7. **The socket receiver only puts received messages on the queue**
8. Messages to be handled internally are delivered **to the internal object — that is, the node**

#### The worker's judgement has two steps

Because item 6 merges external sends into one type, the first judgement is a dichotomy, and because there is no controller (§1.2), the second judgement is a dichotomy as well.

```text
Message taken from the queue
   ├── target address is not me → send as is to that address (same for another agent or OUTER)
   └── it is mine
         ├── to the agent itself → node create/delete, inference intake, hardware survey
         └── to a node           → model load/release, prefill/decode
```

**Relaying does not interpret the body.** For item 7 to hold, the receiver must not decode the body, and for the first judgement to hold, the target address must be in the **envelope**. This is why `P-34` (self-describing address), `P-48` (envelope/body separation) and `P-51` (the envelope carries the queue classification) are prerequisites of the core.

#### CPS spec — followed strictly

Premises 8 and 9 are pinned down as an executable spec. Allow a single exception and that path becomes blocking again.

1. **No message handling waits for a response.**
2. If there is something to respond with, the structure must be **sending a message toward that recipient**.
3. Therefore **every handling function is a procedure.** It has no return value, and a response proceeds as **the side effect of putting a message on the agent's message queue**.
4. **Ordered messages are not guaranteed by queue order.** Order is guaranteed by the place that handles a message **registering the next message**.
5. **The requesting side does not wait for the response.** It must **prepare a paired handler** that will be called with the response as its argument.

Item 5 is the substance of this spec. The side that sends a request does not "fire and forget"; it **registers a continuation**. When the response arrives on the queue, a worker finds that continuation and calls it with the response as its argument. Without this, item 3 would mean "responses can never be received".

Item 4 means: do not build an ordered queue. The order `INGRESS_ACCEPTED → TOKEN → DONE` holds not because of the queue but because **each step registers the next step**. The queue need not promise any order, which is why it is safe for workers to pull from it competitively.

**Current code this spec invalidates**

| Target | What violates it |
|---|---|
| `P4Handler::handle(...) -> Result<()>` | Signals completion with a return value. Violates item 3 (`D-26`) |
| `P4Transport::dispatch` | Opens a socket and reads until terminal. Violates item 1 (`D-27`) |
| `forward::capture` | Hands the adapter response back **as a return value**. Violates item 2 (`D-28`) |
| The six lifecycle handlers | Branch on return values. Violates item 3 (`D-29`) |
| The 600-second polling loop | A long-running job is trapped inside one blocking call. Violates item 1 (`D-30`) |
| `ResponseSink` | The call stack holds the response destination. There is no paired handler as item 5 requires |

`ResponseSink` matters most. Today the response path is **a sink bound to the call stack**, so the place to respond disappears when the call ends. Under item 5, the response path is not the stack but **a continuation registered on the queue**, and only then does the place to respond survive when a long job is split into several Tasks (`P-17`·`P-19`).

#### Two-level queue — a node has its own queue

**A node is an abstraction for long-running GPU work.** A node's processing time therefore cannot become a worker's time.

1. The agent's message queue **is emptied immediately.** A worker **does not wait** for the node to process a message
2. **A separate message queue exists per node.** A worker **is released by moving the message there**
3. Draining a node queue is **event-driven**. There are two triggers — **a message arriving in the node queue**, and **the concrete object bound to the node finishing a GPU hop**. Each time, the node inspects its own queue and takes action

```text
socket receiver ──▶ [agent main queue] ──▶ worker (prompt drain)
                                            ├── external address → send as is
                                            ├── agent itself → handle immediately
                                            └── node → move to [node queue] and release
                                                                    │
                                                          ┌─────────┴─────────┐
                                                   message arrives      GPU hop ends
                                                          └─────────┬─────────┘
                                                           inspect queue → act
```

**This separation is what makes the design observable.** Agent queue depth and worker occupancy become **independent of GPU time**. When throughput collapses under load, a shallow agent queue with a deep node queue means the cause is below P4, and the reverse means it is in P4. **"Is P4 to blame?" becomes a question the two queue depths can answer.**

Since the GPU hop boundary is the node's only decision point, **deadline checks and cancellation also happen at that boundary**. There is no way to cut in mid-hop, and no need to — simply do not start the next hop (`P-20`).

#### Do not generalize — the known workloads decide the shape

**The goal is not a general-purpose agent that covers vague communication scenarios.** The workloads this system will carry are already known in detail, and both the core and the mock adapter are built **to fit them**. Generalities unrelated to them are not pursued.

There are four known workloads.

| Workload | Shape | What the mock must reproduce |
|---|---|---|
| **Distributed load** | A model is loaded onto several nodes, split into layer ranges | Progress flows separately per node, and overall completion is decided by **the slowest stage** |
| **Prefill chain** | One source-routed pass through `Node[0] → … → Node[n]` | **Cost attaches to the role, not the device** — head ~64s, tail 32–40s (§92) |
| **Decode (regeneration) ring** | Node-driven cycle. One token is one lap of the ring | Per-hop latency and the loop termination condition |
| **Batching** | Cohort/window, credit, merging | Concurrent acceptance within the declared cap, cohort replacement, window splitting |

**The measurements in §92 are the calibration reference for the mock.** Stage overlap 271.5 tok/s (4.11x versus 66.0 single), stage wait 8.0–87.3 s, KV about 188 MiB/layer/48 sessions. The mock only has to reproduce this **shape**; it does not need to compute the numbers — it can be given them and imitate them as declared.

The mock profile therefore has to express **per-role cost, stage composition, ring hops and cohort windows**, not just a few `latency` values. Otherwise the reproduced load has a different shape from reality and the verification becomes meaningless.

**Even so, the agent must be completed and proven before any concrete adapter.** The two constraints do not conflict — the workloads decide **what the agent must support**, and **everything that supplies it is the mock**. llama.cpp does not appear in this phase.

```text
① node adapter interface     ← expresses the four workloads with 0 backend names
② agent core                 ← completed on top of the interface
③ mock adapter               ← second implementation of the interface; reproduces the four workloads
④ fleet sustained-load proof ← this is where the agent is judged "complete"
─────────────────────── no llama.cpp up to here ───────────────────────
⑤ concrete adapter           ← after that
```

**The agent's completion verdict ends at ④.** It must be possible to judge it without any concrete adapter; if it cannot be judged, the interface under-expresses the workloads — if there is load the mock cannot reproduce, go back to the interface.

#### This core is GPU-independent — that is where the means of proof comes from

None of the items above needs a GPU. A "GPU hop" is merely **the abstraction of a hop boundary**, and what provides that boundary is the node's adapter interface. **This entire implementation can therefore be proven with just the adapter interface and a mock adapter that follows it.**

There are two consequences.

1. **`D-64` and `Q-51` are resolved.** The defect was that the adapter interface "does not exist as an explicit artifact", and the mock adapter **forces that interface to exist.** What the mock can implement is the interface, and if the mock can complete its implementation without backend concepts, the boundary is clean. The interface is proven not by a document but **by a second implementation**
2. **Load and simulation verification need no GPU.** Arrival storms, cohort replacement, cancellation, deadline expiry and back-pressure from slow hops can all run on the mock. What is observed then is purely P4's behavior — nothing below is left to blame

**This is the completion criterion for this branch.** If the core withstands load on the mock adapter, there are grounds to state that problems arising later on the real system are not P4's problems.

#### Folder separation principle — structure protects the code

**This is not a matter of mere optimization.** Considering later maintenance and feature additions, the code must be carefully separated into folders.

The reason lies in how the tools fail. **AI causes far too many regressions in file splitting and in-file structure, and the structure collapses as edits repeat.** When several reasons for change are mixed in one file, that file is opened every time, and each time unrelated parts are shaken along with it.

So **protect the code by splitting it into folders so finely that it cannot be touched even when someone wants to.** This is not indiscriminate splitting, though; it follows **purpose, role and rate of change**.

| Axis | Question | Split if different |
|---|---|---|
| **Purpose** | What does this exist for | If the reasons to exist differ, do not put them in the same folder |
| **Role** | Who owns and calls this | If the owners differ, split |
| **Rate of change** | How often does this change | If frequently changing code mixes with rarely changing code, the rare side gets opened often |

**The test is "does a folder have exactly one reason to change?"** If it has two, split it. This is stronger than the repository contract's 400-line rule — 400 lines is a resulting cap, while this principle applies **regardless of line count**. Even a 20-line file is split if it has two reasons to change.

**Protective property:** in a well-split structure, one change opens one folder. A change that must open several folders at once is **a sign that the split is wrong**, not that the change is large.

#### Goal: the best possible agent implementation — constraints set by accumulated measurements

The results that several sessions accumulated through experiments, commits and measurements are already sufficient. Taking this chance to rebuild the core from scratch, **build what those results taught into the design up front.** The following must not be rediscovered.

| Source | Fact | What the core must uphold |
|---|---|---|
| Handoff §4.1 | **Only the agent refuses; everything below it waits.** The adapter queue, scheduler and native side each absorb load | Keep a single refusal point. Adding gates in lower layers makes arrivals and reported limits diverge |
| Handoff §4.1 | The agent could not wait because **`TaskHandler::handle` was a synchronous contract** | The CPS spec removes this constraint — the node queue absorbs load, so refusing and waiting are no longer either/or |
| Handoff §4.2 | **One constant sized three things** — connection acceptance, request acceptance and queue depth. Reducing one reduced all three | **Declare the three axes separately from the start** |
| `capacity/mod.rs` | With a process-global constant as the gate, **the throttle sat three levels above the GPU** | Put batching/window decisions below the node queue. The core carries caps but does not derive them |
| §92 / 2026-08-15 | Concurrency above the declared cap is **not safe** on the current native side | Use the cap only as a cap. Do not silently allow excess concurrency |
| Handoff trap | **When a child stage disappears, the supervisor dies with it** — an availability defect | A node must survive and report the disappearance of its adapter. Do not design in cascading shutdown |
| Handoff caveat | `controller_id` was **an unauthenticated string** | It disappears with the controller. Do not create a replacement |
| Memory | Non-final stages **reserved the whole model footprint** | Load reports must be per stage. A single total hides this defect (`P-45` already has that shape) |

**This table is the operational definition of "the best implementation".** The new core must not go through these items again, and each item gets one scenario that the mock adapter can reproduce.

#### Verification runs on the whole real fleet

Unit tests are not enough. **Since the purpose of this implementation is stabilization, prove it by connecting every available PC on top of the mock adapter and running complex, heavy, continuous scenarios.**

The fact that the mock needs no GPU is decisive here — **every machine participates on equal terms.** Machines without a backend, weak machines and machines running other OSes can all run agents and nodes.

| Axis | What the fleet covers |
|---|---|
| OS/architecture | Windows x64, Windows ARM64, macOS ARM64, Linux ARM64 — **a measurement of core spec item 1 (multi-platform)** |
| Machine count | 7 on the LAN + 2 external |
| Firewall topology | The external machines are **beyond the WAN boundary**. The entry agent and relaying of premise 11 are verified against **a real firewall**, not an artificial setup |

Connection details are managed in a local operations document outside the repository (`F:\dev\REMOTE_SSH_ACCESS.md`). **Do not copy addresses, accounts or secrets into this repository.**

Scenarios must be **sustained load**, not short smoke tests: arrivals while node queues are deep, cohort replacement, paths that cross relay hops several times, flows that mix cancellation and deadlines, and long durations. The earlier measurements in §92 (stage wait 8.0~87.3 s, native crash at 30 arrivals) are the reference scale, but since the layer below is replaced by the mock this time, **what is observed is pure P4 behavior**.

#### Gap with the current code

| Item | Current |
|---|---|
| 1 | Holds |
| 2 | **Half.** `foundation/task_queue` has tokio workers, but `P4Handler::handle` in [`transport/mod.rs`](layers/runtime/src/foundation/transport/mod.rs) is a synchronous completion contract (`D-26`) |
| 3 | **Broken.** `TcpTransport::dispatch` opens a socket on every call and blocks until terminal — the worker stalls inside the queue (`D-27`·`D-30`) |
| 4·5 | **Missing.** The envelope has no concept of classification (`D-69`) |
| 6 | **Missing.** Relaying is a static route map, and the return to OUTER is a separate path (`D-48`) |
| 7 | **No.** `serve()` spawns a thread per connection and calls the handler inline |
| 8 | Exists, but the kind branching is duplicated (`D-66`) and messages skip the node and go straight to the adapter (`D-67`) |

---

# Topic A. Hardware query protocol

For premise 1 to hold, the outside must be able to query machine specs and orchestrate nodes on top of them. The current protocol has **no contract** for this query.

## 2. Current status (verified)

The `INVENTORY_QUERY`(34) → `HARDWARE_REPORT`(35) round trip is implemented.

- Request: `controller_id`, `request_id`
- Response: `agent_id`, `report_id`, `snapshot` (bounded text)
- Direction: `ExternalController` — [`catalog/mod.rs`](layers/protocol/src/catalog/mod.rs)
- Class: Terminal. The response closes the route

The whole implementation of `snapshot` is 35 lines in [`domain/hardware/mod.rs`](layers/runtime/src/domain/hardware/mod.rs).

```json
{ "observed_at_unix_ms": 0, "os": "windows", "arch": "x86_64",
  "cpu_physical": 16, "cpu_logical": 32,
  "gpus": ["GPU-abc…, NVIDIA RTX 4090, 24564, 23100, 550.54.14"],
  "adapters": [...], "nodes": [...] }
```

`os` is `env::consts::OS`, that is, **a compile-time constant**. `gpus` is **an array of raw strings** from `nvidia-smi --format=csv,noheader,nounits`.

## 3. Defects

### D-1. `snapshot` has no schema contract
At the P4 layer, `snapshot` is just text of 256 KiB or less. It has no version, no required fields and no validation.
Evidence: [`message/mod.rs`](layers/protocol/src/contract/message/mod.rs) `HardwareReport.snapshot: String`

### D-2. RAM information is entirely missing
Neither total nor available. There is no basis at all for judging whether CPU offload is possible, the KV budget or mmap suitability. The primary input to node orchestration is missing.

### D-3. GPU information is unstructured and NVIDIA-only
- Raw `nvidia-smi` CSV strings → the outside has to split strings without any convention
- Units are undocumented (`nounits` is MiB)
- No AMD, Intel or Apple path
- No compute capability, PCIe bus or NVLink peers → multi-GPU placement cannot be judged
- A process is spawned on every query. On failure it silently returns an empty array, so **"no GPU" and "driver error" are indistinguishable**

### D-4. CPU and OS information falls short of what orchestration needs
Only 2 core counts (`cpu_physical`, `cpu_logical`). No model name, clock, socket/NUMA or ISA extensions (AVX-512, AMX). The OS is only a family string, without kernel, version or distribution.

### D-5. No storage capacity information
No total or free model storage. Whether a model can be loaded cannot be known before placement.

### D-6. The agent has no way to speak first
Every P4 response goes out only on an already open `route_id`. `HARDWARE_REPORT` is terminal and closes the route. Both [`agent-link.mjs`](tools/controller/client/transport/agent-link.mjs) and [`controller-instance.mjs`](tools/controller/client/controller-instance.mjs) implement only request initiation.

Result: the outside can ask **only endpoints it already knows**. The outside has no starting point for "which nodes exist". For premise 1 this is more critical than D-1~D-5.

### D-7. There is no way to detect a process replacement (redefined)
`format!("agent-{host}-{pid}")` — [`domain/agent/mod.rs`](layers/runtime/src/domain/agent/mod.rs).

This originally said "after a restart the same machine becomes a different agent, so records cannot be keyed". **The keying problem was resolved in P-2** — the access address is the identity, so the key is already stable.

The real remaining defect points the other way. Because the address is stable, **OUTER cannot tell when an agent restarts and loses all of its `NodeSlot`s and bindings.** There is no marker for detecting a process replacement (P-39).

### D-8. Immutable and volatile information are lumped together
`cpu_physical` (unchanged until the hardware is replaced) and `memory.free` (changes by the second) sit in the same document with the same trust level. Without separating them, there is no way to enforce the rule "do not use observed free values for orchestration".

## 4. Remediation design (draft)

### P-1. Separate capability and occupancy

| Kind | Nature | Contents | Externalization |
|---|---|---|---|
| **capability** | Changes only when hardware, drivers or **the build** change | **Runtime variant** (P-61), CPU (model, physical/logical, sockets, NUMA, ISA), total RAM, per GPU (uuid, vendor, model, total VRAM, architecture, PCIe, NVLink peers), total storage, OS/kernel version | Stored as an external record. Changes detected via `capability_revision` |
| **occupancy** | Changes by the second | Free VRAM/RAM, utilization, temperature/power, currently loaded bindings | Must not be stored. For diagnosis and verification only |

**Orchestration rule (mandatory):** node orchestration does not use occupancy as input. Compute `capability total − placements declared by the external record`, and use observed free values only to verify that result.
Rationale: with placement based on free VRAM, two controllers see the same free space and commit at the same time. What prevents the race is the external record, not the report.

### P-2. The access address is the agent's identity (rewritten under P-34)

**Draft (withdrawn):** the proposal was a 3-layer identity of `machine_id` / `boot_id` / `agent_instance_id`, written as the fix for D-7, where `agent_id` is tied to the PID.

**Decided:** per P-34, every message self-describes the access address of its target agent, and that address is an infrastructure fact owned by OUTER (premise 1). **A separate agent ID is therefore meaningless — the access address itself is the agent's ID.**

| Existing use | Replacement |
|---|---|
| Key of the capability record | Access address |
| Identity check in `is_local_bypass` | Access address comparison |
| `HARDWARE_REPORT.agent_id` | Unnecessary — the asker already knows the address |
| `Participant.agent_id` | Access address |

`machine_id` is unnecessary too. The address is stable across restarts and reboots — more stable, in fact, than a PID-derived value. The same goes for `boot_id`: occupancy is not stored (P-1), so there is nothing whose validity range needs marking.

**The one thing that remains: a process incarnation marker** — see P-39 below. It is a generation marker, not an identity.

`node_id` is already issued externally (`randomUUID` in `createNode` of [`controller-instance.mjs`](tools/controller/client/controller-instance.mjs)) — this axis is already consistent with premise 1.

### P-39. Process incarnation marker (a generation, not an identity)

Even with the address standing in for identity, there is one fact it cannot cover. **When an agent restarts, all of its `NodeSlot`s and bindings disappear** — the current registry lives only in process memory and is not persisted (D-9 territory). The address stays the same, so OUTER cannot tell that its records have become invalid.

What is needed is not an ID but **a monotonically increasing marker that distinguishes "same address, different incarnation"**. It plays for the agent process the role that `runtime_generation` plays for the binding instance.

This is the real fix for D-7 — the problem was not "the ID is tied to the PID" but **"a process change cannot be detected"**. Introducing persistence (§94.1) changes the required scope, so the two are judged together (Q-42).

### P-3. Make the `snapshot` schema normative
Standardize it as versioned JSON (`schema_version` required). Do not promote it to wire fields — hardware attributes change faster than the codec, so promoting them to fields would bump the protocol version every time a GPU attribute is added. Instead, pin the schema down as **a norm** and validate it. The best-effort nature remains only in the occupancy section.

### P-4. agent-initiated announce (depends on Q-3)
Required if premise 1 is pushed all the way.
- New direction: Agent → External (not in the current `TaskDirection`)
- Non-terminal update frame (the current `HARDWARE_REPORT` is terminal)
- Announce on appearance + re-announce on capability change

**This is a P4B1 v6-class change.** It means giving up v5 compatibility, so it is judged together in Q-3.

---

# Topic B. Node ownership and the node's concrete instance

This is the result of checking premise 3 against the code.

## 5. Current status (verified)

### 5.1 Where the code already matches premise 3

- **A node definition is a hardware constraint, but `NodeSlot` has no constraints.** `NodeSlot` has only `controller_id`, `adapter_id`, `max_inflight`, `admission` and `bindings` — [`registry/node/mod.rs`](layers/runtime/src/domain/agent/registry/node/mod.rs)
- **The agent does not know the detailed constraints.** As the comment in [`node_spec/mod.rs`](layers/runtime/src/domain/agent/lifecycle/node_spec/mod.rs) says — the Agent reads only the single field `p4_max_inflight` and ignores the rest
- **The concrete instance appears at model load.** `bind()` creates `Binding{deployment_id, generation}` only on `MODEL_BOUND(state=ready)`
- **The partial-loading strategy is decided and injected from outside.** `stage_plan` is not interpreted by the Agent and passes through to the adapter
- **Unloading keeps the id and removes only the concrete instance.** `unbind()` only removes the entry from `bindings`
- **`runtime_generation` is issued by the adapter.** The Agent only records the value from `MODEL_BOUND`

### 5.2 Where the code diverges from premise 3

Nodes are currently **controller-owned**. `NODE_CREATE` carries `controller_id`, and `NodeSlot::new(controller_id, …)` stamps ownership. After that, `MODEL_LOAD`, `MODEL_UNLOAD`, `HEALTH_CHECK` and `EXECUTE` all go through `owned_node()` — [`authorization/mod.rs`](layers/runtime/src/domain/agent/authorization/mod.rs).

## 6. Defects

### D-9. Node removal is not in the protocol
There is no `nodes.remove` call anywhere in the repository and no `NODE_DELETE` kind. **A node's lifetime equals the Agent process's lifetime.** This gap must be filled if the outside is to manage the node list authoritatively.

### D-10. Re-applying `NODE_CREATE` is a silent no-op
`entry(node_id).or_insert_with(…)` in [`lifecycle/mod.rs`](layers/runtime/src/domain/agent/lifecycle/mod.rs). Creating again with the same `node_id` ignores the new `node_spec`, and the caller gets a success response and wrongly assumes it was applied. Update and ignore are indistinguishable.

### D-11. The hardware constraints in `node_spec` are not stored
`create_node` extracts only `max_inflight` and discards the original. It is passed to the adapter but not kept in the registry. The constraints are not merely absent from the instance; **they vanish inside the agent.** If the outside does not remember them, nobody does — consistent with premise 1, but since there is no external record yet either, they are simply lost.

### D-12. The `controller_id` gate is not authentication
`controller_id` is **a self-declared value** written in the frame, and there is no wire authz. Sending a different value goes straight through. So the `ForeignController` refusal is a guard against mistakes, not security.
**Removing this gate therefore costs no security.** If real access control is needed, that belongs to a separate authz layer; it was never something the `controller_id` string could solve.

### D-13. `plan_revision` is used only as a log string
`MODEL_LOAD` carries it, but it is neither stored nor compared; it is only inserted into the adapter's `detail` message. A slot for the version of the external intent already exists but is empty.

## 7. Remediation design (draft)

### P-5. Four-layer model

| Layer | Owner | Lifetime | Involvement |
|---|---|---|---|
| Orchestration intent (hardware constraints, placement plan) | **External record** | Permanent | Outside P4 |
| Node slot (id + adapter + capacity) | Agent | create ~ delete | External → agent |
| Node instance (concrete adapter, partially loaded layers) | Adapter | load ~ unload | External → agent → adapter |
| Execution | **Controller** | Per request | Where the controller first appears |

A node slot is a shell with only an id and capacity; the concrete instance exists only in the third layer. Orchestration constraints exist only in the first layer, and P4 does not carry them.

### P-6. Remove `controller_id` from lifecycle
Remove it from `NODE_CREATE`, `MODEL_LOAD`, `MODEL_UNLOAD` and `HEALTH_CHECK`. `controller_id` in `EXECUTE` stays, but its meaning changes **from an ownership gate to relay-target identification and tracing** (Q-6).
Knock-on changes: add an external→agent direction to `TaskDirection`, rewrite `allows_direction`, delete `ForeignController` from `authorization` (keep `UnknownNode`, `Dangling` and `BindingNotReady`), remove `NodeSlot.controller_id`.

### P-7. Add `NODE_DELETE` / `NODE_DELETED`
Fixes D-9. Handling when active bindings or executions exist is Q-8.

### P-8. Separate the two axes `plan_revision` / `runtime_generation`
- `plan_revision` — **the version of the external intent.** Issued by the external record; the agent records and compares it
- `runtime_generation` — **the generation of the adapter instance.** Keeps its current role and is still issued by the adapter

The two axes have different update cycles and issuers, so they are not merged.

---

# Topic C. Model load protocol

Premise 4. Load instructions come from outside, and their expressiveness must match the level of the concrete runtime.

## 8. Current status (verified)

### 8.1 What the contract defines

The items defined by `stage_plan.load_options` in [`docs/model-load.md`](docs/model-load.md) are, in full:

`flash_attention`, `mmap`, `kv_cache.{type_k, type_v, offload}`, `batching.{strategy, max_sequences, node_limits[], context_batch_tokens, context_ubatch_tokens, calculation}`, `adapter_options` (free-form object)

### 8.2 Actual handling

| Party | Behavior |
|---|---|
| P4 protocol | Does not interpret `stage_plan`. Bounded text |
| Pipeline adapter | Reads **only one field**, `load_options.batching.max_sequences` — [`capacity/mod.rs`](layers/adapters/adapter/src/domain/capacity/mod.rs) |
| Pipeline adapter (load) | POSTs the whole `stage_plan` **as is** to the host supervisor `/api/runtime-groups`. No validation — [`lifecycle/load/mod.rs`](layers/adapters/adapter/src/application/lifecycle/load/mod.rs) |
| stock llama.cpp adapter | **Refuses** if any `load_options` key **is present** (`require_process_start_compatible`), because it points at an already started process |

### 8.3 Actual upstream surface

In the pinned [`apps/llama/upstream`](../llama/upstream), `common/arg.cpp` has **347** `add_opt` calls. Even counting only those that matter at load time, the following categories are missing from the contract.

| Category | Representative flags |
|---|---|
| Layer/tensor placement | `--n-gpu-layers`, `--override-tensor`, `--n-cpu-moe`, `--cpu-moe`, `--tensor-split`, `--main-gpu`, `--device`, `--rpc` |
| KV/context | `--ctx-size`, `--parallel`, `--kv-unified`, `--ctx-checkpoints`, `--checkpoint-min-step`, `--defrag-thold`, `--swa-full`, `--context-shift`, `--cache-ram`, `--cache-reuse`, `--cache-idle-slots` |
| Memory loading | `--mlock`, `--direct-io`, `--no-repack`, `--check-tensors`, `--load-mode`, `--numa` |
| RoPE/attention | `--rope-freq-base`, `--rope-freq-scale`, `--rope-scaling`, `--yarn-*`(5), `--grp-attn-n/w`, `--flash-attn`, `--attention` |
| Threads/batching | `--threads`, `--threads-batch`, `--batch-size`, `--ubatch-size`, `--cpu-mask`, `--cpu-range`, `--cpu-strict`, `--poll`, `--prio`, `--cont-batching` |
| Speculative decoding | `--spec-draft-model`, `--spec-draft-ngl`, `--spec-draft-n-max/min`, `--spec-draft-device`, `--spec-draft-type-k/v`, `--eagle3`, `--mtp` |
| Auxiliary artifacts | `--lora`, `--lora-scaled`, `--control-vector`, `--control-vector-layer-range`, `--mmproj`, `--mmproj-offload` |
| Metadata/templates | `--override-kv`, `--chat-template`, `--jinja`, `--reasoning-budget` |

The absence of `--override-tensor` is a particular problem. It is the actual tool for partial layer loading, yet the contract has no place for it.

## 9. Defects

### D-14. The expressiveness of load options covers only a tiny part of the runtime
Comparing the item count in §8.1 with the surface in §8.3 makes this obvious. In particular, tensor-level placement (`--override-tensor`), MoE separation (`--n-cpu-moe`) and device selection (`--device`, `--tensor-split`) are missing, so partial-loading strategies cannot be expressed in the protocol.
**Resolution path (decided):** per premise 6, P4 does not own the option schema. Options pass through as opaque strings and the concrete adapter interprets them. This defect moves down from a protocol defect to **adapter implementation scope** — expressiveness becomes a question of how much of the upstream surface the adapter covers.

### D-15. `MODEL_LOAD.model` is a single string and cannot express multiple artifacts
Draft models, mmproj, LoRA and control vectors are all **additional artifacts**. Speculative decoding means loading a second model.
**Resolution path (decided):** auxiliary artifact paths go inside the option string, and the adapter interprets them. The `model` field remains an identifier that points at the main model. No wire change is therefore needed (Q-11 withdrawn).

### D-16. The validating party described in the docs differs from the implementation
`docs/model-load.md` says "the selected adapter validates, filters and applies the common fields of `load_options` and `adapter_options`", but the Pipeline adapter validates nothing other than `batching.max_sequences` and forwards everything wholesale. No code at the P4 layer enforces the rule "unsupported options must not be silently ignored".

### D-17. The same field is either entirely ignored or entirely refused, depending on the adapter
Pipeline passes it through; stock llama.cpp returns `ERROR` just because it is present. The protocol gives the caller no way to know which applies. `ADAPTER_REGISTER.descriptor` is the natural place, but it does not declare the set of supported options.

### D-18. `load_options` is a cluster plan, but `MODEL_LOAD` is per node
`batching.node_limits[]` carries **the values of other nodes as well**. That is why the fallback rule "top-level `max_sequences` is the minimum of all `node_limits`" became necessary. The cluster-wide plan is sent redundantly to every node, and each adapter has to pick out its own share.

### D-19. The host API receives `deployment_id` under the name `controller_id`
`start.insert("controller_id", deployment)` in [`lifecycle/load/mod.rs`](layers/adapters/adapter/src/application/lifecycle/load/mod.rs). The field name and content do not match, and it is a leftover that conflicts with the controller separation of premises 3 and 4.

### D-60. Load-time declarations fix values that the runtime derives

`capacity::declare()` reads `stage_plan.load_options.batching.max_sequences` at `MODEL_LOAD` time and **installs it as a per-deployment semaphore gate** — [`capacity/mod.rs`](layers/adapters/adapter/src/domain/capacity/mod.rs). A load-time constant governs execution-time admission.

**The mechanism is confirmed** (evidence as of 2026-08-13). `max_sequences` is the cap of the credit ledger, and the scheduler fills only while `in_flight < credit_limit`. Then `scheduler_window_target` splits the active cohort into `pipeline_stage_count` windows **below that cap**. In other words, the runtime already derives this in real time, and the load-time declaration **functions as a cap.**

The problem is not that the declaration exists but that **it is installed as an authoritative gate**. As long as window splitting is derived, the declaration has no reason to be more than a safety guard.

The failure that the comment in the same file warns about repeats at a different level — a process-global constant as a gate put "the throttle three levels above the GPU", and a load-time declaration as a gate has the same shape.

Note also that the entire `node_limits[]`/`calculation` machinery of `D-18` exists only to compute this static number.

**This item depends on the other session's scheduler conclusion (Q-48).** It is not finalized here on its own.

### D-84. Overlap depth exists only as an environment variable
Stage overlap is enforced by `LINKER_PIPELINE_WINDOW_DEPTH`, and its default is derived from `pipeline_stage_count`. **There is no protocol surface.**

Yet depth is entangled with orchestration — as the evidence document shows, without overlap the wall time follows the **sum** of stage costs, so a balanced placement (13/27) is worse than 20/20; with overlap the wall time follows the **maximum**, and the calculation flips. Session count, layer split and depth form one bundled decision.

Depth is therefore **planning knowledge** (P-60), and OUTER decides it. Per premise 6, the value can simply be passed as an opaque load option, but **as long as it exists only as an environment variable, OUTER has no channel through which to specify it.**

### D-61. The protocol has no connection establishment policy

Source routing (P-22) and self-describing addresses (P-34) assume that **a connection can be established at every hop**. But P4 has no rules for retries, backoff or connection budgets.

A parallel session is working on `connect()` returning `EHOSTUNREACH` during ring formation and on connection budgets and backoff. Once the chain moves into the protocol, this policy must also become part of the contract — at which hop to retry how many times, and when to terminate a failure as `ERROR`.

### D-20. Load reports are tied to the request route, so there is no reconnection path
`LOAD_PROGRESS` and `DRAFT_REPORT` are non-terminal and `MODEL_BOUND` is terminal, and all of them go out only on the `route_id` the request came in on. **If the requester disconnects, neither progress nor completion is delivered anywhere.** Loading takes minutes, so this is a real risk, and the hole remains even if premise 5 (load reports go to the requesting outside party) holds.

From the viewpoint of premise 1's external record it is worse — missing the completion makes the external record and the actual load state diverge, and there is no query with which to recover (the same root as D-6).

## 10. Remediation design (draft)

### P-9. Options pass through as opaque strings (decided)

Premise 6. P4 does not own the structure of load options. `MODEL_LOAD` carries options as bounded text, and interpretation is entirely up to the concrete adapter.

Rationale: upstream has 347 `add_opt` calls and the number keeps growing. Hard-coding them as a schema would break the P4 contract every time upstream moves. Auxiliary artifact paths (draft, mmproj, LoRA, control vector) also go inside this string, so promoting wire fields is unnecessary.

**Design discarded as a result:** the draft that would have defined a three-part split of options (artifacts/placement/tuning) as a P4 contract. That distinction is meaningful, but it is **a convention shared by adapters and the external planner**, not P4's concern.

### P-10. Remaining problem — a means of discovery in advance (reduced)

P-9 resolves most of D-14~D-17. Since the adapter interprets the options, it can return `ERROR` for unsupported ones, and the "ignore vs refuse" split also moves down into adapter implementation rules.

**One thing remains:** the external planner has no way to know what the adapter supports **before sending**. Today the only way is trial and error: send and get `ERROR`. Loading is expensive, so failures are costly.

There are two options.
- (a) **Only declare** the set of supported option keys and the schema version in `ADAPTER_REGISTER.descriptor`. P4 still does not interpret it and only passes it outside
- (b) Accept trial and error. Consider it sufficient for the `ERROR` detail to name the unsupported keys

(a) does not conflict with P-9 — the declaration is a string the adapter produces, and P4 only carries it. Judged in Q-9.

### P-13. Redirecting load reports and reconnection (premise 5)

**The redirection is close to a relabeling.** Even today the external client that called `loadModel` receives `LOAD_PROGRESS` → `DRAFT_REPORT` → `MODEL_BOUND` on the same route ([`controller-instance.mjs`](tools/controller/client/controller-instance.mjs)). What needs to change is the part where `allows_direction` classifies them as `NodeController`, and that belongs to the same change as P-6's direction redefinition.

**The real work is D-20.** A way to recover progress and completion after the request route drops is needed. Options:
- (a) Add a request that queries an in-progress load — ask for the current state by `operation_id`
- (b) Include load state in the node state query — piggyback on D-6's query mechanism without a separate message
- (c) Carry load completion in the agent-initiated announce (P-4) — requires adopting Q-3

Judged in Q-14. (b) is the likely choice because it adds no new message, but it gives up real-time progress.

### P-44. Demote `batching.*` from an authoritative gate to a cap

The fix for D-60. **Measurements agree with the demotion option** — the runtime already derives the windows, and the declaration works as a cap (§92).

| Option | Contents |
|---|---|
| Keep | Current behavior. The load-time declaration is the admission gate |
| **Demote** | The declaration is used only as **a cap hint**, and the runtime derives the actual width from the active cohort |
| Remove | Take `batching.*` out of the contract; the runtime owns it entirely |

The demotion option fits the principle of §92 best — P4 does not own throughput; **it only hands over the knobs**. A cap is meaningful as a safety guard, but the width at each moment must be decided by the layer closest to the GPU.

Whether it is demoted or removed, the `node_limits[]`/`calculation` structure of `D-18` is cleaned up along with it.

### P-11. Separate the cluster plan from node instructions
`MODEL_LOAD` carries **only what that node has to do**. The cluster-wide plan stays in the external record, and only the necessary cross-node information (total stage count, neighboring stage identifiers and so on) is passed as explicit fields. This removes D-18's fallback rule and the redundant per-node sending.

### P-12. Redirecting load instructions
Per premise 4, move `MODEL_LOAD`/`MODEL_UNLOAD` to the external→agent direction (the same change as P-6). The `controller_id` leftover of D-19 is removed at the same time.

---

# Topic D. Load/release preconditions and the node state machine

Premises 7 and 8. Commands depend on the node's prior state, and failures call the requester as messages.

## 11. Current status (verified)

### 11.1 Where precondition checks already exist

`MODEL_LOAD` and `MODEL_UNLOAD` go through `exclusive()` — `admission::lifecycle(slot)` acquires **all of the slot's permits** without blocking, via `try_acquire_many_owned(max_inflight)`. If even one execution is in progress, it fails immediately and emits `ERROR("node … admission is full")`.

So **"load/unload fails while the node participates in inference" is already implemented.** Only this part of premise 7 is satisfied.

### 11.2 Response delivery structure

`capture` in [`forward/mod.rs`](layers/runtime/src/domain/agent/lifecycle/forward/mod.rs) **streams every adapter response straight through to the caller first**, and keeps a copy of the terminal among them. The agent looks at that copy and decides belatedly whether to update the registry.

This structure collides head-on with premise 8. By the time a response has been streamed through, the requester has already been called.

## 12. Defects

### D-21. Releasing an already released target is reported as success
[`lifecycle/unload/mod.rs`](layers/adapters/adapter/src/application/lifecycle/unload/mod.rs) **treats HTTP 404** from `DELETE /api/runtime-groups/{deployment}` **as success** and emits `MODEL_UNBOUND`. This contradicts premise 7 ("fail if already unloaded").

### D-22. Two terminals can go out on one route
Following D-21, the agent calls `slot.unbind()` after receiving `MODEL_UNBOUND`, and on `UnknownBinding` emits `ERROR` via `refuse()`. Since `capture` has already streamed `MODEL_UNBOUND` through, **the requester receives `ERROR` after `MODEL_UNBOUND`.**

This violates the contract that a terminal closes the route. On the client side, `AgentLink` deletes the route at the first terminal, so the trailing `ERROR` is **silently dropped** — a failure is observed as a success.

### D-23. Loading onto an already loaded node silently overwrites it
`NodeSlot::bind()` is `bindings.insert()`. There is no precondition check. This contradicts premise 7 ("loading fails if the node already has a model loaded"), and the existing binding is replaced without warning.

### D-24. Node:binding is 1:N, so "the node's concrete instance" is not defined as singular
`NodeSlot.bindings` is `HashMap<binding_id, Binding>`. One node can hold several bindings at once.

The model of premises 3 and 7 ("the node's instance = the loaded concrete adapter object"; no instance after release) assumes **0 or 1**. Unless this mismatch is resolved, the verdict "the node is already loaded" cannot even be defined. This is the prerequisite problem for D-23.

### D-25. The unit of release differs by layer
P4's `MODEL_UNLOAD` works per `binding_id`, but the adapter `DELETE`s per `deployment_id`. If the same deployment has several bindings, **releasing one deletes the whole group.** Premise 7's "complete teardown of the instance" can reach further than intended.

## 13. Remediation design (draft)

### P-14. Preconditions are settled before any emit (premise 8)

In CPS, an emit is a call to the requester and cannot be undone. Therefore:

1. Check every precondition **before emitting any message**
2. Once the checks pass, the command has exactly one result message — a success terminal or a failure terminal
3. **Forbid structures in which the agent reverses its judgement after streaming an adapter response through**

`forward::capture` needs a redesign. The adapter's terminal must reach the requester only after the agent finishes its judgement, or the agent must emit its own terminal in its place. This is the root fix for D-22.

### P-15. Make the node state machine explicit

```text
(none) ──NODE_CREATE──▶ empty ──MODEL_LOAD──▶ bound ──EXECUTE──▶ active
                          ▲                     │                  │
                          └────MODEL_UNLOAD─────┘◀─────done────────┘
   empty ──NODE_DELETE──▶ (none)
```

| Command | Allowed prior state | Otherwise |
|---|---|---|
| `MODEL_LOAD` | `empty` | Fail (`bound` is already loaded, `active` is executing) |
| `MODEL_UNLOAD` | `bound` | Fail (`empty` is already released, `active` is executing) |
| `NODE_DELETE` | `empty` | Q-8 |
| `EXECUTE` | `bound` | Fail |

Replacement is not a single command. It is possible only in two steps: `MODEL_UNLOAD` → `MODEL_LOAD`.
Blocking in `active` is already implemented by `admission::lifecycle` (§11.1). What is newly needed is the `empty`/`bound` check.

### P-16. Narrow node:binding to 1:1 (depends on Q-15)
P-15's state machine is defined only on the premise that a node has at most one instance. Narrowing `NodeSlot.bindings` to `Option<Binding>` resolves D-23 and D-24 together, and D-25's unit mismatch also lines up as "one node = one instance".

If several models are wanted on one node, creating several nodes is what is consistent with the model of premises 3 and 5.

---

# Topic E. Full CPS audit

Premise 9. This is the result of surveying every return-value-based handling path.

## 14. Current status (verified)

Handling paths split in two, and only one side is CPS.

### 14.1 Paths that follow CPS

`INGRESS_SUBMIT`, `EXECUTE`, `CANCEL`. [`dispatch/mod.rs`](layers/runtime/src/application/dispatch/mod.rs) puts the follow-up Task on the queue and returns immediately, and `QueueResponseSink` re-enters remote responses into the queue via `enqueue`. The `causation_id` chain continues.

### 14.2 Return-value-based paths

`NODE_CREATE`, `MODEL_LOAD`, `MODEL_UNLOAD`, `HEALTH_CHECK`, `INVENTORY_QUERY`, `ADAPTER_REGISTER` — **the whole control plane**. `dispatch::compatibility` is this path, and the function name itself already admits what it is.

The three foundational contracts are all synchronous-completion style — [`foundation/transport/mod.rs`](layers/runtime/src/foundation/transport/mod.rs).

```rust
trait P4Handler   { fn handle(&self, message, responses) -> Result<()>; }
trait P4Transport { fn dispatch(&self, message, responses) -> Result<()>; }
trait ResponseSink{ fn emit(&mut self, message) -> Result<()>; }
```

## 15. Defects

### D-26. `P4Handler`/`P4Transport` are synchronous completion contracts
Both traits mean "if it returned, it is done". The signatures leave no place to exit to the queue mid-processing. The violation of premise 9 is **baked into the foundational contract**, not into individual implementations.

### D-27. `TcpTransport::dispatch` is a blocking RPC
Each call connects a new `TcpStream` and loops on `read_message` until a terminal arrives. It does not use persistent socket multiplexing (`peer_mux`). Even setting CPS aside, it wastes resources.

### D-28. `forward::capture` hands responses back as a return value
A head-on violation of premises 8 and 9, and the direct cause of `D-22` in topic D. After streaming every response through to the caller, it copies the terminal and **returns** it, and the agent branches on that return value.

### D-29. All six lifecycle handlers branch on return values
`create_node` records the slot after checking for `NodeCreated{state=="ready"}`, `load_model` calls `bind()` after seeing `ModelBound{state=="ready"}`, and `unload_model` calls `unbind()` after seeing `ModelUnbound`. All of them inspect `capture`'s return value.

### D-30. Long-running work is trapped inside one blocking call
The adapter's load is a synchronous `http::json` call, and it runs **a 600-second deadline polling loop** inside the handler — [`lifecycle/load/mod.rs`](layers/adapters/adapter/src/application/lifecycle/load/mod.rs). A job that lasts minutes is not split into Tasks, so its progress never appears on the queue.

Because `compatibility` hands the work off via `spawn_blocking`, **the queue worker itself is not blocked.** But that only pushes the work onto the blocking pool; it does not make it CPS, and the three items below are the price.

### D-31. `deadline_unix_ms` is not enforced on the control plane
Deadline checks exist only in admission ([`agent_host/mod.rs`](layers/runtime/src/application/agent_host/mod.rs)) and in the execute path of `peer_mux`. Neither the queue worker nor the `compatibility` path has one. A lifecycle job admitted just before its deadline **runs without limit.**

### D-32. `CANCEL` does not reach the control plane
`dispatch::cancel` only `abort()`s relays in the `active` map, and only the execute path registers entries in that map. Lifecycle jobs handed to `spawn_blocking` **have no cancel handle at all.** In other words, a model load in progress cannot be stopped.

### D-33. The causation chain breaks in blocking sections
What happens inside one `dispatch` call is not a Task, so no `task_id`/`causation_id` is created. The queue cannot show at which step a load stopped, and no resume point is defined. This has the same root as `D-20` in topic C (no recovery when the route drops).

## 16. Remediation design (draft)

### P-17. Unify the output path into one queue
Make `ResponseSink` the only output, and **remove return values that stand for a result** from handling functions. A return means no more than whether the item was accepted onto the queue. Discard `forward::capture`, and re-enter adapter responses as follow-up Tasks. `QueueResponseSink` already has that shape, so this is a generalization.

### P-18. Replace the adapter boundary with persistent socket multiplexing
Replace `TcpTransport` with `peer_mux`. This resolves D-27 and is a prerequisite of P-17 — for a response to arrive later, the socket must be decoupled from the call.

### P-19. Break long-running work into multi-step Tasks
Split loading into `start request → progress observation → completion verdict` steps, each of which queues the next. Progress polling becomes a Task that re-enqueues itself. D-30 and D-33 are resolved together, and the recovery path for `D-20` in topic C also comes from here.

### P-20. Apply `deadline` and `CANCEL` to every path
Once work is split into Tasks, the deadline can be checked on entry to each step, and `CANCEL` takes effect by blocking the enqueue of the next step. Resolves D-31 and D-32.

**Dependencies:** they stack in the order P-17 ← P-18 ← P-19 ← P-20. None of them holds unless the foundational contract (D-26) changes first.

---

# Topic F. Inference path and node chain

This is the result of checking the inference path of §1.2 against the code.

## 17. Current status (verified)

### 17.1 What already matches the target structure

**Single streaming mode.** P4 has no stream on/off switch. The only form of inference results is `TOKEN*` → `DONE`, and adapters also stream SSE deltas straight through. "Always streaming" already holds and is **a property to preserve**, not something to fix.

### 17.2 What diverges

`INGRESS_SUBMIT` carries a **single** `node_id`. There is no place to express a chain.

The chain currently lives outside P4. The Pipeline runtime's deployment (`/api/runtime-groups`) configuration owns the stage composition, and stage-to-stage exchange uses `linker-pipeline-inference-stream-v1`. The controller does not manage the chain — `ControllerProcessor` only turns the ingress into a single `EXECUTE` and hands it to one node.

## 18. Defects

### D-34. An inference request cannot express a node list
`INGRESS_SUBMIT.node_id` is singular. The target structure, "OUTER passes a node list to the controller", cannot be expressed on the current wire.

### D-35. The party that manages the chain is outside P4
The stage composition is baked into the Pipeline runtime's deployment configuration. The target structure has the controller compose the chain **at request time**, which cannot hold if the chain is fixed in load-time runtime configuration.

It also conflicts with premise 3 — the chain is orchestration intent (owned by OUTER), yet it currently sits inside the node instance (owned by the adapter).

### D-36. P4 has no concept of "the end of the chain"
`TOKEN`/`DONE` simply go adapter → agent → route owner. No field expresses which node is last and that its result must return to the controller.

### D-38. `EXECUTE` cannot carry the chain either, and node-to-node addressing is tied to static configuration
`node_id` in `ExecutionRequest` is also singular. Source routing needs the message to carry the whole chain, but there is no place for it.

Addressing is the more fundamental issue. Today the only way to reach another node is `RouteProcessor`'s `HashMap<node_id, SharedTransport>`, which is **a static map injected by configuration at startup** — [`routing/processor/mod.rs`](layers/runtime/src/application/routing/processor/mod.rs). A static map cannot follow a chain that changes per request.

### D-39. There is no reporting path for mid-chain failures
In source routing, hops only move forward. If `Node[2]` fails, there is no backward edge to tell the controller. The current structure has only one hop, so this problem never surfaced.

### D-40. `CANCEL` cannot reach the whole chain
`dispatch::cancel` only stops the active relay of its own `route_id`. The rest of the chain keeps advancing without knowing about the cancellation. This is a gap on a different axis from `D-32` in topic E (not reaching the control plane).

### D-37. The classification of the `NodeNode` direction was wrong (corrected)
This document classified `NodeNode` as "dead surface — decide whether to implement or delete" because no code emits it. **In the target structure it is a required direction.** Hidden-state transfer between nodes is the core of the inference path, so it is an implementation target, not a deletion candidate. §94.3 was corrected accordingly.

## 19. Remediation design (draft)

### P-21. The inference request carries an ordered node list
Replace the single `node_id` of `INGRESS_SUBMIT` with an ordered node specification. Each entry must specify `(agent, node_id, binding, runtime_generation)` to point at an executable instance (§1.3's 4-tuple rule).

The controller takes this list and composes the chain, but does not query the concrete state of each node. OUTER guarantees validity at orchestration time.

### P-22. Carry the chain in the message and source-route it (decided)

The prefill `EXECUTE` that the controller sends to `Node[0]` **carries the whole chain.** Each node reads its own position and the next target from that message and forwards on its own. The controller does not intervene at each hop.

Hidden state itself is still moved by the native data plane (the distinction in §6.2 is kept). What P4 carries is **the chain, the order and the correlation**; tensors do not go into frames.

Resolves D-35. This is where the `NodeNode` direction comes to life.

### P-25. Chain entries must be self-contained addresses
A static route map (D-38) cannot follow a chain that changes per request. A chain entry must be enough on its own to reach the node and execute.

```text
chain[i] = (agent reachable address, node_id, binding_id, runtime_generation)
```

Without `binding_id`/`runtime_generation`, execution at the hop destination may land on a stale instance (the same reason as §1.3's 4-tuple rule). It is also consistent with premise 2 — the message carries the state that is to be passed as arguments.

### P-26. Carry the return address in the message (target changed by premise 11)
The gist stays: failure reports from intermediate nodes (D-39) and result returns from the last node need a return address. But **the return target is not the controller.**

It was originally written as "controller return address", but under premise 11 internal-network nodes cannot reach the controller directly. The return address is **the entry agent**, which passes things on to the controller. If the reporting node already belongs to the entry agent, one hop is skipped (P-36).

With this, `TOKEN`/`DONE`/`ERROR` do not climb back up the chain; they go **directly to the entry agent**. Backward propagation remains unnecessary.

### P-27. Define `CANCEL` as chain-propagating
Cancellation must reach the whole chain (D-40). Since the chain is in the message, specify either that cancellation propagates forward along the same path, or that each node stops on its own per correlation. Q-27.

### P-23. Specify the return path of the last node
The last node of the chain sends generated tokens **to the controller**. For now the controller passes them straight through to OUTER, but the contract reserves room for later filtering and additional work.

So the direction rule for `TOKEN`/`DONE` stays two-level, `NodeController` → `ExternalController`; this part is unchanged from today.

### P-24. Pin down the single streaming mode in the contract
It effectively behaves this way today, but there is no written rule. If a backend accepts a non-stream switch in the `options` string, the contract can break silently, so specify that adapters refuse it.

---

# Topic G. Stage reports and the decode loop

Prefill is a compute step and also **a state-check step**. Each node is responsible for reporting entry and completion.

## 20. Current status (verified)

During inference a node emits **only two** messages: `TOKEN` (event) and `DONE` (terminal). There is neither an acceptance report nor a completion report.

The only precondition check is `binding_is_ready(binding_id, deployment_id, generation)` — it only checks that the binding exists and that the generation matches.

`Binding` holds only `deployment_id` and `generation` — [`domain/state/mod.rs`](layers/adapters/adapter/src/domain/state/mod.rs), [`registry/node/mod.rs`](layers/runtime/src/domain/agent/registry/node/mod.rs). **Neither the layer range nor the context size is recorded anywhere.**

`DRAFT_REPORT` is the only statistic, and it is a load-time memory measurement (`model_bytes`/`kv_bytes`/`layer_bytes`/`ffn_bytes`). No message carries inference statistics.

## 21. Defects

### D-41. There are no stage acceptance/completion report messages
No kind expresses "I received this prefill and can process it" or "I finished successfully". In the current single-hop structure, one `DONE` stood in for both, but a chain needs **two points in time per node**.

### D-42. There is no place to carry inference statistics
No message holds processing time, throughput, the size of the generated hidden-state sequence and the like. OUTER's monitoring can only be built on this information.

### ~~D-43. The node's layer range is recorded nowhere~~ (withdrawn)
The observation that `Binding`, `NodeSlot` and the P4 messages have no layer range is true. But **this is not a defect; it is an intended abstraction.**

A node is kept as unaware of its own load state as possible (premise 10). Judgements about layer numbers are therefore excluded from the entry check. OUTER, which decided the placement, guarantees the continuity, start and end of chain ranges at orchestration time — consistent with P-5's "orchestration constraints exist only in the first layer, and P4 does not carry them".

### D-44. There is no channel for context-overflow verdicts (reduced)
This originally listed "the binding's context size is not recorded" as a defect, but per premise 10 **it is not metadata for P4 to record.** Context size is a property of the instance, so the adapter already knows it from its own runtime.

The only remaining gap is **a channel to report the result** of that verdict, and that is covered by D-41 (no entry report). It is not treated as a separate item.

### D-45. The decode loop's cycle cannot be expressed
For the last node to ask node 1 to generate the next token, the chain must be **a ring**. The current `EXECUTE` points at a single target, and topic F's source-routed chain also assumed only linear forward movement.

### D-46. The disposition of `phase=DECODE` is settled (corrected)
This document had marked `phase=DECODE` as "no code generates it, so decide whether to delete it" and deferred it until after P-22. **In a structure where the decode loop cycles under node control, it is required.** Prefill hops and decode hops differ in payload and in cycle shape, so they need to be distinguished. §94.3 was corrected.

## 22. Remediation design (draft)

### P-28. Add stage entry/completion reports
Each node reports two points in time.

| Point | Meaning | On failure |
|---|---|---|
| **Entry** | Received the message and is able to process it | Precondition violation → `ERROR`, the chain stops advancing |
| **Completion** | Finished its own range and passed it on | — |

The preconditions checked at entry are limited to **what the node can judge without knowing its own load state** (premise 10).

| Check | Judged by |
|---|---|
| Does the specified binding exist, and does its generation match | Agent (`binding_is_ready`) |
| Is a model loaded (is the state `bound`) | Agent (P-15 state machine) |
| Does the prompt fit within the context | Adapter — a property of its own runtime, known without a query |
| ~~Does the layer range match this node's turn~~ | **Excluded.** OUTER guarantees it at orchestration time (D-43) |

Per premise 8, **the entry report must go out before any computation starts**, and a failure terminates with `ERROR` instead of computing.

### P-29. Inference statistics schema
Items carried by the completion report: processing time, throughput, size of the generated hidden-state sequence, node/binding identifiers, range position. For the same reason as premise 6, detailed extensions go into a string, but the minimum set needed for monitoring is kept as explicit fields (Q-29).

The controller does not interpret this and passes it through to OUTER — consistent with the controller's role in §1.2.

### ~~P-30. Expose the layer range as binding metadata~~ (withdrawn)
The proposal was for `MODEL_BOUND` to report the layer range and for the agent to record it in `Binding`. **It contradicts premise 10 and P-5.** Nodes and agents would learn the load structure, breaking the principle that orchestration constraints exist only in the first layer. Replaced by P-33.

### P-33. A node does not know its own load structure (premise 10)

What nodes and agents know stops at **whether a binding exists, and its generation**. They do not know which layers they took on or from which number to which.

| Concern | Owner |
|---|---|
| Which node takes which layer range | **OUTER** (orchestration intent, the first layer of the four-layer model) |
| Validity of the continuity, start and end of chain ranges | **OUTER** — guaranteed at orchestration time |
| Whether that range was actually loaded | Adapter — if not, the load itself fails |
| Whether this binding is valid at execution time | Agent — looks only at existence and generation |

With this, P4 does not need to carry the placement structure, and nodes remain replaceable parts. The price of leaving verification to OUTER alone is that **a mis-composed chain shows up only during execution** (Q-33).

This points in the same direction as load options being opaque strings (premise 6) — P4 interprets neither instructions nor structure.

### P-31. Define the chain as a ring and distinguish hops by `phase`
- `phase=PREFILL` — linear forward. `chain[i] → chain[i+1]`
- `phase=DECODE` — cyclic. `chain[n] → chain[0]`; the loop exits when the termination condition is met

The last node carries two responsibilities at once — it reports a token or the end of generation to the controller, and it asks node 1 to generate the next token.

### P-32. KV belongs to the node for the lifetime of the request (externalization exception)
For the decode loop to work, each node must hold that request's KV between hops. This has the same nature as topic A's occupancy — **a fact about the process, not a record.**

This is written down as an exception to premise 2. What gets externalized is orchestration intent and the chain; KV is an occupied resource tied to the request lifetime. A failure of a mid-chain node therefore means restarting that request, not migrating it.

---

# Topic H. Network topology and relaying

Premise 11. The firewall decides the structure.

## 23. Current status (verified)

**The current implementation assumes flat reachability.**

- `ControllerInstance` takes an `endpoint` and **connects directly over TCP** to that agent — [`agent-link.mjs`](tools/controller/client/transport/agent-link.mjs). It is a model in which OUTER connects to each agent individually
- `peer_mux` connects agent → agent endpoints directly
- `RouteProcessor` picks targets from a static `HashMap<node_id, SharedTransport>` map
- `TcpTransport::dispatch` opens a new socket to the destination on every call

In short, **it stands on the premise that every participant can reach every other.** There is no way at all to express a deployment split by a firewall.

## 24. Defects

### D-47. The protocol has no concept of hierarchical relaying
A message knows only its sender and its final target. There is no place to express "through this gateway, then over there". In premise 11's deployment, all external traffic is relayed in two steps, but that structure does not show up on the wire.

### D-48. The controller's relay is based on a static map
`ControllerProcessor::Remote(RouteProcessor)` forwards only with the route map injected at startup. It cannot relay to targets that vary per request. The control path has to go through the controller, but that passage is static.

### D-49. Agents cannot relay
An agent handles only messages it is to perform itself. **There is no handling path that simply forwards to another agent.** `AgentProcessor::handle` treats every kind as its own, and there is no place to express that the target is a different agent.

Under premise 11, the entry agent must **pass on, not consume,** many of the messages it receives. Moreover, relaying is not a privilege of the entry agent but **a general capability every agent must have**.

### ~~D-50. The gateway for the load path is undefined~~ (resolved)
This said "at load time there is no node 1 yet, so the gateway is undecided", but the issue disappeared once the entry agent was defined as a pure entry point unrelated to node placement. Node create/delete, load/release and inference **all pass through the same entry agent**. Q-34 is closed with it.

### ~~D-52. Agents do not report their own reachable address~~ (withdrawn)
The observation that `HARDWARE_REPORT.snapshot` lacks the agent's own endpoint is true, but **it is not a defect.**

OUTER owns infrastructure facts and already knows every agent's access address (premise 1). Trying to discover this through the protocol creates the ordering contradiction "you need the address to ask, but you have to ask to learn the address". An agent **does not even need to know its own address** — the same direction as the principle premise 10 applies to nodes.

So the reachable address is not put into the capability report. Q-40 is closed with it.

**The remaining distinction:** the adapter's `ADAPTER_REGISTER.endpoint` is different. Adapter processes appear and disappear dynamically, and their address is a fact internal to the agent, so self-registration is right. **Do not confuse infrastructure facts owned by OUTER with dynamic facts internal to the agent.**

### D-54. The agent issues `session_id`
If `INGRESS_SUBMIT.session_id` is empty, the agent issues one in the form `{controller_id}-session-{n}`. What is more, **the issuer exists in two places, with counters that do not know about each other** — [`ingress/mod.rs`](layers/runtime/src/domain/agent/ingress/mod.rs) and [`routing/processor/mod.rs`](layers/runtime/src/application/routing/processor/mod.rs). If the same controller_id takes both paths, they collide.

This was first recorded as a plain duplication defect, but **it violates the issuance principle of §1.3.** OUTER issues identifiers, and the moment the agent names something, the agent is the only party that knows the name. It is also a leftover in that it embeds the abolished `controller_id` in the name (P-6).

Fix: OUTER issues `session_id`, and empty values are not allowed. Remove both issuers.

### D-53. Self-describing addresses combine with the absence of wire authz
Once addresses are carried in messages, an agent **opens connections to wherever the message says.** P4 currently has neither TLS nor wire authz (document §9), so anyone who can inject a frame can make an agent connect to an arbitrary address.

This is acceptable on the premise that the internal network is the trust boundary, but **in premise 11's deployment the entry agent straddles the boundary.** Record it as a design decision and handle it together with the introduction of authz (Q-41).

### D-51. More hops land on the token path
`TOKEN` is produced for every token. If the last node is on an agent other than the entry agent, every token gets one extra relay hop: **entry agent → controller → OUTER**. This directly affects the aggregate TPS target.

Now that node placement is free (premise 11 correction), this remains an orchestration optimization issue — putting the last node on the entry agent bypasses it.

## 25. Remediation design (draft)

### P-34. Messages self-describe their target address (decided)

The message standard must carry not only the target **object** but also **the information needed to reach the agent that the object belongs to**. The envelope self-describes enough to connect: URL, host, port and so on.

This is a **prerequisite** of P-35. If the relay decision had to consult a lookup table, the worker loop would hold state, breaking premise 2 ("state passed as arguments lives outside") and premise 9 (workers only do their own job and queue messages) at the same time. **Relaying is stateless only with self-describing addresses.**

It applies in three places, with one unified notation.

| Place | Contents |
|---|---|
| Envelope target address | Connection info of the agent this message must finally reach |
| Chain entry (P-25) | `(agent connection info, node_id, binding_id, runtime_generation)` |
| Return address (P-26) | Connection info of the entry agent |

Resolves D-47 and D-48. It replaces both the static map of `RouteProcessor` and the fixed endpoint of `TcpTransport`. Q-25 and Q-35 are closed by this decision.

**This is a frame-layer change.** The current routed envelope carries only `route_id` and `deadline`, so there is no place for a target address. It belongs to P4B1 v6 and is handled in the same round as Q-3 (announce).

### P-35. Build relaying into queue dispatch as a default behavior

**It is not a handler-layer feature.** It must be a decision that the basic logic taking messages off the queue makes by default for every message, so that higher-level code need not be aware of relaying.

It sits **just before** the handler call in the worker loop — the point where the `spawn` loop in [`task_queue/worker/mod.rs`](layers/runtime/src/foundation/task_queue/worker/mod.rs) takes out an envelope and passes it to `handler.handle`.

| Decision | Action |
|---|---|
| Target is self | Pass to the handler — current behavior |
| Target is another agent | Queue it for the next hop **without going through the handler** |

The decision is based on the envelope's target address (P-34). `TaskEnvelope` already has `source`/`target` Participants and `is_local_bypass()`, so this places the symmetric concept in the same spot — **if bypass is the decision that "folds inward", relaying is the decision that "pushes outward"**, and the two are two sides of the same condition.

This placement has three advantages.
- Handlers know only their own messages. No relay branch appears in `AgentProcessor`
- When a new message kind is added, relaying follows automatically. No per-kind relay rules need to be written
- **It fits premise 9 naturally.** Relaying is the simplest form of "do your own job and put messages on the queue". No return value, no blocking

When relaying, the agent does not interpret the content — premise 3's "relaying is not owning" applies to agents as well as to the controller. So "entry agent" is not a separate role; it is just a name for **an agent in a position the controller can reach**, and `ParticipantRole` need not grow (Q-35). Topology is a result of deployment, not a protocol type.

### P-36. Same-agent bypass
If the target is self, the relay hop is skipped. The decision is **agent identity**, the same rule `TaskEnvelope::is_local_bypass` already uses — `source.agent_id == target.agent_id`.

It reuses the existing in-process bypass concept, so no new decision logic is needed. It is the same condition as P-35's two-way decision.

### P-37. Pin down the controller's relay responsibility in the contract
For messages the controller relays, **interpretation, validation and holding state are forbidden**. The rule for what the controller does in inference (does not interpret the chain, passes it through) is applied to the control path as well.

This does not conflict with P-6 (removing `controller_id` as an ownership gate). The controller appears as **a relay address**, not as an owner.

---

# Topic I. Expressiveness of inference requests

Premise 12. The sampling and decoding spec falls short of what the concrete runtime offers.

## 26. Current status (verified)

### 26.1 What P4 carries

`ExecutionRequest` has four generation-related fields: `max_tokens` (u32), `temperature` (f32), `prompt` (text) and `options` (opaque JSON). Among the sampling parameters, **only `temperature` has been promoted to a wire field**.

### 26.2 What the adapters actually do — the exact opposite

| Adapter | Behavior |
|---|---|
| stock llama.cpp | **Passes everything through.** Protects only `model`/`messages`/`stream` and forwards the remaining options as is — [`llamacpp/.../options/mod.rs`](layers/adapters/llamacpp/src/application/options/mod.rs) |
| **Pipeline** | **Whitelists only 5.** Anything other than `["max_tokens", "temperature", "top_p", "top_k", "seed"]` is **silently dropped** — [`adapter/.../execution/options/mod.rs`](layers/adapters/adapter/src/application/execution/options/mod.rs) |

Both adapters **hard-code** the prompt as `messages: [{"role":"user","content": prompt}]`.

### 26.3 Actual upstream surface

In the pinned upstream `common/common.h`, `common_params_sampling` has about 35 fields plus a sampler order array, grammar, logit_bias and the reasoning budget.

```text
seed n_prev n_probs min_keep top_k top_p min_p xtc_probability xtc_threshold
typ_p temp dynatemp_range dynatemp_exponent penalty_last_n penalty_repeat
penalty_freq penalty_present dry_multiplier dry_base dry_allowed_length
dry_penalty_last_n dry_sequence_breakers adaptive_target adaptive_decay
mirostat mirostat_tau mirostat_eta top_n_sigma ignore_eos timing_per_token
samplers[] grammar grammar_lazy grammar_triggers preserved_tokens
logit_bias[] logit_bias_eog reasoning_budget_* backend_sampling
```

Of these, the Pipeline whitelist covers **four**: `seed`, `top_k`, `top_p` and `temp`.

## 27. Defects

### D-55. The Pipeline adapter silently drops sampling options
Every key outside the 5 in `SUPPORTED` disappears with neither a warning nor an error. The caller sends `min_p` or `repeat_penalty` and believes it was applied. **This is a head-on violation of premise 12's "no silent omission"**, and it also contradicts the rule that `docs/model-load.md` declares for load options.

### D-56. Structured output is impossible in principle on the Pipeline path
Structured output works through **logit filtering** in the grammar sampler. It cannot be replaced by parsing after generation — the method constrains the token distribution so that the model emits only that format in the first place.

`grammar`, `grammar_lazy`, `grammar_triggers`, `json_schema` and `preserved_tokens` are all outside the whitelist, so **there is no way to turn on structured output on the Pipeline path.** This is not a weak feature; it is a missing one.

For the same reason, `logit_bias`, `ignore_eos` and `reasoning_budget_*` cannot be handled in post-processing either, and all of them are missing.

### D-57. The same `options` field is handled in opposite ways by different adapters
stock passes everything; Pipeline passes only 5. The caller cannot know which applies, and the protocol has no place to express the difference. The same structural problem as `D-17` in topic C (ignoring vs refusing load options) exists on the inference side too.

### D-58. The prompt is a single string and cannot express conversation structure
`prompt: String` is hard-coded as `[{"role":"user","content": prompt}]` in both adapters. **There is no way to send a system prompt**, and neither multi-turn messages, assistant prefill nor multimodal parts can be expressed.

`session_id` exists, but it is only a session identifier, not a means of passing conversation history.

### D-59. The criterion for promoting wire fields is arbitrary and duplicated
Of the sampling parameters, only `temperature` is a wire field. `top_p`, `top_k` and `min_p` live in the option string, and there is no rationale for promoting only `temperature`. Moreover, both adapters must merge same-named keys in `options` with the wire fields, so **the precedence rule differs per adapter** — stock uses `or_insert_with` (options win), Pipeline inserts later (wire wins).

## 28. Remediation design (draft)

### P-40. Unify inference options as opaque pass-through (premise 12)
The same rule as for load options. P4 does not interpret `options`, and the adapter forwards them as is to the concrete runtime. The stock llama.cpp adapter's current behavior is already correct, so **align the Pipeline adapter with it.**

Without the whitelist, neither P4 nor the adapter has to change when upstream adds a sampler.

### P-41. Forbid silent omission
When an adapter meets a key it cannot interpret, it terminates with `ERROR` instead of dropping the key. This is the enforcement clause of premise 12 and sits on the same decision axis as P-10 in topic C (a means of discovery in advance) — whether adapters declare their supported keys is decided together with Q-9.

### P-42. Turn the prompt into a conversation structure
Replace the single string with a message array. It must be able to express system/user/assistant roles, multiple turns and multimodal parts. Resolves D-58.

Per premise 12, P4 does not need to interpret this structure — **it only needs to be expressible.** Carrying it in the option string is therefore also a valid option (Q-45).

### P-43. Clean up the wire fields
Remove the double structure in which `temperature` and `max_tokens` are wire fields and also exist as same-named keys in the options. There are two options, and either way the goal is **for the precedence rule to disappear** (Q-44).
- Move all of them down into the options — P4 knows no generation parameters at all
- Move all of them up to the wire — contradicts premise 12, so not adopted

---

# Topic J. Adapter boundary

Target layering: `P4 adapter interface ← concrete adapter ← concrete backend`. **P4 must not know about llama.cpp.**

## 29. Current status (verified)

### 29.1 What is being upheld

**The dependency direction is correct.**

```text
p4-protocol   (0 dependencies)
     ▲   ▲   ▲
     │   │   └── p4-llamacpp
     │   └────── p4-adapter
     └────────── p4-runtime
```

There are no back-references. The whole of `layers/protocol/src` contains **0** occurrences of the strings `llama|gguf|ggml|cuda|vulkan|metal|rocm|nvidia`.

`adapter_kind` is merely self-declared by the adapter as `"pipeline"`/`"llamacpp"`, and protocol does not interpret the value. `stage_plan`, `node_spec`, `descriptor` and `options` pass through as bounded text.

### 29.2 Leak points

| Location | Contents |
|---|---|
| `contract/message/mod.rs` | `kv_bytes`·`layer_bytes`·`ffn_bytes` in `DraftReport` |
| `contract/execution/mod.rs` | `temperature`·`max_tokens` (D-59), `text` |
| `contract/phase/mod.rs` | `Prefill`/`Decode` |
| `docs/model-load.md` | Codifies llama.cpp knobs as the formal schema |
| `domain/hardware/mod.rs` | `nvidia-smi` (D-3) |

## 30. Defects

### D-62. Transformer internals are in the protocol contract
`kv_bytes`, `layer_bytes` and `ffn_bytes` in `DRAFT_REPORT` mean that P4 knows that **"a model consists of a KV cache and FFN blocks"**. This is not llama.cpp-specific, but it is not something the adapter interface should know either, and it loses its meaning on backends with a different structure.

### D-63. The docs leak more than the code
[`docs/model-load.md`](docs/model-load.md) explicitly codifies `flash_attention`, `mmap`, `kv_cache.type_k/type_v/offload`, `context_batch_tokens` and `context_ubatch_tokens` as the **formal schema** of `load_options`. All of them are llama.cpp knobs.

The code passes them through opaquely, yet **the docs declare them a contract of the P4 layer.** Now that premises 6 and 12 are settled, that document must be an example, not a contract.

### D-64. The adapter interface does not exist as an explicit artifact
The contract adapters must implement is not a separate artifact but **the P4 message contract itself**. That design is legitimate in itself, but then **the entire burden of backend neutrality falls on the message contract.** D-62, D-63 and D-59 are evidence that it is not bearing that burden.

### D-70. The contract does not distinguish self-contained nodes from stage nodes

This came out of checking whether vLLM could be adopted.

| Kind | Nature | Chain length | Examples |
|---|---|---|---|
| **Self-contained** | Serves the whole model by itself. Internal TP/PP is its own business | 1 | vLLM, stock llama-server |
| **Stage** | Handles only a layer range and exchanges hidden state | n | Our Pipeline runtime |

The chain design of topics F and G **assumes stage nodes** — we own the stage boundaries, pass hidden state between nodes and run the decode loop from outside. vLLM does pipeline parallelism internally (Ray/NCCL), so its PP stages are not addressed as P4 nodes.

Because the contract lacks this distinction, **it does not state whether chain length 1 is a valid configuration.** If it is stated, self-contained backends fit in naturally; if not, the chain design implicitly assumes the shape of one specific backend.

**Adopting vLLM itself is possible.** The llamacpp adapter's entire inference path is `POST /v1/chat/completions` + SSE, i.e. a single OpenAI-compatible API, which vLLM supports as is. Loading also happens at process start, so the same constraints as for stock llama-server apply. What gets in the way is `D-62` (the required KV/FFN breakdown), `D-58` (single-string prompt) and the lack of a counterpart to `session_id` — **none of these are caused by vLLM; they are existing defects coming to light**.

### D-65. A concrete adapter occupies the interface's name
The crate `p4-adapter` (`layers/adapters/adapter/`) is not the interface but **the Pipeline concrete adapter** — `linker-pipeline-inference-stream-v1`, `/api/runtime-groups`, "Pipeline binding" and so on are baked into it.

[`layers/adapters/README.md`](layers/adapters/README.md) itself calls this directory **`pipeline/`.** The intended name survives in the docs; only the actual directory is `adapter/`.

## 31. Remediation design (draft)

### P-45. Make `DRAFT_REPORT` a structure-neutral report
Do not fix the byte items to the transformer structure. Change the report to a total plus **adapter-defined categories**, with the adapter filling in the category names and values. Same direction as premises 6 and 12 — P4 interprets neither instructions nor reports, but reports are structured (see the P-30 discussion).

### P-46. Demote `docs/model-load.md`
Downgrade it from the formal schema to **an example**. The contract is "options are opaque strings interpreted by the adapter" (premise 6), and concrete key lists move to per-adapter docs.

### P-52. Distinguish node kinds and explicitly validate chain length 1

The fix for D-70.

1. **State in the contract that chain length 1 is a valid configuration.** A self-contained node is a chain of length 1; in that case `Node[0]` is also the last node, so topic G's entry/completion reports and return path hold as they are
2. **The adapter declares the node kind.** `ADAPTER_REGISTER.descriptor` states whether it can take part in stages. This is the same place as P-10 (declaring supported options); P4 does not interpret the value, and OUTER uses it for orchestration
3. **OUTER's orchestration rule:** a self-contained node cannot form a chain with other nodes. OUTER makes this judgement (premise 10, P-33), and the agent only reports failure for an instruction that violates it

With this, self-contained backends such as vLLM, TGI and SGLang take part **without changing the chain design**. Stage chains apply only to backends whose boundaries we own.

### P-47. Name correction
`layers/adapters/adapter/` → `layers/adapters/pipeline/`, crate `p4-adapter` → `p4-pipeline`. The layer README already uses that name. This frees up the interface's place.

**It can go into phase 0.** No wire change and no dependency on other items.

---

# Topic K. Message dispatch layers

Target layering. Each level is **more generic toward the outside and more concrete toward the inside**.

```text
Agent [generic message queue management]
  └─ toss to another agent / bypass decision
      └─ generic worker handling
          └─ deliver to the relevant controller or node
              └─ (node) deliver to the node adapter bound to that node
                  └─ concrete node adapter → concrete inference object
```

**Per-kind interpretation must happen only in the last two levels.** The first four levels act on the envelope alone.

## 32. Current status (verified)

| Target level | Current |
|---|---|
| Generic message queue management | `TaskQueue` exists but **knows the P4 `Message`** — `TaskEnvelope` computes the classification with `queue: message.queue_class()` |
| Toss / bypass decision | **Missing.** Only the bypass decision (`is_local_bypass`) exists; there is no toss (D-49) |
| Generic worker handling | Workers are generic but hand off to kind branching right away |
| Delivery to the controller/node | **Missing.** The participant delivery layer is empty, and the agent goes straight to the adapter transport |
| Node → node adapter | `NodeSlot` is passive data. It is not a delivering party and only holds an `adapter_id` string; `AgentProcessor` does the lookup and delivery instead |
| Concrete adapter → concrete object | Works correctly |

## 33. Defects

### D-66. Kind branching is scattered and duplicated across layers
`match task.message` in [`dispatch/mod.rs:104`](layers/runtime/src/application/dispatch/mod.rs) and `match message` in [`agent/mod.rs:104`](layers/runtime/src/domain/agent/mod.rs) **decompose the same message twice.** In the target layering, kind interpretation is the job of the last two levels, yet two upper places already know it.

### D-67. There is no participant delivery layer
The level corresponding to "deliver to the relevant controller or node" does not exist. `AgentProcessor` performs registry lookup, authorization and transport delivery all at once inside per-kind handlers, and **skips the node, going straight to the adapter transport.**

As a result, `NodeSlot` becomes passive data instead of a delivering party. The layer "a node delivers to its own adapter" does not exist in the code.

### D-68. Even relaying requires decoding the full payload
`read_routed_message` **fully decodes the body** with `decode_payload` as soon as it reads the envelope. Even messages that are only to be handed on must have their content interpreted.

The second level of the target layering (the toss decision) only needs the envelope, and only then does "relaying is not owning" (premise 3) hold at the implementation level as well. Adding relaying to the current structure would **make relay nodes interpret every message that belongs to someone else.**

### D-69. The queue is tied to the P4 message type
`TaskEnvelope::new_routed` calls `message.queue_class()` to choose the lane — [`task/mod.rs:116`](layers/protocol/src/task/mod.rs). For the queue to be generic, **the classification must arrive in the envelope**, and the queue must only read that value.

## 34. Remediation design (draft)

### P-48. Separate envelope decoding from body decoding
On frame receipt, **parse only the envelope (route, target address, classification, deadline) first** and defer the body. Decode the body only when the target is self.

D-68 and D-69 are resolved together. Relay cost drops (mitigating D-51 in §92), and the queue no longer needs to know the message type.

It is a natural extension of premises 6 and 12 (opaque pass-through of options) — **on the relay path, the whole message is opaque.**

### P-49. Establish the participant delivery layer
Make "deliver to the controller or node" an explicit level. The agent picks the participant from the envelope's target, and **the node is responsible for delivering to its own adapter.** `NodeSlot` is promoted from data to a delivering party.

Resolves D-67 and fits naturally with P-15 (state machine) — state decisions and delivery live on the same object.

### P-50. Push kind branching down into the last two levels
Remove the double branching in `dispatch` and `AgentProcessor`. Upper layers route by envelope only, and kind interpretation happens past the node adapter boundary.

**The exception is what the agent owns** — messages that deal with the NodeSlot itself, such as `NODE_CREATE`/`NODE_DELETE`, are interpreted by the agent (the second layer of P-5). Drawing that boundary explicitly is the real work of this item.

### P-51. The envelope carries the queue classification
`queue_class` is not computed from the message; the sender puts it in the envelope. The queue only reads the value to pick a lane and does not know the message. Resolves D-69; prerequisite of P-48.

---

# Topic L. Backend ownership and upstream tracking

Goal: **each concrete adapter owns its backend.** Upstream can always be pulled at its latest version, and we compile it with only the features we need attached. The same rule applies to every concrete adapter — not only llama.cpp but also vLLM and others.

## 35. Current status (verified)

### 35.1 What already has the target shape

For llama.cpp, **the requested structure is already implemented.**

| Item | Current |
|---|---|
| upstream | `apps/llama/upstream` — **a pristine submodule**. `.gitmodules` points to the official `ggml-org/llama.cpp` |
| Patches | `apps/llama/native/compat/<upstream-sha>/` — ordered patch sets. Four SHAs' worth are maintained |
| Verification | `manifest.json` — upstream commit, date, subject, previous pin, per-patch SHA256, patch-set hash, applied tree hash |
| Application | `scripts/prepare-pipeline-upstream.mjs` creates a worktree in the ignored `.cache/`, verifies hashes and applies the patches in order. **The output is not committed** |
| Build split | The stock build compiles the pristine submodule directly. **Only the Pipeline build uses the patches** |
| Header boundary | Our C++ includes only public headers — `llama.h`, `ggml-backend.h`, `ggml-cuda.h` |
| Procedure | The README has a 6-step update procedure and states "do not edit the official submodule to make the build pass" |

So the requirement "always pull the latest and compile with only the needed features attached" **already holds by design.** What remains is where ownership sits and what tracking costs.

### 35.2 Patches by nature

Base commit `3e3a7a416`, 14 patches, 2,219 lines.

| Nature | Patches | Lines | Share | Rebase cost |
|---|---|---:|---:|---|
| **upstream defects** | `0001-ggml-backend`, `0002-ggml-rpc` | 138 | 6% | **Disappears permanently** once contributed upstream |
| **ABI exposure** | `0003-public-pipeline-abi`, `0005`, `0007`, `0008`, `0010`, `0012` | 484 | 22% | Low — mostly headers |
| **Internal modification** | `0004-llama-context`(646), `0006-llama-graph`(641), `0009`, `0011`, `0013`, `0014` | 1,597 | 72% | High |

**`0004` and `0006` together are 1,287 lines, 58% of the total.** The cost of tracking the latest upstream effectively sits in these two files.

`0001` fixes the spot that upstream itself marked with `// FIXME: count the number of inputs instead of only checking when full`.

## 36. Defects

### D-71. Backend ownership sits outside the adapter
P4's Pipeline adapter is in `layers/adapters/adapter`, but the backend it drives (upstream + patches + prep scripts + host supervisor) **lives under `apps/llama`.** The two are connected over HTTP — `/api/runtime-groups`, `linker-pipeline-inference-stream-v1`.

With the adapter and its backend scattered across different apps, **"each concrete adapter owns its backend" does not hold.** Adding or replacing an adapter means touching two places at once.

### D-72. The tracking cost is concentrated in two patches
`0004-llama-context` (646 lines) and `0006-llama-graph` (641 lines) are 58% of the total patch volume. On every upstream bump, resolving conflicts in these two takes most of the work. Step 3 of the README update procedure ("porting") is effectively a rebase of these two files.

### D-73. Fixes for upstream defects live on as our patches
`0001` fixes an upstream `FIXME` spot, and `0002` is a missing `<chrono>` include. **Neither is a feature of ours.** Unless they are contributed upstream, they become a cost that follows every upstream bump forever.

### D-74. The upstream policy for self-contained vs stage backends is not written down
Patches are needed because of **partial loading**, and partial loading is **a requirement of stage nodes only** (D-70).

| Node kind | upstream modification | Current |
|---|---|---|
| Self-contained | **Not needed.** Only binary/package dependencies | `layers/adapters/llamacpp` already has this shape — it attaches to stock llama-server and uses no patches |
| Stage | **Needed.** A compat layer is required | Pipeline adapter |

Because this distinction is not explicit in the structure, there is no basis for judging whether a new backend needs patches. **vLLM is self-contained, so it needs zero upstream modification and a pip pin is enough** — unless this is written down, needless fork reviews keep recurring.

## 37. Remediation design (draft)

### P-53. Move backends under their adapters

```text
adapters/
  llamacpp/          self-contained — no upstream modification
    src/             P4 adapter. Connects to stock llama-server through the OpenAI-compatible API
  pipeline/          stage — modification needed
    upstream/        pristine submodule
    compat/<sha>/    ordered patches + manifest
    scripts/         .cache worktree preparation, hash verification
    src/             P4 adapter
  vllm/              self-contained — pip pin
    src/
```

**This is a move, not new construction.** The upstream, compat and scripts of `apps/llama` move as they are under `adapters/pipeline/`. This resolves D-71 and belongs to the same work as `P-47` (name correction).

Once the move is done, P4's dependency on `apps/llama` disappears. What remains is the HTTP boundary with the host supervisor, and since that also becomes adapter-owned, it turns into the adapter's internal structure rather than a cross-app dependency.

### P-54. Split the upstream policy by node kind

Write it into the contract.

- **Self-contained adapters do not modify upstream.** They depend only on official distributions (binaries, packages) and have no `compat/` layer
- **Only stage adapters have a `compat/` layer**, and only where partial loading is needed
- When adopting a new backend, **first judge whether it can work as self-contained**, and consider a stage adapter only if it cannot

Resolves D-74. This is the structural counterpart of `P-52` (node kind distinction).

### P-55. Contribute the upstream defect patches so they disappear
Submit `0001-ggml-backend` (FIXME fix) and `0002-ggml-rpc` (missing include) as PRs to the official project. Once accepted, they leave the patch set for good — 138 lines and 2 rebase targets disappear.

They are not features of ours, so nothing stands in the way of contributing them. Resolves D-73.

### P-56. Convert internal modifications into ABI exposure (needs review)
The root fix for D-72. From the 1,287 lines of `0004` and `0006`, **pull the logic into our code and leave only hooks in upstream.**

`0003-public-pipeline-abi.patch` (197 lines) already looks like an attempt in that direction. If this is pushed to the extreme so that internal modifications converge into ABI exposure, the tracking cost drops to **the level of a header rebase**.

The convertible scope can only be judged by reading the actual patch contents (Q-56). Even if not all of it can be converted, **just reducing the share has a large effect** — the 58% is the bottleneck.

---

# Topic M. Repository end state

This work happens on **a separate branch**. The exit condition is **"no concrete adapter that agents use exists outside `apps/p4`"**, and `apps/llama` hands over all execution knowledge and **survives as a planning knowledge provider** (P-57·P-60).

The end state was originally set as "only `apps/linker` and `apps/p4`", but once it was decided not to put planning knowledge into the OUTER core (D-77 resolved), a third app remains. The original goal, **P4 owning its backends**, is still achieved.

## 38. Role mapping

The roles that the plan describes abstractly map to actual artifacts as follows.

| Role (§1.2) | Artifact | Rationale |
|---|---|---|
| **OUTER** | `apps/linker` + `packages/linker_domain` | Topology, identity, ownership, catalog and plan state already live here. §1.3's identifier issuance and P-33's orchestration ownership are exactly this app's duties |
| **Controller** | `apps/p4/entrypoints/controller` | Already exists |
| **Agent** | `apps/p4/entrypoints/agent` | Already exists |
| **Node** | `apps/p4/entrypoints/node` | Already exists |
| **Concrete adapters** | `apps/p4/adapters/*` | Targets of the P-53 move |

**Confirming that OUTER is `apps/linker` settles where everything in the plan belongs.** Hardware capability records, node orchestration, load plans, chain composition and identifier issuance all go to `packages/linker_domain` (currently 3,041 lines).

## 39. Current status (verified)

The size of what is being dismantled.

| Component | Size | Nature |
|---|---|---|
| `apps/llama/native/` | 134 files | upstream submodule, `compat/<sha>/` patches, `linker-node`, `linker-expert-worker`, `linker-device-probe`, `linker-moe-verify`, `linker-arch-fixtures` |
| `apps/llama/src/` | 56 files | Web UI + host supervisor (18082, `/api/runtime-groups`, `linker-pipeline-inference-stream-v1`) |
| `apps/llama/scripts/` | 28 files | Build and preparation, such as `prepare-pipeline-upstream.mjs` |
| `packages/llama_domain` | **11,631 lines** (common 6,113 / server 3,493 / front 1) | GGUF inspection, placement heuristics, runtime verification, Pipeline startup policy |

**`packages/llama_domain` is the largest asset in the repository**, about 4 times the size of `linker_domain` (3,041 lines).

## 40. Defects

### D-75. There is no disposition plan for `apps/llama`
`P-53` covers only moving upstream, compat and scripts; it does not cover the web UI, the supervisor or `llama_domain`. **Execution knowledge and planning knowledge are mixed in one app**, so there was no criterion for what to hand over and what to keep. P-60 sets that criterion.

### D-76. `packages/llama_domain` holds the concerns of three parties in one package
By the plan's ownership rules, the 11,631 lines split three ways.

| Contents | Belongs to | Rationale |
|---|---|---|
| Placement heuristics | **OUTER** | P-33 — orchestration is owned by OUTER alone |
| GGUF inspection | **OUTER** (or shared) | Building a load plan requires knowing the model structure |
| Runtime verification | **Adapter** | Premise 10 — judging the instance is the concrete adapter's job |
| Pipeline startup policy | **Pipeline adapter** | Backend-specific |

Right now these four sit in one package, so when `apps/llama` is dismantled there is no single place for the package to go as a whole.

### D-77. Whether OUTER must know model formats is undecided (resolution path fixed)
If D-76's GGUF inspection goes to `linker_domain`, **the OUTER core knows GGUF.** GGUF is a llama.cpp format; vLLM uses safetensors/HF. As self-contained backends multiply, the OUTER core would need an inspector for each format.

This is the OUTER-side version of `D-64` (where the abstraction burden falls). Delegating to the adapter does not work — **orchestration happens before loading**, so at that point the adapter does not have the model.

What planning knowledge consists of has been revealed by measurement — per-role stage cost, KV per layer/session (~188 MiB), VRAM caps, and the interaction between overlap depth and layer split (§92). **This is the content of the 3,182-line `planner`, and it is what OUTER must own.**

**Resolution: do not put planning knowledge into the OUTER core; keep it in per-backend modules (P-60).** OUTER does not know formats; it **consumes modules that know formats**. `linker_domain` stays format-agnostic.

### D-83. The model files that planning knowledge must read are inside the firewall
`readPlannerModel` reads GGUF **files** directly (with a `LLAMA_MODEL_DIR` boundary check). But models live where nodes can load them, that is, **inside the firewall** (`HOST_MODELS_DIR`).

Today `apps/llama` is a host app, so local access works, but once P-57 turns it into an OUTER-side planning app, **it cannot see the files.**

This knocks out one of `D-77`'s arguments. It said "orchestration happens before loading, so the adapter does not have the model", but **model files exist on the host even before loading.** Inspecting files and loading them at runtime are different things, and an agent can read files without loading them.

`P-60` (keeping the OUTER core format-agnostic) still holds on other grounds, but **who runs the inspection** is reopened (Q-70).

| Option | Contents | Cost |
|---|---|---|
| OUTER reads | Keep a model catalog outside the firewall as well | Duplicate placement of model files, or a separate catalog sync |
| Agent reads | Request file inspection through P4 and have the planning module interpret the result | A new inspection request message. Format knowledge is still owned by the planning module |

The latter is consistent with both premise 11 (firewall) and P-60 (format-agnostic core) — **the agent reads bytes, and the planning module interprets them.** The agent need not know GGUF.

### D-78. The repository contract document describes the current structure
The root `CLAUDE.md` spells out the four-part structure of `apps/linker`, `apps/llama`, `packages/linker_domain` and `packages/llama_domain` and their fixed pairings, and prescribes ports, Docker and host supervisor placement. In the end state, all of that will contradict the facts.

## 41. Remediation design (draft)

### P-60. Split by firewall position (decided)

Split not by concern but by **"which side of the firewall needs it"**. This criterion meshes directly with premise 11's topology.

| Knowledge | Location | Ownership | Contents |
|---|---|---|---|
| **Planning knowledge** | Outside the firewall | OUTER **consumes** it | Model format inspection, placement heuristics, capacity estimation, model availability |
| **Execution knowledge** | Inside the firewall | Adapters **own** it | Runtime startup, loading, inference, stage control |

**Planning knowledge does not go into `linker_domain`.** The OUTER core stays format-agnostic, and OUTER consumes per-backend planning modules. This resolves D-77, and the OUTER core does not bloat as backends multiply.

This criterion already matches the seams in the existing code.

| Kind | `llama_domain/common` | `apps/llama/src/server` routes |
|---|---|---|
| Planning | `planner` **3,182 lines** (52% of common) | `/api/models/inspect`, `/api/models/availability`, `/api/plans`, `/api/resources` |
| Execution | `protocol` 1,864 + `pipeline-*` 976 | `/api/processes`, `/api/runtime`, `/api/runtime-groups`, `/api/rpc-runtime-groups` |

### P-57. Disposition of `apps/llama` — it survives as a planning knowledge provider

**Do not dismantle it.** Strip out only the execution knowledge and keep it as a planning knowledge provider.

| Component | Disposition |
|---|---|
| `native/upstream`, `native/compat`, `scripts/prepare-*` | → `apps/p4/adapters/pipeline/` (P-53) |
| `native/linker-node` and the other native code | → `apps/p4/adapters/pipeline/native/` |
| Host supervisor (`/api/processes`, `/api/runtime*`) | → `apps/p4/adapters/pipeline/` — becomes an adapter-internal boundary |
| `/api/models/inspect`, `/api/plans` and their UI | **Survive** — the planning surface OUTER consumes |
| `/api/resources` | **Replaced.** Hardware capability goes through the P4 path (below) |
| `backend-contract.json`, `docs/` | Distributed according to the dispositions above |

**What survives is pure TypeScript.** It was verified that planning knowledge does not call native code — `readPlannerModel` parses GGUF directly in TS, and the 3,182-line `planner` has 0 occurrences of `spawn`, `exec` or `child_process`.

**Correction:** `/api/resources` was originally on the survival list, which was wrong. Its source, `host-resources/cuda-driver-probe.ts`, consumes the C++ `linker-device-probe`, and topic A already made hardware capability reporting **the agent's job** (P-1·D-3). Replacing it with P4's capability path **removes the last native dependency of what survives.**

So under this plan, **C++ and llama.cpp are removed from `apps/llama` completely.**

**The end state has three apps, not two.** Still, the original goal, "P4 owns its backends", is achieved as is — **no concrete adapter that agents use remains** in `apps/llama`.

The mismatch between the name and what the app actually is remains. What survives is not a runtime app but a llama.cpp/GGUF **planning provider** (Q-64).

### P-58. Split `packages/llama_domain`
Split it by P-60's criterion.
- Planning knowledge such as `planner` (3,182) → stays with the surviving part of `apps/llama`. **Do not move it to `linker_domain`**
- Execution knowledge such as `protocol` (1,864), `pipeline-*` (976) and `server` (3,493) → `apps/p4/adapters/pipeline/`

It is a split of 11,631 lines, but P-60's dividing line largely matches the existing directory boundaries, so the cut is cleaner than first expected (Q-61).

### P-59. Update the repository contract document
Align the architecture, ports, Docker and app↔package pairing rules in the root `CLAUDE.md` with the end state. **Apply this at branch merge time** — changing it earlier would make it stop describing the current tree.

---

# Topic N. Multi-platform support and backend variants

Agents run on GB10 Linux, Ubuntu (x86/arm), macOS and Windows. And llama.cpp not only differs per OS but is built separately per GPU as **CUDA, ROCm/HIP, Metal, OpenCL, Vulkan and CPU**. **A node's concrete instance is not a single thing but a matrix.**

## 42. Current status (verified)

### 42.1 The build side already handles the matrix

`apps/llama/scripts` (28 files) and `cmake/LinkerProxy.cmake` handle the variants. Backend string frequencies: `opencl` 35, `rocm` 29, `cuda` 28, `cpu` 21, `vulkan` 11, `metal` 10, `hip` 5.

| Asset | Role |
|---|---|
| `build-node-runtime.{ps1,sh}`, `build.mjs` | Per-platform builds |
| `package-pipeline-runtime.py`, `windows-runtime-pack.psm1`, `write-runtime-pack-manifest.mjs` | **Runtime pack** packaging and manifest |
| `deploy-node-runtime.{ps1,sh}`, `host-service` | Deployment and per-OS service registration |
| `native/linker-device-probe` | Device detection |
| `cmake/LinkerProxy.cmake` | Enforces `LINKER_LLAMA_COMPAT_ID` and stamps the build identity |

The compat README already requires "fleet promotion after verifying real load/chat/unload on CUDA, Metal and OpenCL".

### 42.2 The protocol side does not know the matrix

`HARDWARE_REPORT.snapshot` holds `os` (a compile-time constant), `arch`, CPU core counts, `nvidia-smi` GPU strings, adapters and nodes.

**It does not say which backend the runtimes on this host were built for.** `adapter_kind` is also only `"pipeline"`/`"llamacpp"`, so it cannot distinguish variants.

## 43. Defects

### D-79. The capability report has no runtime build variant
For OUTER to decide placement, it must know **which backend the runtimes on that host were built with**. Metal hosts and CUDA hosts differ in supported quantization, flash attention availability and KV types.

In topic A's classification this belongs to **the capability class** — it changes only on rebuild or redeploy, so it can be stored in the external record. Yet P-1's capability list lacks this item.

### D-80. `adapter_kind` cannot distinguish build variants
`"pipeline"` describes the protocol shape, not the execution capability. A Pipeline adapter built with CUDA and one built with Metal have the same `adapter_kind` but **accept different load options.**

`P-10` (declaring supported options) and `P-52` (declaring the node kind) already chose `descriptor`, so the place exists, but the variant axis is not specified.

### D-81. The patch verification matrix multiplies the tracking cost
`D-72` said the rebase cost sits in the two patches `0004` and `0006`, but **that cost is multiplied by the number of platforms.** A patch applying is not the same as building and working on every target. The current requirement is three (CUDA, Metal, OpenCL); adding ROCm, Vulkan and arm raises the cost of each upstream update accordingly.

### D-82. Platform availability of self-contained backends is an orchestration constraint, but it is not in the contract
`P-54` split self-contained from stage, but **self-contained is not possible on every platform.** vLLM centers on CUDA/ROCm and has no Metal or native Windows path. In other words, **only llama.cpp-family backends are possible on Mac hosts.**

Node kind × platform × backend forms an availability matrix, which is an input to OUTER's orchestration, but there is no place to express it.

## 44. Remediation design (draft)

### P-61. Put runtime variants into capability

Add the following to `P-1`'s capability items.

```text
runtime_variants: [
  { adapter_kind, backend, arch, os, build_id, compat_id }
]
```

- `backend` — `cuda` / `rocm` / `metal` / `opencl` / `vulkan` / `cpu`
- `compat_id` — which upstream compat set it was built with (the value that `LINKER_LLAMA_COMPAT_ID` already enforces)
- `build_id` — the immutable artifact identity that cmake stamps

It is in the capability class, so it changes only on rebuild or redeploy. **It is a declaration, not an observation (occupancy).**

### P-62. `descriptor` declares the build variant
When an adapter registers itself, `ADAPTER_REGISTER.descriptor` carries the variant. It is the same place as `P-10` (supported option set) and `P-52` (node kind), and **P4 does not interpret the value; OUTER uses it for orchestration** (the same rule as premises 6 and 12).

With this, "this adapter is of the pipeline kind, is a CUDA build and accepts such-and-such load options" is expressed in a single declaration.

### P-63. Make the availability matrix an input to OUTER orchestration
OUTER orchestrates knowing the availability of `node kind × platform × backend`. This judgement is OUTER's alone (P-33), and the agent only reports failure for an instruction that does not fit (Q-33 decision).

It belongs to topic M's **planning knowledge** (P-60) — needed outside the firewall and provided by per-backend modules. So it sits on the planning provider side, not in `linker_domain`.

### P-64. Pin down the support matrix in documentation
State which combinations are officially supported and which are best-effort. The cost in `D-81` is proportional to this scope, so **narrowing the matrix is the direct way to reduce the tracking cost** (Q-66).

---

## 89. Open decisions

| # | Contents | Depends on |
|---|---|---|
| Q-1 | Make `snapshot` a normative versioned JSON schema, or promote it to wire fields? | P-3 assumes the former |
| Q-2 | Split capability and occupancy into separate messages, or keep them as separate sections of one snapshot? | P-1 |
| Q-3 | Add agent-initiated announce? | If adopted, v5 compatibility is given up → clean up the dead surfaces in the same round |
| ~~Q-4~~ | ~~How to derive `machine_id` — OS-derived value vs injected configuration~~ | **(resolved)** `machine_id` itself is unnecessary. The access address is the identity (P-2) |
| Q-5 | Is `NODE_CREATE` on an existing id an idempotent no-op or an update? | D-10. Handling when active bindings exist is at stake |
| Q-6 | Keep `controller_id` in `EXECUTE`, or move controller involvement to the route/session layer? | P-6 |
| Q-7 | Where does `HEALTH_CHECK` belong — external→agent diagnosis, or a controller concern? | P-6 |
| Q-8 | When removing a node that has active bindings or executions, refuse or reclaim by force? | P-7 |
| Q-9 | Should adapters **declare** their supported option set in `descriptor`, or should trial and error be accepted? | P-10. Either way, P4 does not interpret options |
| ~~Q-10~~ | ~~Match option key names to llama.cpp flags, or abstract them into backend-neutral names?~~ | **(withdrawn)** Not a P4 concern per premise 6. A convention shared by adapters and the external planner |
| ~~Q-11~~ | ~~Promote artifacts to `MODEL_LOAD` fields?~~ | **(withdrawn)** Decided to carry them in the option string. No wire change |
| ~~Q-12~~ | ~~Should the partial-loading unit be a layer range, or also allow tensor patterns?~~ | **(withdrawn)** Within the adapter's interpretation scope. Not a P4 concern |
| ~~Q-13~~ | ~~Where are load options validated?~~ | **(decided)** In the concrete adapter. Premise 6 |
| Q-14 | How to recover the progress and completion of a load whose request route dropped — a dedicated query, inclusion in the node state query, or announce? | D-20, P-13 |
| Q-15 | Narrow node:binding to 1:1, or keep 1:N and define the state checks differently? | P-16. Prerequisite of the P-15 state machine |
| Q-16 | Unify the release unit on `binding`, or raise it to `deployment`? | D-25. Aligns automatically if Q-15 settles on 1:1 |
| Q-17 | How to redesign `forward::capture` — forward after the agent's judgement, or have the agent emit its own terminal instead? | P-14. The former adds delay; the latter loses the adapter detail |
| Q-18 | Make the inside of the adapter process boundary CPS as well, or require CPS only up to the P4 boundary? | D-30. The adapter is a separate process with its own runtime |
| Q-19 | Into how many steps should long-running work be split — should progress polling be a self-re-enqueueing Task? | P-19. The polling interval becomes queue load |
| Q-20 | Remove return values by replacing the foundational traits, or wrap a CPS adapter over the existing traits? | D-26, P-17. The former is a full overhaul; the latter keeps a dual structure alive |
| ~~Q-21~~ | ~~Carry the chain in every request, or reference a pre-registered chain id?~~ | **(decided)** Carry it in every request. The chain is inside the prefill message |
| ~~Q-22~~ | ~~Wrap stage movement in P4, or leave it all to native?~~ | **(decided)** P4 carries the chain, order and correlation, and nodes forward on their own. Hidden state stays native |
| Q-23 | Limit the state the controller holds during a request to the OUTER return route? | §1.2. Chain state has already moved into the message, so only the return path remains |
| ~~Q-25~~ | ~~How to write the agent reachable address in chain entries?~~ | **(decided)** Unified as self-describing connection info. Envelope, chain and return address use the same notation (P-34) |
| Q-26 | Carry the whole chain in chain entries, or pass on only the remaining segment? | P-22. The former helps observation and retries; the latter keeps frames small |
| Q-27 | Make `CANCEL` propagate forward along the chain, or have each node stop on its own per correlation? | P-27, D-40 |
| Q-28 | Who receives the entry report — the node's own agent, the controller, or both? | The descriptions diverged: the agent for node 1, the controller for node 2. Needs to be unified |
| Q-29 | What is the minimum set of explicit fields in the completion report? | P-29. The rest is a string extension |
| Q-30 | On decode hops, do all nodes report entry and completion, or only the last node? | **The load argument is gone** — stage waits are 8.0~87.3 s (§92). Decide on the need for observability alone |
| Q-31 | Where is context overflow judged — in the adapter's entry check, or by OUTER as pre-validation at orchestration time? | D-44. Per premise 10, P4 does not record it |
| ~~Q-33~~ | ~~Accept that a mis-composed chain shows up only during execution?~~ | **(decided)** Accept it. OUTER remembers the complete chain and load state, and if an instruction does not match reality, the agent only reports failure. In return, **agents and nodes are guaranteed simple mechanical behavior** |
| ~~Q-34~~ | ~~Who is the gateway for the load path?~~ | **(resolved)** The entry agent is a pure entry point unrelated to node placement, so every message passes through the same gateway. Closed together with D-50 |
| ~~Q-35~~ | ~~How to write the target address used for relay decisions?~~ | **(decided)** Self-describing connection info. Relaying is stateless only if the target is reachable without a lookup (P-34) |
| Q-39 | Notation for connection info — URL or `host:port`; should it carry the scheme and transport type? | P-34. A scheme is needed to accommodate TLS and other transports later |
| ~~Q-40~~ | ~~Who decides an agent's reachable address on the internal network?~~ | **(resolved)** OUTER already owns it as an infrastructure fact. Not a target for protocol discovery (premise 1, D-52 withdrawn) |
| Q-41 | How far should self-describing addresses be trusted? | D-53. Whether to assume internal-network trust until authz is introduced |
| Q-42 | Add a process incarnation marker, or replace it with agent state persistence? | P-39, D-7. With persistence, the need to distinguish incarnations shrinks |
| Q-43 | Is `ingress_id` needed separately from `request_id`? | §1.3. Both are issued by OUTER and point to one inference |
| Q-44 | Remove `temperature`/`max_tokens` from the wire? | P-43, D-59. If removed, P4 knows no generation parameters at all |
| Q-45 | Keep the conversation structure as a wire structure, or carry it in the option string? | P-42, D-58. Premise 12 allows the latter |
| Q-46 | In a chain, which node performs sampling and grammar? | In pipeline parallelism, logits come **only from the last stage**. Whether options need to reach every node |
| Q-47 | How much of the sampler surface does the Pipeline runtime actually support? | P-40. Removing the whitelist is pointless if the underlying runtime cannot accept the options — needs measurement |
| Q-48 | **Demote** or **remove** `load_options.batching.*`? | D-60, P-44. **The keep option is out** — measurements confirmed that the runtime already derives the windows and that the declaration works as a cap (§92) |
| Q-49 | Put the connection establishment policy (retries, backoff, connection budget) into the protocol contract? | D-61. Source routing assumes a connection at every hop |
| Q-50 | Keep `Phase` (Prefill/Decode) and `text` in the contract? | D-64. Justifiable as part of the execution contract, but not backend-neutral |
| Q-51 | Make the adapter interface an explicit artifact? | D-64. If not, the neutrality burden stays on the message contract |
| Q-52 | What minimum fields must the envelope carry — are route, target, classification and deadline enough? | P-48·P-51. For relaying to skip the body, the envelope must be self-sufficient |
| Q-53 | Where is the boundary of the kinds the agent interprets directly? | P-50. `NODE_CREATE`/`NODE_DELETE` are agent-owned; the rest pass through |
| Q-54 | Distinguish self-contained and stage nodes through a `descriptor` declaration or through `adapter_kind`? | P-52, D-70. With the former P4 does not interpret it; with the latter the contract knows the kind |
| Q-55 | Make self-contained backends (vLLM and others) an actual adoption target? | D-70. If so, P-52 goes into phase 5 |
| Q-56 | Can the internal modifications in `0004-llama-context`/`0006-llama-graph` be converted into ABI exposure? | P-56, D-72. Needs an analysis of the patch contents first. 58% of the total |
| Q-57 | Limit ownership of the `compat/` layer to the Pipeline adapter alone? | P-53·P-54. If other stage backends appear, each gets its own |
| Q-58 | Keep vLLM as a pip pin or as a submodule? | P-53. A pin is enough for a self-contained backend |
| ~~Q-59~~ | ~~Where does the rest of `apps/llama` go?~~ | **(moved to topic M)** P-57 defines the placement table |
| Q-60 | Keep the surviving planning UI (`/api/models/*`, `/api/plans`) in `apps/llama`, or merge it into `apps/linker`? | P-57. Merging leaves 2 apps, but the OUTER core would get per-format screens |
| Q-61 | Is the cut through `llama_domain` actually clean — does `planner` depend on `protocol` or `pipeline-*`? | P-58. If it does, the split costs more |
| ~~Q-62~~ | ~~Does OUTER know model formats directly, or delegate inspection?~~ | **(resolved)** Neither. It **consumes modules that know the formats**. The OUTER core is format-agnostic (P-60, D-77) |
| Q-64 | Keep the name of the surviving `apps/llama`? | P-57. What it actually is, is a llama.cpp/GGUF planning provider, not a runtime app |
| Q-65 | When adopting vLLM, put its planning module alongside as a sibling? | P-60. If safetensors/HF inspection is needed, it goes in the same place |
| Q-66 | How wide is the official support matrix? | P-64, D-81. **The scope is the upstream tracking cost**. The current verification requirement is three: CUDA, Metal and OpenCL |
| Q-67 | How are runtime packs delivered — placed in advance, or fetched by the agent? | `package-*`, `deploy-*` and `write-runtime-pack-manifest` already exist. Whether the protocol gets involved |
| Q-68 | When does a variant mismatch fail — at OUTER orchestration or at the load entry check? | D-79·D-82. The same axis as Q-33's "accept that it shows up during execution" |
| Q-69 | How is version consistency between the agent binary and the runtime pack guaranteed? | `compat_id` and `build_id` exist, but there is no counterpart on the agent side |
| Q-70 | Who runs model file inspection — does OUTER read the files directly, or does the agent read them and pass them on? | D-83. If the latter, an inspection request message is added, and the agent reads only bytes without knowing the format |
| Q-71 | Keep a model catalog outside the firewall as well? | D-83. If so, duplicate file placement or syncing is needed |
| Q-72 | Promote overlap depth to a load option? | D-84. As long as it is only an environment variable, OUTER has no channel to set it. Per premise 6, it is one key of the opaque options |
| Q-63 | How to tie the branch merge timing to the `CLAUDE.md` update? | P-59, D-78 |
| ~~Q-36~~ | ~~Write down an orchestration constraint that places node 1 on the gateway?~~ | **(withdrawn)** The entry agent has no reason to host nodes, so the constraint is unnecessary. Last-node placement remains only as a performance optimization (D-51) |
| Q-37 | Must the controller handle deployments with several entry points (several internal networks)? | Premise 11 specifies a single entry point. Only check whether an extension is needed |
| Q-38 | Do relayed messages also consume queue lanes, or take a separate path? | P-35. Heavy relay volume eats into the Control lane budget |
| Q-32 | What is the restart unit when a mid-chain node fails — the whole request, or from prefill? | P-32. KV belongs to the node, so migration is impossible |
| ~~Q-24~~ | ~~What if the load-time configuration and the request-time chain disagree?~~ | **(decided)** OUTER's responsibility. The agent only reports failure for instructions that do not match its state; it neither cross-checks nor corrects them (same rationale as Q-33) |

## 90. Wire version decision

**Move to P4B1 v6 and give up v5 compatibility.** This is a cumulative result, not a single judgement — adopting any one of the items below changes the frame or the field structure.

| Reason | Item |
|---|---|
| The routed envelope has no place for a target address | P-34 |
| Remove `Participant.agent_id` and `HardwareReport.agent_id` | P-2 |
| Remove `controller_id` from lifecycle, redefine directions | P-6 |
| Add `NODE_DELETE`/`NODE_DELETED` | P-7 |
| Add stage entry/completion reports | P-28 |
| `EXECUTE` carries the chain and the return address | P-21·P-22·P-26 |
| Single `prompt` string → conversation structure | P-42 |
| agent-initiated announce (if adopted) | P-4 |

So no surface is kept "for compatibility with v5". Unused surfaces (§94.3) are cleaned up in the same round.

**What disappears in v6:** `agent_id`, `controller_id` in lifecycle, `machine_id` (never introduced), and the agent issuing `session_id`.
**What appears in v6:** target address, chain, return address, node removal, stage reports, conversation structure.

## 91. Build order

### 91.0 Rewrite vs revise (decision record)

This is the result of considering "discard the existing structure and rewrite in a new project, referring only to the implementation code". **Conclusion: do not start a new project. The only rewrite target is `layers/runtime`.**

**Code distribution**

| Layer | Lines | Change the plan requires |
|---|---:|---|
| `layers/protocol` | 1,330 | Field surgery — the codec primitives survive |
| **`layers/runtime`** | **3,489** | **Almost total** — topics E + K amount to a rewrite |
| `layers/adapters/adapter` | 2,159 | One file for `P-40`, fields for `P-45`, name correction |
| `layers/adapters/llamacpp` | 986 | Almost unchanged |
| `tools` | 1,400 | v6 adaptation, structure kept |

The destructive changes concentrate in `layers/runtime` (about 29% of the total). The 3,145 lines of adapters survive almost intact.

**Reasons not to recommend a rewrite**

1. **It would throw away what must be kept in order to rewrite what is being discarded anyway.** Of the 61 commits in `apps/p4`+`apps/llama/native`, 22 are fixes or reverts — **36%**. Those scars are not in runtime but in **the adapters and the native code** — the "throttle three levels above the GPU" comment in `capacity/mod.rs`, the stage terminal fixes, the micro-batch gate revert. And that knowledge lives not in the shape of the code but in **comments, docs and commit messages**. "Refer only to the implementation code" is exactly the way to throw that layer away
2. **Sequential revision already has the freedom a rewrite would give.** Breaking changes in v6 are allowed, and the only external consumers are our own tools, so there is no compatibility burden
3. **Three of the 40 open items (`Q-47`, `Q-48`, `Q-49`) depend on another session's measurements.** A rewrite must decide everything before anything runs. Sequential revision proceeds while phase 0 answers `Q-47` and the TPS session answers `Q-48` and `Q-49`. **A rewrite does not remove questions; it only moves up the time they must be answered, and meanwhile loses "run it and see" as a means of verification**
4. **A parallel TPS session is modifying the native code in the same tree.** Forking now would split the tree at the worst possible moment
5. **The verdict on the abstraction layers was "mostly right".** The vLLM check showed a clean dependency direction, and the leaks were 3~4 contract fields, one document, one crate name and the dispatch layer

**Execution shape:** build new modules under `layers/runtime`, move things over, then delete the old ones. Phase 1 plus topic K is exactly the runtime rewrite, and **since the wire is unchanged, the system keeps running in the meantime.**

**Conditions that would flip the decision to a rewrite**
- Phases 1 and 2 turn out not to be wire-neutral after all
- Open items resolve in a way that invalidates the adapters as well (e.g. `Q-51` settles on an adapter interface fundamentally different from today's adapters)
- The parallel TPS work ends and the native code freezes, which removes the fork cost

---

**The build is sequential.** Each phase is complete on its own, the system works when it ends, and it does not assume that later phases exist. Phase boundaries are drawn by **wire compatibility** — finish everything that can be done while keeping v5 first, and only then move to v6.

| Phase | Nature | wire | State at the end |
|---:|---|---|---|
| 0 | Adapter only | v5 | Structured output works |
| 1 | Runtime internals | v5 | The control plane becomes CPS |
| 2 | Semantic cleanup | v5 | The node state machine holds |
| 3 | Frame extension | **v6** | Addresses are self-describing and relaying works |
| 4 | Ownership and direction | v6 | OUTER→agent control holds |
| 5 | Expressiveness | v6 | Runtime specs are passed through in full |
| 6 | Chain | v6 | The inference path is expressed in the protocol |

---

### Phase 0 — Remove the adapter whitelist

| Item | Contents |
|---|---|
| P-40 | Remove the Pipeline adapter's 5-item whitelist; opaque pass-through |
| P-41 | Return `ERROR` instead of silently dropping unsupported keys |
| P-46 | Demote `docs/model-load.md` from formal schema to example |
| P-47 | Rename `adapter` → `pipeline` (crate and directory) |
| P-53 | Move the backend (upstream, compat, scripts) under the adapter |
| P-54 | Write down the upstream policy for self-contained vs stage backends |
| P-55 | Contribute the 2 upstream defect patches to the official project |

`P-53` is the same move as `P-47`, so the two are done together. When `P-55` lands upstream is outside our control, so this phase only starts it.

**No wire change. No dependency on other phases.** It is one file (`adapter/.../execution/options`), and the stock llama.cpp adapter already has the correct shape, so follow it.

**This phase alone resolves D-56 (structured output impossible in principle).** That is why it goes first.

**Gate:** Q-47 (what the underlying runtime actually accepts). If `ERROR`s increase after the whitelist is removed, runtime-side work follows.

---

### Phase 1 — Make the control plane CPS (wire unchanged)

**The wire is not touched.** The protocol as seen from outside stays the same; only the internal handling structure changes. Existing clients and bench tools therefore keep working.

There is an order.

**1-a. Persistent multiplexing at the adapter boundary (P-18)**
Replace `TcpTransport` with a `peer_mux`-based transport. This removes opening a new socket on every call (D-27). The frame format stays v5, and the adapter-side listener already handles routed frames, so **neither side changes**.

**1-b. Unify the output path through the queue (P-17·P-14)**
Discard `forward::capture`. The six lifecycle handlers, which received adapter responses as return values and branched on them, are changed to re-enter the responses as follow-up Tasks. Preconditions are settled before emitting.

At this point `D-22` (two terminals on one route) and `D-28` are resolved. Remove the `dispatch::compatibility` path.

**1-c. Long-running work as multi-step Tasks (P-19)**
Split loading into `start → progress observation → completion verdict`. The 600-second blocking polling loop (D-30) disappears, and progress shows up on the queue.

**1-d. Deadlines and cancellation in the worker loop (P-20)**
With work split into Tasks, the deadline can be checked on entry to each step, and `CANCEL` takes effect by blocking the enqueue of the next step. Resolves `D-31`, `D-32` and `D-33`.

**Gates:** Q-20 (replace the foundational traits vs wrap an adapter around them), Q-18 (whether to make the adapter internals CPS too), Q-19 (number of polling steps).

---

### Phase 2 — Clean up node semantics (wire unchanged)

The field structure stays the same; **only the behavior rules** change. Callers see this as more refusals.

| Item | Contents | Gate |
|---|---|---|
| P-16 | `NodeSlot.bindings` from `HashMap` → `Option<Binding>` | **Q-15** |
| P-15 | Enforce the `empty`/`bound`/`active` state machine | Q-5, Q-8 |
| P-8 | Turn on recording and comparison of `plan_revision` | — |
| D-21 | Remove the adapter's treatment of HTTP 404 as success | — |
| D-23 | Make loading onto an already loaded node fail | Q-15 |
| D-54 | Forbid an empty `session_id`, remove both issuers | — |

Phase 1's `P-14` must come first. If precondition checks do not finish before emitting, the state machine produces "refusal after a success notice".

**Q-15 is the prerequisite of this whole phase.** Without 1:1, the "already loaded" verdict is undefined.

---

### Phase 3 — v6 frame: self-describing addresses and relaying

**This is where the wire breaks.** The clients (`agent-link.mjs`, `controller-instance.mjs`) and the bench tools move over in the same phase.

**3-a. Target address in the envelope (P-34)**
Add the target agent's connection info to the routed envelope. At this point every target is self, so **behavior does not change.** Only the field takes its place.

**3-b. Abolish `agent_id` (P-2)**
Remove `Participant.agent_id`, `HardwareReport.agent_id` and the `agent-{host}-{pid}` generation, and replace them with the address. `is_local_bypass` becomes an address comparison.

**3-c. Separate envelope and body decoding (P-48·P-51)**
Parse only the envelope (route, target, classification, deadline) first and defer the body. The envelope carries the queue classification. This is the prerequisite for relaying without interpreting the body.

**3-d. Build relaying into queue dispatch (P-35·P-36)**
Insert the decision just before the handler call in the worker loop. If the target is self, pass the message to the handler; otherwise queue it for the next hop. 3-a and 3-c must be done first, so that the decision has its basis and the body stays untouched.

**When this phase ends, premise 11's firewall deployment works.** Internal-network agents can be reached through the entry agent.

**Gates:** Q-39 (address notation), Q-38 (whether relaying eats into lane budgets), Q-42 (incarnation marker).

---

### Phase 4 — Reorganize ownership and direction

| Item | Contents | Gate |
|---|---|---|
| P-6 | Remove `controller_id` from lifecycle, redefine `TaskDirection` | Q-6, Q-7 |
| P-37 | Put the relay responsibilities of the controller and agents into the contract | — |
| P-7 | Add `NODE_DELETE`/`NODE_DELETED` | Q-8 |
| P-12·P-13 | Redirect load instructions and load reports | — |
| D-19 | Remove the `controller_id` leftover in the host API | — |
| D-20 | Recover load progress and completion after a route drop | Q-14 |

The "OUTER→entry agent→target agent" path only really exists once phase 3's relaying is in place. `NODE_DELETE`'s preconditions are defined only once phase 2's state machine exists.

---

### Phase 5 — Expressiveness (can run in parallel)

After phase 3 these items are independent of each other and can proceed in parallel.

| Item | Contents | Gate |
|---|---|---|
| P-42·P-43 | Conversation structure, cleanup of wire generation fields | Q-44, Q-45 |
| P-9·P-10 | Opaque pass-through of load options, means of discovery in advance | Q-9 |
| P-1·P-3 | Capability/occupancy separation, snapshot schema | Q-1, Q-2 |
| P-4 | agent-initiated announce | Q-3 |

---

### Phase 6 — Chain

This comes last. It assumes all the earlier phases — self-describing addresses (3), relaying (3), direction rules (4), multi-step Tasks (1).

**6-a. The chain in the message (P-21·P-22·P-25·P-26)**
`EXECUTE` carries an ordered node list and a return address. Each entry is self-contained as a 4-tuple (§1.3).

**6-b. Stage reports (P-28·P-29)**
Entry/completion reports and statistics. OUTER's state observation becomes possible here.

**6-c. Decode loop (P-31·P-27·P-32)**
Ring structure, the `phase` distinction, chain cancellation propagation, and a written rule for KV ownership.

**Native-side work:** the Pipeline runtime must accept a chain per request. Today the stage composition is fixed in the load-time deployment (D-35). However, **agents and nodes stay mechanical** — if an instruction does not match their state they only report failure, and OUTER is responsible for chain consistency (P-33, Q-24/Q-33 decisions).

**Gates:** Q-26, Q-27, Q-28, Q-29, Q-30, Q-32, Q-46.

---

### Rollback between phases

Phases 0~2 keep the wire unchanged, so each can be rolled back individually. From phase 3 on it is v6, so **rolling back to before phase 3 requires rolling back the clients as well.** The only practical rollback boundary is the one between phases 2 and 3.

## 92. Relationship to throughput

### Where throughput lives — P4 does not own it

This repository's yardstick is **aggregate TPS** relative to a single session. But **throughput is not decided at the P4 layer.** Optimization happens in the node queue and in the physical concrete layer below it.

| Layer | Location | What it decides |
|---|---|---|
| Node queue | [`listener/queue.rs`](layers/adapters/adapter/src/infrastructure/listener/queue.rs) | Batch merging (`batch_coalesce_ms`), decode credit, dynamic batch composition |
| Capacity gate | [`capacity/mod.rs`](layers/adapters/adapter/src/domain/capacity/mod.rs) | Per-deployment cap on concurrent sequences |
| Physical concrete layer | `apps/llama` native | Micro-batching, stage overlap, stage terminal, layer occupancy |

The comment in `capacity/mod.rs` describes this boundary directly — with a process-global constant as the gate, "the throttle sat three levels above the GPU", so the native side reported `limit=50` while only 16 requests actually arrived.

**This reorganization therefore does not work against the throughput effort.** The two touch different layers and do not compete.

### P4 has only two duties

Raising throughput is not P4's job. P4 is responsible for the following two things.

**1. Deliver in full the declarations that the throughput-owning layer needs.**
This duty is currently broken. The capacity gate reads `stage_plan.load_options.batching.max_sequences` (capacity/mod.rs), but the Pipeline adapter passes through only 5 inference options (D-55). **Options that directly affect throughput, such as batching and speculative decoding, disappear midway through the protocol.**

`P-40` and `P-9` fulfil this duty, and they are the **only, precise point of contact** between the reorganization and throughput. The protocol does not raise TPS; it hands the knobs intact to the layer that can.

**2. Do not make the token path needlessly heavy.**
The "Costs" items below cover this.

### Side benefits

| Item | Effect |
|---|---|
| P-18 | `TcpTransport` **opens a new socket on every call** (D-27). Persistent multiplexing removes the connection cost on the control path |
| P-19 | Loading no longer occupies the blocking pool, so concurrent loads can scale |

### Costs

| Item | Cost | Judgement |
|---|---|---|
| D-51 | If the last node is outside the entry agent, +1 relay hop per token | Avoidable through orchestration — place the last node on the entry agent |
| P-22 | A P4 `EXECUTE` hop per stage move | Hidden state stays native, so these are control frames only |
| Q-26 | Copying the whole chain at every hop enlarges frames | Option to pass only the remaining segment |
| Q-38 | Relaying eats into the Control lane budget | Separate lanes |

### Stage reports are not counted as a cost (decided)

`P-28`'s entry/completion reports are not counted as token-path load, for two reasons.

- They are very small signals. They carry neither hidden state nor tokens — only correlation and statistics
- **Without them, OUTER cannot collect statistics on each node's state and behavior.** Losing observability costs more than the signals do

Prefill pipelining (cohort window splitting, keeping the arrival rate up during prefill) is a recently optimized path, so the reports do ride on top of it, but given the signal size it is hard to call this real interference. Q-30 is still open, but it will not be decided on the grounds of "reduce them because of load".

### What needs measuring

- **Q-47** — the range of samplers the Pipeline runtime actually accepts. If the lower layer cannot accept them, removing the whitelist only increases `ERROR`s. To be checked right after phase 0
- **Q-26** — frame growth from copying the chain. It is proportional to the length of the address notation, so compute it after Q-39 is settled

### What the parallel session established (as of 2026-08-13)

Throughput optimization proceeds in a separate session, whose conclusions are committed to [`docs/runtime-evidence.md`](docs/runtime-evidence.md). This reorganization does not touch that layer, but **the facts established there confirm or overturn several items of this plan.**

| Fact | Figures | Implication for this plan |
|---|---|---|
| Stage overlap works | **271.5 tok/s** at 16/24, 64 sessions, depth 2 — **4.11x** versus 66.0 for a single session | The chain design (topic F) is backed by measurement |
| Stage cost attaches to the **role** | Head ~64s, tail 32–40s — the same even when the cards are swapped | Placement must be computed by role, not by device. **An input to OUTER orchestration** |
| The cliff is **VRAM spill**, not the scheduler | The layer axis and the session axis end at the same wall (16 GiB head) | Grounds that orchestration based on `P-1`'s capability (totals) is right |
| KV cost model | **~188 MiB** per layer per 48 sessions | The substance of the planning knowledge OUTER holds (topic M) |
| A balanced placement can be slower | 13/27 is worse than 20/20 (without overlap, wall = sum) | The orchestration calculation changes depending on overlap |
| First-stage wait | 8.0–87.3 s | **Ends the debate over control message cost** — signals of a few hundred bytes are noise at this scale |

### Points of contact that became settled

| Item | Before | Now |
|---|---|---|
| `D-60`·`P-44`·`Q-48` | Waiting for the other session's conclusion | **Mechanism confirmed** — `max_sequences` is the credit cap, and below it `scheduler_window_target` derives the windows from `pipeline_stage_count` and the active cohort. In other words, **the runtime already derives the width, and the declaration functions as a cap.** P-44's demotion option matches the actual behavior |
| `D-51`·`Q-30` | Concern about token-path cost | **Resolved** — stage waits are measured in seconds |
| `D-61`·`Q-49` | Waiting | In progress — `3276d5d fix(pipeline): give ring wiring real backlog and a load-scaled connect budget` has landed. The connection budget and backoff are being shaped by measurement and will be lifted into the contract once settled |

**Still not settled unilaterally:** the protocol pinning down the throughput layer's decisions ahead of that layer is exactly the failure pattern `D-60` points out. The facts above are reflected only in the form of **the contract following conclusions that layer has reached**.

### What this document does not cover

Node queue batching policy, micro-batch size, stage overlap and layer occupancy are **not targets of this reorganization.** Throughput work proceeds independently in that layer, and the two efforts do not block each other.

Conversely, **do not try to solve throughput problems with protocol changes.** It is already established that the farther the throttle is from the GPU, the more actual arrivals and reported limits diverge (capacity/mod.rs), and adding gates to P4 would repeat that mistake.

### 2026-08-15: measurement confirmed that oversubscription is unsafe

The evidence is recorded in [`docs/runtime-evidence.md`](docs/runtime-evidence.md#2026-08-15-the-terminal-stage-access-violation-is-corruption-not-the-churn-or-the-oversubscription-ratio). This run met the retry condition that the 2026-08-13 entry had left open ("explain the access violation first").

Running 10, 20 and 30 concurrent requests in turn against a capacity of 10 slots, **10 and 20 passed and only 30 crashed.** At 20, real cohort replacement happened (20 requests completed while `peak=10` held) and the run still passed, so **session replacement itself is not the trigger.** The cause narrowed down to probabilistic memory corruption proportional to the total processing window volume — the grounds are that the old crash dump (`0xC0000409`, CRT heap integrity check failure) and this crash (`0xC0000005`, access violation) appeared with different exception codes under the same load pattern. Without a new dump with symbols, the exact defect location cannot be pinned down, so the investigation stopped here.

**Implication for this reorganization:** `D-60`/`P-44` (demoting the load-time `max_sequences` declaration to a cap) gained one more premise — concurrency above that cap (oversubscription) **cannot be guaranteed safe on the current native runtime.** Using the cap only as a cap and operating within it is currently the only verified safety boundary. Whether or not the protocol enforces this cap, this investigation established that the protocol must not silently allow concurrency above it.

## 93. Impact scope (to be detailed once the Qs are settled)

| Target | Expected change |
|---|---|
| [`domain/hardware/mod.rs`](layers/runtime/src/domain/hardware/mod.rs) | Full rewrite. Split out a vendor-neutral probe; add RAM, storage and NUMA |
| [`contract/message/mod.rs`](layers/protocol/src/contract/message/mod.rs) | New kinds for Q-2/Q-3, `NODE_DELETE`/`NODE_DELETED`, remove `controller_id` from lifecycle, artifacts fields depending on Q-11 |
| [`catalog/mod.rs`](layers/protocol/src/catalog/mod.rs) | class/queue/direction for the new kinds, rewrite `allows_direction` |
| [`task/mod.rs`](layers/protocol/src/task/mod.rs) | Extend `TaskDirection` (external→agent, plus agent→external if Q-3 is adopted) |
| [`registry/node/mod.rs`](layers/runtime/src/domain/agent/registry/node/mod.rs) | Remove `NodeSlot.controller_id`, add a removal operation |
| [`authorization/mod.rs`](layers/runtime/src/domain/agent/authorization/mod.rs) | Delete `ForeignController`, shrink the gate |
| [`lifecycle/mod.rs`](layers/runtime/src/domain/agent/lifecycle/mod.rs) | Settle re-creation semantics (Q-5), record `plan_revision` |
| [`adapters/adapter/.../lifecycle/load`](layers/adapters/adapter/src/application/lifecycle/load/mod.rs) | Introduce option validation (Q-13), remove the `controller_id` leftover |
| [`adapters/llamacpp/.../model_load_options`](layers/adapters/llamacpp/src/application/model_load_options/mod.rs) | From refusing everything to selective interpretation. Unsupported keys get an explicit `ERROR` |
| [`adapter/.../execution/options`](layers/adapters/adapter/src/application/execution/options/mod.rs) | **Remove the 5-item whitelist.** Switch to opaque pass-through (P-40, D-55) |
| [`contract/execution/mod.rs`](layers/protocol/src/contract/execution/mod.rs) | Single `prompt` string → conversation structure, cleanup of wire generation fields (P-42, P-43) |
| `ADAPTER_REGISTER.descriptor` contract | If Q-9 is adopted, declare the supported option key set (P-10) |
| [`controller-instance.mjs`](tools/controller/client/controller-instance.mjs) | Update the query, lifecycle and load surfaces |
| [`docs/model-load.md`](docs/model-load.md) | Full revision, including D-16's description mismatch |
| [`docs/message-pairs.md`](docs/message-pairs.md) | Update the pair table |
| `apps/llama` native Pipeline runtime | Accept a chain per request (phase 6). Move the stage composition, currently fixed in the load-time deployment, to per-request (D-35) |
| `apps/llama/upstream` · `native/compat` · `scripts` | Move under the adapter (P-53). upstream stays pristine; the patches, manifest and prep scripts move with it |
| `native/compat/<sha>/0004`·`0006` | Review conversion to ABI exposure (P-56, Q-56). 58% of the total patch volume |
| [`tools/controller/evidence`](tools/controller/evidence) | On the v6 switch, move the bench tools together with phase 3 |
| [`foundation/transport/mod.rs`](layers/runtime/src/foundation/transport/mod.rs) | **Replace the foundational contract.** Remove the synchronous completion signatures of `P4Handler`/`P4Transport` (D-26, Q-20) |
| [`task_queue/worker/mod.rs`](layers/runtime/src/foundation/task_queue/worker/mod.rs) | **Build the relay decision into the worker loop** — just before the handler call (P-35). The deadline check goes at the same point (P-20) |
| [`protocol/task/mod.rs`](layers/protocol/src/task/mod.rs) | Add the relay decision as the mirror of `is_local_bypass`, replace `Participant.agent_id` with the access address (P-2, P-34) |
| [`contract/message/mod.rs`](layers/protocol/src/contract/message/mod.rs) | Remove `HardwareReport.agent_id` (P-2) |
| [`domain/agent/mod.rs`](layers/runtime/src/domain/agent/mod.rs) | Remove the `agent-{host}-{pid}` generation. Replace it with an incarnation marker if needed (P-39, Q-42) |
| [`lifecycle/forward/mod.rs`](layers/runtime/src/domain/agent/lifecycle/forward/mod.rs) | Discard. Replace with re-entry as follow-up Tasks (P-17) |
| [`domain/agent/lifecycle/mod.rs`](layers/runtime/src/domain/agent/lifecycle/mod.rs) | Rewrite the six handlers as multi-step Tasks (D-29, P-19) |
| [`application/dispatch/mod.rs`](layers/runtime/src/application/dispatch/mod.rs) | Remove the `compatibility` path, apply deadlines and cancellation to every path (P-20) |
| [`adapters/adapter/.../lifecycle/load`](layers/adapters/adapter/src/application/lifecycle/load/mod.rs) | Break the 600-second polling loop into step Tasks (D-30, Q-18) |

## 94. Remaining topics

### 94.1 State externalization — boundary settled (conclusion)

Decisions accumulated over all the meetings have **settled the boundary** of this topic. It does not need to be opened as a separate topic.

**Authority model:** OUTER is the sole authority (premise 1). The choice between "external store as authority + agent reconciliation" and "agent as authority + external projection" no longer exists — identifier issuance (§1.3), orchestration intent (P-5) and placement structure (P-33) have all been settled as OUTER's.

**What is externalized:** the node list, placement, the load plan, chain composition, all business identifiers and agent access addresses.
**What is not externalized, and why:**

| Item | Reason | Basis |
|---|---|---|
| occupancy (free VRAM etc.) | A fact about the process, not a record | P-1 |
| KV | Belongs to the node for the lifetime of the request | P-32 |
| Execution credit (semaphore) | Physical occupancy | Topic A §admission |
| Socket/transport handles | Process-local | P-17 |
| Process incarnation | A generation marker, not state | P-39 |

**Throughput guardrail:** satisfied automatically as a consequence of premise 11. OUTER is not on the execution path in the first place — during execution OUTER is involved only in receiving returned tokens, and orchestration and loading all happen before execution. The rule "keep external access off the execution path" need not be enforced separately.

**Still open:** Q-42 (whether to persist agent state) and, in that case, the restart reconciliation procedure. This is not a question of externalization scope but of **agent-side recovery**.

### 94.2 Continuation externalization — scope reduced (conclusion)

The question was moving the 5 in-process maps keyed by `route_id` (`recipients`, `ingress_routes`, `prepared`, `active`, `deliveries`) into the envelope. After phases 1 and 4, **much of it disappears.**

| Map | Disposition |
|---|---|
| `prepared` | Disappears with P-19's multi-step Tasks. There is no longer any reason for preparation state to sit between Tasks |
| `active` | Replaced by P-20's cancellation redesign. Cancellation works by blocking the enqueue of the next step |
| `deliveries` | CPS-internal tracking, so it stays process-local |
| `recipients`·`ingress_routes` | **Remain.** Route ownership is a live connection, not something to serialize |

The requirement to split `AsyncExecution` into "serializable execution intent" and "process-local handles" is valid, and P-17 and P-19 are that work.

**One resulting asymmetry:** since KV belongs to the node (P-32), **inference continuation cannot be externalized.** If a mid-chain node dies, the request is restarted, not resumed (Q-32). Only **control continuation** is a target for externalization — recovering load progress and completion after a route drop (D-20, Q-14).

Once this asymmetry is accepted, continuation externalization is not a separate large reorganization topic but a side effect of phases 1 and 4.

### 94.3 Cleanup of unused surfaces (corrected by D-37 and D-46)
This section is no longer about "deciding on deletion". Both items are settled as **implementation targets**.
- **The `NodeNode` direction** — the core of the §1.2 inference path (D-37, P-22)
- **`phase=DECODE`** — required for the node-driven decode loop. Its cycle shape differs from that of prefill hops (D-46, P-31)

### 94.4 Not yet opened as topics

All the major reorganization items have been covered, but the following still need separate judgement.

- **The three possible meanings of `INGRESS_ACCEPTED`** (before validation / before credit / after credit). Phase 1's P-14 removes the structural cause, but **which point becomes the contract** still has to be decided. Taken together with Q-43 (keeping `ingress_id`), even the need for `INGRESS_ACCEPTED` itself is up for review
- **wire authz and TLS.** D-53 recorded the risk that overlaps with self-describing addresses, but it is outside the scope of this reorganization. Q-41 is judged together with their introduction
- **Multiple controllers / multiple entry points.** Q-37. Premise 11 specifies a single entry point, so the current reorganization assumes one
- **Agent state persistence.** Q-42. The item still open from §94.1

※ The `session_id` issuance issue was promoted to D-54 and removed from this section.

## 95. Revision history

| Date | Contents |
|---|---|
| 2026-08-16 | **Core spec item 1 (multi-platform) is covered by measurement — except Windows ARM64.** The source was moved to 2 Mac minis (macOS ARM64) and a DGX Spark GB10 (Linux aarch64) and built on each machine. Neither conditional compilation nor per-target dependencies were needed, and all four machines show **196 tests passed, 0 warnings** on `rustc 1.97.1`. In the chain `stage-0` Windows x64 → `stage-1` macOS ARM64 → `stage-2` Linux aarch64 → `tail-3` macOS ARM64, **the machine and OS change at every hop, and in most cases so does the architecture**: 800×48 three times, all 800/800 (80,723-81,799 frames per second), and 400×24 with a delayed backend also 400/400. **Windows ARM64 (Surface) is still unverified because of a key authentication failure.** As a side result, a **macOS operational constraint** was confirmed — an agent detached with `nohup` does not inherit local network access permission, so **receiving works but outbound traffic is silently dropped**. The symptoms point at P4 but the cause is not P4; the tell is that `consumed` and `forwarded` rise while there is no socket in the caller's direction |
| 2026-08-15 | **Scope boundary settled — do not generalize, but finish before any concrete adapter** (§1.4). This is not a general-purpose agent covering vague communication. The workloads to carry are already known in detail (**distributed load / prefill chain / decode ring / batch cohorts**), and both the core and the mock are built to fit them. The mock profile must therefore express **per-role cost, stage composition, ring hops and cohort windows**, not just a few latency values — §92's measurements (head ~64s, tail 32–40s, overlap 271.5 tok/s, stage wait 8.0–87.3 s, KV 188 MiB/layer/48 sessions) are the calibration reference for the shape, and the mock does not compute the numbers but is given them and imitates them. **At the same time, the agent must be completed and proven before any concrete adapter, independently of llama.cpp.** The two constraints do not conflict — the workloads decide what the agent supports, and the mock supplies all of it. The order is settled: ① node adapter interface (0 backend names) → ② agent core → ③ mock adapter → ④ fleet load proof (**completion is judged here**) → ⑤ concrete adapter. If completion cannot be judged at ④, the interface under-expresses the workloads, so go back to ① |
| 2026-08-15 | **Verification scope settled as the whole real fleet** (§1.4). Since stabilization is the purpose, do not stop at unit tests; **connect every available PC on the mock adapter and run complex, continuous scenarios**. The mock needs no GPU, so **machines with no backend and weak machines take part on equal terms**, which brings in Windows x64, Windows ARM64, macOS ARM64 and Linux ARM64 and **covers core spec item 1 (multi-platform) by measurement** (the same verification axis as topic N). The 2 external machines are beyond the WAN boundary, so **premise 11's entry agent and relaying are verified against a real firewall, not an artificial setup**. Connection details are managed in a local operations document outside the repository, and addresses, accounts and secrets are not copied into the repository |
| 2026-08-15 | **Settled that the core is GPU-independent, and fixed the means of proof** (§1.4). A "GPU hop" is merely the abstraction of a hop boundary, so **the whole implementation can be proven with the adapter interface and a mock adapter alone.** Two consequences — **`D-64` and `Q-51` are resolved** (the mock adapter resolves the defect that the adapter interface does not exist as an explicit artifact; what the mock can implement is the interface, and if the implementation completes without backend concepts, the boundary is clean — proven **by a second implementation**, not by a document), and **load and simulation need no GPU** (arrival storms, cohort replacement, cancellation, deadline expiry and back-pressure from slow hops all run on the mock, and what is observed is pure P4 behavior). **This is the branch's completion criterion** — if the core withstands load on the mock, there are grounds to state that later problems on the real system are not P4's problems |
| 2026-08-15 | **Two-level queue settled** (§1.4). A node is an abstraction for long-running GPU work, so a node's time must not become a worker's time. The agent main queue is emptied immediately, and for messages bound for a node, all a worker does is **move them to the per-node queue and be released**. Node queue draining is event-driven, with two triggers — message arrival, and **the moment the concrete object finishes a GPU hop**. **This separation creates observability** — agent queue depth becomes independent of GPU time, so under load a shallow agent queue with a deep node queue means the cause is below P4, and the reverse means it is P4. "Is P4 to blame?" becomes a question the two queue depths can answer. Deadlines and cancellation are also judged only at hop boundaries — there is no way to cut in mid-hop; simply do not start the next hop |
| 2026-08-15 | **CPS spec pinned down as 5 items** (§1.4). What premises 8 and 9 had only set as a direction is settled as an executable spec — every handling function is a procedure, a response is the side effect of queueing a message, and **the requesting side prepares a paired handler that will be called with the response as its argument** (continuation registration). Order is guaranteed not by the queue but by **each step registering the next step**. This invalidates `ResponseSink` — if the response path is bound to the call stack, the place to respond disappears when the call ends, so long-running work cannot be split into Tasks. A table maps which item each of `D-26`·`D-27`·`D-28`·`D-29`·`D-30` violates |
| 2026-08-15 | **Controller discarded, and the agent core set as the first implementation target.** Four corrections pointed in one direction. ① The controller's reason to exist was not being an inference front end that knows the node list but being **the agent's entry point, needed because of the firewall**. Per premise 1, the node list is already OUTER's external state, and the controller only **has it injected** through inference and load commands. ② But the entry point is the agent itself, so there is no unique state or judgement for a separate object to own — **the controller is removed from the participants.** The agent's only internal entity is **the node**. ③ A node is **an id shell** bound at load time to an instantiated concrete adapter (the code's `NodeSlot` already has this shape). ④ The messages an agent receives are node create/delete, inference intake and hardware survey, and **what a node receives is model load/release and prefill/decode** — inference is two steps: the agent takes the request and passes a prefill to the node. §1.0 and §1.2 fully revised; the controller removed from premises 1, 3, 5 and 11. **§1.4 added** — the 8 agent core specs (multi-platform socket, tokio workers + CPS, main queue, three message kinds, unified external send, receivers only enqueue, internal delivery goes to nodes) and the gap with the current code. With this, the worker's judgement is settled as a two-step dichotomy: `external address \| internal` → `agent itself \| node`. **`P-6` grows rather than shrinks** — it is no longer just removing `controller_id` from lifecycle but deleting `ParticipantRole::Controller`, the 3 `TaskDirection` variants, `ControllerProcessor` and the whole `p4-controller` binary |
| 2026-08-15 | **Branch `p4/adapter-boundary` started.** Targeting topics J and L, landed `P-47` (`adapters/adapter/` → `pipeline/`, crate `p4-adapter` → `p4-pipeline`), `P-45` (making `DRAFT_REPORT` structure-neutral) and `P-53` (backend move). **Found an omission: `P-45` had not been assigned to any phase in §91** — it removes transformer structure from the protocol contract, but phase 0 is "wire unchanged", so it was not included automatically. With it included, **the wire was bumped to v6** (the direction §90 had already settled, and the first payload-breaking change; without bumping the version byte, old binaries on bench hosts would misread frames instead of refusing them). **Side finding:** `kv_bytes` and `layer_bytes` came from `POST /api/models/inspect`, which is planning knowledge on the other side of the firewall line drawn by `P-60` — resolving `D-62` also resolved a firewall violation. **The plan's description of the `P-53` move boundary was wrong** — `compat/` held not only pure patches but also our C++ that is statically linked into `linker-node`, 37 files referenced the `apps/llama/upstream` path, and `layout.test.mjs` pinned down with assertions the very ownership structure that was being reversed. The actual self-contained unit is **pristine upstream + ordered patches + materialization script**, and the consumer of its output is connected through a single CMake variable |
| 2026-08-13 | First edition. Topic A (hardware query) — D-1~D-8, P-1~P-4, Q-1~Q-4 |
| 2026-08-13 | Added topic B (node ownership) — D-9~D-13, P-5~P-8, Q-5~Q-8. Added premise 3 |
| 2026-08-13 | **Withdrawn:** the first edition's item "redefine `runtime_generation` as the version of the external desired-state record". It contradicts premise 3, which puts the node instance in the adapter. Replaced by P-8's two-axis split — external intent is `plan_revision`, the instance generation is `runtime_generation` |
| 2026-08-13 | Added topic C (model load) — D-14~D-19, P-9~P-12, Q-9~Q-13. Added premise 4. Checked against upstream `common/arg.cpp` (pinned commit `3e3a7a4`, 347 `add_opt`) |
| 2026-08-13 | Added premises 5 and 6. **Decided:** load options pass through as opaque strings and are interpreted by the concrete adapter — P-9 rewritten, Q-13 decided, Q-10/Q-11/Q-12 withdrawn, resolution paths written into D-14/D-15. P-10 reduced to the remaining "means of discovery in advance" problem |
| 2026-08-13 | Reflected premise 5 (load reports are sent to the outside) — added P-13. The redirection itself is a relabeling; the real work is recovery after a route drop (D-20, Q-14) |
| 2026-08-13 | Added topic D (preconditions and state machine) — D-21~D-25, P-14~P-16, Q-15~Q-17. Added premises 7 and 8. Confirmed that `admission::lifecycle` already implements blocking in `active` |
| 2026-08-13 | Added topic E (full CPS audit) — D-26~D-33, P-17~P-20, Q-18~Q-20. Added premise 9. Fixed duplicate section numbers (introduced when topic D was inserted) and renumbered topic E onward as §17~§20 |
| 2026-08-13 | Added **the OUTER name** and the §1.2 target architecture flow — control path and inference path described separately. Added topic F (inference path and chain) — D-34~D-37, P-21~P-24, Q-21~Q-24 |
| 2026-08-13 | Chain delivery settled — **source routing**, in which the prefill message the controller sends to `Node[0]` carries the whole chain and the nodes forward on their own. §1.2 diagram fixed, P-22 rewritten, P-25~P-27 added, D-38~D-40 added, Q-21/Q-22 marked decided |
| 2026-08-13 | Added topic G (stage reports and decode loop) — D-41~D-46, P-28~P-32, Q-28~Q-32. Defined prefill as a state-check step. Reflected the node-driven decode loop in the §1.2 diagram. Wrote down KV node ownership as an externalization exception to premise 2 (P-32) |
| 2026-08-13 | **Premise 11 corrected — the entry agent is a pure entry point unrelated to node placement.** Discarded the draft "the agent of node 1 is the gateway". There are only two constraints: ① the controller can reach exactly one agent ② that agent reaches all the rest. Rewrote the §1.2 topology/inference diagrams and the role table, revised P-26/P-35/P-36, redefined D-49 as "no relay capability", resolved D-50/Q-34, withdrew Q-36, added Q-38 |
| 2026-08-13 | Relaying settled as **a general capability of every agent** — a handling path that simply forwards to another agent, besides the messages the agent performs itself. P-35 rewritten. "Entry agent" is a topological position, not a role, so `ParticipantRole` does not grow |
| 2026-08-14 | **Reflected the measured conclusions of the parallel TPS session (MI250, Mac, GB10).** Based on [`docs/runtime-evidence.md`](docs/runtime-evidence.md). Established facts — stage overlap gives **271.5 tok/s** (4.11x versus 66.0 single), stage cost attaches to the **role**, not the device (head ~64s / tail 32–40s), the cliff is **VRAM spill**, not the scheduler (the layer axis and the session axis end at the same 16 GiB wall), KV **~188 MiB/layer/48 sessions**, whether a balanced placement helps flips depending on overlap, stage wait 8.0~87.3 s. **Turned into settled:** the keep option of `Q-48` is out — `max_sequences` is the credit cap and below it `scheduler_window_target` derives `pipeline_stage_count` windows from the cohort, so P-44's demotion option matches the actual behavior. The load argument of `D-51`/`Q-30` is gone. **Added D-84** — overlap depth exists only as the `LINKER_PIPELINE_WINDOW_DEPTH` environment variable, so OUTER has no channel to set it (Q-72) |
| 2026-08-14 | **Verified whether "C++ and llama.cpp are removed from `apps/llama` completely" — they are.** Confirmed that planning knowledge does not call native code: `readPlannerModel` parses GGUF in pure TS, and the 3,182-line `planner` has 0 occurrences of `spawn`, `exec` or `child_process`. **P-57 corrected** — putting `/api/resources` on the survival list was a mistake. Its source, `cuda-driver-probe.ts`, consumes the C++ `linker-device-probe`, and hardware capability is already the agent's job per topic A, so it is replaced with the P4 path. This removes the last native dependency of what survives. **Added D-83** — the model files that planning knowledge must read are inside the firewall. This knocks out one of D-77's arguments (model files exist on the host even before loading, so an agent can read them without loading). Added Q-70/Q-71 |
| 2026-08-14 | Added topic N (multi-platform support and backend variants) — D-79~D-82, P-61~P-64, Q-66~Q-69. Agents run on GB10 Linux, Ubuntu (x86/arm), macOS and Windows, and llama.cpp is built separately for CUDA, ROCm, Metal, OpenCL, Vulkan and CPU. **The build side already handles the matrix** (28 script files, runtime pack packaging, `LINKER_LLAMA_COMPAT_ID` enforcement, the CUDA/Metal/OpenCL verification requirement), **but the protocol does not know it** — `HARDWARE_REPORT` has no runtime variant, and `adapter_kind` cannot distinguish variants. Settled runtime variants as capability class (they change only on rebuild) and added them to P-1's items. Recorded that `D-72`'s rebase cost is **multiplied by the number of platforms** (D-81) and that the platform availability of self-contained backends is an orchestration constraint (D-82; vLLM has no Metal or Windows path) |
| 2026-08-14 | **Split criterion settled as firewall position (P-60).** Split not by concern but by "which side of the firewall needs it" — planning knowledge (model format inspection, placement heuristics, capacity estimation) is **consumed** by OUTER outside the firewall, and execution knowledge (startup, loading, inference) is **owned** by adapters inside the firewall. **`apps/llama` is not dismantled; it survives as a planning knowledge provider** (P-57 rewritten) — only the concrete adapters that agents use are handed over. This **resolves D-77**: planning knowledge does not go into `linker_domain`, so the OUTER core stays format-agnostic and does not bloat as backends multiply. Q-62 resolved, Q-64/Q-65 added. The end state changes from 2 apps to 3, but the goal (P4 owning its backends) is still achieved. Confirmed that the dividing line matches the existing seams — `planner` 3,182 lines (52% of common) vs `protocol`+`pipeline-*` 2,840 lines, `/api/models\|plans\|resources` vs `/api/processes\|runtime*` |
| 2026-08-14 | Added topic M (repository end state) — D-75~D-78, P-57~P-59, Q-60~Q-63. At branch end, **only `apps/linker` and `apps/p4` remain, and `apps/llama` is dismantled**. **Role mapping settled: OUTER = `apps/linker` + `packages/linker_domain`** — this decides where capability records, orchestration, load plans, chain composition and identifier issuance belong. Controller/Agent/Node already exist in `apps/p4/entrypoints/*`. The dismantling covers 134 native files, 56 src files, 28 script files and **11,631 lines of `packages/llama_domain`**, the latter split three ways into placement heuristics and model inspection (→OUTER) and runtime verification and startup policy (→adapter). Q-59 moved to topic M |
| 2026-08-14 | Added topic L (backend ownership and upstream tracking) — D-71~D-74, P-53~P-56, Q-56~Q-59. **Verification showed that "always pull the latest and compile with only the needed features attached" is already implemented** — pristine submodule, hash-verified ordered patches in `native/compat/<sha>/`, application in a `.cache/` worktree (output not committed), a patch-free stock build, and our C++ including only public headers. The remaining problems are **where ownership sits** (adapter and backend spread over different apps) and **the tracking cost** (of 2,219 patch lines, `0004`/`0006` account for 1,287 lines, 58%). P-54 writes down the policy that self-contained backends need zero upstream modification (vLLM needs only a pip pin) and only stage backends need `compat/`. The 2 upstream defect patches (138 lines) are to disappear through upstream contribution |
| 2026-08-14 | **§91.0 rewrite vs revise decision record.** Considered rewriting as a new project and **concluded against it.** Destructive changes concentrate in `layers/runtime` (3,489 lines, 29% of the total), and the 3,145 lines of adapters largely survive. The scars (22 fix/revert commits out of 61 = 36%) are in the adapters and native code, not in runtime, and they live in comments, docs and commit messages rather than in the shape of the code, so "refer only to the implementation code" throws exactly that layer away. Three open items depend on the other session's measurements, so a rewrite would only move up the time they must be answered and lose the means of verification. The execution shape is a module-by-module rewrite of `layers/runtime`, and 3 conditions that would flip the decision are recorded alongside |
| 2026-08-14 | Checked the adapter abstraction against the possibility of adopting vLLM — D-70, P-52, Q-54/Q-55. **As a single node it is possible** (the llamacpp adapter's entire inference path is a single OpenAI-compatible `POST /v1/chat/completions`+SSE). What gets in the way is D-62, D-58 and the lack of a `session_id` counterpart, all of which are existing defects coming to light. **As a chain stage it is impossible**, because vLLM does PP internally — not because of a P4 defect. This revealed that the contract lacks the distinction between **self-contained nodes and stage nodes** — stating that chain length 1 is valid lets self-contained backends fit in without design changes |
| 2026-08-14 | Added topic K (message dispatch layers) — D-66~D-69, P-48~P-51, Q-52/Q-53. The target layering is **more generic toward the outside and more concrete toward the inside**, and kind interpretation belongs to the last two levels. Today kind branching is duplicated in `dispatch` and `AgentProcessor` (D-66), there is no participant delivery layer so the agent skips the node and goes straight to the adapter (D-67), even relaying requires decoding the whole body (D-68), and the queue is tied to the P4 message type (D-69). Added `3-c` (envelope/body separation) to phase 3. **Changed the section numbering convention — moved the synthesis sections to fixed numbers at §89 and above** so that adding topics does not shift the numbers |
| 2026-08-14 | Added topic J (adapter boundary) — D-62~D-65, P-45~P-47, Q-50/Q-51. **Audit result: the dependency direction is correct** — `p4-protocol` has 0 dependencies, there are no back-references, and `layers/protocol/src` has 0 backend strings. The leaks are in contracts and docs — KV/FFN in `DRAFT_REPORT`, `docs/model-load.md` codifying llama.cpp knobs (the code is opaque, but the docs declare a contract), no adapter interface artifact, and a concrete adapter occupying the interface's name as `p4-adapter` (the layer README already calls it `pipeline/`). Added P-46/P-47 to phase 0. Renumbered sections (§90~§95) |
| 2026-08-14 | Recorded the points of contact with the parallel TPS session (Mac+GB10, MI250) — D-60 (the load-time `max_sequences` declaration fixes a runtime-derived value as a gate), D-61 (no connection establishment policy), P-44 (`batching.*` demotion option), Q-48/Q-49. **This document settles neither on its own** — the protocol pinning down the throughput layer's decisions ahead of that layer is the failure pattern D-60 points out, so it is not repeated at the planning stage |
| 2026-08-14 | **Rewrote §92 as "Relationship to throughput".** Throughput is decided not by P4 but by **the node queue and the physical concrete layer** — batch merging and decode credit (`listener/queue.rs`), the capacity gate (`capacity/mod.rs`), and native micro-batching and stage overlap. This reorganization therefore does not work against the throughput effort; they touch different layers. P4 has only two duties: **① deliver in full the declarations that layer needs, and ② not make the token path heavy**; `P-40`/`P-9` fulfil ① and are the only point of contact between the reorganization and throughput. Stated explicitly: "do not solve throughput problems with protocol gates" — the failure when the throttle moves away from the GPU is already on record |
| 2026-08-14 | **Rewrote §91 as a sequential build order.** The earlier statement "phase 1 cannot be separated" **was wrong** — it confused `P-35` (relaying) needing `P-34` (addresses) with the CPS foundation needing addresses. `P-17` and `P-18` are wire-neutral and stand on their own. Redrew the phase boundaries by **wire compatibility** and restructured the phases as 0~6: finish everything possible while keeping v5 (0~2), then move to v6 (3~6). The single rollback boundary is between phases 2 and 3. **Decided:** Q-33/Q-24 — OUTER remembers the chain and load state, and the agent only reports failure on a mismatch. In return, agents and nodes become mechanically simple. **Decided:** stage reports are not counted as token-path cost — the signals are small, and without them OUTER cannot collect statistics. Added the native runtime and the bench tools to the impact scope |
| 2026-08-14 | **Investigation complete.** Added the summary section and the reading order. Added §90 wire version decision (v6 settled, with 8 reasons), §91 reorganization order (6-phase dependency graph and per-phase gate Qs), and §92 throughput impact assessment (3 benefits, 5 costs, 3 items to measure). Promoted §94.1 state externalization from an "unfinished section" to **a settled-boundary conclusion**, and §94.2 continuation externalization to **a reduced-scope conclusion** — settling the asymmetry that inference continuation cannot be externalized because of KV ownership (P-32) and that only control continuation is a target. Redefined §94.4 as "not yet opened as topics". Renumbered sections §90~§95 |
| 2026-08-13 | Added topic I (expressiveness of inference requests) — D-55~D-59, P-40~P-43, Q-44~Q-47. Added premise 12 (opaque pass-through of inference options + no silent omission). Checked against upstream `common_params_sampling` (about 35 fields). The key defect is that **the Pipeline adapter whitelists 5 sampling options and silently drops the rest**; structured output relies on logit filtering and cannot be done in post-processing, so it is unusable in principle on the Pipeline path (D-56). Renumbered sections (§89~§92) |
| 2026-08-13 | **Added §1.3 identifier ownership.** Prompted by the decision that OUTER assigns the `request_id` of each inference, collected the accumulated issuer decisions into one table and wrote down the principle **"OUTER issues identifiers"**. The only exceptions are `runtime_generation` (adapter) and transport/CPS-internal IDs. This resolves "§1.3's 4-tuple rule", which P-21 and P-25 had referenced without an actual section. Promoted D-54 (the agent issuing `session_id`), added Q-43 |
| 2026-08-13 | **Agent ID abolished (P-2 rewritten).** Every message self-describes the access address, so a separate agent ID is meaningless — **the access address itself is the agent's ID**. Withdrew the 3-layer `machine_id`/`boot_id`/`agent_instance_id` draft and resolved Q-4. `HardwareReport.agent_id`, `Participant.agent_id` and `agent-{host}-{pid}` are slated for removal. Redefined D-7 as "a process replacement cannot be detected", split it off into P-39 (incarnation marker) and added Q-42 |
| 2026-08-13 | **Withdrawn (D-52, Q-40):** "agents do not report their reachable address" is not a defect. OUTER owns infrastructure facts and already knows the addresses, and protocol discovery would create an ordering contradiction. Wrote into premise 1 **"distinguish what must be learned through the protocol from what OUTER already knows"**, and removed the reachable address from the capability items. The adapter's `ADAPTER_REGISTER.endpoint` is a dynamic fact internal to the agent, so self-registration stays |
| 2026-08-13 | **Self-describing addresses settled (P-34)** — messages self-describe not only the target object but also the connection info (URL, port and so on) of the agent it belongs to. A prerequisite for stateless relaying. The envelope, chain entries and return address share the same notation. Q-25/Q-35 marked decided, Q-39~Q-41 added. Added D-52 (agents do not report their reachable address) and reflected it in topic A's capability items; added D-53 (self-describing addresses and missing authz). A frame-layer change, hence v6 |
| 2026-08-13 | **Relay placement settled** — built into the basic dispatch that takes messages off the queue, not into the handler layer. It sits just before the `handler.handle` call in the worker loop, as the mirror decision of `is_local_bypass`. Higher-level code is not aware of relaying. P-35 rewritten; `task_queue/worker` and `protocol/task` added to the impact scope |
| 2026-08-13 | **Added premise 11 (network reachability) — the firewall constraint.** Rewrote premises 3 and 5: control instructions **pass through** the controller without being owned by it. Added a topology section to §1.2, fully revised the control path diagram, changed the inference return path to go through the gateway, and added a topology column to the role summary. Changed P-26's target (controller → gateway). Added topic H — D-47~D-51, P-34~P-37, Q-34~Q-37 |
| 2026-08-13 | Added premise 10 (a node does not know its own load structure). **Withdrawn:** D-43 (no layer range) is not a defect but an intended abstraction — layer judgements are excluded from the entry check. **Withdrawn:** P-30 (expose the layer range as binding metadata) contradicted P-5's "orchestration constraints exist only in the first layer". Replaced by P-33. D-44 absorbed into D-41 and reduced. Added Q-33 |
| 2026-08-13 | **Corrected (D-46):** ended the deferral of the deletion decision on `phase=DECODE`. It is settled as an implementation target because the node-driven decode loop requires it. Rewrote §94.3 from a "deletion decision" section into an "implementation settled" section |
| 2026-08-13 | **Corrected (D-37):** classifying the `NodeNode` direction as "dead surface — deletion candidate" was a mistake. It is the core of the target structure's inference path, so it is an implementation target. Rewrote §94.3 accordingly. The deletion decision on `phase=DECODE` was also deferred until P-22 is settled |
