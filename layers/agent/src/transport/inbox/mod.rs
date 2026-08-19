//! The listener. It reads frames and puts them on the queue.
//!
//! That is the whole of it. No handler runs here, no body is decoded, and no
//! connection gets a thread of its own to do work in — the previous listener
//! spawned a thread per connection and called the handler inline, which put
//! every handler's duration inside the read loop.
//!
//! A frame's length comes from its header, so a reader knows when it has a
//! whole frame without decoding any of it.

use crate::queue::main::Sender;

use p4_protocol::QueueClass;
use p4_protocol::frame::{self, Frame};
use p4_protocol::return_channel::is_capability_channel;
use std::collections::{HashMap, HashSet, VecDeque};
#[cfg(windows)]
use std::ffi::c_void;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::ptr::null_mut;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::sync::{Mutex, Semaphore, mpsc};

const HEADER_BYTES: usize = 16;
const SUBSCRIPTION_DEPTH: usize = 1024;
const MAX_SUBSCRIPTION_SLOTS: usize = 1024;

/// Binds a logical OUTER return channel to the socket that registered it.
///
/// The binding is deliberately separate from an address: several OUTER
/// clients may share one agent address, and a reconnect may use a different
/// ephemeral socket. Frames arriving while a channel is disconnected remain
/// in a bounded buffer and are drained by the next registration.
#[derive(Clone, Default)]
pub struct Subscriptions {
    inner: Arc<Mutex<HashMap<String, Slot>>>,
    journal_root: Option<Arc<PathBuf>>,
    journal_errors: Arc<Mutex<HashSet<String>>>,
}

struct Slot {
    generation: u64,
    sender: Option<mpsc::Sender<Frame>>,
    pending: VecDeque<Frame>,
    unacked: VecDeque<Frame>,
    dropped: usize,
    loaded: bool,
}

