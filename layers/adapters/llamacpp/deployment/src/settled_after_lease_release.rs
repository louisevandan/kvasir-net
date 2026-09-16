//! The one invariant `SEALED-CONTRACT.md` §1 says makes dropping
//! `Close`/`Closed` safe: **`Settled` must not be delivered until the
//! manager's lease release has already happened.**
//!
//! ## Evidence, read rather than assumed
//!
//! `apps/llama/src/server/pipeline-runtime-manager/manager.ts` and
//! `apps/llama/src/server/pipeline-inference-stream.ts` are outside this
//! crate's allowlist (both belong to the "llama path" agent's rows in
//! `SEALED-CONTRACT.md` §7), so this file documents what was found there
//! by reading, rather than adding a test to those files directly.
//!
//! `RingRuntimeManager.infer` (`manager.ts:173-179`, current v1 path):
//!
//! ```ts
//! const result = await group.control.infer({
//!     requestId,
//!     sequenceId: lease.sequenceId,
//!     ...input
//!   }, onToken, signal).finally(() => {
//!   group.sequences.release(lease);
//! });
//! ```
//!
//! `.finally()`'s callback -- `group.sequences.release(lease)`, the lease
//! release -- runs once `group.control.infer(...)`'s promise settles, and
//! the `.finally(...)` chain's own promise does not resolve until that
//! callback has returned. That chain is what line 173's `await` is waiting
//! on, so `RingRuntimeManager.infer` cannot proceed past it until the
//! release has already happened.
//!
//! Its caller, `pipeline-inference-stream.ts`'s `dispatchLine`
//! (`pipeline-inference-stream.ts:162`, `const result = await
//! manager.infer(...)`), cannot observe `manager.infer`'s resolution any
//! earlier than that either. Only after that `await` resolves does
//! `dispatchLine` send the `"done"` event (`pipeline-inference-stream.ts
//! :174-180`) -- v1's analogue of `Settled`. So for v1, as it stands today:
//! lease release happens-before the terminal event reaches the wire, by the
//! structure of one promise chain, not by a timing accident that could slip.
//!
//! This is v1 evidence. v2's coordinator is the same "llama path" allowlist
//! row and did not exist in the tree when this crate was written -- the
//! ordering has to hold there too for the sealed contract's claim about
//! `Settled` to be true, and this checkpoint's report asks the root/llama-path
//! agent to add an executable regression pinning exactly this
//! happens-before relationship wherever v2's coordinator lands (most likely
//! next to `manager.test.ts`). This crate cannot add that test itself
//! without leaving its allowlist.
//!
//! ## What this crate pins instead
//!
//! Its own dependence on the invariant being true. Whatever the P4 broker
//! keeps per submission -- a route, a receiver, anything scoped to that
//! submission's lifetime -- can only correctly be reclaimed once `Settled`
//! is the one and only signal it treats as terminal-and-final, because
//! `Settled` is the one event this contract guarantees is preceded by the
//! lease release. Nothing in this client independently re-verifies a
//! release; it trusts the contract's ordering claim exactly as far as the
//! contract says it may, and no further. The test below is the shape of
//! that trust: `Ledger` only reaches
//! [`crate::ledger::SubmissionState::Done`] -- the reclaimable state -- on
//! `Settled`, never on `Accepted` or any number of `Produced` events.

#[cfg(test)]
mod tests {
    use crate::contract::{Accepted, Event, Produced, SettleReason, Settled, Submit};
    use crate::ledger::{Ledger, SubmissionState};

    #[test]
    fn settled_is_the_sole_and_immediate_reclaim_signal_this_client_relies_on() {
        let mut ledger = Ledger::new(1);
        ledger.begin(Submit {
            deployment_id: "dep".into(),
            deployment_generation: 1,
            submission_id: "s1".into(),
            deadline_unix_ms: 0,
            request: "req".into(),
        });

        ledger.apply(&Event::Accepted(Accepted {
            submission_id: "s1".into(),
        }));
        assert_eq!(ledger.state_of("s1"), Some(SubmissionState::Accepted));

        // A `Produced` carries tokens, not permission to reclaim anything.
        ledger.apply(&Event::Produced(Produced {
            submission_id: "s1".into(),
            event_ordinal: 0,
            text: "x".into(),
            generated_tokens: 1,
        }));
        assert_eq!(ledger.state_of("s1"), Some(SubmissionState::Accepted));

        // `Settled` is the one and only transition to `Done` -- the point at
        // which a caller may reclaim anything it held for this submission,
        // trusting that the manager's lease release already happened before
        // this event was ever sent.
        ledger.apply(&Event::Settled(Settled {
            submission_id: "s1".into(),
            reason: SettleReason::Stop,
            generated_tokens: 1,
        }));
        assert_eq!(ledger.state_of("s1"), Some(SubmissionState::Done));
    }
}
