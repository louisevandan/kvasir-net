//! Head-approved OUTER evidence. Internal stage controls keep their own wire.
//! Submission authority precedes OUTPUT; terminal OUTPUT precedes the receipt.
use super::{IssuedWorkProof, OutcomePayload};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(from = "ApprovedOutputWire")]
pub struct ApprovedOutputPayload {
    #[serde(flatten)]
    pub outcome: OutcomePayload,
    pub submission_event_id: String,
    pub incarnation: u64,
    /// Assigned by the head before its release effects execute. Present only
    /// on the terminal sampled output, never learned from a RELEASED receipt.
    pub release_operation_id: Option<u64>,
    /// Only a terminal sampled output seals the accepted issue history.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issued_work: Option<IssuedWorkProof>,
}

// serde's flatten and deny_unknown_fields do not compose. Decode the flat
// wire with an explicit strict DTO, then project to the public outcome view.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovedOutputWire {
    load_generation: u64,
    session_id: String,
    request_id: String,
    sequence_id: u32,
    token: i32,
    text: String,
    position: u32,
    stop: Option<String>,
    submission_event_id: String,
    incarnation: u64,
    release_operation_id: Option<u64>,
    issued_work: Option<IssuedWorkProof>,
}

impl From<ApprovedOutputWire> for ApprovedOutputPayload {
    fn from(wire: ApprovedOutputWire) -> Self {
        Self {
            outcome: OutcomePayload {
                load_generation: wire.load_generation,
                session_id: wire.session_id,
                request_id: wire.request_id,
                sequence_id: wire.sequence_id,
                token: wire.token,
                text: wire.text,
                position: wire.position,
                stop: wire.stop,
            },
            submission_event_id: wire.submission_event_id,
            incarnation: wire.incarnation,
            release_operation_id: wire.release_operation_id,
            issued_work: wire.issued_work,
        }
    }
}

