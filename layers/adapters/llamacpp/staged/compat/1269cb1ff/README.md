# llama.cpp compatibility layer 1269cb1ff

This directory is the only Linker-owned patch boundary for the official
`ggml-org/llama.cpp` commit recorded in `manifest.json`.

- `apps/p4/layers/adapters/llamacpp/upstream` remains a pristine ignored official clone.
- Stock RPC/server builds compile that clone directly.
- Pipeline builds run `apps/p4/layers/adapters/llamacpp/staged/scripts/prepare-pipeline-upstream.mjs`, which creates
  a generated worktree under ignored `.cache/`, verifies every patch hash, and
  applies this ordered patch list there.
- No generated worktree or patched llama.cpp source is committed.

## Update procedure

1. Fetch official `master`, select one immutable commit, and record a new
   versioned compatibility directory without editing the official clone.
2. Prove an unchanged stock `ggml-rpc-server` and `llama-server` build.
3. Port this ordered patch set in a temporary worktree. Preserve new official
   model and scheduler behavior when resolving conflicts.
4. Replace this versioned directory and update `manifest.json`, including the
   patch hashes and the canonical applied diff hash.
5. Run the preparer twice: the first run must create and patch; the second must
   validate and reuse the same generated source path.
6. Build stock and Pipeline artifacts independently, then validate real
   load/chat/unload on CUDA, Metal, and OpenCL before fleet promotion.

If any patch does not apply cleanly, the update stops before CMake. Do not edit
the official checkout to make the build pass.
