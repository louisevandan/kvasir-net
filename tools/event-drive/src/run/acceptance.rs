use super::{AcceptanceConfig, RequestArtifact, ResponseExpectation, RunConfig};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct AcceptanceSummary {
    pub passed: bool,
    pub requests: Vec<RequestAcceptance>,
}

#[derive(Debug, Serialize)]
pub struct RequestAcceptance {
    pub request_id: String,
    pub passed: bool,
    pub sampled_tokens: usize,
    pub generated_tokens: usize,
    pub terminal_stop: Option<String>,
    pub failures: Vec<String>,
}

pub fn evaluate(config: &RunConfig, requests: &[RequestArtifact]) -> AcceptanceSummary {
    let evaluations = requests
        .iter()
        .enumerate()
        .map(|(index, request)| {
            let expectation = config.acceptance.responses.get(index);
            evaluate_request(&config.acceptance, expectation, request, config.max_tokens)
        })
        .collect::<Vec<_>>();
    AcceptanceSummary {
        passed: evaluations.iter().all(|evaluation| evaluation.passed),
        requests: evaluations,
    }
}

fn evaluate_request(
    acceptance: &AcceptanceConfig,
    expectation: Option<&ResponseExpectation>,
    request: &RequestArtifact,
    max_tokens: u32,
) -> RequestAcceptance {
    let mut failures = Vec::new();
    let sampled_tokens = request.outcomes.len();
    let terminal = request
        .outcomes
        .last()
        .and_then(|outcome| outcome.stop.clone());
    let terminal_is_empty_eos = request
        .outcomes
        .last()
        .is_some_and(|outcome| outcome.stop.as_deref() == Some("eos") && outcome.text.is_empty());
    let generated_tokens = sampled_tokens.saturating_sub(usize::from(terminal_is_empty_eos));
    let minimum_generated_tokens = expectation
        .and_then(|value| value.minimum_generated_tokens)
        .unwrap_or(acceptance.minimum_generated_tokens);

    // Recheck the complete preserved artifact, not only the last output or a
    // claimed completed timestamp. Online acceptance calls this same guard.
    // Nothing is stripped or truncated when an over-budget token is found.
    let mut terminal_seen = false;
    for (emitted, outcome) in request.outcomes.iter().enumerate() {
        if terminal_seen {
            failures.push("output arrived after a terminal outcome".into());
        }
        if let Err(error) = super::output_budget::validate_output(max_tokens, emitted, outcome) {
            failures.push(error);
        }
        terminal_seen |= outcome.stop.is_some();
    }
    if terminal.is_none() {
        failures.push("output stream ended without a terminal outcome".into());
    }
    if request.prefill_rows == 0 {
        failures.push("request has no measured prefill boundary".into());
    }
    match request.outcomes.first() {
        None => failures.push("request did not emit any output".into()),
        Some(first) if first.position as usize != request.prefill_rows => failures.push(format!(
            "first output position {} does not equal measured prefill rows {}",
            first.position, request.prefill_rows
        )),
        Some(_) => {}
    }

    if request.completed_ms.is_none() {
        failures.push("request did not emit a terminal outcome".into());
    }
    if request.submission_event_id.is_empty() || !request.released {
        failures.push("request has no acknowledged submission release".into());
    }
    if request.release_member.as_ref().is_none_or(|member| {
        member.request_id != request.request_id
            || member.submission_event_id != request.submission_event_id
            || member.incarnation == 0
            || member.operation_id == 0
            || request
                .outcomes
                .last()
                .is_none_or(|last| member.sequence_id != last.sequence_id)
    }) {
        failures.push("request has no matching terminal release member".into());
    }
    if request.response.is_empty() {
        failures.push("response is empty".into());
    }
    if let Some(error) = &request.service_error {
        failures.push(format!("OUTER response processing failed: {error}"));
    }
    if generated_tokens < minimum_generated_tokens {
        failures.push(format!(
            "generated token count {generated_tokens} is below {minimum_generated_tokens}"
        ));
    }
    if let Some(expected) = acceptance.expected_prefill_rows {
        if request.prefill_rows != expected {
            failures.push(format!(
                "prefill row count {} does not equal {expected}",
                request.prefill_rows
            ));
        }
    }
    if !acceptance.allowed_stop_reasons.is_empty()
        && terminal.as_ref().is_none_or(|stop| {
            !acceptance
                .allowed_stop_reasons
                .iter()
                .any(|allowed| allowed == stop)
        })
    {
        failures.push(format!(
            "terminal stop {:?} is not allowed",
            terminal.as_deref()
        ));
    }
    if let Some(expectation) = expectation {
        evaluate_response(expectation, &request.response, &mut failures);
    }

    RequestAcceptance {
        request_id: request.request_id.clone(),
        passed: failures.is_empty(),
        sampled_tokens,
        generated_tokens,
        terminal_stop: terminal,
        failures,
    }
}

