//! Actual socket pumps retain the same Event allocation through local writes.
//! A completed write is not remote acceptance; failed writes are never replayed.
use super::{RuntimeLimits, next};
use p4_adapter::node_adapter::{CompletionMailbox, RetainedCompletion};
use p4_agent_core::event_broker::{DispatchError, DispatchOutcome, RetainedEventBroker};
use p4_protocol::Address;
use p4_protocol::event::{Endpoint, Event, OuterEndpoint, decode, encode};
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::io;
use std::sync::{Arc, Mutex as StdMutex};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore, mpsc};
use tokio::task::JoinSet;

mod hop;

static CONNECTIONS: AtomicU64 = AtomicU64::new(1);
const MAX_FRAME: usize = 2 * 1024 * 1024 * 1024;
#[derive(Clone)]
struct ConnectionSender {
    events: mpsc::Sender<RetainedCompletion>,
    controls: Option<mpsc::Sender<HopCommand>>,
}
impl ConnectionSender {
    async fn send(&self, event: RetainedCompletion)
        -> Result<(), mpsc::error::SendError<RetainedCompletion>>
    { self.events.send(event).await }
    fn same_channel(&self, other: &Self) -> bool { self.events.same_channel(&other.events) }
    fn is_closed(&self) -> bool { self.events.is_closed() }
    async fn control(&self, frame: p4_protocol::event::hop::HopFrame) -> io::Result<()> {
        self.controls.as_ref().ok_or_else(|| io::Error::other("legacy connection has no hop control"))?
            .send(HopCommand::Send(frame)).await.map_err(|_| io::Error::other("hop control writer closed"))
    }
    async fn received(&self, frame: p4_protocol::event::hop::HopFrame) -> io::Result<()> {
        self.controls.as_ref().ok_or_else(|| io::Error::other("legacy connection has no hop control"))?
            .send(HopCommand::Received(frame)).await.map_err(|_| io::Error::other("hop control writer closed"))
    }
    async fn peer_closed(&self, error: impl Into<String>) {
        if let Some(controls) = &self.controls {
            let _ = controls.send(HopCommand::PeerClosed(error.into())).await;
        }
    }
}
type Connections = Arc<Mutex<HashMap<OuterEndpoint, Option<ConnectionSender>>>>;

struct WriteFailure {
    error: io::Error,
    // False means encoding failed before touching the socket. True remains
    // uncertain even if the peer happened to accept the whole frame.
    started: bool,
    current: RetainedCompletion,
    pending: VecDeque<RetainedCompletion>,
}

enum HopCommand {
    Send(p4_protocol::event::hop::HopFrame),
    Received(p4_protocol::event::hop::HopFrame),
    PeerClosed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HopFailureState { RejectedLocal, RejectedRemote, Uncertain, Conflict, Unknown }

struct HopOutstanding {
    attempt: u64,
    digest: p4_protocol::event::hop::EventDigest,
    event: RetainedCompletion,
}

struct HopAckIntent {
    attempt: u64,
    digest: p4_protocol::event::hop::EventDigest,
}

struct HopWriteFailure {
    error: io::Error,
    state: HopFailureState,
    current: Option<HopOutstanding>,
    outstanding: VecDeque<HopOutstanding>,
    pending_acks: VecDeque<HopAckIntent>,
    pending: VecDeque<RetainedCompletion>,
}

struct StoredFailure {
    id: u64,
    created_unix_ms: u64,
    value: Failure,
}

enum Failure {
    Writer { value: WriteFailure, _slot: Arc<OwnedSemaphorePermit> },
    HopWriter { value: HopWriteFailure, target: Option<Address>, generation: u64,
        _slot: Arc<OwnedSemaphorePermit> },
    Ingress { error: String, event: Event, _slot: Arc<OwnedSemaphorePermit> },
    Undelivered { error: String, event: RetainedCompletion },
}

struct Shared {
    tasks: StdMutex<JoinSet<()>>,
    failures: StdMutex<Vec<StoredFailure>>,
    next_failure: AtomicU64,
    slots: Arc<Semaphore>,
    connections: Connections,
    peers: Mutex<HashMap<Address, Result<ConnectionSender, String>>>,
    limits: RuntimeLimits,
    local_writes: AtomicU64,
    receipts: StdMutex<hop::ReceiptStore>,
    sender_id: String,
}

impl Shared {
    fn new(limits: RuntimeLimits) -> Arc<Self> {
        Self::with_identity("test-agent".into(), limits)
    }
    fn with_identity(sender_id: String, limits: RuntimeLimits) -> Arc<Self> {
        Arc::new(Self { tasks: StdMutex::new(JoinSet::new()), failures: StdMutex::new(Vec::new()),
            next_failure: AtomicU64::new(1),
            slots: Arc::new(Semaphore::new(limits.connections)), connections: Arc::new(Mutex::new(HashMap::new())),
            peers: Mutex::new(HashMap::new()),
            limits, local_writes: AtomicU64::new(0),
            receipts: StdMutex::new(hop::ReceiptStore::new(limits.hop_receipts, limits.hop_receipt_bytes)),
            sender_id })
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
        self.failures.lock().unwrap_or_else(|e| e.into_inner()).push(StoredFailure {
            id: self.next_failure.fetch_add(1, Ordering::Relaxed),
            created_unix_ms: now_unix_ms(), value: failure });
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
        ConnectionSender { events: sender, controls: None }
    }

