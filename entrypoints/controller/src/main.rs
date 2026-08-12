//! Remote P4 controller relay. See `apps/p4/docs/architecture.md`.

use p4_runtime::{ControllerProcessor, parse_bindings, serve};
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (listen, routes) = parse_bindings()?;
    serve(
        listen,
        "P4_CONTROLLER",
        Arc::new(ControllerProcessor::remote(routes)),
    )
}
