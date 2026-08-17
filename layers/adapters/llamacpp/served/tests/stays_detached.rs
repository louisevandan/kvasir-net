//! The property that makes this adapter maintainable, checked rather than
//! asserted in a comment.
//!
//! llama.cpp moves fast. An adapter that reaches into it inherits that pace:
//! every upstream commit is a rebase, and the patch series next door — fifty
//! three files across four upstream revisions — is what that costs when the
//! answer is "patch it". This adapter pays none of it because it attaches to
//! the public HTTP surface and compiles against nothing of llama.cpp's.
//!
//! That is only true while it stays true. A `cc` build script, a `-sys` crate,
//! a bindgen dependency or an include of `llama.h` would each be a reasonable
//! thing for someone to reach for and would each end the property silently —
//! the code would still work, and the next upstream release would be a
//! problem again. So it is a test.

use std::path::{Path, PathBuf};

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Nothing in the manifest links, builds or binds to llama.cpp.
#[test]
fn the_crate_compiles_against_nothing_of_llama_cpp() {
    let manifest = read(&crate_root().join("Cargo.toml"));
    for forbidden in [
        "build.rs",
        "links",
        "cc =",
        "bindgen",
        "-sys",
        "cmake",
        "pkg-config",
    ] {
        assert!(
            !manifest.contains(forbidden),
            "the manifest mentions {forbidden}, which would tie this crate to a \
             llama.cpp build:\n{manifest}"
        );
    }
    assert!(
        !crate_root().join("build.rs").exists(),
        "a build script would be the first step towards compiling llama.cpp here"
    );
}

/// No source file reaches for llama.cpp's own headers or tree.
///
/// Naming llama.cpp is not the hazard and used to be caught as though it were:
/// a blanket ban on the token `llama_` fired on `fn llama_cpp`, the function
/// that composes `llama-server`'s flags. That function is the adapter doing its
/// job. Knowing a backend's command line is knowledge about a program, which
/// changes when someone renames a flag; knowing its ABI is knowledge about a
/// build, which changes when anyone commits. Only the second is what this file
/// exists to keep out, so only the second is checked.
#[test]
fn no_source_file_reaches_into_the_backend() {
    let mut checked = 0;
    for entry in walk(&crate_root().join("src")) {
        // Comments are read, not compiled. Naming `llama_state_seq_save_file`
        // to explain why a capability is refused is prose worth having, and a
        // guard that fired on it would be silenced by the first person to hit
        // it — which is how a guard stops guarding.
        let text = without_comments(&read(&entry));
        for forbidden in [
            "llama.h",
            "ggml.h",
            "upstream",
            "extern \"C\"",
            "libloading",
            "#[link",
        ] {
            assert!(
                !text.contains(forbidden),
                "{} mentions {forbidden}",
                entry.display()
            );
        }
        checked += 1;
    }
    assert!(
        checked >= 4,
        "the sources were actually read: {checked} files"
    );
}

/// The check the blocklist above is only an approximation of.
///
/// A blocklist catches the spellings somebody thought of. This catches the
/// whole category: calling a foreign function requires `unsafe`, so a crate
/// with no `unsafe` in it calls into no C library at all — llama.cpp's or
/// anyone's — whatever it is named or however it was reached.
#[test]
fn nothing_here_can_call_into_a_native_library() {
    for entry in walk(&crate_root().join("src")) {
        let text = without_comments(&read(&entry));
        assert!(
            !text.contains("unsafe"),
            "{} uses unsafe: this crate talks to its backend over a socket, and \
             the only reason to need unsafe would be to stop doing that",
            entry.display()
        );
    }
}

/// What the adapter does depend on, stated so a change to it is deliberate.
///
/// Two crates: the interface it implements, and a JSON parser. The second is
/// there because the bytes come from another process and escapes are where
/// hand-rolled parsers are wrong.
#[test]
fn the_dependencies_are_the_two_that_were_chosen() {
    let manifest = read(&crate_root().join("Cargo.toml"));
    let deps: Vec<&str> = manifest
        .lines()
        .skip_while(|line| !line.starts_with("[dependencies]"))
        .skip(1)
        .filter(|line| line.contains('='))
        .map(|line| line.split('=').next().unwrap_or_default().trim())
        .collect();
    assert_eq!(
        deps,
        vec!["p4-adapter", "serde_json"],
        "a new dependency here is worth a second look"
    );
}

/// The compatibility boundary is the HTTP surface, and it is the whole of it.
///
/// If this list ever needs a llama.cpp-specific path on it, the adapter has
/// started depending on a particular build rather than on the contract three
/// backends share.
#[test]
fn the_only_coupling_is_a_surface_three_backends_serve() {
    let mut paths = Vec::new();
    for entry in walk(&crate_root().join("src")) {
        for line in read(&entry).lines() {
            for start in line.match_indices("\"/").map(|(index, _)| index) {
                if let Some(end) = line[start + 1..].find('"') {
                    paths.push(line[start + 1..start + 1 + end].to_owned());
                }
            }
        }
    }
    paths.sort();
    paths.dedup();
    assert_eq!(
        paths,
        vec!["/v1/chat/completions", "/v1/models"],
        "only the OpenAI-compatible endpoints are reached"
    );
}

fn walk(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(walk(&path));
        } else if path.extension().is_some_and(|kind| kind == "rs") {
            found.push(path);
        }
    }
    found
}

/// Drops line comments, so the guards read what the compiler reads.
///
/// Line comments only: this crate has no block comments, and a full Rust
/// lexer to catch a `//` inside a string literal would be more machinery than
/// the thing it protects.
fn without_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| match line.find("//") {
            Some(start) => &line[..start],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}
