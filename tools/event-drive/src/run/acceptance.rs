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
            evaluate_request(&config.acceptance, expectation, request)
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

    if request.completed_ms.is_none() {
        failures.push("request did not emit a terminal outcome".into());
    }
    if request.response.is_empty() {
        failures.push("response is empty".into());
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

    fn request(response: &str, token_count: usize, stop: &str) -> RequestArtifact {
        let mut outcomes = (0..token_count)
            .map(|position| OutcomePayload {
                load_generation: 1,
                session_id: "session".into(),
                request_id: "request".into(),
                sequence_id: 0,
                token: position as i32,
                text: "x".into(),
                position: position as u32,
                stop: None,
            })
            .collect::<Vec<_>>();
        outcomes.last_mut().unwrap().stop = Some(stop.into());
        RequestArtifact {
            request_id: "request".into(),
            prompt: "prompt".into(),
            arrival_ms: 0,
            first_output_ms: Some(1),
            completed_ms: Some(1),
            prefill_rows: 500,
            decode_rows: token_count,
            verify_rows: 0,
            replay_rows: 0,
            prefill_elapsed_ms: None,
            generation_elapsed_ms: None,
            logical_prefill_tps: None,
            logical_generation_tps: None,
            response: response.into(),
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
        let result = evaluate_request(&acceptance, None, &request("b", 1, "eos"));
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
        );
        assert!(result.passed, "{:?}", result.failures);
    }
}
