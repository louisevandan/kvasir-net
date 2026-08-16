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
