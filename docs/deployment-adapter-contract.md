# The deployment-submission contract

What replaces `Work`/`Hop` at the boundary a backend implements, once P4
stops making batching decisions. Written 2026-08-22 for checkpoint 1 of the
P4/llama execution-boundary rework; the contract itself is sealed and this
document only records what it means and why, not new decisions.

## 1. The contract

```
Submit { deployment_id, deployment_generation, submission_id, request }
Cancel { submission_id }

Accepted  { submission_id }
Rejected  { submission_id, reason: Full | Conflict | Invalid | DeploymentClosed }
Produced  { submission_id, event_ordinal, text, generated_tokens }
Settled   { submission_id, reason: Stop | Length | Canceled | Error, generated_tokens }
```

`Submit` names one independent unit of work. Nothing above this boundary
batches submissions together any more; if a backend batches at all, that
happens entirely behind whatever answers `Client::submit`.

Rust types: `apps/p4/layers/adapters/adapter/src/deployment/command/mod.rs`
(`Submit`, `Cancel`) and
`apps/p4/layers/adapters/adapter/src/deployment/event/mod.rs` (`Accepted`,
`Rejected`, `Produced`, `Settled`, and the `DeploymentEvent` sum type). The
trait a P4 llama client implements is `deployment::Client` in
`apps/p4/layers/adapters/adapter/src/deployment/mod.rs:46`; results arrive at
a `deployment::Sink`, never as a return value, for the same reason
`Adapter::start` and `EventSink` are split — a submission's duration must not
become a caller's own worker time.

TypeScript types:
`packages/llama_domain/src/common/protocol/pipeline-submission/types.ts`.
Sibling of `pipeline-runtime`, not a replacement — see that module's
`RingRuntime*`/`RingChatBatch*` types for the launch and observability
surface this does not touch.

## 2. What was deliberately left out, and why

**No multi-turn KV residency.** `sessionId` today is a lease key that
prevents numeric sequence collisions during one inference, not a
conversation handle. `RingSequenceLeasePool.release()` deletes both its
`bySession` and `bySequence` maps together
(`packages/llama_domain/src/common/pipeline-session/lease-pool.ts:32`), and
the next request starts with `reset_sequence` + `FRAME_RESET`, which clears
KV (`apps/llama/native/linker-node/inference/session.inc:15`). Resubmitting
the same `sessionId` for a later turn does not continue the earlier KV.
Multi-turn residency is a separate project needing its own identity —
`ConversationId` + `TurnId` + an explicit native close — and is out of scope
here.

**No `Close`/`Closed`.** The existing native cancel path cannot be mapped
onto it: once a slot goes terminal it becomes inactive
(`apps/llama/native/linker-node/control/terminal-return.inc:81`), and a
cancel against an inactive slot is dropped —
`apps/llama/native/linker-node/control/scheduler.inc:205` reads
`if (!active[sequence_id]) continue;` before ever looking at the cancel. The
TypeScript client already returns `false` for a cancel with nothing pending
(`apps/llama/src/server/pipeline-control/client.ts:187`). A `Close` verb
would have no native operation underneath it to call.

**No claim of immediate physical KV deletion.** This contract relies only on
the existing safety property that reuse happens after `RESET → ACK`
completes. If "observe physical KV freed immediately after `Settled`" ever
becomes a requirement, it needs its own native
`ResetIdleSequence → ResetComplete` pair — not built here, and not implied by
anything in this module.

## 3. `Full` is a value, not a substring

`Rejected.reason` is a closed four-value set on both sides:

- Rust: `RejectedReason` enum, `apps/p4/layers/adapters/adapter/src/deployment/event/mod.rs:31`
  (`Full`, `Conflict`, `Invalid`, `DeploymentClosed`).
- TypeScript: `PipelineSubmissionRejectedReason` string-literal union,
  `packages/llama_domain/src/common/protocol/pipeline-submission/types.ts:52`
  (`"full" | "conflict" | "invalid" | "deployment_closed"`).

A consumer tells `Full` apart from the rest with a `match` arm in Rust or
`=== "full"` in TypeScript — never by searching an error message. The
condition this exists to name is ordinary and already has a concrete source:
`RingSequenceLeasePool.acquire()` throws a plain `Error` reading
`"pipeline sequence capacity exhausted: …"` when the pool is full
(`packages/llama_domain/src/common/pipeline-session/lease-pool.ts:26`). This
contract's whole point at that call site is that whatever catches capacity
exhaustion converts it to a typed `Rejected { reason: Full }` there, once,
rather than leaving every downstream consumer to pattern-match that string.

`SettledReason` is the same shape for the settle side (`Stop`, `Length`,
`Canceled`, `Error` / `"stop" | "length" | "canceled" | "error"`).

## 4. The wire

Both sides encode and parse the same JSON, hand-validated field by field
rather than derived from a schema — the discipline every other parser in
`packages/llama_domain/src/common/protocol` already uses (see
`native-process/parse.ts`), and the one
`apps/p4/layers/adapters/adapter/src/deployment/wire/mod.rs` applies on the
Rust side so a malformed message fails for a traceable reason rather than
because a derive macro happened to reject it. Every message carries
`protocol: "linker-pipeline-submission-v1"` and a `type` discriminant
(`"submit" | "cancel" | "accepted" | "rejected" | "produced" | "settled"`).

The canonical shape of every command and event, plus the malformed shapes
both sides must reject, live in twin fixture files:

- `apps/p4/layers/adapters/adapter/src/deployment/fixtures.json`
- `packages/llama_domain/src/common/protocol/pipeline-submission/fixtures.json`

The two are hand-kept identical; nothing in this checkpoint generates one
from the other, so a future change to either must be applied to both and
re-verified against `wire/tests.rs` and `parse.test.ts`.

## 5. The structural reader

`apps/p4/layers/adapters/adapter/src/deployment/reader/mod.rs::check` and its
TypeScript twin
`packages/llama_domain/src/common/protocol/pipeline-submission/reader.ts::checkPipelineSubmissionEvents`
replay one submission's recorded events, in order, and report the first way
they fail to hold the contract:

1. `Produced.event_ordinal` not contiguous from zero.
2. A `Produced` before this submission's `Accepted`.
3. `Accepted` and `Rejected` both present for the same submission.
4. A second `Settled`.
5. Any event recorded after `Settled`.

Both are deliberately blind to content — they never read `text` or a
`reason`'s value, only the shape of the stream — so the same reader checks a
real recorded run or a hand-built fixture without caring which language or
which client produced it. This is the oracle checkpoint 2/3 fleet-level
verification is meant to call, not a description of what a client must do
internally to satisfy it.

## 6. What is not here

No adapter implementation, no client, no transport. This module is types,
parser, fixtures, and readers only — the P4 path
(`apps/p4/layers/adapters/llamacpp/deployment/**`,
`apps/p4/entrypoints/agent/src/adapters/**`) implements `deployment::Client`
against a real llama backend, and the llama path
(`apps/llama/src/server/pipeline-runtime-manager/**`,
`apps/llama/src/server/pipeline-control/**`) is what actually converts a
capacity-exhausted lease acquisition into `Rejected { reason: Full }` and
guarantees lease release happens before `Settled` is raised.
