# 구현 배치

| 경로 | 소유 |
| --- | --- |
| `adapter/src/` | construction·retained·lifecycle·ipc·process Rust 역할 |
| `python/p4hfadapter/models/qwen3_5_0_8b/` | 제조사 기준·부분 적재/forward·state·모델 스케줄링 |
| `python/p4hfadapter/transport/framing/` | 제한된 frame 직렬화·송수신 |
| `python/p4hfadapter/integration/` | HF controller의 P4 packet/transport |
| `scripts/deployment/` | worker bundle와 단일 P4 source archive |
| `scripts/models/` | 모델별 CLI·checkpoint 준비 |
| `scripts/testing/`, `scripts/verification/` | 시험 실행과 독립 변이/실기 검증 |
| `environments/`, `manifests/`, `plans/`, `scenarios/` | 고정 환경·artifact 정체성·분할·입력 |
| `tests/`, `adapter/tests/` | 시험·fixture·검증 의도·실행 보고 |

구체 역할은 [폴더 계약](structure/README.md)을 따른다.
