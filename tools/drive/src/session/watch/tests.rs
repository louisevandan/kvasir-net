use super::Peaks;

/// A snapshot as the service layer writes one.
const SAMPLE: &str = "address=tcp://127.0.0.1:52001 forwarded=0 consumed=12 to_nodes=12 \
unrouted=0 control=0 prefill=3 decode=1 response=0 peers=1 waiting=4
node=tail-1 depth=20 running=10 routes=[r1-q4,r1-q5]";

#[test]
fn it_reads_the_three_figures_that_matter() {
    let peaks = Peaks::default();
    peaks.observe(SAMPLE);
    assert_eq!(
        peaks.node_depth.load(std::sync::atomic::Ordering::SeqCst),
        20
    );
    assert_eq!(peaks.running.load(std::sync::atomic::Ordering::SeqCst), 10);
    // The deepest of the four lanes, not their sum.
    assert_eq!(peaks.lane.load(std::sync::atomic::Ordering::SeqCst), 3);
    assert_eq!(peaks.samples(), 1);
}

/// Only the highest survives, because that is the claim: never more than the
/// ceiling, and never a main queue holding the backlog.
#[test]
fn a_later_shallower_sample_does_not_lower_a_peak() {
    let peaks = Peaks::default();
    peaks.observe(SAMPLE);
    peaks.observe(
        "control=0 prefill=0 decode=0 response=0\nnode=tail-1 depth=0 running=0 routes=[]",
    );
    assert_eq!(
        peaks.node_depth.load(std::sync::atomic::Ordering::SeqCst),
        20
    );
    assert_eq!(peaks.running.load(std::sync::atomic::Ordering::SeqCst), 10);
    assert_eq!(peaks.samples(), 2);
}

/// More than one node on the agent: the peak is the deepest of them, since the
/// claim is about every node rather than about their total.
#[test]
fn several_nodes_give_the_deepest_of_them() {
    let peaks = Peaks::default();
    peaks.observe("control=0 prefill=0 decode=0 response=0\nnode=a depth=4 running=2 routes=[]\nnode=b depth=17 running=9 routes=[]");
    assert_eq!(
        peaks.node_depth.load(std::sync::atomic::Ordering::SeqCst),
        17
    );
    assert_eq!(peaks.running.load(std::sync::atomic::Ordering::SeqCst), 9);
}

/// A route named after a key must not be read as one. Routes are caller-chosen
/// and the snapshot puts them on the same line.
#[test]
fn a_route_that_looks_like_a_field_is_not_one() {
    let peaks = Peaks::default();
    peaks.observe(
        "control=0 prefill=0 decode=0 response=0\nnode=a depth=2 running=1 routes=[depth=999]",
    );
    assert_eq!(
        peaks.node_depth.load(std::sync::atomic::Ordering::SeqCst),
        2
    );
}

#[test]
fn nothing_observed_is_visibly_nothing_observed() {
    let peaks = Peaks::default();
    assert_eq!(peaks.samples(), 0);
}
