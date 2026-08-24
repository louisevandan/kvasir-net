use p4_agent_core::event_broker::{EventBroker, EventReceiver};
use p4_protocol::Address;
use p4_protocol::event::{Endpoint, Event, OuterEndpoint, decode, encode};
use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, mpsc};

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
            Ok((stream, _)) => {
                let broker = Arc::clone(&broker);
                let connections = connections.clone();
                tokio::spawn(async move {
                    if let Err(error) = serve(stream, broker, connections).await {
                        eprintln!("P4_EVENT_CONNECTION_STOPPED error={error}");
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

pub async fn deliver_outer(mut receiver: EventReceiver, connections: OuterConnections) {
    while let Some(event) = receiver.recv().await {
        let Endpoint::Outer(target) = event.envelope.target.clone() else {
            continue;
        };
        let sender = connections.0.lock().await.get(&target).cloned();
        match sender {
            Some(sender) if sender.send(event).await.is_ok() => {}
            _ => eprintln!("P4_EVENT_OUTER_MISSING target={target:?}"),
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
