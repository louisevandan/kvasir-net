# HF adapter working rules

First read AGENTS.md at the P4 root and the current roadmap, verification conventions and isolation contract, then follow the relevant contract in the HF README.
This folder is a concrete adapter of P4. Do not create a separate Git repository or workspace, or an adjacent HF checkout.

- The Rust bridge owns retained/IPC, identity, capacity, output and child lifetime. The per-model Python owns computation, scheduling, KV/recurrent state and codecs.
- Keep per-model and per-role folders. Do not build a common model framework up front. Do not lift model semantics into another adapter or the P4 common core.
- Rust consumes the P4 root Cargo.lock. Python environment locks and model manifests are owned by HF.
- Models go in the root `.cache/hf/models/`, environments in `.cache/hf/environments/`, and results in `target/hf/`. Use a new run directory.
- Verify functional changes with a real consumption counterexample and with removal mutations on an independent copy. Check the retained claim, buffer, state and child reclaim together.
- HF fixture tests require a standard Python. It can be set with HF_TEST_PYTHON; its absence is not treated as PASS.
- The status and paths recorded at the time in docs/history and dated reports are not the current contract. The BF16 failure and the unverified scope still stand.
- Use Windows paths in Windows commands. Do not widen the scope on your own to remote execution, push or changes to other projects.
