use crate::{
    construction::Input,
    ipc::{self, Job},
    process::{Launch, Worker},
};
use p4_adapter::node_adapter::*;
use p4_protocol::{
    Address,
    event::{Endpoint, Event, EventClass},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Wake, Waker},
};
use tokio::sync::{Notify, mpsc};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Node {
    agent: String,
    node: String,
    generation: u64,
}
impl Node {
    fn endpoint(&self) -> Result<Endpoint, String> {
        let address: Address = self
            .agent
            .parse()
            .map_err(|e| format!("invalid agent address: {e}"))?;
        let endpoint = Endpoint::node(address, &self.node, self.generation);
        endpoint.validate().map_err(|e| e.to_string())?;
        Ok(endpoint)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Load {
    op: String,
    generation: u64,
    launch: Launch,
    nodes: Vec<Node>,
    index: usize,
}

struct State {
    worker: Option<Worker>,
    generation: u64,
    epoch: u64,
    serial: u64,
    nodes: Vec<Endpoint>,
    index: usize,
    owner: Option<Endpoint>,
    uncertain: Option<String>,
    cleanup_error: Option<String>,
}

impl State {
    async fn command(
        &mut self,
        endpoint: &Endpoint,
        event: &Event,
        meta: Value,
        body: &[u8],
    ) -> Result<(Value, Vec<u8>, Endpoint), String> {
        if &event.envelope.target != endpoint
            || event.envelope.adapter_kind.as_deref() != Some("hf-transformers")
            || event.envelope.payload_content_type != ipc::COMMAND
        {
            return Err("wrong adapter/target/content type".into());
        }
        if meta["op"] == "load" {
            let load: Load = serde_json::from_value(meta).map_err(|e| e.to_string())?;
            if load.op != "load"
                || !body.is_empty()
                || self.worker.is_some()
                || self.uncertain.is_some()
                || load.generation <= self.generation
                || load.nodes.is_empty()
                || load.nodes.len() > 16
                || load.index >= load.nodes.len()
                || !matches!(event.envelope.source, Endpoint::Outer(_))
                || event
                    .envelope
                    .return_route
                    .as_ref()
                    .map(|r| Endpoint::Outer(r.clone()))
                    != Some(event.envelope.source.clone())
            {
                return Err("invalid/duplicate/stale LOAD".into());
            }
            let nodes = load
                .nodes
                .iter()
                .map(Node::endpoint)
                .collect::<Result<Vec<_>, _>>()?;
            if nodes[load.index] != *endpoint
                || nodes
                    .iter()
                    .enumerate()
                    .any(|(i, n)| nodes[..i].contains(n))
            {
                return Err("LOAD topology mismatch".into());
            }
            if load.launch.identity["generation"] != load.generation || load.launch.identity["index"] != load.index
                || load.launch.identity["nodes"] != serde_json::to_value(&load.nodes.iter().map(|n| json!({"agent":n.agent,"node":n.node,"generation":n.generation})).collect::<Vec<_>>()).unwrap() {
                return Err("LOAD handshake topology/generation mismatch".into());
            }
            // Burn the generation before spawn: a failed/partial initialization cannot be replayed.
            self.generation = load.generation;
            self.nodes = nodes.clone();
            self.index = load.index;
            self.owner = Some(event.envelope.source.clone());
            let (worker, ready) = Worker::start(load.launch).await.map_err(|error| {
                if error.cleanup_error.is_some() {
                    self.uncertain = Some(error.detail.clone());
                }
                self.cleanup_error = error.cleanup_error;
                self.worker = error.worker;
                error.detail
            })?;
            self.worker = Some(worker);
            self.epoch = 1;
            self.serial = 0;
            self.nodes = nodes;
            self.index = load.index;
            self.owner = Some(event.envelope.source.clone());
            return Ok((
                json!({"ok":true,"op":"loaded","generation":self.generation,"ready":ready}),
                vec![],
                event.envelope.source.clone(),
            ));
        }
        let job: Job =
            serde_json::from_value(meta["job"].clone()).map_err(|e| format!("invalid job: {e}"))?;
        if job.generation != self.generation {
            return Err("stale load generation".into());
        }
        let owner = self.owner.as_ref().ok_or("not loaded")?.clone();
        let direct = matches!(job.kind.as_str(), "unload" | "abort" | "cache");
        let source = if direct || self.index == 0 {
            &owner
        } else {
            &self.nodes[self.index - 1]
        };
        if &event.envelope.source != source
            || event
                .envelope
                .return_route
                .as_ref()
                .map(|r| Endpoint::Outer(r.clone()))
                != Some(owner.clone())
        {
            return Err("job source/return route mismatch".into());
        }
        if job.kind == "abort" {
            // Abort is explicit abandonment of this LOAD generation, including a partially
            // completed epoch barrier. It is never a replayable request/epoch operation.
            if !body.is_empty()
                || job.epoch != 0
                || job.serial != 0
                || !job.request.is_empty()
                || job.issue != 0
                || job.position != 0
            {
                return Err("invalid load-abort identity".into());
            }
            let cleanup = if let Some(worker) = self.worker.as_mut() {
                worker.stop(true).await.err()
            } else {
                None
            };
            if let Some(error) = cleanup {
                self.cleanup_error = Some(error.clone());
                return Err(error);
            }
            self.worker = None;
            let first = self.uncertain.take();
            self.cleanup_error = None;
            return Ok((
                json!({"ok":true,"op":"aborted","job":job,"first_error":first,"graceful":false}),
                vec![],
                owner,
            ));
        }
        if let Some(error) = &self.uncertain {
            return Err(format!("fenced: {error}"));
        }
        let epoch_change = job.kind == "epoch";
        if job.epoch != self.epoch + u64::from(epoch_change)
            || job.serial != self.serial.checked_add(1).ok_or("serial exhausted")?
            || job.request.len() > 128
            || !matches!(
                job.kind.as_str(),
                "step" | "release" | "cancel" | "epoch" | "unload" | "cache"
            )
        {
            return Err("stale/out-of-order/unsupported job".into());
        }
        let receipts = meta["receipts"].as_array().ok_or("missing receipt chain")?;
        let required = if direct { 0 } else { self.index };
        if receipts.len() != required
            || receipts
                .iter()
                .enumerate()
                .any(|(i, r)| r["index"] != i || r["job"] != meta["job"] || r["ok"] != true)
        {
            return Err("invalid prior settlement chain".into());
        }
        let worker = self.worker.as_mut().ok_or("not loaded")?;
        let packet = ipc::pack(&json!({"job":job}), body, worker.launch.frame_bytes)?;
        // Once written, an unknown result is fenced. No serial retry is ever issued automatically.
        let bytes = match worker.exchange(&packet).await {
            Ok(bytes) => bytes,
            Err(error) => {
                self.uncertain = Some(error.clone());
                return Err(error);
            }
        };
        let (reply, output) = match ipc::unpack(&bytes) {
            Ok(value) => value,
            Err(error) => {
                self.uncertain = Some(error.clone());
                return Err(error);
            }
        };
        if reply["job"] != meta["job"] || !reply["ok"].is_boolean() {
            self.uncertain = Some("worker result identity mismatch".into());
            return Err(self.uncertain.clone().unwrap());
        }
        if reply["ok"] != true {
            let error = reply["error"]
                .as_str()
                .unwrap_or("worker rejected")
                .to_string();
            if reply["disposition"] != "rejected" {
                self.uncertain = Some(error.clone());
            }
            return Err(error);
        }
        if job.kind == "unload" {
            if reply["active"] != 0 || !output.is_empty() {
                self.uncertain = Some("unload without quiescence".into());
                return Err(self.uncertain.clone().unwrap());
            }
            if let Err(error) = worker.stop(false).await {
                self.cleanup_error = Some(error.clone());
                self.uncertain = Some(error.clone());
                return Err(error);
            }
            self.worker = None;
        }
        if job.kind != "cache" {
            self.serial = job.serial;
        }
        if epoch_change {
            self.epoch = job.epoch;
        }
        let mut chain = receipts.clone();
        chain.push(json!({"index":self.index,"job":job,"ok":true,"report":reply["report"]}));
        let target = if !direct && self.index + 1 < self.nodes.len() {
            self.nodes[self.index + 1].clone()
        } else {
            owner
        };
        Ok((
            json!({"ok":true,"job":job,"receipts":chain}),
            output.to_vec(),
            target,
        ))
    }
}

struct Signal(Arc<Notify>);
impl Wake for Signal {
    fn wake(self: Arc<Self>) {
        self.0.notify_one();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.notify_one();
    }
}

pub(crate) async fn run(
    endpoint: Endpoint,
    mut receiver: mpsc::Receiver<Input>,
    publisher: CompletionPublisher,
    mailbox: Arc<CompletionMailbox>,
    snapshot: Arc<Mutex<String>>,
    stop: Arc<AtomicBool>,
    notify: Arc<Notify>,
) {
    let mut state = State {
        worker: None,
        generation: 0,
        epoch: 0,
        serial: 0,
        nodes: vec![],
        index: 0,
        owner: None,
        uncertain: None,
        cleanup_error: None,
    };
    let mut sequence = 0u64;
    let mut failed_input: Option<Input> = None;
    let capacity = Arc::new(Notify::new());
    let waker = Waker::from(Arc::new(Signal(capacity.clone())));
    let _registration = publisher
        .capacity_listener(&waker)
        .expect("one capacity listener");
    loop {
        let input = tokio::select! { value=receiver.recv()=>value, _=notify.notified()=>None };
        let Some(input) = input else { break };
        if stop.load(Ordering::Acquire) {
            break;
        }
        let event = input.completion.as_ref().unwrap().event();
        *snapshot.lock().unwrap() = "busy".into();
        sequence = match sequence.checked_add(1) {
            Some(n) => n,
            None => break,
        };
        let context = match event.envelope.return_context() {
            Ok(context) => context,
            Err(error) => {
                *snapshot.lock().unwrap() = format!("failed: {error}");
                notify.notified().await;
                drop(input);
                break;
            }
        };
        let envelope = match context.reply(&event.envelope,
            format!("hf:{endpoint:?}:{sequence}"), endpoint.clone(),
            EventClass::Output, sequence, ipc::RESULT) {
            Ok(envelope) => envelope,
            Err(error) => {
                *snapshot.lock().unwrap() = format!("failed: {error}");
                notify.notified().await;
                drop(input);
                break;
            }
        };
        let mut output = Event {
            envelope,
            payload: vec![],
        };
        let parsed = ipc::unpack(&event.payload);
        let frame = parsed
            .as_ref()
            .ok()
            .and_then(|(m, _)| m.get("launch"))
            .and_then(|m| m["frame_bytes"].as_u64())
            .map(|n| n as usize)
            .or_else(|| state.worker.as_ref().map(|w| w.launch.frame_bytes))
            .unwrap_or(1024)
            .clamp(256, ipc::FRAME_LIMIT);
        // Charge the largest possible output endpoint before Python can have effects.
        let mut envelope_cost = retained_event_bytes(&output).unwrap_or(usize::MAX);
        for node in &state.nodes {
            let mut candidate = output.clone();
            candidate.envelope.target = node.clone();
            envelope_cost =
                envelope_cost.max(retained_event_bytes(&candidate).unwrap_or(usize::MAX));
        }
        // This bound includes the receipt header as well as the opaque body. An oversized
        // worker result is retained/fenced; it cannot escape as a successful completion.
        let output_bound = frame;
        let cost = envelope_cost.saturating_add(output_bound);
        let reservation = loop {
            match publisher.try_reserve(1, cost) {
                Ok(r) => break Some(r),
                Err(ReserveError::Full) => {
                    tokio::select! {_=capacity.notified()=>{},_=notify.notified()=>break None}
                }
                Err(_) => break None,
            }
        };
        let Some(reservation) = reservation else {
            // No worker effect occurred. Emit a bounded diagnostic when the declared response cannot fit.
            output.payload = ipc::pack(
                &json!({"ok":false,"error":"response reservation exceeds retained budget"}),
                &[],
                1024,
            )
            .unwrap();
            let result = publisher.try_publish_owned(output);
            if result.is_err() {
                *snapshot.lock().unwrap() = "failed: response budget".into();
                // Retain the refused event and accepted input until explicit owner abandonment.
                notify.notified().await;
                drop(result);
                break;
            }
            continue;
        };
        let operation = async {
            match parsed {
                Ok((meta, body)) => {
                    if (meta["op"] == "load"
                        || matches!(
                            meta["job"]["kind"].as_str(),
                            Some("unload" | "abort" | "epoch")
                        ))
                        && mailbox.storage_snapshot().retained_count > 1
                    {
                        Err("lifecycle barrier has unretired output".into())
                    } else {
                        state.command(&endpoint, event, meta, body).await
                    }
                }
                Err(error) => Err(error),
            }
        };
        let result = tokio::select! {value=operation=>Some(value),_=notify.notified()=>None};
        let Some(result) = result else { break };
        let (meta, body, target) = match result {
            Ok(value) => value,
            Err(error) => (
                json!({"ok":false,"error":error,"uncertain":state.uncertain,
                "cleanup_error":state.cleanup_error}),
                vec![],
                output.envelope.target.clone(),
            ),
        };
        output.envelope.target = target;
        if matches!(output.envelope.target, Endpoint::Node { .. }) {
            output.envelope.payload_content_type = ipc::COMMAND.into();
            output.envelope.class = EventClass::Data;
        }
        output.payload = match ipc::pack(&meta, &body, output_bound) {
            Ok(payload) => payload,
            Err(error) => {
                state.uncertain = Some(error.clone());
                output.envelope.target = Endpoint::Outer(context.route.clone());
                output.envelope.class = EventClass::Output;
                output.envelope.payload_content_type = ipc::RESULT.into();
                ipc::pack(
                    &json!({"ok":false,"error":error,"uncertain":true}),
                    &[],
                    frame,
                )
                .unwrap()
            }
        };
        let mut pending = Some((output, reservation));
        while let Some((event, reservation)) = pending.take() {
            match publisher.publish_reserved(event, reservation) {
                Ok(()) => break,
                Err(error) => {
                    if error.reason != ReservedPublishReason::Full {
                        *snapshot.lock().unwrap() = "failed: publication".into();
                        notify.notified().await;
                        drop(error);
                        break;
                    }
                    pending = Some((error.event, error.reservation));
                    tokio::select! {_=capacity.notified()=>{},_=notify.notified()=>break}
                }
            }
        }
        if state.uncertain.is_some() && failed_input.is_none() {
            failed_input = Some(input);
        } else {
            drop(input);
        }
        if state.worker.is_none() && state.uncertain.is_none() {
            failed_input = None;
        }
        if state.uncertain.is_none() {
            if let Some(worker) = state.worker.as_mut() {
                worker.last_response.clear();
            }
        }
        *snapshot.lock().unwrap() = if state.uncertain.is_some() {
            "uncertain"
        } else if state.worker.is_some() {
            "ready"
        } else {
            "unloaded"
        }
        .into();
        // Held outputs remain in the authoritative mailbox budget after dequeue.
        let _ = mailbox.storage_snapshot();
    }
    if let Some(worker) = state.worker.as_mut() {
        let _ = worker.stop(true).await;
    }
    *snapshot.lock().unwrap() = "closed".into();
}
