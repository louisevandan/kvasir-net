//! Disjoint P4-drive runtime evidence collection.
//!
//! The adapter report is opaque to P4 routing, but it is an explicit evidence
//! surface for this tool. This module is the only place that interprets
//! `P4_RUNTIME_SAMPLE_V1`; typed status snapshots are consumed as typed values.

use p4_service::status::{ActiveHopLane, StatusSnapshot};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

mod json;
pub(crate) mod model;

use model::{NodeObservation, RuntimePhase, RuntimeSample, SampleKey, State, TelemetryEvidence};

const SAMPLE_PREFIX: &str = "P4_RUNTIME_SAMPLE_V1 ";
const TOTAL_PREFIX: &str = "P4_RUNTIME_TOTAL_V1 ";

#[derive(Clone, Default)]
pub(crate) struct TelemetryCollector {
    state: Arc<Mutex<State>>,
}

impl TelemetryCollector {
    /// Reads each node's typed backend report. Status polling repeats the
    /// adapter's retained sample window, so samples are deduplicated first.
    pub(crate) fn observe(&self, snapshot: &StatusSnapshot) {
        let mut state = self.state.lock().expect("telemetry lock");
        state.status_snapshot_schema_max = state.status_snapshot_schema_max.max(snapshot.schema);
        for node in &snapshot.nodes {
            let active = node.active_hop.as_ref();
            state.nodes.push(NodeObservation {
                address: snapshot.address.clone(),
                node: node.node.clone(),
                snapshot_seq: snapshot.snapshot_seq,
                generated_at_unix_ms: snapshot.generated_at_unix_ms,
                depth: node.depth,
                active_hop_id: active.map(|hop| hop.id),
                active_phase: active.map(|hop| phase_from_typed(hop.lane)),
                active_sequences: active
                    .map(|hop| {
                        hop.requests
                            .iter()
                            .map(|request| {
                                if request.request_id.is_empty() {
                                    request.route.clone()
                                } else {
                                    request.request_id.clone()
                                }
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            });
            for line in node.backend.lines() {
                if let Some(sample) = parse_sample(&node.node, line) {
                    let key = SampleKey {
                        node: sample.node.clone(),
                        phase: sample.phase,
                        sequence: sample.sequence.clone(),
                        position: sample.position,
                    };
                    if state.sample_keys.insert(key) {
                        state.samples.push(sample);
                    }
                } else if let Some(total) = parse_total(&node.node, line) {
                    let key = SampleKey {
                        node: total.node.clone(),
                        phase: total.phase,
                        sequence: total.sequence.clone(),
                        position: total.position,
                    };
                    // A total is a cumulative counter, not an immutable
                    // retained sample. Replace the previous observation for
                    // this node/phase/sequence key so the final snapshot wins.
                    if let Some(index) = state.total_indices.get(&key).copied() {
                        state.totals[index] = total;
                    } else {
                        let index = state.totals.len();
                        state.totals.push(total);
                        state.total_indices.insert(key, index);
                    }
                }
            }
        }
    }

    pub(crate) fn evidence(&self, run_elapsed: Duration) -> TelemetryEvidence {
        let state = self.state.lock().expect("telemetry lock");
        TelemetryEvidence::from_state(&state, run_elapsed)
    }
}

/// `ActiveHopSnapshot::lane` is the queue lane a hop was composed from
/// (`p4_adapter::Hop` carries no phase of its own — see
/// `docs/adapter-boundary.md`), and in practice a hop's lane is always
/// `Prefill` or `Decode`: control and response traffic never reaches this
/// field. Anything else collapses to `Prefill`, the same default the node
/// itself used when this lane was still folded into the removed `Phase`.
fn phase_from_typed(lane: ActiveHopLane) -> RuntimePhase {
    match lane {
        ActiveHopLane::Decode => RuntimePhase::Generation,
        ActiveHopLane::Prefill => RuntimePhase::Prefill,
    }
}

fn parse_sample(node: &str, line: &str) -> Option<RuntimeSample> {
    let fields: BTreeMap<_, _> = line
        .strip_prefix(SAMPLE_PREFIX)?
        .split_whitespace()
        .filter_map(|field| field.split_once('='))
        .collect();
    let phase = RuntimePhase::parse(fields.get("phase")?)?;
    let sequence = decode_hex(fields.get("sequence_hex")?)?;
    if sequence.is_empty() {
        return None;
    }
    Some(RuntimeSample {
        node: node.to_owned(),
        hop_id: fields.get("hop_id")?.parse().ok()?,
        phase,
        sequence,
        position: fields
            .get("position")
            .and_then(|value| value.parse().ok())
            .unwrap_or_default(),
        tokens: fields.get("tokens")?.parse().ok()?,
        elapsed_us: fields.get("elapsed_us")?.parse().ok()?,
    })
}

fn parse_total(node: &str, line: &str) -> Option<RuntimeSample> {
    let fields: BTreeMap<_, _> = line
        .strip_prefix(TOTAL_PREFIX)?
        .split_whitespace()
        .filter_map(|field| field.split_once('='))
        .collect();
    let phase = RuntimePhase::parse(fields.get("phase")?)?;
    let sequence = decode_hex(fields.get("sequence_hex")?)?;
    if sequence.is_empty() {
        return None;
    }
    Some(RuntimeSample {
        node: node.to_owned(),
        hop_id: 0,
        phase,
        sequence,
        // Keep generation totals away from the stage-zero position-zero
        // priming exclusion used for ordinary hop samples.
        position: if phase == RuntimePhase::Generation {
            1
        } else {
            0
        },
        tokens: fields.get("tokens")?.parse().ok()?,
        elapsed_us: fields.get("elapsed_us")?.parse().ok()?,
    })
}

fn decode_hex(value: &str) -> Option<String> {
    if !value.len().is_multiple_of(2) {
        return None;
    }
    let bytes = (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16))
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests;
