//! Remote P4 Node Communication Host. See `apps/p4/docs/architecture.md`.

use p4_runtime::{NodeProcessor, parse_bindings, serve};
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (listen, routes) = parse_bindings()?;
    serve(listen, "P4_NODE", Arc::new(NodeProcessor::adapters(routes)))
}
