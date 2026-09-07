use super::{RunConfig, config::node_endpoint};
use p4_llamacpp_staged_adapter::v2::{BatchObservation, OutcomePayload, ReleaseReceipt, StageSpan};
use p4_protocol::event::{Endpoint, Event, EventClass, OuterEndpoint};
use std::collections::BTreeSet;

pub(super) struct InferenceIdentity {
    outer: Endpoint,
    outer_route: OuterEndpoint,
    first: Endpoint,
    nodes: Vec<Endpoint>,
    load_generation: u64,
    session_id: String,
    first_output_position: Option<usize>,
}

impl InferenceIdentity {
    pub(super) fn new(
        config: &RunConfig,
        outer: &OuterEndpoint,
    ) -> Result<Self, p4_protocol::ProtocolError> {
        let endpoints = config
            .nodes
            .iter()
            .map(node_endpoint)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            outer: Endpoint::Outer(outer.clone()),
            outer_route: outer.clone(),
            first: endpoints.first().expect("validated run has nodes").clone(),
            nodes: endpoints,
            load_generation: config.load_generation,
            session_id: config.session_id.clone(),
            first_output_position: config.acceptance.expected_prefill_rows,
        })
    }

    pub(super) fn output(
        &self,
        event: &Event,
        outcome: &OutcomePayload,
        previous: Option<&OutcomePayload>,
    ) -> Result<(), String> {
        // Native sampling happens at the tail, but only the head commits the
        // issued membership and publishes approved output. A tail-origin event
        // would bypass that settlement authority, even on a configured route.
        self.route(event, &self.first, EventClass::Output, &outcome.request_id)?;
        if outcome.load_generation != self.load_generation || outcome.session_id != self.session_id
        {
            return Err("output load or session identity is stale".into());
        }
        if previous.is_none()
            && self
                .first_output_position
                .is_some_and(|position| outcome.position as usize != position)
        {
            return Err("first output position does not follow the exact prefill boundary".into());
        }
        if let Some(prior) = previous {
            if outcome.sequence_id != prior.sequence_id {
                return Err("output changed sequence identity within one request".into());
            }
            if prior.position.checked_add(1) != Some(outcome.position) {
                // Named, because the interesting part of a gap is which
                // request and which two positions: a repeat, a skip and a
                // rewind are three different defects behind one sentence.
                return Err(format!(
                    "output token positions are not contiguous: request {} sequence {} went {} -> {}",
                    outcome.request_id, outcome.sequence_id, prior.position, outcome.position
                ));
            }
        }
        Ok(())
    }

    pub(super) fn released(
        &self,
        event: &Event,
        payload: &ReleaseReceipt,
        known_requests: &BTreeSet<String>,
    ) -> Result<(), String> {
        self.route_known_request(event, &self.first, EventClass::Telemetry, known_requests)?;
        payload.validate().map_err(str::to_owned)?;
        // This producer registers correlation=request_id before send. A known
        // but unrelated request cannot lend its envelope to another receipt.
        if !payload
            .members
            .iter()
            .any(|member| member.request_id == event.envelope.correlation_id)
        {
            return Err("release receipt correlation is not one of its members".into());
        }
        if payload.load_generation != self.load_generation || payload.session_id != self.session_id
        {
            return Err("release completion load or session identity is stale".into());
        }
        Ok(())
    }

    pub(super) fn observation(
        &self,
        event: &Event,
        observation: &BatchObservation,
        known_requests: &BTreeSet<String>,
    ) -> Result<(), String> {
        self.route_known_request(event, &self.first, EventClass::Telemetry, known_requests)?;
        if observation.load_generation != self.load_generation
            || observation.session_id != self.session_id
            || observation.observation_id.is_empty()
            || observation.logical_ordinal == 0
            || observation.logical_rows == 0
            || observation.physical_batches.is_empty()
        {
            return Err("batch observation identity or dimensions are invalid".into());
        }
        let mut mixed = 0usize;
        let mut observed_rows = 0usize;
        let execution_ids = observation
            .physical_batches
            .iter()
            .map(|batch| batch.execution_id)
            .collect::<BTreeSet<_>>();
        if execution_ids.len() != observation.physical_batches.len() {
            return Err("batch observation repeats a physical execution".into());
        }
        for batch in &observation.physical_batches {
            let request_ids = batch
                .owned_requests
                .iter()
                .map(|request| request.request_id.as_str())
                .collect::<BTreeSet<_>>();
            let slots = batch
                .owned_requests
                .iter()
                .map(|request| request.sequence_id)
                .collect::<BTreeSet<_>>();
            let measured =
                batch
                    .owned_requests
                    .iter()
                    .try_fold([0usize; 5], |mut totals, request| {
                        if !known_requests.contains(&request.request_id) {
                            return Err(
                                "batch observation references an unknown request".to_owned()
                            );
                        }
                        if request.submission_event_id.is_empty()
                            || request.incarnation == 0
                            || request.request_issue_index == 0
                            || request.rows.is_empty()
                        {
                            return Err(
                                "batch observation contains invalid owned issue identity".into()
                            );
                        }
                        let mut actual = [0usize; 4];
                        let mut unique = BTreeSet::new();
                        for row in &request.rows {
                            let phase = match row.phase {
                                p4_llamacpp_staged_adapter::v2::Phase::Prefill => 0,
                                p4_llamacpp_staged_adapter::v2::Phase::Decode => 1,
                                p4_llamacpp_staged_adapter::v2::Phase::Verify => 2,
                                p4_llamacpp_staged_adapter::v2::Phase::Replay => 3,
                            };
                            if !unique.insert((phase, row.position)) {
                                return Err("batch observation repeats an owned row".into());
                            }
                            actual[phase] += 1;
                        }
                        let values = [
                            request.prefill_rows,
                            request.decode_rows,
                            request.verify_rows,
                            request.replay_rows,
                        ];
                        if values != actual {
                            return Err(
                                "batch observation owned counters differ from issued rows".into()
                            );
                        }
                        if values.iter().all(|value| *value == 0) {
                            return Err("batch observation contains an empty request".to_owned());
                        }
                        for (index, value) in values.into_iter().enumerate() {
                            totals[index + 1] = totals[index + 1]
                                .checked_add(value)
                                .ok_or_else(|| "batch observation row count overflow".to_owned())?;
                            totals[0] = totals[0]
                                .checked_add(value)
                                .ok_or_else(|| "batch observation row count overflow".to_owned())?;
                        }
                        Ok(totals)
                    })?;
            observed_rows = observed_rows
                .checked_add(batch.rows)
                .ok_or_else(|| "batch observation row count overflow".to_owned())?;
            if batch.execution_id == 0
                || batch.rows == 0
                || batch.rows
                    != batch
                        .prefill_rows
                        .checked_add(batch.decode_rows)
                        .and_then(|v| v.checked_add(batch.verify_rows))
                        .and_then(|v| v.checked_add(batch.replay_rows))
                        .ok_or("physical batch row count overflow")?
                || batch.rows < measured[0]
                || batch.prefill_rows < measured[1]
                || batch.decode_rows < measured[2]
                || batch.verify_rows < measured[3]
                || batch.replay_rows < measured[4]
                || batch.request_count < request_ids.len()
                || batch.request_count == 0
                || batch.request_count > batch.rows
                || batch.owned_requests.len() != request_ids.len()
                || batch.owned_requests.len() != slots.len()
                || batch.sequence_count == 0
                || batch.sequence_count < slots.len()
                || batch.sequence_count > batch.rows
            {
                return Err("physical batch observation is internally inconsistent".into());
            }
            if batch.prefill_rows > 0
                && batch.decode_rows + batch.verify_rows + batch.replay_rows > 0
            {
                mixed += 1;
            }
        }
        if mixed != observation.mixed_physical_batches {
            return Err("mixed physical batch count is inconsistent".into());
        }
        if observed_rows != observation.logical_rows {
            return Err("logical and physical batch row counts differ".into());
        }
        if observation
            .physical_batches
            .iter()
            .all(|batch| batch.owned_requests.is_empty())
        {
            return Err("batch observation has no recipient-owned request".into());
        }
        if !observation
            .physical_batches
            .iter()
            .flat_map(|batch| &batch.owned_requests)
            .any(|request| request.request_id == event.envelope.correlation_id)
        {
            return Err("batch observation carrier is not a recipient-owned member".into());
        }
        Ok(())
    }

    pub(super) fn error(
        &self,
        event: &Event,
        known_requests: &BTreeSet<String>,
    ) -> Result<(), String> {
        if !self.nodes.contains(&event.envelope.source) {
            return Err("inference error source is not a configured node".into());
        }
        self.route_known_request_source_agnostic(event, EventClass::Output, known_requests)
    }

    /// A stage span may come from any node of the pipeline; the source names
    /// which, and the answer is its index. The four timestamps must be in
    /// order, or the span is not a span.
    pub(super) fn span(
        &self,
        event: &Event,
        span: &StageSpan,
        known_requests: &BTreeSet<String>,
    ) -> Result<usize, String> {
        self.route_known_request_source_agnostic(event, EventClass::Telemetry, known_requests)?;
        let node = self
            .nodes
            .iter()
            .position(|node| *node == event.envelope.source)
            .ok_or_else(|| "stage span source is not a pipeline node".to_owned())?;
        if span.load_generation != self.load_generation
            || span.session_id != self.session_id
            || span.execution_ids.is_empty()
            || span.rows == 0
            || span.ingress_unix_ms > span.start_unix_ms
            || span.start_unix_ms > span.end_unix_ms
            || span.end_unix_ms > span.forward_unix_ms
        {
            return Err("stage span identity or timestamps are invalid".into());
        }
        let ids = span.execution_ids.iter().copied().collect::<BTreeSet<_>>();
        let detailed = span
            .executions
            .iter()
            .map(|execution| execution.execution_id)
            .collect::<BTreeSet<_>>();
        if ids.len() != span.execution_ids.len()
            || ids.contains(&0)
            || ids != detailed
            || detailed.len() != span.executions.len()
        {
            return Err("stage span execution membership is invalid".into());
        }
        let mut any_owned = false;
        for execution in &span.executions {
            let mut requests = BTreeSet::new();
            let mut slots = BTreeSet::new();
            for owner in &execution.owned_requests {
                any_owned = true;
                if !known_requests.contains(&owner.request_id)
                    || owner.incarnation == 0
                    || !requests.insert(&owner.request_id)
                    || !slots.insert(owner.sequence_id)
                {
                    return Err("stage span owned request membership is invalid".into());
                }
            }
        }
        if !any_owned {
            return Err("stage span has no recipient-owned request".into());
        }
        if !span
            .executions
            .iter()
            .flat_map(|execution| &execution.owned_requests)
            .any(|owner| owner.request_id == event.envelope.correlation_id)
        {
            return Err("stage span carrier is not a recipient-owned member".into());
        }
        Ok(node)
    }

    fn route_known_request(
        &self,
        event: &Event,
        source: &Endpoint,
        class: EventClass,
        known_requests: &BTreeSet<String>,
    ) -> Result<(), String> {
        if !known_requests.contains(&event.envelope.correlation_id) {
            return Err("inference event correlation is not a submitted request".into());
        }
        self.route(event, source, class, &event.envelope.correlation_id)
    }

    fn route_known_request_source_agnostic(
        &self,
        event: &Event,
        class: EventClass,
        known_requests: &BTreeSet<String>,
    ) -> Result<(), String> {
        if !known_requests.contains(&event.envelope.correlation_id) {
            return Err("inference event correlation is not a submitted request".into());
        }
        if event.envelope.target != self.outer
            || event.envelope.class != class
            || event.envelope.causation_id.is_none()
            || event.envelope.adapter_kind.as_deref() != Some("llamacpp")
            || event.envelope.return_route.as_ref() != Some(&self.outer_route)
        {
            return Err("inference event route is not self-consistent".into());
        }
        Ok(())
    }

    fn route(
        &self,
        event: &Event,
        source: &Endpoint,
        class: EventClass,
        correlation: &str,
    ) -> Result<(), String> {
        self.route_known_request_source_agnostic(
            event,
            class,
            &BTreeSet::from([correlation.to_owned()]),
        )?;
        if event.envelope.source != *source || event.envelope.correlation_id != correlation {
            return Err("inference event source or correlation is incorrect".into());
        }
        Ok(())
    }
}
