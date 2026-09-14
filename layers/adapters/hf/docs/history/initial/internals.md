> 역사 기록: 독립 저장소 시점의 요구·상태·측정이다. 현재 배치와 사용법은 [HF 안내](../../../README.md)를 따른다. 원본 전체는 이관 시 보존한 Git bundle에 있다.

# 내부 구현 계획

지위: 모델·bridge 구현 계획. 구현된 IPC 폴더와 역할별 상세 소유권은 [폴더 규칙](../../structure/README.md)을 따릅니다.
Qwen 모델의 실제 폴더는 [모델 계약](../../models/qwen3_5_0_8b/README.md)에 있습니다. 아래 표의 bridge는 예정입니다.

| 예정 위치 | 역할 |
| --- | --- |
| `crates/p4-hf-adapter/src/<role>/` | retained 전달·원장·IPC 등 bridge 역할별 폴더 |
| `python/p4hfadapter/models/<model>/<role>/` | configuration/loading/forward/state/quantization/boundary/reference별 폴더 |
| `python/p4hfadapter/workers/<model>/<role>/` | lifecycle/queue/completion 등 해당 worker 역할별 폴더 |
| `tests/` | model parity, bridge 소비 경로, 오류/변이, 다중 호스트 검증 |
| `manifests/` | 검증된 실행 조합의 명세와 schema |

`<model>`은 선택 모델의 구현 식별자이며 family 전체 지원을 약속하지 않습니다.
첫 모델에 필요한 코드를 해당 역할 폴더에 직접 작성합니다. 범용 model base class, registry,
자동 layer 탐색/분할, 공통 cache abstraction, 범용 quantization backend를 먼저 만들지 않습니다.
후속 모델도 별도 Python 구현으로 추가할 수 있습니다. 둘 이상의 실제 구현에서 공통성이 확인될 때만
필요한 코드를 추출하며, 코드 재사용을 위해 모델별 연산 의미를 바꾸지 않습니다.

## 모델별 첫 감사

제조사의 실제 `forward`와 generation 준비를 따라 입력 token/embedding → position/RoPE/mask →
레이어 → 최종 norm/lm_head → logits → sampling 순서로 소유 연산을 표기합니다.
전체 `generate()`를 각 노드에 호출하는 방식으로 분할하지 않습니다.

- 전체 layer index와 stage-local index를 분리하고 cache 객체의 index 접근을 점검합니다.
- `past_key_values`가 전체 레이어 수를 전제하는지 확인합니다. 빈 자리 배열을 넣는 것만으로 안전하다고 보지 않습니다.
- tied embedding/lm_head, shared KV, sliding window, recurrent cache, MoE routing/experts를 찾습니다.
- 요청마다 position/valid length/attention mask를 분리하고 padding을 실제 토큰 수로 세지 않습니다.
- 캐시 mutation, 장치 stream 완료, 취소 뒤 재사용 안전성을 실제 호출 순서에서 확인합니다.

## 적재와 양자화

meta 초기화와 선택 tensor 읽기 등을 검토해 비담당 레이어의 가중치/상태 할당을 피합니다.
표준 Linear 교체와 커스텀 모듈 변환을 분리하고 양자화 보조 tensor가 누락되면 LOAD를 거부합니다.
라이브러리의 전체 모델 로더를 일부 모델 로더처럼 사용할 수 있는지는 실제 코드로 확인합니다.
양자화된 nn.Module을 역양자화하거나 일반 Linear로 바꿔서 부분 실행 오류를 숨기지 않습니다.

## 배치와 물리 상태

adapter 스케줄러가 승인한 immutable 요청/행 membership을 worker가 소비합니다.
초기에는 단순하고 유한한 스텝 실행으로 정확성을 고정한 뒤 연속 수용·chunked prefill을 연결합니다.
이 단순 단계는 성능/제품 완료가 아닙니다. `generate_batch`의 scheduler/cache를 재사용할 때는
누가 admission·membership·취소를 결정하는지 [아키텍처](architecture.md)의 단일 계약에 맞춥니다.

기본 worker는 하나의 물리 state owner와 직렬 실행으로 시작하는 제안입니다.
병렬 stream/sampler와 동일 GPU 다중 stage는 안전성·자원 예산·실제 수익을 확인한 뒤 활성화합니다.
카드당 한 노드를 일반 제약으로 만들지는 않습니다.

## IPC와 오류

로컬 IPC의 [blocking binary framing](../../transport/framing/README.md)을 먼저 구현했습니다.
현재 실측 환경은 Windows이며, worker 실행 방식·deadline·종료 제어는 아직 미정입니다.
요청 큐·완료 큐·serialized bytes·GPU staging buffer의 상한을 함께 정의합니다.
Python stdout 로그와 명령/결과 frame을 섞지 않습니다.
worker crash·IPC timeout·부분 tensor 수신·결과 전달 실패에서 원장/버퍼 소유자를 남깁니다.
worker가 죽었다는 사실만으로 모든 요청이 미실행이거나 정상 해제됐다고 판정하지 않습니다.

## 관측

load peak/resident, 실제 kernel/fallback, 각 단계 queue/transfer/compute/settle 시간,
실제 토큰 길이·KV/state byte·retained byte·요청별 terminal/release를 기록합니다.
GPU 실행 시간을 측정할 때 비동기 enqueue 시간과 device 완료 시간을 구별합니다.
측정 때문에 매 스텝 불필요한 전역 synchronize를 넣으면 측정용 arm임을 명시합니다.
