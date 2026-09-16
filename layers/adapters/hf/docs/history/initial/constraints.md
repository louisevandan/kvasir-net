> Historical record: requirements, status and measurements from the time of the standalone repository. The current layout and usage follow the [HF guide](../../../README.md). The complete original is in the Git bundle preserved during the migration.

> 2026-09-14: P4 integration is being implemented under the user's §0 instruction. For the current status of the earlier unconnected/read-only descriptions, the [integration specification](../../integration/README.md) and its acceptance report take precedence.

# Constraints and limits of the evidence

Status: the current limitations and the boundaries that future implementation must respect.

## Current limitations

- Local IPC and the Qwen3.5-0.8B text partition script are implemented. The supported scope follows the [model contract](../../models/qwen3_5_0_8b/README.md).
- The model, revision, and dense execution versions for Windows/local GPU/CPU are pinned. Quantization, a multi-physical-host fleet and SLO acceptance are unfinished.
- P4 is a read-only reference, and its existing dirty working tree is left as is.
- The existence of a Python model example does not mean that every attention/quantization backend for that model is supported.
- The current P4 entrypoint cannot create this adapter. Sending the planned name is rejected until it is registered.

## Implementation constraints

- If roles differ, even a single file goes in a separate folder. Detailed ownership follows the [folder rules](../../structure/README.md).
- Investigate per-model shared state, legal cuts, sequence length and batch constraints, and explicitly reject anything unsupported.
- Do not imitate distributed loading by keeping the weights/KV of unassigned layers resident on every node.
- Do not assume that every quantization format runs at the same speed on every GPU.
- Choose formats such as FP8/NVFP4 by actual device/kernel conditions, and distinguish dtype support from hardware acceleration.
- Count long-context KV/state, calibration activations, loading peak, kernel workspace and transport buffers separately from the weight budget.
- Python/GIL/IPC/copy/synchronization costs are to be measured. Do not conclude performance superiority or inferiority from the use of Python alone.
- Do not use transport ACK, buffer return, GPU completion, KV stop and request release as evidence for one another.
- Reject invalid identity, shape, quantization metadata and over-budget requests before any native effect.

## Operations and support scope

The first real tests are limited to the specified trusted network and the devices the user allowed.
Authentication, access control and durable settlement after restart are separate features, and P4's existing logical endpoints are not treated as authenticated identities.
Remote execution and deployment permission is distinct from documentation and local development permission.
Redistribution terms for model/kernel code and weights are checked against the selected artifacts. No license is assigned to the current project on our own initiative.

## Verdict

Document checks do not certify semantic correctness or real model behavior.
A single host, a small model or a mock is local evidence and does not replace the final multi-physical-host acceptance.
Before real measurements, do not claim performance superiority or a full replacement of llama.cpp.
If required devices or permissions are missing, finish the local work that is possible and record only the affected real-hardware stage as BLOCKED.
