//! Forwards one command to an adapter while streaming its responses through
//! to the caller and retaining the terminal one for the Agent's own decision.

use crate::foundation::transport::{ResponseSink, Result, SharedTransport, terminal};
use p4_protocol::Message;

/// Dispatches `message` and returns the terminal response. Every response is
/// still forwarded downstream in order; the capture only lets the Agent decide
/// whether to record a slot or binding after the adapter has spoken.
pub(crate) fn capture(
    responses: &mut dyn ResponseSink,
    transport: &SharedTransport,
    message: Message,
) -> Result<Message> {
    let mut forwarded = CaptureSink {
        downstream: responses,
        terminal: None,
    };
    transport.dispatch(message, &mut forwarded)?;
    forwarded
        .terminal
        .ok_or_else(|| "P4 transport completed without a terminal response".into())
}

struct CaptureSink<'a> {
    downstream: &'a mut dyn ResponseSink,
    terminal: Option<Message>,
}

impl ResponseSink for CaptureSink<'_> {
    fn emit(&mut self, message: Message) -> Result<()> {
        if terminal(&message) {
            self.terminal = Some(message.clone());
        }
        self.downstream.emit(message)
    }
}

#[cfg(test)]
mod tests;
