use super::PumpEvent;
use crate::client::permits::Permits;
use crate::transport::TransportReader;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::thread::{self, JoinHandle};

/// Reads one connection until it ends. The permit bound makes a slow pump
/// stop draining the socket instead of accumulating events in memory.
pub(super) fn spawn_reader(
    sender: Sender<PumpEvent>,
    mut reader: Box<dyn TransportReader>,
    epoch: u64,
    inbound: Arc<Permits>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        loop {
            match reader.recv() {
                Ok(Some(event)) => {
                    if !inbound.acquire() {
                        return;
                    }
                    if sender.send(PumpEvent::Inbound { epoch, event }).is_err() {
                        inbound.release();
                        return;
                    }
                }
                Ok(None) | Err(_) => {
                    let _ = sender.send(PumpEvent::ConnectionLost { epoch });
                    return;
                }
            }
        }
    })
}
