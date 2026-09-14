> 역사 기록: 독립 저장소 시점의 요구·상태·측정이다. 현재 배치와 사용법은 [HF 안내](../../../README.md)를 따른다. 원본 전체는 이관 시 보존한 Git bundle에 있다.

> 2026-09-14: 사용자 §0 지시로 P4 통합 구현을 진행한다. 이전 미연결/읽기 전용 설명의 현재 상태는 [통합 명세](../../integration/README.md)와 그 수용 보고가 우선한다.

# 아키텍처와 P4 연결

지위: 설계 제안. P4의 현재 관측과 새 구현 계획을 구분합니다.

## 예정 실행 구조

```text
OUTER: 모델·토폴로지·SLO·요청·배포 선택
  -> P4 event transport / broker / node lifecycle
    -> 이 프로젝트의 Rust adapter bridge (RetainedNodeAdapter 구현 예정)
      -> 노드 로컬의 제한된 IPC
        -> Python model worker
          -> 모델별 부분 forward + 요청별 KV/recurrent 상태
            -> Transformers / PyTorch / 양자화 라이브러리 / 장치 커널
```

노드 간 경계 텐서는 adapter-owned payload로 P4 이벤트 전달 경로에 올리는 것을 첫 설계로 둡니다.
Python worker가 임의로 다른 노드에 직접 송신하여 P4 backpressure를 우회하지 않습니다.
큰 payload의 분할·재조립·바이트 예약·버퍼 수명은 adapter codec과 bridge에서 결속합니다.
추후 별도 데이터 전송 경로가 필요하면 같은 소유권과 용량 보존을 검증하는 별도 계약으로 다룹니다.

## 책임

Python 실행기는 선택 모델 전용입니다. 모델마다 서로 다른 구현·상태 구조·경계 tensor·양자화 경로를
가질 수 있으며 공통 base class나 모델 자동 발견을 요구하지 않습니다.
공통으로 지킬 계약은 P4 연결, 이벤트 전달과 실행 identity, 자원·출력 소유권입니다.
Rust bridge 내부 코드의 재사용도 가능하지만 여러 모델을 위한 framework 구축을 선행 조건으로 삼지 않습니다.

| 구성 | 소유 | 경계 밖 |
| --- | --- | --- |
| OUTER | 모델/양자화 조합·배치 위치·요청 목표·배포·운영 정책 | 엔진 상태 직접 조작 |
| P4 공통 코어 | opaque 이벤트·라우팅·노드 수명·범용 전달/용량 소유권 | 모델 레이어·토큰·KV·양자화 형식 해석 |
| Rust bridge와 어댑터 원장 | 요청/실행 식별·예약·배치 발행·정산·출력 승인·P4와 IPC 연결 | PyTorch tensor/private 엔진 타입을 P4에 노출 |
| Python 모델 실행기 | 부분 적재/연산·물리 KV/상태·장치 완료·커널 선택·측정 | 정산 전 OUTER 토큰 직접 출력 |
| 모델별 모듈 | forward 의미·합법 cut·tensor/state schema·특수 연산·샘플링 입력 | 다른 모델의 캐시 구조 추정 |

Python이 실제 토큰과 상태를 계산하고, 어댑터 원장이 결과의 요청 귀속과 외부 출력을 승인합니다.
원장 commit과 effect intent는 결속하되 네트워크/장치 호출 자체를 순수 transaction에 넣지 않습니다.
오류로 실행 여부가 불명확하면 fenced/uncertain으로 남기고 같은 작업을 무조건 재실행하지 않습니다.

## 모델 분할

60층 decoder의 단순 예시는 A=embedding+0..19, B=20..39, C=40..59+norm/lm_head입니다.
이는 특정 모델의 합법 cut이나 균형 배치라는 주장이 아닙니다.
각 단계는 담당 레이어의 가중치와 요청별 상태를 보유하고, 단계 사이에는 모델이 요구하는 경계 텐서를 보냅니다.
꼬리의 다음 토큰은 원장 승인 후 다음 decode 입력으로 돌아갑니다.

순수 dense 모델도 position/RoPE/mask/global layer index/tied weights를 보존해야 합니다.
shared KV·cross-layer state·recurrent state·MoE가 있으면 hidden state 하나만 전달한다고 가정하지 않습니다.
의존 레이어를 같은 단계로 묶거나 추가 경계 상태를 정의하고 불법 cut은 적재 전에 거부합니다.

호스트 사이는 PP(레이어 단위 분할)를 우선 검토합니다. 빠른 동일 호스트 GPU 간 TP는 후속 선택입니다.
TP는 양자화 packing/group 경계와 collective 제약까지 별도 검증합니다. `device_map="auto"`는 다중 호스트 실행기가 아닙니다.

## 배칭

Transformers의 continuous batching·paged KV·chunked prefill은 재사용 후보입니다.
각 Python stage에서 독립적으로 전체 `generate_batch`를 실행하는 구조는 분산 스텝 정합성을 보장하지 않습니다.
전체 pipeline의 요청 멤버십/위치/취소를 한 adapter 스케줄링 계약으로 정하고 stage는 승인된 실행을 소비합니다.
기존 llama.cpp 어댑터의 L0~L5 분리는 설계 참고이며 소스 전체 재사용 승인이 아닙니다.

## 현재 P4 연결 지점과 향후 컴파일

2026-09-13 참고 HEAD는 `d122125bafeaa6d32790761669f1bfa5868d8078`입니다.
`layers/adapters/adapter/src/node_adapter/mod.rs`에 `RetainedNodeAdapter`가 있고,
`entrypoints/agent/src/event_runtime/control.rs`의 create는 현재 `llamacpp`만 생성합니다.
`entrypoints/agent/Cargo.toml`이 concrete adapter를 조립합니다. 동적 Python plugin 자동 발견은 현재 관측된 기능이 아닙니다.

향후 권장 연결은 이 저장소가 독립 Rust bridge crate와 Python worker 패키지를 제공하고,
P4 composition root가 새 adapter kind를 등록해 함께 컴파일하는 방식입니다.
Python 코드는 Rust 바이너리에 자동으로 네이티브 컴파일되는 것이 아니므로 별도 worker 배포/환경 lock이 필요합니다.

| 시점 | 할 일 | P4 변경 |
| --- | --- | --- |
| 초기화 완료 | 문서·별도 Git 준비 | 없음 |
| 독립 구현 | bridge/worker/시험 host를 이 저장소에서 구현; 필요 시 중립 P4 crate를 읽기 전용 참조 | 없음 |
| 명시적 통합 작업 | P4 entrypoint 의존·kind 등록·설정·배포 묶음·중립성 시험 | 별도 사용자 지시 후 |

개발 시 sibling path 의존은 후보입니다. 재현 가능한 배포는 양쪽 revision을 고정한 Git 의존 또는
배포 패키지 방식으로 결정합니다. 한 빌드 안에서 P4 protocol/adapter crate가 경로·Git의 서로 다른 소스로
중복되어 Rust 타입 정체성이 갈라지지 않도록 의존 그래프를 확인합니다.
독립 빌드의 target·lock·환경·산출물은 이 저장소에 둡니다. P4에 workspace member나 submodule을 지금 추가하지 않습니다.
