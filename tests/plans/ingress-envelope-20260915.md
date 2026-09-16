# Ingress agent envelope and return route verification plan

Created 2026-09-15. Baseline P4 `4f2db5cda`. This is a follow-up fix limited to the user's requirement to route through an ingress agent.
The stop after 3 FINISH failures is not to be read as a resumption of the whole roadmap.
[Contract](../../docs/event-protocol-v2.md#reception-agent-and-an-outer-reachable-only-through-a-gateway).

## Environment and goal

Windows PowerShell, two real local TCP agents, the current HF feature, and llama.cpp/HF Qwen0.8B.
OUTER connects only to A, and model nodes are created on B. This is not acceptance of a real VPC/multi-computer deployment.
The network return target is ingress A; the OUTER channel and generation are A's local delivery information. Payload and identity are preserved.

## Procedure and expected values

1. After a real Runtime A→B control→A→OUTER round trip, check A's OUTER binding1/B0. Preserve the pre-fix B1 counterexample.
2. B's OUTER output goes outbound to A, and only A delivers it to the local OUTER mailbox. On Full, the original pointer, cost and receipt are preserved.
3. In an independent worktree with a separate empty build path, detect each of two mutations: removal of the registration guard, and premature local delivery of OUTER.
4. Aggregate every summary and the final exit of `cargo test --workspace --features hf-transformers --no-fail-fast`.
5. Confirm real llama.cpp/HF generation, EOS, release and UNLOAD/DELETE over OUTER→A→B, and that both agents end with empty nodes.
6. Terminate only the owned test agents, and record source/binary hashes, commands, logs, exits and the unverified scope.

Raw data: `target/ingress-envelope-20260915/`. Models, previously installed apps and Studio code are not changed.
The existing FINISH RED is kept separately, and its expected values are not relaxed. Stop if implementation verification in this scope fails 3 times.
