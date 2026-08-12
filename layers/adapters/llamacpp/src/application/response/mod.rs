use p4_protocol::{Message, RoutedMessage};
use std::sync::mpsc::SyncSender;

#[derive(Clone)]
pub(crate) struct RouteResponder {
    route_id: String,
    deadline_unix_ms: u64,
    outbound: SyncSender<RoutedMessage>,
}

impl RouteResponder {
    pub(crate) fn new(
        route_id: String,
        deadline_unix_ms: u64,
        outbound: SyncSender<RoutedMessage>,
    ) -> Self {
        Self {
            route_id,
            deadline_unix_ms,
            outbound,
        }
    }

    pub(crate) fn emit(&self, message: Message) -> Result<(), Box<dyn std::error::Error>> {
        self.outbound
            .send(RoutedMessage {
                route_id: self.route_id.clone(),
                deadline_unix_ms: self.deadline_unix_ms,
                message,
            })
            .map_err(|_| "P4 llama.cpp response route is closed".into())
    }
}