impl Subscriptions {
    /// Enables an optional channel journal. The journal is deliberately
    /// opt-in: callers that do not provide a state root retain the existing
    /// process-local behavior and compatibility surface.
    pub fn with_journal(root: impl Into<PathBuf>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            journal_root: Some(Arc::new(root.into())),
            journal_errors: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub async fn bind(&self, channel: &str) -> (u64, mpsc::Receiver<Frame>, Vec<Frame>) {
        if self.journal_errors.lock().await.contains(channel) {
            let (_, receiver) = mpsc::channel(SUBSCRIPTION_DEPTH);
            return (0, receiver, Vec::new());
        }
        let restored = match &self.journal_root {
            Some(root) => match load_slot(root, channel).await {
                Ok(state) => state,
                Err(_) => {
                    self.journal_errors.lock().await.insert(channel.into());
                    let (_, receiver) = mpsc::channel(SUBSCRIPTION_DEPTH);
                    return (0, receiver, Vec::new());
                }
            },
            None => JournalState::default(),
        };
        let mut slots = self.inner.lock().await;
        if !slots.contains_key(channel) && slots.len() >= MAX_SUBSCRIPTION_SLOTS {
            let (_, receiver) = mpsc::channel(SUBSCRIPTION_DEPTH);
            return (0, receiver, Vec::new());
        }
        let slot = slots.entry(channel.to_owned()).or_insert_with(|| Slot {
            generation: 0,
            sender: None,
            pending: VecDeque::new(),
            unacked: VecDeque::new(),
            dropped: 0,
            loaded: false,
        });
        if !slot.loaded {
            slot.pending = restored.pending;
            slot.unacked = restored.unacked;
            slot.loaded = true;
        }
        slot.generation = slot.generation.wrapping_add(1).max(1);
        let generation = slot.generation;
        let (sender, receiver) = mpsc::channel(SUBSCRIPTION_DEPTH);
        slot.sender = Some(sender.clone());
        let mut replay: Vec<Frame> = slot.unacked.iter().cloned().collect();
        replay.extend(slot.pending.drain(..));
        if persist_slot(self.journal_root.as_deref(), channel, slot)
            .await
            .is_err()
        {
            self.journal_errors.lock().await.insert(channel.into());
            // The channel is fail-closed when its durable state cannot be
            // committed. Returning replay here would let a caller send
            // frames whose recovery point was never persisted.
            return (0, receiver, Vec::new());
        }
        (generation, receiver, replay)
    }

    pub async fn deliver(&self, channel: &str, frame: Frame) -> bool {
        let mut slots = self.inner.lock().await;
        let Some(slot) = slots.get_mut(channel) else {
            // An unknown channel must continue through the normal
            // duties/fallback path; creating a subscription here would
            // swallow legacy replies that never performed an ingress bind.
            return false;
        };
        if frame.envelope.event_seq != 0 {
            remember_unacked(slot, &frame);
            if persist_slot(self.journal_root.as_deref(), channel, slot)
                .await
                .is_err()
            {
                self.journal_errors.lock().await.insert(channel.into());
                return false;
            }
        }
        let durable = frame.envelope.event_seq != 0;
        if let Some(sender) = slot.sender.as_ref() {
            match sender.try_send(frame) {
                Ok(()) => return true,
                Err(mpsc::error::TrySendError::Full(frame)) => {
                    if !durable {
                        if slot.pending.len() >= SUBSCRIPTION_DEPTH {
                            slot.pending.pop_front();
                            slot.dropped += 1;
                        }
                        slot.pending.push_back(frame);
                    }
                }
                Err(mpsc::error::TrySendError::Closed(frame)) => {
                    slot.sender = None;
                    if !durable {
                        slot.pending.push_back(frame);
                    }
                }
            }
        } else if !durable {
            slot.pending.push_back(frame);
        }
        while slot.pending.len() > SUBSCRIPTION_DEPTH {
            slot.pending.pop_front();
            slot.dropped += 1;
        }
        let persisted = persist_slot(self.journal_root.as_deref(), channel, slot)
            .await
            .is_ok();
        if !persisted {
            self.journal_errors.lock().await.insert(channel.into());
        }
        persisted
    }

    async fn unbind(&self, channel: &str, generation: u64) {
        let mut slots = self.inner.lock().await;
        if let Some(slot) = slots.get_mut(channel)
            && slot.generation == generation
        {
            slot.sender = None;
        }
    }

    async fn requeue_failed<I>(&self, channel: &str, generation: u64, frames: I)
    where
        I: IntoIterator<Item = Frame>,
    {
        let mut slots = self.inner.lock().await;
        let Some(slot) = slots.get_mut(channel) else {
            return;
        };
        if slot.generation != generation {
            return;
        }
        slot.sender = None;
        for frame in frames {
            if slot.pending.len() >= SUBSCRIPTION_DEPTH {
                slot.pending.pop_front();
                slot.dropped += 1;
            }
            slot.pending.push_back(frame);
        }
        if persist_slot(self.journal_root.as_deref(), channel, slot)
            .await
            .is_err()
        {
            self.journal_errors.lock().await.insert(channel.into());
        }
    }

    pub async fn record_delivered(&self, channel: &str, generation: u64, frame: &Frame) {
        if frame.envelope.event_seq == 0 {
            return;
        }
        let mut slots = self.inner.lock().await;
        let Some(slot) = slots.get_mut(channel) else {
            return;
        };
        if slot.generation != generation {
            return;
        }
        remember_unacked(slot, frame);
        if persist_slot(self.journal_root.as_deref(), channel, slot)
            .await
            .is_err()
        {
            self.journal_errors.lock().await.insert(channel.into());
        }
    }

    pub async fn acknowledge(
        &self,
        channel: &str,
        generation: u64,
        stream_id: &str,
        event_seq: u64,
    ) -> bool {
        if stream_id.is_empty() || event_seq == 0 {
            return false;
        }
        let mut slots = self.inner.lock().await;
        let Some(slot) = slots.get_mut(channel) else {
            return false;
        };
        if generation == 0 || slot.generation != generation {
            return false;
        }
        let before = slot.unacked.len();
        slot.unacked.retain(|frame| {
            !(frame.envelope.stream_id == stream_id && frame.envelope.event_seq <= event_seq)
        });
        if persist_slot(self.journal_root.as_deref(), channel, slot)
            .await
            .is_err()
        {
            self.journal_errors.lock().await.insert(channel.into());
        }
        before != slot.unacked.len()
    }

    pub async fn metrics(&self) -> SubscriptionMetrics {
        let slots = self.inner.lock().await;
        slots
            .values()
            .fold(SubscriptionMetrics::default(), |mut total, slot| {
                total.pending += slot.pending.len();
                total.unacked += slot.unacked.len();
                total.dropped += slot.dropped;
                total
            })
    }

    #[cfg(test)]
    async fn generation(&self, channel: &str) -> Option<u64> {
        self.inner
            .lock()
            .await
            .get(channel)
            .map(|slot| slot.generation)
    }
}

const JOURNAL_MAGIC: &[u8] = b"P4SUB1";

#[derive(Default)]
struct JournalState {
    pending: VecDeque<Frame>,
    unacked: VecDeque<Frame>,
}

fn remember_unacked(slot: &mut Slot, frame: &Frame) {
    let same = |candidate: &Frame| {
        candidate.envelope.stream_id == frame.envelope.stream_id
            && candidate.envelope.event_seq == frame.envelope.event_seq
    };
    if slot.unacked.iter().any(same) {
        return;
    }
    if slot.unacked.len() >= SUBSCRIPTION_DEPTH {
        slot.unacked.pop_front();
        slot.dropped += 1;
    }
    slot.unacked.push_back(frame.clone());
}

fn journal_path(root: &Path, channel: &str) -> PathBuf {
    let encoded = channel
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    root.join(format!("{encoded}.p4sub"))
}

fn append_frame(out: &mut Vec<u8>, frame: &Frame) -> Result<(), String> {
    let encoded = frame::encode(&frame.envelope, &frame.body).map_err(|error| error.to_string())?;
    let length = u32::try_from(encoded.len()).map_err(|_| "journal frame too large".to_owned())?;
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(&encoded);
    Ok(())
}

fn encode_journal(slot: &Slot) -> Result<Vec<u8>, String> {
    let mut out = JOURNAL_MAGIC.to_vec();
    let unacked = u32::try_from(slot.unacked.len()).map_err(|_| "journal too deep".to_owned())?;
    let pending = u32::try_from(slot.pending.len()).map_err(|_| "journal too deep".to_owned())?;
    out.extend_from_slice(&unacked.to_le_bytes());
    for frame in &slot.unacked {
        append_frame(&mut out, frame)?;
    }
    out.extend_from_slice(&pending.to_le_bytes());
    for frame in &slot.pending {
        append_frame(&mut out, frame)?;
    }
    Ok(out)
}

fn take_u32(bytes: &[u8], offset: &mut usize) -> Result<u32, String> {
    let end = offset.checked_add(4).ok_or("journal offset overflow")?;
    let value = bytes.get(*offset..end).ok_or("truncated journal count")?;
    *offset = end;
    Ok(u32::from_le_bytes(value.try_into().unwrap()))
}

fn take_frame(bytes: &[u8], offset: &mut usize) -> Result<Frame, String> {
    let length = take_u32(bytes, offset)? as usize;
    let end = offset.checked_add(length).ok_or("journal frame overflow")?;
    let encoded = bytes.get(*offset..end).ok_or("truncated journal frame")?;
    *offset = end;
    frame::decode(encoded).map_err(|error| error.to_string())
}

fn decode_journal(bytes: &[u8]) -> Result<JournalState, String> {
    if !bytes.starts_with(JOURNAL_MAGIC) {
        return Err("journal magic mismatch".into());
    }
    let mut offset = JOURNAL_MAGIC.len();
    let unacked_count = take_u32(bytes, &mut offset)? as usize;
    let mut unacked = VecDeque::with_capacity(unacked_count.min(SUBSCRIPTION_DEPTH));
    for _ in 0..unacked_count {
        if unacked.len() < SUBSCRIPTION_DEPTH {
            unacked.push_back(take_frame(bytes, &mut offset)?);
        } else {
            let _ = take_frame(bytes, &mut offset)?;
        }
    }
    let pending_count = take_u32(bytes, &mut offset)? as usize;
    let mut pending = VecDeque::with_capacity(pending_count.min(SUBSCRIPTION_DEPTH));
    for _ in 0..pending_count {
        if pending.len() < SUBSCRIPTION_DEPTH {
            pending.push_back(take_frame(bytes, &mut offset)?);
        } else {
            let _ = take_frame(bytes, &mut offset)?;
        }
    }
    if offset != bytes.len() {
        return Err("trailing journal bytes".into());
    }
    Ok(JournalState { pending, unacked })
}

async fn load_slot(root: &Path, channel: &str) -> Result<JournalState, String> {
    let path = journal_path(root, channel);
    match tokio::fs::read(path).await {
        Ok(bytes) => decode_journal(&bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(JournalState::default()),
        Err(error) => Err(error.to_string()),
    }
}

async fn persist_slot(root: Option<&PathBuf>, channel: &str, slot: &Slot) -> Result<(), String> {
    let Some(root) = root else {
        return Ok(());
    };
    tokio::fs::create_dir_all(root)
        .await
        .map_err(|error| error.to_string())?;
    let bytes = encode_journal(slot)?;
    let path = journal_path(root, channel);
    let temp = path.with_extension("p4sub.tmp");
    let mut file = tokio::fs::File::create(&temp)
        .await
        .map_err(|error| error.to_string())?;
    file.write_all(&bytes)
        .await
        .map_err(|error| error.to_string())?;
    file.sync_all().await.map_err(|error| error.to_string())?;
    replace_journal_file(&temp, &path).map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(not(windows))]
fn replace_journal_file(temp: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::rename(temp, destination)
}

#[cfg(windows)]
fn replace_journal_file(temp: &Path, destination: &Path) -> std::io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW, ReplaceFileW,
    };

