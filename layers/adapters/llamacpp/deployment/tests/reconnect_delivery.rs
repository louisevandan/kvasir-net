//! What a reconnect is actually for: getting the answer.
//!
//! The client's replay was already proven -- a submission in flight when a
//! socket dies is resent on the next one, exactly once. What was never
//! proven is the half that matters to a caller: that the *results* of the
//! run already in progress come back on the new connection.
//!
//! They did not. The server bound each submission's event sink to the
//! socket the submission arrived on, so a resend of a still-running
//! `submission_id` was answered `Accepted` and nothing else, while every
//! `Produced` and the `Settled` went on being written into a closed pipe.
//! The client waited for output that was already being produced, and the
//! run's own tests passed throughout because they only ever checked that
//! the server settled it internally.
//!
//! This drives the real v2 server over a real socket, kills the connection
//! mid-run, and requires the terminal to arrive on the connection that
//! replaced it.

mod support;

use p4_adapter::deployment::Client as DeploymentClientTrait;
use p4_llamacpp_deployment::DeploymentClient;
use p4_llamacpp_deployment::contract::{Event, Submit};
use p4_llamacpp_deployment::transport::TransportFactory;
use p4_llamacpp_deployment::transport::tcp::TcpTransportFactory;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use support::{CollectingSink, Fixture, neutral_request};

/// A TCP relay in front of the fixture whose sockets a test can cut.
///
/// Nothing in the client or the server exposes "drop this connection", and
/// faking the break inside the client would prove only that the client's
/// own replay works -- which is not what was broken. Cutting the wire is
/// the only version of this that exercises the server's side of a
/// reconnect.
struct Breaker {
    addr: SocketAddr,
    live: Arc<Mutex<Vec<TcpStream>>>,
}

impl Breaker {
    fn in_front_of(upstream: SocketAddr) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind the relay");
        let addr = listener.local_addr().expect("relay address");
        let live: Arc<Mutex<Vec<TcpStream>>> = Arc::new(Mutex::new(Vec::new()));
        let held = Arc::clone(&live);
        std::thread::spawn(move || {
            for accepted in listener.incoming() {
                let Ok(downstream) = accepted else { return };
                eprintln!("BREAKER accepted");
                let Ok(up) = TcpStream::connect(upstream) else {
                    eprintln!("BREAKER upstream connect failed");
                    return;
                };
                let _ = downstream.set_nodelay(true);
                let _ = up.set_nodelay(true);
                let mut sockets = held.lock().expect("relay lock");
                for pair in [&downstream, &up] {
                    sockets.push(pair.try_clone().expect("clone for the kill switch"));
                }
                drop(sockets);
                pump(
                    downstream.try_clone().expect("clone"),
                    up.try_clone().expect("clone"),
                );
                pump(up, downstream);
            }
        });
        Self { addr, live }
    }

    /// Cuts every socket currently open through the relay. Later
    /// connections are accepted normally, which is what the client's
    /// reconnect needs.
    fn cut(&self) {
        let mut sockets = self.live.lock().expect("relay lock");
        for socket in sockets.drain(..) {
            let _ = socket.shutdown(Shutdown::Both);
        }
    }
}

fn pump(mut from: TcpStream, mut to: TcpStream) {
    std::thread::spawn(move || {
        let mut buffer = [0u8; 8192];
        loop {
            match from.read(&mut buffer) {
                Ok(0) | Err(_) => {
                    let _ = to.shutdown(Shutdown::Both);
                    return;
                }
                Ok(read) => {
                    if to.write_all(&buffer[..read]).is_err() {
                        return;
                    }
                }
            }
        }
    });
}

#[test]
fn a_run_in_flight_delivers_its_terminal_on_the_connection_that_replaced_the_dead_one() {
    let fixture = Fixture::spawn();
    let sink = CollectingSink::new();
    let breaker = Breaker::in_front_of(fixture.addr);
    let factory: Arc<dyn TransportFactory> = Arc::new(TcpTransportFactory::for_deployment(
        breaker.addr,
        fixture.deployment_id.clone(),
    ));
    let client = DeploymentClient::connect(
        factory,
        sink.clone(),
        fixture.deployment_id.clone(),
        fixture.deployment_generation,
        Duration::from_millis(20),
    )
    .expect("connect");

    client
        .try_submit(Submit {
            deployment_id: fixture.deployment_id.clone(),
            deployment_generation: fixture.deployment_generation,
            submission_id: "s-reconnect".into(),
            deadline_unix_ms: 0,
            request: neutral_request(),
        })
        .expect("the submission is enqueued");

    // Wait until one chunk reached the client before breaking anything. A
    // cut after only Accepted misses the hard case: the replacement socket
    // receives the server journal's already-delivered Produced prefix.
    sink.wait_for(Duration::from_secs(10), |events| {
        events
            .iter()
            .any(|event| matches!(event, Event::Produced(produced) if produced.event_ordinal == 0))
    });

    // The fixture emits its chunks 60ms apart, so this lands inside the
    // run rather than after it.
    breaker.cut();

    // The terminal has to arrive. Before the server looked its sink up per
    // event, this waited the full timeout while the run finished into a
    // socket nobody was reading.
    sink.wait_for(Duration::from_secs(20), |events| {
        events
            .iter()
            .any(|event| matches!(event, Event::Settled(_)))
    });

    let events = sink.snapshot();
    let settled = events
        .iter()
        .filter(|event| matches!(event, Event::Settled(_)))
        .count();
    assert_eq!(
        settled, 1,
        "exactly one terminal reaches the caller across the reconnect: {events:?}"
    );
    let produced = events
        .iter()
        .filter_map(|event| match event {
            Event::Produced(produced) => Some((produced.event_ordinal, produced.text.as_str())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        produced,
        vec![(0, "He"), (1, "llo")],
        "the reconnect must recover the complete body with contiguous ordinals"
    );

    client.close();
}
