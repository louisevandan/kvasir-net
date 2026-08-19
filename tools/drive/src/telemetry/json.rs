use super::model::{
    AggregateEvidence, NodeEvidence, SampleEvidence, SessionEvidence, TelemetryEvidence,
    TpsEvidence,
};

impl TelemetryEvidence {
    pub(crate) fn to_json(&self) -> String {
        let mut out = format!(
            "{{\"schema\":\"p4-drive-telemetry-v1\",\"mode\":\"non-mtp\",\"sample_format\":\"P4_RUNTIME_SAMPLE_V1\",\"metric_semantics\":{{\"legacy\":\"stage_sample_sum\",\"logical\":\"one_per_sequence_phase_hop\"}},\"status_snapshot_schema_max\":{},\"run_elapsed_us\":{},",
            self.status_snapshot_schema_max, self.run_elapsed_us
        );
        out.push_str(&format!(
            "\"observed_total_lines\":{},\"samples\":[",
            self.observed_total_lines
        ));
        join_json(&mut out, self.samples.iter().map(SampleEvidence::json));
        out.push_str("],\"nodes\":[");
        join_json(&mut out, self.nodes.iter().map(NodeEvidence::json));
        out.push_str("],\"sessions\":[");
        join_json(&mut out, self.sessions.iter().map(SessionEvidence::json));
        out.push_str("],\"aggregate\":");
        out.push_str(&self.aggregate.json());
        out.push('}');
        out
    }
}

fn join_json(out: &mut String, values: impl Iterator<Item = String>) {
    for (index, value) in values.enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&value);
    }
}

fn json_string(value: &str) -> String {
    let mut out = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if character.is_control() => {
                out.push_str(&format!("\\u{:04x}", character as u32))
            }
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

fn optional_number(value: Option<f64>) -> String {
    value
        .map(|number| format!("{number:.6}"))
        .unwrap_or_else(|| "null".into())
}

impl SampleEvidence {
    fn json(&self) -> String {
        format!(
            "{{\"node\":{},\"hop_id\":{},\"phase\":{},\"sequence\":{},\"position\":{},\"tokens\":{},\"elapsed_us\":{}}}",
            json_string(&self.node),
            self.hop_id,
            json_string(self.phase.name()),
            json_string(&self.sequence),
            self.position,
            self.tokens,
            self.elapsed_us
        )
    }
}

impl NodeEvidence {
    fn json(&self) -> String {
        format!(
            "{{\"address\":{},\"node\":{},\"snapshot_seq\":{},\"generated_at_unix_ms\":{},\"depth\":{},\"active_hop_id\":{},\"active_phase\":{}}}",
            json_string(&self.address),
            json_string(&self.node),
            self.snapshot_seq,
            self.generated_at_unix_ms,
            self.depth,
            self.active_hop_id
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".into()),
            self.active_phase
                .map(|value| json_string(value.name()))
                .unwrap_or_else(|| "null".into())
        )
    }
}

impl TpsEvidence {
    fn json(&self) -> String {
        format!(
            "{{\"tokens\":{},\"elapsed_us\":{},\"tps\":{}}}",
            self.tokens,
            self.elapsed_us,
            optional_number(self.tps)
        )
    }
}

impl SessionEvidence {
    fn json(&self) -> String {
        let phases = self
            .active_phases
            .iter()
            .map(|phase| json_string(phase.name()))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"sequence\":{},\"prefill\":{},\"generation\":{},\"logical_prefill_tokens\":{},\"logical_generation_tokens\":{},\"logical_prefill_elapsed_us\":{},\"logical_generation_elapsed_us\":{},\"logical_prefill_compute_tps\":{},\"logical_generation_compute_tps\":{},\"active_phases\":[{}],\"node_depth_peak\":{}}}",
            json_string(&self.sequence),
            self.prefill.json(),
            self.generation.json(),
            self.logical_prefill_tokens,
            self.logical_generation_tokens,
            self.logical_prefill_elapsed_us,
            self.logical_generation_elapsed_us,
            optional_number(self.logical_prefill_compute_tps),
            optional_number(self.logical_generation_compute_tps),
            phases,
            self.node_depth_peak
        )
    }
}

impl AggregateEvidence {
    fn json(&self) -> String {
        format!(
            "{{\"prefill\":{},\"generation\":{},\"prefill_tps_over_run\":{},\"generation_tps_over_run\":{},\"logical_prefill_tokens\":{},\"logical_generation_tokens\":{},\"logical_prefill_tps_over_run\":{},\"logical_generation_tps_over_run\":{},\"average_session_prefill_tps\":{},\"average_session_generation_tps\":{}}}",
            self.prefill.json(),
            self.generation.json(),
            optional_number(self.prefill_tps_over_run),
            optional_number(self.generation_tps_over_run),
            self.logical_prefill_tokens,
            self.logical_generation_tokens,
            optional_number(self.logical_prefill_tps_over_run),
            optional_number(self.logical_generation_tps_over_run),
            optional_number(self.average_session_prefill_tps),
            optional_number(self.average_session_generation_tps)
        )
    }
}