    fn hop_writer<W: AsyncWrite + Unpin + Send + 'static>(self: &Arc<Self>, mut writer: W,
        slot: Arc<OwnedSemaphorePermit>, finish: Option<Arc<AtomicBool>>, max_outstanding: usize,
        connection_generation: u64, target: Option<Address>)
        -> ConnectionSender
    {
        let (events, receiver) = mpsc::channel(self.limits.queue);
        let (controls, control_receiver) = mpsc::channel(self.limits.hop_outstanding.max(4));
        let shared = Arc::clone(self);
        self.spawn(async move {
            if let Err(value) = write_hop_loop(&mut writer, receiver, control_receiver,
                &shared.local_writes, finish, max_outstanding, &shared.sender_id,
                connection_generation).await {
                if target.is_none() && value.current.is_none() && value.outstanding.is_empty()
                    && value.pending_acks.is_empty() && value.pending.is_empty() {
                    // Accepted receipts remain authoritative in ReceiptStore.
                    // A lost Receipt/QueryResult with no Event or ACK intent is
                    // recovered by exact Query and must not leak a socket slot.
                    p4_llamacpp_staged_adapter::v2::record::record(&format!(
                        "P4_EVENT_HOP_CONTROL_CLOSED retained=0 result={:?} error={}",
                        value.state, value.error));
                    return;
                }
                p4_llamacpp_staged_adapter::v2::record::record(&format!(
                    "P4_EVENT_HOP_WRITE_FAILED retained_pending={} outstanding={} pending_acks={} result={:?} error={}",
                    value.pending.len(), value.outstanding.len() + usize::from(value.current.is_some()),
                    value.pending_acks.len(), value.state, value.error));
                for sender in shared.connections.lock().await.values_mut() {
                    if sender.as_ref().is_some_and(ConnectionSender::is_closed) { *sender = None; }
                }
                if let Some(target) = &target {
                    shared.peers.lock().await.insert(target.clone(), Err("hop writer failed; reconciliation required".into()));
                }
                shared.preserve(Failure::HopWriter { value, target, generation: connection_generation,
                    _slot: slot });
            }
        });
        ConnectionSender { events, controls: Some(controls) }
    }
}

pub(super) struct Owner(Arc<Shared>);
#[derive(Clone)]
pub(super) struct Inspector(Arc<Shared>);

impl Inspector {
    #[cfg(test)]
    pub(super) fn detached(limits: RuntimeLimits) -> Self { Self(Shared::new(limits)) }

    pub(super) fn snapshot(&self) -> serde_json::Value {
        use serde_json::json;
        let receipts = self.0.receipts.lock().unwrap_or_else(|e| e.into_inner()).snapshot();
        let failures = self.0.failures.lock().unwrap_or_else(|e| e.into_inner());
        let mut states: std::collections::BTreeMap<&'static str, usize> = std::collections::BTreeMap::new();
        let mut retained_event_bytes = 0usize;
        let mut ids = Vec::with_capacity(failures.len());
        for failure in failures.iter() {
            let state = failure_state(&failure.value);
            *states.entry(state).or_default() += 1;
            retained_event_bytes = retained_event_bytes.saturating_add(failure_bytes(&failure.value));
            ids.push(json!({"failure_id":format!("transport-{}", failure.id),
                "state":state, "created_unix_ms":failure.created_unix_ms}));
        }
        json!({
            "receipts":{"limit_count":receipts.limit_count,"limit_bytes":receipts.limit_bytes,
                "records":receipts.records,"reserved_bytes":receipts.reserved_bytes,
                "pending":receipts.pending,"accepted":receipts.accepted,"rejected":receipts.rejected,
                "oldest_unix_ms":receipts.oldest_unix_ms},
            "failures":{"count":failures.len(),"retained_event_bytes":retained_event_bytes,
                "oldest_unix_ms":failures.iter().map(|value| value.created_unix_ms).min(),
                "states":states,"failure_ids":ids},
        })
    }

    pub(super) async fn reconcile(&self, failure_id: &str) -> serde_json::Value {
        use serde_json::json;
        let Some(number) = failure_id.strip_prefix("transport-")
            .and_then(|value| value.parse::<u64>().ok()) else {
            return json!({"ok":false,"state":"rejected_local","detail":"invalid transport failure ID"});
        };
        let stored = {
            let mut failures = self.0.failures.lock().unwrap_or_else(|e| e.into_inner());
            let Some(index) = failures.iter().position(|value| value.id == number) else {
                return json!({"ok":false,"state":"unknown","detail":"transport failure ID not found"});
            };
            failures.remove(index)
        };
        match reconcile_failure(Arc::clone(&self.0), stored).await {
            Ok((state, detail)) => json!({"ok":true,"state":state,"detail":detail}),
            Err((stored, state, detail)) => {
                self.0.failures.lock().unwrap_or_else(|e| e.into_inner()).push(stored);
                json!({"ok":false,"state":state,"detail":detail})
            }
        }
    }
}

