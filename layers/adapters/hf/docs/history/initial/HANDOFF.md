> Historical record: requirements, status and measurements from the time of the standalone repository. The current layout and usage follow the [HF guide](../../../README.md). The complete original is in the Git bundle preserved during the migration.

# Next-session handoff

2026-09-14. HF-0 to HF-3 of §0 of the P4 development plan were implemented and verified.
Current results and limitations are owned by the [acceptance report](../../../tests/reports/p4-integration/20260914_023000.md),
and the execution, budget and deployment contract by the [integration specification](../../integration/README.md).

## Implementation and verification

- HF owns a standalone Rust bridge and the per-model Python. P4 wires up the optional feature, the external dependency and factory/INSPECT.
- The OUTER model controller uses P4 events. Each stage owns its assigned weights/cache and its Python process.
- Within one LOAD, 8 requests × 3 epochs, cancellation, release, rejection of the previous epoch, UNLOAD/DELETE, and a compatible Python A→B replacement were verified.
- Real Qwen passed the existing v1 8 combinations/47 steps, the final local event 99 steps/198 stage-caches,
  two physical hosts 75 steps/150 stage-caches, and re-acceptance after a response disconnection 4 steps/8 stage-caches.
- The existing llama.cpp passed real GGUF normal generation, reclaim and UNLOAD/DELETE with the feature both on and off.
- Full P4 Rust was 1450 passed/0 failed/7 ignored in each run; the 9 external HF tests are counted separately.
  5 Rust mutations, 3 cache/epoch mutations and 4 existing Qwen mutations were detected in independent copies.
- The exact source/binary/model/host/command/failure/cleanup evidence is in the acceptance report. No remote push was performed.

## Starting new work

1. First audit both HEADs, their dirty state, and the runtime source difference from the acceptance report. The two repositories are separate Git repositories and workspaces.
2. Read [AGENTS](../../../AGENTS.md) and the [folder rules](../../structure/README.md).
   If roles differ, split them into separate folders, and keep per-model computation, batching and state inside Python.
3. To restore the adjacent source, use the build bundle and standalone restore.py from the integration specification.
   Because the dependency is an optional path, the HF source is required even with the feature off. Environments and weights are prepared separately.
4. When re-running verification, supply a new output directory and a new node generation. Do not overwrite a running bundle.
5. The current P4 follow-up is Release A. Completing §0 here does not automatically widen permission for large-model work, remote changes or push.

## Preserved limitations

The RTX4080+3090 BF16 split is a FAIL for exceeding the logits threshold. It was not papered over with the FP32 pass, and the tolerance was not raised.
The [existing model command](../../models/qwen3_5_0_8b/README.md) is standalone v1 and keeps the cumulative active+retired limit.
v2 creates new bounded state only in a drained epoch.
Small-Qwen conformance is not the very large H0–H7 work or performance/SLO. Quantization, physical batching, arbitrary models,
and durable recovery from a whole-host failure are outside the acceptance scope.

The earlier 2026-09-13 “P4 read-only/unconnected” statement described the work scope of that time.
On 2026-09-14 the user approved implementation and follow-up verification in both repositories. Past plans and reports are preserved as history.
Models, environments and detailed logs live in the Git-excluded models/.venv/artifacts/target and are reconstructed from the tracked manifest and lock.
