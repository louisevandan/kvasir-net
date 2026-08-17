use super::Fleet;
use p4_protocol::Address;

fn at(port: u16) -> Address {
    Address::tcp("127.0.0.1", port)
}

/// A single chain reads exactly as it always did.
///
/// The grammar gained a separator rather than changing, so every command line
/// and every document naming a chain still means what it meant — including the
/// node names, which a staged backend reads a position out of.
#[test]
fn one_deployment_is_written_and_named_the_way_it_always_was() {
    let fleet = Fleet::parse("127.0.0.1:52001,127.0.0.1:52002").expect("parsed");
    assert_eq!(fleet.deployments().len(), 1);
    assert_eq!(fleet.stages(), 2);
    assert_eq!(fleet.node_of(0, 0), "stage-0");
    assert_eq!(fleet.node_of(0, 1), "tail-1");
}

/// Replicas carry which one they are, because they can share a machine.
///
/// A box with two cards holds two deployments at one agent. Without the
/// replica in the name the second `CreateNode` names the node the first one
/// already made, and the fleet quietly becomes one deployment with the requests
/// of two.
#[test]
fn replicas_are_named_apart() {
    let fleet = Fleet::parse("127.0.0.1:52001,127.0.0.1:52002;127.0.0.1:52001,127.0.0.1:52003")
        .expect("parsed");
    assert_eq!(fleet.deployments().len(), 2);
    assert_eq!(fleet.node_of(0, 0), "stage-0-d0");
    assert_eq!(fleet.node_of(1, 0), "stage-0-d1");
    assert_eq!(fleet.node_of(1, 1), "tail-1-d1");
    assert_ne!(fleet.node_of(0, 0), fleet.node_of(1, 0));
}

/// Every machine once, in the order first seen.
#[test]
fn a_machine_shared_by_two_replicas_is_watched_once() {
    let fleet = Fleet::parse("127.0.0.1:52001,127.0.0.1:52002;127.0.0.1:52001,127.0.0.1:52003")
        .expect("parsed");
    assert_eq!(fleet.addresses(), vec![at(52001), at(52002), at(52003)]);
}

/// Replicas of different lengths are refused rather than run.
///
/// `P4_DRIVE_SERVE` names stage indices, so a fleet whose deployments have
/// different shapes would have one index meaning different things in each — and
/// the failure would be a hop addressed to the wrong half of a deployment,
/// which looks like a backend that stopped answering.
#[test]
fn replicas_must_be_the_same_shape() {
    let refused = Fleet::parse("127.0.0.1:1,127.0.0.1:2;127.0.0.1:3").expect_err("refused");
    assert!(refused.contains("stages"), "{refused}");
}

#[test]
fn a_fleet_that_names_nothing_is_refused() {
    assert!(Fleet::parse("").is_err());
    assert!(Fleet::parse(";").is_err());
}

/// Whitespace and a trailing separator are tolerated, because a chain written
/// across a shell is usually pasted.
#[test]
fn spacing_and_a_trailing_separator_are_read_through() {
    let fleet = Fleet::parse(" 127.0.0.1:1 , 127.0.0.1:2 ; ").expect("parsed");
    assert_eq!(fleet.deployments().len(), 1);
    assert_eq!(fleet.deployments()[0], vec![at(1), at(2)]);
}

/// A plan falls back from the most specific naming to the least.
#[test]
fn a_replica_can_be_planned_apart_from_its_twin() {
    let fleet = Fleet::parse("127.0.0.1:1,127.0.0.1:2;127.0.0.1:3,127.0.0.1:4").expect("parsed");
    // SAFETY: single-threaded test, and the reads are below in this test.
    unsafe {
        std::env::set_var("P4_DRIVE_PLAN_1_0", "second-replica-first-stage");
        std::env::set_var("P4_DRIVE_PLAN_1", "any-replica-second-stage");
    }
    let plans = fleet.plans("fallback");
    unsafe {
        std::env::remove_var("P4_DRIVE_PLAN_1_0");
        std::env::remove_var("P4_DRIVE_PLAN_1");
    }
    assert_eq!(plans[0][0], "fallback");
    assert_eq!(plans[0][1], "any-replica-second-stage");
    assert_eq!(plans[1][0], "second-replica-first-stage");
    assert_eq!(plans[1][1], "any-replica-second-stage");
}
