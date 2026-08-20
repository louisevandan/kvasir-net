//! Runtime measurements emitted through the existing typed status wire.
//!
//! This is deliberately adapter-local. The P4 agent does not learn how a
//! staged backend counts tokens; it only relays the backend report already
//! carried by `StatusSnapshot`.

use crate::HopPhase;
use p4_adapter::Hop;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::Duration;

const RETAINED_SAMPLES: usize = 512;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeSample {
    pub hop_id: u64,
    pub phase: HopPhase,
    pub sequence: String,
    pub position: u32,
    pub tokens: u32,
    pub elapsed_us: u64,
}

#[derive(Default)]
struct State {
    samples: VecDeque<RuntimeSample>,
    dropped: u64,
    totals: HashMap<(String, String), (u64, u64)>,
}

#[derive(Default)]
pub struct RuntimeEvidence {
    state: Mutex<State>,
}

impl RuntimeEvidence {
    pub fn record(
        &self,
        hop: &Hop,
        phase: HopPhase,
        token_counts: impl IntoIterator<Item = (String, u32)>,
        elapsed: Duration,
    ) {
        let elapsed_us = elapsed.as_micros().min(u128::from(u64::MAX)) as u64;
        let mut state = self.state.lock().expect("staged telemetry lock");
        for (sequence, tokens) in token_counts {
            if tokens == 0 {
                continue;
            }
            let position = hop
                .sequences
                .iter()
                .find(|candidate| candidate.sequence == sequence)
                .map(|_| 0u32)
                .unwrap_or_default();
            let total = state
                .totals
                .entry((phase_name(phase).to_owned(), sequence.clone()))
                .or_insert((0, 0));
            total.0 = total.0.saturating_add(u64::from(tokens));
            total.1 = total.1.saturating_add(elapsed_us);
            if state.samples.len() == RETAINED_SAMPLES {
                state.samples.pop_front();
                state.dropped = state.dropped.saturating_add(1);
            }
            state.samples.push_back(RuntimeSample {
                hop_id: hop.id,
                phase,
                sequence,
                position,
                tokens,
                elapsed_us,
            });
        }
    }

    /// Machine-readable lines inside the opaque adapter report. The caller
    /// gets the exact runtime token count and duration, not a wall-clock
    /// estimate made by the outer benchmark.
    pub fn report(&self) -> String {
        let state = self.state.lock().expect("staged telemetry lock");
        let mut report = format!(
            "P4_RUNTIME_EVIDENCE_V1 retained={} dropped={}\n",
            state.samples.len(),
            state.dropped
        );
        for ((phase, sequence), (tokens, elapsed_us)) in &state.totals {
            report.push_str(&format!(
                "P4_RUNTIME_TOTAL_V1 phase={} sequence_hex={} tokens={} elapsed_us={}\n",
                phase,
                hex(sequence.as_bytes()),
                tokens,
                elapsed_us
            ));
        }
        for sample in &state.samples {
            report.push_str(&format!(
                "P4_RUNTIME_SAMPLE_V1 hop_id={} phase={} sequence_hex={} position={} tokens={} elapsed_us={}\n",
                sample.hop_id,
                phase_name(sample.phase),
                hex(sample.sequence.as_bytes()),
                sample.position,
                sample.tokens,
                sample.elapsed_us
            ));
        }
        report
    }
}

fn phase_name(phase: HopPhase) -> &'static str {
    match phase {
        HopPhase::Prefill => "prefill",
        HopPhase::Decode => "generation",
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{HopPhase, RuntimeEvidence};
    use p4_adapter::{Hop, Sequence};
    use std::time::Duration;

    fn hop() -> Hop {
        Hop {
            id: 7,
            deployment: "deployment".into(),
            sequences: vec![Sequence {
                sequence: "r1-q0".into(),
                state: None,
                prompt: Some("prompt".into()),
                remaining: 2,
                options: "{}".into(),
            }],
        }
    }

    #[test]
    fn report_preserves_runtime_phase_token_count_and_duration() {
        let evidence = RuntimeEvidence::default();
        evidence.record(
            &hop(),
            HopPhase::Prefill,
            [("r1-q0".into(), 13)],
            Duration::from_micros(2_500),
        );
        let report = evidence.report();
        assert!(report.contains("P4_RUNTIME_EVIDENCE_V1 retained=1 dropped=0"));
        assert!(report.contains("phase=prefill"));
        assert!(report.contains("sequence_hex=72312d7130"));
        assert!(report.contains("tokens=13 elapsed_us=2500"));
    }
}
