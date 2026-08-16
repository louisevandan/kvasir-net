//! Which stages an inference visits.
//!
//! The one piece of the driver that decides something. It exists because a
//! backend spreading a model internally has shares that are loaded and serve
//! nothing, and the driver must be told which is which rather than reading a
//! plan to find out — a plan is opaque above the adapter boundary, and a tool
//! that peeked at one would be a tool that knew a backend.

use super::serving;

#[test]
fn every_stage_serves_unless_told_otherwise() {
    assert_eq!(serving(None, 3).unwrap(), vec![0, 1, 2]);
}

/// The distributed llama.cpp shape: a share that holds and a front that serves.
#[test]
fn a_held_share_can_be_left_out_of_the_chain() {
    assert_eq!(serving(Some("1"), 2).unwrap(), vec![1]);
    assert_eq!(serving(Some("0, 2"), 3).unwrap(), vec![0, 2]);
}

#[test]
fn a_stage_that_is_not_in_the_deployment_is_refused() {
    let error = serving(Some("0,2"), 2).unwrap_err().to_string();
    assert!(error.contains("stage 2 of 2"), "{error}");
}

#[test]
fn a_chain_that_serves_nothing_is_refused() {
    let error = serving(Some(""), 2).unwrap_err().to_string();
    assert!(error.contains("names no stage"), "{error}");
}

/// A chain runs in stage order, so the indices have to.
#[test]
fn a_chain_that_goes_backwards_is_refused() {
    let error = serving(Some("1,0"), 2).unwrap_err().to_string();
    assert!(error.contains("ascend"), "{error}");
}

#[test]
fn a_stage_named_twice_is_refused() {
    assert!(serving(Some("1,1"), 2).is_err());
}

/// Node names carry their position in the whole deployment, so the stage a
/// chain ends on is the one that was created as its tail. A chain ending
/// anywhere else would decode against a node never told it was last.
#[test]
fn a_chain_must_end_at_the_deployments_tail() {
    let error = serving(Some("0"), 2).unwrap_err().to_string();
    assert!(error.contains("must end at stage 1"), "{error}");
}
