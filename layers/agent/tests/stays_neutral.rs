//! That the core has not learned a backend.
//!
//! The dependency graph already makes naming a backend *type* a compile error:
//! this crate depends on the adapter contract and the protocol, and on nothing
//! that knows what a model is. That is the strong half and it needs no test.
//!
//! The weak half is everything a compiler cannot see. Tuning is when a
//! scheduler acquires opinions about the thing it feeds — a batch size that
//! suits one runtime, a lane that means something only to a server that
//! batches a particular way, a constant chosen by watching one backend. None
//! of that is a type error, and all of it ends the same way: a core that is
//! fast for the backend it was tuned against and wrong for the next one.
//!
//! So the names are checked. A file here that mentions llama.cpp, vLLM, CUDA,
//! HTTP or a model format is a file that has stopped being neutral, whatever
//! it does with the knowledge.

use std::path::{Path, PathBuf};

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn sources(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|kind| kind == "rs") {
            found.push(path);
        }
    }
}

/// Words that only mean something if you know which backend is behind the
/// boundary. Comments included on purpose: a constant explained by naming the
/// runtime it was measured against is exactly the coupling this guards.
const BACKENDS: &[&str] = &[
    "llama",
    "llamacpp",
    "vllm",
    "sglang",
    "gguf",
    "ggml",
    "cuda",
    "rocm",
    "openai",
    "http",
    "tokenizer",
    "kv_cache",
    "safetensors",
];

#[test]
fn no_source_in_the_core_names_a_backend() {
    let mut files = Vec::new();
    sources(&crate_root().join("src"), &mut files);
    assert!(files.len() > 10, "the sources were not found");

    // Tests are exempt, and deliberately. One of them explains that a lap of a
    // one-link chain is a decode step "for vLLM and SGLang alike" — naming
    // backends there is a statement that the rule covers them, which is the
    // opposite of the coupling this guards against. What must stay clean is
    // the code that decides.
    for file in files
        .into_iter()
        .filter(|path| path.file_name().is_some_and(|name| name != "tests.rs"))
    {
        let text = std::fs::read_to_string(&file)
            .unwrap_or_default()
            .to_ascii_lowercase();
        for name in BACKENDS {
            assert!(
                !text.contains(name),
                "{} names {name}: the core decides for every backend or for none",
                file.display()
            );
        }
    }
}

/// What this crate is allowed to depend on.
///
/// Stated rather than left to review. A new dependency here is how a backend
/// gets in without any file naming one — a client, a tokeniser, a format
/// reader — and the graph is the invariant that makes the rest of it hold.
#[test]
fn the_core_depends_on_the_contract_the_protocol_and_a_runtime() {
    let manifest = std::fs::read_to_string(crate_root().join("Cargo.toml")).expect("manifest");
    let declared: Vec<&str> = manifest
        .lines()
        .skip_while(|line| line.trim() != "[dependencies]")
        .skip(1)
        .take_while(|line| !line.trim().starts_with('['))
        .filter_map(|line| line.split('=').next())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect();
    assert_eq!(
        declared,
        vec!["p4-adapter", "p4-protocol", "tokio"],
        "the core's dependencies changed"
    );
}

/// The window composer decides from lanes and a ceiling, and from nothing else.
///
/// This is the file tuning pressure lands on: it is where throughput is won or
/// lost, and where a rule shaped around one runtime's batching would be
/// invisible. Its inputs are the guard — a policy that cannot see a body, a
/// plan or a deployment cannot be written against a particular backend even by
/// accident.
#[test]
fn the_window_composer_is_given_nothing_that_names_a_deployment() {
    let text = std::fs::read_to_string(crate_root().join("src/node/window/mod.rs"))
        .expect("the window composer");

    // Its signature, not its prose. Prose says "while the deployment has room",
    // which is what the rule means; what matters is that the function cannot
    // reach a deployment, a body or a plan even if someone wanted it to.
    assert!(
        text.contains("pub fn compose(waiting: &[Waiting], ceiling: usize, now_unix_ms: u64)"),
        "the composer's inputs changed: they are the guard, because a policy \
         that cannot see a body or a plan cannot be written for one backend"
    );
    for reaching in ["use crate::node::payload", "use crate::agent", "Frame"] {
        assert!(
            !text.contains(reaching),
            "the composer reaches for {reaching}: it decides from lanes and a ceiling"
        );
    }
}
