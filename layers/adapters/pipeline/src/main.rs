//! Linker Pipeline P4 adapter entrypoint.

mod application;
mod domain;
mod infrastructure;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    application::adapter::run()
}
