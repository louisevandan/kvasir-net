use super::*;
use std::sync::Mutex;

#[derive(Default)]
struct Recorder(Mutex<Vec<Event>>);

impl EventSink for Recorder {
    fn raise(&self, event: Event) {
        self.0.lock().expect("recorder lock").push(event);
    }
}

#[test]
fn raising_an_event_returns_nothing_for_an_adapter_to_wait_on() {
    let recorder = Recorder::default();
    recorder.raise(Event::Unloaded {
        deployment: "d".into(),
    });
    assert_eq!(recorder.0.lock().unwrap().len(), 1);
}
