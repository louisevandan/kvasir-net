use p4_agent_core::event_broker::{EventBroker, EventReceiver};
use p4_protocol::Address;
use p4_protocol::event::{Endpoint, Event, OuterEndpoint, decode, encode};
use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, mpsc};

/// Serial number for accepted connections, so log lines can name one.
static CONNECTIONS: AtomicU64 = AtomicU64::new(1);

const CONNECTION_CAPACITY: usize = 65_536;
const MAX_FRAME: usize = 2 * 1024 * 1024 * 1024;
type ConnectionSender = mpsc::Sender<Event>;

#[derive(Clone, Default)]
pub struct OuterConnections(Arc<Mutex<HashMap<OuterEndpoint, ConnectionSender>>>);

pub async fn accept(
    listener: TcpListener,
    broker: Arc<EventBroker>,
    connections: OuterConnections,
) {
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                let broker = Arc::clone(&broker);
                let connections = connections.clone();
                // Named so a connection that ends mid-run can be told from
                // the one still carrying the run: a 2026-09-01 four-node run
                // lost output to a close that no log line could attribute.
                let id = CONNECTIONS.fetch_add(1, Ordering::Relaxed);
                eprintln!("P4_EVENT_CONNECTION_OPENED connection={id} peer={peer}");
                tokio::spawn(async move {
                    let result = serve(stream, broker, connections).await;
                    match result {
                        Ok(()) => eprintln!("P4_EVENT_CONNECTION_STOPPED connection={id} peer={peer} error=none"),
                        Err(error) => eprintln!("P4_EVENT_CONNECTION_STOPPED connection={id} peer={peer} error={error}"),
                    }
                });
            }
            Err(error) => eprintln!("P4_EVENT_ACCEPT_FAILED error={error}"),
        }
    }
}

async fn serve(
    stream: TcpStream,
    broker: Arc<EventBroker>,
    connections: OuterConnections,
) -> io::Result<()> {
    stream.set_nodelay(true)?;
    let (mut reader, writer) = stream.into_split();
    let (sender, receiver) = mpsc::channel(CONNECTION_CAPACITY);
    tokio::spawn(write_loop(writer, receiver));
    loop {
        let event = read_event(&mut reader).await?;
        if let Endpoint::Outer(route) = &event.envelope.source {
            connections
                .0
                .lock()
                .await
                .insert(route.clone(), sender.clone());
        }
        broker.dispatch(event).map_err(io::Error::other)?;
    }
}

/// Writes events to the OUTER that owns each target endpoint.
///
/// An undeliverable event is *lost*, and the loss is visible to nobody but
/// this log line unless it is counted: a 2026-09-01 four-node run under
/// continuous arrivals dropped 24 consecutive Output events here and the
/// only reason anyone noticed was the drive's own position-contiguity check.
/// So each discard is counted per endpoint and reported as a running total,
/// and a route whose write side has gone is evicted rather than left in the
/// map to swallow everything addressed to it until the OUTER happens to send
/// something that re-registers it.
///
/// Counting is not delivery. Whether P4 owes an OUTER its output across a
/// broken connection is a contract question this layer cannot settle alone;
/// see the restructure plan's open surface.
pub async fn deliver_outer(mut receiver: EventReceiver, connections: OuterConnections) {
    let mut discarded: HashMap<OuterEndpoint, u64> = HashMap::new();
    while let Some(event) = receiver.recv().await {
        let Endpoint::Outer(target) = event.envelope.target.clone() else {
            continue;
        };
        let sender = connections.0.lock().await.get(&target).cloned();
        let delivered = match sender {
            Some(sender) => {
                let sent = sender.send(event).await.is_ok();
                if !sent {
                    // The write loop behind this sender is gone. Leaving the
                    // entry keeps every later event going to a closed channel.
                    connections.0.lock().await.remove(&target);
                }
                sent
            }
            None => false,
        };
        if !delivered {
            let total = discarded.entry(target.clone()).or_insert(0);
            *total += 1;
            eprintln!("P4_EVENT_OUTER_MISSING discarded={total} target={target:?}");
        }
    }
}

pub async fn deliver_outbound(mut receiver: EventReceiver) {
    let mut peers: HashMap<Address, ConnectionSender> = HashMap::new();
    while let Some(event) = receiver.recv().await {
        let target = event.envelope.target.agent_address().clone();
        let mut sender = peers.get(&target).cloned();
        if sender.is_none() {
            sender = connect(&target).await.ok();
            if let Some(value) = &sender {
                peers.insert(target.clone(), value.clone());
            }
        }
        let delivered = match sender {
            Some(sender) => sender.send(event).await.is_ok(),
            None => false,
        };
        if !delivered {
            peers.remove(&target);
            eprintln!("P4_EVENT_PEER_DELIVERY_FAILED target={target}");
        }
    }
}

async fn connect(address: &Address) -> io::Result<ConnectionSender> {
    let stream = TcpStream::connect((address.host.as_str(), address.port)).await?;
    stream.set_nodelay(true)?;
    let (_reader, writer) = stream.into_split();
    let (sender, receiver) = mpsc::channel(CONNECTION_CAPACITY);
    tokio::spawn(write_loop(writer, receiver));
    Ok(sender)
}

async fn write_loop<W: AsyncWrite + Unpin>(mut writer: W, mut receiver: mpsc::Receiver<Event>) {
    while let Some(event) = receiver.recv().await {
        if let Err(error) = write_event(&mut writer, &event).await {
            eprintln!("P4_EVENT_WRITE_FAILED error={error}");
            break;
        }
    }
}

async fn read_event<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Event> {
    let size = reader.read_u32_le().await? as usize;
    if size == 0 || size > MAX_FRAME {
        return Err(io::Error::other("invalid event frame size"));
    }
    let mut bytes = vec![0; size];
    reader.read_exact(&mut bytes).await?;
    decode(&bytes).map_err(io::Error::other)
}

async fn write_event<W: AsyncWrite + Unpin>(writer: &mut W, event: &Event) -> io::Result<()> {
    let bytes = encode(event).map_err(io::Error::other)?;
    let size = u32::try_from(bytes.len()).map_err(|_| io::Error::other("event frame too large"))?;
    writer.write_u32_le(size).await?;
    writer.write_all(&bytes).await?;
    writer.flush().await
}
