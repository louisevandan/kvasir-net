//! Persistent request-id multiplexing core for one remote P4 peer Agent.
//! The compatibility server also uses it for a process-separated adapter until
//! that adapter is registered as a co-resident in-memory handler.

use p4_protocol::{Message, Phase, RoutedMessage, decode_routed_message, encode_routed_message};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, mpsc};

type AsyncError = Box<dyn std::error::Error + Send + Sync>;
type ResponseSender = mpsc::Sender<Message>;
const HEADER_BYTES: usize = 16;
const MAX_FRAME_BYTES: usize = 1024 * 1024;
const PREFILL_QUEUE: usize = 1024;
const CONTROL_QUEUE: usize = 4096;
const RESPONSE_QUEUE: usize = 1024;

#[derive(Default)]
pub(crate) struct PeerMuxPool {
    channels: Mutex<HashMap<String, Arc<PeerMux>>>,
}

impl PeerMuxPool {
    pub(crate) async fn execute(
        &self,
        endpoint: &str,
        route_id: String,
        deadline_unix_ms: u64,
        message: Message,
    ) -> Result<mpsc::Receiver<Message>, AsyncError> {
        let channel = self.channel(endpoint).await?;
        channel.execute(route_id, deadline_unix_ms, message).await
    }

    pub(crate) fn cancel(
        self: &Arc<Self>,
        endpoint: &str,
        route_id: &str,
        request_id: &str,
        reason: &str,
    ) {
        let pool = Arc::clone(self);
        let endpoint = endpoint.to_owned();
        let route_id = route_id.to_owned();
        let request_id = request_id.to_owned();
        let reason = reason.to_owned();
        tokio::spawn(async move {
            let channel = pool.channels.lock().await.get(&endpoint).cloned();
            if let Some(channel) = channel {
                channel.cancel(route_id, request_id, reason).await;
            }
        });
    }

    async fn channel(&self, endpoint: &str) -> Result<Arc<PeerMux>, AsyncError> {
        let mut channels = self.channels.lock().await;
        if let Some(channel) = channels.get(endpoint).filter(|value| value.is_healthy()) {
            return Ok(Arc::clone(channel));
        }
        let channel = Arc::new(PeerMux::connect(endpoint).await?);
        channels.insert(endpoint.into(), Arc::clone(&channel));
        Ok(channel)
    }
}

struct PeerMux {
    prefill: mpsc::Sender<RoutedMessage>,
    control: mpsc::Sender<RoutedMessage>,
    pending: Arc<Mutex<HashMap<String, ResponseSender>>>,
    healthy: Arc<AtomicBool>,
}

impl PeerMux {
    async fn connect(endpoint: &str) -> Result<Self, AsyncError> {
        let stream = TcpStream::connect(endpoint).await?;
        stream.set_nodelay(true)?;
        let (reader, writer) = stream.into_split();
        let (prefill, prefill_rx) = mpsc::channel(PREFILL_QUEUE);
        let (control, control_rx) = mpsc::channel(CONTROL_QUEUE);
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let healthy = Arc::new(AtomicBool::new(true));
        tokio::spawn(write_loop(
            writer,
            prefill_rx,
            control_rx,
            Arc::clone(&pending),
            Arc::clone(&healthy),
        ));
        tokio::spawn(read_loop(
            reader,
            Arc::clone(&pending),
            Arc::clone(&healthy),
        ));
        Ok(Self {
            prefill,
            control,
            pending,
            healthy,
        })
    }

    fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Acquire)
    }

    async fn execute(
        &self,
        route_id: String,
        deadline_unix_ms: u64,
        message: Message,
    ) -> Result<mpsc::Receiver<Message>, AsyncError> {
        if !self.is_healthy() {
            return Err("P4 peer multiplex channel is closed".into());
        }
        if deadline_unix_ms > 0 && deadline_unix_ms <= unix_millis() {
            return Err("P4 peer route deadline expired before dispatch".into());
        }
        let phase = match &message {
            Message::Execute(request) => request.phase.clone(),
            _ => return Err("P4 peer execution channel accepts Execute only".into()),
        };
        let (responses, receiver) = mpsc::channel(RESPONSE_QUEUE);
        if self
            .pending
            .lock()
            .await
            .insert(route_id.clone(), responses)
            .is_some()
        {
            return Err("duplicate active P4 route_id".into());
        }
        let outbound = if phase == Phase::Prefill {
            &self.prefill
        } else {
            &self.control
        };
        if outbound
            .send(RoutedMessage {
                route_id: route_id.clone(),
                deadline_unix_ms,
                message,
            })
            .await
            .is_err()
        {
            self.pending.lock().await.remove(&route_id);
            return Err("P4 peer multiplex writer is closed".into());
        }
        if deadline_unix_ms > 0 {
            let pending = Arc::clone(&self.pending);
            tokio::spawn(async move {
                let delay = deadline_unix_ms.saturating_sub(unix_millis());
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                if let Some(sender) = pending.lock().await.remove(&route_id) {
                    let _ = sender
                        .send(Message::Error {
                            request_id: "deadline".into(),
                            detail: "P4 peer route deadline exceeded".into(),
                        })
                        .await;
                }
            });
        }
        Ok(receiver)
    }

    async fn cancel(&self, route_id: String, request_id: String, reason: String) {
        self.pending.lock().await.remove(&route_id);
        let _ = self
            .control
            .send(RoutedMessage {
                route_id,
                deadline_unix_ms: 0,
                message: Message::Cancel { request_id, reason },
            })
            .await;
    }
}

async fn write_loop(
    mut writer: tokio::net::tcp::OwnedWriteHalf,
    mut prefill: mpsc::Receiver<RoutedMessage>,
    mut control: mpsc::Receiver<RoutedMessage>,
    pending: Arc<Mutex<HashMap<String, ResponseSender>>>,
    healthy: Arc<AtomicBool>,
) {
    loop {
        let message = tokio::select! {
            biased;
            value = control.recv() => value,
            value = prefill.recv() => value,
        };
        let Some(message) = message else { break };
        let frame = match encode_routed_message(&message) {
            Ok(frame) => frame,
            Err(error) => {
                fail_all(&pending, &healthy, format!("P4 encode failed: {error}")).await;
                return;
            }
        };
        if let Err(error) = writer.write_all(&frame).await {
            fail_all(&pending, &healthy, format!("P4 peer write failed: {error}")).await;
            return;
        }
    }
    fail_all(&pending, &healthy, "P4 peer writer stopped".into()).await;
}

async fn read_loop(
    mut reader: tokio::net::tcp::OwnedReadHalf,
    pending: Arc<Mutex<HashMap<String, ResponseSender>>>,
    healthy: Arc<AtomicBool>,
) {
    loop {
        let routed = match read_message(&mut reader).await {
            Ok(message) => message,
            Err(error) => {
                fail_all(&pending, &healthy, format!("P4 peer read failed: {error}")).await;
                return;
            }
        };
        let terminal = routed.message.is_terminal();
        let sender = pending.lock().await.get(&routed.route_id).cloned();
        if let Some(sender) = sender {
            let _ = sender.send(routed.message).await;
        }
        if terminal {
            pending.lock().await.remove(&routed.route_id);
        }
    }
}

async fn read_message(
    reader: &mut (impl AsyncReadExt + Unpin),
) -> Result<RoutedMessage, AsyncError> {
    let mut header = [0u8; HEADER_BYTES];
    reader.read_exact(&mut header).await?;
    let length = u32::from_le_bytes(header[8..12].try_into()?) as usize;
    if length > MAX_FRAME_BYTES {
        return Err("invalid P4 frame length".into());
    }
    let mut frame = Vec::with_capacity(HEADER_BYTES + length);
    frame.extend_from_slice(&header);
    frame.resize(HEADER_BYTES + length, 0);
    reader.read_exact(&mut frame[HEADER_BYTES..]).await?;
    Ok(decode_routed_message(&frame)?)
}

fn unix_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

async fn fail_all(
    pending: &Mutex<HashMap<String, ResponseSender>>,
    healthy: &AtomicBool,
    detail: String,
) {
    healthy.store(false, Ordering::Release);
    let requests = std::mem::take(&mut *pending.lock().await);
    for (request_id, sender) in requests {
        let _ = sender
            .send(Message::Error {
                request_id,
                detail: detail.clone(),
            })
            .await;
    }
}

#[cfg(test)]
mod tests;
