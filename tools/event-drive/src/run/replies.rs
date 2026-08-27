use super::{NODE_RESULT, wire};
use p4_llamacpp_staged_adapter::v2::ERROR_CONTENT_TYPE;
use p4_protocol::event::{Endpoint, Event};
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub(super) async fn receive_exact<R, W>(
    wire: &mut wire::EventWire<R, W>,
    content_type: &str,
    expected: Vec<ExpectedReply>,
    correlation_id: &str,
    timeout_ms: u64,
) -> Result<(), Box<dyn std::error::Error>>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let mut remaining: HashMap<String, Endpoint> = expected
        .into_iter()
        .map(|item| (item.causation_id, item.source))
        .collect();
    while !remaining.is_empty() {
        let event = wire.receive(deadline).await?;
        if event.envelope.payload_content_type == ERROR_CONTENT_TYPE {
            return Err(event_error(&event).into());
        }
        if event.envelope.payload_content_type == content_type {
            consume_expected_reply(&event, content_type, correlation_id, &mut remaining)?;
            if content_type == NODE_RESULT {
                let result: serde_json::Value = serde_json::from_slice(&event.payload)?;
                if result.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
                    let detail = result
                        .get("detail")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("node control failed");
                    return Err(detail.to_owned().into());
                }
            }
        }
    }
    Ok(())
}

fn consume_expected_reply(
    event: &Event,
    content_type: &str,
    correlation_id: &str,
    remaining: &mut HashMap<String, Endpoint>,
) -> Result<(), String> {
    if event.envelope.correlation_id != correlation_id {
        return Err(format!(
            "{content_type} correlation mismatch: expected={correlation_id} actual={}",
            event.envelope.correlation_id
        ));
    }
    let causation = event
        .envelope
        .causation_id
        .as_ref()
        .ok_or_else(|| format!("{content_type} response omitted causation identity"))?;
    let source = remaining.remove(causation).ok_or_else(|| {
        format!("{content_type} response has duplicate or unknown causation_id={causation}")
    })?;
    if event.envelope.source != source {
        return Err(format!(
            "{content_type} source mismatch for causation_id={causation}: expected={source:?} actual={:?}",
            event.envelope.source
        ));
    }
    Ok(())
}

pub(super) struct ExpectedReply {
    causation_id: String,
    source: Endpoint,
}

impl ExpectedReply {
    pub(super) fn from_request(request: &Event) -> Self {
        Self {
            causation_id: request.envelope.event_id.clone(),
            source: request.envelope.target.clone(),
        }
    }
}

fn event_error(event: &Event) -> String {
    format!(
        "node event error source={:?} target={:?} event_id={} correlation_id={} causation_id={:?} payload={}",
        event.envelope.source,
        event.envelope.target,
        event.envelope.event_id,
        event.envelope.correlation_id,
        event.envelope.causation_id,
        String::from_utf8_lossy(&event.payload)
    )
}

#[cfg(test)]
mod tests {
    use super::super::Sender;
    use super::*;
    use p4_llamacpp_staged_adapter::v2::{LOAD_CONTENT_TYPE, LOADED_CONTENT_TYPE};
    use p4_protocol::Address;
    use p4_protocol::event::{EventClass, OuterEndpoint};

    #[test]
    fn node_error_retains_routing_identity_and_payload() {
        let outer = OuterEndpoint {
            ingress_agent: Address::tcp("127.0.0.1", 52003),
            channel: "diagnostic".into(),
            connection_generation: 1,
        };
        let mut sender = Sender::new(outer);
        let mut event = sender.event(
            Endpoint::Outer(sender.outer.clone()),
            EventClass::Output,
            ERROR_CONTENT_TYPE,
            br#"{"code":"failed"}"#.to_vec(),
            "load",
        );
        event.envelope.source = Endpoint::node(Address::tcp("127.0.0.1", 52004), "stage-1", 1);
        let error = event_error(&event);
        assert!(error.contains("stage-1"), "{error}");
        assert!(error.contains("correlation_id=load"), "{error}");
        assert!(error.contains("{\"code\":\"failed\"}"), "{error}");
    }

    fn expected_reply_fixture() -> (Event, Event, HashMap<String, Endpoint>) {
        let outer = OuterEndpoint {
            ingress_agent: Address::tcp("127.0.0.1", 52003),
            channel: "diagnostic".into(),
            connection_generation: 1,
        };
        let mut sender = Sender::new(outer.clone());
        let target = Endpoint::node(Address::tcp("127.0.0.1", 52004), "stage-1", 1);
        let request = sender.event(
            target.clone(),
            EventClass::Control,
            LOAD_CONTENT_TYPE,
            Vec::new(),
            "load",
        );
        let mut reply = request.clone();
        reply.envelope.source = target.clone();
        reply.envelope.target = Endpoint::Outer(outer);
        reply.envelope.causation_id = Some(request.envelope.event_id.clone());
        reply.envelope.payload_content_type = LOADED_CONTENT_TYPE.into();
        let remaining = HashMap::from([(request.envelope.event_id.clone(), target)]);
        (request, reply, remaining)
    }

    #[test]
    fn reply_identity_consumes_each_causation_once() {
        let (_request, reply, mut remaining) = expected_reply_fixture();
        assert_eq!(
            consume_expected_reply(&reply, LOADED_CONTENT_TYPE, "load", &mut remaining),
            Ok(())
        );
        assert!(remaining.is_empty());
        assert!(
            consume_expected_reply(&reply, LOADED_CONTENT_TYPE, "load", &mut remaining)
                .unwrap_err()
                .contains("duplicate or unknown")
        );
    }

    #[test]
    fn reply_identity_rejects_wrong_source_and_correlation() {
        let (_request, mut reply, mut remaining) = expected_reply_fixture();
        reply.envelope.source = Endpoint::node(Address::tcp("127.0.0.1", 52005), "stage-2", 1);
        assert!(
            consume_expected_reply(&reply, LOADED_CONTENT_TYPE, "load", &mut remaining)
                .unwrap_err()
                .contains("source mismatch")
        );

        let (_request, mut reply, mut remaining) = expected_reply_fixture();
        reply.envelope.correlation_id = "other".into();
        assert!(
            consume_expected_reply(&reply, LOADED_CONTENT_TYPE, "load", &mut remaining)
                .unwrap_err()
                .contains("correlation mismatch")
        );
    }
}
