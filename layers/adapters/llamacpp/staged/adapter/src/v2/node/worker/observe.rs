use super::*;
use std::collections::{BTreeMap, HashSet};

impl Worker {
    pub(super) fn emit_batch_observation(
        &mut self,
        base: &Event,
        session_id: &str,
        logical_rows: usize,
        physical: &CapsuleSet,
        pacing: BatchPacing,
    ) -> Result<(), ()> {
        let mut replies = Vec::new();
        let mut physical_batches = Vec::with_capacity(physical.0.len());
        for capsule in &physical.0 {
            let mut request_ids = HashSet::new();
            let mut sequence_ids = HashSet::new();
            let mut request_rows = BTreeMap::<String, (usize, usize, usize, usize)>::new();
            let mut prefill_rows = 0usize;
            let mut decode_rows = 0usize;
            let mut verify_rows = 0usize;
            let mut replay_rows = 0usize;
            for owner in &capsule.owners {
                request_ids.insert(owner.request_id.as_str());
                sequence_ids.insert(owner.sequence_id);
                match owner.phase {
                    Phase::Prefill => {
                        prefill_rows += 1;
                        request_rows.entry(owner.request_id.clone()).or_default().0 += 1;
                    }
                    Phase::Decode => {
                        decode_rows += 1;
                        request_rows.entry(owner.request_id.clone()).or_default().1 += 1;
                    }
                    Phase::Verify => {
                        verify_rows += 1;
                        request_rows.entry(owner.request_id.clone()).or_default().2 += 1;
                    }
                    Phase::Replay => {
                        replay_rows += 1;
                        request_rows.entry(owner.request_id.clone()).or_default().3 += 1;
                    }
                }
                let reply: ReplySpec = serde_json::from_str(&owner.reply).map_err(|_| ())?;
                if !replies.contains(&reply) {
                    replies.push(reply);
                }
            }
            physical_batches.push(PhysicalBatchObservation {
                execution_id: capsule.execution_id,
                rows: capsule.owners.len(),
                prefill_rows,
                decode_rows,
                verify_rows,
                replay_rows,
                request_count: request_ids.len(),
                sequence_count: sequence_ids.len(),
                requests: request_rows
                    .into_iter()
                    .map(
                        |(request_id, (prefill_rows, decode_rows, verify_rows, replay_rows))| {
                            BatchRequestObservation {
                                request_id,
                                prefill_rows,
                                decode_rows,
                                verify_rows,
                                replay_rows,
                            }
                        },
                    )
                    .collect(),
            });
        }
        let mixed_physical_batches = physical_batches
            .iter()
            .filter(|batch| {
                batch.prefill_rows > 0
                    && batch.decode_rows + batch.verify_rows + batch.replay_rows > 0
            })
            .count();
        let execution_ids = physical_batches
            .iter()
            .map(|batch| batch.execution_id.to_string())
            .collect::<Vec<_>>()
            .join("-");
        let observation = BatchObservation {
            observation_id: format!("{session_id}:{execution_ids}"),
            load_generation: self.state.load_generation,
            session_id: session_id.to_owned(),
            logical_rows,
            physical_batches,
            mixed_physical_batches,
            stage_ms: pacing.stage_ms,
            idle_ms: pacing.idle_ms,
            idle_gated: pacing.idle_gated,
            ready_rows: pacing.ready_rows,
            ready_sequences: pacing.ready_sequences,
        };
        for reply in replies {
            let ingress = Address::from_str(&reply.ingress_agent).map_err(|_| ())?;
            self.emit_reply_json(
                base,
                reply,
                ingress,
                EventClass::Telemetry,
                BATCH_OBSERVATION_CONTENT_TYPE,
                &observation,
            )?;
        }
        Ok(())
    }
}

/// Milliseconds since the Unix epoch, for a span other processes will read.
pub(super) fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

impl Worker {
    /// Reports one node's handling of one batch to every outer that owns a
    /// row in it. Emitted by first, middle and last nodes alike, which is the
    /// point: the first node's own pacing was measured before this and could
    /// not say what the other three were doing at the time.
    pub(super) fn emit_stage_span(
        &mut self,
        base: &Event,
        session_id: &str,
        physical: &CapsuleSet,
        ingress_unix_ms: u64,
        start_unix_ms: u64,
        end_unix_ms: u64,
    ) -> Result<(), ()> {
        // One span per node per batch. It is routed by a request correlation
        // because that is how the outer admits telemetry, but the span is
        // about the batch, so any one owner will do - the first. A version
        // that told every owner produced a span per request per node per
        // batch: 155,112 of them for 1,768 batches, and a 70 MB artifact.
        let rows = physical.0.iter().map(|capsule| capsule.owners.len()).sum::<usize>();
        let Some(owner) = physical.0.iter().flat_map(|capsule| &capsule.owners).next() else {
            return Ok(());
        };
        let reply: ReplySpec = serde_json::from_str(&owner.reply).map_err(|_| ())?;
        let replies = vec![reply];
        let span = StageSpan {
            load_generation: self.state.load_generation,
            session_id: session_id.to_owned(),
            execution_ids: physical.0.iter().map(|capsule| capsule.execution_id).collect(),
            rows,
            ingress_unix_ms,
            start_unix_ms,
            end_unix_ms,
            forward_unix_ms: unix_ms(),
        };
        for reply in replies {
            let ingress = Address::from_str(&reply.ingress_agent).map_err(|_| ())?;
            self.emit_reply_json(
                base,
                reply,
                ingress,
                EventClass::Telemetry,
                STAGE_SPAN_CONTENT_TYPE,
                &span,
            )?;
        }
        Ok(())
    }
}
