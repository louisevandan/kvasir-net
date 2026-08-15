//! What an adapter reports, and it reports by raising an event.
//!
//! Nothing here is a return value. A node advances only when an event reaches
//! its queue, which is what keeps a hop's duration out of any worker's time.
//!
//! Split in two: `report` is the vocabulary and grows with observability
//! needs, `sink` is how events travel and should not move at all.

pub mod report;
pub mod sink;

pub use report::{Allocation, Event, Outcome};
pub use sink::EventSink;
