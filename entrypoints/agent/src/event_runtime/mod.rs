mod adapters;
mod control;
mod transport;

use p4_adapter::node_adapter::{
    CompletionMailbox, CompletionPublisher, OwnedPoll, RetainedCompletion,
    completion_mailbox_with_limits,
};
use p4_agent_core::event_broker::{DispatchError, RetainedDispatchFailure, RetainedEventBroker};
use p4_protocol::Address;
use std::sync::Arc;
use tokio::net::TcpListener;

const ROUTE_CAPACITY: usize = 65_536;
const DUPLICATE_WINDOW: usize = 262_144;

#[derive(Clone, Copy)]
struct RuntimeLimits {
    queue: usize,
    retained: usize,
    bytes: usize,
    connections: usize,
    hop_receipts: usize,
    hop_receipt_bytes: usize,
    hop_outstanding: usize,
}

impl RuntimeLimits {
    fn configured() -> Result<Self, Box<dyn std::error::Error>> {
        let bytes = match std::env::var("P4_EVENT_RETAINED_BYTES") {
            Ok(value) => value.parse::<usize>()?,
            Err(std::env::VarError::NotPresent) => 256 * 1024 * 1024,
            Err(error) => return Err(error.into()),
        };
        if bytes == 0 {
            return Err("P4_EVENT_RETAINED_BYTES must be positive".into());
        }
        let hop_receipts = positive_env("P4_EVENT_HOP_RECEIPTS", ROUTE_CAPACITY)?;
        let hop_receipt_bytes = positive_env("P4_EVENT_HOP_RECEIPT_BYTES", 64 * 1024 * 1024)?;
        let hop_outstanding = positive_env("P4_EVENT_HOP_OUTSTANDING", 256)?;
        u32::try_from(hop_outstanding)
            .map_err(|_| "P4_EVENT_HOP_OUTSTANDING exceeds protocol range")?;
        Ok(Self {
            queue: ROUTE_CAPACITY,
            retained: ROUTE_CAPACITY,
            bytes,
            connections: 256,
            hop_receipts,
            hop_receipt_bytes,
            hop_outstanding,
        })
    }
    fn mailbox(self) -> (CompletionPublisher, Arc<CompletionMailbox>) {
        completion_mailbox_with_limits(self.queue, self.retained, self.bytes)
            .expect("validated event mailbox limits/allocation")
    }
}

fn positive_env(name: &str, default: usize) -> Result<usize, Box<dyn std::error::Error>> {
    let value = match std::env::var(name) {
        Ok(value) => value.parse::<usize>()?,
        Err(std::env::VarError::NotPresent) => default,
        Err(error) => return Err(error.into()),
    };
    if value == 0 {
        return Err(format!("{name} must be positive").into());
    }
    Ok(value)
}

async fn next(receiver: &CompletionMailbox) -> Option<RetainedCompletion> {
    match std::future::poll_fn(|cx| receiver.poll_take_owned(cx)).await {
        OwnedPoll::Event(event) => Some(event),
        OwnedPoll::Closed => None,
        OwnedPoll::Empty => unreachable!("mailbox registers a wake for empty input"),
    }
}

async fn dispatch(
    broker: &RetainedEventBroker,
    mut event: RetainedCompletion,
) -> Result<(), RetainedDispatchFailure> {
    loop {
        match broker.dispatch_retained(event) {
            Ok(_) => return Ok(()),
            Err(failure) if matches!(failure.error, DispatchError::Full(_)) => {
                event = *failure.completion;
                // Same bounded retry contract as EventNode. No broker lock or
                // destination reservation survives this transport-owned wait.
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
            Err(failure) => return Err(failure),
        }
    }
}

struct Runtime {
    control: tokio::task::JoinHandle<control::Remainder>,
    transport: transport::Owner,
}

impl Runtime {
    fn start(listener: TcpListener, own: Address, limits: RuntimeLimits) -> Self {
        let (agent_tx, agent_rx) = limits.mailbox();
        let (outer_tx, outer_rx) = limits.mailbox();
        let (outbound_tx, outbound_rx) = limits.mailbox();
        let broker = Arc::new(RetainedEventBroker::new(
            own.clone(),
            agent_tx,
            outer_tx,
            outbound_tx,
            DUPLICATE_WINDOW,
        ));
        let transport =
            transport::Owner::start(listener, Arc::clone(&broker), outer_rx, outbound_rx, limits);
        let control = tokio::spawn(control::run(
            own,
            broker,
            agent_rx,
            limits,
            transport.inspector(),
        ));
        Self { control, transport }
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        // Explicit runtime teardown is local abandonment, not graceful drain
        // or remote receiver acknowledgement.
        self.control.abort();
        self.transport.abort();
    }
}

pub async fn run(listener: TcpListener, own: Address) -> Result<(), Box<dyn std::error::Error>> {
    let limits = RuntimeLimits::configured()?;
    eprintln!(
        "P4_EVENT_RETAINED_LIMITS queue={} retained={} bytes_per_store={} connections={} hop_receipts={} hop_receipt_bytes={} hop_outstanding={}",
        limits.queue,
        limits.retained,
        limits.bytes,
        limits.connections,
        limits.hop_receipts,
        limits.hop_receipt_bytes,
        limits.hop_outstanding
    );
    let _runtime = Runtime::start(listener, own, limits);
    tokio::signal::ctrl_c().await?;
    Ok(())
}

#[cfg(test)]
mod tests;
