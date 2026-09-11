//! Stage-to-head cost feedback. Semantic authority remains in flight/KV ledgers.
use super::*;
use crate::v2::scheduler::service::{ServiceSample, ServiceShape};

pub(super) fn configured_budget() -> (crate::v2::scheduler::service::ServiceBudget, Option<String>)
{
    use crate::v2::scheduler::service::ServiceBudget;
    match std::env::var("P4_STAGED_PREFILL_SERVICE_MS") {
        Err(std::env::VarError::NotPresent) => (ServiceBudget::default(), None),
        Ok(value) => match value
            .parse::<u64>()
            .ok()
            .and_then(|v| v.checked_mul(1000))
            .filter(|v| *v > 0)
        {
            Some(us) => (ServiceBudget::new(us), None),
            None => (
                ServiceBudget::default(),
                Some(
                    "P4_STAGED_PREFILL_SERVICE_MS must be a positive non-overflowing integer"
                        .into(),
                ),
            ),
        },
        Err(e) => (ServiceBudget::default(), Some(e.to_string())),
    }
}

pub(super) fn physical_shape(set: &CapsuleSet) -> Option<ServiceShape> {
    let rows: Vec<_> = set.0.iter().flat_map(|c| &c.owners).collect();
    if rows.is_empty()
        || rows
            .iter()
            .any(|r| !matches!(r.phase, Phase::Prefill | Phase::Decode))
    {
        return None;
    }
    Some(ServiceShape {
        prefill_rows: rows.iter().filter(|r| r.phase == Phase::Prefill).count(),
        decode_rows: rows.iter().filter(|r| r.phase == Phase::Decode).count(),
        members: rows
            .iter()
            .map(|r| (r.sequence_id, r.incarnation))
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        last_position: rows.iter().map(|r| r.position).max().unwrap(),
    })
}

impl Worker {
    /// Candidate preparation is read-only. Only the plan eventually accepted
    /// by native issue may advance scheduler fairness or request/flight state.
    pub(super) fn prepare_generation_service_plan(
        &self, demands: &[Demand], session: &PipelineSession, issue_cap: usize,
        limits: crate::v2::scheduler::OrdinaryLimits, decoding: bool,
        mut plan: crate::v2::scheduler::PreparedPlan,
    ) -> Result<(crate::v2::scheduler::PreparedPlan, Option<crate::v2::scheduler::service::ServiceDecision>), String> {
        use crate::v2::scheduler::service::ServiceVerdict;
        let prepare = |demands: &[Demand], limits| self.scheduler.prepare_plan_with_limits(
            demands, self.state.batch_capacity.min(issue_cap), self.state.physical_capacity.min(issue_cap),
            self.state.equal_sequence_ubatch, self.state.max_atomic_sequences,
            self.state.atomic_batch_exclusive, limits).map_err(|e| format!("service candidate: {e:?}"));
        let mut examined = Vec::new();
        loop {
            let Some(shape) = self.planned_service_shape(plan.allocations()) else { return Ok((plan, None)); };
            let Some(mut decision) = self.service_budget.decide(self.state.load_generation,
                &session.command.session_id, session.command.stages.len(), &shape, decoding,
                &self.state.open_batches) else { return Ok((plan, None)); };
            examined.push(shape.prefill_rows);
            if matches!(decision.verdict, ServiceVerdict::PurePrefill | ServiceVerdict::DecodeOnly | ServiceVerdict::Admit) {
                decision.examined_prefill_rows = examined;
                decision.selected_prefill_rows = Some(shape.prefill_rows);
                return Ok((plan, Some(decision)));
            }
            if shape.prefill_rows > 1 {
                // Zero means unbounded in OrdinaryLimits. Never express
                // "no prefill" by writing zero into that field.
                let smaller = crate::v2::scheduler::OrdinaryLimits {
                    prefill_rows: shape.prefill_rows / 2, ..limits
                };
                plan = prepare(demands, smaller)?;
                continue;
            }
            if !self.service_budget.has_open_prefill(&self.state.open_batches)
                || (decision.verdict == ServiceVerdict::Cold
                    && !self.service_budget.has_open_calibration_probe(&self.state.open_batches)) {
                // Unknown/infeasible service cannot starve prompts forever.
                // Calibrate the smallest quantum, never the rejected original
                // full chunk. This explicit probe does not promise an SLO.
                // One cold one-row probe may follow an older large prefill:
                // otherwise learning its smaller shape would require first
                // draining the very pipeline this policy should keep fed.
                decision.examined_prefill_rows = examined;
                decision.selected_prefill_rows = Some(shape.prefill_rows);
                return Ok((plan, Some(decision)));
            }
            if decision.verdict == ServiceVerdict::Cold {
                decision.verdict = ServiceVerdict::CalibrationWait;
            }
            let decodes: Vec<_> = demands.iter().filter(|d| d.phase != Phase::Prefill).cloned().collect();
            plan = prepare(&decodes, limits)?;
            decision.examined_prefill_rows = examined;
            decision.selected_prefill_rows = Some(0);
            return Ok((plan, Some(decision)));
        }
    }

