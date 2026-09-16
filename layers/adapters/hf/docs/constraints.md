# Limitations

- One bridge has one model command. Physical batching, quantization and support for arbitrary models are not implemented.
- The existing logits FAIL for the RTX4080+3090 BF16 split stands. The FP32 pass does not replace it.
- The epoch changes only in a drained state. A stale generation/epoch must not modify new state.
- Durable recovery from a whole-host failure, and authentication, are outside the scope of this in-memory contract.
- Small-Qwen conformance and the directory-migration verification are not acceptance of the very large H0–H7 models or of performance/SLO.
- The Python import name `p4hfadapter` is kept, but no standalone repository or external path dependency is used.
- The automatic planner currently allows only FP32 on CPU/CUDA for the pinned Qwen3.5-0.8B revision. Manual execution of the BF16 experimental plan is a separate matter.
- Stage profiles are observations from real loader/cache runs. The maximum observed on synthetic inputs is not an absolute memory ceiling for every input.
  A caller reserve and a re-check of occupancy right before LOAD are required. IPC/network cost and contention from concurrent GPU execution are not yet included in the cost.

Detailed budget and failure semantics follow the [integration contract](integration/README.md); unverified quantization candidates follow the [quantization design](quantization.md).