async fn reconcile_failure(shared: Arc<Shared>, mut stored: StoredFailure)
    -> Result<(&'static str, String), (StoredFailure, &'static str, String)>
{
    match stored.value {
        Failure::Undelivered { error: _, event } => {
            let target = event.event().envelope.target.agent_address().clone();
            let sender = match connect(&target, &shared).await {
                Ok(value) => value,
                Err(error) => {
                    stored.value = Failure::Undelivered { error: error.to_string(), event };
                    return Err((stored, "not_started", error.to_string()));
                }
            };
            shared.peers.lock().await.insert(target.clone(), Ok(sender.clone()));
            match sender.send(event).await {
                Ok(()) => Ok(("restarted", "original Event queued on an acknowledged generation".into())),
                Err(error) => {
                    stored.value = Failure::Undelivered { error: "replacement writer closed".into(), event: error.0 };
                    Err((stored, "not_started", "replacement writer closed".into()))
                }
            }
        }
        Failure::HopWriter { mut value, target: Some(target), generation, _slot } => {
            let mut queries = Vec::new();
            for item in value.outstanding.iter() { queries.push((item.attempt, item.digest)); }
            if let Some(item) = &value.current { queries.push((item.attempt, item.digest)); }
            for &(attempt, digest) in &queries {
                let result = match query_receipt(&target, &shared, generation, attempt, digest).await {
                    Ok(value) => value,
                    Err(error) => {
                        value.state = HopFailureState::Unknown;
                        stored.value = Failure::HopWriter { value, target: Some(target), generation, _slot };
                        return Err((stored, "unknown", error.to_string()));
                    }
                };
                if result != p4_protocol::event::hop::ReceiptStatus::AcceptedExact {
                    let (state, state_name) = match result {
                        p4_protocol::event::hop::ReceiptStatus::Conflict => (HopFailureState::Conflict, "conflict"),
                        p4_protocol::event::hop::ReceiptStatus::Rejected => (HopFailureState::RejectedRemote, "rejected_remote"),
                        p4_protocol::event::hop::ReceiptStatus::Unknown => (HopFailureState::Unknown, "unknown"),
                        p4_protocol::event::hop::ReceiptStatus::AcceptedExact => unreachable!(),
                    };
                    value.state = state;
                    stored.value = Failure::HopWriter { value, target: Some(target), generation, _slot };
                    return Err((stored, state_name, "remote receipt did not prove exact acceptance".into()));
                }
            }
            for (attempt, digest) in queries {
                if !value.pending_acks.iter().any(|intent|
                    intent.attempt == attempt && intent.digest == digest) {
                    value.pending_acks.push_back(HopAckIntent { attempt, digest });
                }
            }
            if let Some(item) = value.current.take() { item.event.retire(); }
            for item in value.outstanding.drain(..) { item.event.retire(); }
            if let Err(error) = send_receipt_acks(&target, &shared, generation, &value.pending_acks).await {
                value.state = HopFailureState::Uncertain;
                stored.value = Failure::HopWriter { value, target: Some(target), generation, _slot };
                return Err((stored, "uncertain", error.to_string()));
            }
            let sender = match connect(&target, &shared).await {
                Ok(value) => value,
                Err(error) => {
                    value.state = HopFailureState::Uncertain;
                    stored.value = Failure::HopWriter { value, target: Some(target), generation, _slot };
                    return Err((stored, "not_started", error.to_string()));
                }
            };
            shared.peers.lock().await.insert(target.clone(), Ok(sender.clone()));
            while let Some(event) = value.pending.pop_front() {
                if let Err(error) = sender.send(event).await {
                    value.pending.push_front(error.0);
                    value.state = HopFailureState::Uncertain;
                    stored.value = Failure::HopWriter { value, target: Some(target), generation, _slot };
                    return Err((stored, "not_started", "replacement writer closed".into()));
                }
            }
            Ok(("accepted_exact", "exact receipts retired predecessors and resumed the original queue".into()))
        }
        value => {
            stored.value = value;
            let state = failure_state(&stored.value);
            Err((stored, state, "failure is not eligible for explicit replay".into()))
        }
    }
}

fn failure_state(failure: &Failure) -> &'static str {
    match failure {
        Failure::Writer { value, .. } => if value.started { "uncertain" } else { "rejected_local" },
        Failure::HopWriter { value, .. } => match value.state {
            HopFailureState::RejectedLocal => "rejected_local",
            HopFailureState::RejectedRemote => "rejected_remote",
            HopFailureState::Uncertain => "uncertain",
            HopFailureState::Conflict => "conflict",
            HopFailureState::Unknown => "unknown",
        },
        Failure::Ingress { .. } => "rejected_remote",
        Failure::Undelivered { .. } => "not_started",
    }
}

