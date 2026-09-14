> 역사 기록: 독립 저장소 시점의 요구·상태·측정이다. 현재 배치와 사용법은 [HF 안내](../../../README.md)를 따른다. 원본 전체는 이관 시 보존한 Git bundle에 있다.

> 2026-09-14: 사용자 §0 지시로 P4 통합 구현을 진행한다. 이전 미연결/읽기 전용 설명의 현재 상태는 [통합 명세](../../integration/README.md)와 그 수용 보고가 우선한다.

# 예정 API와 이벤트 계약

지위: 설계 초안. 아래 연산/필드/상태 이름은 현재 실행 가능한 API나 확정 wire schema가 아닙니다.

구현된 하위 전송 계약은 [로컬 IPC framing](../../transport/framing/README.md)입니다.
독립 Qwen worker의 step/release/shutdown과 계획 형식은 [Qwen 스크립트](../../models/qwen3_5_0_8b/README.md)에 있습니다.
아래 P4 bridge의 의미 연산 전체와 제품용 payload schema는 여전히 예정입니다.

이 계약은 bridge와 선택 모델 전용 worker 사이의 실행·소유권 계약입니다.
모든 모델이 구현해야 하는 범용 Python API를 뜻하지 않습니다. payload와 boundary/state schema는
선택 모델에 맞춰 정하고, capabilities는 그 구현의 지원 범위를 명시하는 용도로만 사용합니다.

## 외부 P4 경계

현재 참조 경계는 `p4_adapter::node_adapter::RetainedNodeAdapter`입니다.
주요 표면은 `try_offer_retained`, `peek_retained_completion`, `try_take_retained_matching`,
`poll_take_retained`, `snapshot`, `completion_storage_snapshot`입니다.
정확한 signature는 고정한 P4 revision에서 재확인하며 Rust trait를 Python에 그대로 노출하지 않습니다.

Full은 원본 이벤트·버퍼/예약 소유권을 반환하는 일시적 수용 불가입니다. Closed와 구분합니다.
수용 성공은 실행 완료가 아니며 완료는 별도 이벤트로 나갑니다.
어댑터 kind 후보는 `hf-transformers`이고 content-type 이름·버전은 구현 단계에서 확정합니다.
새 필드와 텐서 형식은 concrete adapter payload에 두며 P4 공용 envelope를 모델별로 확장하지 않습니다.

## bridge ↔ worker 의미 연산

| 연산 | 입력 | 결과/완료 조건 |
| --- | --- | --- |
| Inspect/Capabilities | worker/runtime identity, 모델 recipe | 실제 지원 연산·dtype·커널·state/cut 제약 |
| Load | model/artifact manifest, stage 범위, device, 예산, generation | 담당 가중치 적재·커널 warmup·실측 메모리 또는 명시 실패 |
| BindSession | load identity, session incarnation, 필요한 경로/경계 schema | 상태/경계 합의; 적재와 경로 바인딩을 분리 |
| ExecuteStep | issue ID, immutable membership, 요청별 위치/길이, tensor bundle | 실제 장치 완료와 상태 진척·경계 tensor/꼬리 결과 |
| Cancel/Quiesce | 요청 incarnation, cutoff issue | 실행 중지/종료와 상태 정지점의 증거 |
| Release | 동일 identity, 정산된 상태 범위 | 실제 KV/state·예약 반환 확인 |
| Unload | load generation, 세션/비행 정리 조건 | worker 자원 회수; 불확실/미정리이면 성공 금지 |

Persist/Restore와 speculative 연산은 첫 범위 밖입니다. capabilities에 없는 기능은 실행 전에 거부합니다.
timeout은 GPU kernel의 즉시 interrupt를 보장하지 않습니다. 취소 수신과 물리 정지·반환을 분리합니다.

## 필수 identity

모든 실행·완료·반환에 model artifact identity, load generation, stage ID, session incarnation,
request ID, issue/operation ID를 결속합니다. 요청 ID·KV sequence ID·operation ID의 의미를 구분합니다.
이전 generation의 늦은 completion/RELEASE가 새 요청의 상태를 변경하지 못해야 합니다.
같은 identity의 재전달은 정의한 replay 결과로 처리하고 네트워크 재시도를 GPU 재실행으로 바로 바꾸지 않습니다.

## 경계 tensor bundle

버전, 모델 boundary schema, tensor 이름/역할, dtype, shape, layout, byte length,
요청/행 mapping, position/mask 관련 metadata, issue identity를 포함할 예정입니다.
요소 수 곱·byte 범위·상한·unknown dtype/schema를 검증합니다. pickle이나 raw GPU pointer는 wire 형식으로 사용하지 않습니다.
프로세스 로컬 handle은 소유자·수명·release를 가진 IPC 내부 값이며 원격 주소처럼 전달하지 않습니다.
전송 버퍼를 ack로 반환하는 것과 KV/state가 정지·해제된 것은 서로 다른 증거입니다.

## 예정 상태 전이

```text
load: unloaded -> loading -> ready -> draining -> unloaded
request: queued -> admitted -> executing -> settled -> releasing -> released
error: 실행 여부 불명 -> uncertain/fenced -> reconcile 또는 명시 실패 정리
```

실제 원장은 issue별 accepted/settled와 effect pending/sent/uncertain을 구분해야 합니다.
꼬리가 token을 계산해도 정산 전에는 OUTER로 내보내지 않습니다.
부분 stage 실패, stale receipt, 잘못된 membership은 승인된 출력/예약/state를 손상시키지 않아야 합니다.
프로세스 재시작 뒤 durable exactly-once는 이 상태 그림만으로 제공되지 않습니다.

## 모델/실행 manifest 필수 항목

- model ID/revision, 원본/양자화 tensor hash, tokenizer/chat-template hash, 제조사 code revision.
- Python/Transformers/PyTorch/양자화 라이브러리/커널 및 P4/bridge revision.
- 모듈별 bit/group/대칭성/scale/packing/제외 규칙, 보정 dataset digest, 실제 실행 dtype.
- stage 레이어 범위·공유 모듈·boundary/state schema·장치·physical host·memory pool.
- context·요청 수·prefill token budget·decode/flight 상한·KV/state/workspace/전송 예산.
- workload·샘플링·stop/EOS·품질/SLO 허용값·지원 capability·fallback 정책.

manifest 문법/스키마와 명령행 옵션은 아직 만들지 않았습니다. 위 필수 의미를 보존하는 JSON/TOML 등으로
첫 구현 때 확정하고 validator와 반례를 함께 작성합니다.
