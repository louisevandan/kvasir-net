//! Current-run OUTER authority, registered before transmission. A receipt may
//! discharge a terminal expectation but can never create one. This does not
//! mint restart epochs or permit multiple simultaneous attempts of one request.
use p4_llamacpp_staged_adapter::v2::{ApprovedOutputPayload, ReleaseMember, ReleaseReceipt};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Attempt {
    submission_event_id: String,
    owner: Option<(u32, u64)>,
    expected: Option<ReleaseMember>,
    released: bool,
}

pub(super) struct OutputApproval {
    request: String,
    candidate: Attempt,
    pub(super) expected: Option<ReleaseMember>,
}

#[derive(Default, Debug, PartialEq, Eq)]
pub(super) struct SubmissionLedger {
    attempts: BTreeMap<String, Attempt>,
    submission_ids: BTreeSet<String>,
}

impl SubmissionLedger {
    pub(super) fn register(&mut self, request: &str, submission: &str) -> Result<(), String> {
        if request.is_empty() || submission.is_empty() || submission.contains('\0') {
            return Err("submission requires request and event identity".into());
        }
        if self.attempts.contains_key(request) || self.submission_ids.contains(submission) {
            return Err("duplicate submitted request or attempt identity".into());
        }
        self.attempts.insert(
            request.into(),
            Attempt {
                submission_event_id: submission.into(),
                owner: None,
                expected: None,
                released: false,
            },
        );
        self.submission_ids.insert(submission.into());
        Ok(())
    }

    /// The caller first checks route, output positions and the sampled budget.
    /// Only a fully approved terminal output establishes the release member.
    pub(super) fn approve_output(
        &mut self,
        output: &ApprovedOutputPayload,
    ) -> Result<Option<ReleaseMember>, String> {
        let approval = self.prepare_output(output)?;
        let expected = approval.expected.clone();
        self.commit_output(approval);
        Ok(expected)
    }

    pub(super) fn prepare_output(
        &self,
        output: &ApprovedOutputPayload,
    ) -> Result<OutputApproval, String> {
        output.validate().map_err(str::to_owned)?;
        let attempt = self
            .attempts
            .get(&output.outcome.request_id)
            .ok_or("output references an unregistered submission")?;
        if attempt.submission_event_id != output.submission_event_id {
            return Err("output submission identity does not match the sent attempt".into());
        }
        let owner = (output.outcome.sequence_id, output.incarnation);
        if attempt.owner.is_some_and(|expected| expected != owner) {
            return Err("output changed sequence or incarnation within one attempt".into());
        }
        if attempt.expected.is_some() || attempt.released {
            return Err("output arrived after a terminal release expectation".into());
        }
        let expected = output
            .release_operation_id
            .map(|operation_id| ReleaseMember {
                request_id: output.outcome.request_id.clone(),
                submission_event_id: attempt.submission_event_id.clone(),
                sequence_id: output.outcome.sequence_id,
                incarnation: output.incarnation,
                operation_id,
            });
        let mut candidate = attempt.clone();
        candidate.owner = Some(owner);
        candidate.expected = expected.clone();
        Ok(OutputApproval {
            request: output.outcome.request_id.clone(),
            candidate,
            expected,
        })
    }

    /// The synchronous consumer performs all other fallible evidence checks
    /// before committing this small candidate. No receipt can create it.
    pub(super) fn commit_output(&mut self, approval: OutputApproval) {
        *self
            .attempts
            .get_mut(&approval.request)
            .expect("prepared attempt exists") = approval.candidate;
    }

    /// Validate every member before changing any attempt. Exact member replay
    /// on a fresh envelope has no new effect; same-event-ID rejection remains
    /// the caller's independent envelope policy.
    pub(super) fn apply_receipt(
        &mut self,
        receipt: &ReleaseReceipt,
    ) -> Result<Vec<ReleaseMember>, String> {
        receipt.validate().map_err(str::to_owned)?;
        let mut newly_released = Vec::new();
        for member in &receipt.members {
            let attempt = self
                .attempts
                .get(&member.request_id)
                .ok_or("release receipt references an unregistered request")?;
            if attempt.expected.as_ref() != Some(member) {
                return Err("release receipt does not match an approved terminal member".into());
            }
            if !attempt.released {
                newly_released.push(member.clone());
            }
        }
        for member in &newly_released {
            self.attempts
                .get_mut(&member.request_id)
                .expect("whole receipt validated")
                .released = true;
        }
        Ok(newly_released)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use p4_llamacpp_staged_adapter::v2::OutcomePayload;

    fn terminal(request: &str, slot: u32) -> ApprovedOutputPayload {
        ApprovedOutputPayload {
            outcome: OutcomePayload {
                load_generation: 1,
                session_id: "s".into(),
                request_id: request.into(),
                sequence_id: slot,
                token: 42,
                text: "normal".into(),
                position: 4,
                stop: Some("length".into()),
            },
            submission_event_id: format!("sent-{request}"),
            incarnation: 3,
            release_operation_id: Some(7 + slot as u64),
            issued_work: Some(p4_llamacpp_staged_adapter::v2::IssuedWorkProof {
                revision: 1,
                issue_count: 1,
                last_ordinal: 1,
                authority_digest: [0; 32],
                digest: [1; 32],
            }),
        }
    }

    #[test]
    fn later_bad_member_preserves_every_release_and_exact_replay_is_zero() {
        let mut ledger = SubmissionLedger::default();
        let mut members = Vec::new();
        for (request, slot) in [("a", 0), ("b", 1)] {
            ledger
                .register(request, &format!("sent-{request}"))
                .unwrap();
            members.push(
                ledger
                    .approve_output(&terminal(request, slot))
                    .unwrap()
                    .unwrap(),
            );
        }
        let mut receipt = ReleaseReceipt {
            load_generation: 1,
            session_id: "s".into(),
            members,
        };
        let before = format!("{ledger:?}");
        receipt.members[1].operation_id += 1;
        assert!(ledger.apply_receipt(&receipt).is_err());
        assert_eq!(format!("{ledger:?}"), before);
        receipt.members[1].operation_id -= 1;
        assert_eq!(ledger.apply_receipt(&receipt).unwrap().len(), 2);
        assert!(ledger.apply_receipt(&receipt).unwrap().is_empty());
        receipt.members[0].incarnation += 1;
        assert!(ledger.apply_receipt(&receipt).is_err());
    }

    #[test]
    fn receipt_cannot_establish_an_expectation_and_failed_output_cannot_bind_an_owner() {
        let mut ledger = SubmissionLedger::default();
        ledger.register("a", "sent-a").unwrap();
        let before = format!("{ledger:?}");
        let receipt = ReleaseReceipt {
            load_generation: 1,
            session_id: "s".into(),
            members: vec![ReleaseMember {
                request_id: "a".into(),
                submission_event_id: "sent-a".into(),
                sequence_id: 0,
                incarnation: 3,
                operation_id: 7,
            }],
        };
        assert!(ledger.apply_receipt(&receipt).is_err());
        let mut output = terminal("a", 0);
        output.submission_event_id = "old-attempt".into();
        assert!(ledger.approve_output(&output).is_err());
        assert_eq!(format!("{ledger:?}"), before);
        ledger.approve_output(&terminal("a", 0)).unwrap();
        assert_eq!(ledger.apply_receipt(&receipt).unwrap().len(), 1);
    }
}