impl ApprovedOutputPayload {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.submission_event_id.is_empty()
            || self.submission_event_id.contains('\0')
            || self.incarnation == 0
        {
            return Err("approved output requires submission and incarnation");
        }
        match (
            self.outcome.stop.is_some(),
            self.release_operation_id,
            self.issued_work,
        ) {
            (false, None, None) => Ok(()),
            (true, Some(operation), Some(proof)) if operation != 0 => proof.validate(),
            _ => Err(
                "release operation and issued-work proof must be present only on terminal output",
            ),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReleaseMember {
    pub request_id: String,
    pub submission_event_id: String,
    pub sequence_id: u32,
    pub incarnation: u64,
    pub operation_id: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReleaseReceipt {
    pub load_generation: u64,
    pub session_id: String,
    pub members: Vec<ReleaseMember>,
}

impl ReleaseReceipt {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.load_generation == 0
            || self.session_id.is_empty()
            || self.session_id.contains('\0')
            || self.members.is_empty()
        {
            return Err("release receipt requires load, session and members");
        }
        let mut requests = BTreeSet::new();
        let mut slots = BTreeSet::new();
        for member in &self.members {
            if member.request_id.is_empty()
                || member.request_id.contains('\0')
                || member.submission_event_id.is_empty()
                || member.submission_event_id.contains('\0')
                || member.incarnation == 0
                || member.operation_id == 0
                || !requests.insert(&member.request_id)
                || !slots.insert(member.sequence_id)
            {
                return Err("release receipt contains invalid or repeated members");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(terminal: bool) -> ApprovedOutputPayload {
        ApprovedOutputPayload {
            outcome: OutcomePayload {
                load_generation: 7,
                session_id: "pipeline".into(),
                request_id: "a".into(),
                sequence_id: 0,
                token: 42,
                text: "answer".into(),
                position: 4,
                stop: terminal.then(|| "length".into()),
            },
            submission_event_id: "sent-a".into(),
            incarnation: 3,
            release_operation_id: terminal.then_some(11),
            issued_work: terminal.then_some(IssuedWorkProof {
                revision: 1,
                issue_count: 2,
                last_ordinal: 3,
                authority_digest: [17; 32],
                digest: [23; 32],
            }),
        }
    }

    fn receipt() -> ReleaseReceipt {
        ReleaseReceipt {
            load_generation: 7,
            session_id: "pipeline".into(),
            members: vec![
                ReleaseMember {
                    request_id: "a".into(),
                    submission_event_id: "sent-a".into(),
                    sequence_id: 0,
                    incarnation: 3,
                    operation_id: 11,
                },
                ReleaseMember {
                    request_id: "b".into(),
                    submission_event_id: "sent-b".into(),
                    sequence_id: 1,
                    incarnation: 4,
                    operation_id: 12,
                },
            ],
        }
    }

    #[test]
    fn approved_output_binds_attempt_and_only_terminal_release_intent() {
        for terminal in [false, true] {
            let normal = output(terminal);
            normal.validate().unwrap();
            let bytes = serde_json::to_vec(&normal).unwrap();
            assert_eq!(
                serde_json::from_slice::<ApprovedOutputPayload>(&bytes).unwrap(),
                normal
            );
            for bad in ["", "old\0attempt"] {
                let mut invalid = normal.clone();
                invalid.submission_event_id = bad.into();
                assert!(invalid.validate().is_err());
            }
            let mut invalid = normal.clone();
            invalid.incarnation = 0;
            assert!(invalid.validate().is_err());
            for operation in [None, Some(0), Some(11)] {
                invalid = normal.clone();
                invalid.release_operation_id = operation;
                assert_eq!(
                    invalid.validate().is_ok(),
                    (terminal && operation == Some(11)) || (!terminal && operation.is_none())
                );
            }
        }
    }

    #[test]
    fn old_outcome_and_scalar_release_cannot_masquerade_as_current_evidence() {
        let legacy = serde_json::to_vec(&output(true).outcome).unwrap();
        assert!(serde_json::from_slice::<ApprovedOutputPayload>(&legacy).is_err());
        assert!(
            serde_json::from_str::<ReleaseReceipt>(
                r#"{"load_generation":7,"session_id":"pipeline","released":2}"#
            )
            .is_err()
        );
        let mut value = serde_json::to_value(receipt()).unwrap();
        value["released"] = 2.into();
        assert!(serde_json::from_value::<ReleaseReceipt>(value).is_err());
        let mut value = serde_json::to_value(receipt()).unwrap();
        value["members"][0]["aliases"] = "b".into();
        assert!(serde_json::from_value::<ReleaseReceipt>(value).is_err());
    }

    #[test]
    fn terminal_requires_strict_versioned_witness_and_nonterminal_forbids_it() {
        let normal = output(true);
        let mut missing = normal.clone();
        missing.issued_work = None;
        assert!(missing.validate().is_err());
        let mut nonterminal = output(false);
        nonterminal.issued_work = normal.issued_work;
        assert!(nonterminal.validate().is_err());
        for field in ["revision", "issue_count", "last_ordinal"] {
            let mut wire = serde_json::to_value(&normal).unwrap();
            wire["issued_work"][field] = 0.into();
            assert!(
                serde_json::from_value::<ApprovedOutputPayload>(wire)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        let mut unknown = serde_json::to_value(&normal).unwrap();
        unknown["unknown"] = true.into();
        assert!(serde_json::from_value::<ApprovedOutputPayload>(unknown).is_err());
        let mut unknown = serde_json::to_value(&normal).unwrap();
        unknown["issued_work"]["unknown"] = true.into();
        assert!(serde_json::from_value::<ApprovedOutputPayload>(unknown).is_err());
        for field in ["authority_digest", "digest"] {
            for length in [0, 31, 33] {
                let mut wire = serde_json::to_value(&normal).unwrap();
                wire["issued_work"][field] = serde_json::json!(vec![0u8; length]);
                assert!(serde_json::from_value::<ApprovedOutputPayload>(wire).is_err());
            }
        }
    }

    #[test]
    fn a_receipt_allows_multiple_exact_members_but_never_repeated_requests_or_slots() {
        let normal = receipt();
        normal.validate().unwrap();
        assert_eq!(
            serde_json::from_slice::<ReleaseReceipt>(&serde_json::to_vec(&normal).unwrap())
                .unwrap(),
            normal
        );
        let mut invalid = normal.clone();
        invalid.members[1].request_id = invalid.members[0].request_id.clone();
        assert!(invalid.validate().is_err());
        invalid = normal.clone();
        invalid.members[1].sequence_id = invalid.members[0].sequence_id;
        assert!(invalid.validate().is_err());
        invalid = normal;
        invalid.members.clear();
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn every_receipt_member_needs_canonical_request_attempt_and_nonzero_authority() {
        for index in [0, 1] {
            for field in ["request_id", "submission_event_id"] {
                for bad in ["", "bad\0identity"] {
                    let mut value = serde_json::to_value(receipt()).unwrap();
                    value["members"][index][field] = bad.into();
                    assert!(
                        serde_json::from_value::<ReleaseReceipt>(value)
                            .unwrap()
                            .validate()
                            .is_err()
                    );
                }
            }
            for field in ["incarnation", "operation_id"] {
                let mut value = serde_json::to_value(receipt()).unwrap();
                value["members"][index][field] = 0.into();
                assert!(
                    serde_json::from_value::<ReleaseReceipt>(value)
                        .unwrap()
                        .validate()
                        .is_err()
                );
            }
        }
        let mut invalid = receipt();
        invalid.load_generation = 0;
        assert!(invalid.validate().is_err());
        for session in ["", "s\0other"] {
            invalid = receipt();
            invalid.session_id = session.into();
            assert!(invalid.validate().is_err());
        }
    }
}
