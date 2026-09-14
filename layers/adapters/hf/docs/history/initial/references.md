> 역사 기록: 독립 저장소 시점의 요구·상태·측정이다. 현재 배치와 사용법은 [HF 안내](../../../README.md)를 따른다. 원본 전체는 이관 시 보존한 Git bundle에 있다.

# 참고 프로젝트와 공식 자료

확인일: 2026-09-13. 외부 문서는 계속 변하므로 구현 시 버전/commit을 고정하고 다시 확인합니다.
대화에서 실제 읽은 공식 문서와 로컬 코드의 근거입니다. 링크의 기능 설명은 본 프로젝트의 구현 증거가 아닙니다.

## P4 — 읽기 전용 참고 프로젝트

위치: `F:\dev\p4`.
코드 참고 HEAD: `d122125bafeaa6d32790761669f1bfa5868d8078`.
작업 트리는 dirty였으므로 HEAD와 working copy 문서를 구별합니다.
초기화 중 P4 HEAD는 `484b856ee7e53aea5b850b654c45da53cb0724a6`으로 이동했습니다.
이 문서의 코드 근거는 최초 기준이며, 동시 변경 관측은 초기화 기록이 소유합니다.
P4의 이후 변경을 이 프로젝트에 자동 반영하거나 P4 파일을 수정하지 않습니다.

| 자료 | 경로 | 참고할 내용 |
| --- | --- | --- |
| 진입점 | [README](../../../../../../README.md) | 문서 색인과 실행 경로 구분 |
| 현재 상태 | [로드맵](../../../../../../docs/distributed-batching-roadmap.md) | 최신 상태/실기 제한; P4 개발 재개 지시로 읽지 않음 |
| 검증 규약 | [검증](../../../../../../docs/distributed-batching-verification.md) | 다중 컴퓨터·정상 응답·품질/성능·실패 증거 |
| 격리 계약 | [계층](../../../../../../docs/layer-isolation-contract.md) | core/adapter/native/backend 책임 |
| 문서 소유 | [문서 안내도](../../../../../../docs/document-map.md) | 계약과 역사 자료 구분 |
| 이벤트 | [event protocol](../../../../../../docs/event-protocol-v2.md) | opaque envelope/payload·load/session·backpressure |
| 배치 | [adapter batching](../../../../../../docs/adapter-batching-layers.md) | 원장/정산/발행/상태 권한 참고 |
| 현행 trait | [node_adapter](../../../../adapter/src/node_adapter/mod.rs) | RetainedNodeAdapter와 ownership 반환 |
| event node | [event_node](../../../../../agent/src/event_node/mod.rs) | 실제 retained 소비 경로 |
| event broker | [event_broker](../../../../../agent/src/event_broker/mod.rs) | 중립 라우팅/전달 경계 |
| 노드 등록 | [control.rs](../../../../../../entrypoints/agent/src/event_runtime/control.rs) | create에서 현재 llamacpp만 지원 |
| 조립 의존 | [Cargo.toml](../../../../../../entrypoints/agent/Cargo.toml) | 향후 concrete adapter 등록 위치 |
| 중립 crate | [adapter Cargo](../../../../adapter/Cargo.toml) | 별도 bridge가 참조할 경계 |

상대 링크는 두 폴더가 `F:\dev` 아래의 sibling일 때 동작합니다. 다른 컴퓨터에서는 경로를 맞추고
해당 P4 revision을 확보합니다. 이 저장소에 P4 소스나 문서의 수정 가능한 복제본을 vendoring하지 않았습니다.

## Transformers/PyTorch 생태계

| ID | 공식 자료 | 대화에서 확인한 용도/제약 |
| --- | --- | --- |
| R1 | [Continuous batching](https://huggingface.co/docs/transformers/main/en/continuous_batching) | generate_batch·manager·paged KV·chunked prefill·TP; P4 PP 자동 통합은 아님 |
| R2 | [Tensor parallelism](https://huggingface.co/docs/transformers/main/en/perf_infer_gpu_multi) | 모델별 plan과 레이어마다 통신; 빠른 연결 필요 |
| R3 | [Accelerate big model inference](https://huggingface.co/docs/accelerate/usage_guides/big_modeling) | device_map/offload; 다중 호스트 PP 실행기와 구분 |
| R4 | [Accelerate distributed inference](https://huggingface.co/docs/accelerate/usage_guides/distributed_inference) | 실험적 PyTorch 기반 PP 참고 |
| Q1 | [bitsandbytes](https://huggingface.co/docs/transformers/quantization/bitsandbytes) | Linear 4/8bit 교체·compute dtype·장치/오프로딩 제약 |
| Q2 | [HQQ](https://huggingface.co/docs/transformers/quantization/hqq) | 보정 데이터 없는 양자화·모듈별 설정 |
| Q3 | [GPTQ](https://huggingface.co/docs/transformers/quantization/gptq) | GPTQModel 보정·저장·압축 커널·Marlin 제약 |
| Q4 | [AWQ](https://huggingface.co/docs/transformers/quantization/awq) | 보정 기반 후보; 유지되는 패키지/버전 재확인 |
| Q5 | [Metal](https://huggingface.co/docs/transformers/quantization/metal) | MPS 2/4/8bit affine 커널·non-MPS dequantize |
| Q6 | [compressed-tensors](https://huggingface.co/docs/transformers/quantization/compressed_tensors) | 저장 형식·첫 forward 복원·지원 FP8 최적화 모드 구분 |
| Q7 | [Fine-grained FP8](https://huggingface.co/docs/transformers/quantization/finegrained_fp8) | weight/activation FP8·장치/커널 조건 |
| Q8 | [GGUF](https://huggingface.co/docs/transformers/main/quantization/gguf) | 제한된 압축 경로와 legacy 복원; 버전/모델별 확인 |
| Q9 | [Quantization overview](https://huggingface.co/docs/transformers/quantization/overview) | 장치·비트·라이브러리 선택표 재확인용 |

## 재검증 규칙

Qwen3.5-0.8B의 고정 checkpoint·실제 Transformers 소스·환경 lock은 [모델 문서](../../models/qwen3_5_0_8b/README.md)를 참조합니다.

공식 문서의 main은 배포된 패키지와 다를 수 있습니다. 구현 시 정확한 버전과 소스 commit을 기록합니다.
model card의 예제는 실제 선택 모델 revision에서 확인하고 remote model code도 같은 방식으로 고정합니다.
라이브러리 지원표는 조사 근거이며 모델별 커널 실행·압축 유지·성능의 증거는 실제 시험에서 얻습니다.
