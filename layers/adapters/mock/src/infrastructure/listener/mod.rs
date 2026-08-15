//! Tokio listener. Lifecycle stays blocking; execution multiplexes.
//!
//! The first frame on a connection decides its shape, which is the contract a
//! real adapter offers the Agent: an `EXECUTE` opens a persistent multiplexed
//! execution connection, anything else is one lifecycle exchange.

use crate::application::adapter::handle_message;
use crate::application::execution;
use crate::domain::state::Config;
use p4_protocol::{ExecutionRequest, Message, RoutedMessage};
use std::collections::HashMap;
use std::net::TcpListener as StdTcpListener;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::Runtime;
use tokio::sync::{Mutex, Semaphore, mpsc};

mod wire;
use wire::{encode, read_message, restore_request_id};

pub(super) type AsyncError = Box<dyn std::error::Error + Send + Sync>;

const RESPONSE_QUEUE: usize = 4096;
const MAX_CONNECTIONS: usize = 1024;

/// A live route: where to send this correlation's frames and when it expires.
#[derive(Clone)]
struct Route {
    request_id: String,
    deadline_unix_ms: u64,
}

pub(crate) fn serve(
    listener: StdTcpListener,
    config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    listener.set_nonblocking(true)?;
    runtime()?.block_on(accept_loop(listener, config))
}

fn runtime() -> Result<Runtime, std::io::Error> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(
            std::thread::available_parallelism()
                .map(|value| value.get())
                .unwrap_or(1)
                .clamp(2, 32),
        )
        .max_blocking_threads(32)
        .enable_io()
        .enable_time()
        .build()
}

async fn accept_loop(
    listener: StdTcpListener,
    config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::from_std(listener)?;
    let permits = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    loop {
        let (stream, _) = match listener.accept().await {
            Ok(connection) => connection,
            Err(error) => {
                eprintln!("P4_MOCK_ACCEPT_ERROR {error}");
                tokio::time::sleep(Duration::from_millis(10)).await;
                continue;
            }
        };
        let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else {
            continue;
        };
        let config = config.clone();
        tokio::spawn(async move {
            let result = dispatch(stream, config).await;
            drop(permit);
            if let Err(error) = result {
                eprintln!("P4_MOCK_ERROR {error}");
            }
        });
    }
}

async fn dispatch(mut stream: TcpStream, config: Config) -> Result<(), AsyncError> {
    stream.set_nodelay(true)?;
    let routed = read_message(&mut stream).await?;
    if matches!(&routed.message, Message::Execute(_)) {
        return serve_executions(stream, routed, config).await;
    }
    let standard = stream.into_std()?;
    standard.set_nonblocking(false)?;
    tokio::task::spawn_blocking(move || {
        handle_message(standard, routed.message, config).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("mock lifecycle task failed: {error}"))?
    .map_err(Into::into)
}

async fn serve_executions(
    stream: TcpStream,
    first: RoutedMessage,
    config: Config,
) -> Result<(), AsyncError> {
    let (mut reader, mut writer) = stream.into_split();
    let (responses, mut response_rx) = mpsc::channel::<Message>(RESPONSE_QUEUE);
    let routes = Arc::new(Mutex::new(HashMap::<String, Route>::new()));
    let writer_routes = Arc::clone(&routes);
    let writer_task = tokio::spawn(async move {
        while let Some(message) = response_rx.recv().await {
            let route_id = message.correlation_id().to_owned();
            // A frame whose route is gone is dropped, not forced through. That
            // is what makes a cancelled or expired route observable: the
            // caller stops receiving, and no terminal appears from nowhere.
            let Some(route) = writer_routes.lock().await.get(&route_id).cloned() else {
                continue;
            };
            let terminal = message.is_terminal();
            let routed = RoutedMessage {
                route_id: route_id.clone(),
                deadline_unix_ms: route.deadline_unix_ms,
                message: restore_request_id(message, &route.request_id),
            };
            writer.write_all(&encode(&routed)?).await?;
            if terminal {
                writer_routes.lock().await.remove(&route_id);
            }
        }
        Ok::<(), AsyncError>(())
    });

    schedule(first, &config, &responses, &routes).await?;
    loop {
        match read_message(&mut reader).await {
            Ok(routed) if matches!(&routed.message, Message::Execute(_)) => {
                schedule(routed, &config, &responses, &routes).await?;
            }
            Ok(RoutedMessage {
                route_id,
                message: Message::Cancel { .. },
                ..
            }) => {
                routes.lock().await.remove(&route_id);
            }
            Ok(other) => {
                eprintln!(
                    "P4_MOCK_ROUTE_ERROR route={} unsupported={:?}",
                    other.route_id,
                    other.message.kind()
                );
            }
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|value| value.kind() == std::io::ErrorKind::UnexpectedEof) =>
            {
                break;
            }
            Err(error) => return Err(error),
        }
    }
    drop(responses);
    writer_task.await??;
    Ok(())
}

/// Registers the route and starts the simulated generation.
///
/// The admission gate is taken inside the spawned task rather than before it,
/// so an arrival above capacity waits on the semaphore instead of blocking the
/// read loop. A caller that overloads this adapter still gets its frames read.
async fn schedule(
    routed: RoutedMessage,
    config: &Config,
    responses: &mpsc::Sender<Message>,
    routes: &Arc<Mutex<HashMap<String, Route>>>,
) -> Result<(), AsyncError> {
    let RoutedMessage {
        route_id,
        deadline_unix_ms,
        message: Message::Execute(request),
    } = routed
    else {
        return Err("scheduled a frame that is not an execution".into());
    };
    routes.lock().await.insert(
        route_id.clone(),
        Route {
            request_id: request.request_id.clone(),
            deadline_unix_ms,
        },
    );
    let profile = config.deployments.profile(&request.deployment_id);
    let gate = config.deployments.gate(&request.deployment_id);
    let responses = responses.clone();
    let routes = Arc::clone(routes);
    tokio::spawn(async move {
        let _permit = match gate {
            Some(gate) => gate.acquire_owned().await.ok(),
            None => None,
        };
        if expired(deadline_unix_ms) {
            let _ = responses
                .send(Message::Error {
                    request_id: route_id.clone(),
                    detail: "mock deployment saw the deadline pass before it started".into(),
                })
                .await;
            return;
        }
        let internal = internal_correlation(&request, &route_id);
        if execution::run(internal, profile, &responses).await.is_err() {
            routes.lock().await.remove(&route_id);
        }
    });
    Ok(())
}

/// The simulator answers on the transport route, so responses correlate the
/// way the writer expects. The caller's own request id is restored on the way
/// out.
fn internal_correlation(request: &ExecutionRequest, route_id: &str) -> ExecutionRequest {
    let mut internal = request.clone();
    internal.request_id = route_id.to_owned();
    internal
}

fn expired(deadline_unix_ms: u64) -> bool {
    deadline_unix_ms != 0
        && SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_millis() as u64)
            .is_ok_and(|now| now > deadline_unix_ms)
}

#[cfg(test)]
mod tests;
