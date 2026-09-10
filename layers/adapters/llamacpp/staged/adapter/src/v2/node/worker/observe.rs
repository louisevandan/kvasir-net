use super::*;
use p4_protocol::event::{Envelope, OuterEndpoint};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub(super) enum TelemetryPayload {
    Batch(BatchObservation),
    Span(StageSpan),
}

/// Fully prepared recipient-owned data; only the local forward timestamp is
/// filled after forwarding. No request/engine state is read while publishing.
#[derive(Clone, Debug)]
pub(super) struct PreparedTelemetry {
    pub base: Envelope,
    pub reply: ReplySpec,
    pub ingress: Address,
    pub payload: TelemetryPayload,
}

impl PreparedTelemetry {
    pub(super) fn forwarded_at(&mut self, stamp: u64) {
        if let TelemetryPayload::Span(span) = &mut self.payload {
            span.forward_unix_ms = stamp;
        }
    }
    fn validate_encoding(&self) -> Result<(), String> {
        match &self.payload {
            TelemetryPayload::Batch(value) => serde_json::to_vec(value),
            TelemetryPayload::Span(value) => serde_json::to_vec(value),
        }
        .map(|_| ())
        .map_err(|e| format!("observation encoding failed: {e}"))
    }
}

struct Recipient {
    route: OuterEndpoint,
    base: Envelope,
    reply: ReplySpec,
    owners: BTreeMap<String, Option<String>>,
}

impl Worker {
    fn observation_recipients(
        &self,
        base: &Event,
        session: &str,
        rows: &[&RowOwner],
        head: bool,
    ) -> Result<Vec<Recipient>, String> {
        let mut recipients: Vec<Recipient> = Vec::new();
        let mut identities = BTreeMap::new();
        for owner in rows {
            if owner.load_generation != self.state.load_generation
                || owner.session_id != session
                || !owner.has_canonical_request_identity()
                || owner.incarnation == 0
            {
                return Err("observation row identity is invalid".into());
            }
            let reply: ReplySpec = serde_json::from_str(&owner.reply)
                .map_err(|_| "observation reply contract is invalid")?;
            if reply.channel.is_empty()
                || reply.channel.contains('\0')
                || reply.ingress_agent.contains('\0')
                || reply.correlation_id.is_empty()
                || reply.connection_generation == 0
            {
                return Err("observation reply contract is incomplete".into());
            }
            let ingress = Address::from_str(&reply.ingress_agent)
                .map_err(|_| "observation reply ingress is invalid")?;
            let route = OuterEndpoint {
                ingress_agent: ingress,
                channel: reply.channel.clone(),
                connection_generation: reply.connection_generation,
            };
            Endpoint::Outer(route.clone())
                .validate()
                .map_err(|e| e.to_string())?;
            let identity = (owner.sequence_id, owner.incarnation, reply.clone());
            if identities
                .insert(owner.sequence_key.clone(), identity.clone())
                .is_some_and(|old| old != identity)
            {
                return Err("observation request has inconsistent ownership".into());
            }
            let (carrier, submission) = if head {
                let request = self
                    .state
                    .requests
                    .get(&owner.sequence_key)
                    .ok_or("observation request is missing")?;
                let authority = request.issue_authority()?;
                if authority.head != self.endpoint
                    || authority.outer != route
                    || authority.load_generation != owner.load_generation
                    || authority.session_id != owner.session_id
                    || authority.request_id != owner.request_id
                    || authority.sequence_id != owner.sequence_id
                    || authority.incarnation != owner.incarnation
                    || request.reply != owner.reply
                {
                    return Err("observation row differs from the original submission".into());
                }
                (
                    request.template.envelope.clone(),
                    Some(authority.submission_event_id),
                )
            } else {
                // The physical wire has no original submission event ID.
                // OUTER joins these owners to the head's approved attempt.
                (base.envelope.clone(), None)
            };
            let index = match recipients.iter().position(|group| group.route == route) {
                Some(index) => index,
                None => {
                    recipients.push(Recipient {
                        route,
                        base: carrier,
                        reply,
                        owners: BTreeMap::new(),
                    });
                    recipients.len() - 1
                }
            };
            recipients[index]
                .owners
                .insert(owner.sequence_key.clone(), submission);
        }
        Ok(recipients)
    }

