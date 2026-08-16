//! One sequence's stream, kept open between hops.
//!
//! This is the whole of the impedance mismatch. P4 generates by lapping a
//! chain: a hop reports one token and the request comes round again. An
//! OpenAI-compatible backend generates by streaming a whole completion down
//! one connection. Reconciling them by asking for one token per hop would
//! re-prefill the conversation on every lap, which is the difference between
//! a working system and a demonstration.
//!
//! So a sequence's completion is requested once, on its prefill, and read by a
//! thread that pushes tokens into a channel. Each later hop takes the next
//! token from that channel. The stream is the sequence's state and lives as
//! long as the sequence does.

use crate::chat::{Chunk, Request, chunk};
use crate::endpoint::{Closer, Endpoint};
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError, channel};
use std::time::Duration;

/// What a hop takes from a sequence.
pub enum Next {
    Token {
        text: String,
        position: u32,
    },
    /// The stream ended, with the backend's reason.
    Done(String),
    Failed(String),
}

pub struct Session {
    tokens: Receiver<Result<Chunk, String>>,
    /// How many tokens this sequence has been given.
    ///
    /// The backend is what knows how far a sequence has got — the request
    /// does not carry its progress back down — so the position on an outcome
    /// is counted here. Reporting the position the node handed in would make
    /// every token of a stream claim the same place in it.
    delivered: u32,
    /// Set once the stream has ended, so a later hop is answered from here
    /// rather than from a channel that will never speak again.
    ended: Option<String>,
    /// Ends the socket when this sequence is let go of.
    ///
    /// The reader thread owns the stream and is blocked on it, so it cannot
    /// close anything itself and will not notice this is gone until the read
    /// times out. That timeout is generous on purpose — a first token can be
    /// far off — which made every abandoned sequence hold a connection and a
    /// thread for a quarter of an hour.
    closer: Option<Closer>,
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(closer) = &self.closer {
            closer.close();
        }
    }
}

impl Session {
    /// Starts the completion. Returns once the request is on the wire, not
    /// once it has answered — the first token may be a long way off and the
    /// node has other sequences to admit meanwhile.
    pub fn start(
        endpoint: &Endpoint,
        model: &str,
        prompt: &str,
        max_tokens: u32,
        options: &str,
    ) -> Result<Self, String> {
        let body = Request {
            model,
            prompt,
            max_tokens,
            options,
        }
        .body();
        let mut events = endpoint.stream("/v1/chat/completions", &body)?;
        let closer = events.closer();
        let (sender, tokens) = channel();
        std::thread::spawn(move || {
            loop {
                match events.event() {
                    Ok(Some(payload)) => {
                        let parsed = chunk(&payload);
                        let stop =
                            matches!(&parsed, Ok(value) if value.stop.is_some()) || parsed.is_err();
                        if sender.send(parsed).is_err() || stop {
                            return;
                        }
                    }
                    // The stream closed without a finish reason. Said plainly:
                    // a caller left waiting for a terminal that never comes is
                    // what a leaked route looks like.
                    Ok(None) => {
                        let _ = sender.send(Ok(Chunk {
                            text: String::new(),
                            stop: Some("stop".into()),
                        }));
                        return;
                    }
                    Err(error) => {
                        let _ = sender.send(Err(error));
                        return;
                    }
                }
            }
        });
        Ok(Self {
            tokens,
            delivered: 0,
            ended: None,
            closer,
        })
    }

    /// The next token for this sequence.
    ///
    /// Waits, because a hop that returned nothing would have the node report a
    /// lap with no text and come round again — a busy loop against a device.
    /// A token that has not arrived within `patience` is reported as a failure
    /// rather than waited on forever, and the deadline above would answer for
    /// it anyway.
    pub fn token(&mut self, patience: Duration) -> Next {
        if let Some(reason) = &self.ended {
            return Next::Done(reason.clone());
        }
        loop {
            match self.tokens.recv_timeout(patience) {
                Ok(Ok(chunk)) => {
                    if let Some(reason) = chunk.stop {
                        self.ended = Some(reason.clone());
                        // A last chunk may carry text as well as a reason, and
                        // dropping it would lose the final token.
                        return if chunk.text.is_empty() {
                            Next::Done(reason)
                        } else {
                            self.delivered += 1;
                            Next::Token {
                                text: chunk.text,
                                position: self.delivered,
                            }
                        };
                    }
                    if chunk.text.is_empty() {
                        // A keep-alive. Keep waiting rather than reporting a
                        // lap that produced nothing.
                        continue;
                    }
                    self.delivered += 1;
                    return Next::Token {
                        text: chunk.text,
                        position: self.delivered,
                    };
                }
                Ok(Err(error)) => return Next::Failed(error),
                Err(RecvTimeoutError::Timeout) => {
                    return Next::Failed(format!("no token within {patience:?}"));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    self.ended = Some("stop".into());
                    return Next::Done("stop".into());
                }
            }
        }
    }

    /// Whether the stream has already ended, for a caller tidying up.
    pub fn finished(&mut self) -> bool {
        if self.ended.is_some() {
            return true;
        }
        matches!(self.tokens.try_recv(), Err(TryRecvError::Disconnected))
    }
}

#[cfg(test)]
mod tests;