fn evaluate_response(
    expectation: &ResponseExpectation,
    response: &str,
    failures: &mut Vec<String>,
) {
    if let Some(minimum) = expectation.minimum_response_chars {
        let actual = response.chars().count();
        if actual < minimum {
            failures.push(format!(
                "response character count {actual} is below {minimum}"
            ));
        }
    }
    if let Some(expected) = &expectation.exact_response {
        if response != expected {
            failures.push("response does not exactly match the expectation".into());
        }
    }
    if let Some(expected) = &expectation.expected_json {
        match serde_json::from_str::<serde_json::Value>(response) {
            Ok(actual) if actual == *expected => {}
            Ok(_) => failures.push("response JSON does not match the expected value".into()),
            Err(_) => failures.push("response is not valid JSON".into()),
        }
    }
    for required in &expectation.required_substrings {
        if !response.contains(required) {
            failures.push(format!(
                "response is missing required substring: {required}"
            ));
        }
    }
    for forbidden in &expectation.forbidden_substrings {
        if response.contains(forbidden) {
            failures.push(format!(
                "response contains forbidden substring: {forbidden}"
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use p4_llamacpp_staged_adapter::v2::OutcomePayload;

    fn config(max_tokens: u32) -> RunConfig {
        // Acceptance does not open a connection or create a deployment. Keep
        // its input shape real without fabricating an unrelated node fixture.
        serde_json::from_value(serde_json::json!({
            "ingress_agent": "tcp://127.0.0.1:52000",
            "channel": "outer", "connection_generation": 1,
            "load_generation": 1, "session_id": "session", "request_id": "request",
            "nodes": [], "prompt": "prompt", "max_tokens": max_tokens,
            "acceptance": {"minimum_generated_tokens": 1}
        }))
        .unwrap()
    }

    fn request(response: &str, token_count: usize, stop: &str) -> RequestArtifact {
        let mut outcomes = (0..token_count)
            .map(|position| OutcomePayload {
                load_generation: 1,
                session_id: "session".into(),
                request_id: "request".into(),
                sequence_id: 0,
                token: position as i32,
                text: "x".into(),
                position: 500 + position as u32,
                stop: None,
            })
            .collect::<Vec<_>>();
        outcomes.last_mut().unwrap().stop = Some(stop.into());
        RequestArtifact {
            output_received_ms: Vec::new(),
            request_id: "request".into(),
            submission_event_id: "sent-request".into(),
            submission_authority: None,
            submission: crate::run::SubmissionState::Delivered,
            issued_work: None,
            release_member: Some(p4_llamacpp_staged_adapter::v2::ReleaseMember {
                request_id: "request".into(),
                submission_event_id: "sent-request".into(),
                sequence_id: 0,
                incarnation: 1,
                operation_id: 1,
            }),
            released: true,
            prompt: "prompt".into(),
            eligible_ms: 0,
            send_started_ms: 0,
            send_completed_ms: Some(0),
            arrival_ms: 0,
            first_output_ms: Some(1),
            completed_ms: Some(1),
            release_ms: Some(2),
            prefill_rows: 500,
            decode_rows: token_count,
            verify_rows: 0,
            replay_rows: 0,
            prefill_elapsed_ms: None,
            generation_elapsed_ms: None,
            logical_prefill_tps: None,
            logical_generation_tps: None,
            response: response.into(),
            model_response: None,
            response_processor: None,
            service_error: None,
            service_completed_ms: None,
            outcomes,
        }
    }

    #[test]
    fn rejects_structurally_complete_but_too_short_output() {
        let acceptance = AcceptanceConfig {
            minimum_generated_tokens: 180,
            expected_prefill_rows: Some(500),
            allowed_stop_reasons: vec!["eos".into()],
            responses: Vec::new(),
        };
        let result = evaluate_request(&acceptance, None, &request("b", 1, "eos"), 200);
        assert!(!result.passed);
        assert!(result.failures[0].contains("below 180"));
    }

    #[test]
    fn accepts_content_and_per_request_oracles() {
        let acceptance = AcceptanceConfig {
            minimum_generated_tokens: 3,
            expected_prefill_rows: Some(500),
            allowed_stop_reasons: vec!["eos".into()],
            responses: Vec::new(),
        };
        let expectation = ResponseExpectation {
            minimum_response_chars: Some(10),
            required_substrings: vec!["ownership".into(), "borrow".into()],
            forbidden_substrings: vec!["request-other".into()],
            ..ResponseExpectation::default()
        };
        let result = evaluate_request(
            &acceptance,
            Some(&expectation),
            &request("ownership and borrow", 3, "eos"),
            200,
        );
        assert!(result.passed, "{:?}", result.failures);
    }

    #[test]
    fn semantic_json_oracle_rejects_wrong_arithmetic_and_fenced_output() {
        let acceptance = AcceptanceConfig {
            minimum_generated_tokens: 1,
            expected_prefill_rows: Some(500),
            allowed_stop_reasons: vec!["eos".into()],
            responses: Vec::new(),
        };
        let expectation = ResponseExpectation {
            expected_json: Some(serde_json::json!({
                "rows": [{"id": "R00196", "power_mW": 174928}],
                "temperature_measured": false,
            })),
            ..Default::default()
        };
        let correct = request(
            "{\"temperature_measured\":false,\"rows\":[{\"power_mW\":174928,\"id\":\"R00196\"}]}",
            1,
            "eos",
        );
        assert!(evaluate_request(&acceptance, Some(&expectation), &correct, 10).passed);

        let wrong = request(
            "{\"rows\":[{\"id\":\"R00196\",\"power_mW\":174832}],\"temperature_measured\":false}",
            1,
            "eos",
        );
        let result = evaluate_request(&acceptance, Some(&expectation), &wrong, 10);
        assert_eq!(
            result.failures,
            ["response JSON does not match the expected value"]
        );

        let fenced = request("```json\n{}\n```", 1, "eos");
        let result = evaluate_request(&acceptance, Some(&expectation), &fenced, 10);
        assert_eq!(result.failures, ["response is not valid JSON"]);
    }

    #[test]
    fn acceptance_rejects_two_sampled_outputs_for_a_one_token_request() {
        let request = request("xx", 2, "length");
        let result = evaluate(&config(1), &[request]);
        assert!(
            !result.passed,
            "max_tokens=1 must not approve two sampled outputs"
        );
        assert_eq!(result.requests[0].sampled_tokens, 2);
        assert!(
            result.requests[0]
                .failures
                .iter()
                .any(|failure| failure == "output sampled token count 2 exceeds max_tokens 1")
        );
    }

    #[test]
    fn acceptance_rechecks_early_length_unknown_stop_and_missing_terminal() {
        for (stop, expected) in [
            (
                "length",
                "length stop arrived before max_tokens: sampled 1 of 3",
            ),
            ("timeout", "output stop reason is unknown: timeout"),
        ] {
            let result = evaluate(&config(3), &[request("x", 1, stop)]);
            assert!(!result.passed);
            assert_eq!(result.requests[0].failures, [expected]);
        }
        let mut incomplete = request("x", 1, "eos");
        incomplete.outcomes[0].stop = None;
        // A fabricated completed timestamp cannot substitute for a terminal.
        let result = evaluate(&config(3), &[incomplete]);
        assert!(!result.passed);
        assert_eq!(
            result.requests[0].failures,
            ["output stream ended without a terminal outcome"]
        );
    }

    #[test]
    fn acceptance_preserves_early_stop_exact_length_and_visible_eos_accounting() {
        for (max_tokens, count, stop) in [(3, 1, "stop"), (3, 1, "eos"), (3, 3, "length")] {
            let result = evaluate(&config(max_tokens), &[request("visible", count, stop)]);
            assert!(result.passed, "{:?}", result.requests[0].failures);
            assert_eq!(result.requests[0].sampled_tokens, count);
        }
        let mut eos = request("visible", 2, "eos");
        eos.outcomes.last_mut().unwrap().text.clear();
        let result = evaluate(&config(2), &[eos]);
        assert!(result.passed, "{:?}", result.requests[0].failures);
        assert_eq!(result.requests[0].sampled_tokens, 2);
        assert_eq!(result.requests[0].generated_tokens, 1);
    }

    #[test]
    fn acceptance_rejects_outputs_after_an_earlier_stop_without_discarding_them() {
        let mut after_stop = request("xx", 2, "eos");
        after_stop.outcomes[0].stop = Some("stop".into());
        let result = evaluate(&config(3), &[after_stop]);
        assert!(!result.passed);
        assert_eq!(result.requests[0].sampled_tokens, 2);
        assert_eq!(
            result.requests[0].failures,
            ["output arrived after a terminal outcome"]
        );
    }

    #[test]
    fn acceptance_requires_first_output_at_the_measured_nonzero_prefill_boundary() {
        let mut wrong_position = request("x", 1, "eos");
        wrong_position.outcomes[0].position = 501;
        let result = evaluate(&config(3), &[wrong_position]);
        assert!(!result.passed);
        assert_eq!(
            result.requests[0].failures,
            ["first output position 501 does not equal measured prefill rows 500"]
        );

        let mut missing_boundary = request("x", 1, "eos");
        missing_boundary.prefill_rows = 0;
        missing_boundary.outcomes[0].position = 0;
        let result = evaluate(&config(3), &[missing_boundary]);
        assert!(!result.passed);
        assert_eq!(
            result.requests[0].failures,
            ["request has no measured prefill boundary"]
        );

        let mut missing_output = request("x", 1, "eos");
        missing_output.outcomes.clear();
        let result = evaluate(&config(3), &[missing_output]);
        assert!(!result.passed);
        assert!(
            result.requests[0]
                .failures
                .iter()
                .any(|failure| failure == "request did not emit any output")
        );
    }

    #[test]
    fn acceptance_requires_preserved_submission_and_exact_release_evidence() {
        for mutation in 0..7 {
            let mut value = request("normal", 1, "eos");
            match mutation {
                0 => value.submission_event_id.clear(),
                1 => value.released = false,
                2 => value.release_member = None,
                3 => value.release_member.as_mut().unwrap().request_id = "other".into(),
                4 => value.release_member.as_mut().unwrap().submission_event_id = "old".into(),
                5 => value.release_member.as_mut().unwrap().sequence_id += 1,
                _ => value.release_member.as_mut().unwrap().operation_id = 0,
            }
            let result = evaluate(&config(1), &[value]);
            assert!(!result.passed, "release evidence mutation {mutation}");
            assert!(
                result.requests[0]
                    .failures
                    .iter()
                    .any(|value| value.contains("release"))
            );
        }
    }
}