fn failure_bytes(failure: &Failure) -> usize {
    let cost = |event: &Event| p4_adapter::node_adapter::retained_event_bytes(event).unwrap_or(0);
    match failure {
        Failure::Writer { value, .. } => cost(value.current.event())
            .saturating_add(value.pending.iter().map(|value| cost(value.event())).sum()),
        Failure::HopWriter { value, .. } => value.current.as_ref().map_or(0, |value| cost(value.event.event()))
            .saturating_add(value.outstanding.iter().map(|value| cost(value.event.event())).sum())
            .saturating_add(value.pending.iter().map(|value| cost(value.event())).sum()),
        Failure::Ingress { event, .. } => cost(event),
        Failure::Undelivered { event, .. } => cost(event.event()),
    }
}

impl Owner {
    #[cfg(test)]
    pub(super) async fn outer_route_count(&self) -> usize {
        self.0.connections.lock().await.len()
    }
    pub(super) fn start(listener: TcpListener, broker: Arc<RetainedEventBroker>, outer: Arc<CompletionMailbox>, outbound: Arc<CompletionMailbox>, limits: RuntimeLimits) -> Self {
        let shared = Shared::with_identity(broker.local_address().to_string(), limits);
        shared.spawn(deliver_outer(outer, Arc::clone(&shared)));
        shared.spawn(deliver_outbound(outbound, Arc::clone(&shared)));
        shared.spawn(accept(listener, broker, Arc::clone(&shared)));
        Self(shared)
    }
    pub(super) fn abort(&self) {
        self.0.tasks.lock().unwrap_or_else(|e| e.into_inner()).abort_all();
    }
    pub(super) fn inspector(&self) -> Inspector { Inspector(Arc::clone(&self.0)) }
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
    let first = match read_body(&mut reader).await {
        Ok(Some(value)) => value,
        Ok(None) => { eprintln!("P4_EVENT_CONNECTION_STOPPED connection={id} error=FINISH before handshake"); return; }
        Err(error) => { eprintln!("P4_EVENT_CONNECTION_STOPPED connection={id} error={error}"); return; }
    };
    match p4_protocol::event::hop::decode(&first) {
        Ok(Some(p4_protocol::event::hop::HopFrame::Hello { sender_id, connection_generation,
            max_outstanding, max_receipt_bytes: _ })) => {
            serve_hop(id, reader, writer, broker, shared, slot, sender_id,
                connection_generation, max_outstanding as usize).await;
            return;
        }
        Ok(Some(_)) => { eprintln!("P4_EVENT_CONNECTION_STOPPED connection={id} error=first hop frame is not hello"); return; }
        Err(error) => { eprintln!("P4_EVENT_CONNECTION_STOPPED connection={id} error={error}"); return; }
        Ok(None) => {}
    }
    let first = match decode(&first) {
        Ok(value) => value,
        Err(error) => { eprintln!("P4_EVENT_CONNECTION_STOPPED connection={id} error={error}"); return; }
    };
    let finish = Arc::new(AtomicBool::new(false));
    let sender = shared.writer_with_finish(writer, Arc::clone(&slot), Some(Arc::clone(&finish)));
    let mut first = Some(first);
    loop {
        let mut event = match first.take() {
            Some(event) => event,
            None => match read_event(&mut reader).await {
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
            }
        };
        if let Endpoint::Outer(route) = &event.envelope.source
            && &route.ingress_agent == broker.local_address() {
            // A forwarded request preserves its OUTER source. Only its
            // reception agent may bind that identity to an external socket.
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

async fn serve_hop(id: u64, mut reader: tokio::net::tcp::OwnedReadHalf,
    writer: tokio::net::tcp::OwnedWriteHalf, broker: Arc<RetainedEventBroker>,
    shared: Arc<Shared>, slot: Arc<OwnedSemaphorePermit>, peer_sender_id: String,
    peer_generation: u64, peer_max_outstanding: usize)
{
    use p4_protocol::event::hop::{HopFrame, ReceiptStatus};
    let finish = Arc::new(AtomicBool::new(false));
    let local_generation = CONNECTIONS.fetch_add(1, Ordering::Relaxed).max(1);
    let max_outstanding = peer_max_outstanding.min(shared.limits.hop_outstanding).max(1);
    let sender = shared.hop_writer(writer, Arc::clone(&slot), Some(Arc::clone(&finish)),
        max_outstanding, local_generation, None);
    if sender.control(HopFrame::HelloAck {
        accepted_connection_generation: peer_generation,
        sender_id: shared.sender_id.clone(), connection_generation: local_generation,
        max_outstanding: shared.limits.hop_outstanding as u32,
        max_receipt_bytes: shared.limits.hop_receipt_bytes as u64,
    }).await.is_err() { return; }

    loop {
        let body = match read_body(&mut reader).await {
            Ok(Some(value)) => value,
            Ok(None) => {
                finish.store(true, Ordering::Release);
                shared.connections.lock().await.retain(|_, current|
                    !current.as_ref().is_some_and(|current| current.same_channel(&sender)));
                eprintln!("P4_EVENT_CONNECTION_FINISH connection={id}");
                return;
            }
            Err(error) => {
                sender.peer_closed(error.to_string()).await;
                eprintln!("P4_EVENT_CONNECTION_STOPPED connection={id} error={error}");
                return;
            }
        };
        let frame = match p4_protocol::event::hop::decode(&body) {
            Ok(Some(value)) => value,
            Ok(None) => { sender.peer_closed("legacy Event after hop hello").await; return; }
            Err(error) => { sender.peer_closed(error.to_string()).await; return; }
        };
        match frame {
            HopFrame::Data { attempt, digest, event: bytes } => {
                let event = match decode(&bytes) {
                    Ok(value) => value,
                    Err(error) => {
                        let _ = sender.control(HopFrame::Receipt { attempt, digest,
                            status: ReceiptStatus::Rejected, detail: error.to_string() }).await;
                        continue;
                    }
                };
                let event_id = event.envelope.event_id.clone();
                let key = hop::ReceiptKey { sender_id: peer_sender_id.clone(),
                    connection_generation: peer_generation, attempt };
                let reservation = shared.receipts.lock().unwrap_or_else(|e| e.into_inner())
                    .reserve(key.clone(), digest, &event_id);
                match reservation {
                    Ok(hop::Reservation::Existing(view)) => {
                        let _ = sender.control(HopFrame::Receipt { attempt, digest,
                            status: view.status, detail: view.detail }).await;
                        continue;
                    }
                    Err(error) => {
                        let (status, detail) = match error {
                            hop::ReserveError::Conflict => (ReceiptStatus::Conflict, "attempt digest differs"),
                            hop::ReserveError::Full => (ReceiptStatus::Rejected, "hop receipt store full"),
                            hop::ReserveError::TooLarge => (ReceiptStatus::Rejected, "hop receipt exceeds byte limit"),
                        };
                        let _ = sender.control(HopFrame::Receipt { attempt, digest, status,
                            detail: detail.into() }).await;
                        continue;
                    }
                    Ok(hop::Reservation::Reserved) => {}
                }
                if let Endpoint::Outer(route) = &event.envelope.source
                    && &route.ingress_agent == broker.local_address() {
                    let mut routes = shared.connections.lock().await;
                    if !matches!(routes.get(route), Some(None)) {
                        routes.insert(route.clone(), Some(sender.clone()));
                    }
                }
                let mut current = event;
                let result = loop {
                    match broker.dispatch_ingress(current) {
                        Ok(outcome) => break Ok(outcome),
                        Err(failure) if matches!(failure.error, DispatchError::Full(_)) => {
                            current = *failure.event;
                            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                        }
                        Err(failure) => break Err(failure),
                    }
                };
                let (status, detail) = match result {
                    Ok(_) => (ReceiptStatus::AcceptedExact, String::new()),
                    Err(failure) => {
                        let detail = failure.error.to_string();
                        shared.preserve(Failure::Ingress { error: detail.clone(), event: *failure.event,
                            _slot: Arc::clone(&slot) });
                        (ReceiptStatus::Rejected, detail)
                    }
                };
                let committed = shared.receipts.lock().unwrap_or_else(|e| e.into_inner())
                    .commit(&key, digest, status, detail);
                match committed {
                    Ok(view) => {
                        if sender.control(HopFrame::Receipt { attempt, digest, status: view.status,
                            detail: view.detail }).await.is_err() { return; }
                    }
                    Err(error) => { sender.peer_closed(error).await; return; }
                }
            }
            HopFrame::ReceiptAck { sender_id, connection_generation, attempt, digest } => {
                let key = hop::ReceiptKey { sender_id, connection_generation, attempt };
                let _ = shared.receipts.lock().unwrap_or_else(|e| e.into_inner())
                    .acknowledge(&key, digest);
            }
            HopFrame::Query { sender_id, connection_generation, attempt, digest } => {
                let key = hop::ReceiptKey { sender_id, connection_generation, attempt };
                let view = shared.receipts.lock().unwrap_or_else(|e| e.into_inner()).query(&key, digest);
                if sender.control(HopFrame::QueryResult { attempt, digest, status: view.status,
                    detail: view.detail }).await.is_err() { return; }
            }
            HopFrame::Receipt { .. } | HopFrame::QueryResult { .. } => {
                if sender.received(frame).await.is_err() { return; }
            }
            HopFrame::Hello { .. } | HopFrame::HelloAck { .. } => {
                sender.peer_closed("hop hello repeated after handshake").await;
                return;
            }
        }
    }
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
    while let Some(event) = next(&receiver).await {
        let target = event.event().envelope.target.agent_address().clone();
        if !shared.peers.lock().await.contains_key(&target) {
            let sender = connect(&target, &shared).await.map_err(|e| e.to_string());
            shared.peers.lock().await.insert(target.clone(), sender);
        }
        let peer = shared.peers.lock().await.get(&target).cloned().expect("inserted peer");
        let failure = match peer {
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
    let (mut reader, mut writer) = stream.into_split();
    let generation = CONNECTIONS.fetch_add(1, Ordering::Relaxed).max(1);
    let hello = p4_protocol::event::hop::encode(&p4_protocol::event::hop::HopFrame::Hello {
        sender_id: shared.sender_id.clone(), connection_generation: generation,
        max_outstanding: shared.limits.hop_outstanding as u32,
        max_receipt_bytes: shared.limits.hop_receipt_bytes as u64,
    }).map_err(io::Error::other)?;
    write_bytes_frame(&mut writer, &hello).await?;
    let response = read_body(&mut reader).await?
        .ok_or_else(|| io::Error::other("peer finished during hop hello"))?;
    let max_outstanding = match p4_protocol::event::hop::decode(&response).map_err(io::Error::other)? {
        Some(p4_protocol::event::hop::HopFrame::HelloAck { accepted_connection_generation,
            max_outstanding, .. }) if accepted_connection_generation == generation =>
                (max_outstanding as usize).min(shared.limits.hop_outstanding).max(1),
        Some(p4_protocol::event::hop::HopFrame::HelloAck { .. }) =>
            return Err(io::Error::other("hop hello ACK generation mismatch")),
        Some(_) => return Err(io::Error::other("peer did not acknowledge hop hello")),
        None => return Err(io::Error::other("peer does not support acknowledged hop transport")),
    };
    let sender = shared.hop_writer(writer, Arc::new(slot), None, max_outstanding, generation,
        Some(address.clone()));
    let reader_sender = sender.clone();
    shared.spawn(async move {
        loop {
            match read_body(&mut reader).await {
                Ok(Some(body)) => match p4_protocol::event::hop::decode(&body) {
                    Ok(Some(frame @ (p4_protocol::event::hop::HopFrame::Receipt { .. }
                        | p4_protocol::event::hop::HopFrame::QueryResult { .. }))) => {
                        if reader_sender.received(frame).await.is_err() { return; }
                    }
                    Ok(Some(_)) => { reader_sender.peer_closed("unexpected frame on outbound hop").await; return; }
                    Ok(None) => { reader_sender.peer_closed("legacy Event on outbound hop").await; return; }
                    Err(error) => { reader_sender.peer_closed(error.to_string()).await; return; }
                },
                Ok(None) => { reader_sender.peer_closed("unexpected FINISH on outbound hop").await; return; }
                Err(error) => { reader_sender.peer_closed(error.to_string()).await; return; }
            }
        }
    });
    Ok(sender)
}

async fn open_hop_control(address: &Address, shared: &Arc<Shared>) -> io::Result<TcpStream> {
    let mut stream = TcpStream::connect((address.host.as_str(), address.port)).await?;
    stream.set_nodelay(true)?;
    let generation = CONNECTIONS.fetch_add(1, Ordering::Relaxed).max(1);
    let hello = p4_protocol::event::hop::encode(&p4_protocol::event::hop::HopFrame::Hello {
        sender_id: shared.sender_id.clone(), connection_generation: generation,
        max_outstanding: 1, max_receipt_bytes: shared.limits.hop_receipt_bytes as u64,
    }).map_err(io::Error::other)?;
    write_bytes_frame(&mut stream, &hello).await?;
    let body = read_body(&mut stream).await?.ok_or_else(|| io::Error::other("FINISH during reconcile hello"))?;
    match p4_protocol::event::hop::decode(&body).map_err(io::Error::other)? {
        Some(p4_protocol::event::hop::HopFrame::HelloAck { accepted_connection_generation, .. })
            if accepted_connection_generation == generation => Ok(stream),
        _ => Err(io::Error::other("reconcile hello was not acknowledged")),
    }
}

async fn query_receipt(address: &Address, shared: &Arc<Shared>, connection_generation: u64,
    attempt: u64, digest: p4_protocol::event::hop::EventDigest)
    -> io::Result<p4_protocol::event::hop::ReceiptStatus>
{
    let mut stream = open_hop_control(address, shared).await?;
    let query = p4_protocol::event::hop::encode(&p4_protocol::event::hop::HopFrame::Query {
        sender_id: shared.sender_id.clone(), connection_generation, attempt, digest,
    }).map_err(io::Error::other)?;
    write_bytes_frame(&mut stream, &query).await?;
    let body = read_body(&mut stream).await?.ok_or_else(|| io::Error::other("FINISH before query result"))?;
    let status = match p4_protocol::event::hop::decode(&body).map_err(io::Error::other)? {
        Some(p4_protocol::event::hop::HopFrame::QueryResult { attempt: seen_attempt,
            digest: seen_digest, status, .. }) if seen_attempt == attempt && seen_digest == digest => status,
        _ => return Err(io::Error::other("receipt query result identity mismatch")),
    };
    stream.write_u32_le(0).await?;
    stream.flush().await?;
    if stream.read_u32_le().await? != 0 { return Err(io::Error::other("invalid reconcile FINISH ACK")); }
    Ok(status)
}

async fn send_receipt_acks(address: &Address, shared: &Arc<Shared>, connection_generation: u64,
    intents: &VecDeque<HopAckIntent>) -> io::Result<()>
{
    if intents.is_empty() { return Ok(()); }
    let mut stream = open_hop_control(address, shared).await?;
    for intent in intents {
        let ack = p4_protocol::event::hop::encode(&p4_protocol::event::hop::HopFrame::ReceiptAck {
            sender_id: shared.sender_id.clone(), connection_generation,
            attempt: intent.attempt, digest: intent.digest }).map_err(io::Error::other)?;
        write_bytes_frame(&mut stream, &ack).await?;
    }
    stream.write_u32_le(0).await?;
    stream.flush().await?;
    if stream.read_u32_le().await? != 0 { return Err(io::Error::other("invalid reconcile FINISH ACK")); }
    Ok(())
}

async fn write_hop_loop<W: AsyncWrite + Unpin>(writer: &mut W,
    mut events: mpsc::Receiver<RetainedCompletion>, mut controls: mpsc::Receiver<HopCommand>,
    local_writes: &AtomicU64, finish: Option<Arc<AtomicBool>>, max_outstanding: usize,
    sender_id: &str, connection_generation: u64)
    -> Result<(), HopWriteFailure>
{
    let mut outstanding = VecDeque::new();
    let mut pending_acks = VecDeque::new();
    let mut next_attempt = 1u64;
    let mut events_open = true;
    let mut controls_open = true;
    loop {
        if !events_open && outstanding.is_empty() && pending_acks.is_empty() {
            if finish.as_ref().is_some_and(|value| value.load(Ordering::Acquire)) {
                if let Err(error) = writer.write_u32_le(0).await {
                    return Err(hop_failure(error, HopFailureState::Uncertain,
                        None, outstanding, pending_acks, events));
                }
                if let Err(error) = writer.shutdown().await {
                    return Err(hop_failure(error, HopFailureState::Uncertain,
                        None, outstanding, pending_acks, events));
                }
            }
            return Ok(());
        }
        if !controls_open && !outstanding.is_empty() {
            return Err(hop_failure(io::Error::new(io::ErrorKind::UnexpectedEof,
                "hop receipt channel closed with outstanding Events"), HopFailureState::Uncertain,
                None, outstanding, pending_acks, events));
        }
        tokio::select! {
            biased;
            command = controls.recv(), if controls_open => match command {
                Some(HopCommand::Send(frame)) => {
                    let bytes = match p4_protocol::event::hop::encode(&frame) {
                        Ok(value) => value,
                        Err(error) => return Err(hop_failure(io::Error::other(error),
                            HopFailureState::RejectedLocal, None, outstanding, pending_acks, events)),
                    };
                    if let Err(error) = write_bytes_frame(writer, &bytes).await {
                        return Err(hop_failure(error, HopFailureState::Uncertain,
                            None, outstanding, pending_acks, events));
                    }
                }
                Some(HopCommand::Received(p4_protocol::event::hop::HopFrame::Receipt {
                    attempt, digest, status, detail })) => {
                    let Some(index) = outstanding.iter().position(|value| value.attempt == attempt) else {
                        return Err(hop_failure(io::Error::other("receipt has no outstanding attempt"),
                            HopFailureState::Conflict, None, outstanding, pending_acks, events));
                    };
                    if outstanding[index].digest != digest {
                        return Err(hop_failure(io::Error::other("receipt digest differs from outstanding Event"),
                            HopFailureState::Conflict, None, outstanding, pending_acks, events));
                    }
                    let value = outstanding.remove(index).expect("located outstanding attempt");
                    if status != p4_protocol::event::hop::ReceiptStatus::AcceptedExact {
                        let state = match status {
                            p4_protocol::event::hop::ReceiptStatus::Rejected => HopFailureState::RejectedRemote,
                            p4_protocol::event::hop::ReceiptStatus::Conflict => HopFailureState::Conflict,
                            p4_protocol::event::hop::ReceiptStatus::Unknown => HopFailureState::Unknown,
                            p4_protocol::event::hop::ReceiptStatus::AcceptedExact => unreachable!(),
                        };
                        return Err(hop_failure(io::Error::other(format!("hop receipt refused Event: {detail}")),
                            state, Some(value), outstanding, pending_acks, events));
                    }
                    value.event.retire();
                    pending_acks.push_back(HopAckIntent { attempt, digest });
                    let ack = p4_protocol::event::hop::encode(&p4_protocol::event::hop::HopFrame::ReceiptAck {
                        sender_id: sender_id.into(), connection_generation, attempt, digest })
                        .expect("validated receipt ACK");
                    if let Err(error) = write_bytes_frame(writer, &ack).await {
                        return Err(hop_failure(error, HopFailureState::Uncertain,
                            None, outstanding, pending_acks, events));
                    }
                    pending_acks.pop_front();
                }
                Some(HopCommand::Received(p4_protocol::event::hop::HopFrame::QueryResult { .. })) => {
                    return Err(hop_failure(io::Error::other("unsolicited hop query result"),
                        HopFailureState::Conflict, None, outstanding, pending_acks, events));
                }
                Some(HopCommand::Received(_)) => {
                    return Err(hop_failure(io::Error::other("unexpected hop frame reached writer"),
                        HopFailureState::Conflict, None, outstanding, pending_acks, events));
                }
                Some(HopCommand::PeerClosed(error)) => {
                    return Err(hop_failure(io::Error::new(io::ErrorKind::UnexpectedEof, error),
                        HopFailureState::Uncertain, None, outstanding, pending_acks, events));
                }
                None => controls_open = false,
            },
            event = events.recv(), if events_open && outstanding.len() < max_outstanding => match event {
                Some(event) => {
                    let encoded = match encode(event.event()) {
                        Ok(value) => value,
                        Err(error) => {
                            let current = HopOutstanding { attempt: next_attempt,
                                digest: [0; 32], event };
                            return Err(hop_failure(io::Error::other(error), HopFailureState::RejectedLocal,
                                Some(current), outstanding, pending_acks, events));
                        }
                    };
                    let digest = p4_protocol::event::hop::event_digest(&encoded);
                    let attempt = next_attempt;
                    next_attempt = match next_attempt.checked_add(1) {
                        Some(value) => value,
                        None => return Err(hop_failure(io::Error::other("hop attempt identity exhausted"),
                            HopFailureState::RejectedLocal, None, outstanding, pending_acks, events)),
                    };
                    let frame = match p4_protocol::event::hop::encode(
                        &p4_protocol::event::hop::HopFrame::Data { attempt, digest, event: encoded }) {
                        Ok(value) => value,
                        Err(error) => return Err(hop_failure(io::Error::other(error),
                            HopFailureState::RejectedLocal, None, outstanding, pending_acks, events)),
                    };
                    let current = HopOutstanding { attempt, digest, event };
                    if let Err(error) = write_bytes_frame(writer, &frame).await {
                        return Err(hop_failure(error, HopFailureState::Uncertain,
                            Some(current), outstanding, pending_acks, events));
                    }
                    local_writes.fetch_add(1, Ordering::Relaxed);
                    outstanding.push_back(current);
                }
                None => events_open = false,
            }
        }
    }
}

fn hop_failure(error: io::Error, state: HopFailureState, current: Option<HopOutstanding>,
    outstanding: VecDeque<HopOutstanding>, pending_acks: VecDeque<HopAckIntent>,
    mut pending: mpsc::Receiver<RetainedCompletion>) -> HopWriteFailure
{
    let pending = drain_pending(&mut pending);
    HopWriteFailure { error, state, current, outstanding, pending_acks, pending }
}

fn drain_pending(receiver: &mut mpsc::Receiver<RetainedCompletion>) -> VecDeque<RetainedCompletion> {
    receiver.close();
    let mut pending = VecDeque::with_capacity(receiver.len());
    while let Ok(event) = receiver.try_recv() { pending.push_back(event); }
    pending
}

async fn write_loop<W: AsyncWrite + Unpin>(mut writer: W, mut receiver: mpsc::Receiver<RetainedCompletion>, local_writes: &AtomicU64) -> Result<(), WriteFailure> {
    while let Some(event) = receiver.recv().await {
        let bytes = match encode(event.event()) {
            Ok(bytes) => bytes,
            Err(error) => {
                let pending = drain_pending(&mut receiver);
                return Err(WriteFailure { error: io::Error::other(error), started: false, current: event, pending });
            }
        };
        let result = write_frame(&mut writer, &bytes).await;
        if let Err(error) = result {
            let pending = drain_pending(&mut receiver);
            return Err(WriteFailure { error, started: true, current: event, pending });
        }
        // Socket completion retires only this local allocation. Broker receipt,
        // remote delivery, native completion and KV authority are independent.
        local_writes.fetch_add(1, Ordering::Relaxed);
        event.retire();
    }
    Ok(())
}

async fn read_event<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Option<Event>> {
    match read_body(reader).await? {
        Some(bytes) => decode(&bytes).map(Some).map_err(io::Error::other),
        None => Ok(None),
    }
}

async fn read_body<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Option<Vec<u8>>> {
    let size = reader.read_u32_le().await? as usize;
    if size == 0 { return Ok(None); }
    if size > MAX_FRAME { return Err(io::Error::other("invalid event frame size")); }
    let mut bytes = vec![0; size];
    reader.read_exact(&mut bytes).await?;
    Ok(Some(bytes))
}

async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, bytes: &[u8]) -> io::Result<()> {
    write_bytes_frame(writer, bytes).await
}

async fn write_bytes_frame<W: AsyncWrite + Unpin>(writer: &mut W, bytes: &[u8]) -> io::Result<()> {
    let size = u32::try_from(bytes.len()).map_err(|_| io::Error::other("event frame too large"))?;
    if bytes.len() > MAX_FRAME { return Err(io::Error::other("event frame too large")); }
    writer.write_u32_le(size).await?;
    writer.write_all(bytes).await?;
    writer.flush().await
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_millis() as u64
}

#[cfg(test)]
mod tests;
