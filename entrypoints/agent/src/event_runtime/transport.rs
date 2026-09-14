//! Actual socket pumps retain the same Event allocation through local writes.
//! A completed write is not remote acceptance; failed writes are never replayed.
use super::{RuntimeLimits, next};
use p4_adapter::node_adapter::{CompletionMailbox, RetainedCompletion};
use p4_agent_core::event_broker::{DispatchError, DispatchOutcome, RetainedEventBroker};
use p4_protocol::Address;
use p4_protocol::event::{Endpoint, Event, OuterEndpoint, decode, encode};
use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::sync::{Arc, Mutex as StdMutex};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore, mpsc};
use tokio::task::JoinSet;

static CONNECTIONS: AtomicU64 = AtomicU64::new(1);
const MAX_FRAME: usize = 2 * 1024 * 1024 * 1024;
type ConnectionSender = mpsc::Sender<RetainedCompletion>;
type Connections = Arc<Mutex<HashMap<OuterEndpoint, Option<ConnectionSender>>>>;

struct WriteFailure {
    error: io::Error,
    // False means encoding failed before touching the socket. True remains
    // uncertain even if the peer happened to accept the whole frame.
    started: bool,
    current: RetainedCompletion,
    pending: mpsc::Receiver<RetainedCompletion>,
}

enum Failure {
    Writer { value: WriteFailure, _slot: Arc<OwnedSemaphorePermit> },
    Ingress { error: String, event: Event, _slot: Arc<OwnedSemaphorePermit> },
    Undelivered { error: String, event: RetainedCompletion },
}

struct Shared {
    tasks: StdMutex<JoinSet<()>>,
    failures: StdMutex<Vec<Failure>>,
    slots: Arc<Semaphore>,
    connections: Connections,
    limits: RuntimeLimits,
    local_writes: AtomicU64,
}

impl Shared {
    fn new(limits: RuntimeLimits) -> Arc<Self> {
        Arc::new(Self { tasks: StdMutex::new(JoinSet::new()), failures: StdMutex::new(Vec::new()),
            slots: Arc::new(Semaphore::new(limits.connections)), connections: Arc::new(Mutex::new(HashMap::new())),
            limits, local_writes: AtomicU64::new(0) })
    }
    fn spawn(&self, future: impl Future<Output = ()> + Send + 'static) {
        let mut tasks = self.tasks.lock().unwrap_or_else(|e| e.into_inner());
        while let Some(result) = tasks.try_join_next() {
            if let Err(error) = result { eprintln!("P4_EVENT_TRANSPORT_TASK_FAILED error={error}"); }
        }
        tasks.spawn(future);
    }
    fn preserve(&self, failure: Failure) {
        // Each retained delivery still consumes its upstream store's count and
        // bytes. Raw ingress failures additionally retain their connection slot.
        // No dropped values, reconstruction, automatic replay, or uncharged copy.
        self.failures.lock().unwrap_or_else(|e| e.into_inner()).push(failure);
    }
    fn writer<W: AsyncWrite + Unpin + Send + 'static>(self: &Arc<Self>, writer: W, slot: Arc<OwnedSemaphorePermit>) -> ConnectionSender {
        self.writer_with_finish(writer, slot, None)
    }
    fn writer_with_finish<W: AsyncWrite + Unpin + Send + 'static>(self: &Arc<Self>, mut writer: W, slot: Arc<OwnedSemaphorePermit>, finish: Option<Arc<AtomicBool>>) -> ConnectionSender {
        let (sender, receiver) = mpsc::channel(self.limits.queue);
        let shared = Arc::clone(self);
        self.spawn(async move {
            if let Err(value) = write_loop(&mut writer, receiver, &shared.local_writes).await {
                p4_llamacpp_staged_adapter::v2::record::record(&format!(
                    "P4_EVENT_WRITE_FAILED retained_pending={} result={} error={}", value.pending.len(),
                    if value.started { "uncertain" } else { "not_started" }, value.error));
                for sender in shared.connections.lock().await.values_mut() {
                    if sender.as_ref().is_some_and(|sender| sender.is_closed()) { *sender = None; }
                }
                shared.preserve(Failure::Writer { value, _slot: slot });
            } else if finish.as_ref().is_some_and(|finish| finish.load(Ordering::Acquire)) {
                // All sender clones are gone and every queued Event has been
                // locally written. This ACK has no request/receipt/KV authority.
                if let Err(error) = writer.write_u32_le(0).await {
                    eprintln!("P4_EVENT_CONNECTION_FINISH_ACK_FAILED error={error}");
                } else if let Err(error) = writer.shutdown().await {
                    eprintln!("P4_EVENT_CONNECTION_FINISH_ACK_FAILED error={error}");
                }
            }
        });
        sender
    }
}