    pub(super) fn validate_observation_rows(
        &self,
        base: &Event,
        session: &str,
        rows: &[&RowOwner],
        head: bool,
    ) -> Result<(), String> {
        self.observation_recipients(base, session, rows, head)
            .map(|_| ())
    }

    /// Predicted indices are candidates, not approval. The committed witness
    /// is independently compared before any forward/observation publication.
    pub(super) fn prepare_batch_observation(
        &self,
        base: &Event,
        session: &str,
        ordinal: u64,
        logical_rows: usize,
        physical: &CapsuleSet,
        pacing: BatchPacing,
    ) -> Result<Vec<PreparedTelemetry>, String> {
        if ordinal == 0 {
            return Err("observation issue ordinal is zero".into());
        }
        let rows = physical
            .0
            .iter()
            .flat_map(|c| &c.owners)
            .collect::<Vec<_>>();
        let recipients = self.observation_recipients(base, session, &rows, true)?;
        let mut deliveries = Vec::new();
        for recipient in recipients {
            let mut batches = Vec::new();
            for capsule in &physical.0 {
                let mut requests = BTreeMap::<String, BatchRequestObservation>::new();
                let mut request_ids = BTreeSet::new();
                let mut sequence_ids = BTreeSet::new();
                let mut counts = [0; 4];
                for owner in &capsule.owners {
                    request_ids.insert(&owner.request_id);
                    sequence_ids.insert(owner.sequence_id);
                    counts[phase_index(owner.phase)] += 1;
                    let Some(submission) = recipient.owners.get(&owner.sequence_key) else {
                        continue;
                    };
                    let request = self
                        .state
                        .requests
                        .get(&owner.sequence_key)
                        .ok_or("observation request disappeared")?;
                    let index = request
                        .issued_work
                        .map(|w| w.issue_count())
                        .unwrap_or(0)
                        .checked_add(1)
                        .ok_or("observation issue index overflow")?;
                    let detail = requests
                        .entry(owner.sequence_key.clone())
                        .or_insert_with(|| BatchRequestObservation {
                            request_id: owner.request_id.clone(),
                            submission_event_id: submission.clone().expect("head authority"),
                            sequence_id: owner.sequence_id,
                            incarnation: owner.incarnation,
                            request_issue_index: index,
                            rows: Vec::new(),
                            prefill_rows: 0,
                            decode_rows: 0,
                            verify_rows: 0,
                            replay_rows: 0,
                        });
                    detail.rows.push(IssuedRow {
                        phase: owner.phase,
                        position: owner.position,
                    });
                    match owner.phase {
                        Phase::Prefill => detail.prefill_rows += 1,
                        Phase::Decode => detail.decode_rows += 1,
                        Phase::Verify => detail.verify_rows += 1,
                        Phase::Replay => detail.replay_rows += 1,
                    }
                }
                for detail in requests.values_mut() {
                    detail
                        .rows
                        .sort_by_key(|row| (phase_index(row.phase), row.position));
                }
                batches.push(PhysicalBatchObservation {
                    execution_id: capsule.execution_id,
                    rows: capsule.owners.len(),
                    prefill_rows: counts[0],
                    decode_rows: counts[1],
                    verify_rows: counts[2],
                    replay_rows: counts[3],
                    request_count: request_ids.len(),
                    sequence_count: sequence_ids.len(),
                    owned_requests: requests.into_values().collect(),
                });
            }
            let ids = batches
                .iter()
                .map(|b| b.execution_id.to_string())
                .collect::<Vec<_>>()
                .join("-");
            let observation = BatchObservation {
                scheduling: pacing.scheduling.clone(),
                observation_id: format!("{session}:{ordinal}:{ids}"),
                load_generation: self.state.load_generation,
                session_id: session.to_owned(),
                logical_ordinal: ordinal,
                logical_rows,
                mixed_physical_batches: batches
                    .iter()
                    .filter(|b| {
                        b.prefill_rows > 0 && b.decode_rows + b.verify_rows + b.replay_rows > 0
                    })
                    .count(),
                physical_batches: batches,
                stage_ms: pacing.stage_ms,
                idle_ms: pacing.idle_ms,
                idle_gated: pacing.idle_gated,
                ready_rows: pacing.ready_rows,
                ready_sequences: pacing.ready_sequences,
            };
            let delivery = PreparedTelemetry {
                base: recipient.base,
                reply: recipient.reply,
                ingress: recipient.route.ingress_agent,
                payload: TelemetryPayload::Batch(observation),
            };
            delivery.validate_encoding()?;
            deliveries.push(delivery);
        }
        Ok(deliveries)
    }

