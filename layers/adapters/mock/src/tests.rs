use super::*;
use p4_adapter::{Adapter, Event, EventSink, Hop, Load, Sequence, Unload, Work};
use std::sync::Mutex as StdMutex;

#[derive(Default)]
struct Recorder(StdMutex<Vec<Event>>);

impl EventSink for Recorder {
    fn raise(&self, event: Event) {
        self.0.lock().unwrap().push(event);
    }
}

impl Recorder {
    fn events(&self) -> Vec<Event> {
        self.0.lock().unwrap().clone()
    }
}

fn sequence(id: &str, remaining: u32) -> Sequence {
    Sequence {
        sequence: id.into(),
        session_epoch: 0,
        state: None,
        prompt: Some("p".into()),
        remaining,
        options: "{}".into(),
    }
}

fn hop(width: usize) -> Work {
    Work::Hop(Hop {
        id: 1,
        deployment: "d".into(),
        sequences: (0..width).map(|i| sequence(&format!("s{i}"), 4)).collect(),
    })
}

fn named_hop(id: &str) -> Work {
    Work::Hop(Hop {
        id: 1,
        deployment: "d".into(),
        sequences: vec![Sequence {
            sequence: id.into(),
            session_epoch: 0,
            state: None,
            prompt: Some("p".into()),
            remaining: 4,
            options: "{}".into(),
        }],
    })
}

mod cache;
mod core;
mod observations;
mod transactions;
