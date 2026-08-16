//! Whether a node may serve, and for which deployment.
//!
//! Its own file because it changes with the load transaction and with nothing
//! else. The event loop next door changes when scheduling does; these three
//! states change when what counts as a materialised model changes, and the two
//! have never moved together.
//!
//! A load is a transaction across machines that no single machine can see the
//! whole of. Nothing coordinates it — there is nowhere to put a coordinator
//! that would not become a controller — so each stage enforces its own half: it
//! serves the generation it bound, and a stage whose load failed serves
//! nothing. A chain composed over a failed stage is refused by that stage
//! rather than answered from half a model.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bound {
    /// Never loaded. A backend that needs no load is legitimate, so this
    /// serves — the ones that must not serve are the two below.
    Never,
    At(u64),
    /// A load failed here. Nothing runs until one succeeds.
    Refused,
}

impl Bound {
    /// Whether work carrying `generation` may run.
    pub fn admits(self, generation: u64) -> bool {
        match self {
            Self::Never => true,
            Self::At(bound) => bound == generation,
            Self::Refused => false,
        }
    }

    pub fn why(self, generation: u64) -> String {
        match self {
            Self::Never => String::new(),
            Self::At(bound) => {
                format!("node is bound at generation {bound}, work carries {generation}")
            }
            Self::Refused => "node has no model: its load failed".into(),
        }
    }
}
