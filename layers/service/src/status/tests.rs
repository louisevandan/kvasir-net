use super::*;

#[test]
fn a_name_that_would_break_the_snapshot_is_escaped() {
    // A node id and a route both come from a caller. Left alone, one of them
    // could make a snapshot claim a node that is not there.
    assert_eq!(escape("n0\nnode=fake"), "n0_node=fake");
    assert_eq!(escape("r,1"), "r_1");
    assert_eq!(escape("[boxed]"), "_boxed_");
    assert_eq!(escape("ordinary-route-7"), "ordinary-route-7");
}
