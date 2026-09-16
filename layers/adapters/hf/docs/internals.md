# Implementation layout

| Path | Owns |
| --- | --- |
| `adapter/src/` | Rust roles: construction, retained, lifecycle, ipc, process |
| `python/p4hfadapter/models/qwen3_5_0_8b/` | Vendor reference, partial load/forward, state, model scheduling |
| `python/p4hfadapter/transport/framing/` | Bounded frame serialization, send and receive |
| `python/p4hfadapter/integration/` | P4 packet/transport for the HF controller |
| `scripts/deployment/` | Worker bundle and the single P4 source archive |
| `scripts/models/` | Per-model CLI and checkpoint preparation |
| `scripts/testing/`, `scripts/verification/` | Test execution and independent mutation/real-hardware verification |
| `environments/`, `manifests/`, `plans/`, `scenarios/` | Pinned environments, artifact identity, partitioning, inputs |
| `tests/`, `adapter/tests/` | Tests, fixtures, verification intent, run reports |

Specific roles follow the [folder contract](structure/README.md).
