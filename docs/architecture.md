# How a message moves

> 문서 지위 (2026-09-06): **경로별 참고·재감사 필요**. 기존 Chain/Hop 설명과 당시 결정을 포함한다. event 경로의 현재 보장은 코드 및 새 검증 규약으로 확인한다.
> 현재 목표·상태·순서는 [실행 로드맵](distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](document-map.md)를 따른다.

현재 층별 권한·의존성과 llama.cpp 및 구상 backend의 변경 격리는
[계층 격리 계약](layer-isolation-contract.md)이 소유한다. 이 문서의 아래 경로 설명과 구분한다.

The diagram below is the retained legacy hop path. A registered deployment
uses the thinner path first:

```
socket -> P4 route/identity -> bounded deployment client queue
       -> adapter-owned persistent stream -> adapter scheduler/native Pipeline
       <- Produced* / Settled <- adapter-owned lifecycle and batching
```

P4 does not compose Prefill/Decode windows on that path. It validates routing,
selects the registered deployment and enqueues a backend-neutral submission.
If an inference frame names a registered deployment but cannot be decoded as a
fresh submission, it is refused; it never falls through into the hop path.
The native llama adapter owns admission backpressure, request shaping,
continuous mixed Prefill/Decode ubatches, sequence slots and Pipeline stages.

```
socket ──▶ [ main queue: control | response | decode | prefill ]
                              │
                        dispatcher (one task)
                              │  route → worker, so a route stays sequential
                     ┌────────┴────────┐
                  worker            worker  …
                     │
        ┌────────────┼────────────┐
     forward      agent's own   node's queue ──▶ hop ──▶ adapter ──▶ backend
    (to a peer     duties            │                    │
     or OUTER)                       └────── hop ends ────┘
```

## The socket reader only enqueues

No handler runs in it, no body is decoded, no connection gets a thread to work
in. A frame's length comes from its header, so the reader knows it has a whole
frame without looking inside. A full lane is counted and reported rather than
answered, because these connections carry traffic one way and there is nowhere
in band to say so.

## Workers are chosen by route

Order within a route is the only ordering guarantee there is, and it comes from
registration: a node enqueues a token before the lap that will produce the next
one. Handing consecutive frames of one route to different workers throws that
away, and a caller sees it as P4 reordering its stream. Hashing the route keeps
each route sequential while different routes still run at once.

Forwarding happens on the dispatcher rather than in a task of its own, for the
same reason.

## Lanes are preferred, not obeyed

Control over everything, decode over prefill — a lap belongs to a request
already holding KV across a chain, where a prefill has not started.

But the preference is bounded: every sixteenth frame the order gives way and
all four lanes compete. Strict priority is not a preference but a veto, and a
busy lane that is never empty means the ones under it are never polled at all.

## The node holds the long work

A worker's job for a node-bound frame is to move it to that node's queue and be
finished. Whether the node acts in a microsecond or a minute stops being
something the agent's depth reflects — which is the separation that lets a
slowdown be attributed to one side of the adapter boundary or the other.

A node advances on two events and no others: work arriving, and a hop ending.
No timer, because a node's pace is the backend's pace and a third trigger would
be a guess at it. It runs one hop at a time by construction: it starts the next
only when it sees the previous end.

Deadlines and cancellation are decided at that boundary. There is no way to
interrupt a hop and no need — not starting the next one is the whole mechanism.

## A hop carries a window

Batching is the node's decision, bounded by what the load declared and never
derived. The window is composed next to the node rather than upstream, because
a gate further from the work was measured reporting a limit its arrivals
disagreed with.

## Nothing returns a value

Handlers are procedures whose only output is a frame on a queue. A requester
registers a continuation instead of waiting, because a response path pinned to
a call stack dies with that frame — and long work is split across tasks.

Backpressure runs the other way down the same chain: a full peer queue holds
the dispatcher, which fills the lanes, which holds a node's outbox, which slows
the node at its next hop. The chain ends at the thing producing the work.
