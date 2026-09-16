> Historical record: requirements, status and measurements from the time of the standalone repository. The current layout and usage follow the [HF guide](../../../README.md). The complete original is in the Git bundle preserved during the migration.

# Requirements and decision record

Status: separates user-confirmed requirements, design proposals from the conversation, and decisions still needed.

## User-confirmed requirements

| ID | Requirement |
| --- | --- |
| U1 | Investigate a distributed inference adapter that supports a specific model's vendor Python/Transformers code on a per-model basis. |
| U2 | Align the distributed nodes with the P4 specification, and design how quantization is prepared and executed. |
| U3 | Create a fully separate project, Git repository and complete set of planning documents in `F:\dev\p4hfadapter`. |
| U4 | State in the documents that `F:\dev\p4` is the reference project. |
| U5 | Do not change the P4 project itself now. Do not remove existing files by moving them either. |
| U6 | Later, integrate so that P4 compiles with this adapter included. |
| U7 | The user opens a new project session to continue. Work must be resumable from the documents alone. |
| U8 | Writing dedicated Python execution code for each model is the core of the project. Neither a general-purpose processor nor a common model interface that accommodates multiple models is required. |
| U9 | If roles differ, split them into folders, even for a single file. Within each per-model division, split every implementation role, tests, fixtures and tools into folders. |
| U10 | Start development in the standalone repository. P4 changes and integration keep their existing separate work boundary. |
| U11 | The first model is Qwen3.5-0.8B; add scripts that handle it according to a node partition plan that specifies various scenarios. |

U8 was confirmed by the user's follow-up instruction on the 2026-09-13 development plan.
Per-model loader, forward, cache/state, partitioning and quantization code is implemented directly, without up-front
abstraction for supporting other models. The P4 connection, event and resource ownership contracts are maintained independently of this principle.

## Design proposals — verify and confirm during implementation

| ID | Proposal | Reason |
| --- | --- | --- |
| D1 | P4 neutral boundary + standalone Rust bridge + Python worker | Connects the existing event runtime with per-model Python computation |
| D2 | Cross-host contiguous-layer PP first | Each stage keeps its assigned weights/state, and only the boundary is transferred |
| D3 | Initial 4-bit weight candidates; FP16/BF16 candidates for boundary/KV | Addresses the capacity problem first and isolates communication/KV quantization error |
| D4 | Investigate published quantizations first; load-time or offline PTQ if needed | Choose on reuse, preparation cost and quality using real artifacts |
| D5 | Pin a per-model quantized artifact and deploy only per-stage tensors | Separates node-cut changes from quantization preparation |
| D6 | Stage-internal quantization formats may differ, but boundary dtype/schema is agreed | Uses heterogeneous kernels; mixed quality is verified end to end |
| D7 | Record the quantization format and the actual compressed kernel together in the manifest | Detects first-run dequantization and unexpected memory growth |
| D8 | Separate baselines: original → same quantization → partitioned → multi-host | Distinguishes quantization error from distribution errors |

D1~D8 are this document's recommendations; they do not mean that implementation or performance has been proven, or that the user has finished the detailed technical choices.

## Undecided

- Follow-up feature scope and acceptance goals. The model, revision and library baseline for the first script are pinned in the [Qwen contract](../../models/qwen3_5_0_8b/README.md).
- The physical fleet to use, GPU/OS/network, model access rights and available memory.
- Published quantized artifact or own conversion; bits/group/symmetry/excluded modules/kernels.
- Calibration dataset, separate evaluation inputs, allowed logits/quality error, TTFT/ITL/useful TPS and resource criteria.
- Context, concurrent requests, maximum output, batch/flight budget and sampling settings.
- Python/PyTorch/Transformers/quantization package and Rust dependency versions, and the lock method.
- IPC transport, adapter kind/content-type/schema, standalone test host setup.
- The future P4 dependency reference/build/deploy method, repository remote, license and distribution policy.

## Assumptions not adopted

We do not assume that `device_map="auto"` alone completes P4 multi-host execution, that calling the full `generate()` on each node
yields partitioned execution, or that the same 4-bit format gives the same kernels and quality on every device.
Nor do we exaggerate the difficulties of quantization as grounds for denying that Python execution is feasible.
Specific model/device combinations are narrowed down and solved with real code, artifacts and measurements.