pub(super) struct Owner(Arc<Shared>);
impl Owner {
    pub(super) fn start(listener: TcpListener, broker: Arc<RetainedEventBroker>, outer: Arc<CompletionMailbox>, outbound: Arc<CompletionMailbox>, limits: RuntimeLimits) -> Self {
        let shared = Shared::new(limits);
        shared.spawn(deliver_outer(outer, Arc::clone(&shared)));
        shared.spawn(deliver_outbound(outbound, Arc::clone(&shared)));
        shared.spawn(accept(listener, broker, Arc::clone(&shared)));
        Self(shared)
    }
    pub(super) fn abort(&self) {
        self.0.tasks.lock().unwrap_or_else(|e| e.into_inner()).abort_all();
    }
}
impl Drop for Owner {
    fn drop(&mut self) { self.abort(); }
}

async fn accept(listener: TcpListener, broker: Arc<RetainedEventBroker>, shared: Arc<Shared>) {
    loop {
        // Failed owners keep their slots. Exhaustion stops new admissions;
        // healthy route/node work runs in independent tasks.
        let Ok(slot) = Arc::clone(&shared.slots).acquire_owned().await else { return; };
        match listener.accept().await {
            Ok((stream, peer)) => {
                let id = CONNECTIONS.fetch_add(1, Ordering::Relaxed);
                eprintln!("P4_EVENT_CONNECTION_OPENED connection={id} peer={peer}");
                let broker = Arc::clone(&broker);
                let connection = Arc::clone(&shared);
                shared.spawn(async move { serve(id, stream, broker, connection, Arc::new(slot)).await; });
            }
            Err(error) => eprintln!("P4_EVENT_ACCEPT_FAILED error={error}"),
        }
    }
}

async fn serve(id: u64, stream: TcpStream, broker: Arc<RetainedEventBroker>, shared: Arc<Shared>, slot: Arc<OwnedSemaphorePermit>) {
    if let Err(error) = stream.set_nodelay(true) {
        eprintln!("P4_EVENT_CONNECTION_STOPPED connection={id} error={error}"); return;
    }
    let (mut reader, writer) = stream.into_split();
    let finish = Arc::new(AtomicBool::new(false));
    let sender = shared.writer_with_finish(writer, Arc::clone(&slot), Some(Arc::clone(&finish)));
    loop {
        let mut event = match read_event(&mut reader).await {
            Ok(Some(event)) => event,
            Ok(None) => {
                // Explicit connection-scoped FINISH, not input EOF. Detach
                // only this socket's live bindings; retain failure tombstones
                // and any replacement socket. Existing senders drain normally.
                finish.store(true, Ordering::Release);
                shared.connections.lock().await.retain(|_, current| !current.as_ref().is_some_and(|current| current.same_channel(&sender)));
                eprintln!("P4_EVENT_CONNECTION_FINISH connection={id}");
                return;
            }
            Err(error) => {
                eprintln!("P4_EVENT_CONNECTION_STOPPED connection={id} error={error}"); break;
            }
        };
        if let Endpoint::Outer(route) = &event.envelope.source {
            let mut routes = shared.connections.lock().await;
            if !matches!(routes.get(route), Some(None)) { routes.insert(route.clone(), Some(sender.clone())); }
        }
        loop {
            match broker.dispatch_ingress(event) {
                Ok(DispatchOutcome::Duplicate) => {
                    eprintln!("P4_EVENT_DUPLICATE_SUPPRESSED connection={id}"); break;
                }
                Ok(_) => break,
                Err(failure) if matches!(failure.error, DispatchError::Full(_)) => {
                    // One original per socket: stop reading until destination
                    // admission succeeds, keeping TCP backpressure on the peer.
                    event = *failure.event;
                    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                }
                Err(failure) => {
                    eprintln!("P4_EVENT_INGRESS_FAILED connection={id} error={}", failure.error);
                    shared.preserve(Failure::Ingress { error: failure.error.to_string(), event: *failure.event, _slot: Arc::clone(&slot) });
                    shared.connections.lock().await.retain(|_, current| !current.as_ref().is_some_and(|current| current.same_channel(&sender)));
                    return;
                }
            }
        }
    }
    // Input EOF may be a TCP half-close: the peer can still be reading outputs.
    // Keep its writer route. Only a write failure blocks that generation;
    // replacing a route drops the old sender without deleting a newer binding.
}

