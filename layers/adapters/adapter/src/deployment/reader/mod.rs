//! The structural oracle: given every event recorded for one submission, in
//! delivery order, does the ordering itself obey the contract -- regardless
//! of which client, which backend, or which language produced it.
//!
//! This is deliberately blind to content. It never reads `text` or a
//! `reason`'s value; every check here is about the shape of the stream
//! itself. Mirrored exactly by the TypeScript twin at
//! packages/llama_domain/src/common/protocol/pipeline-submission/reader.ts,
//! which checks the same five things against the same wire events.

use super::event::DeploymentEvent;

/// One way a recorded event stream can violate the contract.
///
/// Distinguished rather than collapsed into a single error, because the
/// acceptance bar for this reader is proving it catches each kind on its
/// own, not merely that it rejects something.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Violation {
    /// `Produced.event_ordinal` was not the next value contiguity from zero
    /// required.
    OrdinalGap { expected: u64, found: u64 },
    /// A `Produced` arrived before this submission's `Accepted`.
    ProducedBeforeAccepted,
    /// `Accepted` and `Rejected` both appear for the same submission.
    AcceptedAndRejectedBothPresent,
    /// A second `Settled` was recorded.
    SettledMoreThanOnce,
    /// Some event was recorded after `Settled`, other than a second
    /// `Settled` itself -- see `SettledMoreThanOnce` for that case.
    EventAfterSettled,
}

/// Replays one submission's recorded events in order and reports the first
/// way they fail to hold the contract, or `Ok(())` if they hold it fully.
///
/// `events` must already be scoped to a single `submission_id`; this
/// function does not check that they share one, because whether two streams
/// belong to the same submission is a fact about how they were recorded, not
/// about their shape.
pub fn check(events: &[DeploymentEvent]) -> Result<(), Violation> {
    let mut accepted = false;
    let mut rejected = false;
    let mut settled = false;
    let mut next_ordinal = 0u64;

    for event in events {
        if settled {
            return Err(match event {
                DeploymentEvent::Settled(_) => Violation::SettledMoreThanOnce,
                _ => Violation::EventAfterSettled,
            });
        }
        match event {
            DeploymentEvent::Accepted(_) => {
                if rejected {
                    return Err(Violation::AcceptedAndRejectedBothPresent);
                }
                accepted = true;
            }
            DeploymentEvent::Rejected(_) => {
                if accepted {
                    return Err(Violation::AcceptedAndRejectedBothPresent);
                }
                rejected = true;
            }
            DeploymentEvent::Produced(produced) => {
                if !accepted {
                    return Err(Violation::ProducedBeforeAccepted);
                }
                if produced.event_ordinal != next_ordinal {
                    return Err(Violation::OrdinalGap {
                        expected: next_ordinal,
                        found: produced.event_ordinal,
                    });
                }
                next_ordinal += 1;
            }
            DeploymentEvent::Settled(_) => {
                settled = true;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
