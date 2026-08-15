use super::*;
use p4_protocol::{Address, Envelope, Recipient};

fn frame(route: &str, lane: QueueClass) -> Frame {
    Frame {
        envelope: Envelope {
            target: Address::tcp("127.0.0.1", 19001),
            recipient: Recipient::Agent,
            lane,
            route: route.into(),
            deadline_unix_ms: 0,
            reply_to: None,
            chain: None,
        },
        body: Vec::new(),
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn control_is_taken_before_inference() {
    // Under an inference flood the ability to create or delete a node must
    // survive, so control cannot queue behind prefill.
    runtime().block_on(async {
        let (sender, mut receiver, _) = channel(Lanes::default(), Budget::default());
        sender.offer(frame("p", QueueClass::Prefill)).unwrap();
        sender.offer(frame("d", QueueClass::Decode)).unwrap();
        sender.offer(frame("c", QueueClass::Control)).unwrap();

        assert_eq!(receiver.take().await.unwrap().envelope.route, "c");
    });
}

#[test]
fn decode_is_taken_before_prefill() {
    runtime().block_on(async {
        let (sender, mut receiver, _) = channel(Lanes::default(), Budget::default());
        sender.offer(frame("p", QueueClass::Prefill)).unwrap();
        sender.offer(frame("d", QueueClass::Decode)).unwrap();

        assert_eq!(receiver.take().await.unwrap().envelope.route, "d");
    });
}

#[test]
fn a_full_lane_refuses_instead_of_blocking_the_reader() {
    // A socket receiver only enqueues. If a full lane blocked it, one busy
    // route would stall every other route sharing that connection.
    runtime().block_on(async {
        let lanes = Lanes {
            prefill: 1,
            ..Lanes::default()
        };
        let (sender, _receiver, _) = channel(lanes, Budget::default());
        sender.offer(frame("first", QueueClass::Prefill)).unwrap();

        let refused = sender.offer(frame("second", QueueClass::Prefill));
        assert_eq!(refused, Err(Refused(frame("second", QueueClass::Prefill))));
    });
}

#[test]
fn one_full_lane_does_not_close_the_others() {
    runtime().block_on(async {
        let lanes = Lanes {
            prefill: 1,
            ..Lanes::default()
        };
        let (sender, _receiver, _) = channel(lanes, Budget::default());
        sender.offer(frame("p1", QueueClass::Prefill)).unwrap();
        assert!(sender.offer(frame("p2", QueueClass::Prefill)).is_err());
        assert!(sender.offer(frame("c", QueueClass::Control)).is_ok());
        assert!(sender.offer(frame("d", QueueClass::Decode)).is_ok());
    });
}

#[test]
fn in_flight_width_is_independent_of_lane_depth() {
    runtime().block_on(async {
        let (_, receiver, in_flight) = channel(
            Lanes {
                prefill: 4096,
                ..Lanes::default()
            },
            Budget {
                in_flight: 2,
                ..Budget::default()
            },
        );
        // Deep lanes, narrow width. Conflating the two is the defect this
        // separation exists to prevent.
        assert_eq!(in_flight.available_permits(), 2);
        assert_eq!(receiver.depth().total(), 0);
    });
}

#[test]
fn depth_reports_each_lane_separately() {
    runtime().block_on(async {
        let (sender, receiver, _) = channel(Lanes::default(), Budget::default());
        sender.offer(frame("c", QueueClass::Control)).unwrap();
        sender.offer(frame("p1", QueueClass::Prefill)).unwrap();
        sender.offer(frame("p2", QueueClass::Prefill)).unwrap();

        let depth = receiver.depth();
        assert_eq!(depth.control, 1);
        assert_eq!(depth.prefill, 2);
        assert_eq!(depth.decode, 0);
        assert_eq!(depth.total(), 3);
    });
}
