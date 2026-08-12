//! Default combined P4 controller/node agent. See `apps/p4/docs/architecture.md`.

use p4_runtime::{parse_agent_options, serve_agent};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    serve_agent(parse_agent_options()?)
}
