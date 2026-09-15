# P4 제품 릴리즈 개발 계획과 투자 근거

2026-09-13 조사. [외부 구조 분석 덱](https://docs.google.com/presentation/d/1Ycq1kuAR1BX1zO6kJHvF18WaRLsB318hSVwPx93ZpPc/edit)의 15장, 특히 10–15장의 시사점을 최신 공식 모델·엔진 자료 및 실제 P4 코드/실기 기록과 대조했다. **이전 R1–R8을 예정 릴리즈 목록으로 삼는 안은 철회한다.** 제품으로 완결될 수 있다는 사실만으로 개발 투자 가치가 생기지는 않는다.

이 문서는 투자 판단·제품 범위·구현 범위·인수인계를 소유하는 **단일 개발 계획**이다. 별도 개별 버전 계획 파일이나 이전 대화를 요구하지 않는다. 출시 우선순위는 [로드맵](distributed-batching-roadmap.md#current-status), 시험·성능 판정은 [검증 규약](distributed-batching-verification.md), 수정 소유 층은 [격리 계약](layer-isolation-contract.md)을 따른다. 현재는 계획 확정이며 제품 개발·원격 재시험·배포는 실행하지 않았다.

**2026-09-14 우선순위 변경: 첫 개발 대상은 [외부 HF 어댑터 수용](#hf-integration)이다.** 그 통합 뒤 [Release A](#release-a)의 장문·취소·회수·다음 웨이브 작업을 진행한다. 2026-09-15 사용자 지시로 기준 모델을 Qwen3.5-122B-A10B로 변경했다. [배치 G1–G6의 채택 범위](#batch-decisions), [DFlash/DSpark를 포함한 가속 판단](#speculation-decision), [새 세션 시작 절차](#fresh-session)를 함께 읽는다. A/S/B/C는 제품 계약 식별자이며 과거 R1–R8 단계 번호나 실제 Git tag가 아니다.

<a id="hf-integration"></a>

> HF §0의 독립 저장소 배치는 2026-09-14 사용자 지시로 폐기했다. 현재 소유·빌드·이관은 [HF 안내](hf-integration.md)를 따른다. 아래 착수 감사는 당시 기록이며 Release A 이후 기능 계획은 유지한다.

## 0. 최우선 작업 — p4hfadapter를 실제 P4 event 경로에 수용

2026-09-14 사용자 지시로 기존 A/S/B/C보다 먼저 편성한다. **모델별 Python 개발과 Rust 구상 어댑터를
`layers/adapters/hf`가 소유하고, P4는 외부 crate를 정적으로 연결해 생성·발견·수명 계약을 소비한다.**
이 작업의 납품물은 외부 HF 실행 구성을 사용할 수 있는 P4 통합 배포물이다. 등록만 하고 실행은 A 이후로
미루는 작업이 아니다. 실제 LOAD→요청→취소/해제→UNLOAD→DELETE·재생성을 함께 검증한다.
Qwen3.5-0.8B는 이 연결의 conformance 모델이며 초대형 모델 성능/최종 H0–H7 승격을 대체하지 않는다.

2026-09-14 HF-0~3 수용 완료: [소비 구성](hf-integration.md)과 [수용 보고](../layers/adapters/hf/tests/reports/p4-integration/20260914_023000.md)에 실제 두 호스트·취소/재수용·Python 교체·재현 빌드·기존 llama.cpp on/off 검증을 기록했다. 2026-09-15 후속 전송 R1–R9와 최종 소스의 양쪽 실제 어댑터 재검증은 [Release A 수용 보고](../tests/reports/release-a/20260915_183158.md)를 따른다. 아래 부족분/감사 설명은 착수 시점 상태이며 Release A의 다음 단계는 Qwen122B A-PLAN이다.

### 0.1 현재 양쪽 코드와 투자 이유

- P4 감사 HEAD: `6bd01d7e12f2711cb27e93b3b45533703e3caf9b`, 시작 dirty 없음. 공통 경계는
  [RetainedNodeAdapter](../layers/adapters/adapter/src/node_adapter/mod.rs), 실제 조립은
  [p4-agent Cargo](../entrypoints/agent/Cargo.toml)와 [event create/remove](../entrypoints/agent/src/event_runtime/control.rs)다.
  구 service `adapters::registry()`를 연결 대상으로 삼지 않는다.
- HF 감사 HEAD: `df4f81b7c774e60cba78d71ad868c515e619cb15`, 시작 dirty 없음.
  [HF 인수인계](../layers/adapters/hf/docs/history/initial/HANDOFF.md), [Qwen 실행 보고](../layers/adapters/hf/tests/reports/qwen3_5_0_8b/20260913_220709.md)를
  현재 Python 코드와 대조했다. **Rust bridge crate는 아직 없다.** Python framing과 Qwen 전용 loader/forward/state/worker,
  로컬 controller가 있다. 보고된 8개 조합·47스텝 비교는 같은 물리 컴퓨터의 기록이며 이번에 모델을 재실행한 결과가 아니다.
- Qwen3.5-0.8B revision `2fc06364715b967f1860aea9cf38778875588b17`, dense FP32 분할을 최초 연결 대상으로 삼는다.
  RTX 4080+3090 BF16 분할의 logits 기준 초과는 미해결로 보존한다. 양자화·물리 batching·여러 물리 host·
  cache 전 원소 비교·P4 retained 수용은 아직 증명되지 않았다.
- 목적은 최신 모델의 제조사 Python 구현을 독립적으로 수정·검증·배포할 경로를 확보하는 것이다.
  llama.cpp에 없는 memory 클래스나 draft 기능을 공통 코어로 끌어오는 대안이 아니다. 같은 P4 바이너리로
  호환되는 Python 배포물을 교체할 수 있는지를 시험해 모델 개발 주기의 독립성을 증명한다.

### 0.2 소유권과 P4 변경 한도

| 위치 | 책임·필수 산출물 | 포함하지 않는 책임 |
| --- | --- | --- |
| P4 entrypoint/루트 빌드 | 외부 Rust crate 의존성·source 매핑·lock, `hf-transformers` 생성과 실제 지원 kind 광고, 통합/기존 경로 회귀 시험 | 모델명/Qwen class 등록, Python 설치·모델 weight 다운로드, 모델별 tensor 해석 |
| HF `layers/adapters/hf/adapter/` | construction/retained/process/ipc/lifecycle 역할 폴더. trait 구현, bounded input/completion, Python 감독·IPC, identity·예약·전달/출력 소유권, 오류/종료 증거 | 모델 layer/KV를 이해하는 Rust 스케줄러를 다시 구현 |
| HF 모델별 Python | loader·부분 forward·KV/recurrent·모델 배치/선택 정책·샘플링·양자화·tensor codec. 모델별 디렉터리 안에서도 역할 분리 | 모든 모델이 상속해야 하는 공통 모델 인터페이스, P4 broker를 우회하는 노드 간 전달 |
| HF 실행 명세/도구 | worker 환경 lock·entry/argv/env·모델/분할/장치·IPC/capability identity·배포·통합 scenario/fixture/report | P4 공통 envelope에 Qwen 필드 추가, llama 전용 PLAN/LOAD wire 재사용을 강제 |

**“의존성과 생성 분기만”은 제품 로직의 경계이지 수정 파일 두 개라는 뜻이 아니다.**
현재 [INSPECT](../entrypoints/agent/src/event_runtime/control/inspection/mod.rs)의 ADAPTERS도 `llamacpp` 상수다.
컴파일 가능한 종류와 실제 CREATE의 종류가 일치하게 entrypoint의 작은 정적 생성/지원 목록을 결속한다.
Python 환경/모델이 준비됐다는 광고는 별도 LOAD readiness 검증이다. 범용 dynamic plugin loader를 만들지 않는다.
공통 P4 core/trait/모델 wire 변경이 필요하다고 판단되면 실제 반례와 함께 [격리 계약](layer-isolation-contract.md#external-hf-boundary)으로
심사하고 별도 범위로 기록한다. HF를 llama staged 프로토콜에 맞추기 위해 그 경로를 재설계하지 않는다.

모델 스케줄러는 Python에 한 벌만 둔다. head/controller가 선택한 membership·position·issue를 Rust가
전달 전 예약/권한 계약으로 검증하고 immutable하게 결속한다. Rust의 전달 가능 여부 판단과 Python의
모델 배치 선택은 다른 책임이다. Python에 줄 수 없는 예산은 발행을 승인하지 않으며, Python의 token
계산 완료만으로 P4 출력이 승인되지 않는다. 이 기준은 HF 문서의 이전 Rust 배치 발행 설명보다 최신 사용자 지시를 우선한 결정이다.

### 0.3 먼저 연결해야 할 실제 부족분

1. **로컬 시험 controller를 P4 경로로 전환:** 현재 HF `routing.Pipeline`은 `host=local`만 허용하고
   `Peer.exchange`를 순서대로 호출한다. 이 코드는 reference로 보존한다. 통합 경로에서는 Python stage 결과가
   Rust retained completion→P4 broker/다음 node→다음 Python worker를 거친다. tail 결과/다음 step의 위치와
   head 또는 모델 controller의 스케줄링 소유자를 HF 실행 명세에서 고정한다. 별도 controller가 모든 worker의
   stdin/stdout을 직접 잡고 P4는 껍데기 노드만 만드는 구성은 불합격이다.
2. **실행 명세와 handshake:** CREATE는 Rust와 bounded mailbox만 만들고 LOAD가 지정 Python을 실행한다.
   현재 worker의 `ready/run_id`만으로 bridge/worker 호환성이 증명되지는 않는다. IPC 버전, worker 배포물 hash,
   model/tokenizer/plan·stage·load generation·dtype/boundary·지원 operation·byte bound를 서로 확인한다.
   stdout은 framing 전용, stderr는 별도 bounded 수집. LOAD 실패/중복 LOAD/부분 초기화의 자식·예약을 회수한다.
3. **retained 소유권:** Full/Closed에서 원래 allocation/claim을 돌려주고 peek는 소비하지 않는다. matching take는
   전체 front identity를 확인하며 wake 등록/해제와 capacity 재개를 구현한다. queued·held·in-flight·pending output을
   끝까지 계수한다. `completion_storage_snapshot=None`을 빈 것으로 바꾸지 않는다. 입력 byte, IPC 복사·scratch,
   출력·receipt 예약을 별도 계산한다. 32MiB frame 한도는 전체 heap 한도가 아니다.
4. **정산·취소·종료:** 현재 Python `release`와 step 경계 취소를 P4 요청 incarnation/issue/cutoff에 결속한다.
   partial write/worker death/timeout은 실행 불명으로 보존하고 같은 issue를 자동 재실행하지 않는다. token/native 효과의
   정산과 실제 외부 전달을 구분한다. worker 종료/상태 회수와 held output 0을 확인한 뒤만 unloaded를 표시한다.
   현재 P4 DELETE는 `snapshot()`의 empty/unloaded/closed와 권위 있는 retained count 0, 건강한 node task를 검사한다.
   Python이 종료됐다는 이유로 이 검사를 우회하지 않는다. worker 사망을 곧바로 전체 adapter completion stream 종료로 바꿔 node task를 끝내면 현재 DELETE가 거부하므로, 복구 가능한 오류는 제어/결과를 정산할 façade 수명과 분리한다. 진행 가능한 제어 경로와 자기 소유 child만 정리하는 절차를 둔다.
5. **동일 적재의 재수용:** 현재 `StageSessions`는 `len(active)+len(retired)`를 `max_requests`와 비교하므로
   해제한 ID도 한도를 차지한다. 기본 8건 처리 뒤 다음 8건을 같은 worker에 넣는 것은 현재 지원이 아니다.
   통합에서는 활성 요청 한도와 중복 방지 기록의 수명을 분리한다. 예를 들어 전 stage 정산/상태 해소를 확인한
   명시 session epoch 전환으로 bounded retired 기록을 회수하고 옛 epoch를 거부할 수 있다. 단순 `retired.clear()`나
   상한 증대로 우회하지 않는다. 기존 v1 독립 스크립트의 누적 한도 계약/반례는 보존하고 새 장기 실행 의미를
   버전화한다. slot/epoch 재사용 후 늦은 step/result/release가 새 요청을 바꾸지 않는지 반드시 검증한다.
6. **독립 배포:** Rust bridge/P4 ABI가 바뀌면 P4 재빌드, 호환되는 Python 구현 교체는 새 worker bundle identity로
   기존 P4 바이너리에서 LOAD한다. 실행 중인 worker를 덮어쓰지 않고 UNLOAD 후 전환한다. 비호환 bundle은 LOAD 전에
   거부한다. HF worker/환경 준비 명령과 실패 진단은 HF 저장소가 소유하고 P4 문서는 그 정확한 revision을 가리킨다.

### 0.4 Cargo 결합과 재현 가능한 배포

개발은 P4 내부 `layers/adapters/hf/adapter` path 연결로 시작할 수 있다. crate가 존재하기 전 빈 crate나
무조건 성공하는 stub을 P4에 등록하지 않는다. 독립 HF crate는 P4의 `p4-adapter`/`p4-protocol`만 필요한
공개 경계로 의존하고 P4 agent/llama private 구현을 의존하지 않는다. P4와 HF를 서로 workspace member로
흡수하거나 P4 소스를 복사해 별도 trait를 만드는 방식은 제외한다.

한 빌드의 `p4-adapter`/`p4-protocol` **package ID/source/version이 각각 하나**인지 Cargo metadata와
실제 생성자의 `Arc<dyn RetainedNodeAdapter>` 변환으로 검증한다. Git source를 path로 통일할 경우 소비자인
P4 루트의 `[patch]`가 적용돼야 하며 HF 하위 crate의 patch만으로 해결됐다고 보지 않는다.
[Cargo 공식 override 규칙](https://doc.rust-lang.org/cargo/reference/overriding-dependencies.html)을 따른다.
두 저장소를 참조하는 package와 양쪽 lockfile의 역할도 구분한다. 최종 P4 빌드는 P4 root lock과 양쪽 source seal이 기준이다.

선택 feature를 쓴다면 기본 off로 연결하고 enabled/disabled 생성·INSPECT·회귀를 모두 시험한다.
**optional path 의존성이면 HF checkout 없이도 기존 P4가 빌드된다는 가정은 금지한다.** Cargo lock 해석에는
optional dependency도 관여한다. [공식 resolver 설명](https://doc.rust-lang.org/cargo/reference/resolver.html#features).
개발 path를 출하 계약으로 남길 경우 두 repo의 정확한 commit을 복원하는 build bundle/스크립트를 반드시 납품한다.
배포용 Git revision/crate version을 택하려면 실제 접근 가능한 source와 재현 빌드를 먼저 검증한다. 아직 없는 remote URL이나
배포 버전을 추측하지 않고 remote 생성/push도 이 계획으로 자동 수행하지 않는다. worker bundle은 Rust crate source와 별도로 배포한다.

### 0.5 통합 완료 조건과 작업 인계

| 구현 묶음 (별도 릴리즈 아님) | 소유 저장소와 종료 증거 |
| --- | --- |
| HF-0 경계/명세 고정 | 양쪽 HEAD·dirty·trait·kind·제출/결과/오류/종료 wire와 예산·source mapping·테스트 fixture를 고정. HF 현재 worker와의 차이표 작성 |
| HF-1 독립 Rust bridge | HF 저장소에서 실제 retained mailbox와 fixture worker로 거부/포화/부분 I/O/사망/취소/미회수 출력/종료를 시험. 모델 없는 계약 검사와 실제 Qwen 비교를 구분 |
| HF-2 P4 event 연결 | P4 entrypoint의 의존성·생성·INSPECT·회귀/중립성 검증. 외부 crate의 구상 타입을 실제 생성하고 event 요청으로 worker를 실행 |
| HF-3 실행·배포 수용 | 같은 모델/정밀도/시나리오의 독립 기준과 P4 소비 결과 비교. 두 물리 host의 실제 stage 전달, 취소/해제/다음 요청/UNLOAD/DELETE, Python 교체와 재현 빌드까지 검증 |

필수 시험과 구체적 반례는 [HF 통합 검증 계약](distributed-batching-verification.md#hf-integration-contract)이 소유한다.
각 묶음의 통과는 중간 개발 증거다. **HF-3까지 동작하고 배포/운영/재현 명령이 있어야 HF 수용 완료**다.
작은 Qwen의 연결 성공을 초대형 모델 성능 승격으로 표현하지 않으며 H0–H7은 기존 제품별 목표에서 별도로 요구한다.
기존 llama timer RED를 숨기지 않는다. HF/core 변경에 필요한 회귀이면 먼저 해결하고, 무관하면 A의 미해결로
보존하며 전체 workspace GREEN을 주장하지 않는다. A의 배치 개선 전체를 HF 통합의 선행 구현으로 되돌리지 않는다.

새 세션은 먼저 HF의 HANDOFF/AGENTS/모델 문서를 읽고 Rust crate 생성 여부부터 확인한다. HF 저장소의
“P4 읽기 전용”은 그 작업의 기존 범위이며, 이 계획은 P4의 후속 수용 작업을 편성한 것이다. 이 문서 변경으로
다른 세션에 개발 메시지를 보내거나 HF 소스/문서·원격 환경을 변경하지 않았다. 실제 개발 시 외부 crate/worker
수정은 HF checkout에서, 생성 연결은 P4 checkout에서 각각 검증·커밋한다. 두 저장소 dirty를 함께 stage하지 않는다.

## 1. 지향점과 투자 원칙

**제안하는 제품 지향점은 보유한 이기종 LAN 장비에서, 실무에 선택할 만한 최신 대형 공개 가중치 모델을 정상 장문 작업에 지속 사용하게 만드는 것이다.** 모델·backend·memory 클래스 지원 개수는 목표 지표에서 제외한다. 2024년 이전 모델만을 위한 확장은 실제 사용자가 그 모델을 필요로 하는 구체적 근거가 없으면 채택하지 않는다. 오래된 알고리즘이라도 현재 모델의 병목을 해결한다면 검토한다.

초점은 세 가지다.

1. **현재 적재한 모델의 실용성:** 처리하지 못한 장문 요청을 끝내고 다음 요청을 계속 받는다. 서버에 모델이 올라가는 것과 사용자 작업을 끝내는 것을 구분한다.
2. **반복 작업의 낭비 제거:** 같은 코드·문서·대화 문맥을 매번 다시 계산하는 비용이 실제로 크면 이를 줄인다.
3. **투자 대비 모델 선택권:** 새 모델이 기존 모델보다 더 좋은 작업 결과나 더 적은 시간/메모리를 제공할 때만 지원 비용을 지불한다.

주 지표는 정상 작업 완료 수/시간, 고정 부하의 useful generation TPS, 사용자 대기 TTFT/ITL, 실패 후 사용 불가 시간이다. GPU 사용률·총 파라미터 수·지원 클래스 수는 설명 변수다. API/도구 호출을 요구하는 시나리오에서는 최종 답변·형식·도구 인자까지 판정하고, 추론 토큰을 많이 생산했다는 이유로 작업 완료를 인정하지 않는다.

코드 분석·장문 기술 문서 질의·여러 차례의 후속 질의는 **이번 검토의 실무 workload 제안**이다. 실제 고객 사용 비중은 확보하지 않았다. 그러므로 prefix hit rate나 MTP 수익성을 실사용 통계처럼 가정하지 않는다. 사용자에게 더 큰 GPU나 다른 장비를 구입시키는 것을 P4 개선의 기본 대안으로 삼지도 않는다.

**채택 문턱:** 대상 모델/작업, 현재 손실, 가능한 이득, 더 싼 대안, 비용과 중단 조건이 모두 설명돼야 한다. 이득이 미확인인 항목은 짧은 타당성 조사만 편성하고 릴리즈로 예약하지 않는다.

## 2. 현재 코드와 실측이 말하는 출발점

### 2.1 기준 갱신

- 최초 코드 감사는 6b175fb8535898ec4120297f1aadcd623879766d였다. 최종 감사 코드 HEAD는 **245d6b785c96ec770dc6041ded35457c2ef97260**이다. 직전 484b856ee 이후 532deee47/2ff8cc703은 OUTER 적재 정책과 검증을 수정했다. 후속 245d6b785는 적재 모듈을 `tools/model-loading/`으로 이동했고 핵심 정책 로직은 보존했다. 이 구간의 Rust/native 변경은 없다. 새 세션은 문서 커밋 이후 HEAD 차이를 다시 감사한다.
- llama.cpp 검토 pin은 451b89bae, 활성 패치는 27개다. 아래 Nemotron 실기는 별도 봉인 바이너리/upstream 434ddbbc0를 사용했다. 최신 pin의 실행 결과로 전용하지 않는다.
- [placement-policy](../tools/model-loading/src/placement-policy.ts)는 부분집합 전수 탐색/20장치 제한을 제거했다. [model-loading-policy](../tools/model-loading/src/model-loading-policy.ts)와 [model-loading-planner](../tools/model-loading/src/model-loading-planner.ts)는 typed fleet, 모델별 layer 수요, 측정 service time을 연결한다. 따라서 “계획기 없음/20장치 확장부터 구현”이라는 이전 제안은 이미 중복이다. 함수 결과와 native PLAN→LOAD의 실기 일치는 별도다.

### 2.2 확인한 손실

| 증거 | 현재 확인한 결과 | 투자 판단에 주는 의미 |
| --- | --- | --- |
| V1.1 [봉인 실기 기록](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md) | 한 호스트 MI250에서 생성 전용 raw TPS 개선이 있었지만 100k arm들은 deadline/정상 응답 수용 실패. 종료 watchdog도 한도 초과 | 새 배치 알고리즘을 하나 더 넣는 것보다 장문에서 native 비용·반환·회수가 어디서 막히는지 분리해야 함 |
| Nemotron [load-summary](../target/nemotron550-all-fleet/load-summary.json) | 7물리 호스트/8stage LOAD·SESSION 8/8, LOAD 약 2,114초 | 적재 경로는 이미 존재. 단순 LOAD 성공을 새 모델 서비스 출시로 삼을 수 없음 |
| Nemotron [test-progress](../target/nemotron550-all-fleet/test-progress.json) | 100,038토큰 입력 8건의 long arm이 약 7,203초 후 completed/released 0/0. deadline과 관측 누락, UNLOAD busy 기록 | 지원된 최신 모델의 실제 효용을 막는 직접 증거. 관측 누락도 있어 순수 compute 부족이나 scheduler 탓으로 단정할 수 없음 |
| Nemotron [mixed progress](../target/nemotron550-mixed-waves/progress.json) | old native work remains로 후속 혼합 시험 시작 거부 | 회수 문제는 다음 workload와 개발 검증 시간까지 잃게 함. 새로운 mixed 성능 결과는 없음 |
| [M3 MSA 거부 기록](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-13-minimax-m3-msa-distributed-rejection.md) | 정상 MSA GGUF는 stage residency 거부. 불완전 static flag patch 회수 | 최신 모델을 여는 가치와 별개로 의미 구현·커널 비용을 먼저 산정해야 함 |
| OUTER 갱신 | 532deee47에서 available·unified host 한도·합법 cut·배치 목적 비교 수정. 공유 topology를 표현하지 못한 복수 unified accelerator 입력은 거부. [6,678건 비교 기록](../tools/model-loading/README.md)은 합성 수요/참조 정책 일치이며 실기 아님 | 과거 저수준 capacity 100/합계 120 반례를 현재 상위 planner 결함으로 재사용하지 않는다. native PLAN/LOAD·현재 INSPECT와의 결속이 남은 대상 |
| 배치 선택 시험 재검토 | 최초 문서의 45/45 기록과 달리 재실행은 scheduler 33/33, bounded 9/10, service 2/2. timer 시험 단독 재현도 실패 | 현재 **44 passed / 1 failed**, 원인 미확정. Release A의 첫 회귀 과제이며 45 GREEN을 인수인계하지 않음 |

Nemotron progress 파일은 이번에 로컬에서 읽었으며 새 실기나 전체 로그 재감사는 아니다. 로드맵의 과거 “진행 중”보다 파일의 종료 상태가 새 정보지만, 원인과 정상 goodput은 아직 판정되지 않았다. 측정 arm은 수정하지 않았다.

## 3. 미지원 기능을 현재 모델과 연결해서 평가

아래 수치는 공식 모델 카드의 구조 설명이다. 서로 다른 모델의 활성 파라미터 수만으로 속도/품질 순위를 만들지 않는다. 최대 context도 P4의 지원 길이나 실용 처리 길이가 아니다.

| 모델/계열 | 최신 공식 근거와 실무 관련성 | P4 관점의 판단 |
| --- | --- | --- |
| Nemotron 3 Ultra 550B-A55B | 2026-06 공개, Mamba-2/attention/LatentMoE와 MTP, 최대 1M context. 공식 BF16 배포 예시는 대형 datacenter GPU를 전제로 함. [NVIDIA 모델 카드](https://huggingface.co/nvidia/NVIDIA-Nemotron-3-Ultra-550B-A55B-BF16) | 이미 7호스트 적재 근거가 있어 가장 낮은 추가 모델 지원 비용으로 실용성 개선을 시험할 기준 후보. 현재 Q5 GGUF/LAN의 품질·비용은 별도 측정 |
| Qwen3.5-397B-A17B | Gated DeltaNet+attention hybrid, native 262,144 context와 MTP. 코드·도구·장문 용도와 연결됨. [Qwen 모델 카드](https://huggingface.co/Qwen/Qwen3.5-397B-A17B) | pin factory는 Qwen3.5를 일반 hybrid로 분기한다. hybrid-idx 미지원 때문에 Qwen3.5도 미지원이라고 판단하면 오류. 기존 hybrid 경로의 비용/품질 대조 후보이며 현재 P4 실기 지원 승인은 아님 |
| MiniMax M3 약 428B/23B active | 2026-06 MSA 논문과 공개 모델, 긴 코드/문서 및 multimodal 작업. [모델 카드](https://huggingface.co/MiniMaxAI/MiniMax-M3), [MSA 논문](https://arxiv.org/abs/2606.13392) | 구형 모델 호환 작업은 아니다. 하지만 모델 고유 품질과 현재 장비의 효율을 증명하기 전 대규모 이식은 보류. vision/video 지원까지 함께 끌어오지 않음 |
| DeepSeek V4-Flash 284B/13B active | 2026년 공개, CSA/HCA와 1M context. Pro는 1.6T/49B. [공식 모델 카드](https://huggingface.co/deepseek-ai/DeepSeek-V4-Flash) | 가중치 크기를 이유로 우선 지원한다는 판단은 철회. 압축/indexer/MTP 상태와 backend 정확성 위험을 먼저 평가해야 함. GB10/RPC의 응답 손상 보고도 있어 해당 조합의 native 품질 검증 전 분산 확장하지 않음. Pro의 1.6T 적재는 현재 목표로 편성하지 않음 |
| DeepSeek V3.2/DSA | 2025-12 정식 공개. [공식 발표](https://api-docs.deepseek.com/news/news251201/) | 역시 2024 이전 지원이 아니다. 다만 V4-Flash 및 이미 가능한 모델보다 작업 품질/장비 적합성이 나은 근거가 없으면 DSA 전체 지원을 별도 목표로 삼지 않음 |

중요한 비용 차이가 있다. 현재 [MiniMax 공식 MSA 커널](https://github.com/MiniMax-AI/MSA)은 NVIDIA SM100·Linux x86_64를 요구하고 aarch64는 미시험으로 명시한다. 논문의 H800 측정과 오늘의 커널 저장소 대상도 구분해야 한다. [KTransformers M3 경로](https://github.com/kvcache-ai/ktransformers/blob/main/doc/en/kt-kernel/MiniMax-M3-Tutorial.md)는 CPU expert offload 대안을 제공하지만 해당 안내의 지원 GPU는 SM90이고 upstream SGLang 경로는 SM100이다. 이를 3090/Metal/MI250용 즉시 대체품으로 계산하지 않는다. MSA 알고리즘의 장점이 P4의 ggml 경로에서 나오려면 별도의 backend 구현/검증이 필요할 수 있다.

### 3.1 클래스 10개 중 몇 개 지원하는지는 투자 척도가 아니다

검토 pin의 llama_model::create_memory 분기를 직접 확인했다. 복합 모델은 metadata에 따라 경로가 달라질 수 있으므로 아래는 “이름 하나=지원 보장” 표가 아니다.

| memory 경로 | 코드에서 확인한 연결 | 결정 |
| --- | --- | --- |
| 일반 KV / iSWA / recurrent / hybrid | 기존 opt-in. Nemotron·Qwen3.5의 hybrid 경로 포함 | 목표 모델의 실제 서비스·조합 검증에 집중 |
| MSA | MINIMAX_M3 | M3의 과제 품질·장비별 sparse 비용이 투자 문턱을 넘을 때만 구현 |
| DSV4 | DEEPSEEK4, trunk의 특수 cache와 별도 MTP 경로 | Flash의 실효용과 kernel/state 적합성 비교를 먼저 수행 |
| DSA | DEEPSEEK32, GLM_DSA, HY_V4의 일부 checkpoint | 실제 선정 모델이 이 경로를 요구할 때만 범위 확정 |
| DSA-ISWA | DOTS3NOTE | 공식 [vLLM 구현](https://docs.vllm.ai/en/latest/api/vllm/models/dots3_note/nvidia/model/)은 확인했지만 이번 조사에서 P4 대상 작업 대비 고유 효용은 확인하지 못함. 보류 |
| hybrid-idx | QWEN4EXP, 공개 [Qwen3.8-Flash-Next](https://huggingface.co/Qwen/Qwen3.8-Flash-Next) | 현재 공개 모델과 연결되므로 무수요 enum으로 취급하지 않음. QSA 계산 미완과 recurrent/indexer 상태 위험 때문에 sparse 안정화 타당성 후보로 분류 |
| hybrid-ISWA | hybrid 모델에 SWA metadata가 있는 경우 | 특정 선정 checkpoint가 필요로 하지 않으면 일반 구현 목표로 편성하지 않음 |

### 3.2 희소 attention: 실행 경로의 존재를 안정화로 오판하지 않는다

사용자가 제공한 분석의 후반 정정을 반영한다. 이 문맥의 희소 지원은 검색용 sparse vector 저장이 아니라 **indexer로 attention 대상을 고르는 모델의 실행·상태 지원**이다. 다음 여섯 조건을 분리한다: artifact 보존 → 모델 계산 → 상태 정합성 → backend 실제 계산량 → P4 stage 실행 → 다중 컴퓨터 서비스 수용. 앞 조건의 성공이 뒤 조건의 승인이 아니다.

upstream 재확인 snapshot은 002a12ad25503a93501b2e188c360029830a241a(2026-09-13)다. P4 pin과 별개이며 최신 upstream을 P4에 적용한 것은 아니다.

| 항목 | 직접 확인한 사실 / 보고의 한계 | P4 투자 판단 |
| --- | --- | --- |
| M3 MSA | [모델 코드](https://github.com/ggml-org/llama.cpp/blob/002a12ad25503a93501b2e188c360029830a241a/src/models/minimax-m3.cpp#L222)는 flash attention과 stream 조건으로 MSA를 켠다. 다중 sequence+unified KV에서는 dense fallback. decode gather 구현이 있어도 그 조합의 sparse 서비스가 되는 것은 아님 | 현재 P4 [compat 검사](../layers/adapters/llamacpp/staged/server/src/compat/p4_llama_compat.cpp)는 이 조합을 명시적으로 거부한다. stage residency도 미승인. 단순 opt-in flag 추가를 개발 계획에서 제외 |
| Qwen3.8 QSA 계산 | [모델 코드](https://github.com/ggml-org/llama.cpp/blob/002a12ad25503a93501b2e188c360029830a241a/src/models/qwen4exp.cpp#L747)에 sparse 활성화 TODO와 전체 범위를 쓰는 호출이 남음 | indexer 비용을 지불하면서 sparse 이득을 못 얻는 구간은 투자 후보. backend shape·pooling·state까지 포함한 구현 범위를 산정해야 함 |
| Qwen 과거 두 세션 충돌 | [#27994](https://github.com/ggml-org/llama.cpp/issues/27994)는 closed이며 수정으로 지목한 [#27941](https://github.com/ggml-org/llama.cpp/pull/27941)은 2026-09-01 병합 | “현재도 그 버그가 그대로 있다”는 주장은 제외. sequence별 동일 logical position을 섞는 회귀 입력은 반드시 유지 |
| Qwen rollback | [#28019](https://github.com/ggml-org/llama.cpp/issues/28019)는 기본 제외된 rollback을 강제로 허용했을 때 multi-seq replay가 실패했다는 공개 재현 보고 | 기본 경로가 같은 손상을 낸다고 단정하지 않음. allowlist 확장만으로 MTP/rollback 지원을 선언할 수 없다는 반례 |
| DSA의 MLA/LID 두 cache | [cache 구현](https://github.com/ggml-org/llama.cpp/blob/002a12ad25503a93501b2e188c360029830a241a/src/llama-kv-cache-dsa.cpp#L61)은 양쪽에 remove/copy/shift/state 호출을 전달 | 이 구조만으로 현재 손상 버그를 확정할 수도, 원자적 정합성을 증명할 수도 없음. allocation 실패·변환·복원 후 물리 mapping과 실제 결과를 검사해야 함 |
| 불필요한 indexer V-cache | [#28296](https://github.com/ggml-org/llama.cpp/issues/28296)는 closed. 연결된 [#28330](https://github.com/ggml-org/llama.cpp/pull/28330)은 Qwen3.8 indexer의 V 할당 제거로 2026-09-10 병합 | “모든 최신 indexer가 불필요한 V를 할당한다”는 전제를 철회. DSA 등 다른 클래스의 실제 할당량까지 고쳤다는 의미도 아님. 모델별 PLAN/LOAD bytes로 투자량 결정 |
| V4 장비별 응답 품질 | [#28132](https://github.com/ggml-org/llama.cpp/issues/28132)에 GB10 2노드 RPC·특정 quant/build의 문자/응답 손상 보고 | 모델 전체나 unified KV가 원인이라는 증명은 아님. 해당 장비 후보는 native 기준 응답 확인을 먼저 하고 모델 적재량만으로 순위를 올리지 않음 |

[QSA #28734](https://github.com/ggml-org/llama.cpp/issues/28734)의 약 4배 개선은 작성자의 별도 패치·5×3090·F16 KV·긴 context 조건의 보고다. upstream 기본 성능이나 P4에서 재현한 수치가 아니며 전체 정상 응답/지속 서비스 검증도 아니다. 큰 개선 가능성을 조사할 근거로만 사용한다. 첨부의 V4 draft/prefix 세부 문제는 이번에 원인·버전까지 독립 확인하지 못했으므로 확정 결함 목록에서 제외했다.

공식 MSA 저장소의 SM100 조건은 **그 저장소 커널의 조건**이다. 별도 llama.cpp ggml 구현이 3090에서 원천 불가능하다는 근거로 쓰지 않는다. 반대로 특정 CUDA 성공이 Metal/ROCm에서 같은 sparse 경로와 품질을 보장하지도 않는다.

#### 상태 안정화의 구체적 검증 단위

핵심 identity는 sequence ID 하나가 아니라 **모델/상태 generation + sequence membership + logical position → block 구성원 → 현재 physical cell**이다. 공유 prefix는 하나의 cell에 여러 sequence가 속할 수 있으므로 단순 `(seq_id, position)` 일대일 구현을 강요하지 않는다. P4 공통 원장에 이 구조를 넣지 않고 llama.cpp 어댑터/native의 상태 계약으로 다룬다.

| 제안 시험 ID | 실패를 드러낼 입력 | 관측/통과 기준 |
| --- | --- | --- |
| SP-ARTIFACT | 같은 이름의 원형/indexer 제거 artifact, metadata 누락, tensor shape 불일치 | 실제 GGUF header와 tensor 목록·shard hash를 확인. 요구 구성 누락은 LOAD 전 거부하고 예약/출력 효과를 남기지 않음. HF architecture 표시는 보조 정보 |
| SP-SEQUENCE | 서로 다른 정답을 가진 A/B가 동일 position에서 시작; block 경계 앞뒤로 interleave; A 취소 후 C에 slot 재사용 | B 결과가 혼합 순서·C 내용에 의존하지 않음. 독립 실행 대비 선택 cell·상태와 logits/정상 과제 결과 비교; 허용 오차는 backend 기준선에서 사전 고정 |
| SP-LIFECYCLE | prefix copy/share, suffix remove, rewind, shift, slot 이동/압축, save/restore, prepare 실패 | **지원한다고 선언한 연산만** 실제 경로에서 조합 시험. 본 KV·indexer·recurrent의 identity/coverage가 맞고 stale cell 접근 없음. 거부 입력은 모든 관련 상태·예약 보존. 지원하지 않는 연산은 요청 전 명시적 거부 |
| SP-SPEC | partial accept/reject, replay, 다중 sequence 분할, 종료 뒤 재요청 | 전체 보조 상태까지 non-spec 기준과 일치. 단일 sequence restore 통과를 다중 sequence 증거로 대체하지 않음 |
| SP-KERNEL | block 수 경계를 넘는 4k/32k/100k 등 context, prefill/decode 구분, 대상 KV dtype·장비 | 실행 graph/kernel과 읽은 KV 범위·indexer/attention 시간·전체 응답 시간을 함께 기록. sparse 이름의 op 또는 top-k 생성만으로 계산 절감을 승인하지 않음 |
| SP-STAGED | 승인한 cut으로 여러 물리 호스트에 배치, 연속 도착·slot 재사용·취소 및 후속 정상 wave | stage의 본/보조 상태 residency·PLAN/LOAD bytes·회수와 최종 응답을 확인. 미지원 조합에서 silent dense 전환으로 sparse 개선을 포장하지 않음 |

이는 **새 시험 제안이며 실행 결과가 아니다.** 구현 변경에는 실제 소비 경로 반례와 수정 제거 변이를 붙이고 검증 규약을 적용한다. 모든 조합의 지원을 요구하지 않는다. 예를 들어 non-unified 전용 제품도 동시 사용자 수·메모리 비용·거부 동작·반복 wave가 약속을 충족하면 완결될 수 있다. 다만 non-unified 설정 한 줄을 정합성 증명으로 취급하지 않는다.

## 4. 기능별 조사와 투자 결정

### 4.1 지금 채택할 문제: 장문 계산 비용·회수·반환의 실제 병목

[vLLM의 현재 문서](https://docs.vllm.ai/en/latest/configuration/optimization/)는 decode 우선 chunked prefill과 total token budget을 이미 일반적인 방법으로 설명하고, 작은 budget의 ITL과 큰 budget의 TTFT 사이 trade-off를 명시한다. P4도 이 구조를 이미 구현했다. 따라서 “chunked prefill 도입”이나 knob 증가를 새 가치로 잡지 않는다.

P4의 부족한 부분은 실제 n_kv/문맥 길이·CPU mask·attention·CPU expert·경계 복사/전송·return stall을 나눈 비용과 선택 정책의 결속이다. 100k 실패를 scheduler와 GPU 둘 중 하나로 추측해 전면 수정하지 않는다. 원인이 CPU offload 또는 장비별 native 비용이면 해당 어댑터/backend 경로를 고치거나 검증된 cut을 바꿔야 한다.

**투자 이유:** 이미 보유·적재한 현대 모델에서 사용자 작업과 다음 시험을 모두 막고 있다. 이 문제를 줄이는 효과는 새 모델·cache·MTP에도 남는다.

**더 싼 대안:** 현 설정/합법 cut의 제한된 교정, 반환 OS 연결 문제 복구, 불필요한 CPU 경로 제거를 먼저 비교한다. 로드맵의 기존 input cap·독립 cohort·receipt 회계 수정은 재구현하지 않는다. 모든 내구 복구와 인증을 함께 만들지 않는다.

**중단 조건:** 짧은 비용 분해 뒤에도 병목이 분리되지 않으면 큰 구현에 들어가지 않는다. 측정 가능한 계산/대역폭 하한이 목표 시간을 이미 넘으면 scheduler만의 해결 계획을 폐기하고 모델/배치 위치/제품 부하의 별도 선택 문제로 올린다. 기존 실패 workload나 SLO를 낮춰 성공으로 바꾸지 않는다.

### 4.2 높은 잠재 가치, 사용 패턴 확인 필요: 반복 문맥 재사용

[vLLM APC](https://docs.vllm.ai/en/latest/features/automatic_prefix_caching/)는 동일 문서의 반복 질의와 다회 대화에서 prefill 재계산을 줄이지만 decode 시간은 줄이지 않는다고 명시한다. [SGLang HiCache](https://www.lmsys.org/blog/2025-09-10-sglang-hicache/)는 긴 coding-agent 문맥의 재사용 사례를 제시한다. 이 사례는 도입 가능성의 근거이며 보고된 개선율을 P4 예상치로 옮기지 않는다.

**이전 계획 수정:** SSD save/restore를 먼저 완제품으로 만들고 자동 재사용을 먼 뒤로 미루는 순서는 근거가 약했다. 실무가 반복 문맥이라면 먼저 사용자 요청이 실제로 공유하는 token prefix를 분석하고, 모델 상태에 맞는 메모리 내 재사용을 검토해야 한다. 디스크 계층은 재사용 간격 때문에 메모리에서 축출되는 경우에 추가한다.

Nemotron/Qwen 같은 hybrid에서는 recurrent state를 임의 prefix 길이로 잘라 되돌릴 수 없다. [Marconi 연구](https://arxiv.org/abs/2411.19379)와 현재 vLLM의 Mamba checkpoint 경계 설명이 이 추가 비용을 뒷받침한다. “파일 저장 API가 있으니 일반 radix cache를 붙인다”는 공수 산정은 잘못이다.

**가치 계산:** 반복 1회당 기대 절감은 h×T_prefill − T_lookup − h×T_restore − 분담 write 비용 − eviction 재계산 비용이다. h는 실제 token prefix/유효 state 기준 hit 비율이며 문장이 비슷한 비율이 아니다. 계층별 snapshot bytes와 다른 요청이 잃는 resident 용량도 포함한다.

**채택 조건:** 재현할 문서/코드 다회 질의에서 위 순절감이 양수이고 전체 작업 시간의 유의한 비중을 차지해야 한다. cold/no-hit 요청의 악화도 검사한다. 반복성이 낮거나 generation이 지배하면 보류한다. durable SSD 저장소 자체를 출시 목표로 잡지 않는다.

<a id="speculation-decision"></a>

### 4.3 native MTP만으로 가속 계획을 닫지 않는다

**수정 결론:** 기본 비교군은 non-spec, 기존 MTP는 저비용 후보, DFlash/DSpark는 현재 대형 모델용 artifact까지 확인할 적극 비교 대상이다. “외부 draft는 언젠가”로 일괄 보류한 이전 판단을 철회한다. 모든 알고리즘을 구현하는 대신 선정 모델에서 이득이 있는 한 방식을 제품 C로 완성한다. A는 spec-off 정상 서비스 자체로 완결되며 C를 기다리지 않는다.

고정 근거는 [451b89bae0c4b1dd612eb503ceace906c01ddcc9의 speculative 구현](https://github.com/ggml-org/llama.cpp/blob/451b89bae0c4b1dd612eb503ceace906c01ddcc9/common/speculative.cpp)과 [해당 pin 안내](https://github.com/ggml-org/llama.cpp/blob/451b89bae0c4b1dd612eb503ceace906c01ddcc9/docs/speculative.md)다. **이 pin에도 DFlash/DSpark가 있다.** 반면 P4 [compat](../layers/adapters/llamacpp/staged/server/src/compat/p4_llama_compat.cpp)의 `LlamaPlan::requests_unsupported_speculative`은 외부 draft 및 NONE/DRAFT_MTP 외 유형을 거부한다. 따라서 필요한 일은 upstream 기능 신규 작성보다 분산 실행 계약과 상태 소비 경로의 연결이다.

| 방식 | 입력·추가 비용·현재 근거 | 도입 판단 |
| --- | --- | --- |
| Native MTP | target 내 next-token head와 Verify/Replay. 별도 외부 draft checkpoint를 요구하지 않는 모델이 있음. P4 physical MTP 경로 존재 | 현재 Nemotron/Qwen3.5의 실제 tensor·rollback 가능 범위를 확인해 가장 싼 비교군으로 유지. “이미 있으니 충분”이라는 결론은 금지 |
| `draft-simple` | 작은 독립 모델이 token으로 순차 제안. 별도 가중치/KV·tokenizer 호환·draft 계산 비용 | 목표 모델에 맞는 작은 draft가 있고 hidden-state 전달보다 싸다는 근거가 있을 때만 후보. 작은 모델 두 개가 로드됐다는 사실은 효용 아님 |
| EAGLE3 | target의 선택 layer hidden state와 target에 맞춰 학습한 draft·vocabulary mapping | DFlash/DSpark와 같은 feature 수집/identity 문제가 있음. 선정 target의 artifact 또는 성능 이점이 없으면 별도 구현하지 않음 |
| DFlash | target hidden state를 받아 block 후보를 병렬 생성하는 diffusion draft. [논문](https://arxiv.org/abs/2602.06036)·[공식 코드](https://github.com/z-lab/dflash)·pin 구현 존재 | 긴 target cycle을 amortize할 가능성. draft block 확대 시 verify rows/KV와 다른 요청의 대기를 함께 평가 |
| DSpark | DFlash 계열 block 계산에 이전 token의 저차원 Markov 보정, 지원 checkpoint에서는 confidence에 따른 유효 prefix 선택. [논문](https://arxiv.org/abs/2607.05147)·[DeepSpec](https://github.com/deepseek-ai/DeepSpec)·pin 구현 존재 | 이름만 다른 옵션이 아님. 학습 block/anchor/confidence head/vocabulary와 target 조합을 검증하고 DFlash보다 확정 token당 전체 비용이 낮은지 비교 |
| N-gram 계열 | 기존 token 반복에서 후보를 가져와 추가 draft weight 없이 검증. upstream 여러 variant 존재 | 코드 수정·반복 문구 과제에서 값싼 대조군. 일반 추론 가속을 약속하지 않음. P4 Verify/Replay·취소 결속 비용은 여전히 있음 |

#### 지금 투자 검토할 실제 모델 쌍

- **Qwen3.5-397B-A17B + [Z-Lab DFlash draft](https://huggingface.co/z-lab/Qwen3.5-397B-A17B-DFlash):** 공식 카드가 target 쌍을 명시한다. 8×B200·BF16·SGLang·greedy/thinking·5회 반복에서 동시성 1과 32, built-in MTP와 비교한 결과를 공개했다. 이는 고부하 DFlash도 검토해야 할 근거다. 해당 runtime/장비의 보고값을 P4 Q5/LAN 이득으로 전용하지 않는다. GGUF 변환과 pin의 해당 draft graph·selected layer·hybrid replay는 별도 미검증이다.
- **DeepSeek-V4-Flash-0731 + [ggml-org DSpark GGUF](https://huggingface.co/ggml-org/DeepSeek-V4-Flash-0731-GGUF):** 같은 배포에서 DSpark artifact가 공개돼 있다. 표시 크기는 약 10.8–10.9GB로, 작은 draft라는 이름만으로 여유 VRAM에 무료 적재된다고 볼 수 없다. pin `src/models/dflash.cpp`에도 DSV4 graph 분기가 있다. 다만 P4 target의 DSV4 stage/state 자체가 선행 미승인이다. 이전 Flash variant와 0731 target을 임의 혼용하지 않는다.
- **Qwen3.5-122B-A10B + native MTP:** 변경된 A와 같은 target의 비교 후보. MTP tensor 포함만으로 Verify/Replay 지원을 승인하지 않는다. 전용 DFlash/DSpark artifact의 호환성을 별도 확인한다. 직접 학습 프로젝트를 C에 몰래 포함하지 않는다.

DeepSpec의 공개 소형 checkpoint는 해당 target의 non-thinking 데이터로 학습됐다고 명시한다. 이것만으로 큰 target·thinking·우리 코드 과제의 acceptance를 추정하지 않는다. 학습/데이터 준비는 큰 별도 비용이므로 적합한 artifact가 없으면 모델 쌍을 보류한다. 문서의 오래된 “Qwen3만 지원” 설명보다 pin의 실제 model graph와 artifact metadata를 우선하고, 그 역시 전체 backend 지원으로 확대하지 않는다.

#### P4에 남는 구체적 구현과 반례

1. **Feature 전달:** pin `common/speculative.cpp`는 draft metadata의 `target_layer_ids`로 target의 layer 입력 embedding을 수집한다. P4는 그 layer들이 다른 stage에 있을 수 있다. 필요한 tap·position·membership·generation·dtype를 adapter-owned capsule로 전달하고, 보내기 전 bytes/수명/rollback 보존량을 예약한다. tail의 마지막 hidden state 하나나 다른 프로세스의 context 포인터로 대체할 수 없다.
2. **배치와 배치 위치:** target/draft tensor·KV·scratch·feature 전송을 native PLAN에 모두 반영한다. 같은 GPU/별도 GPU/CPU 중 가능한 유한 배치만 비교한다. draft 때문에 resident 수가 줄면 그 손실을 비교에 포함한다. upstream의 `ctx_other`와 tensor 공유가 프로세스·backend 경계를 넘는다는 가정은 금지한다.
3. **상태와 정산:** partial accept 0/1/k/all, rejected suffix 제거, bonus token/EOS/stop, 재시도·late result·취소를 모든 참여 stage에 결속한다. 승인되지 않은 token을 먼저 출력하지 않는다. target KV뿐 아니라 recurrent/indexer/draft 상태를 non-spec 기준과 비교한다. unsupported rewind는 발행 전 거부한다.
4. **Fence 범위:** 현재 [Worker drive](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/drive.rs)는 `verify_fenced()`에서 head 발행 전체를 막는다. 독립 요청 B가 A의 검증 때문에 얼마나 지연되는지 측정한다. fence 축소는 immutable membership·자원/정산 독립성을 증명한 뒤에만 적용한다.
5. **채택 정책:** draft block을 학습 상한으로 제한하고 confidence 사용에 필요한 head를 확인한다. 미지원 옵션은 초기 거부한다. 낮은 동시성·R 포화·혼합 웨이브에서 spec-off/MTP/가능한 외부 draft를 비교해 정적 모드와 안전한 off 경로를 출시한다. adaptive 전환은 자체 반례가 없으면 추가하지 않는다.

확정 token 수 n에 대해 `n × baseline decode cycle`과 `draft + feature copy/network + target verify + replay + settle`을 비교하되 최종 목적함수는 전체 작업 완료 시간과 유효 TPS다. acceptance·draft TPS·GPU busy만으로 승인하지 않는다. synthetic acceptance 옵션은 모델 분포를 보존하지 않으므로 성능/품질 수용에서 금지하고 진단 arm으로만 분리한다. C의 출시 시험은 [§6.5](#later-releases)와 검증 규약을 따른다.

### 4.4 좁혀서 채택: 검증된 배치 명세와 자원 계획

다중 컴퓨터는 P4의 중요한 사용 조건이다. 다만 모든 장치 조합을 자동 최적화하는 제품보다, 목표 모델의 **검증된 유한 배치 명세**와 native PLAN→LOAD 일치를 제공하는 것이 지금 더 저렴하다.

[llama.cpp RPC](https://raw.githubusercontent.com/ggml-org/llama.cpp/master/tools/rpc/README.md)는 직접 분산 실행 대조 후보지만, 공식 문서 자체가 proof-of-concept·취약한 상태라고 설명한다. “RPC가 있으니 P4 불필요”도, “P4가 항상 더 빠르다”도 근거가 없다. 같은 모델/양자화/장비에서 가능한 경우에만 배포 비용·정상 부하·회수를 대조한다. vLLM/SGLang 또는 KTransformers가 지원 장비에서 요구를 이미 충족하면 새로운 P4 어댑터를 만드는 대신 그 엔진 사용을 대안으로 기록한다.

**채택 범위:** 현재 OUTER 구현과 중복하지 않고 shared host pool, 합법 cut, 시점이 바뀐 가용량, model/runtime identity 검증을 실제 배포 입력에 연결한다. 새 global optimizer·자동 autoscaling·새 엔진 staged 이식은 제외한다. 정확성과 실패 배치 방지가 목적이면 TPS 향상률을 억지로 만들지 않는다.

### 4.5 새 모델 지원: 이름보다 대체 가치를 먼저 증명

신규 후보가 통과해야 할 다섯 조건은 공개/재현 가능한 checkpoint, 목표 과제에서의 품질, 실제 quantized 파일과 runtime 메모리의 fleet 적합성, 목표 backend의 native kernel 경로, 기존 대안보다 나은 작업 효용이다. 로컬 배포 허용 조건도 해당 artifact에서 확인하되 이 문서는 법률 해석을 하지 않는다.

**수정한 제안:** 기존 경로의 Nemotron/Qwen3.5 실용성 개선과 별도로 Qwen3.8 QSA·M3 MSA·V4·DSA의 artifact/상태/커널 위험을 서면 비교한다. 그중 작업 효용과 목표 장비 적합성이 가장 유망한 한 모델만 작은 실행 타당성 조사에 진입한다. Flash를 크기만으로 우선하거나 M3의 gather 구현을 안정성으로 해석하지 않는다. 새 계열의 지원 일정은 이 조사 전 확정하지 않는다.

**중단 조건:** 요구 backend에 사실상 새 kernel stack이 필요하거나, 기존 hybrid 모델에 비해 품질/자원/시간 중 어느 것도 실무적으로 개선하지 못하면 지원을 미룬다. 수십 일의 공수를 투입한 뒤 “미지원 클래스 하나 감소”만 얻는 결과는 채택하지 않는다. M3의 기존 종결 판정은 유지한다.

### 4.6 이번 릴리즈에서 제외할 투자

| 항목 | 조사에 따른 판단 | 다시 검토할 조건 |
| --- | --- | --- |
| P/D disaggregation | [vLLM](https://docs.vllm.ai/en/latest/features/disagg_prefill/)은 주목적을 TTFT/ITL 독립 조절로 설명하며 해당 기능 자체의 throughput 개선을 주장하지 않는다. P4의 레이어 pipeline과도 다른 구조다. 모델 복제 용량·KV 전송이 추가되는 접근을 현재 LAN에 선행 도입하지 않음 | 중복 모델을 둘 자원과 KV 전송 대역폭이 있고, chunked prefill로 못 맞춘 명시적 tail SLO가 있을 때 |
| P4 내부 전체 PKI/권한 제품 | [vLLM 보안 지침](https://docs.vllm.ai/en/latest/usage/security/)도 내부 분산 경로를 신뢰 네트워크로 제한한다. 현 목표는 신뢰된 LAN이며 먼저 검증된 네트워크 격리/인증 계층을 활용할 수 있는지 판단 | 서로 신뢰하지 않는 주체를 같은 fleet에 받거나 외부 control 노출이 실제 요구일 때. API key만으로 내부 TCP가 보호된다고 보지 않음 |
| durable 요청 replay·무중단 crash continuation | [Ray Serve](https://docs.ray.io/en/latest/serve/architecture.html)도 replica/controller 재시작과 휘발성 요청 상태 손실을 구분한다. P4의 CPU/GPU/KV·외부 출력까지 복구하는 비용은 watchdog/cold restart보다 훨씬 큼 | 실제 장애 빈도×잃는 작업 비용이 구현·운영 비용보다 클 때. token 전달의 exactly-once를 성급히 약속하지 않음 |
| 전체 backend·모델·memory 조합 지원 | 이름이나 enum 존재만으로 수요가 입증되지 않음 | 선정 모델/장비의 사용자 과제에 필수인 조합 |
| 프로파일 근거 없는 sampler 병렬화·zero-copy | 중요할 수 있지만 전체 시간에서 작은 부분이면 공수 대비 효과가 작음 | 해당 구간이 실제 critical path에서 큰 비중이며 안전한 제거/병렬화가 가능한 경우 |

어느 구간의 전체 시간 비중이 f이면 그 구간만 무한히 빨라져도 이론상 속도 향상 상한은 1/(1−f)다. 예를 들어 5% 구간을 완전히 제거해도 약 1.053배다. 이 계산은 P4의 측정값이 아니라 작은 병목에 큰 투자를 하지 않기 위한 산술 기준이다.

<a id="batch-decisions"></a>

## 5. 배치·아키텍처·운영 문제를 연결한 채택 결정

[배치 코드 검토](batching-code-review.md)는 G1–G6의 상세 경로/selector 수치를 소유한다. 아래의 “A 포함”은 **필수 계약을 만족시킬 책임**이며 모든 후보 알고리즘을 무조건 작성하라는 뜻이 아니다. 기존 경로가 실제 소비 시험을 통과하면 재사용하고, 실패가 확인된 최소 경로를 수정한다. 공수 일수와 일률적인 20% 투자 문턱은 실측 근거가 부족해 삭제한다. 성능 승격 수치는 기존 H5를 적용하고, 큰 변경은 비용 분해와 반례를 먼저 제출해 범위를 재산정한다.

| 후보 | 실제 문제와 손실 가설 | 제품 결정·수정 경계 | 증명/중단 조건 |
| --- | --- | --- | --- |
| G1 기본 배처의 P 진행 | ordinary selector는 D가 capacity를 채우면 P를 선택하지 않음. cap 8·D8/P1을 32번 재제공하면 D256/P0; bounded는 D224/P32. 실제 웨이브의 기아를 측정한 값은 아님 | **A 필수.** 일반 경로에도 유한 P 대기/진행 계약. 이미 있는 bounded 정책과 비교해 최소 변경. hybrid equal-width·verify/replay 불변식은 유지 | A-BATCH: D 수요가 cap 미만/같음/초과, cap 1/8, 허용·거부 반복. 선택기뿐 아니라 실제 Worker에서 P 진전·D 지연·거부 시 상태 보존 |
| G2 UBATCH가 decode 완료 뒤 일괄 전송 | native가 callback 출력을 모아 `llama_decode` 종료 뒤 PhysicalResult로 반환. 작은 n_ubatch가 다음 stage의 조기 시작을 보장하지 않음 | **A는 독립 logical issue로 가능한 overlap을 검증.** 진짜 부분 UBATCH 전송은 C 또는 별도 가치가 입증된 후속 제품에 조건부 포함 | native compute/capture/return/forward timeline으로 임계경로 비중 확인. downstream이 일부 결과를 소비한 뒤 upstream 실패하는 commit/abort·credit 계약까지 필요. callback만 여는 안은 제외 |
| G3 pipeline D 폭·window | active 인구/window에서 폭 계산. 고정 cohort identity나 비용 최적 정책이 아니며 D 폭 축소는 RPC/launch 고정비를 늘릴 수 있음 | **A 포함.** 승인된 유한 profile 선택, D 폭/window/P budget의 제한된 실험. 무제한 online 탐색·전역 최적화 제외 | 동일 cut/메모리에서 window 1/2/4와 합법적인 이웃 row cap 비교. profile 수·선정 기준을 A/B 전 고정. 마지막 holdout으로 선택 편향 검사 |
| G4 서비스 비용의 실제 shape | max position이 실제 n_kv와 같지 않으며 다른 sequence의 unified KV, CPU/offload, mask/graph 비용을 빠뜨릴 수 있음 | **A 필수 비용 관측.** adapter/native가 실제 n_kv·phase·rows·membership·placement를 산출. 소비 근거 없는 온라인 controller는 off 유지 | 같은 logical position/rows, 다른 KV 점유와 CPU/GPU 배치를 대조. profile 오차·동기화·전송과 실제 TTFT/ITL 연결. 비용 함수를 더해도 순비용이 줄지 않으면 정적 profile 유지 |
| G5 한 요청의 복수 P fragment | 현재 pipeline에서는 prefill_fragments≠1 거부. chunk를 줄여도 한 요청이 여러 flight를 채우지 못함 | **A는 1 유지.** 여러 독립 요청의 overlap으로 서비스 완결. 복수 fragment는 긴 단일 요청 bubble이 전체 작업 손실을 지배할 때만 별도 실험 | 일반 KV와 recurrent/hybrid를 따로 판정. prefix frontier·역순/중복·취소·부분 실패·verify/replay 반례를 통과한 model 조합만 승격. 제한 숫자 삭제 금지 |
| G6 요청별 대기와 session 선택 | Demand에 age/deadline/실제 KV가 부족하고 stage service RPC는 사용자 ITL이 아님. `first_session_with_work`는 첫 eligible session을 선택하며 회전하지 않음 | **A 필수.** 요청·세션별 blocked reason/기간, accepted issue 기준 진행 회계. 필요 시 round-robin/age/deficit 중 가장 단순한 정책 선택 | 서로 다른 session A/B와 동일 session 여러 요청을 모두 시험. 거부는 차례·age 예산을 소모하지 않고, overload는 명시 거부. 순서 risk를 실기 기아 확정으로 보고하지 않음 |

### 5.1 외부 덱과 기존 논의의 누락 대장

C1–C9는 이번 감사에서 붙인 주제 ID이며 이미 구현된 시험 이름이 아니다. 각 요구의 구현 소유는 [격리 계약](layer-isolation-contract.md)을 따른다.

| 주제/덱 | 현재 구현 또는 부족한 계약 | 최종 귀속 |
| --- | --- | --- |
| C1 운영 Cancel/Drain, 10·12장 | event Worker에 운영 요청의 Cancel/Drain 소비 경로가 없음. teardown·edge credit 반환은 모든 stage의 KV quiescence가 아님 | A-LIFE: 신규 발행 중지, 진행 중 결과 분류·정산, 회수 후 slot 재사용. 응답 불명 host는 epoch 격리 후 cold recovery; remote KV 0을 추정하지 않음 |
| C2 자원/receipt, 9–11장 | input request budget과 native output/return/receipt 한도는 다름. event broker ledger는 개수 중심 보관. 기존 ControlBudget RELEASE/SETTLE 수정은 존재 | A-BYTES: pending wire·retained output·중복 판정 receipt·scratch의 수명별 선예약과 거부 무효과. 기존 회계 재작성 대신 실제 consumer까지 결속 |
| C3 스케줄링, 10–11장 | 덱의 patience 설명은 ordinary 기본 배처 전체를 증명하지 않음. controller는 기본 off이며 service 추정과 TTFT/ITL은 다름 | A-BATCH/A-COST 및 G1/G3/G4/G6. G2/G5는 조건부 |
| C4 배치/적재 계획, 1–8장 | OUTER의 최신 available/legal-cut/unified 검증은 이미 구현·합성 시험. native PLAN→LOAD와 실제 pool 분리·장치 배치 검증은 남음 | A-PLAN. 과거 20장치 제한·상위 공유 pool 결함으로 중복 개발하지 않음 |
| C5 특수 아키텍처, 14장 | MSA/DSV4/DSA/hybrid-idx는 tensor shape뿐 아니라 보조 상태·alias·합법 cut 문제 | S의 SP 전 게이트. A의 Qwen3.5 hybrid도 recurrent 정합성/실제 KV bytes/회수는 검사 |
| C6 재사용/영속, 12장 | native save/restore가 있어도 event v2에 Persist/Restore 소비가 연결된 것은 아님. 기존 `adapter/cache_transactions.inc.rs`를 현재 경로로 오독 금지 | B가 실제 사용자 요청부터 재사용까지 완성. 최초 in-memory 범위이면 SSD는 명시 미지원 |
| C7 가속, 13장 | P4 native MTP 지원과 upstream DFlash/DSpark 지원의 층이 다름. 전체 head verify fence·hidden-state 전송·보조 상태 rollback이 비용/정합성 쟁점 | C: §4.3의 target 쌍 중 채택한 방식 하나를 end-to-end 완성. MTP만으로 검토 종료 금지 |
| C8 격리/확장, 1–8·14–15장 | 공개 facade 뒤 Impl accessor·raw ordinal·transitive include/link도 경계. clean patch replay는 의미 호환 아님 | 전 제품: adapter×memory×backend별 PLAN/LOAD/physical/KV/spec 증거 matrix, compiler·상태 변경 권한·의미 conformance를 각각 검증 |
| C9 보안/복구, 15장 | trusted LAN 운영, TLS/source 인증·durable replay 제품 아님 | A 운영 명세에 bind/방화벽·접근 주체·cold recovery 절차 포함. 외부 노출/다중 불신 tenant는 지원 범위 밖. 재시작 뒤 이전 요청 자동 재실행 금지 |

C6의 별도 저장소 위험도 보존한다. [native KV](../layers/adapters/llamacpp/staged/server/src/runtime/llama_stage_runtime_kv.cpp)는 모델 identity가 없을 때 경로를 사용하고, build identity는 upstream/system 정보에 의존하며, 128MiB 저장 상한과 remove 결과 확인 문제가 있다. SSD를 제품 B에 넣는 경우 shard/tokenizer/template·patch/state ABI·KV layout identity, bounded chunk I/O, 저장 완료와 메모리 회수 실패의 별도 상태, 부분 restore/재시작을 K 게이트로 증명해야 한다. in-memory B를 선택해도 “이미 SSD 운영 지원”으로 기록하지 않는다.

### 5.2 모델별로 함께 풀어야 하는 조합

| 실행 구조 | 배치와 stage 경계 | 재사용/가속에서 추가되는 상태 | 판단 |
| --- | --- | --- | --- |
| Nemotron Mamba-2/attention hybrid | recurrent equal-width·순서 제약, CPU expert와 실제 n_kv 비용. weight 용량만 맞춘 cut이 최적이라는 보장 없음 | 임의 prefix rewind 대신 유효 checkpoint. MTP partial replay는 recurrent까지 포함 | A에서는 기존 non-spec/1 fragment로 서비스 수용. B/C에서는 해당 연산만 추가 승인 |
| Qwen3.5 GDN hybrid | 일반 hybrid와 hybrid-idx를 구분. DFlash selected layers가 여러 stage에 놓일 수 있음 | linear-attention 상태·draft feature·block verify. tokenizer 일치만으로 state 호환 아님 | 큰 현대 모델의 C 우선 비교 후보; 먼저 non-spec native/분산 기준 품질 필요 |
| M3 MSA | indexer/attention 두 비용과 다중 seq+unified 제한. stage-local residency는 미승인 | indexer cell과 본 KV를 copy/remove/restore/slot reuse에서 함께 취급 | S, current fail-closed 유지. dense fallback 결과를 sparse 성과로 발표 금지 |
| DSA/GLM-DSA | MLA/LID cache의 결속. GLM-DSA는 이전 full layer의 top-k를 뒤 layer가 공유하는 graph 경로가 있어 cut이 그 의존성을 끊을 수 있음 | 물리 cell mapping·공유 top-k 수명·복합 cache prepare 실패 | SP와 합법 cut/forward payload 증명 전 지원 확대 금지 |
| DSV4/DSpark | compressed/indexed cache와 draft graph·추가 약 11GB artifact, target variant 결속 | target 보조 cache·MTP/DSpark·prefix가 서로 독립적으로 안정화됐다고 가정 불가 | S와 C의 계약을 같은 선정 모델에서 모두 통과해야 출시; 중간 “적재판” 출시 없음 |
| Qwen3.8 QSA/hybrid-idx | sparse 계산 TODO와 실제 kernel 경로, recurrent+indexer 복합 state | sequence membership/block key·rollback 경계 | 최신 버그 수정은 보존하고 아직 없는 계산/의미만 비용 평가. 직접 CUDA/Metal/ROCm 증거를 교환하지 않음 |

<a id="release-a"></a>

## 6. HF 수용 이후 릴리즈 A — 장문 작업을 지속 처리하는 분산 서비스

### 6.1 사용자 약속과 출하 범위

**사용자는 승인된 fleet에서 Qwen3.5-122B-A10B를 한 번 적재한 뒤 여러 장단문 작업을 제출하고, 진행 상태/정상 응답을 받으며, 요청 취소나 과부하 뒤에도 새 작업을 실행할 수 있다.** 긴 문서 분석은 비동기 작업으로 제공하고 짧은 후속 질의는 streaming으로 제공한다. 유지보수 시 admission 중단→drain→UNLOAD를 수행한다. 실패가 불명인 경우에는 성공을 가장하지 않고 명시 실패·격리·cold recovery를 제공한다. B의 prefix cache나 C의 가속 없이 이 작업 전체가 가능해야 A 출시다.

기존 CLI/event 경로를 제품 진입점으로 사용한다. 새 웹 UI·OpenAI 호환 API·범용 인증 서버는 A 요구가 아니다. 다만 사용자가 실제로 제출/진행 조회/취소/종료할 수 있는 **문서화된 OUTER 명령과 event 소비 경로**는 필수다. 명령 이름/프로토콜 필드를 아직 구현된 API처럼 문서에 미리 만들어 놓지 않는다. 구현 체크포인트에서 확정한 CLI 도움말·예제·오류 코드를 함께 출하한다.

지원 목표를 다음으로 고정한다. 현재 실기 승인이라는 의미는 아니다.

- 기준 모델(2026-09-15 사용자 변경): `unsloth/Qwen3.5-122B-A10B-MTP-GGUF`, `Qwen3.5-122B-A10B-UD-Q5_K_S-00001-of-00003.gguf`에서 시작하는 3 shard. 로컬 원본은 `S:/models/unsloth/Qwen3.5-122B-A10B-MTP-GGUF/`다. GGUF `qwen35moe.block_count=49`, `nextn_predict_layers=1`을 확인했다. 로컬 native PLAN에서 n_layer48/n_layer_all49를 확인했으며 전체 합법 cut·allocation은 별도 검증한다. MTP 포함 파일이어도 A의 speculative 실행은 비활성이다. 실제 hash·metadata·tokenizer/template를 새 Qwen 명세에 고정하고 같은 파일명의 교체는 새 artifact로 취급한다.
- 기준 fleet: 기존 장비 중 최소2물리 host의 실제 분산 실행. 정확한 host/device/cut/stage 수는 Qwen의 새 INSPECT·native PLAN·공유 pool 예산·비용 비교로 정하고 실행 전에 봉인한다. 과거 Qwen5host/6stage cut은 탐색 시작 후보이며 resident8/context 조건의 수용 증거가 아니다. 아래550B 표를 Qwen에 재사용하지 않는다. IP·device ordinal을 하드웨어 identity로 쓰지 않는다.
- resident 8, sequence당 context 102,400, total 819,200, F16 K/V, flash attention on, unified KV, native batch/ubatch 128/64, spec none. target 외 추가 모델/새 backend는 출시 범위 밖이다.
- 기본 공개 모드는 승인한 정적 batch profile 하나다. pipeline/controller의 전역 기본값을 실기 전 바꾸지 않는다. 후보는 기존 ordinary와 bounded/pipeline의 유한 비교로 선정한다. 자동 service controller·복수 prefill fragment·외부 draft·자동 prefix 재사용은 A 비활성.

| 과거550B host suffix (192.168.0.x), 새 대상에 미적용 | stage / layer 범위 [begin,end) | backend 참고 |
| --- | --- | --- |
| .29 | 0 [0,24), 1 [24,48) | CUDA0/1, 한 host의 두 stage |
| .26 | 2 [48,70) | CUDA |
| .20 / .21 | 3 [70,78), 4 [78,88) | Metal |
| .17 / .19 / .6 | 5 [88,94), 6 [94,98), 7 [98,108) | CUDA; 중앙 .6은 해당 승인 장치 identity 확인 |

현재 OUTER DDR profile은 whole-layer CPU stage를 표현하며 GPU expert offload·같은 장치 여러 stage의 contention까지 최적화하지 않는다. A는 기존 CPU expert 배치를 **유한 승인 layout으로 명시**하고 native PLAN/실제 allocation·calibration으로 검증할 수 있다. 표현하지 못한 배치를 planner가 자동 승인한 것으로 포장하지 않는다. 범용 expert optimizer 추가는 A의 필수 요건이 아니다.

550B 원본 [config](../target/nemotron550-all-fleet/config.json)와 실패 결과는 역사 증거로 보존하며 새 Qwen 대상의 선행 재실행을 요구하지 않는다. Qwen의 모델·tensor override·실제 metadata·tokenizer/template·device/cut을 새로 고정한다. 과거 binary/channel/session/port를 그대로 재사용하지 않는다. 새 실행은 새 namespace/epoch와 현재 빌드 산출물로 봉인한다. layout 개선은 별도 실험축으로 먼저 결정하며, 그 이득을 batch 정책 개선율과 합산하지 않는다. fleet가 없거나 해당 artifact에 접근할 수 없으면 로컬 개발 후 H6을 BLOCKED로 남긴다. 승인된 Qwen122B 이외의 소형 conformance 모델로 A 완료를 대신하지 않는다.

### 6.2 구현 단위 — 각각은 중간 커밋이며 별도 릴리즈가 아니다

| 단위 | 구현 진입점과 완료 산출물 | 반례/소비 경로 |
| --- | --- | --- |
| A0 재현·명세 고정 | 기존 timer RED 원인 분리, 기존 Nemotron 실패를 보존하고 새 Qwen trace의 compute/return/cleanup 분해. `tools/event-drive/src/run`의 수용 판정과 실제 명령 검토. tracked corpus·benchmark manifest·재현 명령 작성 | A-RED/A-COST. 새 계측 전 기존 실패가 재현되는지 확인. 누락된 단계가 계산 중인지 반환/관측 유실인지 추정하지 않음 |
| A1 배포 전 정확한 거부 | `tools/model-loading/index.ts`의 공개 입력/결과→native PLAN→LOAD→실제 allocation 비교. pool·artifact·capability mismatch에서는 전체 배포 실패와 이미 생성한 자원 회수 | A-PLAN. fresh available·통합 pool·합법 cut·실제 CPU expert 배치·shard/ABI mismatch. planner 알고리즘 재작성 금지 |
| A2 유한 작업 수명 | OUTER/event adapter dispatch→Worker 발행/정산→native quiescence→모든 stage 회수→결과 terminal. Cancel/Drain·상태 조회·admission의 실제 사용자 명령 | A-LIFE. 취소 전 commit된 출력과 취소 후 금지 출력 구분. 결과 불명 상태·first error·cleanup error 보존. 완료/취소/실패와 자원 회수 상태를 분리 |
| A3 byte 수용과 제어 진행 | `layers/agent/src/event_broker/ledger.rs`, 기존 control ownership, adapter input/output/return 경로의 한도 결속. output receipt/payload 중복 소유를 계산 | A-BYTES. 거부 시 원장·예약·credit·token 효과 0, duplicate/late reply에서도 1회 효과. 데이터 포화 중 취소/정산 제어 진행 |
| A4 배치 진행과 profile | `v2/scheduler.rs`, `scheduler/pipeline.rs`, `node/state.rs`, `worker/drive.rs`, `worker/service.rs`, native physical capture/return. G1/G3/G4/G6 계약의 최소 변경 | A-BATCH/A-COST. request와 session 공정성을 구분. 검증된 범위의 KV/hybrid 불변식 보존. G2/G5 확대 없이도 다음 wave가 진행해야 함 |
| A5 제품 수용·운영 패키지 | 현재 executable/동반 DLL·profile·지원/거부 matrix·CLI 예제·정상/취소/drain/cold recovery 절차·test runner/report. 새 세션이 같은 명령으로 재현 | A-SERVICE 및 H0–H7. source/binary 결속과 최종 전체 시험. 배포/성능 승격은 해당 실기 통과 후만 기록 |

A0에서 장문 계산의 측정 하한이 제품 SLO를 이미 넘으면 필요한 kernel/placement 변경 규모를 먼저 산정한다. 작은 scheduler 변경으로 된다고 밀어붙이지 않는다. 실현 가능한 최소 수정이 범위를 넘으면 **A 미완료와 구체적 병목/필요 자원**을 보고한다. 이번 사용자 모델 변경은 새 target/spec으로 관리하며 과거550B 실패를 통과로 바꾸지 않는다. Qwen의 100k 입력·정답·SLO를 실행 뒤 완화해 출시로 만들지 않는다. 개발 중 회귀 고정·기능 연결·검증 완료마다 커밋하고 남은 작업을 로드맵에 기록한다.

### 6.3 릴리즈를 판정할 시험·운영 계약

구체적 입력, 제한값과 판정 소유는 [Release A 검증 계약](distributed-batching-verification.md#release-a-contract)이다. A-RED/PLAN/BYTES/LIFE/BATCH/COST/SERVICE는 **이번에 정의한 예정 시험 ID**이며 기존 test 함수가 아니다. 개발자가 이를 실제 실행 runner/테스트에 연결하고, 기대 실패와 수정 제거 변이를 남겨야 한다.

필수 산출물은 다음과 같다.

1. **지원 명세:** model shards/hash·runtime/patch/state ABI·장치 identity·cut·KV·batch/profile·trust boundary·지원 operation. PLAN 성공/LOAD 성공/native conformance/분산 정상 서비스/spec 각각 별도 열. 범위 밖은 사전 거부한다.
2. **고정 실행 명세:** 정상 corpus와 oracle·tokenization·seed·도착 schedule·deadline/SLO·resident/queue/byte bound·A/B/holdout 순서. loader가 placeholder·빠진 상한·미지원 조합을 거부해야 한다.
3. **자동 판정과 증거:** 모든 요청 전문/정상 stop·정산/회수·late result·누락/오류, host별 할당/compute/전송 자료, source/binary/library hash. `target/`만을 유일한 인수인계 위치로 삼지 않고 재생 가능한 manifest·corpus·실행 명령·결과 요약을 tracked 경로에 보존한다. 대용량 raw는 hash와 접근 경로를 기록한다.
4. **사용자 운용:** 설치·제출·상태·취소·새 요청·drain·UNLOAD·통신 장애 뒤 복구를 같은 출하 구성에서 재현. 통신이 끊긴 host의 상태를 아는 것처럼 즉시 slot을 재사용하지 않는다. 이전 epoch 차단과 소유 프로세스 종료/새 load를 확인한 뒤 cold restart한다.

baseline이 정상 완료 0이면 개선율을 무한대로 보고하지 않는다. A의 첫 성과는 실패→정상 서비스와 회수다. 양쪽 정상 arm이 성립하는 범위의 최적화는 H5로 증명한다. 관측만 완성하거나 local GREEN만 얻은 상태는 A 출시가 아니다.

### 6.4 기본값·실패·호환성 결정

- 잘못된 profile/artifact/backend 조합, byte 한도 초과는 발행 전에 명시 거부하고 caller가 이유를 확인할 수 있어야 한다. 처리 중 일반 요청 실패와 서버 전체 poison을 구분한다.
- 취소 수신은 native GPU 작업의 즉시 강제 중단을 뜻하지 않는다. 새 발행을 막고 이미 발행한 작업의 경계까지 정산하거나 bounded fault recovery로 이동한다. 불명 작업을 성공 drain으로 표기하지 않는다.
- 안전상 필요한 protocol/identity 변경은 버전을 올리고 구형 peer와 혼합 실행을 시작 전에 거부한다. backend별 private 타입을 공통 protocol/원장으로 노출하지 않는다.
- 실험 후보가 이득/안전성 기준을 못 넘으면 기본 프로파일에 포함하지 않는다. 기존 A 서비스가 이 옵션 없이 완결되는지 수용한다. 이전 기능이 실패했는데 옵션만 끄고 기존 회귀가 해결됐다고 보고하지 않는다.

<a id="later-releases"></a>

### 6.5 후속 제품 — 기능 목록이 아닌 채택 조건이 있는 완제품

S/B/C의 실행 우선순위는 로드맵이 소유한다. 각 후보의 타당성 판정 전에는 개발 일정을 예약하지 않는다. 선정 대상이 없다는 것은 지금 모든 모델을 구현하라는 요구가 아니라 투자 보류다.

| 제품 | 독립적으로 제공할 사용자 효용 | 확정해야 할 범위·필수 구현 | 출시 증명·투자 중단 |
| --- | --- | --- | --- |
| S: 선정 현대 희소 모델의 장문 서비스 | A의 모델보다 해당 과제를 더 잘 풀거나 같은 품질을 더 적은 자원/시간에 제공 | §3 후보 중 한 target/artifact/quant/backend·동시성/context를 선정. 본/보조 cache·합법 cut·실제 sparse kernel·admission·취소/회수·배포/운영 모두 포함 | 기존 모델과 **같은 과제** 품질/작업시간/메모리 비교, 해당 SP 전 게이트, H0–H7. native sparse 미실행·dense fallback·한 sequence만 통과한 다중 서비스는 실패. 새 kernel stack 비용만 남고 실무 이점이 없으면 보류 |
| B: 반복 문서/코드의 재사용 대화 | 같은 문서를 여러 차례 묻는 전체 작업의 대기를 줄임 | 선정 모델의 유효 prefix/checkpoint, identity·admission·eviction·cold fallback·사용자 간 오염 방지·취소/회수. 첫 범위는 in-memory, SSD는 실제 축출 간격이 필요로 할 때만 | 4k/32k/100k 대상의 4회 후속 질의, 공통 prefix 0/50/90%, cold/hit/no-hit·다른 사용자 혼합. 전체 시간과 다른 요청 손실을 측정. hybrid 임의 rewind 금지; 기능별 K/H 게이트·오염 반례·변이. hit rate만 높고 순절감이 없으면 보류 |
| C: target에 맞는 speculative 응답 서비스 | 같은 target/품질을 유지하면서 명시한 동시성 영역에서 더 빨리 답변 | §4.3의 model 쌍을 먼저 선정. non-spec/MTP/가능한 DFlash·DSpark·저비용 draftless 비교에서 한 방식을 채택. feature 수집·draft 배치·verify/replay·fence·off mode·취소/회수를 한 제품에 포함 | 동시성 1/2/4/R와 혼합/overload에서 전체 비용·tail·유효 TPS·draft 메모리 비교. partial reject/slot 재사용·target/draft identity mismatch·출력 분포/품질 시험과 H5. 포화 영역에서 off 경로가 수용돼야 하며 gain이 없는 방식은 지원 수를 위해 추가하지 않음 |

B/C의 세부 spec도 구현 전에 target artifact·SLO·자원 상한을 봉인한다. greedy는 같은 승인 layout의 non-spec token/position 및 logits 기준을 쓰고 stochastic는 rejection/sampling 수학·고정된 분포 검증과 정상 과제를 함께 검사한다. seed가 같다는 이유로 서로 다른 sampling 경로의 token 비트 동일성을 강요하지 않는다. MTP/DFlash/DSpark가 사용하는 recurrent/indexer 연산이 미승인이면 C는 S의 상태 계약까지 함께 만족해야 하며, 불안정한 target 위의 가속만 먼저 출시하지 않는다.

<a id="fresh-session"></a>

## 7. 컨텍스트 없는 새 세션의 시작과 인수인계

현재 첫 요청은 “개발 계획 §0의 p4hfadapter 수용 작업을 구현하라”다. HF 수용 완료 후에는 “Release A를 구현하라”로 이어간다. 새 세션은 AGENTS → 로드맵 전체 → 검증 규약 전체 → 격리 계약 → 문서 안내도 → 이 문서와 배치 코드 검토 순서로 읽는다. 문서가 가리키는 과거 실행 중 상태를 현재 프로세스 상태로 취급하지 않는다.

### 7.1 먼저 양쪽 기준 확인, HF 완료 후 A 재개

HF 착수 시 P4와 HF 양쪽에서 `git rev-parse HEAD`/`git status --short`를 확인하고,
`Test-Path F:/dev/p4/layers/adapters/hf/adapter/Cargo.toml`로 아직 없는 crate를 있다고 전제하지 않는다.
현재 HF 독립 빠른 시험은 그 저장소의 `python -B tools/testing/run.py`다. 실제 모델/변이 명령은 HF 모델 문서를 따른다.
HF 빌드/통합 명령은 crate/feature가 구현된 뒤 확정하며 예정 옵션을 실행하지 않는다.

다음 명령은 **HF 완료 뒤 A를 재개할 때의 기존 회귀 명령**이며 HF 구현에 앞서 A 전체를 실행하는 지시가 아니다.
아래는 현재 존재하는 명령이다. 저장소 루트의 PowerShell에서 실행한다. 정상 실기/원격 배포 명령이 아니다.

```powershell
Get-Location
git rev-parse HEAD
git status --short
git diff 245d6b785c96ec770dc6041ded35457c2ef97260 -- layers tools test
cargo test -p p4-llamacpp-staged-adapter --lib v2::node::worker::loop_tests::bounded_strategy::phase_pacing_actual_loop_expires_decode_wait_without_another_tail_or_input -- --exact --nocapture
cargo test -p p4-llamacpp-staged-adapter --lib v2::scheduler -- --nocapture
cargo test -p p4-llamacpp-staged-adapter --lib v2::node::worker::loop_tests::bounded_strategy -- --nocapture
cargo test -p p4-llamacpp-staged-adapter --lib v2::node::worker::loop_tests::service_budget -- --nocapture
npm run test:model-loading
node --test test/benchmarks/cluster-inference/*.test.mjs
```

먼저 timer 실패가 기능 결함인지, 정상 RELEASE가 관측 snapshot 사이에 들어온 시험 동기화 문제인지 event trace로 판별한다. 현재 재현은 `bounded_strategy.rs:283`, `next_event 57→58`, `free_sequences []→[1]`이다. 기대값/timeout을 임의 완화하지 않는다. 시험 수정이 필요하면 timer가 상태를 바꾸지 않는다는 본래 oracle을 유지하고, 잘못된 timer state mutation을 넣은 독립 변이가 실패함을 증명한다.

새 수용 runner의 구현 진입점은 `tools/event-drive/src/run/{config,acceptance,inference_evidence,teardown_preserves_failure_tests}.rs`다. 현재 CLI는 `cargo run -p p4-event-drive -- CONFIG.json ARTIFACT.json` 형태이나 현재 acceptance의 문자열 조건/최소 token 수만으로 H1/H5 전체를 판정할 수 없다. **A runner/corpus/manifest gate 작성도 개발 범위**다. 생성될 profile을 기존 파일처럼 실행하거나 현재 CLI에 미구현 cancel 옵션을 넘기지 않는다.

### 7.2 개발 중단과 완료의 정확한 기록

- 실행 소스에 차이가 있으면 관련 소비 경로·시험을 먼저 갱신하고 문서 기준 차이를 남긴다. 봉인 arm/사용자 변경을 덮어쓰지 않는다. 변이는 독립 checkout과 실제 재컴파일/hash로 결속한다.
- HF 수용 후 A 재개 시 timer RED 해소→A1–A4 실제 경로 반례/변이→관련 native/backend conformance→A5 실기 순으로 진행한다. A1–A4 내부 순서는 의존 관계에 따라 조정할 수 있고 별도 출시하지 않는다. 최종 `cargo test --workspace --no-fail-fast` 종료와 모든 summary를 집계한다.
- native는 현재 pin/27 patch manifest를 검사하고 해당 backend를 빌드한다. Windows 실행에는 검증한 동반 DLL을 함께 배포하고 hash/timestamp를 확인한다. 오래된 서버 binary를 새 Rust 결과와 섞지 않는다.
- 수치 예산은 검증 규약의 A 계약과 native PLAN/INSPECT 결과에서 구체화해 run 이전 봉인한다. 한계가 안 맞으면 INVALID/BLOCKED/FAIL로 기록한다. 미측정 성능을 승인으로 채우지 않는다.
- 실행 보고에는 code hash·명령·시험 ID·passed/failed/ignored/미실행·native/다중 host 구분·첫 오류/cleanup 오류·남은 작업·다음 첫 행동을 기록한다. 로컬 안전성 완료와 제품 출시는 분리한다.
- 원격 자원 사용은 개발 요청의 승인 범위에서 수행한다. 무관한 프로세스 종료·기존 봉인 arm 수정·push는 확대 권한이 아니다. 자원이 없으면 할 수 있는 로컬 작업을 마치고 필요한 host/artifact와 H gate를 구체적으로 남긴다.

## 8. 근거 보존과 현재 검증 상태

2026-09-14 HF 편성 감사는 양쪽 저장소를 읽고 P4 계획 문서만 수정했다. 아래 09-13 시험은 그때의 기록이며 HF 통합 결과가 아니다.

- 외부 덱 15장 전체 텍스트와 렌더링을 검토했다. 원문/이미지는 `target/external-slide-review-20260913/`에 보존했다. 첨부 희소 분석을 실제 pin/최신 upstream 상태와 나누어 검증했으며 확정 못 한 원인 주장은 §3에서 제외했다.
- 최신 upstream 검토 snapshot은 `002a12ad25503a93501b2e188c360029830a241a`, P4 pin은 `451b89bae0c4b1dd612eb503ceace906c01ddcc9`다. 최신 소스 5개는 `target/external-slide-review-20260913/upstream-002a12ad/`에 있다. DFlash/DSpark 사실은 P4 pin의 `common/speculative.cpp`·`src/models/dflash.cpp`와 공식 model 자료로 대조했다. 원격 master의 설명을 P4 실행 결과로 쓰지 않았다.
- Nemotron `test-progress.json` SHA-256은 `1FEF255A5A2DFCE443B4BF56B8CA052A167D300E0C83589ADDF7115ACA5AF60A`, mixed progress는 `9B14E2DCC5F2FC400843C186A91FC14629B53A05194903E61502E731C82C12D6`다. 100k 실패/혼합 미시작을 보존한다. 현재 원격 상태는 새로 확인하지 않았다.
- 배치 재검토의 현재 집계는 **44/1/0**이며 전체 workspace 집계가 아니다. timer 단독 실패 로그는 `target/external-slide-review-20260913/review-20260913/phase-pacing-alone.log`, SHA-256 `4d3e2a4979ac853b91998c3832d45603af615188b726d63d9a1a472cadb471ec`다. 최초 작성자의 45/45는 역사 기록으로만 보존한다. [배치 문서 정정](batching-code-review.md#review-correction) 참조.
- 저수준 공유 pool probe는 상위 planner의 최신 수정·합성 6,678건 비교와 구분한다. 합성 KV/runtime 수요는 실제 native tensor/할당/cut conformance를 대신하지 않는다.
- 이번 결과는 **개발 계획 완성**이다. A/SP/C의 새 시험 구현·새 모델 다운로드·GPU/원격 실기·성능 개선은 미실행이다. source/링크/EOL/docs-lint 확인은 문서 검사이며 제품 수용 증명이 아니다.

문서 마감 재확인: OUTER 이동 후 `npm run test:model-loading` 34/34 통과, timer 단독 시험 0/1로 RED 재현.
제품 소스/기존 시험은 수정하지 않았다. 문서 링크/anchor와 docs-lint 회귀 12/12를 확인했다.

2026-09-14 변경 검증: P4 문서 100개 docs-lint clean, docs-lint 회귀 12/12, 수정 문서의 파일/anchor 링크와 `git diff --check` 통과. HF HEAD/dirty는 읽기 전후 동일하며 Rust/Python/Cargo 구현이나 모델 실기는 변경·실행하지 않았다.