    let wide = |path: &Path| {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>()
    };
    let temp_wide = wide(temp);
    let destination_wide = wide(destination);
    // ReplaceFileW preserves the old destination until the replacement is
    // committed. MoveFileExW handles the first creation where no destination
    // exists; neither path deletes the old journal as a separate step.
    let replaced = unsafe {
        if destination.exists() {
            ReplaceFileW(
                destination_wide.as_ptr(),
                temp_wide.as_ptr(),
                null_mut(),
                0,
                null_mut::<c_void>(),
                null_mut(),
            ) != 0
        } else {
            false
        }
    };
    if replaced {
        return Ok(());
    }
    let moved = unsafe {
        MoveFileExW(
            temp_wide.as_ptr(),
            destination_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        ) != 0
    };
    if moved {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SubscriptionMetrics {
    pub pending: usize,
    pub unacked: usize,
    pub dropped: usize,
}

/// Accepts connections until the listener is dropped.
///
/// `connections` bounds how many sockets are held at once, separately from how
/// deep the queue is and how much is in flight.
pub async fn serve(listener: TcpListener, queue: Sender, connections: usize) {
    serve_with_subscriptions(listener, queue, connections, Subscriptions::default()).await;
}

pub async fn serve_with_subscriptions(
    listener: TcpListener,
    queue: Sender,
    connections: usize,
    subscriptions: Subscriptions,
) {
    let permits = Arc::new(Semaphore::new(connections));
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue;
        };
        let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else {
            // At the connection ceiling. Closing immediately is the honest
            // answer; holding the socket unread would look like a hang.
            drop(stream);
            continue;
        };
        let queue = queue.clone();
        let subscriptions = subscriptions.clone();
        tokio::spawn(async move {
            let _permit = permit;
            read_frames(stream, queue, subscriptions).await;
        });
    }
}