async fn deliver_outer(receiver: Arc<CompletionMailbox>, shared: Arc<Shared>) {
    while let Some(event) = next(&receiver).await {
        let sender = match &event.event().envelope.target {
            Endpoint::Outer(target) => shared.connections.lock().await.get(target).cloned().flatten(),
            _ => None,
        };
        let failed = match sender {
            Some(sender) => sender.send(event).await.err().map(|failure| failure.0),
            None => Some(event),
        };
        if let Some(event) = failed {
            p4_llamacpp_staged_adapter::v2::record::record("P4_EVENT_OUTER_MISSING retained=1 result=not_started");
            if let Endpoint::Outer(target) = &event.event().envelope.target {
                shared.connections.lock().await.insert(target.clone(), None);
            }
            shared.preserve(Failure::Undelivered { error: "OUTER route unavailable".into(), event });
        }
    }
}

async fn deliver_outbound(receiver: Arc<CompletionMailbox>, shared: Arc<Shared>) {
    // A failed connection is kept closed. Later events never pass an uncertain
    // predecessor by silently reconnecting; explicit reconciliation is separate.
    let mut peers: HashMap<Address, Result<ConnectionSender, String>> = HashMap::new();
    while let Some(event) = next(&receiver).await {
        let target = event.event().envelope.target.agent_address().clone();
        if !peers.contains_key(&target) {
            let sender = connect(&target, &shared).await.map_err(|e| e.to_string());
            peers.insert(target.clone(), sender);
        }
        let failure = match peers.get(&target).expect("inserted peer") {
            Ok(sender) => sender.send(event).await.err().map(|e| ("peer writer closed".into(), e.0)),
            Err(error) => Some((error.clone(), event)),
        };
        if let Some((error, event)) = failure {
            eprintln!("P4_EVENT_PEER_DELIVERY_FAILED target={target} retained=1 error={error}");
            shared.preserve(Failure::Undelivered { error, event });
        }
    }
}

async fn connect(address: &Address, shared: &Arc<Shared>) -> io::Result<ConnectionSender> {
    let slot = Arc::clone(&shared.slots).acquire_owned().await.map_err(io::Error::other)?;
    let stream = TcpStream::connect((address.host.as_str(), address.port)).await?;
    stream.set_nodelay(true)?;
    let (_reader, writer) = stream.into_split();
    Ok(shared.writer(writer, Arc::new(slot)))
}

async fn write_loop<W: AsyncWrite + Unpin>(mut writer: W, mut receiver: mpsc::Receiver<RetainedCompletion>, local_writes: &AtomicU64) -> Result<(), WriteFailure> {
    while let Some(event) = receiver.recv().await {
        let bytes = match encode(event.event()) {
            Ok(bytes) => bytes,
            Err(error) => {
                receiver.close();
                return Err(WriteFailure { error: io::Error::other(error), started: false, current: event, pending: receiver });
            }
        };
        let result = write_frame(&mut writer, &bytes).await;
        if let Err(error) = result {
            receiver.close();
            return Err(WriteFailure { error, started: true, current: event, pending: receiver });
        }
        // Socket completion retires only this local allocation. Broker receipt,
        // remote delivery, native completion and KV authority are independent.
        local_writes.fetch_add(1, Ordering::Relaxed);
        event.retire();
    }
    Ok(())
}

async fn read_event<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Option<Event>> {
    let size = reader.read_u32_le().await? as usize;
    if size == 0 { return Ok(None); }
    if size > MAX_FRAME { return Err(io::Error::other("invalid event frame size")); }
    let mut bytes = vec![0; size];
    reader.read_exact(&mut bytes).await?;
    decode(&bytes).map(Some).map_err(io::Error::other)
}

async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, bytes: &[u8]) -> io::Result<()> {
    let size = u32::try_from(bytes.len()).map_err(|_| io::Error::other("event frame too large"))?;
    writer.write_u32_le(size).await?;
    writer.write_all(bytes).await?;
    writer.flush().await
}

#[cfg(test)]
mod tests;
