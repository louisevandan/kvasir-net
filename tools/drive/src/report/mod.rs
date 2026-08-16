//! What a run is allowed to claim.
//!
//! Four assertions, printed as pass or fail rather than as prose, because the
//! point of a fleet run is a verdict. Throughput is reported but is not one of
//! them: this layer does not own throughput, and a number from a simulated
//! backend would say nothing about a real one.

use crate::session::Outcome;
use std::time::Duration;

pub fn print(outcome: &Outcome, elapsed: Duration, requests: usize, tokens: u32) {
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
}

fn verdict(claim: &str, held: bool) {
    println!("  [{}] {claim}", if held { "pass" } else { "FAIL" });
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
