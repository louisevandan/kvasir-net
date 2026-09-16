# Commission return context common contract verification plan

2026-09-15. Baseline `434bd97fc`. At the user's instruction, implement consistency between the envelope and the earlier analysis.
[Public contract](../../docs/event-protocol-v2.md#required-request-return-context).
The existing FINISH stop and the unfinished overall roadmap are kept distinct from this scope.

## Contract and procedure

1. OUTER knows the full placement and designates the ingress agent for each commission. source/target/return_route are kept separate.
2. Every valid event requires a return route. Reject routes that conflict with the OUTER source/target, and the P4E3 absent flag.
3. On a real retained broker rejection, confirm that the original pointer and held cost are kept and the queue/reservation/receipt are unchanged. A normal input with the same ID is accepted afterwards.
4. Both adapters use the common ReturnContext. For llama.cpp mixed results, select the route, correlation and deadline per owner, and keep the carrier causation.
5. Check two real retained node/llama worker stages and the TCP route ingress A→worker B→A→OUTER. For HF, also verify the real bridge/IPC and the OUTER reader.
6. In an independent worktree, remove common validation, next inheritance and owner selection, each separately. Record the actual recompilation in a separate build directory, the binary hash and the failure.
7. After `cargo test --workspace --features hf-transformers --no-fail-fast` ends, aggregate every summary. Do not hide the known FINISH failure.
8. Check real generation and reclaim for the existing llama.cpp and HF Qwen0.8B, and the final A/B nodes=[]. Terminate only the owned test processes.

## Scope and evidence

Windows PowerShell, two local agents; OUTER connects only to A, and nodes live on B. This is not acceptance of multiple physical computers/VPC or of large-model performance.
Raw data `target/return-context-20260915/`: source seal, commands/logs/exits, mutations, real-model results and cleanup evidence.
Normal fixtures are updated with an explicit return context. Existing malformed inputs are not deleted; both the earlier wire rejection and the direct consumer checks are preserved.
