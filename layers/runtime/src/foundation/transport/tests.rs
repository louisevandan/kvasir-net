use super::*;

struct Echo;

impl P4Handler for Echo {
    fn handle(&self, message: Message, responses: &mut dyn ResponseSink) -> Result<()> {
        responses.emit(message)
    }
}

#[test]
fn in_memory_transport_uses_the_same_handler_contract() {
    let transport = in_memory(Arc::new(Echo));
    let mut responses = ResponseCollector::new();
    transport
        .dispatch(
            Message::Error {
                request_id: "request".into(),
                detail: "expected".into(),
            },
            &mut responses,
        )
        .unwrap();
    assert!(matches!(responses.terminal(), Some(Message::Error { .. })));
}