async fn read_frames(stream: TcpStream, queue: Sender, subscriptions: Subscriptions) {
    let (reader, writer) = stream.into_split();
    let writer = Arc::new(Mutex::new(writer));
    let mut reader = reader;
    let mut refusals = Refusals::default();
    let mut writers: HashMap<String, (u64, tokio::task::JoinHandle<()>, oneshot::Sender<()>)> =
        HashMap::new();
    // Once another socket has replaced a channel, this connection must not
    // reclaim it merely by sending another frame. A new TCP connection has an
    // empty set and may bind; the old one is permanently retired for that
    // logical channel.
    let mut retired_channels = HashSet::new();
    loop {
        let mut header = [0u8; HEADER_BYTES];
        if reader.read_exact(&mut header).await.is_err() {
            break;
        }
        let Ok(total) = frame::frame_len(&header) else {
            break;
        };
        let mut bytes = Vec::with_capacity(total);
        bytes.extend_from_slice(&header);
        bytes.resize(total, 0);
        if reader.read_exact(&mut bytes[HEADER_BYTES..]).await.is_err() {
            break;
        }
        let Ok(mut frame) = frame::decode(&bytes) else {
            break;
        };
        // Connection identity is local-only. Stamp the generation owned by
        // this reader so ACK handling can reject a stale socket after rebind.
        if let Some(channel) = frame.envelope.return_channel.as_deref()
            && let Some((generation, _, _)) = writers.get(channel)
        {
            frame.envelope.ingress_generation = *generation;
        }
        if frame.envelope.lane != QueueClass::Response
            && let Some(channel) = frame.envelope.return_channel.as_deref()
            && is_capability_channel(channel)
            && writers
                .get(channel)
                .is_none_or(|(_, task, _)| task.is_finished())
        {
            if let Some((_, task, stop)) = writers.remove(channel) {
                let _ = stop.send(());
                let _ = task.await;
                retired_channels.insert(channel.to_owned());
            }
            if !retired_channels.contains(channel) {
                let (generation, receiver, pending) = subscriptions.bind(channel).await;
                if generation == 0 {
                    return;
                }
                let channel = channel.to_owned();
                let task_channel = channel.clone();
                let unbind_channel = task_channel.clone();
                let registry = subscriptions.clone();
                let writer = Arc::clone(&writer);
                let (stop, stop_rx) = oneshot::channel();
                let task = tokio::spawn(async move {
                    write_subscription(
                        writer,
                        receiver,
                        registry.clone(),
                        task_channel.clone(),
                        generation,
                        pending,
                        stop_rx,
                    )
                    .await;
                    registry.unbind(&unbind_channel, generation).await;
                });
                writers.insert(channel, (generation, task, stop));
            }
        }
        if let Err(refused) = queue.offer(frame) {
            // The lane is full, and this connection carries traffic one way —
            // the sender answers on its own connection back to us and never
            // reads this one, so there is nowhere in band to say so. Inventing
            // a target to reply to would send the refusal somewhere nobody is
            // listening, which loses it and looks like a leak.
            //
            // So it is counted and said out loud, and the sender learns from
            // its deadline. A lane that fills is a sizing fact, and one that
            // fills quietly is the thing worth preventing.
            refusals.record(&refused.0);
        }
    }
    for (channel, (generation, task, stop)) in writers {
        let _ = stop.send(());
        let _ = task.await;
        subscriptions.unbind(&channel, generation).await;
    }
}

