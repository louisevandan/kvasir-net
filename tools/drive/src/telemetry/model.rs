use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum RuntimePhase {
    Prefill,
    Generation,
}

impl RuntimePhase {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "prefill" => Some(Self::Prefill),
            "generation" => Some(Self::Generation),
            _ => None,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Prefill => "prefill",
            Self::Generation => "generation",
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct SampleKey {
    pub(crate) node: String,
    pub(crate) phase: RuntimePhase,
    pub(crate) sequence: String,
    pub(crate) position: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct RuntimeSample {
    pub(crate) node: String,
    pub(crate) hop_id: u64,
    pub(crate) phase: RuntimePhase,
    pub(crate) sequence: String,
    pub(crate) position: u32,
    pub(crate) tokens: u64,
    pub(crate) elapsed_us: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct NodeObservation {
    pub(crate) address: String,
    pub(crate) node: String,
    pub(crate) snapshot_seq: u64,
    pub(crate) generated_at_unix_ms: u64,
    pub(crate) depth: usize,
    pub(crate) active_hop_id: Option<u64>,
    pub(crate) active_phase: Option<RuntimePhase>,
    pub(crate) active_sequences: Vec<String>,
}

#[derive(Default)]
pub(crate) struct State {
    pub(crate) samples: Vec<RuntimeSample>,
    pub(crate) sample_keys: HashSet<SampleKey>,
    pub(crate) totals: Vec<RuntimeSample>,
    pub(crate) total_indices: HashMap<SampleKey, usize>,
    pub(crate) nodes: Vec<NodeObservation>,
    pub(crate) status_snapshot_schema_max: u16,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TelemetryEvidence {
    pub(crate) run_elapsed_us: u64,
    pub(crate) status_snapshot_schema_max: u16,
    pub(crate) samples: Vec<SampleEvidence>,
    pub(crate) nodes: Vec<NodeEvidence>,
    pub(crate) sessions: Vec<SessionEvidence>,
    pub(crate) aggregate: AggregateEvidence,
    /// Number of cumulative adapter lines parsed from typed status reports.
    /// This is diagnostic evidence: zero means the run cannot claim that
    /// long-run logical metrics were sourced from cumulative counters.
    pub(crate) observed_total_lines: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SampleEvidence {
    pub(crate) node: String,
    pub(crate) hop_id: u64,
    pub(crate) phase: RuntimePhase,
    pub(crate) sequence: String,
    pub(crate) position: u32,
    pub(crate) tokens: u64,
    pub(crate) elapsed_us: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NodeEvidence {
    pub(crate) address: String,
    pub(crate) node: String,
    pub(crate) snapshot_seq: u64,
    pub(crate) generated_at_unix_ms: u64,
    pub(crate) depth: usize,
    pub(crate) active_hop_id: Option<u64>,
    pub(crate) active_phase: Option<RuntimePhase>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TpsEvidence {
    pub(crate) tokens: u64,
    pub(crate) elapsed_us: u64,
    pub(crate) tps: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SessionEvidence {
    pub(crate) sequence: String,
    pub(crate) prefill: TpsEvidence,
    pub(crate) generation: TpsEvidence,
    /// One logical token stream, deduplicated across stage copies.
    pub(crate) logical_prefill_tokens: u64,
    pub(crate) logical_generation_tokens: u64,
    pub(crate) logical_prefill_elapsed_us: u64,
    pub(crate) logical_generation_elapsed_us: u64,
    /// Logical tokens divided by the slowest stage's elapsed time per hop.
    pub(crate) logical_prefill_compute_tps: Option<f64>,
    pub(crate) logical_generation_compute_tps: Option<f64>,
    pub(crate) active_phases: BTreeSet<RuntimePhase>,
    pub(crate) node_depth_peak: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct AggregateEvidence {
    pub(crate) prefill: TpsEvidence,
    pub(crate) generation: TpsEvidence,
    pub(crate) prefill_tps_over_run: Option<f64>,
    pub(crate) generation_tps_over_run: Option<f64>,
    /// Logical prompt/output tokens, counted once per sequence and hop.
    pub(crate) logical_prefill_tokens: u64,
    pub(crate) logical_generation_tokens: u64,
    /// Logical tokens divided by the complete inference wall time.
    pub(crate) logical_prefill_tps_over_run: Option<f64>,
    pub(crate) logical_generation_tps_over_run: Option<f64>,
    /// Arithmetic mean of per-session logical compute throughput. This is
    /// distinct from aggregate throughput, which benefits from batching.
    pub(crate) average_session_prefill_tps: Option<f64>,
    pub(crate) average_session_generation_tps: Option<f64>,
}

type LogicalHopKey = (String, RuntimePhase, u32);
type LogicalHop = (u64, u64);

impl TelemetryEvidence {
    pub(crate) fn from_state(state: &State, run_elapsed: Duration) -> Self {
        let metric_samples = if state.totals.is_empty() {
            &state.samples
        } else {
            &state.totals
        };
        let mut sessions = BTreeMap::<String, SessionEvidence>::new();
        for sample in metric_samples {
            let session =
                sessions
                    .entry(sample.sequence.clone())
                    .or_insert_with(|| SessionEvidence {
                        sequence: sample.sequence.clone(),
                        ..SessionEvidence::default()
                    });
            let metric = match sample.phase {
                RuntimePhase::Prefill => &mut session.prefill,
                RuntimePhase::Generation => &mut session.generation,
            };
            metric.tokens += sample.tokens;
            metric.elapsed_us += sample.elapsed_us;
            metric.tps = tps(metric.tokens, metric.elapsed_us);
        }
        for ((sequence, phase, _), (tokens, elapsed_us)) in logical_hops(metric_samples) {
            let session = sessions
                .entry(sequence.clone())
                .or_insert_with(|| SessionEvidence {
                    sequence,
                    ..SessionEvidence::default()
                });
            match phase {
                RuntimePhase::Prefill => {
                    session.logical_prefill_tokens += tokens;
                    session.logical_prefill_elapsed_us += elapsed_us;
                    session.logical_prefill_compute_tps = tps(
                        session.logical_prefill_tokens,
                        session.logical_prefill_elapsed_us,
                    );
                }
                RuntimePhase::Generation => {
                    session.logical_generation_tokens += tokens;
                    session.logical_generation_elapsed_us += elapsed_us;
                    session.logical_generation_compute_tps = tps(
                        session.logical_generation_tokens,
                        session.logical_generation_elapsed_us,
                    );
                }
            }
        }
        for node in &state.nodes {
            let Some(phase) = node.active_phase else {
                continue;
            };
            for sequence in &node.active_sequences {
                if let Some(session) = sessions.get_mut(sequence) {
                    session.active_phases.insert(phase);
                    session.node_depth_peak = session.node_depth_peak.max(node.depth);
                }
            }
        }
        let mut aggregate = aggregate(metric_samples, run_elapsed);
        aggregate.average_session_prefill_tps = mean(
            sessions
                .values()
                .filter_map(|session| session.logical_prefill_compute_tps),
        );
        aggregate.average_session_generation_tps = mean(
            sessions
                .values()
                .filter_map(|session| session.logical_generation_compute_tps),
        );
        Self {
            run_elapsed_us: duration_us(run_elapsed),
            status_snapshot_schema_max: state.status_snapshot_schema_max,
            samples: state.samples.iter().map(SampleEvidence::from).collect(),
            nodes: state.nodes.iter().map(NodeEvidence::from).collect(),
            sessions: sessions.into_values().collect(),
            aggregate,
            observed_total_lines: state.totals.len(),
        }
    }
}

impl From<&RuntimeSample> for SampleEvidence {
    fn from(sample: &RuntimeSample) -> Self {
        Self {
            node: sample.node.clone(),
            hop_id: sample.hop_id,
            phase: sample.phase,
            sequence: sample.sequence.clone(),
            position: sample.position,
            tokens: sample.tokens,
            elapsed_us: sample.elapsed_us,
        }
    }
}

impl From<&NodeObservation> for NodeEvidence {
    fn from(node: &NodeObservation) -> Self {
        Self {
            address: node.address.clone(),
            node: node.node.clone(),
            snapshot_seq: node.snapshot_seq,
            generated_at_unix_ms: node.generated_at_unix_ms,
            depth: node.depth,
            active_hop_id: node.active_hop_id,
            active_phase: node.active_phase,
        }
    }
}

fn logical_hops(samples: &[RuntimeSample]) -> BTreeMap<LogicalHopKey, LogicalHop> {
    let mut hops = BTreeMap::new();
    for sample in samples {
        // The first decode hop at position zero is the stage-0 KV priming
        // hop. It has a runtime cost, but it is not a returned output token;
        // the tail emits that token at position one. Counting it here would
        // overstate logical generation throughput by one per sequence.
        if sample.phase == RuntimePhase::Generation && sample.position == 0 {
            continue;
        }
        let key = (sample.sequence.clone(), sample.phase, sample.position);
        let entry = hops.entry(key).or_insert((0, 0));
        entry.0 = entry.0.max(sample.tokens);
        entry.1 = entry.1.max(sample.elapsed_us);
    }
    hops
}

fn aggregate(samples: &[RuntimeSample], run_elapsed: Duration) -> AggregateEvidence {
    let mut result = AggregateEvidence::default();
    for sample in samples {
        let metric = match sample.phase {
            RuntimePhase::Prefill => &mut result.prefill,
            RuntimePhase::Generation => &mut result.generation,
        };
        metric.tokens += sample.tokens;
        metric.elapsed_us += sample.elapsed_us;
        metric.tps = tps(metric.tokens, metric.elapsed_us);
    }
    let logical = logical_hops(samples);
    for ((_, phase, _), (tokens, _)) in logical {
        match phase {
            RuntimePhase::Prefill => result.logical_prefill_tokens += tokens,
            RuntimePhase::Generation => result.logical_generation_tokens += tokens,
        }
    }
    let span_us = duration_us(run_elapsed);
    result.prefill_tps_over_run = tps(result.prefill.tokens, span_us);
    result.generation_tps_over_run = tps(result.generation.tokens, span_us);
    result.logical_prefill_tps_over_run = tps(result.logical_prefill_tokens, span_us);
    result.logical_generation_tps_over_run = tps(result.logical_generation_tokens, span_us);
    result
}

pub(crate) fn tps(tokens: u64, elapsed_us: u64) -> Option<f64> {
    (elapsed_us > 0).then(|| tokens as f64 * 1_000_000.0 / elapsed_us as f64)
}

fn mean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let values: Vec<_> = values.collect();
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

fn duration_us(duration: Duration) -> u64 {
    duration.as_micros().min(u128::from(u64::MAX)) as u64
}
