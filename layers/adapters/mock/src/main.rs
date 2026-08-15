//! P4 mock adapter entrypoint.
//!
//! A concrete adapter with no backend. It answers the whole adapter contract
//! from a declared simulation profile, so a run exercises P4 -- framing,
//! routing, admission, queueing, cancellation, deadlines -- with the layer
//! beneath it reduced to arithmetic. When something goes wrong here, nothing
//! below P4 is available to blame.

mod application;
mod domain;
mod infrastructure;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    application::adapter::run()
}
