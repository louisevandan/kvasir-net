# Release A connection reclaim verification plan

Created 2026-09-15 KST. Baseline P4 `530985320`; no native/model changes.
Covers the remaining A0 control-connection saturation from the [roadmap](../../docs/distributed-batching-roadmap.md#current-status).

## Goal, environment and prerequisites

Windows PowerShell, Rust/Cargo, real local TCP and bounded mailboxes. Remotely installed apps are left as they are.
Return the slot of an explicitly finished connection while preserving delayed responses on a normal TCP half-close.
EOF is not proof that the peer will accept no more output. An explicit FINISH is not native/request/KV settlement either.
The change contract follows [event transport](../../docs/event-protocol-v2.md#connection-finish-candidate).

## Procedure and expected values

1. Repeat a real Runtime→INSPECT→response→close→next connection cycle more times than the connection limit. Preserve the baseline failure.
2. Keep the pending output originals and costs, send all of them before the ACK, and return the slot. Keep delayed responses on half-close.
3. Check that FINISH on an earlier socket does not delete the new socket's binding or failure tombstone.
4. Late output and partial write failures preserve the original, the stored cost and the failure owner, with no automatic resend or settlement.
5. The Rust/Python OUTER consumers verify the close ACK, split receive, EOF, unexpected output and timeout.
6. Verify independent-copy removal mutations, the full workspace, real generation/reclaim on both adapters, and the current callers and fleet.

## Records and stop

Raw data `target/release-a-transport-20260915/`: per-command log/exit, source-manifest.json.
Failures of intended baseline counterexamples and removal mutations are detection evidence. Implementation verification failures, including compile and test-writing errors,
stop the work once they reach 3 cumulatively in the same phase. An intermediate PASS does not reset the count.
The user's stop condition applies more strictly than the existing maximum of 3 rounds. After a stop, make no further fixes, runs or deployments, and
record the WIP source, failures and items not run. No UI.

## 2026-09-15 deterministic resumption

At the user's instruction, work resumes with first-run success as the default goal. The three earlier failures are not erased or
reclassified as successes in a new round. Before running a new candidate, confirm the whole contract below with code, callers and counterexamples.

| Boundary | Result fixed before the first run |
| --- | --- |
| Normal ACK | Receiving u32-LE 0 in split pieces still succeeds, and later sends are rejected. |
| Valid abnormal frame | Read the length prefix and the whole body within the same deadline and keep them in the client buffer; record the frame/buffer byte counts in the error and in the run cleanup artifact. The Event is not approved or settled. |
| EOF/timeout during an abnormal frame | Do not discard the received prefix/body; fail with EOF/timeout. No resend or reuse. |
| Oversized frame | Keep only the prefix that exceeds the protocol/client limit; do not allocate or read the body. |
| agent FINISH | Detach only that socket's live route, drain the output queue it already owns, then ACK. The replacement route, failure tombstone and receipt/KV are not changed. |
| Callers | Rust event-drive and the HF Python client use the same close semantics and diagnostic preservation. |

The first candidate runs only after the whole table above is implemented together with the existing half-close/route/cost counterexamples, real TCP and the Python suite.
After a PASS in round 1, no functionality is changed; confirmation comes from the full workspace, independent removal mutations and real llama.cpp/HF generation/reclaim.
Each run records its purpose, source hash, command and full summary.

`first_pass` is the result of the first complete gate, which includes the docs gate, the full workspace with the feature off and on, and the required real
consumption paths; it is not a targeted crate/Python bundle. A partial test pass is not promoted to first-pass success.
