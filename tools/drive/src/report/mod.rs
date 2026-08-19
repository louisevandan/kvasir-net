//! What a run is allowed to claim.
//!
//! Four assertions, printed as pass or fail rather than as prose, because the
//! point of a fleet run is a verdict. Throughput is reported but is not one of
//! them: this layer does not own throughput, and a number from a simulated
//! backend would say nothing about a real one.

use crate::session::Outcome;
use crate::telemetry::model::TelemetryEvidence;
use std::time::Duration;

pub fn write_evidence(
    path: &str,
    prompt: &str,
    outcome: &Outcome,
    telemetry: &TelemetryEvidence,
    elapsed: Duration,
) -> Result<(), String> {
    let mut document = String::new();
    document.push_str("# P4 distributed inference evidence\n\n");
    document.push_str("This file contains the exact prompt and complete response text retained by the drive for every request.\n\n");
    document.push_str(&format!("- elapsed_ms: {}\n", elapsed.as_millis()));
    document.push_str(&format!("- completed: {}\n- failed: {}\n- unanswered: {}\n- total_tokens: {}\n\n", outcome.completed, outcome.failed, outcome.unanswered, outcome.tokens));
    document.push_str("## Aggregate telemetry\n\n```json\n");
    document.push_str(&telemetry.to_json());
    document.push_str("\n```\n\n");
    for (index, stream) in outcome.streams.iter().enumerate() {
        document.push_str(&format!("## Session {}\n\n", index + 1));
        document.push_str(&format!("- request_id: `{}`\n- stream_id: `{}`\n- tokens: {}\n- completed: {}\n- failed: {}\n\n", stream.request_id, stream.stream_id, stream.tokens.len(), stream.done.is_some(), stream.failed.as_deref().unwrap_or("")));
        document.push_str("### Prompt\n\n");
        document.push_str(&prompt_for_session(prompt, index));
        document.push_str("\n\n### Complete response\n\n");
        document.push_str(&stream.text);
        document.push_str("\n\n---\n\n");
    }
    std::fs::write(path, document).map_err(|error| format!("cannot write evidence {path}: {error}"))
}

fn prompt_for_session(prompt: &str, index: usize) -> String {
    if std::env::var("P4_DRIVE_VARY").is_ok_and(|value| value != "0") {
        format!("Request {}.\n\n{}", index, prompt)
    } else {
        prompt.to_owned()
    }
}

