use super::*;

fn link(node: &str, port: u16) -> Link {
    Link {
        address: Address::tcp("127.0.0.1", port),
        node: node.into(),
        binding: format!("{node}-binding"),
        generation: 1,
    }
}

fn chain_of(count: u16) -> Chain {
    Chain::new(
        (0..count)
            .map(|i| link(&format!("n{i}"), 52001 + i))
            .collect(),
    )
    .unwrap()
}

#[test]
fn a_chain_of_one_is_valid() {
    // This is how vLLM and SGLang participate: they spread the model inside
    // themselves, so the whole model is one addressable node.
    let chain = chain_of(1);
    assert_eq!(chain.len(), 1);
    assert!(chain.is_first() && chain.is_last());
    assert_eq!(chain.peek_next(), None);
    assert_eq!(chain.advance(), None);
}

#[test]
fn a_chain_naming_nobody_is_refused_at_construction() {
    assert!(Chain::new(Vec::new()).is_err());
}

#[test]
fn advancing_walks_the_order_and_stops_at_the_end() {
    let mut chain = chain_of(3);
    assert_eq!(chain.current().node, "n0");
    assert!(chain.is_first() && !chain.is_last());

    chain = chain.advance().expect("second hop");
    assert_eq!(chain.current().node, "n1");
    assert!(!chain.is_first() && !chain.is_last());

    chain = chain.advance().expect("third hop");
    assert_eq!(chain.current().node, "n2");
    assert!(chain.is_last());
    assert_eq!(chain.advance(), None);
}

#[test]
fn the_whole_chain_travels_so_a_later_node_still_knows_the_order() {
    let chain = chain_of(3).advance().unwrap().advance().unwrap();
    assert_eq!(chain.position(), 2);
    assert_eq!(chain.links().len(), 3);
    assert_eq!(chain.links()[0].node, "n0");
}

#[test]
fn a_decode_lap_restarts_rather_than_advance_wrapping() {
    // Wrapping would make "finished a pass" and "continuing generation" the
    // same event, and the ring would have no visible boundary.
    let end = chain_of(3).advance().unwrap().advance().unwrap();
    assert!(end.is_last());
    let lap = end.restart();
    assert!(lap.is_first());
    assert_eq!(lap.current().node, "n0");
}

#[test]
fn a_position_past_the_end_is_refused() {
    let links = chain_of(2).links().to_vec();
    assert!(Chain::at(links.clone(), 1).is_ok());
    assert!(Chain::at(links, 2).is_err());
}

#[test]
fn a_link_names_the_generation_so_a_rebound_node_is_not_executed_against() {
    let mut stale = link("n0", 52001);
    let fresh = Link {
        generation: 2,
        ..stale.clone()
    };
    stale.generation = 1;
    assert_ne!(stale, fresh);
}
