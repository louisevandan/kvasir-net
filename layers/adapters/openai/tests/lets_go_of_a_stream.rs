//! What happens to a connection when nobody wants it any more.
//!
//! Separate from the conversation tests next door because it is about the
//! opposite moment: not what a stream says, but what becomes of it once the
//! sequence using it is gone. That is a resource question rather than a
//! protocol one, and it is the half a backend feels — it has a finite number
//! of workers, and every stream nobody ended is one of them held.

use p4_openai::endpoint::Endpoint;
use p4_openai::session::Session;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// A stream that opens and then says nothing, which is a backend still
/// thinking about its first token.
fn silent_stub() -> (u16, Arc<AtomicUsize>) {
    let held = Arc::new(AtomicUsize::new(0));
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let counter = Arc::clone(&held);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let counter = Arc::clone(&counter);
            std::thread::spawn(move || {
                let mut stream = stream;
                let mut reader = BufReader::new(stream.try_clone().expect("clone"));
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap_or(0) > 0 {
                    if line.trim().is_empty() {
                        break;
                    }
                    line.clear();
                }
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n"
                );
                let _ = stream.flush();
                counter.fetch_add(1, Ordering::SeqCst);
                // Answer nothing further, and hold the connection until the
                // other end ends it. Reading is how we learn that it did.
                let mut sink = Vec::new();
                use std::io::Read;
                let _ = stream.read_to_end(&mut sink);
                counter.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });
    (port, held)
}

/// Letting go of a sequence ends its connection, rather than leaving it to a
/// read timeout.
///
/// The reader thread owns the socket and is blocked on it, so it cannot close
/// anything itself and will not notice the session is gone. The timeout that
/// would eventually free it is deliberately generous — a first token can be far
/// off — which made every abandoned sequence hold a connection and a thread for
/// a quarter of an hour. Sixty-four of those left a backend that serves one
/// connection per generation with no room: of eighty later requests, nineteen
/// reached it and the rest waited on a worker that was never coming back.
#[test]
fn dropping_a_session_closes_its_connection() {
    let (port, held) = silent_stub();
    let mut endpoint = Endpoint::new("127.0.0.1", port);
    // Long, as a real plan's is. The point is that the close does not wait for
    // it: a test that passed because the timeout expired would prove nothing.
    endpoint.idle = Duration::from_secs(600);

    let session = Session::start(&endpoint, "m", "prompt", 8, "{}").expect("started");
    until(
        || held.load(Ordering::SeqCst) == 1,
        "the server took the connection",
    );

    drop(session);
    until(
        || held.load(Ordering::SeqCst) == 0,
        "the connection ended when the session did",
    );
}

/// Waits for a condition, and says which one if it never comes.
fn until(mut ready: impl FnMut() -> bool, claim: &str) {
    for _ in 0..600 {
        if ready() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("{claim}");
}
