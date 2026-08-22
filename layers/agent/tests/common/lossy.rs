//! A frame-aware relay, built only for these tests, that drops exactly one
//! occurrence of a chosen wire tag and passes every other frame whole.
//!
//! `p4_link`'s own relay promises never to drop or reorder a byte -- that is
//! the point of a byte-honest impairment simulator, and it never parses a
//! frame at all. Proving convergence after a lost `SessionClose` or
//! `SessionClosed` needs the opposite: a frame that provably never arrives,
//! exactly once, so the retry that follows is what closes the gap rather
//! than a race the test cannot tell apart from success. So this reads real
//! frame boundaries (mirroring `transport::inbox::read_frames`) and drops on
//! purpose, on one connection, in one direction, by design -- not a general
//! impairment tool and not meant to become one.

use p4_protocol::frame;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

// Mirrors `frame::mod.rs`'s own private `HEADER_BYTES`: magic(4) +
// version(1) + reserved(3) + envelope_len(4) + body_len(4). Not exported,
// so `transport::inbox::read_frames` duplicates the same constant locally;
// this does the same for the same reason.
const HEADER_BYTES: usize = 16;

/// A body-matching predicate, boxed once so `Drop` itself stays a plain
/// struct instead of spelling this trait object out at every use.
type BodyMatch = Box<dyn Fn(&[u8]) -> bool + Send + Sync>;

/// What to drop, and how many times it has been seen and actually dropped.
pub struct Drop {
    /// Matched against a frame's whole body. A prefix check (`starts_with`)
    /// is normally enough and is what every caller in this tree uses -- a
    /// single leading tag byte is ambiguous the moment two message bodies
    /// share one (as `close|`/`closed|` do in the plain-text test
    /// vocabulary), so this matches on the distinguishing prefix instead of
    /// one byte.
    matches: BodyMatch,
    seen: AtomicUsize,
    dropped: AtomicUsize,
    /// `false` (the original behaviour): only the first match is dropped,
    /// proving one lost frame still converges. `true`: every match is
    /// dropped, for proving what happens when a retry never lands at all --
    /// see `Drop::always_with_prefix`.
    always: bool,
}

impl Drop {
    /// Drops the first frame whose body starts with `prefix`.
    pub fn first_with_prefix(prefix: &'static [u8]) -> Arc<Self> {
        Self::first_matching(move |body| body.starts_with(prefix))
    }

    /// Drops the first frame whose body satisfies `matches`, for a case a
    /// prefix cannot state.
    pub fn first_matching(matches: impl Fn(&[u8]) -> bool + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            matches: Box::new(matches),
            seen: AtomicUsize::new(0),
            dropped: AtomicUsize::new(0),
            always: false,
        })
    }

    /// Drops every frame whose body starts with `prefix`, not only the
    /// first -- what a permanently unreachable peer looks like, as opposed
    /// to one lost frame. Exists to prove a bounded retry actually gives up
    /// (and says so) rather than retrying forever, which `first_with_prefix`
    /// cannot exercise: it lets every retry after the first through.
    pub fn always_with_prefix(prefix: &'static [u8]) -> Arc<Self> {
        Arc::new(Self {
            matches: Box::new(move |body| body.starts_with(prefix)),
            seen: AtomicUsize::new(0),
            dropped: AtomicUsize::new(0),
            always: true,
        })
    }

    pub fn dropped(&self) -> usize {
        self.dropped.load(Ordering::Relaxed)
    }

    pub fn seen(&self) -> usize {
        self.seen.load(Ordering::Relaxed)
    }

    fn should_drop(&self, body: &[u8]) -> bool {
        if !(self.matches)(body) {
            return false;
        }
        let seen = self.seen.fetch_add(1, Ordering::Relaxed);
        if self.always || seen == 0 {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            true
        } else {
            false
        }
    }
}

/// Accepts inbound connections and relays each whole to `target`, except for
/// the one frame `drop` says to skip in the inbound-to-outbound direction.
///
/// One direction only. An agent's reply travels back over the *other* side's
/// own outbound connection (`transport::outbound::Peers` -- one connection
/// per peer, opened by whichever side has something to send), which never
/// touches this listener at all. So wrapping one agent's listener with this
/// and filtering only what arrives here is enough to target one direction of
/// one exchange without needing to know anything about the other.
pub async fn serve(listener: TcpListener, target: String, drop: Arc<Drop>) {
    loop {
        let Ok((inbound, _)) = listener.accept().await else {
            return;
        };
        let target = target.clone();
        let drop = Arc::clone(&drop);
        tokio::spawn(async move {
            let Ok(outbound) = TcpStream::connect(&target).await else {
                return;
            };
            let _ = inbound.set_nodelay(true);
            let _ = outbound.set_nodelay(true);
            relay(inbound, outbound, drop).await;
        });
    }
}

async fn relay(inbound: TcpStream, outbound: TcpStream, drop: Arc<Drop>) {
    let (client_read, client_write) = inbound.into_split();
    let (server_read, server_write) = outbound.into_split();
    let forward = tokio::spawn(filtered_copy(client_read, server_write, drop));
    let backward = tokio::spawn(plain_copy(server_read, client_write));
    let _ = tokio::join!(forward, backward);
}

/// The direction nothing here is asked to drop. A byte-exact pipe is cheaper
/// than parsing a stream this test never inspects, and correct for the same
/// reason `p4_link`'s own relay is: TCP does not reorder or drop on its own.
async fn plain_copy(
    mut from: tokio::net::tcp::OwnedReadHalf,
    mut to: tokio::net::tcp::OwnedWriteHalf,
) {
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = match from.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        if to.write_all(&buffer[..read]).await.is_err() {
            break;
        }
    }
    let _ = to.shutdown().await;
}

/// Reads whole frames, exactly as `transport::inbox::read_frames` does, and
/// either forwards one byte-for-byte or swallows it whole when `drop` says
/// to -- never a partial frame either way, so a peer downstream never sees a
/// truncated one.
async fn filtered_copy(
    mut from: tokio::net::tcp::OwnedReadHalf,
    mut to: tokio::net::tcp::OwnedWriteHalf,
    drop: Arc<Drop>,
) {
    loop {
        let mut header = [0u8; HEADER_BYTES];
        if from.read_exact(&mut header).await.is_err() {
            break;
        }
        let Ok(total) = frame::frame_len(&header) else {
            break;
        };
        let mut bytes = vec![0u8; total];
        bytes[..HEADER_BYTES].copy_from_slice(&header);
        if from.read_exact(&mut bytes[HEADER_BYTES..]).await.is_err() {
            break;
        }
        let Ok(decoded) = frame::decode(&bytes) else {
            break;
        };
        if drop.should_drop(&decoded.body) {
            continue;
        }
        if to.write_all(&bytes).await.is_err() {
            break;
        }
    }
    let _ = to.shutdown().await;
}
