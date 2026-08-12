use super::startup::parse_agent_options_from;
use super::*;
use crate::foundation::transport::{P4Handler, ResponseCollector, ResponseSink, Result};
use p4_protocol::Message;
use std::sync::Arc;

struct DirectNode;

impl P4Handler for DirectNode {
    fn handle(&self, message: Message, responses: &mut dyn ResponseSink) -> Result<()> {
        let Message::Execute(request) = message else {
            unreachable!("controller local path sends EXECUTE")
        };
        responses.emit(Message::Done(p4_protocol::ExecutionDone {
            controller_id: request.controller_id,
            node_id: request.node_id,
            request_id: request.request_id,
            session_id: request.session_id,
            reason: "stop".into(),
            generated_tokens: 1,
        }))
    }
}

#[test]
fn agent_startup_accepts_default_or_explicit_workers() {
    let default = parse_agent_options_from(&["127.0.0.1:29017".into()]).unwrap();
    assert_eq!(default.listen, "127.0.0.1:29017");
    assert_eq!(default.workers, None);
    let explicit =
        parse_agent_options_from(&["127.0.0.1:29017".into(), "--workers".into(), "48".into()])
            .unwrap();
    assert_eq!(explicit.workers, Some(48));
    assert!(parse_agent_options_from(&["a".into(), "--workers".into(), "0".into()]).is_err());
    assert!(parse_agent_options_from(&["a".into(), "--workers".into(), "1025".into()]).is_err());
    assert!(parse_agent_options_from(&["a".into(), "unexpected".into()]).is_err());
}

#[test]
fn controller_and_node_share_the_in_memory_transport_contract() {
    let controller = ControllerProcessor::local(Arc::new(DirectNode));
    let mut responses = ResponseCollector::new();
    controller
        .handle(
            Message::IngressSubmit {
                controller_id: "controller-a".into(),
                ingress_id: "ingress-a".into(),
                request_id: "request-a".into(),
                session_id: String::new(),
                node_id: "node-a".into(),
                deployment_id: "deployment-a".into(),
                binding_id: "binding-a".into(),
                runtime_generation: 1,
                max_tokens: 1,
                temperature: 0.7,
                prompt: "hello".into(),
                options: "{}".into(),
            },
            &mut responses,
        )
        .unwrap();
    assert!(matches!(
        responses.into_messages().as_slice(),
        [Message::IngressAccepted { .. }, Message::Done(_)]
    ));
}
