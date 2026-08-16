//! Which server is behind the surface, and the little that follows from it.
//!
//! The three backends this adapter serves — llama.cpp, vLLM, SGLang — speak the
//! same OpenAI-compatible HTTP, which is why there is one adapter rather than
//! three. What a flavour carries is only what genuinely differs, and the list
//! is short on purpose: a field here that could be a plan key is a field that
//! makes the adapter know a backend for no reason.
//!
//! Nothing here changes the wire. It changes what a load has to establish
//! before the wire is used.

/// What a name registered in the agent's registry means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavour {
    /// `llama-server`. One model per process, and it answers to any name asked
    /// of it, so a plan that omits the model is served anyway.
    LlamaCpp,
    /// vLLM's OpenAI server. It matches the request's `model` against what it
    /// serves and answers 404 to anything else, so a plan that omits the model
    /// has to be told what the server is holding before a request is built.
    Vllm,
    /// SGLang's OpenAI server. Lenient about the name like llama.cpp, and
    /// separate from it because being lenient is a fact about today's SGLang
    /// rather than a promise, and a name of its own is where that gets fixed
    /// if it changes.
    Sglang,
}

impl Flavour {
    /// The name this backend is registered under.
    pub fn name(self) -> &'static str {
        match self {
            Self::LlamaCpp => "llamacpp",
            Self::Vllm => "vllm",
            Self::Sglang => "sglang",
        }
    }

    /// Whether a load must find out what the server calls its model.
    ///
    /// Only vLLM refuses a name it does not serve. Asking every backend anyway
    /// would be simpler and would be wrong: it turns one round trip that a
    /// deployment needs into one that every deployment pays, and it hides the
    /// difference rather than stating it.
    pub fn insists_on_the_model_name(self) -> bool {
        matches!(self, Self::Vllm)
    }

    /// Reads the name a registry entry was made under.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "llamacpp" => Some(Self::LlamaCpp),
            "vllm" => Some(Self::Vllm),
            "sglang" => Some(Self::Sglang),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
