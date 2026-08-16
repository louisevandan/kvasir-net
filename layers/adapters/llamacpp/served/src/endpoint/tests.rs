use super::*;

#[test]
fn an_endpoint_is_read_from_the_form_a_plan_carries() {
    let parsed = Endpoint::parse(" 127.0.0.1:8080 ").expect("parsed");
    assert_eq!(parsed.host, "127.0.0.1");
    assert_eq!(parsed.port, 8080);
}

#[test]
fn an_ipv6_literal_keeps_its_own_colons() {
    let parsed = Endpoint::parse("[::1]:8080").expect("parsed");
    assert_eq!(parsed.host, "[::1]");
    assert_eq!(parsed.port, 8080);
}

#[test]
fn something_that_is_not_an_endpoint_is_refused_rather_than_guessed_at() {
    for value in ["", "localhost", "localhost:", ":8080", "localhost:http"] {
        assert!(
            Endpoint::parse(value).is_none(),
            "{value} should be refused"
        );
    }
}

#[test]
fn a_length_framed_body_is_read_whole() {
    let headers = vec!["content-length: 11".to_string()];
    let mut reader = std::io::BufReader::new(&b"hello world"[..]);
    assert_eq!(read_body(&mut reader, &headers).unwrap(), "hello world");
}

/// `llama-server` answers a stream chunked, so this is the framing that
/// carries every token.
#[test]
fn a_chunked_body_is_reassembled() {
    let headers = vec!["transfer-encoding: chunked".to_string()];
    let raw = "5\r\nhello\r\n1\r\n \r\n5\r\nworld\r\n0\r\n\r\n";
    let mut reader = std::io::BufReader::new(raw.as_bytes());
    assert_eq!(read_body(&mut reader, &headers).unwrap(), "hello world");
}

#[test]
fn headers_end_at_the_blank_line_and_are_lowered() {
    let raw = "HTTP/1.1 200 OK\r\nContent-Type: TEXT/EVENT-STREAM\r\n\r\nbody";
    let mut reader = std::io::BufReader::new(raw.as_bytes());
    let headers = read_headers(&mut reader).unwrap();
    assert_eq!(headers[0], "http/1.1 200 ok");
    assert!(headers.iter().any(|header| header.contains("event-stream")));
}

#[test]
fn a_backend_that_hangs_up_before_answering_is_an_error_not_an_empty_reply() {
    let mut reader = std::io::BufReader::new(&b""[..]);
    assert!(read_headers(&mut reader).is_err());
}

/// The defect a fleet run found and eight tests missed.
///
/// `llama-server` answers `503 Loading model` while a large model is still
/// coming off disk. Read as a body with no `data:` lines, that is a stream
/// which ended having produced nothing — so every request "completed", every
/// verdict passed, and no token was ever generated. A backend saying no must
/// not look like a backend saying nothing.
#[test]
fn a_backend_that_refuses_is_an_error_rather_than_an_empty_answer() {
    let headers = vec!["http/1.1 503 service unavailable".to_string()];
    let body = r#"{"error":{"message":"Loading model","code":503}}"#;
    let error = check_status(&headers, body).unwrap_err();
    assert!(error.contains("503"), "{error}");
    assert!(
        error.contains("Loading model"),
        "the reason survives: {error}"
    );
}

#[test]
fn a_refusal_with_no_body_still_names_the_status() {
    let headers = vec!["http/1.1 500 internal server error".to_string()];
    let error = check_status(&headers, "").unwrap_err();
    assert!(error.contains("500"), "{error}");
    assert!(error.contains("internal server error"), "{error}");
}

#[test]
fn every_success_code_is_accepted() {
    for status in [
        "http/1.1 200 ok",
        "http/1.1 201 created",
        "http/1.1 299 odd",
    ] {
        assert!(check_status(&[status.to_string()], "").is_ok(), "{status}");
    }
}

#[test]
fn a_missing_or_unreadable_status_line_is_refused_rather_than_assumed_fine() {
    assert!(check_status(&[], "").is_err());
    assert!(check_status(&["not a status line".to_string()], "").is_err());
}
