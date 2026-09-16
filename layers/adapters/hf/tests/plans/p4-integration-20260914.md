# P4 HF integration acceptance test plan

Created: 2026-09-14 KST. The goal is to verify HF-0/1/2/3 of §0 of the P4 development plan and the existing llama.cpp behavior.

## Environment and prerequisites

Requires adjacent clean P4/HF commits on Windows, Rust/Cargo, the Qwen-pinned CPython 3.13.15/torch 2.14.0+cu130/transformers 5.17.0,
the pinned checkpoint, and two physical hosts. The existing native llama server and a working GGUF are recorded separately.
Read the [execution specification](../../docs/integration/README.md) and the P4 verification conventions first.

## Order and expected values

1. Check both HEADs, locks and dirty state, exactly one each of p4-adapter/protocol in Cargo metadata, and that the kind matches between the real factory and INSPECT.
2. Real retained+Python pipe tests of the HF crate: check original-claim rejection, held output, wake, epoch, partial/death/timeout/identity,
   and the façade and abort after a failure. Earlier inputs, reservations and effects are preserved, and a normal request must succeed after a rejection.
3. Recompile guard/identity/reservation mutations in an independent copy. Record the baseline pass, the failure of the relevant test under each mutation,
   the source/binary hashes and the distinct targets.
4. Re-run the existing v1 8 combinations / 47 logits steps. Apply the same criteria to the event controller, and also compare every cache element.
5. Process 8 requests × 3 epochs in the same load, and reject previous-epoch step/release/cancel. Record both the cancelled requests and the normal responses and releases.
6. Confirm the stage route, cancel/re-request and UNLOAD/DELETE across two real hosts. Leave host/PID/device/transport traces.
7. With the same agent binary, check Python A→B replacement, rejection of an incompatible bundle, and the existing llama.cpp normal response/release/UNLOAD/DELETE.
8. Run creation/INSPECT with the feature off, and llama.cpp on hardware. Record both full Rust test runs, the Python tests and the final docs-lint exit.
9. Restore the clean-commit source bundle into a directory separate from the original, and run a --locked build.

Required cache/epoch regression: `python scripts/verification/qwen_cache/run.py` (4 tests).
Independent-copy mutations: `python scripts/verification/qwen_cache/run.py --mutations artifacts/integration/cache-epoch-mutations`.
Confirm every cache element and member/shape/dtype, preservation of effects on a live-epoch rejection, re-acceptance of the same ID after drain, and rejection of the previous epoch.
Cargo graph: `python scripts/verification/package_graph/run.py --output artifacts/integration/package-graph`.
It inspects the real root metadata, and the graph gate must reject an independent Cargo fixture that adds a protocol from a separate source.
2-host response disconnection: give `scripts/verification/distributed_recovery/run.py` the same plan/nodes/deployments/bundle/agent-binary as the matrix,
and a new output. Drop the TCP connection after the response length prefix, and check result not acknowledged → explicit abort/delete → reload on the same agent.
After the previous LOAD command is rejected, require a normal short response, a full cache comparison and UNLOAD/DELETE. Keep this distinct from recovery from a failure of the host itself.

## Logs and verdict

For each artifact, preserve source/hash, model/plan/scenario, PID/host, stdout/stderr, the first error and cleanup errors,
logits/greedy/cache comparisons, per-request responses, stop reasons, release chains and final state.
Do not make a run pass by changing the criteria. Keep excluded/not-run/ignored/failed items distinct from overall GREEN, and leave items that cannot be reached externally as BLOCKED.
Browser tests are outside this CLI scope.
