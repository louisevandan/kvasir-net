P4(Proxy Pipeline Parallel Protocol)는 외부 ingress, 논리 컨트롤러, agent-owned NodeSlot, 반복 가능한 모델 binding, concrete adapter를 분리한 실행 제어 프로토콜이다.

전체 경계와 현재 보장은 [P4_IMPLEMENTATION_SPEC.md](P4_IMPLEMENTATION_SPEC.md)에 고정한다.

| Goal | File |
| --- | --- |
| Understand purpose | [docs/overview.md](docs/overview.md) |
| Architecture (router) | [docs/architecture.md](docs/architecture.md) |
| API reference | [docs/api.md](docs/api.md) |
| Model-load options and measured batching | [docs/model-load.md](docs/model-load.md) |
| Usage | [docs/usage.md](docs/usage.md) |
| Constraints & blast radius | [docs/constraints.md](docs/constraints.md) |
| Internals & decisions | [docs/internals.md](docs/internals.md) |
| Testing | [docs/testing.md](docs/testing.md) |
| Every P4 request/response pair | [docs/message-pairs.md](docs/message-pairs.md) |
| Agent task envelope and workers | [docs/task-runtime.md](docs/task-runtime.md) |
| Runtime evidence | [docs/runtime-evidence.md](docs/runtime-evidence.md) |
| 2-node 500-token report (EN) | [P4_2NODE_DISTRIBUTED_INFERENCE_REPORT_2026-08-08.md](../../P4_2NODE_DISTRIBUTED_INFERENCE_REPORT_2026-08-08.md) |
| 2-node 500-token report (KO) | [P4_2NODE_DISTRIBUTED_INFERENCE_REPORT_2026-08-08.ko.md](../../P4_2NODE_DISTRIBUTED_INFERENCE_REPORT_2026-08-08.ko.md) |
