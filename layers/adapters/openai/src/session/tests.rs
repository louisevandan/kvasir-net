use super::*;

/// The stream ends without a finish reason, which a backend that is killed
/// mid-answer produces. Reported as an ending rather than waited on: a caller
/// left holding a route that never terminates is the failure this avoids.
#[test]
fn a_stream_that_just_stops_is_reported_as_an_ending() {
    let (sender, tokens) = channel::<Result<Chunk, String>>();
    drop(sender);
    let mut session = Session {
        delivered: 0,
        tokens,
        ended: None,
        // No socket: these build a session directly to test the token

        // bookkeeping, so there is nothing to end when they are dropped.
        closer: None,
    };
    assert!(matches!(
        session.token(Duration::from_millis(50)),
        Next::Done(_)
    ));
    assert!(session.finished());
}

#[test]
fn a_finished_session_keeps_saying_so() {
    let (_sender, tokens) = channel::<Result<Chunk, String>>();
    let mut session = Session {
        delivered: 0,
        tokens,
        ended: Some("length".into()),
        closer: None,
    };
    match session.token(Duration::from_millis(10)) {
        Next::Done(reason) => assert_eq!(reason, "length"),
        _ => panic!("a finished session answers from what it recorded"),
    }
}

/// A last chunk may carry a token as well as a reason, and dropping it loses
/// the final word of every answer.
#[test]
fn the_text_on_a_final_chunk_is_not_thrown_away() {
    let (sender, tokens) = channel();
    sender
        .send(Ok(Chunk {
            text: "끝".into(),
            stop: Some("stop".into()),
        }))
        .unwrap();
    let mut session = Session {
        delivered: 0,
        tokens,
        ended: None,
        // No socket: these build a session directly to test the token

        // bookkeeping, so there is nothing to end when they are dropped.
        closer: None,
    };
    match session.token(Duration::from_millis(50)) {
        Next::Token { text, position } => {
            assert_eq!(text, "끝");
            assert_eq!(position, 1, "and is the first token of the stream");
        }
        other => panic!(
            "the token came first: {}",
            match other {
                Next::Done(reason) => format!("done {reason}"),
                Next::Failed(detail) => format!("failed {detail}"),
                Next::Token { .. } => unreachable!(),
            }
        ),
    }
    // And the ending is still reported on the next hop.
    assert!(matches!(
        session.token(Duration::from_millis(10)),
        Next::Done(_)
    ));
}

/// Keep-alives are not laps. Reporting one as a token would have the ring go
/// round producing nothing.
#[test]
fn an_empty_chunk_is_waited_through_rather_than_reported() {
    let (sender, tokens) = channel();
    sender.send(Ok(Chunk::default())).unwrap();
    sender
        .send(Ok(Chunk {
            text: "실제".into(),
            stop: None,
        }))
        .unwrap();
    let mut session = Session {
        delivered: 0,
        tokens,
        ended: None,
        // No socket: these build a session directly to test the token

        // bookkeeping, so there is nothing to end when they are dropped.
        closer: None,
    };
    match session.token(Duration::from_millis(100)) {
        Next::Token { text, .. } => assert_eq!(text, "실제"),
        _ => panic!("waited through the keep-alive to the real token"),
    }
}

#[test]
fn a_backend_that_goes_quiet_is_a_failure_rather_than_a_wait_forever() {
    let (_sender, tokens) = channel::<Result<Chunk, String>>();
    let mut session = Session {
        delivered: 0,
        tokens,
        ended: None,
        // No socket: these build a session directly to test the token

        // bookkeeping, so there is nothing to end when they are dropped.
        closer: None,
    };
    match session.token(Duration::from_millis(30)) {
        Next::Failed(detail) => assert!(detail.contains("no token"), "{detail}"),
        _ => panic!("a silent backend is reported"),
    }
}