async fn write_subscription(
    writer: Arc<Mutex<OwnedWriteHalf>>,
    mut receiver: mpsc::Receiver<Frame>,
    registry: Subscriptions,
    channel: String,
    generation: u64,
    pending: Vec<Frame>,
    mut stop: oneshot::Receiver<()>,
) {
    for index in 0..pending.len() {
        let frame = pending[index].clone();
        tokio::select! {
            result = write_frame(&writer, &frame) => {
                if result.is_err() {
                    registry
                        .requeue_failed(&channel, generation, pending[index..].iter().cloned())
                        .await;
                    return;
                }
                registry.record_delivered(&channel, generation, &frame).await;
            }
            _ = &mut stop => {
                registry
                    .requeue_failed(&channel, generation, pending[index..].iter().cloned())
                    .await;
                return;
            }
        }
    }
    loop {
        tokio::select! {
            frame = receiver.recv() => {
                let Some(frame) = frame else { return };
                tokio::select! {
                    result = write_frame(&writer, &frame) => {
                        if result.is_err() {
                            registry
                                .requeue_failed(&channel, generation, std::iter::once(frame))
                                .await;
                            while let Ok(frame) = receiver.try_recv() {
                                registry
                                    .requeue_failed(&channel, generation, std::iter::once(frame))
                                    .await;
                            }
                            return;
                        }
                        registry.record_delivered(&channel, generation, &frame).await;
                    }
                    _ = &mut stop => {
                        registry
                            .requeue_failed(&channel, generation, std::iter::once(frame))
                            .await;
                        while let Ok(frame) = receiver.try_recv() {
                            registry
                                .requeue_failed(&channel, generation, std::iter::once(frame))
                                .await;
                        }
                        return;
                    }
                }
            }
            _ = &mut stop => {
                while let Ok(frame) = receiver.try_recv() {
                    registry
                        .requeue_failed(&channel, generation, std::iter::once(frame))
                        .await;
                }
                return;
            }
        }
    }
}

async fn write_frame(writer: &Arc<Mutex<OwnedWriteHalf>>, frame: &Frame) -> Result<(), ()> {
    let bytes = frame::encode(&frame.envelope, &frame.body).map_err(|_| ())?;
    writer.lock().await.write_all(&bytes).await.map_err(|_| ())
}

/// Counts refusals and says so, without saying so on every frame.
#[derive(Default)]
struct Refusals {
    total: usize,
}

impl Refusals {
    fn record(&mut self, frame: &Frame) {
        self.total += 1;
        // The first, then decade by decade. A lane that overflows tends to
        // keep overflowing, and a line per frame would bury the run it is
        // reporting on.
        if self.total == 1 || self.total.is_power_of_two() {
            eprintln!(
                "P4_AGENT_LANE_FULL lane={:?} route={} refused={}",
                frame.envelope.lane, frame.envelope.route, self.total
            );
        }
    }
}

#[cfg(test)]
mod tests;
