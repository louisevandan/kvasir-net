use super::*;

#[test]
fn one_route_always_lands_on_one_worker() {
    // Order within a route is what the CPS rule buys. Spreading a route's
    // frames across workers throws it away, and a caller sees that as P4
    // reordering its stream.
    for workers in [1usize, 2, 4, 8, 13] {
        let first = route_worker("inference-42", workers);
        for _ in 0..64 {
            assert_eq!(route_worker("inference-42", workers), first);
        }
    }
}

#[test]
fn a_worker_index_is_always_in_range() {
    for workers in [1usize, 2, 3, 7, 16] {
        for route in ["a", "", "route-1", "매우-긴-경로-이름", "\u{1F600}"] {
            assert!(route_worker(route, workers) < workers);
        }
    }
}

#[test]
fn different_routes_spread_across_workers() {
    // Otherwise the pool would be one worker wearing a hat.
    let workers = 8;
    let used: std::collections::HashSet<usize> = (0..200)
        .map(|index| route_worker(&format!("route-{index}"), workers))
        .collect();
    assert!(used.len() > 1, "routes shared a single worker");
}

#[test]
fn the_pool_is_never_empty_and_never_wider_than_the_budget() {
    assert_eq!(worker_count(0), 1);
    assert!(worker_count(1) >= 1);
    assert!(worker_count(4) <= 4);
}