    pub(super) fn planned_service_shape(&self, allocations: &[Allocation]) -> Option<ServiceShape> {
        let mut shape = ServiceShape {
            prefill_rows: 0,
            decode_rows: 0,
            members: allocations.len(),
            last_position: 0,
        };
        for allocation in allocations {
            let request = self.state.requests.get(&allocation.request_id)?;
            let first = match allocation.phase {
                Phase::Prefill => {
                    shape.prefill_rows += allocation.rows;
                    u32::try_from(request.prompt_issued).ok()?
                }
                Phase::Decode => {
                    shape.decode_rows += allocation.rows;
                    request.ready.as_ref()?.position
                }
                _ => return None,
            };
            shape.last_position = shape
                .last_position
                .max(first.checked_add(u32::try_from(allocation.rows.checked_sub(1)?).ok()?)?);
        }
        Some(shape)
    }

    pub(super) fn register_service_issue(
        &mut self,
        session: &PipelineSession,
        ordinal: u64,
        physical: &CapsuleSet,
        rpc_us: u64,
    ) {
        if !self.service_budget.enabled() {
            return;
        }
        let Some(shape) = physical_shape(physical) else {
            return;
        };
        let sample = ServiceSample {
            load_generation: self.state.load_generation,
            session_id: session.command.session_id.clone(),
            execution_ids: physical.0.iter().map(|c| c.execution_id).collect(),
            stage_index: 0,
            shape,
            rpc_us,
        };
        self.service_budget.register(
            sample,
            ordinal,
            session.command.stages.len(),
            &self.state.open_batches,
        );
    }

    pub(super) fn prepare_service_feedback(
        &self,
        event: &Event,
        session: &PipelineSession,
        physical: &CapsuleSet,
        rpc_us: u64,
    ) -> Result<Option<effects::CommittedEffect>, String> {
        if !self.service_budget.enabled() || physical.0.len() > 1024 {
            return Ok(None);
        }
        let Some(shape) = physical_shape(physical) else {
            return Ok(None);
        };
        let sample = ServiceSample {
            load_generation: self.state.load_generation,
            session_id: session.command.session_id.clone(),
            execution_ids: physical.0.iter().map(|c| c.execution_id).collect(),
            stage_index: session.command.stage_index,
            shape,
            rpc_us,
        };
        let body = serde_json::to_vec(&sample).map_err(|e| e.to_string())?;
        if body.len() > 65536 {
            return Err("service feedback exceeds 64 KiB".into());
        }
        Ok(Some(effects::CommittedEffect::Forward {
            base: event.envelope.clone(),
            target: session.first.clone(),
            class: EventClass::Telemetry,
            content_type: SERVICE_SAMPLE_CONTENT_TYPE,
            body,
        }))
    }

    pub(super) fn service_sample(&mut self, event: &Event) -> Result<(), String> {
        if event.envelope.target != self.endpoint
            || !matches!(event.envelope.source, Endpoint::Node { .. })
        {
            return Err("service feedback requires a stage-to-head route".into());
        }
        if event.payload.len() > 65536 {
            return Err("service feedback exceeds 64 KiB".into());
        }
        let sample: ServiceSample = serde_json::from_slice(&event.payload)
            .map_err(|e| format!("invalid service feedback: {e}"))?;
        // Late optimization telemetry cannot reopen a completed load or turn
        // successful UNLOAD into an inference failure. It grants no authority.
        if sample.load_generation != 0
            && sample.load_generation <= self.state.last_load_generation
            && (self.state.load_generation == 0
                || sample.load_generation < self.state.load_generation)
        {
            return Ok(());
        }
        if sample.load_generation != self.state.load_generation {
            return Err("service feedback load generation is stale".into());
        }
        let session = self
            .state
            .sessions
            .get(&sample.session_id)
            .ok_or("service feedback session is unknown")?;
        if session.command.role() != NodeRole::First || sample.stage_index == 0 {
            return Err("service feedback must target head from a downstream stage".into());
        }
        let declared = session
            .command
            .stages
            .get(sample.stage_index)
            .ok_or("service feedback stage is out of range")?;
        self.require_stage_source(event, &node_endpoint(declared)?, "service feedback")?;
        if self.service_budget.enabled() {
            self.service_budget.observe(&sample)?;
        }
        Ok(())
    }
}
