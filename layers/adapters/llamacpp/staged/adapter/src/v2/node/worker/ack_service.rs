//! Narrow ACK service while one unchanged outbound Event waits for capacity.
//! This does not resume native work, recursively dispatch events, or bypass a
//! non-ACK at the input head. Persistent input/diagnostic storage adds only one
//! held input and one deferred error; this is not a retained-byte or RSS bound.
use super::*;

impl Worker {
    /// Inspect at most one already accepted input. The two ACK consumers must
    /// finish all fallible interpretation before their first state write and
    /// must not publish, flush effects, call native, or drive another batch.
    pub(super) fn service_blocked_ack(&mut self) -> Result<(), ()> {
        if self.effects_fenced
            || self.state.prepared_issue.as_ref().is_some_and(|issue| {
                issue.progress == super::super::state::IssueProgress::Uncertain
            })
        {
            return Err(());
        }
        if self.held_input.is_some() {
            return Ok(());
        }
        let event = match self.receiver.try_recv() {
            Ok(WorkerInput::Event(event)) => event,
            // Input EOF is not permission to discard the active completion.
            // The existing publisher/shutdown path owns that lifetime.
            Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => {
                return Ok(());
            }
        };
        let result = match event.envelope.payload_content_type.as_str() {
            RELEASED_CONTENT_TYPE => self.released_without_flush(&event),
            SETTLED_CONTENT_TYPE => self.settled(&event),
            _ => {
                self.held_input = Some(event);
                #[cfg(test)]
                self.observe_issue_state("blocked_non_ack_held");
                return Ok(());
            }
        };
        if let Err(detail) = result {
            // A pending error must not prevent a valid ACK from retiring its
            // already reserved obligation, so classify through the real ACK
            // consumer before deciding whether this diagnostic slot is full.
            if self.deferred_ack_error.is_some() || self.ensure_event_id_obligations(1, 0).is_err()
            {
                // Preserve the exact second invalid/unschedulable input and
                // stop reading past it. Never overwrite the first diagnostic.
                self.held_input = Some(event);
                return Ok(());
            }
            self.deferred_ack_error = Some((event.envelope, detail));
        }
        Ok(())
    }
}
