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