    pub(super) fn validate_accepted_observations(
        &self,
        deliveries: &[PreparedTelemetry],
    ) -> Result<(), String> {
        for delivery in deliveries {
            let TelemetryPayload::Batch(batch) = &delivery.payload else {
                continue;
            };
            for detail in batch
                .physical_batches
                .iter()
                .flat_map(|b| &b.owned_requests)
            {
                let request = self
                    .state
                    .requests
                    .get(&request_key(&batch.session_id, &detail.request_id))
                    .ok_or("accepted observation request is missing")?;
                let witness = request
                    .issued_work
                    .ok_or("accepted observation has no witness")?;
                if witness.last_ordinal() != batch.logical_ordinal
                    || witness.issue_count() != detail.request_issue_index
                {
                    return Err("observation index differs from the accepted issue witness".into());
                }
            }
        }
        Ok(())
    }

    pub(super) fn prepare_stage_span(
        &self,
        base: &Event,
        session: &str,
        physical: &CapsuleSet,
        ingress_unix_ms: u64,
        start_unix_ms: u64,
        end_unix_ms: u64,
        head: bool,
    ) -> Result<Vec<PreparedTelemetry>, String> {
        let rows = physical
            .0
            .iter()
            .flat_map(|c| &c.owners)
            .collect::<Vec<_>>();
        let recipients = self.observation_recipients(base, session, &rows, head)?;
        let mut deliveries = Vec::new();
        for recipient in recipients {
            let executions = physical
                .0
                .iter()
                .map(|capsule| {
                    let mut owners = BTreeMap::new();
                    for owner in &capsule.owners {
                        if recipient.owners.contains_key(&owner.sequence_key) {
                            owners.insert(
                                owner.sequence_key.clone(),
                                StageRequestObservation {
                                    request_id: owner.request_id.clone(),
                                    sequence_id: owner.sequence_id,
                                    incarnation: owner.incarnation,
                                },
                            );
                        }
                    }
                    StageExecutionObservation {
                        execution_id: capsule.execution_id,
                        owned_requests: owners.into_values().collect(),
                    }
                })
                .collect();
            let span = StageSpan {
                load_generation: self.state.load_generation,
                session_id: session.to_owned(),
                execution_ids: physical.0.iter().map(|c| c.execution_id).collect(),
                executions,
                rows: rows.len(),
                ingress_unix_ms,
                start_unix_ms,
                end_unix_ms,
                forward_unix_ms: 0,
            };
            let delivery = PreparedTelemetry {
                base: recipient.base,
                reply: recipient.reply,
                ingress: recipient.route.ingress_agent,
                payload: TelemetryPayload::Span(span),
            };
            delivery.validate_encoding()?;
            deliveries.push(delivery);
        }
        Ok(deliveries)
    }
}

fn phase_index(phase: Phase) -> usize {
    match phase {
        Phase::Prefill => 0,
        Phase::Decode => 1,
        Phase::Verify => 2,
        Phase::Replay => 3,
    }
}
/// Cross-process timestamp; clock synchronization is a separate prerequisite.
pub(super) fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}
