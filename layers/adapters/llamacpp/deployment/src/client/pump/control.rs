use super::PumpEvent;
use crate::contract::{Generation, SubmissionId};
use std::collections::HashSet;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

/// Cancel is idempotent and generation advances supersede older values, so
/// controls are coalesced behind one wake-up instead of appended to an
/// unbounded channel. The cancel set cannot exceed the client's outstanding
/// submission bound in the real relay path.
pub(super) struct ControlMailbox {
    sender: Sender<PumpEvent>,
    state: Mutex<State>,
    live: Arc<Mutex<HashSet<SubmissionId>>>,
}

#[derive(Default)]
struct State {
    cancels: HashSet<SubmissionId>,
    generation: Option<Generation>,
    wake_queued: bool,
}

pub(super) struct ControlBatch {
    pub(super) cancels: Vec<SubmissionId>,
    pub(super) generation: Option<Generation>,
}

impl ControlMailbox {
    pub(super) fn new(sender: Sender<PumpEvent>, live: Arc<Mutex<HashSet<SubmissionId>>>) -> Self {
        Self {
            sender,
            state: Mutex::new(State::default()),
            live,
        }
    }

    pub(super) fn cancel(&self, submission_id: SubmissionId) {
        if !self
            .live
            .lock()
            .expect("live submissions lock")
            .contains(&submission_id)
        {
            return;
        }
        let mut state = self.state.lock().expect("control mailbox lock");
        state.cancels.insert(submission_id);
        self.wake(&mut state);
    }

    pub(super) fn advance_generation(&self, generation: Generation) {
        let mut state = self.state.lock().expect("control mailbox lock");
        state.generation = Some(
            state
                .generation
                .map_or(generation, |old| old.max(generation)),
        );
        self.wake(&mut state);
    }

    pub(super) fn drain(&self) -> ControlBatch {
        let mut state = self.state.lock().expect("control mailbox lock");
        state.wake_queued = false;
        let mut cancels = state.cancels.drain().collect::<Vec<_>>();
        cancels.sort();
        ControlBatch {
            cancels,
            generation: state.generation.take(),
        }
    }

    fn wake(&self, state: &mut State) {
        if state.wake_queued {
            return;
        }
        state.wake_queued = true;
        if self.sender.send(PumpEvent::ControlsReady).is_err() {
            state.wake_queued = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    #[test]
    fn controls_coalesce_behind_one_wake_and_stay_bounded() {
        let (sender, receiver) = channel();
        let live = Arc::new(Mutex::new(
            (0..super::super::COMMAND_QUEUE_BOUND)
                .map(|index| format!("s-{index}"))
                .collect(),
        ));
        let mailbox = ControlMailbox::new(sender, live);
        for index in 0..10_000 {
            mailbox.cancel(format!("s-{index}"));
        }
        mailbox.advance_generation(4);
        mailbox.advance_generation(3);

        assert!(matches!(receiver.try_recv(), Ok(PumpEvent::ControlsReady)));
        assert!(receiver.try_recv().is_err(), "only one wake may be queued");
        let batch = mailbox.drain();
        assert_eq!(batch.cancels.len(), super::super::COMMAND_QUEUE_BOUND);
        assert_eq!(batch.generation, Some(4));
    }

    #[test]
    fn unknown_cancels_cannot_displace_a_live_cancel() {
        let (sender, receiver) = channel();
        let live = Arc::new(Mutex::new(HashSet::from(["live".to_string()])));
        let mailbox = ControlMailbox::new(sender, live);
        for index in 0..10_000 {
            mailbox.cancel(format!("unknown-{index}"));
        }
        mailbox.cancel("live".into());

        assert!(matches!(receiver.try_recv(), Ok(PumpEvent::ControlsReady)));
        assert_eq!(mailbox.drain().cancels, vec!["live"]);
    }
}
