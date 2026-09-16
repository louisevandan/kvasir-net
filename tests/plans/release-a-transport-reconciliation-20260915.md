# Release A settlement and reconnection for unknown transport results

2026-09-15. Baseline `94c796c1e`. The scope is single-hop delivery in the P4 event transport; adapter payload,
native completion, and request/KV settlement authority are not changed. This is the gate after [FINISH acceptance](../reports/release-a/20260915_142237.md),
and it is closed before the full Qwen122B topology run.

## Conclusions fixed before the run

Completion of the current socket write is not evidence of acceptance by the remote broker. The current original of a partial write and the unstarted queue behind it
are kept in agent memory, but there is no API to query or resolve them externally. The remote duplicate window also provides no receipt query or outstanding
pin. Automatic resend after reconnection can therefore produce duplicate native effects, and the absence of a receipt cannot distinguish
non-receipt from eviction. Simply deleting the peer cache or restarting the agent is not an acceptance candidate.

The first implementation candidate provides all of the following states and authorities at once.

| State | Verdict and allowed action |
| --- | --- |
| `rejected_local` | Encode/version/capability was rejected before the socket write. Preserve the original and the no-effect state; do not resend. |
| `not_started` | Connect/route failed, so the socket was not touched. The same original allocation, event ID and digest can be delivered again after an explicit reconnection. |
| `uncertain` | The outcome is unknown after the prefix/body/flush started. Do not resend automatically; query the remote hop receipt. |
| `accepted_exact` | The remote committed the same event ID and canonical bytes to the broker, and the receipt is pinned. Retire only the local original and proceed with the next queue item. |
| `conflict` | Different bytes were observed for the same ID. Preserve the failed original and the evidence, and quarantine that connection generation. |
| `unknown` | No receipt, deadline exceeded, peer unreachable, or outside the proof horizon. Do not presume success or non-receipt; quarantine. |

## Implementation boundaries

1. The protocol versions the connection generation, delivery attempt ID, event ID, canonical event digest and receipt state.
   Old peers without capability negotiation are not sent the new behavior and keep the existing fail-closed behavior.
2. The receiver binds broker commit to hop receipt creation. The count, bytes and horizon of outstanding receipts are
   accounted separately, and if there is no space the event is rejected before commit. Eviction does not remove unsettled receipts.
   Only the receipt ACK that the sender sends after retiring its local original releases the pin; duplicate or lost ACKs are idempotent.
3. Per failure ID, the sender owns the current original, the unstarted queue, whether sending started, the target and the digest. INSPECT does not
   expose the payload; it reports per-state counts, byte counts, the oldest timestamp and the failure IDs.
4. Reconcile changes only an exact receipt into `accepted_exact`. `not_started` redelivery and queue resumption after exact keep
   the original order. After conflict/unknown, a new generation starts only after the existing generation has been isolated.
5. Rust event-drive and the HF Python client also consume OUTER receive receipts with the same semantics. FINISH ACK, request
   terminal, release and receipt/KV settlement do not substitute for one another.

## First complete gate

| ID | Fault injection and pass condition |
| --- | --- |
| R1 | Encode/version rejection: 0 ledger or remote effects and 0 resends. Connect/route failure: original and cost preserved, exactly 1 commit after an explicit reconnection |
| R2 | Reset after part of the prefix / part of the body / flush: all uncertain. Reconnection alone causes 0 resends |
| R3 | Hop receipt lost after remote commit: exact query retires only the local original; 0 additional native/queue effects |
| R4 | Same ID with different bytes, receipt absent, receipt horizon exceeded: conflict/unknown quarantine; 0 promotions to success |
| R5 | Receipt count/bytes boundary ±1 and data queue saturation: ledger, reservation, credit and output effects identical before and after rejection; reconcile control proceeds |
| R6 | Normal event after an uncertain predecessor: 0 overtakes before the predecessor is exact or quarantined; original order kept after resolution |
| R7 | Old peer, invalid version/generation/attempt/digest: rejected before execution; 0 fallback replays |
| R8 | Real TCP OUTER and agent↔agent, Rust/HF clients, llama.cpp/HF normal generation, cancellation, reclaim and re-acceptance |
| R9 | Disconnection between two physical hosts, late receipt, next normal wave after reconnection. Seal the source/binary and the failure/receipt bytes |

The first run includes the R1–R9 implementation, independent oracles and removal mutations, the docs gate, feature off/on
`cargo test --workspace --no-fail-fast`, the full HF Python suite and the real paths of both adapters.
A partial crate pass is not `first_pass`. Round 2 fixes only the limited differences the first gate exposed, and round 3 confirms, with no functional change,
through a clean rebuild, full regression and recompiled mutations in an independent worktree.

## Completion and next phase

Completion means that failed originals, receipts and reserved bytes are explained by per-state limits, and that the next normal wave was accepted
without presuming uncertain to be success or non-receipt. Only after that does the full Qwen122B topology run start: PLAN→LOAD→normal response→
per-request deadlines, 8 waves, and cancel/reclaim/re-acceptance.
