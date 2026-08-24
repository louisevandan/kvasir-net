mod control;
mod transport;

use p4_agent_core::event_broker::{EventBroker, bounded_queue};
use p4_protocol::Address;
use std::sync::Arc;
use tokio::net::TcpListener;

const ROUTE_CAPACITY: usize = 65_536;
const DUPLICATE_WINDOW: usize = 262_144;

pub async fn run(listener: TcpListener, own: Address) -> Result<(), Box<dyn std::error::Error>> {
    let (agent_tx, agent_rx) = bounded_queue(ROUTE_CAPACITY);
    let (outer_tx, outer_rx) = bounded_queue(ROUTE_CAPACITY);
    let (outbound_tx, outbound_rx) = bounded_queue(ROUTE_CAPACITY);
    let broker = Arc::new(EventBroker::new(
        own.clone(),
        agent_tx,
        outer_tx,
        outbound_tx,
        DUPLICATE_WINDOW,
    ));
    let connections = transport::OuterConnections::default();

    tokio::spawn(control::run(own, Arc::clone(&broker), agent_rx));
    tokio::spawn(transport::deliver_outer(outer_rx, connections.clone()));
    tokio::spawn(transport::deliver_outbound(outbound_rx));
    tokio::spawn(transport::accept(
        listener,
        Arc::clone(&broker),
        connections,
    ));

    tokio::signal::ctrl_c().await?;
    Ok(())
}
