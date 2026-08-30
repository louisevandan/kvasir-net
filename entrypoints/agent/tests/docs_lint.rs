//! Runs the documentation gate as part of `cargo test --workspace`, the
//! repository's level-one verification entry point. The gate itself lives in
//! tools/scripts/docs-lint.mjs; this test only enforces that it passes.

use std::process::Command;

#[test]
fn documentation_gate() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let output = Command::new("node")
        .arg("tools/scripts/docs-lint.mjs")
        .current_dir(root)
        .output()
        .expect("node is required to run the documentation gate (tools/scripts/docs-lint.mjs)");
    assert!(
        output.status.success(),
        "docs-lint failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
