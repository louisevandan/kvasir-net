//! Which queue a frame waits in.
//!
//! Carried by the envelope rather than derived from a body, so a receiver can
//! place a frame without decoding it — the property that lets a socket reader
//! do nothing but enqueue.
//!
//! Four, because the workload has four kinds of traffic and they deserve
//! different depths: control must not starve under inference, a decode lap
//! belongs to a request already holding KV across a chain, prefill is the long
//! arrival, and a response is work already done.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueClass {
    Control,
    Prefill,
    Decode,
    Response,
}
