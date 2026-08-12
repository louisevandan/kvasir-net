use super::capture;
use crate::foundation::transport::{
    P4Handler, ResponseCollector, ResponseSink, Result, in_memory,
};
use p4_protocol::Message;
use std::sync::Arc;

struct Scripted(Vec<Message>);

impl P4Handler for Scripted {
    fn handle(&self, _message: Message, responses: &mut dyn ResponseSink) -> Result<()> {
        for message in &self.0 {
            responses.emit(message.clone())?;
        }
        Ok(())
    }
}

fn progress() -> Message {
    Message::LoadProgress {
        operation_id: "op-a".into(),
        node_id: "node-a".into(),
        percent: 50,
        detail: "half".into(),
    }
}

fn bound() -> Message {
    Message::ModelBound {
        operation_id: "op-a".into(),
        node_id: "node-a".into(),
        deployment_id: "deployment-a".into(),
        binding_id: "binding-a".into(),
        runtime_generation: 2,
        state: "ready".into(),
        detail: "ready".into(),
    }
}

#[test]
fn every_response_is_forwarded_in_order_and_the_terminal_is_returned() {
    let transport = in_memory(Arc::new(Scripted(vec![progress(), bound()])));
    let mut collected = ResponseCollector::new();
    let terminal = capture(&mut collected, &transport, progress()).unwrap();

    assert!(matches!(terminal, Message::ModelBound { .. }));
    let messages = collected.into_messages();
    assert_eq!(messages.len(), 2);
    assert!(matches!(messages[0], Message::LoadProgress { .. }));
    assert!(matches!(messages[1], Message::ModelBound { .. }));
}

#[test]
fn an_error_counts_as_terminal() {
    let error = Message::Error {
        request_id: "op-a".into(),
        detail: "backend refused".into(),
    };
    let transport = in_memory(Arc::new(Scripted(vec![error])));
    let mut collected = ResponseCollector::new();
    assert!(matches!(
        capture(&mut collected, &transport, progress()).unwrap(),
        Message::Error { .. }
    ));
}

#[test]
fn a_stream_without_a_terminal_response_is_an_error() {
    let transport = in_memory(Arc::new(Scripted(vec![progress()])));
    let mut collected = ResponseCollector::new();
    assert!(capture(&mut collected, &transport, progress()).is_err());
}
