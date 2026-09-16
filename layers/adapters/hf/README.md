The per-model Hugging Face Transformers adapter for P4: a Rust retained bridge and partial execution in Python.

| Purpose | Document |
| --- | --- |
| Scope and ownership | [Overview](docs/overview.md) |
| Responsibilities and wiring | [Architecture](docs/architecture.md) |
| API | [API](docs/api.md) |
| Running | [Usage](docs/usage.md) |
| Limitations | [Constraints](docs/constraints.md) |
| Implementation layout | [Internals](docs/internals.md) |
| Testing | [Verification](docs/testing.md) |
| Wire, budget, deployment | [Integration contract](docs/integration/README.md) |
| Model | [Qwen3.5-0.8B](docs/models/qwen3_5_0_8b/README.md) |
| Automatic loading plan and measured profiles | [Model planner](docs/models/qwen3_5_0_8b/README.md#automatic-loading-planner) |
| Loading planner verification | [Plan](tests/plans/loading-planner-20260915.md) · [Report](tests/reports/loading-planner/20260915_013600.md) |
| Quantization candidates | [Design](docs/quantization.md) |
| Working rules | [AGENTS](AGENTS.md) |
| Migration, restoration, current evidence | [Migration record](docs/migration/README.md) |
| Migration verification result | [Report](tests/reports/migration/20260914_120000.md) |
| Rust public boundary | [Crate guide](adapter/README.md) |

The repository and the Cargo workspace are P4 alone. The Python import name is `p4hfadapter`, and the agent kind is `hf-transformers`.
The current development order follows the [P4 roadmap](../../../docs/distributed-batching-roadmap.md).
