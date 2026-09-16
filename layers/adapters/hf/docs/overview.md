# Purpose and ownership

In P4, `layers/adapters/hf/` owns both the Rust bridge and the per-model Python.
The current concrete model is Qwen3.5-0.8B. Per-model partial loading, forward and state follow the vendor's Transformers implementation.
There is one repository, one Cargo workspace and one shipped source commit: P4. A separate HF checkout is not needed.
The current development order follows the [P4 roadmap](../../../../docs/distributed-batching-roadmap.md); migration status follows the [migration record](migration/README.md).
