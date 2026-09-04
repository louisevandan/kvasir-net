use super::*;
use crate::v2::commands::ErrorPayload;

impl Worker {
    pub(super) fn emit_batch_errors(
        &mut self,
        bases: &[Event],
        code: &str,
        detail: &str,
    ) -> Result<(), ()> {
        for base in bases {
            self.emit_error(base, code, detail.to_owned())?;
        }
        Ok(())
    }

    pub(super) fn emit_reply_json<T: Serialize>(
        &mut self,
        base: &Event,
        reply: ReplySpec,
        ingress: Address,
        class: EventClass,
        content_type: &str,
        value: &T,
    ) -> Result<(), ()> {
        let payload = serde_json::to_vec(value).map_err(|_| ())?;
        let sequence = self.state.next_event;
        self.state.next_event = self.state.next_event.checked_add(1).ok_or(())?;
        let target = Endpoint::outer(ingress, reply.channel.clone(), reply.connection_generation);
        let mut envelope = base.envelope.next(
            derived_event_id(base, sequence),
            self.endpoint.clone(),
            target,
            class,
            sequence,
            content_type,
        );
        envelope.correlation_id = reply.correlation_id;
        envelope.return_route = match &envelope.target {
            Endpoint::Outer(route) => Some(route.clone()),
            _ => unreachable!(),
        };
        envelope.deadline_unix_ms = reply.deadline_unix_ms;
        self.publish_or_wait(Event { envelope, payload })
    }

    pub(super) fn emit_error(
        &mut self,
        base: &Event,
        code: &str,
        detail: String,
    ) -> Result<(), ()> {
        self.emit_json(
            base,
            reply_target(base),
            EventClass::Output,
            ERROR_CONTENT_TYPE,
            &ErrorPayload {
                code: code.into(),
                detail,
            },
        )
    }

    pub(super) fn emit_json<T: Serialize>(
        &mut self,
        base: &Event,
        target: Endpoint,
        class: EventClass,
        content_type: &str,
        value: &T,
    ) -> Result<(), ()> {
        let payload = serde_json::to_vec(value).map_err(|_| ())?;
        self.emit_bytes(base, target, class, content_type, payload)
    }

    pub(super) fn emit_bytes(
        &mut self,
        base: &Event,
        target: Endpoint,
        class: EventClass,
        content_type: &str,
        payload: Vec<u8>,
    ) -> Result<(), ()> {
        let sequence = self.state.next_event;
        self.state.next_event = self.state.next_event.checked_add(1).ok_or(())?;
        let envelope = base.envelope.next(
            derived_event_id(base, sequence),
            self.endpoint.clone(),
            target,
            class,
            sequence,
            content_type,
        );
        self.publish_or_wait(Event { envelope, payload })
    }

    /// Publishes a completion, waiting for room rather than dropping it.
    ///
    /// The other direction of this pipe learned to hold an event when the
    /// adapter was full; this one used to discard the completion and fail the
    /// worker, which loses a token that has already been computed and ends
    /// the node for a queue that was about to drain. A closed mailbox is
    /// still fatal - nothing will ever read it - and only that is.
    ///
    /// This runs on the worker's own thread, which owns no lock and holds no
    /// llama context between events, so blocking here backs the pressure up
    /// to the node's inbound queue rather than into the stage server.
    fn publish_or_wait(&mut self, event: Event) -> Result<(), ()> {
        let mut pending = event;
        loop {
            match self.publisher.try_publish(pending) {
                Ok(()) => return Ok(()),
                Err(PublishError::Full(event)) => {
                    if self.shutting_down.load(Ordering::SeqCst) {
                        // The reader is going away, so the room this is
                        // waiting for will never come. Abandoning the
                        // completion loses a token; waiting for it hangs the
                        // shutdown, which loses the node.
                        self.set_snapshot("completion_queue_full:abandoned_at_shutdown");
                        return Err(());
                    }
                    self.set_snapshot("completion_queue_full:waiting");
                    pending = event;
                    std::thread::sleep(COMPLETION_RETRY_INTERVAL);
                }
                Err(PublishError::Closed(_)) => {
                    self.set_snapshot("completion_queue_closed");
                    return Err(());
                }
            }
        }
    }
}

fn derived_event_id(base: &Event, sequence: u64) -> String {
    format!("{}:llamacpp:{sequence}", base.envelope.event_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use p4_adapter::node_adapter::{Poll, completion_mailbox};
    use p4_protocol::Address;
    use p4_protocol::event::{Envelope, EventClass, OuterEndpoint};

    fn input(id: &str) -> Event {
        let address = Address::tcp("127.0.0.1", 1);
        Event {
            envelope: Envelope {
                protocol_version: Envelope::VERSION,
                event_id: id.into(),
                correlation_id: "same-correlation".into(),
                causation_id: None,
                source: Endpoint::agent(address.clone()),
                target: Endpoint::agent(address),
                return_route: None,
                class: EventClass::Control,
                sequence: 1,
                deadline_unix_ms: None,
                adapter_kind: Some("llamacpp".into()),
                payload_content_type: "test".into(),
            },
            payload: Vec::new(),
        }
    }

    #[test]
    fn different_causal_events_cannot_generate_the_same_completion_id() {
        assert_ne!(
            derived_event_id(&input("node-a-load"), 1),
            derived_event_id(&input("node-b-load"), 1)
        );
    }

    #[test]
    fn one_failed_mixed_batch_reports_every_participating_request() {
        let address = Address::tcp("127.0.0.1", 1);
        let (_sender, receiver) = mpsc::sync_channel(1);
        let (publisher, mailbox) = completion_mailbox(2);
        let mut worker = Worker::new(
            Endpoint::node(address.clone(), "node", 1),
            receiver,
            publisher,
            Arc::new(Mutex::new("loaded".into())),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let mut first = input("request-a");
        first.envelope.return_route = Some(OuterEndpoint {
            ingress_agent: address.clone(),
            channel: "outer-a".into(),
            connection_generation: 1,
        });
        let mut second = input("request-b");
        second.envelope.return_route = Some(OuterEndpoint {
            ingress_agent: address,
            channel: "outer-b".into(),
            connection_generation: 2,
        });

        assert_eq!(
            worker.emit_batch_errors(
                &[first, second],
                "LLAMA_LOGICAL_BATCH_FAILED",
                "memory_dirty=1;action=reload",
            ),
            Ok(())
        );
        let Poll::Event(first_error) = mailbox.try_take() else {
            panic!("first request did not receive its error event");
        };
        let Poll::Event(second_error) = mailbox.try_take() else {
            panic!("second request did not receive its error event");
        };
        assert_ne!(first_error.envelope.target, second_error.envelope.target);
        assert!(first_error.envelope.event_id.starts_with("request-a:"));
        assert!(second_error.envelope.event_id.starts_with("request-b:"));
        assert_eq!(first_error.payload, second_error.payload);
    }
}