pub fn print(
    outcome: &Outcome,
    telemetry: &TelemetryEvidence,
    elapsed: Duration,
    requests: usize,
    tokens: u32,
) {
    let seconds = elapsed.as_secs_f64().max(f64::MIN_POSITIVE);
    println!("P4_DRIVE_RESULT requests={requests} tokens_each={tokens}");
    println!(
        "  completed={} failed={} unanswered={} routes={}",
        outcome.completed, outcome.failed, outcome.unanswered, outcome.routes
    );
    println!(
        "  tokens={} elapsed_ms={:.0} frames_per_second={:.0}",
        outcome.tokens,
        elapsed.as_secs_f64() * 1000.0,
        (outcome.tokens + outcome.completed) as f64 / seconds
    );
    let latency = percentiles(&outcome.latency_us);
    println!(
        "P4_DRIVE_LATENCY completed={} p50_ms={} p95_ms={} p99_ms={} max_ms={}",
        outcome.latency_us.len(),
        optional_ms(latency[0]),
        optional_ms(latency[1]),
        optional_ms(latency[2]),
        optional_ms(latency[3]),
    );
    println!(
        "P4_DRIVE_LOGICAL_METRICS observed_total_lines={} prefill_tokens={} generation_tokens={} prefill_tps_over_run={} generation_tps_over_run={} average_session_prefill_tps={} average_session_generation_tps={}",
        telemetry.observed_total_lines,
        telemetry.aggregate.logical_prefill_tokens,
        telemetry.aggregate.logical_generation_tokens,
        optional_tps(telemetry.aggregate.logical_prefill_tps_over_run),
        optional_tps(telemetry.aggregate.logical_generation_tps_over_run),
        optional_tps(telemetry.aggregate.average_session_prefill_tps),
        optional_tps(telemetry.aggregate.average_session_generation_tps),
    );

    if !outcome.stalled.is_empty() {
        println!("  stalled_at_tokens={:?}", outcome.stalled);
    }
    // Said before the verdicts, because it changes what they mean. A run the
    // driver walked away from has not shown the deployment failing; it has
    // shown the driver stopping, and the two must never read the same.
    // Where the backlog lived, asked for over the socket while the run was in
    // flight. Both halves are needed: a ceiling held says nothing unless a
    // backlog existed to hold back, and a shallow main queue is also what an
    // idle agent looks like.
    if outcome.samples > 0 {
        println!(
            "  peak_node_queue={} peak_in_adapter={} peak_main_lane={} samples={}",
            outcome.node_depth, outcome.running, outcome.lane, outcome.samples
        );
    }
    if let Some(why) = &outcome.why {
        println!("  first_failure: {why}");
    }
    if outcome.quiet {
        println!("  NOTE the driver stopped waiting; the verdicts below are incomplete");
    }
    // Not a verdict. A run against a real backend has passed all four of these
    // while generating nothing at all — once because a 503 read as a stream
    // that ended, once because the model streamed its tokens under a key the
    // adapter did not read. The words are what tell those apart.
    if !outcome.sample.is_empty() {
        println!("  answer_chars={} first:", outcome.sample.chars().count());
        println!("    {}", head(&outcome.sample, 240));
    }
    verdict("every request answered", outcome.unanswered == 0);
    verdict("no request failed", outcome.failed == 0);
    verdict("every stream in order", outcome.out_of_order == 0);
    verdict(
        "one terminal per route",
        outcome.completed + outcome.failed == requests,
    );
    println!("P4_DRIVE_TELEMETRY_JSON {}", telemetry.to_json());
}

fn verdict(claim: &str, held: bool) {
    println!("  [{}] {claim}", if held { "pass" } else { "FAIL" });
}

fn optional_tps(value: Option<f64>) -> String {
    value
        .map(|number| format!("{number:.6}"))
        .unwrap_or_else(|| "null".into())
}

fn optional_ms(value: Option<f64>) -> String {
    value
        .map(|number| format!("{number:.3}"))
        .unwrap_or_else(|| "null".into())
}

/// Nearest-rank percentiles over completed request latencies. The maximum is
/// reported alongside them so a saturated tail remains visible.
fn percentiles(latencies_us: &[u64]) -> [Option<f64>; 4] {
    if latencies_us.is_empty() {
        return [None, None, None, None];
    }
    let mut values = latencies_us.to_vec();
    values.sort_unstable();
    let rank = |fraction: f64| {
        let index = ((values.len() as f64 * fraction).ceil() as usize)
            .saturating_sub(1)
            .min(values.len() - 1);
        Some(values[index] as f64 / 1_000.0)
    };
    [
        rank(0.50),
        rank(0.95),
        rank(0.99),
        Some(*values.last().unwrap() as f64 / 1_000.0),
    ]
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::percentiles;

    #[test]
    fn latency_percentiles_use_nearest_rank_and_report_max() {
        let values = percentiles(&[1_000, 2_000, 3_000, 4_000]);
        assert_eq!(values[0], Some(2.0));
        assert_eq!(values[1], Some(4.0));
        assert_eq!(values[2], Some(4.0));
        assert_eq!(values[3], Some(4.0));
    }

    #[test]
    fn empty_latency_has_no_percentiles() {
        assert_eq!(percentiles(&[]), [None, None, None, None]);
    }
}

/// The opening of an answer, on one line. A five-thousand-token answer is
/// evidence that tokens were real, not something to read in a terminal.
fn head(text: &str, chars: usize) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let mut kept: String = flat.chars().take(chars).collect();
    if flat.chars().count() > chars {
        kept.push('…');
    }
    kept
}
