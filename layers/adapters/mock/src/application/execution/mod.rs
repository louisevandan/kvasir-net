//! The simulated generation of one request.
//!
//! Emits `TOKEN` at the declared cadence and exactly one terminal frame. Token
//! text is derived from the request id and index, so a caller can prove it
//! received this request's stream and not another's -- the check that catches
//! a routing fault rather than merely a slow one.

use crate::domain::profile::{Fault, Profile};
use p4_protocol::{ExecutionDone, ExecutionRequest, ExecutionToken, Message};
use std::time::Duration;
use tokio::sync::mpsc;

type AsyncError = Box<dyn std::error::Error + Send + Sync>;

/// Runs one request to its terminal frame.
///
/// Cancellation is not handled here. A cancelled route is dropped by the
/// listener, which stops delivering this stream; making the simulator aware of
/// cancellation would let it paper over a route P4 failed to remove.
pub(crate) async fn run(
    request: ExecutionRequest,
    profile: Profile,
    responses: &mpsc::Sender<Message>,
) -> Result<(), AsyncError> {
    if profile.fault == Fault::Hang {
        // Never answers. The deadline or the cancellation has to end this, and
        // whether it does is a fact about P4.
        std::future::pending::<()>().await;
    }
    sleep(profile.prefill).await;
    let total = profile.token_count(request.max_tokens);
    for index in 0..total {
        if profile.fault == Fault::AfterTokens(index) {
            return send(
                responses,
                Message::Error {
                    request_id: request.request_id.clone(),
                    detail: format!("mock deployment was asked to fail after {index} tokens"),
                },
            )
            .await;
        }
        if index > 0 {
            sleep(profile.token).await;
        }
        send(responses, token(&request, index)).await?;
    }
    send(
        responses,
        Message::Done(ExecutionDone {
            controller_id: request.controller_id.clone(),
            node_id: request.node_id.clone(),
            request_id: request.request_id.clone(),
            session_id: request.session_id.clone(),
            reason: "stop".into(),
            generated_tokens: total,
        }),
    )
    .await
}

fn token(request: &ExecutionRequest, index: u32) -> Message {
    Message::Token(ExecutionToken {
        controller_id: request.controller_id.clone(),
        node_id: request.node_id.clone(),
        request_id: request.request_id.clone(),
        session_id: request.session_id.clone(),
        phase: request.phase.clone(),
        position: request.position + index,
        index,
        text: format!("{}#{index} ", request.request_id),
    })
}

async fn send(responses: &mpsc::Sender<Message>, message: Message) -> Result<(), AsyncError> {
    responses
        .send(message)
        .await
        .map_err(|_| "mock response channel closed".into())
}

async fn sleep(duration: Duration) {
    if !duration.is_zero() {
        tokio::time::sleep(duration).await;
    }
}
