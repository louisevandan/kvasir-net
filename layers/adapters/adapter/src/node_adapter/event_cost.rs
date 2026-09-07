//! Storage owned by an immutable Event, without serializing or cloning it.
//!
//! This is an allocation-capacity accounting unit, NOT wire length, allocator
//! overhead, process RSS, native workspace, or the cost of another Event clone.
//! The queue accounts for its own entry/arena metadata separately. Holding an
//! Event outside the queue must retain the same storage claim until ownership
//! moves to another budget or the Event is destroyed.
use p4_protocol::Address;
use p4_protocol::event::{Endpoint, Envelope, Event, OuterEndpoint};
use std::fmt;
use std::mem::size_of;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceCostError {
    Overflow,
}

impl fmt::Display for ResourceCostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Overflow => formatter.write_str("retained event storage cost overflow"),
        }
    }
}

impl std::error::Error for ResourceCostError {}

fn add(total: &mut usize, amount: usize) -> Result<(), ResourceCostError> {
    *total = total
        .checked_add(amount)
        .ok_or(ResourceCostError::Overflow)?;
    Ok(())
}

fn address_heap(address: &Address) -> usize {
    // Exhaustive destructuring makes a new owned field a compilation failure,
    // rather than silently leaving it outside the storage contract.
    let Address {
        scheme: _,
        host,
        port: _,
    } = address;
    host.capacity()
}

fn outer_heap(outer: &OuterEndpoint, total: &mut usize) -> Result<(), ResourceCostError> {
    let OuterEndpoint {
        ingress_agent,
        channel,
        connection_generation: _,
    } = outer;
    add(total, address_heap(ingress_agent))?;
    add(total, channel.capacity())
}

fn endpoint_heap(endpoint: &Endpoint, total: &mut usize) -> Result<(), ResourceCostError> {
    match endpoint {
        Endpoint::Agent(address) => add(total, address_heap(address)),
        Endpoint::Node {
            agent,
            node,
            generation: _,
        } => {
            add(total, address_heap(agent))?;
            add(total, node.capacity())
        }
        Endpoint::Outer(outer) => outer_heap(outer, total),
    }
}

/// Counts Event inline storage once and every independently owned heap buffer
/// by capacity, including currently unused bytes. No scratch allocation occurs.
/// Equal wire bytes may therefore have different retained costs. This function
/// measures a value; it does not validate protocol identity or reserve storage.
pub fn retained_event_bytes(event: &Event) -> Result<usize, ResourceCostError> {
    let Event { envelope, payload } = event;
    let Envelope {
        protocol_version: _,
        event_id,
        correlation_id,
        causation_id,
        source,
        target,
        return_route,
        class: _,
        sequence: _,
        deadline_unix_ms: _,
        adapter_kind,
        payload_content_type,
    } = envelope;
    let mut total = size_of::<Event>();
    for amount in [
        payload.capacity(),
        event_id.capacity(),
        correlation_id.capacity(),
        payload_content_type.capacity(),
        causation_id.as_ref().map_or(0, String::capacity),
        adapter_kind.as_ref().map_or(0, String::capacity),
    ] {
        add(&mut total, amount)?;
    }
    endpoint_heap(source, &mut total)?;
    endpoint_heap(target, &mut total)?;
    if let Some(route) = return_route {
        outer_heap(route, &mut total)?;
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use p4_protocol::event::EventClass;

    fn empty() -> Event {
        Event {
            envelope: Envelope {
                protocol_version: Envelope::VERSION,
                event_id: String::new(),
                correlation_id: String::new(),
                causation_id: None,
                source: Endpoint::agent(Address::tcp(String::new(), 1)),
                target: Endpoint::agent(Address::tcp(String::new(), 2)),
                return_route: None,
                class: EventClass::Data,
                sequence: 1,
                deadline_unix_ms: None,
                adapter_kind: None,
                payload_content_type: String::new(),
            },
            payload: Vec::new(),
        }
    }

    fn spare_string() -> String {
        let mut text = String::with_capacity(257);
        text.push('x');
        text
    }

    #[test]
    fn inline_event_is_counted_once_even_when_empty() {
        assert_eq!(retained_event_bytes(&empty()), Ok(size_of::<Event>()));
    }

    #[test]
    fn every_optional_and_required_owned_string_counts_its_spare_capacity() {
        let setters: [fn(&mut Envelope, String); 6] = [
            |e, text| e.event_id = text,
            |e, text| e.correlation_id = text,
            |e, text| e.payload_content_type = text,
            |e, text| e.causation_id = Some(text),
            |e, text| e.adapter_kind = Some(text),
            |e, text| e.source = Endpoint::agent(Address::tcp(text, 1)),
        ];
        for set in setters {
            let mut event = empty();
            let text = spare_string();
            let allocation = text.capacity();
            set(&mut event.envelope, text);
            assert_eq!(
                retained_event_bytes(&event),
                Ok(size_of::<Event>() + allocation)
            );
        }
    }

    #[test]
    fn nested_routes_are_separate_allocations_not_deduplicated_by_equal_text() {
        let mut event = empty();
        let strings: Vec<_> = (0..6).map(|_| spare_string()).collect();
        let allocations: usize = strings.iter().map(String::capacity).sum();
        let mut strings = strings.into_iter();
        event.envelope.source = Endpoint::node(
            Address::tcp(strings.next().unwrap(), 1),
            strings.next().unwrap(),
            1,
        );
        event.envelope.target = Endpoint::outer(
            Address::tcp(strings.next().unwrap(), 2),
            strings.next().unwrap(),
            1,
        );
        event.envelope.return_route = Some(OuterEndpoint {
            ingress_agent: Address::tcp(strings.next().unwrap(), 3),
            channel: strings.next().unwrap(),
            connection_generation: 1,
        });
        assert_eq!(
            retained_event_bytes(&event),
            Ok(size_of::<Event>() + allocations)
        );
    }

    #[test]
    fn empty_reserved_payload_is_not_free_and_equal_payloads_can_have_different_costs() {
        let mut first = empty();
        let mut second = empty();
        first.payload = Vec::with_capacity(8192);
        second.payload = Vec::with_capacity(32);
        assert_eq!(first, second);
        assert_eq!(
            retained_event_bytes(&first),
            Ok(size_of::<Event>() + first.payload.capacity())
        );
        assert_eq!(
            retained_event_bytes(&second),
            Ok(size_of::<Event>() + second.payload.capacity())
        );
        assert!(retained_event_bytes(&first).unwrap() > retained_event_bytes(&second).unwrap());
        first.payload.push(0xf0);
        assert_eq!(
            retained_event_bytes(&first),
            Ok(size_of::<Event>() + first.payload.capacity())
        );
    }

    #[test]
    fn overflowing_cost_is_rejected_without_allocating_large_buffers() {
        let mut total = usize::MAX - 3;
        assert_eq!(add(&mut total, 4), Err(ResourceCostError::Overflow));
        assert_eq!(
            total,
            usize::MAX - 3,
            "failed accounting leaves the prefix unchanged"
        );
    }
}
