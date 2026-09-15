# P4 Studio 관측 요구 수용안

2026-09-15. **설계 제안이며 구현·기본값 변경·성능 수용 기록이 아니다.**
사용자 요청에 따라 Studio의 관측 요구를 추가 트래픽·추론 비용·계층 책임 기준으로 정리했다.
현재 개발 순서와 재개 조건은 [로드맵](distributed-batching-roadmap.md#current-status),
시험·실기 승인은 [검증 규약](distributed-batching-verification.md),
책임과 의존 경계는 [격리 계약](layer-isolation-contract.md)이 소유한다.
이 문서의 후보 순서는 관측 기능 내부의 의존 순서이며 기존 개발 중단을 해제하지 않는다.

## 1. 분석 기준과 결론

- 관련 실행 경로 확인 기준: P4 `dde0813fb0adbcb43aa6dccb27e9b8a2675b85f2`.
- 입력: Studio의 `F:/dev/p4studio/docs/inference-observability.md`와 사용자가 제공한 구현·검증 보고.
  Studio 문서의 최초 P4 분석 기준은 `434bd97fc1e3e2d73cc5a0976119aa094682fb12`다.
  두 P4 기준 사이의 diff와 현재 telemetry 생산·전달·INSPECT 경로를 대조했다.
- Studio 배포·화면·시험 결과는 입력 보고이며 이번 P4 검토에서 다시 실행한 결과가 아니다.
- 소스 경로 확인은 배포 바이너리의 동일성이나 전체 저장소 재검증을 뜻하지 않는다.

수용 방향은 **상시 집계 + 작은 요청별 요약 + 제한된 상세 진단**이다.
요청·토큰·stage마다 상세 이벤트를 추가하는 방식은 기본 모드에서 제외한다.
새 운영 지표의 선택적 전달과 기존 OUTPUT·정산·해제·batch/span 증거 의무를 구분한다.
아래 수치는 실측 예측이 아니라 구현 시 검증할 초기 비용 예산 제안이다.

## 2. 현재 경로에서 확인한 사실

| 항목 | 근거와 해석 |
| --- | --- |
| 요청별 출력·배치·stage | [commands.rs](../layers/adapters/llamacpp/staged/adapter/src/v2/commands.rs)의 OUTPUT v5, BatchObservation/StageSpan v4와 [observe.rs](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/observe.rs)의 owner별 생산 경로. batch의 공유 시간을 요청 전용 compute 시간으로 합산하지 않음 |
| 발행 직전 snapshot | SchedulingSnapshot은 선택 순간의 pending/eligible/outstanding 등을 제공. ready_rows는 남은 prompt token도 포함하며 지속 대기 요청 수가 아님. idle_gated=0은 다른 blocker 부재를 뜻하지 않음 |
| forward 시각 | [effects.rs](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/effects.rs)의 PublicationAfter::Observed에서 고정. 로컬 completion mailbox 수용 시각이며 socket 송신 완료·원격 도착 ACK가 아님. end→forward는 결과 전달을 넘기기까지의 지연 |
| 관측 완결 | [검증 규약](distributed-batching-verification.md)과 [배치 계약](adapter-batching-layers.md)의 owner별 관측·발행 증거 완결을 따름. 기존 batch/span을 임의 샘플링하면 현 소비자 검증과 전달 의무를 깨뜨릴 수 있음 |
| INSPECT | [inspection/mod.rs](../entrypoints/agent/src/event_runtime/control/inspection/mod.rs)는 호출마다 hardware::observe를 spawn_blocking으로 실행. node·broker snapshot 시각과 나중에 완료되는 hardware probe의 실제 측정 시각을 동일하다고 가정하지 않음 |
| 자원 표본 | [hardware.rs](../entrypoints/agent/src/event_runtime/control/inspection/hardware.rs)의 provider별 지원 차이 유지. unified memory는 RAM과 별도 VRAM으로 중복 합산하지 않음 |
| native 비용 | [physical_cost.hpp](../layers/adapters/llamacpp/staged/server/src/runtime/physical_cost.hpp)의 P4_STAGED_TRACE_COST=1에서 parse/match/sample/encode, setup/decode/capture/post 등을 기록. [0029-cost-observation.patch](../layers/adapters/llamacpp/staged/compat/451b89bae/0029-cost-observation.patch)는 graph 실행·n_kv를 기록하며 명시 backend synchronize를 추가. 기본 비활성 유지 |
| 실제 transport | [transport.rs](../entrypoints/agent/src/event_runtime/transport.rs)의 encode→write_frame→local retire. 로컬 write 완료는 원격 수용·native 완료·KV 회수와 별개이며 실패 write는 자동 replay하지 않음 |

기존 native 비용은 GPU H2D/compute/D2H의 완전한 분해가 아니다. graph 시간에는 dispatch·내부 copy·
synchronization이 포함되고 native decode와 중첩된다. 두 시간을 합산하거나 pure kernel 시간으로 표시하지 않는다.
기존 [CPU 비용 관측](../tests/reports/release-a/20260914_050400.md)과
[550B on/off 비교](../tests/reports/release-a/20260915_011200.md)를 재사용하되,
후자의 E2E +0.851%는 고정 순서 한 쌍이므로 인과적 overhead나 1% 이내 수용의 증거가 아니다.

## 3. 항목별 수용 범위

| 항목 | 상시 수용 후보 | 선택적 진단 후보 |
| --- | --- | --- |
| 요청 lifecycle | 고정 크기 상태·시각·누적 대기 시간, 종료 시 요약 1건 | 모든 상태 전이의 상세 이력 |
| 대기 원인 | 원인별 현재 요청 수·누적 시간·횟수 | 요청별 원인 변경 타임라인 |
| KV·엔진 상태 | stage별 제공 가능한 용량·사용량·예약량·할당 실패 집계 | 요청별 block 배치·eviction/reuse 상세 |
| transport | 연결/edge별 누적 bytes·frame·오류, queue count/bytes, 대기 시간 집계 | execution별 enqueue/write/receive 시각 |
| native 단계 | 추가 동기화 없이 얻는 host 구간 시간 집계 | H2D/compute/D2H, graph·kernel profiling |
| 시계·수집 품질 | monotonic duration, 표본 시각·나이·누락·오류·schema/build identity | 정밀 시계 보정과 상세 분산 trace |
| 운영 export | 수집된 집계를 외부 exporter에서 변환 | request trace/exemplar 연결 |

### 3.1 요청 lifecycle과 대기

어댑터는 요청당 작은 레코드에 첫 수용·eligible·queue 진입·발행·출력·종결 시각과 원인별 누적
대기 시간, 현재 원인과 진입 시각을 보존한다. 공통 transport의 접수 시각과 어댑터 admission은 구분한다.
native-start는 실제 native 경계에서 관측할 때만 제공하며 Worker의 RPC 시작으로 대체하지 않는다.
취소·거부·실패로 중간 종결될 수 있으므로 모든 요청에 하나의 직선 lifecycle을 강제하지 않는다.

- 상태가 바뀔 때 대기 시간을 누적한다. polling마다 전체 요청/이력을 재순회하거나 진단 때문에 scheduler를 재실행하지 않는다.
- 원인은 capacity, outstanding, input dependency, coalescing, downstream backpressure, unknown으로 구분한다.
  실제 owner가 관측한 원인만 기록하며 상세 enum과 선형화 지점은 구현 전 계약으로 확정한다.
- 여러 blocker가 겹치면 대표 원인과 보조 원인을 구분한다. 대표 원인 선택 규칙은 고정하고 중복 시간을 E2E로 합산하지 않는다.
- node의 no-input과 특정 요청의 대기를 구분한다. 원인 분류가 불가능한 시간은 unknown으로 남긴다.
- 서버 첫 출력 준비/승인 시각과 client 첫 수신 시각을 구분한다. 사용자 TTFT는 실제 client send→첫 OUTPUT 수신이다.
- 종결 요약은 새로운 운영 관측이다. 전달 실패·누락이면 coverage를 낮추며 OUTPUT나 정산을 실패/성공으로 다시 판정하지 않는다.
  active·완료 대기 요약 저장소 모두 count/byte 한도를 두고 무한 이력 보존을 금지한다.

### 3.2 KV·엔진 상태

모든 backend를 total/used/free blocks에 맞추지 않는다. llama.cpp/HF와 attention/recurrent의 차이를
어댑터가 상태 종류·단위(bytes/cells/blocks)·측정 시각·지원 여부로 공개한다.

- 실제 할당량, 논리 사용량, 예약량을 분리한다. n_kv는 graph가 사용하는 KV 범위이며 전체 cache 사용량이 아니다.
- eviction/reuse/preemption/recompute/spill은 실제 구현과 성공 지점이 있을 때만 누적한다.
- 미지원은 null/unsupported, 지원하지만 미발생인 값은 0이다. broker receipt는 GPU KV가 아니다.
- 안전한 owner 경계의 기존 계수/캐시를 사용한다. 기본 모드에서 KV 내용 순회·GPU 복사·실행 중 native 강제 조회를 하지 않는다.
- 요청별 실제 귀속이 없는 공유 메모리를 요청별로 나눠 계산하지 않는다.

### 3.3 Transport와 시계

기존 인코딩·I/O 지점에서 계수를 추가한다. 계측만을 위해 payload를 다시 인코딩하거나 복제하지 않는다.

- payload bytes와 framing 포함 bytes, 송신 시도와 실제 local write, 완전 수신과 decode 실패를 구분한다.
- 부분 write/read 실패의 실제 byte를 알 수 없다면 불명으로 남긴다. 예정 frame 길이를 실제 송신량으로 세지 않는다.
- queue count/bytes와 local queue 재시도/network 재전송을 구분한다. 계측 때문에 retry·replay 의미를 바꾸지 않는다.
- 표시 단위는 P4 애플리케이션 전송률이다. TCP 재전송·헤더까지 포함한 NIC 전체 사용량은 별도 측정한다.
- 같은 host의 구간은 monotonic duration으로 기록한다. 다른 host monotonic timestamp를 직접 빼지 않는다.
- wall-clock anchor, clock source, offset/uncertainty의 측정 근거와 표본 나이를 함께 제공한다.
  RTT만으로 정확한 편도 지연을 확정하지 않는다. 근거가 없으면 unknown이다.
- host 간 세부 overlap 수용은 기존 검증 규약의 clock/coverage 조건을 따른다. 음수 지연을 0으로 보정해 성공으로 만들지 않는다.

### 3.4 수집 품질

emitted/dropped/sampled/overwritten/export-failed/parse-failed, 마지막 성공 표본 시각, 실제 수집 주기와
수집 소요 시간을 생산자·전달자·소비자별로 구분한다. sampled와 비의도적 loss를 합치지 않는다.
counter에는 process/epoch와 reset 정보를 결속하고, 재시작 차이를 음수 rate나 폭증으로 표시하지 않는다.
시계·수집 품질은 첫 묶음에 포함한다. 빈 그래프를 무부하로 추정하지 않는다.

## 4. 수집·전달 구조와 비용 예산

1. agent마다 수집기를 하나만 둔다. Studio backend가 받은 자료를 브라우저들에 배포하며 브라우저별 agent polling을 피한다.
2. 변경이 적은 capability와 실행 중 occupancy를 분리한다. 기본 후보는 운영 화면 활성 시 1초, 비활성 시 10초 집계다.
3. INSPECT는 캐시를 읽는다. 느린 probe는 중복 실행하지 않고 이전 표본과 sample_age/probe 상태를 반환한다.
   provider별 timeout과 실패 후 재시도 간격을 제한한다. 이 주기가 실기 증거 수집 규약을 자동 완화하지 않는다.
4. 누적 counter를 전송한다. 중간 snapshot 누락 뒤 총 증가량은 복구할 수 있지만 순간 peak·시간 분포까지 복구됐다고 표시하지 않는다.
5. 새 상세 trace는 기본 비활성이다. 활성 시 대상·기간·sample 규칙·count/bytes·송신량을 고정한다.
   선택한 request/execution의 연결 규칙을 유지하고 partial trace를 complete로 표시하지 않는다.
6. 현재 native 비용 flag는 프로세스 내 static 설정이다. 즉시 on/off할 수 있다고 약속하지 않으며 별도 측정 arm의 안전한 LOAD 경계에서 적용한다.

### 초기 기본 모드 예산

| 항목 | 제안 목표 |
| --- | --- |
| 추가 telemetry 송신량 | agent당 16 KiB/s 이하, envelope/framing 포함 |
| 부하 중 추가 트래픽 | 각 공유 링크에서 기존 비관측 P4 트래픽의 0.5% 이하 |
| 저트래픽·idle | 비율 대신 10초 주기와 절대 byte 상한 적용 |
| 유효 생성 TPS 저하 | 1% 이내 |
| TTFT·ITL p95 악화 | 각각 2% 이내이며 기존 SLO도 충족 |
| 메모리 | 요청·stage·edge 수에 따른 고정 count/byte 예산. 실행 시간에 비례한 증가 금지 |

두 트래픽 한도 중 먼저 도달하는 쪽을 적용한다. 새 snapshot·요청 요약·선택적 trace의 합계를 제한하며
장치·node·edge 수가 많다고 상한을 자동 확대하지 않는다. 불충분하면 상세도를 낮추고 coverage에 반영한다.
구현 전 profile에서 부하/저트래픽 구분, 측정 window, burst 용량, 메모리 정수 상한을 봉인한다.
목표가 노이즈보다 작아 판별 불가하면 PASS 대신 미판정으로 남긴다.

설명용 계산: 8 stage × 초당 100회 실행 × 1 KiB 상세 이벤트는 단일 전달만으로 800 KiB/s다.
stage당 4 KiB의 1초 집계는 32 KiB/s다. 이는 추가 지표의 표현 비교이며 기존 batch/span 대체나 실측 절감치가 아니다.

전체 traffic 보고에서는 다음을 분리한다.

- 기존 payload/control/output, 기존 필수 관측, 새 운영 관측, probe/export의 bytes와 CPU/RSS.
- 실제 edge/반환 경유마다 발생한 송신과 fan-out. 같은 링크의 송신·수신을 중복 합산하지 않는다.
- agent→Studio와 Studio→브라우저 전송. 브라우저 수에 따른 전체 비용을 숨기지 않는다.
- 생성률뿐 아니라 요청 도착률에 비례하는 종결 요약 비용. 동시성 50을 초당 요청 50으로 간주하지 않는다.

Studio에서 수신 후 데이터를 줄여 저장하는 것만으로 P4 송신 traffic은 줄지 않는다.
기존 필수 관측 자체가 비싸면 별도 버전의 증거 계약·압축/묶음 전달을 검토하되 현재 검증을 우회하지 않는다.

## 5. 관측 포화와 책임 경계

hot path에서는 작은 계수·시각만 갱신하고 직렬화·export를 분리한다. 추가 GPU 동기화, payload 복제,
전체 이력 scan, exporter 응답 대기를 기본 모드에서 금지한다.

- 새 snapshot은 최신 값으로 합칠 수 있고 선택적 trace는 예산 초과 전에 생성을 생략할 수 있다.
- 생략·덮어쓰기·전달 실패를 공개한다. 선택적 관측의 유실을 추론 성공·정산 성공으로 해석하지 않는다.
- 기존 OUTPUT·정산·해제·필수 batch/span을 관측 예산 때문에 폐기하지 않는다.
- 이미 전달 의무가 생긴 이벤트는 기존 보존 계약을 따른다. 새 관측 버퍼와 송신 예산을 기존 inference 예약과 분리한다.
- 현 큐에 telemetry를 더 넣고 낮은 우선순위라고 부르는 것만으로 격리가 성립하지 않는다.
  수집기 단절·느린 소비·관측 버퍼 포화에서도 요청·원장·예약·credit·출력의 보존과 정상 진행을 실제 경로에서 검증한다.

| 소유자 | 범위 |
| --- | --- |
| P4 공통 | transport·broker·노드 전달 상태·시계·수집 품질 |
| 구상 어댑터 | 요청 수용/발행 lifecycle·scheduler 대기·KV·native 의미 |
| Studio | client TTFT/E2E/ITL·SLO·장기 보존·차트·요청별 결합 |
| Exporter | Prometheus/OTel 변환 |

공통 계층은 어댑터 상태 문자열을 파싱해 queued/KV full을 판단하지 않는다.
공통 schema/trait 승격이 필요하면 llama를 모르는 mock/두 번째 adapter로 독립 의미와 비용을 검증한다.
현 batch/span의 strict schema에는 새 필드를 무조건 삽입하지 않는다. 새 content-type 또는 협상된 버전,
producer/consumer fixture, 구형 생산자의 unsupported 처리를 함께 설계한다.

Prometheus label에는 request/session/execution ID를 넣지 않는다. node/stage/reason도 활성 집합과
series 수를 제한하며 합칠 수 있는 histogram을 사용한다. 요청 ID는 trace/history/exemplar에서 연결한다.
지침은 [Prometheus instrumentation](https://prometheus.io/docs/practices/instrumentation/)을 참고한다.

## 6. 후보 구현 묶음과 검증

이 절은 미실행 계획이다. 실제 착수와 기존 Release A 잔여 작업의 관계는 로드맵에서 결정한다.

1. INSPECT 캐시·probe 중복 방지·표본 시각/품질.
2. transport bytes·queue·오류 집계.
3. 어댑터 대기 원인 집계와 요청별 작은 요약.
4. 안전하게 읽을 수 있는 KV/엔진 상태 집계.
5. 후속 선택 기능: 제한된 request trace, native 단계 분해, Prometheus/OTel exporter.

구현 전 실패 반례·실제 소비 지점·정상 진행 oracle·독립 제거 변이를 고정한다.
필수 회귀에는 중복/역순/재시작, 부분 송신/수신 실패, probe timeout, 여러 브라우저,
수집기 단절·느린 소비·관측 count/byte 경계±1, 취소·거부 뒤 상태 보존을 포함한다.
poll 때만 갱신하는 가짜 대기 시간, 예정 bytes를 실제 송신으로 세는 계수, 필수 관측의 조용한 누락,
관측 실패가 native 재실행을 만드는 변경을 검출해야 한다.

성능 비교는 같은 소스·바이너리·모델·토폴로지·정책·워크로드에서 추가 계측 off/on을 비교한다.
기존 필수 관측은 양쪽에서 유지한다. 짧은 decode, 긴 prefill, 혼합 연속 웨이브를 포함하고
순서·열·cache 영향과 신뢰구간을 보고한다. 반복 수와 최종 다중 컴퓨터 판정은 검증 규약을 따른다.
유효 TPS의 token/시간 분모, 실제 client 송신 시각, 정상 응답·종결·회수, GPU 분석 window,
telemetry coverage, 최초 오류와 cleanup 오류를 함께 남긴다. 실패 baseline으로 개선율을 승인하지 않는다.

## 7. 보관 시점의 검증 경계와 다음 행동

이번 산출물은 제안 문서와 README/문서 안내도 색인이다. runtime·wire·기본값·배포를 변경하지 않았다.
문서 형식/색인 검사는 구현 의미·성능 검증이 아니며 전체 cargo/native/실기 시험을 다시 수행한 결과도 아니다.

후속 개발의 첫 행동은 HEAD/dirty와 로드맵 중단 조건을 다시 확인하고, 관측 profile·buffer/byte 예산·
schema/소유 계층·실제 소비 시험을 확정하는 것이다. 상시 기본값 승격은 위 비용 예산과 기존 안전성·
실기 수용을 만족한 뒤에만 판정한다. 현재 단계에서 16 KiB/s·0.5%·1%·2% 달성을 주장하지 않는다.
