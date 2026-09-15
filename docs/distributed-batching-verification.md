# 분산 배치 검증·실기 수용 규약

2026-09-06 신설. **아래 T/I/K/H 시험은 요구사항이며, PASS 기록이 없는 항목은 미구현/미검증이다.**
현재 상태와 실행 순서는 [로드맵](distributed-batching-roadmap.md)이 소유한다.
이 문서는 시험의 입력·판정·증거·금지 사항을 소유한다. 역사 문서의 느슨한 기준으로 대체할 수 없다.

## 1. 게이트의 의미

| 층 | 증명하는 것 | 증명하지 못하는 것 |
| --- | --- | --- |
| T: 결정론적/가짜 stage/통합 | 상태 전이, boundedness, 반환 정합, 공정성, 실패 수렴 | 실제 모델 품질·GPU 병렬 실행·실기 TPS |
| Native conformance | 선언 모델/메모리/backend 조합의 실제 연산·상태·분할 계약 | 강한 서비스 웨이브에서의 전체 성능 |
| H: 실제 다중 컴퓨터 웨이브 | 정상 응답을 내는 초대형 분산 모델의 유효 처리량·GPU 활용·지속 동작 | 다른 모델/장치/링크 또는 무한 탐색에서의 전역 최적 |

T를 통과하지 않고 H로 수정 방향을 찾지 않는다. H를 통과하지 않고 제품 목표를 완료했다고 하지 않는다.
프로세스 READY, HELLO 일치, 40/40 terminal, U+FFFD 부재, RPC peak, 평균 GPU util 각각만으로 성공할 수 없다.

## 2. 모든 수정에 적용할 시험 제약

1. 실패 반례를 먼저 저장한다. 기존 코드에서 실패하는지, 원하는 계약에서 왜 실패해야 하는지 적는다.
2. 정상 경로와 부정 경로를 함께 둔다. 거부만 늘려 모든 일을 막는 구현도 실패해야 한다.
3. 값의 불변식은 독립 계산한다. 구현이 만든 counter끼리 같은 식으로 비교하는 항등식을 피한다.
4. 공유 함수를 직접 부르는 단위 시험과 실제 소비자(worker/simulator) 도달 시험을 별도로 둔다.
5. 각 critical fix는 수정 제거 또는 동등한 fault mutation으로 실패해야 한다. **mutation이 통과하면 해당 시험은 게이트로 승격 불가**다.
6. 테스트 전용 fault 주입은 runtime public API를 넓히지 않는다. 사용자의 checkout에서 git checkout/reset으로 변이 복원 금지.
7. 기아는 모든 요청의 진행량과 첫 선택·선택 사이·마지막 선택 이후를 검사한다. ready 수가 capacity보다 작음/같음/큼을 전부 포함한다.
8. 가상 시간 분할 시험은 분할 시점에 실제 in-flight가 남고 두 번째 구간이 새 trace를 만드는지 먼저 단언한다.
9. 오류 검사기는 오류 위에서 무한 실행하거나 전체 이력을 매 tick 재순회하지 않는다. 고정 tick/메모리 상한과 주입 시험을 둔다.
10. async/worker 시험은 wall timeout, join 완료, 잔여 큐/원장, retry/wake 횟수 상한을 함께 본다.
    `yield_now`도 통과하는 `attempts > 1`만으로 backpressure를 증명하지 않는다.
11. 골든 변경은 독립 의미론/위치/토큰 상한 반례가 먼저다. 실제 출력 복사만으로 골든을 재작성하지 않는다.
12. 필수 시험의 runtime early-return을 pass로 세거나 ignore/feature로 숨기지 않는다. 외부 의존 시험은 opt-in을 표시하고 명시 요구 시 부재를 실패로 처리한다.
13. 시험 실패를 고치려다 다음과 같은 대체 완료를 만들지 않는다: 정상 요청 축소, judge 완화, 동시성 축소, 노드 축소, 강제 재시작, timeout 연장만으로 통과.
14. 코드가 공유됐는지와 시험이 양쪽 경로를 보호하는지는 별도 주장이다. 파일 위치나 테스트 이름으로 시험 수준을 분류하지 않는다.
15. 변이마다 실제 source digest·빌드 입력·시험 바이너리를 결속한다. 별도 Cargo target/build directory를 쓰거나
    해당 변이의 재컴파일을 확인한다. 공유 캐시가 baseline 바이너리를 재사용한 실행은 통과/실패 증거 모두 무효다.
16. 거부 시 effect queue의 길이뿐 아니라 각 의도의 대상·본문·순서와 native 호출 계수도 비교한다.
    receipt 보존 창의 상한은 active flight/queue/RSS의 상한을 대신하지 않는다.
17. 제어 명령 여러 개의 예산은 각각 검사한 결과를 합계 승인으로 해석하지 않는다. 각개로는 통과하지만
    전체는 넘는 입력에서 첫 native 호출도 0이어야 한다. 뒤 연산의 예정된 영수증 축소를 앞 연산의 여유로
    미리 쓰지 않는다. 같은 operation의 단독 재시도/정확한 replay는 여전히 진행해야 한다.
18. wire 검증을 강화한 뒤 기존 부정 시험이 encoder에서만 실패하면 worker 회귀가 아니다. 정상 codec으로
    만든 bytes를 명시 변조하거나 의미적으로 유효한 다른 신원을 구성하여 실제 소비자에 도달시킨다.
    새 guard에 막혀 원래 short/count/role fault까지 못 간 시험을 이전 fault의 PASS로 보고하지 않는다.
19. 착수·첫 반례 실행·수정 후·커밋 전·실기 전마다 시행착오 회귀를 점검한다. 상태/소유/용량의
    인과 경로, 반증할 가설, 기대 관측과 정상 진행 조건, 변경 범위를 먼저 적는다. 예상 밖 실패가
    나오면 기존 가설을 재심사하고 그 입력을 보존한다. 설명 없이 다음 손잡이/계층을 바꾸거나,
    기대값을 관측값에 맞추거나, 같은 질문에 답하지 않는 시험·도구를 늘리지 않는다.
    코드상 도달 반례와 실행된 RED, 국소 GREEN과 전체 제품 완료를 각각 구분해 보고한다.
    전체 시험이 읽는 문서·fixture·검사 스크립트도 입력이다. 경량 형식 검사를 먼저 끝내고
    실행 중 입력을 편집하지 않는다. 코드 봉인만으로 문서를 포함한 전체 검증의 봉인을 주장하지 않는다.
20. 이번 후속 작업은 사전 고정한 검증 묶음의 반복을 최대3라운드로 제한한다. 개별 시험3개로
    축소하거나 H의 필수 반복 표본을 줄인다는 뜻이 아니다. 첫 회에는 해당 수정의 정상·거부·포화·
    복구 조건을 모두 넣는다. 둘째 회는 첫 실패의 인과 설명과 그에 필요한 수정만 허용하고,
    critical fix의 제거 변이까지 검증한다. 셋째 회는 봉인 후보의 최종 확인이다. 새 원인/설계
    변경이 필요하거나 필수 검증이 남으면 완료를 주장하지 않고 재설계 필요로 판정한다.
    묶음·입력·기대값을 실행 중 바꾸거나 실패를 제외하여 횟수를 맞추지 않는다. 라운드별로
    실행 명령·변경 이유·남은 횟수를 기록하며, 통과했다면 불필요한 둘째/셋째 반복은 하지 않는다.

## 3. 결정론적 필수 시험 목록

아래 ID는 단계별 PASS 표와 실행 로그에서 재사용한다. 한 시험이 여러 ID를 덮으면 각 입력/단언을 표시한다.

### T00~T04: 기준과 반례

| ID | 입력 / 반드시 검사할 것 |
| --- | --- |
| T00 | source/runtime/build/model/workload/summary 식별 불일치, git 실패·미추적·binary 변경·기록 채널 실패를 clean/success로 해석하지 않음 |
| T01 | 현재 event 경로의 호출 지도를 검증; 과거 Chain/Hop 시험 결과로 대체하는 보고 거부 |
| T02 | 현행 정상 seed: prompt 길이 0/1/여러 chunk, max_tokens 1/2/긴 생성, 첫 token과 decode 위치 연속성 |
| T03 | 기준 suite 전체 실행·exit code·pass/fail/ignored/feature 제외 집계, 외부 fixture 부재와 기능 실패 구분 |
| T04 | R-A/B/C 반례를 저장소 시험으로 이관; 임시 로컬 probe만으로 닫지 않음 |

### T10~T19: 발행과 정산

| ID | 입력 / 통과 조건 |
| --- | --- |
| T10 | 4행 issued/6행 반환, 열린 배치 등록. 거부 후 request·open_batches·credit·예약·출력 의도 모두 변경 없음. tail뿐 아니라 handle의 후속 지속 상태도 검사 |
| T11 | 혼합 A 정상/B 잘못된 반환, 요청 순서 교환, 뒤늦은 outcome/sequence 오류. 이벤트 전체 거부 시 A도 그대로; 외부 부작용 0 |
| T12 | F1 정상 정산→F2 발행→F1을 새 event ID로 재전달. F2 outstanding/position 보존; 원장과 요청 일치. 동일 내용 no-op/receipt, 다른 내용 conflict |
| T13 | outcome 없는 partial prefill: wrong sequence/key/generation/phase/position range, 미등록 execution, 중복/누락 row, owner/invocation 불일치 전부 사전 거부 |
| T14 | 한 논리 batch가 여러 physical capsule로 분할. 일부 반환만으로 batch/sequence 해제 금지; 마지막 유효 반환 한 번만 완료. ordinary 분할은 허용하되 Verify/Replay atomic group의 capsule 간 분할·내부 재정렬은 거부 |
| T15 | plan 생성 후 취소, reserve 일부 실패, stage accept 전 실패, accept 후 응답 유실. planned/issued/settled 구분과 Uncertain 수렴; 이중 발행/rollback 누락 0. 실제 drive의 거부→재계획에서 fairness/cohort resume도 소비되지 않음 |
| T16 | prompt 마지막 fragment가 첫 token을 생성, max_tokens 1에서 추가 decode 0. decode position은 1씩, verify/replay는 별도 명시 계약에 따른 위치/생성량 |
| T17 | Simulator 실제 advance에 malformed travelling fragment 주입. worker와 같은 거부와 상태 보존. simulator의 공유 호출을 별도 부기로 바꾸는 변이가 실패해야 함 |
| T18 | ID wrap/exhaustion, 같은 슬롯 재사용과 이전 generation 반환, cancellation 후 late completion. 같은 load/session/request key/slot을 재사용한 새 incarnation에 옛 RELEASE/RELEASED/SETTLE/SETTLED를 재전달해도 모든 stage의 새 KV·예약·슬롯 보존. head nonce만으로 승인하지 않음. 동일 operation 다른 body/kind·퇴역 incarnation·watermark 상한도 검사 |
| T19 | issued rows = settled rows + travelling rows + 명시 취소/실패 회계. request counter와 식별 원장 독립 대조; 오류 cap과 증분 검사 mutation |

### T20~T28: 실제 워커와 가짜 stage

| ID | 입력 / 통과 조건 |
| --- | --- |
| T20 | 실제 event ingest→worker loop→stage API→capsule→tail→release→output. N=1/2/4/8; 직접 tail만 호출하는 시험과 구분. producer가 input을 계속 채워도 issue/completion/control 각각에 유한한 처리 기회가 옴 |
| T21 | max-open 1/2, 다중 capsule, 부분/마지막 반환. 실제 두 번째 issue가 막히고 해제되는 시점 확인 |
| T22 | 입력 queue Full: 이벤트 보존, 공간 통지 후 정확히 한 번 전달; 빈 완료 mailbox에서 spin/영구 대기 없음 |
| T23 | 출력 mailbox Full/Closed, broker/network 포화: 계산한 token/terminal 유실·중복 0, control 진행 보장 |
| T24 | timeout/reconnect/중복/역순/부분 메시지. physical phase/range 순서 위반을 정산으로 숨기지 않음. downstream 중복 physical 전달에서 native KV/sampler 효과는 첫 승인 외 0; mutating native의 정상 opcode+잘못된 body도 Uncertain/fence 후 추가 실행 0 |
| T25 | load/unload/cancel/release와 decode 경쟁. native 사용 중 삭제 금지, 모든 stage release 증거 전 슬롯 재배정 0 |
| T26 | shutdown while input full/output full/stage blocked. deadline 내 join, 명시 완료/취소/불확실 상태, 무한 drain과 유실 은폐 금지 |
| T27 | per-host 공유 GPU/독립 GPU/느린 stage 모델. dispatch 수와 device 동시성 구분; fake latency를 GPU 성능 증거로 사용하지 않음 |
| T28 | repeat run without agent restart, 부분 실패 후 재사용. 원장/메모리/queue가 선언 steady bound로 복귀 |

T23의 양방향 포화는 단일 출력 큐 복구 시험으로 대체하지 않는다. 실제 broker에 연결된 capacity 1의
EventNode 두 개가 각각 상대에게 보낼 completion을 보유한 상태에서 정상 broker 입력을 채운다.
adapter가 입력을 받을 수 있다면 outbound Full이어도 inbound를 처리해 양쪽이 진행해야 한다.
adapter도 Full인 경우 held input/output 각각 1개 및 다음 bounded queue의 잔존 이벤트를 측정하고,
추가 소비·폐기·중복 없이 공간 복구 후 전체 Event 바이트/순서가 보존되어야 한다. 이 국소 의무를
완전 포화 순환망의 교착 해소 증명으로 확대하지 않는다. 그 판정에는 end-to-end credit/제어 용량이 필요하다.
정리 중 이미 발생한 assertion에 두 번째 panic을 더해 원래 실패를 가리지 않는 것도 시험의 조건이다.

T23의 **동일 worker 내부 제어 진행**도 독립 의무다. 실제 두 stage의 A를 native 해제까지 진행하고
정확한 RELEASED를 보류한다. B의 실제 terminal로 capacity 1 completion을 채운 뒤 A ACK의 입력
수용을 확인한다. 출력 공간을 열기 전에 A 정산이 진행해야 한다. 그 뒤 공간을 복구해 기존 A/B의
token/text/position/stop·native KV·OUTPUT/해제 영수증·관측이 정확히 한 번 완결되는 양성을 유지한다.
현재 반례의 시험명은 `completion_full_cannot_starve_a_genuine_release_acknowledgement`다.
snapshot에 새 Full이 관측되고 실제 B OUTPUT이 큐에 있다는 것을 함께 확인하며, 200ms 대기만을
포화 증거로 세지 않는다. 이 시험의 제한 시간은 회귀 창이지 실시간 SLO나 CPU 무스핀 증명이 아니다.

양보 가능한 pump는 위 정상 ACK 시험과 함께 다음을 통과해야 한다. pending만 설치됐거나 local
native만 적용됐고 아직 제어 Forward가 수용되지 않은 조기 ACK는 whole-event 거부한다. 거부 뒤
pending/slot/Verify fence·native 호출·효과 대상/본문/순서는 모두 보존한다. 오래된 effect 성공 통지를
다른 incarnation/operation에 적용하거나 native replay를 ForwardAccepted로 간주하는 변이도 검출한다.
SETTLED의 정상 terminal proposal과 checkpoint replay를 유지하고, command 내부의 native loop에
다른 제어가 끼어들지 않는지 검사한다. 효과/보류 입력의 count와 byte 합계 상한, 반복 오류/SESSION,
Full→복구, Closed, ID 고갈, shutdown, fatal ERROR 보존은 각각 시험하며 모든 일을 막아 통과하지 않는다.

head 제어 단계의 회귀는 두 경계로 나눠 기록한다. `control_progress_tests`는 pending 상태를
구성한 뒤 실제 codec→Worker::handle의 whole-event 거부와 동일 ACK의 정상 재개를 검사한다.
`control_dispatch_effect_tests`는 load/session/KV 초기 상태를 구성한 뒤 실제 flush_effects→
native Frame/P4ID→receipt/frontier→completion 수용을 검사한다. native 변경 후 응답 손실·변조,
cached replay, Closed/ID 고갈, 잘못된 마지막 구성원도 포함한다. 앞 시험의 handcrafted
ForwardAccepted를 실제 전송 이력으로 세거나, 뒤 시험을 Worker::run/실제 llama의 증명으로
세지 않는다. phase 검사/성공 hook 제거·조기 승격·replay 퇴행 변이는 기대값 변경 없이 실패해야 한다.

SESSION의 사전 응답 준비는 실제 session()/handle()에서 다음을 별도로 검사한다. ID 고갈 시
session/ID/effects 불변, wire-valid 입력이 만든 합산 envelope 초과 시 SESSION 권한 불변,
정상 응답의 exact wire·ID1회 소비·Unicode 원문 보존이다. 일반 ERROR fallback이 아직 같은
큰 원본 ID로 수신 불가능한 진단을 만들 수 있다는 점을 정상 거부 통지 성공으로 오인하지 않는다.
준비 전 설치·ID commit 누락/중복·decode preflight 제거 변이를 검출하고, 미이관 LOAD/UNLOAD/
ERROR나 전체 count/byte 예약·Full 진행까지 이 SESSION 시험의 통과로 확대하지 않는다.

효과 표현 이관에서는 큰 TAIL 본문을 가진 동일 입력으로 OUTPUT/관측/제어의 exact Event를
비교하고, provenance가 payload를 소유하지 않는 타입인지 확인한다. 송신 실패 뒤 남은 본문과
중첩 telemetry의 내용·순서·소유 allocation을 검사하며, 성공 Forward를 후속 observation 실패로
다시 실행하지 않는다. front clone 제거를 단순 시험 개수 증가나 RSS 감소 추정으로 승인하지 않는다.

예산 회귀는 일반 영역을 정확히 채운 상태에서도 미리 승인된 ACK가 자기 미래 통지/ID 예약으로
진행하는 양성과, 그 예약이 없는 새 작업의 사전 거부를 함께 검사한다. ID 여력 경계와 합계 초과,
한 명령의 일부만 예약 성공, 여러 OUTER 그룹, 작은 payload/큰 capacity, telemetry fan-out,
LOAD/UNLOAD/오류 응답 수명을 포함한다. 공간 통지를 예약으로 취급하거나 ACK 이전 예약을 제거한
변이는 실제 소비 경로에서 실패해야 한다. 국소 ACK 시험과 FIFO/broker 전체 포화 시험을 합산해
한 증명으로 부르지 않는다.

캡슐의 선언 개수 검사는 작은 무효 wire의 실제 decoder에서 Vec 예약 **이전** 거부를 검사한다.
outcome 헤더/생성 토큰의 최소 wire 크기와 현재 cursor 잔량으로 불가능한 선할당을 막되 기존
유효 count 한도를 임의 축소하지 않는다. 거대 count 부정 시험은 allocator 호출 전 테스트 전용
안전장치로 중단시킬 수 있어야 하며 개발 호스트의 OOM을 검증 방법으로 쓰지 않는다. 원본 payload
길이, 파싱 객체 capacity, 일시 직렬화 복사, allocator overhead와 전체 RSS는 다른 계수다.

아래 독립 completion 시험은 2026-09-07 중단 시점에 **작성만 된 미실행 후보**다. 재개/실행 예산은
로드맵 §0을 따르며, 이미 GREEN이거나 지금 실행하라는 뜻이 아니다.
T22/T23의 독립 completion 진행은 실제 EventNode와 실제 목적지 슬롯으로 검사한다. 다른 correlation은
슬롯 확보 뒤에만 원본을 제거하고, 같은 source/correlation은 목적지가 달라도 앞지르지 못한다.
Full·front 불일치는 원본 allocation·원장·슬롯을 보존하며 terminal은 기존 양방향 보류물과 후보를
모두 반환한다. broker는 중복/충돌, eviction 중 receipt pin, 잘못된 Envelope·sequence의 거부 우선순위,
세대·채널 변경과 예약 취소를 따로 검사한다. actor 예방 분기는 head 미poll·목적지 capacity0을
유지한 채 genuine R의 전체 pending identity와 정상 C1의 실제 OUTER 전달을 함께 증언해야 한다.
14입력·6결과·8응답·native·최종 normal_progress 판정은 줄이지 않는다. 독립 진행 제거, 같은 순서
영역 검사 제거, front 일치 검사 제거 변이를 별도 checkout/실제 재컴파일로 검출한다.

T22/T23의 **중립 공간 통지**는 위 actor 시험과 구분한다. listener 등록→실제 offer 재시도 순서를
고정하고 등록 전/중/후의 drain·close를 배리어로 검사한다. 마지막 publisher 종료는 빈 reader의
Pending을 깨워 Closed로 만들며, buffered Event는 먼저 소비돼야 한다. 마지막 receiver 종료는
기다리는 publisher를 깨우고 원본 Event를 Closed로 돌려준다. callback의 재진입·자기 등록 해제·
다른 waiter 등록, 마지막 clone의 수명, listener 상한과 해제 후 용량 복구, lock 밖 wake를 검사한다.
notify를 지우거나 실제 close 전에 wake하는 변이가 실패해야 한다. 이 primitive가 존재하는 것만으로
현재 staged worker의 1ms 대기나 EventNode 입력 재시도가 제거됐다고 보고하지 않는다.

T20의 지속 유입은 잠깐의 두 웨이브 합류와 분리한다. 이미 runnable 요청을 먼저 둔 뒤 독립
Tokenize fake가 다음 유효 PREFILL을 입력에 공급해 queue nonempty를 인과적으로 유지한다.
연쇄 0/16/256 및 더 긴 입력에서 첫 issue까지 처리한 이벤트 수가 선언된 유한 기회 계약을
지켜야 한다. 모든 입력을 비운 뒤 결국 완주했다는 것만으로 통과하지 않는다. 같은 방식으로 발행이
계속 가능한 동안 tail/control 처리 기회도 검사한다. 이 계약의 quantum은 actor 공정성 상한이며
최적 TPS 값이나 blocking native 호출의 wall-time deadline이 아니다.

T20의 speculative 경로는 actual run에 전량 수용·직접 부분 수용·checkpoint Replay를 각각 넣는다.
native 경계의 literal 토큰·위치·생성량·KV append/trim/restore/reappend를 독립 대조하고, ordinary
위치당 1회 append oracle를 느슨하게 바꾸지 않는다. atomic group 분할과 ordinary 분할은 구별한다.
SETTLE은 stage를 순회하는 체인이며 최종 SETTLED는 꼬리에서 head로 오는 한 응답이다. 마지막
SETTLE 홉과 최종 SETTLED를 따로 보류하고 각 stage의 실제 효과를 확인한다. 현재 global Verify
barrier의 시험에는 별도 runnable 요청도 두어 단지 대상 요청의 ready 부재로 멈추는 위양성을 막는다.
전량 수용 뒤 barrier 해제·Replay 출력 누락 변이를 각각 검출하며, 전 stage release·새 incarnation을
대조한다. RELEASED까지 재사용이 없다는 관측과 free-slot 반환 기전만의 독립 증명은 구분한다.
토큰/KV fake의 통과를 실제 llama logits·sampler·checkpoint byte의 정상성으로 승인하지 않는다.

T20의 OUTER 경계는 실제 Worker::run의 OUTPUT과 실제 InferenceIdentity/drive 소비를 함께 검사한다.
ordinary 및 checkpoint Replay의 encoded Event를 보존하고, 현재 producer도 같은 의미 투영과 일치해야 한다.
투영에서 제외 가능한 실행별 event_id/causation_id/sequence 값은 이유를 명시하고, 실제 집합의 ID 유일성·
causation 존재·source별 sequence 증가·Event 유효성을 별도 검사한다. source/target/return route의 전체
endpoint, protocol/class/content/adapter/correlation/deadline, Outcome 모든 필드는 임의 정규화하지 않는다.
서로 다른 요청의 도착 순서와 한 요청의 token 순서는 구분한다. 꼬리/중간/다른 head·오래된 세대·잘못된
route를 거부하며, 같은 Event 중복 및 terminal 뒤 출력은 실제 drive에서 검사한다. 단순 수신 helper와
전체 drive, in-memory wire와 실제 network·GPU 범위를 각각 표시한다. producer를 tail 발행으로 바꾸거나
consumer를 tail-only/any-node로 바꾸는 변이 모두 실패해야 한다.

T20의 완료 판정은 출력 발행자 검사와 별개다. 다음은 필수 수용 조건이며 현재 구현 완료 선언이 아니다.
실제 drive와 최종 acceptance 양쪽에서 요청별 제출 상한(빈 EOS도 sampled token에 포함), 허용 terminal,
length 종료와 정확한 상한의 관계를 검사한다. 첫 출력 위치는 전 요청 공통 선택값만으로 대신하지 않고
각 요청의 토크나이즈/승인된 프리필 경계 증거에 결속한다. 서로 다른 프롬프트 길이, 상한 1에서 2개 출력,
요청 하나의 모든 위치를 같은 값만큼 이동한 반례를 포함한다. 별도 관측의 누락·중복·역순도 다룬다.
해제 완료는 total의 덧셈만으로 판정하지 않는다. 실제 승인된 요청/시도/해제 operation의 집합과 대조하고,
fresh event ID를 가진 중복 통지가 다른 요청의 해제를 대신할 수 없어야 한다. 하나의 정상 통지가 여러
요청을 해제하는 양성 대조도 유지한다. correlation만 중복 제거하는 임시 수리로 그 대조를 깨지 않는다.
시험의 해제 oracle는 payload의 자기 보고 count와 독립이어야 하며, scalar-only 기존 wire로 증거가
불가능하면 adapter 명령 버전을 명시적으로 바꿔 fail-closed 소비를 검사한다. P4 transport에는 모델
전용 완료 의미를 넣지 않는다. source 승인·terminal 출력·전 stage KV 해제·OUTER 수신 ACK는 다른 증거다.

fresh-prefill 소비 회귀는 현재 normal peer가 실제 제출 명령을 받은 뒤 서로 다른 길이의 관측과 결과를
반환하게 한다. 누락 관측, 이름만 바꾼 execution 중복, 같은 ID/다른 body, 정확한 재전송과 순서 변경을
양쪽 대조한다. OUTPUT 뒤 관측은 최종 완료/해제 전의 양성이고, 이후 늦은 관측의 재연결 수렴까지
검증했다는 뜻은 아니다. A 정상/B 잘못된 전체 관측 후보 거부와 합산 overflow에서 두 요청의 counters
모두 불변이어야 한다. 최종 artifact의 raw sampled 상한과 관측-첫 위치 대조도 독립 재검사한다.
actual drive의 budget 호출 또는 final 관측 적용을 제거한 변이는 helper 단위 시험만이 아니라 실제
소비 회귀도 깨야 한다. producer의 prefill 관측을 decode로 바꾼 변이는 같은 입력 workload에서
token/KV oracle와 별도로 실제 producer 관측 대조가 검출해야 한다. 기존 OUTPUT 캡처 주변에 synthetic
관측을 추가했다면 그 envelope까지 실제 producer에서 캡처했다고 보고하지 않는다.

T20/T25의 해제 경계는 배치 계약의 해제 권위 사슬을 실제 producer/consumer 양쪽으로 검사한다.
SESSION 선언은 설치 전과 설치 후를 모두 시험한다. 설치 후 불변성 거부만으로 잘못된 최초 선언의
검증을 증명하지 않는다. 각 role의 정방향 설치, 같은 선언 재전달, 빈/단일/중복 stage·잘못된 index/
수신 endpoint·세대·구 wire를 검사한다. OUTER 실제 생산 함수와 수신 기대값도 함께 고정한다.
모든 stage 메시지 계열의 source/target 부정은 codec→실제 handler를 지나 정확한 route 거부를 내야 한다.
정상 source/target으로 원래 body/원장 부정 시험이 여전히 해당 의미 검사를 통과하는지 확인한다.
정상 multi-stage run과 ACK 보류 상태의 잘못된 발신자 주입을 함께 두며, 허용 전부/거부 전부 변이
어느 쪽도 통과하지 않아야 한다. envelope gate의 빈 상태 행렬을 실제 native/원장 효과 시험으로 세지 않는다.
같은 physical batch 안에 서로 다른 OUTER의 A/B terminal이 있다는 것을 먼저 단언하고, 각자의
전체 target/return route·correlation·deadline과 명시 멤버십이 정확히 돌아와야 한다. 하나의 OUTER에
여러 요청을 묶는 양성도 유지한다. 생성된 receipt를 읽어 기대 집합을 만들지 않고 실제 제출/승인 자료를 쓴다.
첫/후속 통지의 Full·Closed·이벤트 ID exhaustion에서 이미 commit된 KV와 남은 notification intent를
각각 검사한다. 오류 뒤 같은 control을 다시 보내 native release 횟수를 늘리는 구현은 실패해야 한다.
내부 ACK는 정상 꼬리 양성 대조와 함께 middle/다른 head/외부 source·오래된 endpoint 세대·잘못된
session/operation/incarnation을 시험한다. body가 정확한 pending member라도 source 역할이 틀리면
head의 free/admission/pending/fence/효과는 그대로여야 한다. 직접 released 호출·실제 handle·broker/
run-loop·native chain 각각의 실행 범위를 구분한다. 한 경로의 성공을 나머지 경계까지 통과로 합산하지 않는다.
새 OUTER/Worker의 재시작과 같은 loaded Worker에서 key/slot 재사용을 별도로 검사하며, 이전 attempt의
fresh-ID receipt가 새 attempt의 terminal/release를 승인할 수 없어야 한다. 구버전 scalar receipt 거부,
후보 A 정상/B 무효의 전량 불변, 모든 member 확인 전 종료 금지 및 중복의 비증가도 포함한다.

완료 wire를 바꿀 때 기존 캡처는 덮어쓰지 않는다. 실제 run에서 원본 PREFILL·OUTPUT·receipt를 함께
새 버전으로 캡처하고, 토큰/text/position/stop·물리 폭·KV oracle는 유지한다. 명시한 신규 필드만 제거한
별도 legacy projection도 일치해야 하며, 새 버전 대조는 **payload 전체 필드**를 보존한다. OUTPUT/receipt
envelope의 명시된 volatile `event_id`/`causation_id`/`sequence`는 semantic equality에서만 제외하며
별도 유일성·sequence 진행·causation 존재 검사를 유지한다. 원본 PREFILL은 envelope까지 정확 대조한다.
소비자가 수신 OUTPUT에서
제출 기대 ID를 만들거나 캡처에 사후로 attempt 필드를 붙이는 fixture는 금지한다. 실제 송신 원문과
terminal 승인·head RELEASE/native 제어 본문을 서로 다른 관찰 지점으로 대조한다.

원본 Envelope와 ReplySpec의 **서로 다른 유효 값**도 시험한다. 잘못된 주소 문법뿐 아니라 ingress/
channel/connection/correlation/deadline/source/target/return_route 불일치, A/B 양순서에서 슬롯·pending·
effect·native 호출 전량 보존을 확인한다. PREFILL의 source/return_route·target 부정은 기억/수용 전에
거부되고 같은 원문의 올바른 선언은 실제 handler에서 계속 진행해야 한다. fixture 이관은 정상 입력
생성 지점에 한정하며 모든 입력의 source를 자동 정규화하여 부정 시험을 무력화하지 않는다.

소비 측 attempt 등록→실제 send 순서는 실패 writer로도 검사한다. fresh-envelope 정확한 receipt
재전달은 해제 0 증가, 같은 envelope ID 거부 정책은 별도다. terminal 없는 receipt·다른 attempt/slot/
incarnation/operation·혼합 member·재전달 변형·남은 B 누락을 실제 drive에 통과시킨다. 순수 원장 시험과
별도 소비 시험을 합치지 않는다. 등록/대조/원자 반영을 우회한 독립 변이도 실제 소비 시험이 검출해야 한다.
Full 계측의 오래된 snapshot만으로 이번 포화를 판정하지 않는다. ACK 미송신·수신 대기·빈 큐를 확인한
뒤 관측 baseline을 설정하고 새 포화를 기다리며, test-only 관찰 reset을 실제 상태 전이로 세지 않는다.

다중 OUTER 양성은 해제뿐 아니라 실제 생성된 OUTPUT/BATCH_OBSERVATION/STAGE_SPAN을 각 실제
소비자에 공급한다. 자기 요청 관측의 누락·타 소유자 request 노출·물리 전체 행 수와 소유 행 수의 혼동·
동일 span의 요청별 중복 계수를 구분한다. 일반 unknown-request 검사를 꺼서 양성을 통과시키면 실패다.
모든 관측을 삭제하는 변이와 전체 요청 body를 route마다 복사하는 변이도 각각 검출해야 한다. 기존
한 OUTER의 여러 요청을 묶는 경로와 원래 물리 폭·요청별 경계 oracle를 함께 유지한다.

관측 완결의 목표 계약은 배치 계약의 **소유자별 관측과 발행 증거의 완결** 절을 따른다. 다음은
T20/T25/T57/T58에 요구하는 반례이며, 내부 원장 시험이 있어도 생산/소비 완결 시험을 대신하지 않는다.

- 실제 원제출과 같은 envelope/command를 송신하는 소비자에 producer 원문을 연결한다. explicit tokens
  입력과 prompt 입력을 서로 같다고 부르지 않는다. fake Tokenize를 쓰면 호출의 실제 prompt와 응답을
  대조하되 실제 llama tokenizer/품질 증명은 별도다.
- terminal OUTPUT와 해제 receipt 뒤에 마지막 관측/span을 보류하면 소비자는 아직 성공하지 않는다.
  기존 전체 timeout 안에 전달하면 성공하고, 영구 누락이면 실패한다. quiet sleep을 완료 조건으로 삼지 않는다.
- prefill 관측은 남긴 채 중간 Decode/Verify/Replay 관측과 해당 모든 span을 함께 삭제한다. 받은
  execution만 대조하는 우회가 통과해서는 안 된다. 개수·phase 합을 보존한 execution/range 교체도 거부한다.
- 요청이 빠진 logical issue ordinal, 물리 조각 순서/도착 역순, 정확한 재전달은 정한 canonical 규칙으로
  처리한다. 동일 identity의 다른 body, 서로 다른 span 그룹에 겹친 execution, 다른 attempt/slot은 거부한다.
- A만 속한 execution과 B만 속한 execution을 포함한 logical batch에서 A에 B-only span을 요구하지 않는다.
  A/B 공동 execution은 각 OUTER에 자기 소유 행만 보이되 물리 전체 폭·비용은 같고 재합산하지 않는다.
- 후순위 owner 오류는 원장/witness/effect 전체 불변, 후순위 fan-out Full/Closed/ID 고갈은 미발행
  intent 보존과 native 재호출 0을 검사한다. 발행 승인 hook을 지우거나 관측 송신 때만 witness를
  계산하는 변이는 actual worker와 actual consumer 연결 시험 모두가 검출해야 한다.
- 긴 생성 이력을 붙여도 RequestState 후보 clone의 증거 상태 크기가 늘지 않으며, 관측 하나 도착마다
  전체 이력을 요청 수만큼 재스캔하지 않는다. 고정 크기 증거와 touched-work 계수를 독립 검사한다.

2026-09-07 내부 issued-work의 실행된 하위 범위는 다음과 같다. 전체 관측 게이트 승격은 아니다.

- `issue_witness/tests.rs`는 독립 Node literal 입력/bytes/digest와 비교한다. Rust가 출력한 값을
  골든으로 다시 저장하지 않는다. 순서 교환 양성, 같은 count/min/max의 다른 interior position,
  execution·각 authority 필드 교체, 정상 Verify/Replay 위치 재사용, ordinal 간격/overflow를 검사한다.
- `node/issue_witness_tests.rs`는 실제 `prepare_issue`/`begin_native_issue`/`accept_prepared_issue`를
  통과한다. 후순위 owner 오류 때 committed 요청뿐 아니라 PreparedIssue 후보·flight·번호·예약 관련
  상태도 보존한다. selector/shared bookkeeping 직통 시험을 Simulation 전체 시험이라 부르지 않는다.
- `worker/loop_tests/issue_witness.rs`는 실제 Worker::run과 native command/capsule/EventWire를
  통과하며 test-only 읽기 관측으로 승인 상태를 본다. 2/4/8-stage의 logical/physical 개수와 독립
  digest, 다중 OUTER·실제 Full·중복 terminal·두 번째 native 오류를 검사한다. 읽기 관측은 상태를
  수정하지 못한다. 기존 exact output token/text/position/stop·KV·release oracle를 그대로 유지한다.
- 승인 witness 설치 제거와 검사 완료 전 조기 commit, execution ID의 hash 입력 누락은 각각 독립
  복사본에서 실제 재컴파일하여 실패해야 한다. production helper만 변이하고 시험/골든은 고정한다.
  정확한 명령·현재 소스·실패 시험·실행파일은 증거 기록에 남긴다.

OUTPUT v5/관측 v4 이관에서는 위 내부 시험 외에 **실제 생산 wire와 실제 drive**를 결속한다.
버전별 계약의 소유자는 배치 계약이며, 실행 결과/봉인/미통과 범위는 로드맵 최신 기록과 증거 색인이 소유한다.

- 기존 OUTPUT v3/v4 캡처를 새 필드로 덮어쓰지 않는다. 새 v5는 actual Worker::run에서 다시 캡처하고,
  원제출·출력·영수증·관측·span을 함께 보존한다. 시험이 만든 승인 후 기대값과 수신한 관측을 구분한다.
  기대 execution 집합은 실제 발행 승인 지점의 읽기 전용 기록에서 얻고 수신 관측으로 역산하지 않는다.
- terminal witness 누락·revision/count/ordinal/digest/authority 변조와 전체 중간 issue 관측 삭제를
  실제 drive에서 거부한다. live 생산의 witness 설치/terminal 복사를 제거하는 변이도 별도로 실패해야 한다.
  소비자에 고정 wire를 재생하는 시험만으로 변경된 생산자를 검사했다고 하지 않는다.
- 두 OUTER와 한 OUTER의 여러 요청을 실제 physical batch에 함께 담는다. 각 route의 원문을 별도 실제
  소비자에 공급하며 foreign owner를 삭제한 합성 body로 양성을 만들지 않는다. empty foreign projection과
  global physical counts는 서로 다른 뜻이다. 동일 실행의 grouping·멤버십·행 수·stage 순서도 대조한다.
- 같은 span의 timestamp/body 충돌·겹친 execution 그룹은 거부한다. 목록 순서 교환과 정확한 재전달은
  정상이다. endpoint/load/session이 다른 숫자 execution ID는 같은 실행으로 합치지 않는다.
- OUTPUT와 해제가 먼저 끝나도 관측 완결까지 대기하며 원래 deadline을 늘리지 않는다. 다음 wave가
  deadline보다 늦게 예정된 경우에도 종료한다. 기존 처리량 elapsed는 해제 경계에 고정하고 늦은 관측
  완료 시간은 별도 nullable 필드로 보존한다. 관측 지연을 TPS 손실로 바꾸는 변이를 검출한다.
- mailbox Full/Closed/이벤트 ID 고갈은 실제 effect pump에서 보존 여부를 검사한다. 성공 forward 뒤
  span 시각은 한 번만 고정한다. 이는 로컬 completion mailbox 승인 시각이지 네트워크 도착 ACK가 아니다.

입력 식별자도 P4 envelope가 허용했다는 이유로 하위 승인 계약에서 유효하다고 가정하지 않는다.
native 뒤에 거부되는 형식은 PREFILL 입구에서 토큰화·원장 변경 전에 검사하고, 이후 정상 제출이
같은 worker에서 완주하는 양성 대조를 유지한다. 이를 무조건 worker 종료로 바꿔 통과시키지 않는다.

제출 row 문자열 경계는 `submission_limits.rs`의 실제 run으로 검사한다. literal serialized ReplySpec의
4095/4096/4097 UTF-8 bytes를 ASCII·JSON escape·다중 바이트로 만들며, 원문 options는 유효 JSON 뒤
공백까지 보존한다. tokens/prompt 양쪽의 정상 출력·KV·해제와 초과 거부 뒤 같은 request ID/새 session key의
재제출을 대조한다. wire 크기 제한의 성공을 실제 native 옵션 문법의 성공으로 보고하지 않는다.
입구 guard 제거·세션키 기록 뒤로 지연·bytes 대신 chars 계수·정확 상한 거부의 변이는 각각 실패해야 한다.
Rust codec 및 모델 없는 C++ `physical_wire_test`에서도 독립 literal 경계를 대조한다. native 한도만
늘린 변이와 Release 거짓 assert 검출은 격리 복사본에서 수행하며 CTest 전체/모델 추론의 대체가 아니다.

T25의 명시적 UNLOAD는 실제 run-loop에서 ordinary tail 반환 보류, speculative 최종 SETTLED 보류,
중간 stage의 잔존 KV를 각각 만든 뒤 보낸다. 요청이 없는 중간 stage, flight가 0인 정산 대기를 반드시
포함한다. 정확한 caller/generation의 오류 응답 1개, UNLOADED 0, native 호출·KV·sampler·release
이력·요청 출력 불변을 확인한다. 그 뒤 같은 요청을 재생성하지 않고 원래 반환을 재개해 literal 출력과
전 stage release를 완주하고 idle UNLOAD가 성공해야 한다. busy guard 제거, requests-only 검사,
무조건 busy 거부가 각각 실패해야 한다. FIRST/LAST의 잘못된 generation도 native 전 거부한다.

정상 busy 거부와 native cleanup의 사후 실패는 다른 경로다. idle UNLOAD가 native shutdown에서
실패하면 오류/불확실 종료를 유지하고 이미 큐잉된 후속 SESSION조차 ACK하지 않아야 한다. 이 fence를
제거한 변이도 실제 run에서 실패해야 한다. 이미 fenced/Uncertain인 worker의 치명적 실패·Drop 정리는
native 자원을 닫을 수 있으므로, healthy busy의 native 0 약속을 그 경로에 적용하지 않는다. 로컬 정지점은
입력 큐·상대 stage·발행된 mailbox의 전역 drain이나 전달 ACK가 아니며 Cancel/Drain은 별도 시험이다.

T20/T26의 actor 종료 시험은 입력이 없는 경우와 입력이 끊긴 경우를 구분한다. 한 논리 발행이
진행됐으면 새 입력 없이 다음 준비 작업을 발행할 수 있어야 하고, 실제 input EOF/중지를 관측한
뒤에는 새 native 요청을 시작하지 않는다. 첫 native 실행 안에서 정상 SESSION을 넣어 다음
자발적 논리 발행보다 먼저 ACK가 나오는지 검사한다. PHYSICAL/SETTLE 한 이벤트 내부의 여러
native 작업이나 이미 실행 중인 동기 호출의 중단까지 이 처리 개수 상한이 보장한다고 하지 않는다.

종료 분류는 cleanup 전의 요청·pending·release/settle·prepared issue·flight·효과·실제 owner/frontier·
PHYSICAL Running/Uncertain/fence를 보존한다. 완료 receipt/Released tombstone은 미완 작업이 아니지만,
Stopped KV와 active attempt가 없는 Uncertain은 아직 미완이다. 각 원장의 독립 증거를 합계 하나로
뭉개지 않는다. unload 중 `closed`를 먼저 발표하거나 unload 실패를 정상 prefix 뒤에만 붙이면 실패다.
원래 실패와 cleanup 실패, 마지막 이벤트 거부를 구분해 보존한다. 로컬 잔량 0이어도 이미 발행한
mailbox/네트워크 결과와 상대 KV의 정산은 별도이며, 이것만으로 graceful drain을 통과시키지 않는다.

T24의 native continuation은 정상 opcode/codec·token budget을 유지한 채 proposal 폭만
physical capacity+1로 만든다. PHYSICAL/SETTLE 응답과 head 반환 승인을 각각 거치며, 폭 1과 정확한
capacity는 후속 Decode/Verify가 진행해야 한다. native 뒤 오류는 성공 출력/receipt가 없고 후속
native 호출 0이어야 하며, 사전 입력 거부와 달리 KV rollback을 주장하지 않는다.

T24의 **새 execution ID 위치 우회**는 receipt 중복 시험과 분리한다. 실제 middle/tail에 정상
partial Prefill→final Prefill→Decode를 먼저 보내고, 지난 위치·미래 gap·Prefill 회귀를 각각 보낸다.
정상 A/잘못된 B를 양순서로 섞어 native 호출 0 및 owner/frontier/receipt/effect 전체 보존을 검사한다.
Verify 전량 수용의 SETTLE 없는 다음 발행, direct partial SETTLE, checkpoint 복원 후 정확한 Replay를
정상 대조군으로 반드시 유지한다. 무조건 Verify 거부·무조건 SETTLE 요구로 음성 시험을 통과시키지 않는다.
임의 SETTLE/변조 Replay/오래된 round·제안 토큰 변경은 실제 효과 전 거부하고, 잘못된 native 응답은
fence 후 추가 native 효과 0을 검사한다. 순수 검사 제거와 실제 worker 연결 제거의 변이는 별개다.
이 메서드 경로는 T20의 전체 Worker::run/네트워크 또는 실제 llama 모델 실행의 대체가 아니다.

T24의 중복 실행 하위 시험은 다음을 포함한다. 이 목록 일부의 통과로 T24 전체를 완료하지 않는다.

- 같은 PHYSICAL을 새 event ID로 middle/tail에 전달: fake native는 매 호출 KV/sampler를 실제 변경하며,
  두 번째 호출 0과 첫 응답 재생을 검사한다. 효과 없는 echo fake만으로 멱등을 증명하지 않는다.
- 설정된 서로 다른 head의 동일 숫자 ID는 정상 실행·각각 재생된다. head endpoint의 agent/node/generation
  각각을 바꾸어도 구분한다. 같은 head에서 session 또는 input/tensor를 바꾼 같은 ID는 conflict다.
- cached+Fresh 혼합, Fresh+뒤 conflict와 그 역순에서 전체 사전 거부 원자성, 실제 Fresh subset와 원래 결과 순서.
- receipt byte/count/Seen/issuer 예산의 바로 아래·경계·초과, 큰 결과의 최초 정상 전달·만료 재전달 거부,
  head별 ID 창·낮은 정상 미수신 ID·창 밖 거부. 상한 제거와 발급 권위 합치기 변이를 검출한다.
- 해제/slot 재사용 뒤 옛 Replay가 새 owner를 획득하거나 native KV를 되살리지 않는다. Replay만 있는 이벤트에
  새 계산 span이 나오지 않고, 혼합 이벤트 span에는 실제 계산한 실행만 있다.
- 새 실행 ID로 같은 시퀀스의 지나간 위치/미래 gap/다른 phase를 보내는 반례는 **별도 frontier 시험**이다.
  캐시된 ID만 거부하는 시험으로 이를 대신하지 않는다. restart·reconnect·credit와 보존 기간도 별도다.

### T30~T38: 수용·credit

| ID | 입력 / 통과 조건 |
| --- | --- |
| T30 | pending count/bytes/prompt tokens/deadline 한계 직전/정확히/초과; 명시 queue 또는 reject, 몰래 삭제 0 |
| T31 | stage별 KV 단가/shape/SWA/recurrent 보조 예산 다름. 상주 가능한 수는 전 stage 예약의 교집합, over-admit 0 |
| T32 | 다중 노드 reserve Prepare/Commit/Release/TTL/reconcile 중 장애. prepared도 회계 포함, stale commit fencing, 부분 확보 누수 0 |
| T33 | edge row와 byte budget을 독립 소진. 작은 행/큰 tensor, 큰 행/작은 tensor, fan-in. 협상 상한 초과 0 |
| T34 | ACK 중복/유실/재연결/취소/이전 epoch. 같은 ticket 반환 한 번, 반환 없는 시간에도 bounded queue/RSS |
| T35 | fragment credit 회수됐어도 compute/KV 미완. 스냅샷/슬롯 재사용이 진행되지 않음 |
| T36 | 작은 prompt 뒤 대형 prompt와 반대 순서, 계속 들어오는 decode/prefill. 수용 대기와 runnable 대기 각각 bound 및 명시 deadline |
| T37 | resident 한도와 decode 폭과 ubatch를 독립 변경. 한 설정의 변경이 다른 예산을 몰래 넓히지 않음 |
| T38 | 다중 fragment 1/2/4, credit 부족/큐 포화/취소 조합. 동일 모델 의미론·행 회계·최대 memory 보존 |

### T40~T47: 정책

| ID | 입력 / 통과 조건 |
| --- | --- |
| T40 | ordinary attention: 여러 프리필 길이 + decode. 합법적인 budget 채움과 요청별 기아 방지; reference allocator의 작은 상태 전수 비교 |
| T41 | equal-width: ready sequences 4/7/8/16/64, capacity 8; decode 1 + prefill 17; 역방향. 개별 첫/중간/끝 gap과 진행량 |
| T42 | 수요 배열 순서 permutation, 동적 도착/완료, sparse ID/wrap. 동일 논리 상태에서 동일 선택·정산; index를 identity로 쓰는 변이 검출 |
| T43 | cohort 분리 제거 시 prefill 폭 붕괴를 검출; cohort 전체 횟수만 보는 시험 금지 |
| T44 | plan budget과 physical split을 별도 검증. 논리 batch>ubatch, 마지막 짧은 chunk, mixed/등폭/atomic 분할 행 보존 |
| T45 | work-conserving 이유 대조: 보내지 못한 eligible row마다 reason, 요청 수를 행 수로 세는 mutation 검출 |
| T46 | 고정 seed trace + 독립 invariant + run(A+B)=run(A);run(B), 실제 비행 중 split/추가 도착 |
| T47 | age/deadline/priority/credit/KV 제약의 충돌. 절대 fairness 불가능한 overload에서는 명시 거절/SLO 실패이지 무한 대기 성공 아님 |

T19/T45의 비용 검사도 필수다. 프롬프트 길이를 늘렸을 때 매 토큰의 발행/정산 후보가 불변 tokens·
원본 event payload를 다시 복제하지 않는지, 관련 없는 열린 batch 수를 늘렸을 때 반환 하나가 전체
owner를 재검색하지 않는지 독립 계수로 측정한다. 안전성 검사를 제거해 비용을 줄이지 않는다.
증분 구현은 시험/debug의 독립 전체 원장 대조와 기존 거부·원자성 변이를 그대로 통과해야 한다.

### T50~T58: native·하네스

| ID | 입력 / 통과 조건 |
| --- | --- |
| T50 | 제품 LOAD/inference 경로에서 build/model/ABI/capability 검증 강제. 드라이브에서만 agree하는 우회 경로 실패. physical 지원만 있고 실행 identity revision 없음/unknown, 다른 bind echo·응답 유실·같은 generation 재사용을 거부하고 슬롯 미공개. 바인딩 시험으로 state ABI/placement 검증을 대체하지 않음 |
| T51 | host UUID/device UUID/backend/plugin/NUMA/MIG/Metal/multi-device/host fallback을 실제 placement로 보고; `CUDA0` 문자열 일치만으로 합의 금지 |
| T52 | pin pristine prepare, patch classification, 비공개 header gate, CPU + declared production backend CTest; stale DLL/cubin/혼합 배포 거부 |
| T53 | 모델별 stage 잔존성·unified sequence 분리·backend conformance의 세 축. memory family별 load/alias/view/split/logits와 사용하는 KV/trim 기능의 수치/상태 게이트 |
| T54 | per-host 기록 [begin,end) 양끝, byte 길이/digest, UTF-8 strict, 기록 실패 sticky/control 신호. 로컬·원격 동일 부정 시험 |
| T55 | 샘플러 직렬 기준선. 병렬 후보는 synchronize/output reorder를 단일 소유하고 독립 logits/sampler로 분리, 고정 seed/멤버십/TSAN 또는 동등 race 검증. 실기 통과만으로 race 부정 금지 |
| T56 | 여러 agent/host 배포·배치 명세, host별 실제 binary/model hash. 원격 SSH 접속만으로 다중 머신이라고 보고하는 fixture 거부 |
| T57 | per-request submit/admit/prefill/first/terminal 시각, token IDs/text, stage/edge/device span. 교차 host 시계 오차 미측정이면 overlap 값 unavailable |
| T58 | report/judge의 missing/duplicate/wrong request/깨진 UTF-8/빈 응답/잘못된 정답/다른 run/stale log/failed channel/조작 hash 부정 fixture |

### I00~I09: 계층 격리와 upstream 추종의 필수 시험

[계층 격리 계약](layer-isolation-contract.md)의 책임/의존 경계를 실제 빌드와 변경 반증으로 검사한다.
기존 private-header 문자열 gate만 통과했다고 이 표를 PASS로 바꾸지 않는다.

| ID | 입력 / 통과 조건 |
| --- | --- |
| I00 | Rust normal dependency graph에 concrete backend 역참조/FFI를 주입하면 실패. policy/ledger/model tests는 llama checkout·GPU·network 없이 빌드·실행, mock event 경계도 유지. generic capacity/전달 경로에 phase·KV·llama sequence 의미를 넣는 우회도 거부 |
| I01 | full build와 imported relink 각각의 consumer에 private/common/ggml-src include 침범 주입. 실제 compile 실패; transitive INTERFACE 경로까지 검사. 허용 bridge는 정상 빌드 |
| I02 | public signature/전방 선언의 common/private type 및 consumer의 internal header·Impl/raw/plan_params 접근 mutation 실패. 허용 compat white-box 시험은 성공. pure DTO의 포인터/상류 ordinal 누출 0; 엔진 전용 tensor codec은 별도 identity/코드표를 결속하고 unknown dtype/flags·ordinal 의미 변경·mixed-codec fleet를 거부 |
| I03 | 의미 보존 private/common API rename을 compat 모듈 안에서 적응. P4 protocol/ledger/policy 소스 변경 0, 동일 normalized input의 trace·정산/멤버십 일치. 새 native enum/옵션을 capability 번역 없이 상위 층에 전달하는 mutation은 실패 |
| I04 | opaque plan의 직접/간접 옵션 소비 모두 보존. parser/default/new unknown 옵션·grammar·sampling·device split golden 비교. 기존 27필드 white-box 단언은 compat test target에서 유지, getter 복제/시험 삭제 금지 |
| I05 | 컴파일 가능한 KV/state/alias/position 의미 변경 mutation. native conformance 또는 state compatibility gate가 실패해야 함. 단순 signature/patch clean으로 승인되는 우회 검출 |
| I06 | clean pin replay·LF/CRLF checkout·패치 분류/의존·흡수 fix 후보. 최종 prepared tree/hash 검증, 미분류/허용 scope 밖 patch 실패, patch 제거는 해당 회귀로 증명 |
| I07 | 선언 CPU/production backend별 fresh build·실제 device/layout·ABI 협상·제품 LOAD의 mixed binary/plugin 부정 시험. kernel/stream/buffer 의미 변경은 해당 conformance로 검출. 사용 가능한 backend 열거를 실제 placement로 세지 않으며 engine-only 변경과 backend-only 변경의 수정 범위를 따로 대조 |
| I08 | Release에서 의도적 거짓 assert와 내부 타입 침범이 각각 CTest/build를 실패시킴. 옵션 내부 시험 target의 권한이 runtime consumer로 전파되지 않음 |
| I09 | 채택 pin 이동 기록과 합성 호환/비호환 변화 모두 보존. 수정 범위/수동 비용/남은 debt 계측; 실제 final wave 비회귀로 승격. 이전 pin rollback은 state 행렬과 별도 대조 |

다음은 위 ID의 **필수 하위 사례**이며 신규 gate가 실행됐다는 기록이 아니다.

- **I00/I01/I02 — 실제 의존 폐쇄**: `cargo metadata`의 normal/build/dev·target/feature별 그래프와
  CMake가 계산한 compile/link 입력을 검사한다. 정상 public consumer, 허용 compat white-box consumer,
  private/common/Impl 침범 consumer를 full source와 imported relink 각각에서 실제 빌드한다.
  이름/문자열 canary의 성공으로 이 검사를 대체하지 않는다. 정적 라이브러리의 link-only 전달은
  API 공개와 구별하고, 금지 호출이 최종 link에서 풀리는 우회도 검출한다.
- **I02/I05 + T18/T24 — Rust/C++ 이중 구현 대조**: 같은 버전의 canonical 명령 transcript를
  Rust adapter와 native C++의 실제 codec/소유권 소비 경로에 넣는다. load bind, 최초 실행, 새 incarnation,
  정확한 replay, 같은 ID의 다른 body, release 이후 늦은 명령, 예산 초과, native 성공 뒤 응답 유실을 포함한다.
  입력별 accept/reject/uncertain, KV/native 효과 횟수, 응답 bytes와 상태 보존을 독립 oracle에 대조한다.
  한 언어의 guard만 제거하는 변이가 실패해야 한다. 양쪽이 같은 오류를 낼 가능성이 있으므로
  상호 결과 일치만으로 승인하지 않는다. native 모델 실행이 없는 경우 그 한계도 표시한다.
- **I07/T50/T52 — 소스 신원과 실제 적재 신원의 분리**: 새 pin의 headers + 이전 pin의 lib/DLL,
  같은 이름의 다른 backend plugin, 동적 검색 경로에 먼저 나타나는 stale DLL을 주입한다.
  소스/lib 불일치는 패키지 검증에서 먼저 거부할 수 있지만 그것만으로 runtime 검증을 대신하지 않는다.
  실제 적재한 모듈 path/hash·plugin·장치/버퍼 placement와 봉인된 build manifest를 대조하고,
  검색 경로의 stale/shadow 등 실제 적재 불일치는 **제품 LOAD가 거부해야 한다**.
  OS/backend가 달라 해당 실증을 못 한 조합은 PASS가 아니라 미검증이다.
- **I04/I08 + T03 — 격리 시험의 실행과 수명**: 모델 없는 parser/scalar/계약 검사는 명시 unit 경로로,
  vocabulary/실제 KV/샘플링 검사는 model-required 경로로 구분한다. 같은 이름의 실행파일에서
  일부 본문이 조기 return하면 전체 conformance pass로 세지 않는다. 선택 모델 부재는 SKIPPED,
  명시 필수 모델 부재는 실패다. opaque handle의 move/clone/destroy와 실패 시 원본 보존을 검사하고,
  moved-from 값을 역참조하는 회귀를 정상 모델 경로와 모델 독립 수명 시험에서 놓치지 않는다.
- **T53/I02 — Replay logical output과 native logits 분리**: 실제 FIRST/downstream 배치 생성 소비자가
  엔진에 전달한 logits 요청을 관찰한다. Replay의 모든 필요한 행을 요청하며 wire owner/output은
  보존되어야 한다. 모델 없는 native API intercept는 배치 생성 본문만 증명한다. 실제 sampler의
  logits 접근·checkpoint 복원·재계산 결과는 모델을 적재한 별도 native/웨이브 게이트가 필요하다.
  mock 성공만으로 이 모델 경로를 완료 처리하지 않고, 실제 소비 지점의 mask 복원 변이를 검출한다.

I03의 동일 trace는 동일 의미/capability 입력에 대한 조건이다. 엔진이 실제 shape·KV 제약을 바꾼 경우
새 capability 계약과 기대 결과를 별도 심사하며, 옛 trace에 맞추려고 실제 제약을 숨기지 않는다.

격리 gate의 입력에는 실제 crate/target/module별 허용 의존 manifest와 public/internal 인터페이스 대장이
필요하다. 아직 목록·실행기가 없으면 I gate는 TODO다. allowlist 확대와 검출기 변경을 같은 수정에 넣어
자체 승인하지 않는다. 정상 consumer·허용 white-box 시험과 금지 침범을 모두 검사한다.
I02의 엔진 전용 codec 예외는 generic P4나 L1/L3에 dtype 해석 권한을 주지 않는다.
T11/T23은 꼬리 출력→head 거부, 일부 output publish→Full/Closed도 포함해야 하며,
실제 외부 output·native settle/release 호출 계수를 본다. head의 내부 counter만 보존하면 충분하지 않다.

### K00~K09: 영속·스냅샷 기능을 켤 때 추가되는 필수 시험

이 시험들은 [저장 규약](kv-state-store-convention.md)의 계약을 실행으로 검사한다.
수렴 방향·identity 정의를 여기 다시 만들지 않는다. B 단계가 PASS여도 K가 자동 PASS가 되지 않는다.

| ID | 입력 / 통과 조건 |
| --- | --- |
| K00 | 같은 operation ID·여러 cut·여러 session·공유/노드 전용 root, 충돌 digest. namespace 충돌·다른 세션 덮어쓰기 0 |
| K01 | CONTROL/lease/token/epoch의 늦은 writer, lock 재획득, host reboot, stale-break. 권위 없는 공유 저장소는 거부, 늦은 publish/release가 새 소유권을 훼손하지 않음 |
| K02 | state/tokens/meta 생성, flush, pointer publish 각각 crash·ENOSPC. 노출 bundle의 부분 조합 0; 고아는 임의 재색인하지 않음 |
| K03 | 모델/variant/sidecar/토큰 파일 1바이트 변조, LoRA scale/보조 artifact 변경, ID prefix 충돌. 전체 identity 대조; N-1 writer→N reader 및 layout 행렬의 실패 조합은 거부 |
| K04 | 각 stage의 Prepare/Committing/Committed 전후, coordinator/노드 재시작, 영수증 일부 유실. 규약의 증거 기반 수렴을 실현하고 휘발 resident 복구를 추측하지 않음 |
| K05 | cache 명령과 compute/release 경합. 대상 sequence의 전 stage quiescence attest 전 export/trim/reuse 금지, 다른 sequence는 계속 진행 |
| K06 | Checkpoint/Fork/RestoreInto/List의 시그니처·버전·immutable snapshot/mutable reference 선택을 먼저 확정. 전체 tuple 대조, OUTER 재시작 원장 복구, alias 없는 분기 |
| K07 | 저장 prefix보다 짧음/분기/동일 요청, arbitrary/bounded/none trim, 부분 TrimTo. 규약의 import 전 판정과 전 stage 장벽, 죽은 suffix 부활 0 |
| K08 | source/target storage domain, read-pin과 Discard/GC, 이동 중 실패. reader 사용 중 삭제 0; cross-domain을 무복사 공유라고 보고하지 않음 |
| K09 | 디스크/RAM/resident tier·chunk 크기·압축의 별도 예산, quota/ENOSPC·복원 예약·TTL 경합. 부분 확보/큰 상태 누수 0, 고정 모델의 logits/position/정상 응답 보존 |

모든 K 시험도 mutation·실제 소비 경로·실패 집계 규칙을 따른다. 저장 기능을 비활성한 릴리스는
해당 시험을 실행하지 않은 이유와 미완 범위를 명시한다. 단순 feature 제외를 기능 완료로 세지 않는다.

## 4. 실기 웨이브 계약 — 이것만 최종 성과 증거다

### H0. 실행 전 승인·봉인할 명세

최종 모델의 이름만으로는 부족하다. 모델/분할 GGUF/보조 artifact digest, 총/활성 parameter 수(MoE 구분),
quantization, context 길이, KV 형식, tokenizer/chat template, sampling/seed, 요청 corpus를 보존한다.
초대형 최종 모델은 소유자가 승인한 모델이며, 목표 context·동시성에서 한 머신으로 충족하지 못하는
가중치 또는 KV 용량 요구와 실제 다중 머신 분산 이유를 명세에 적는다. 파라미터 수만으로 용량을 추정하지 않는다.

`multi_host_final` 승인에는 물리 호스트 2개 이상에서 실제 stage 계산과 KV 소유가 있어야 한다.
현재 자원 안의 `hardware_scoped` 실행은 아래 자원 단계별 범위를 따른다. 각 host identity·장치 UUID·실제 placement,
링크 대역/지연, power cap, driver/backend/plugin, CPU/RAM/VRAM 예산과 stage 컷을 기록한다.
LANES 문자열·프로세스 수·SSH tunnel·원격 한 호스트는 이 조건의 대체가 아니다.

`benchmark-spec`에 아래 값을 A/B 전에 봉인한다. 현재 runner가 이 명세를 모두 지원하는 것은 아니다(B5 작업).

- `source_commit`, `runtime_kind=event`, `scheduler_version`, binary/library hashes, compat pin/patch digest.
- `model_manifest`, `execution_layout`, `workload_digest`, `judge_version`, `summary_version`.
- resident 목표 R, queue count/bytes/tokens, stage KV reserve, edge row/byte bounds.
- 정상/overload 웨이브별 총 요청 수·크기·간격·길이 분포·token cap·절대 timeout.
- TTFT/ITL SLO, 유의한 개선 최소폭, A/B 순서·반복 수·holdout seed, GPU 표본 간격/워밍업 구간.
- 모든 numeric bound는 구체적 값이어야 한다. 무한대·미정인 핵심 예산으로 최종 run 시작 금지.

#### 자원 단계별 증명 범위와 RAM 오프로딩 게이트

현재 사용자가 승인한 자원과 실기 확장 순서는 [로드맵 §1](distributed-batching-roadmap.md)이 소유한다.
`benchmark-spec`에는 `resource_tier= vram_only | ram_offload`, 실제 `physical_host_count`,
`coverage= hardware_scoped | multi_host_final`을 구분한다. 한 호스트 검증에서 H6은 PASS가 아니라
`BLOCKED: only_one_physical_host`이며, 나머지 적용 가능한 게이트 결과를 숨기지 않는다.
이 구분은 기존 runner의 구현 사실이 아니라 B5의 명세/판정 구현 요구다.

**VRAM-only 충분성**은 최소한 다음을 전부 만족해야 한다. 단일 smoke·GPU 사용률·토큰 개수만으로 다음 단계에 올리지 않는다.

- 관련 T/I 안전성·실제 실행 identity 게이트 통과, 감사된 family/backend, 모든 stage의 실제 placement 확인.
- 일반 제어·토크나이즈·CPU sampler·host staging은 별도 표기하되, 실제 실행하는 모델 레이어의
  계산/계산용 가중치/KV가 CPU 또는 host RAM에 의존하면 의도한 설정이어도 `vram_only` 승인 금지다.
  모델 파일 mmap, 계산하지 않는 비소유 레이어의 CPU 표시, 일반 제어용 RAM과 모델 오프로딩을
  혼동하지 않는다. 분류는 옵션 이름이 아니라 실제 compute/storage placement로 결정한다.
- 봉인된 정상 corpus의 H1, cold/sustained/recovery H2, 별도 overload H3, H4 원자료/분모가 통과.
  승인된 큰 모델의 목표 context/resident에서 정상 웨이브와 H7의 지속·drain 안정성을 증명한다.
- 모든 정상 응답 전문 보존, 오류·유실·세션 오염 0, 사전 memory/SLO 예산 준수. 성능 최적화를 주장하면 H5도 통과.
  VRAM-only baseline 승인 자체에 새 TPS 향상 수치를 억지로 만들지 않는다.

**RAM 오프로딩 명세와 부정 시험**은 위 조건에 다음을 더한다. 자동 fallback을 관측하지 않고 계획값만 보고하는 시험은 실패다.

- CPU/host-resident 가중치·expert/레이어·KV, GPU-resident 부분, NUMA(사용 시), compute placement,
  pageable/pinned staging을 구분한 실제 layout과 runtime 옵션. `-ngl` 같은 요청값만으로 실배치를 대신하지 않는다.
- OS/기존 서비스 여유를 뺀 host RAM budget, resident set/commit·pagefile/swap 정책·KV/queue/credit
  예산을 수치로 봉인한다. VRAM 두 장의 합과 RAM을 하나의 연속 pool로 계산하지 않는다.
- 예상 여유 부족은 load/admission 전에 명시 거부한다. 위조된 여유/예약 실패/CPU fallback/전송 지연/취소/
  drain 반례에서 오수용·무응답·누수·모델 품질 손실을 검사한다. 운영 호스트에 무제한 OOM이나 디스크 고갈을 유발하지 않는다.
- 같은 분석창에 host RSS/private commit, available RAM, CPU 사용, page fault·paging I/O(지원 시),
  host↔device 전송 bytes/time, GPU memory/power/util을 수집한다. 미지원 계측은 unavailable과 이유를 남긴다.
- 더 큰 모델의 H1~H4/H7을 다시 통과한다. batch/backend 변경의 수치 오차와 모델 고유 품질은 별도 판정한다.
  작은 VRAM-only 모델 성공이나 동일 API 호출은 큰 모델+offload conformance의 대체 증거가 아니다.
- 정책 비교는 같은 offload layout 안에서 H5를 적용한다. 가능한 동일 모델의 VRAM-only↔offload 비교는
  별도의 자원 배치 실험이며, 서로 다른 모델 TPS를 합쳐 정책 개선율로 보고하지 않는다.

inventory의 평가 단위는 논리 모델+정확한 artifact 집합/variant다. 검증하지 못한 후보도
`unsupported_family`, `insufficient_budget`, `artifact_incomplete`, `path_inaccessible`, `not_yet_tested` 등
구체적인 상태를 남긴다. 파일명에 큰 파라미터 수가 있거나 load만 성공했다는 이유로 전체 후보 검증을 완료하지 않는다.

### H1. 정상 프롬프트와 응답

- 고정 질문 하나만 반복하지 않는다. 일반 설명, 서로 다른 길이의 문서 요약, 주어진 사실에 대한 질의,
  작은 코드/표/형식 결과 등 정상적인 요청 corpus를 사용한다. 긴 입력을 무의미한 반복 문장으로 채우지 않는다.
- 공개/합성 데이터만 쓴다. 각 request ID의 raw prompt, template 적용 텍스트, 실제 token IDs/count,
  응답 전문과 token IDs/positions, stop reason, judge 이유를 보존한다. `...`로 생략된 대표 출력만 보존하면 실패다.
- 요구 언어/형식/질문 관련성/정답 체크를 분리한다. controlled corpus의 machine-checkable oracle과
  일반 응답 전문의 hash-bound 의미 검토가 둘 다 필요하다. TypeScript 키워드 개수만으로 의미 통과 금지.
- 정상 서비스 gate는 요청별 구조·정산·출력·의미 **전부 통과**해야 한다. transport 64/64, 의미 55/64는 실패다.
- 모델 자체도 못 푸는 fixture는 별도 진단 후 모든 arm에 동일하게 버전 변경하고 전부 재실행한다.
  실패한 arm만 다른 seed로 교체하거나 정답 기준을 사후 완화하지 않는다.
- stochastic/다른 batch/backend의 비트 동일성을 무조건 요구하지 않는다. controlled same-layout의 token/position 기준선과
  backend별 logits 허용오차를 사전 선언하고 자연어 품질 게이트는 별도로 유지한다.

### H2. 강한 겹치는 웨이브

개발용 smoke/mixed는 보조다. 최종 정상 서비스는 아래 세 모드를 모두 포함한다.

| 모드 | 최소 구조와 필수 관측 |
| --- | --- |
| Cold burst | 시점 0에 최소 R건을 함께 제출. tokenize/prefill부터 fill까지 기록. prewarming으로 이 비용을 숨기지 않음 |
| Sustained waves | 적어도 8개 웨이브, 총 요청 최소 8R. 각 wave에 짧은/중간/긴 정상 prompt를 섞고, 초기 기준선의 완료 시간보다 짧은 고정 간격으로 도착 |
| Recovery waves | 같은 agent/model load를 유지한 채 load→drain→다음 wave block을 최소 3회. 매 실행 재시작으로 O12를 회피하지 않음 |

도착은 open-loop 명세로 고정하고 A/B 사이 간격을 적응 변경하지 않는다. 클라이언트가 응답을 기다려 요청을 늦추면
약속한 웨이브가 아니다. 실제 send skew와 목표 대비 지연을 기록하고 허용치를 넘으면 INVALID다.
Sustained에서는 초기 웨이브 완료 전 후속 웨이브의 첫 token이 나와야 하며, 반복 웨이브 사이 decode/prefill
활성 구간 중첩을 request별로 증명한다. 파이프라인에 단순히 pending이 많다는 것만으로 overlap을 주장하지 않는다.
모든 요청이 끝나버려 포화되지 않는 구성은 wave 강도를 높여 **양쪽 arm 전체를 같은 새 spec으로** 다시 돌린다.

### H3. 과부하와 수용

별도 overload spec에서 resident 목표 R을 넘는 최소 2R 동시 burst를 준다.
선언 budget 안에서는 queue/서비스, 넘으면 명시 거절·deadline 결과를 반환한다. 허용 거절과 정확한 이유를
정상 서비스 성공 건수와 분리한다. 요청 유실, 무고한 resident 세션 실패, over-admit, 무제한 RSS는 0이어야 한다.
overload에서 거절한 요청을 분모에서 빼 TPS가 오른 것처럼 보고하지 않는다.

### H4. 지표와 분모

- 주 지표 `useful_generation_tps`: 정상 판정된 요청의 실제 생성 token 수 / (첫 예정 송신부터 마지막 terminal까지).
  admission·전송·wave gap을 포함한다. 첫 sampled token도 세되 prefill input과 speculative draft/rejected token은 생성량에 넣지 않는다.
- 별도로 전체 생성 TPS, 유효/실패 요청 수, prefill input rows/s, decode model-evaluation rows/s, total rows/s를 기록한다.
  이들을 같은 TPS 열로 혼합하거나 완료 토큰 한 개의 차이를 감추지 않는다.
- TTFT는 **각 요청의 실제 submit→그 요청의 첫 token**, ITL은 해당 요청의 실제 출력 간격이다.
  실행 시작부터의 first-token 시각을 TTFT로 부르지 않는다. queue/tokenize/prefill 구간도 별도 보고한다.
- 각 GPU의 동일 분석창 내 sample coverage, mean/p50/p90, zero%, memory peak, power,
  useful kernel active time/SM·메모리 지표(지원 시)를 보고한다. 지원 불가면 unavailable이지 0이 아니다.
- `nvidia-smi utilization.gpu`는 GPU 활성 시간 지표이지 SM occupancy나 유효 FLOPS가 아니다.
  device가 아니라 stage RPC span을 “동시 GPU 계산”으로 보고하지 않는다.
- ubatch fill은 물리 batch의 row/실제 해당 stage ubatch 상한으로 산출하며 prefill/decode/mixed를 분리한다.
- stage queue/compute/sample/copy/encode/network wait, node별 KV used/reserved/free,
  edge credits, runnable/blocked/pending, 거절 이유, inflight peak, drain 종료 잔량을 보고한다.
- 여러 머신의 overlap은 clock synchronization 오차를 포함한다. 오차보다 작은 중첩은 확정하지 않는다.
  요청 end-to-end 시간은 동일 클라이언트 단조 시계, device span은 각 장치의 적합한 clock으로 잰다.

### H5. 최적화 비교

기본 비교는 **같은 모델·요청·토폴로지·KV 용량·resident 한도·backend 설정**에서 정책만 바꾼다.
node/cut/placement 변경은 별도 실험축이며 batch 정책 개선 수치에 합산하지 않는다.

최소 8개 paired 반복(예: ABBA/BAAB 균형 블록), 동시 실행 없이 arm 교차, warm/cold 구간 분리,
별도 holdout 최소 4쌍을 수행한다. GPU 온도·클럭·전력·백그라운드 작업·순서 효과를 보존한다.
baseline 이봉/드리프트가 있으면 평균 하나로 합치지 말고 각 run과 분포를 보고한다.

기본 승격 기준(바꾸려면 실행 전에 spec 버전화):

- 모든 정상 서비스 arm의 H1/H2/H4 통과, 실패 arm 삭제 금지.
- paired 유효 생성 TPS 중앙 개선 ≥5%, paired 개선의 95% 신뢰구간 하한 >0, holdout에서도 개선 방향 유지.
- TTFT p95 ≤ baseline의 1.10배, ITL p95 ≤1.05배 및 사전 절대 SLO 만족.
- KV/VRAM/RSS/credit 상한·토큰량·품질을 희생한 개선은 무효.
- GPU util/device active와 TPS의 Pareto 결과를 함께 기록한다. util만 상승하고 TPS가 하락하면 승격 금지.
  TPS는 올랐지만 util이 하락했다면 효율 개선 후보로 남기되 **GPU 활용까지 개선됐다고 보고하지 않는다**.
  포화 부하에서 유용한 작업이 준비됐는데 장치가 쉬는 구간의 원인을 추적하고, 최종 공동 목표의 미달/물리 한계를 명시한다.
- 선택한 탐색 범위/후보/제외 이유/이웃 후보 결과를 보존한다. “최대”는 이 범위와 한계 안에서만 쓴다.

≥5%는 기본 제품 개선 문턱이며 과거 수치에서 도출한 법칙이 아니다. 이미 개선된 기준선을 재정리하는 릴리스는
새 성능 향상을 주장하지 않고 비회귀 판정임을 구분한다. 소유자 승인 없이 문턱을 낮추지 않는다.

### H6. 실제 분산 증거

모든 참여 host에 대해 해당 run의 모델 shard/KV 소유·stage 실행·device trace·배포 hash가 있어야 한다.
트래픽이 실제 host 경계를 건넜다는 연결/전송 바이트 증거를 수집한다. GPU 프로세스만 원격이고 클라이언트가
다른 컴퓨터라는 이유로 모델이 여러 머신에 분산됐다고 하지 않는다.
CPU fallback 또는 다른 backend가 사용되면 실제 placement에 표시한다. 다른 backend family의 이름이 같다는 이유로
state 호환성을 허용하지 않는다. 선언하지 않은 host/device로 흘러간 실행은 INVALID다.

### H7. 지속 운영과 장애

봉인된 최종 후보에서 최소 60분 및 총 32R 요청 중 더 긴 조건까지 정상 웨이브를 지속한다.
동일 load의 반복 웨이브, 취소, 느린/끊긴 edge, 노드 재시작과 늦은 반환은 별도 장애 arm으로 실행한다.
정상 arm은 정상 응답 100%, 장애 arm은 사전 정의한 완료/취소/실패 수렴 100%이며 무응답·유실·다른 세션 오염은 0이다.
종료 후 원장·예약·credit·queue가 기준 상태로 돌아오고 host RSS/VRAM이 사전 허용 범위에 수렴해야 한다.
재시작으로만 반복이 가능하면 제품 목표 미완이다.

## 5. 증거 번들과 보고

run마다 불변 디렉터리를 만든다. 성공과 실패/중단 모두 보존한다. `target/`만 가리키는 링크로 완료하지 않는다.

```text
<run-id>/
  benchmark-spec.json        # 위 H0의 봉인 입력
  provenance.json           # source/runtime/binaries/models/hosts/layout
  requests.jsonl            # raw/template/token IDs + 예정/실제 송신 시각
  responses.jsonl           # 모든 응답 전문/token IDs/positions/stop
  judge.json                # 요청별 구조/의미/정답 판정, 검토자·버전·digest
  telemetry/                # host/device/stage/edge/queue/credit 원자료
  report.json               # 모든 분모·coverage·판정·missing 목록
  commands-and-exits.txt     # 재현 명령과 종료 상태
  checksums.sha256
  failure.json              # 실패/중단 시 원인; 이전 성공 리포트 재사용 금지
```

이 레이아웃은 목표 계약이다. 현재 `config.json/artifact.json/report.json/gpu.csv`는 이 정보를 일부 담는 기존 형식이며
마이그레이션 시 필드/형식 버전과 원자료 대응표를 보존한다.
원자료가 큰 경우 접근 가능한 장기 artifact 저장소에 올리고 위치·전체 digest·복구 명령을 Git evidence에 기록한다.
계정/토큰/개인 데이터를 기록하지 않는다. 재접근 불가능한 로컬 임시 경로는 장기 증거가 아니다.

**Git 포함 심사:** 작다는 이유만으로 원자료를 추적하지 않는다. 유지할 소스·실제 회귀 시험·필수
최소 fixture·간결한 계약/검증 기록을 넣고, 생성 로그·중복 source manifest·일회성 프로브/보관
도구·바이너리·모델·build cache는 기존 무시 경로에서 보존한다. 특정 run 전용 도구를 공용
도구로 승격하려면 재사용 소비자와 경로 독립 실행 시험이 있어야 한다. 실패를 숨기려고 fixture나
필수 시험을 무시하지 않는다. 원자료의 장기 저장 위치가 없으면 재열람/재현 조건을 미충족으로
남기며, 이를 닫기 위해 무단 업로드하거나 Git에 원문을 복사하는 것으로 대체하지 않는다.

코드나 run을 완료할 때 [runtime-evidence](runtime-evidence.md)에 색인을,
`layers/adapters/llamacpp/staged/scripts/validation/evidence/<date>-<topic>.md`에 상세를 남긴다.
기록은 source commit, 실행한/미실행 시험, 실패 counterexample, 모든 arm, 전체 응답 접근 경로,
판정 한계와 다음 작업을 포함한다. 이번 보고에서 하지 않은 검증을 과거 통과로 채우지 않는다.

## 6. 지금 사용할 수 있는 명령

저장소 루트 PowerShell 기준. GPU/원격 명령은 해당 자원·권한과 H0 명세 확인 후 실행한다.

```powershell
git status --short --branch
git rev-parse HEAD
cargo test --workspace --no-fail-fast
cargo test -p p4-llamacpp-staged-adapter
npm run docs-lint
npm run test:docs-lint
node tools/scripts/docs-lint.mjs --all
$harnessTests = rg --files test/benchmarks/p4-4node -g '*.test.mjs'
node --test $harnessTests
node layers/adapters/llamacpp/staged/scripts/validation/validate-private-headers.mjs
```

현재 pin은 HEAD와 `layers/adapters/llamacpp/staged/compat/`에서 다시 확인한다. 다음 경로는 감사 기준 pin이다.

```powershell
node layers/adapters/llamacpp/staged/scripts/validation/validate-compat-manifest.mjs --manifest layers/adapters/llamacpp/staged/compat/0eadefebd/manifest.json
node layers/adapters/llamacpp/staged/scripts/validation/validate-patch-classification.mjs --manifest layers/adapters/llamacpp/staged/compat/0eadefebd/manifest.json
node layers/adapters/llamacpp/staged/scripts/prepare-pipeline-upstream.mjs
node layers/adapters/llamacpp/staged/scripts/build-stage-server.mjs --backend cpu
node layers/adapters/llamacpp/staged/scripts/build-stage-server.mjs --cuda --cuda-architectures '86;89'
```

`86;89`는 과거 개발 장치 예시이며 실제 fleet capability에서 다시 정한다. 따옴표 없이 PowerShell `;`를 넘기지 않는다.
CTest는 build script가 출력한 실제 build directory/config에서 실행한다. 오래된 바이너리의 CTest를 새 소스 증거로 쓰지 않는다.
기존 `node test/benchmarks/p4-4node/run.mjs smoke|service|prefill_mix_35b` 명령은 개발 진단용이며
이 문서의 H0~H7 전체를 자동 강제하는 runner는 아직 아니다. 각 scenario를 별도 명령으로 실행한다.

<a id="v11-gates"></a>

## 6.1 v1.1 통합 진단에 대한 검증 적용

2026-09-11 계획 추가. 실행 순서는 [로드맵 v1.1](distributed-batching-roadmap.md#v11-plan)만 소유한다.
아래는 아직 실행하지 않은 변경별 판정 조건이며 기존 T/I/H/K 게이트를 완화하지 않는다.

| 변경 | 필수 반례·소비 경로 검증 |
| --- | --- |
| 관측 | 같은 시각의 요청별 admitted/eligible/blocked·issue/settle·byte 상태를 재계산. 지연/거부/부분 OUTPUT 뒤 실패에서도 최초 오류와 승인된 부분 증거 보존. 실제 ITL은 연속 OUTPUT 수신시각으로 계산 |
| byte 수용/반환/receipt | 개수는 작지만 payload가 큰 입력, 완료 receipt 누적, 중복 replay, budget 경계±1, 전달 결과 불명, 해제/정산 제어 진행. 거부 전후 원장·예약·credit·출력 효과 동일. TTL/LRU로 아직 유효한 중복 판정 근거를 임의 삭제하지 않음 |
| decode 묶음/prefill quantum | 16 decode를 한 묶음에 소진하는 반례, 같은 길이 장기 요청의 누적 불공정, decode 지속 도착 시 prefill 기아와 반대 경우. 발행한 token range/membership은 이후 재정렬·분할하지 않음 |
| prefill 복수 fragment | stage별 KV prefix보다 앞선 fragment 도착, 중복·역순 반환, 오류/취소 시 뒤 fragment 잔존, recurrent/verify/replay 경계. decode outstanding≤1, node backend 동시 실행≤1. fragment limit를 지우거나 순서 검사를 제거하는 변이는 실패해야 함 |
| 종료/재사용 | timeout 중 전송·native 결과 불명 보존, 진행 가능한 제어 채널, 모든 stage 정산 확인 후 슬롯 재사용·idle UNLOAD. 강제 프로세스 종료만으로 정상 정리 통과를 대신하지 않음 |
| 메모리/오프로딩 | device KV 우선 계획과 실제 backend 할당 비교, CPU weight 포함 peak RAM·VRAM·agent heap/receipt byte 구분. 부족한 구성은 계속 거부. 통합 메모리 중복 합산 금지 |

모든 기능 수정은 실패 반례 → 실제 소비 경로 → 독립 worktree의 재컴파일/해시 결속 변이로 증명한다.
core byte 계약에는 backend 중립성 시험, adapter/native 변경에는 해당 I gate를 적용한다.
관련 recurrent/hybrid/backend 조합이 미검증이면 복수 fragment 승격 범위에서 제외하고 비활성을 유지한다.
UTF-8 회귀는 기존 69~113토큰 실패 입력/seed/바이너리 기준과 조각 경계를 보존하여 별도로 검증한다.

실기는 모델/토폴로지별 기준선과 후보를 고정하고, 적재·prefill·decode·혼합·정리 분석창을 분리한다.
GPU 표본과 RPC span은 별도 지표다. 호스트 간 시계 오차 범위 없는 전역 겹침/홉 지연은 참고값으로만 둔다.
토큰 수/elapsed 분모, 미완료·실패·길이 종료, 동시 부하를 함께 보고한다. 단회 최고치와 3회 선별은 H5 승인이 아니다.
H5의 최소 8 paired 반복·4 holdout쌍, 유효 TPS/신뢰구간·TTFT/ITL 상대 및 절대 SLO 조건을 그대로 적용한다.
정상 종료·내용 품질·긴 웨이브·정산·UNLOAD·메모리 안정성을 함께 통과해야 선언 구성의 수용으로 기록한다.

<a id="hf-integration-contract"></a>

## 6.1.1 HF 어댑터 통합 수용 (2026-09-14 계획)

작업 범위·구현/저장소 소유와 선행 순서는 [HF 수용 계획](external-analysis-improvement-plan.md#hf-integration),
수정 경계는 [격리 계약](layer-isolation-contract.md#external-hf-boundary)을 따른다. 아래 HF-*는 새 예정 시험 ID다.
최초 계획의 과거 결과는 HF 수용 보고, 현재 source 이관은 이관 보고를 따른다. 독립 Python worker 시험을 P4 통합 통과로 승격하지 않는다.

| 예정 ID | 입력·실제 소비 경로 | 필수 판정 |
| --- | --- | --- |
| HF-PKG | P4 root에서 HF crate를 실제 의존한 locked build, package metadata/tree, 구현체→`Arc<dyn RetainedNodeAdapter>` 연결. feature를 쓰면 on/off 별도 build | `p4-adapter`/`p4-protocol` 각각 package ID/source/version 단일성. 중복 source를 넣은 독립 negative fixture는 타입 연결 실패 또는 graph gate 거부. 내부 workspace와 단일 source 복원 검증. P4 전체 workspace와 HF member 시험 모두 종료/summary 기록 |
| HF-REGISTER | 실제 agent event NODE_LOAD/INSPECT. enabled/disabled kind, 잘못된 kind·generation·용량. llama/HF node를 같은 agent에 적재 | LOAD 가능 kind와 광고 일치. disabled HF는 adapter/worker spawn과 모델 할당 전에 거부. Qwen 모델명은 P4 분기에 없음. 기존 llama도 같은 Agent-target 수명 계약 유지 |
| HF-RETAIN | queue/completion cap 1, 작은 고정 byte 한도에서 Full/Closed/peek/matching take/poll wake. 실제 RetainedEventNode·broker가 HF 어댑터를 소비 | 원본 allocation/claim 반환, peek 무소비, stale front 불소비, capacity 재개/wake 유실 없음. queued/held/IPC/출력 권한 보존. bytes 경계±1 거부 무효과, 입력 수용과 실행/하류 수용 분리 |
| HF-IPC | 실제 Rust↔fixture Python binary pipe에 magic/version/reserved/length 오염, partial header/body/write·flush 실패, stdout 오염, ready mismatch·초과 stderr·worker death·hang 주입 | 비호환/한도 초과는 tensor/model 실행 전 거부. 실패 후 stream 임의 재동기화/issue 자동 재실행 없음. uncertain·최초 오류·cleanup 오류 별도 보존. nonblocking retained 호출 안에서 blocking Python I/O 대기 금지 |
| HF-LIFE | P4 NODE_LOAD→여러 요청→취소/Release→NODE_UNLOAD→새 generation NODE_LOAD. 실행 중·결과 보유 중·큐 포화 시 취소/UNLOAD, 오래된 결과/해제 도착 | 취소 접수/발행 중단과 worker 정지 확인 분리. 미회수 출력/unknown snapshot/살아 있는 state가 있으면 UNLOAD 거부. child 종료·물리 state·route·owner와 claim 회수 뒤만 absent 성공. node task 건강성과 출력 수명 검사 우회 금지 |
| HF-MODEL | 아래 고정 Qwen dense FP32를 독립 controller와 P4 경로에서 실행. chunked prefill·decode·요청 교대·취소·slot 재사용·DeltaNet/attention cut·CPU/GPU 지원 조합 | 기존 logits atol=0.125/rtol=0.01와 매 스텝 greedy 동일 기준 유지. state 위치뿐 아니라 선언 cache의 구성/내용 parity를 요청별 참조와 비교. FP32 PASS로 기존 이기종 BF16 FAIL을 덮지 않음. 원본/양자화/분산 오차 분리 |
| HF-DIST | 같은 target·정밀도의 2개 실제 물리 host에서 각 P4 node가 HF worker/담당 weight/state를 소유. P4 event로 경계 tensor와 tail 결과 전달 | host/PID/장치·소스/모델/plan hash·할당·송수신 bytes·요청 귀속 증거. 중앙 Python이 직접 전체 worker를 순회하면 실패. disconnect/late reply 후 정산·다음 정상 요청을 검증. 로컬 여러 프로세스는 이 gate 미실행 |
| HF-SWAP | 한 P4 binary hash로 호환 Python bundle A→정상 UNLOAD→bundle B LOAD→같은 과제. 다른 IPC/schema/identity bundle도 투입 | 호환 교체에서 P4 재컴파일 없음, 실제 사용 bundle hash는 바뀜. 비호환 bundle은 LOAD 실패와 회수. Rust bridge 변경 시에는 별도 P4 binary/hash. 실행 중 bundle 덮어쓰기 금지 |

### HF 고정 시험 구성과 납품물

- Contract fixture는 input/completion queue 1·retained count 2·각 store 4KiB, IPC payload 최대 1KiB의 별도 profile로
  경계±1/큰 payload/출력 포화를 시험한다. 이 수치는 실제 Qwen readiness/tensor용 한도가 아니다.
- 실제 모델은 HF manifest의 Qwen3.5-0.8B revision을 사용한다. 첫 통합 profile은 FP32/quantization none,
  24층의 [0,12)/[12,24), context 2,048·요청 한도 8·출력 cap 64, 물리 batch 1·동시에 outstanding step 1이다.
  원본 `short/chunked_prefill/interleaved_cancel` 시나리오와 합법 attention 경계 profile을 유지한다.
  CUDA/CPU의 승인 FP32 구성을 사용하고 physical host/device identity는 실행 전 manifest에 고정한다.
- 기존 worker의 frame 상한 32MiB와 readiness metadata 64KiB, tensor/state 크기·복사·P4 retained/receipt 수명을
  합산해 모델 profile의 store/IPC/RSS 정수 예산을 산출한다. 모든 필드가 없으면 실행 전 거부한다. frame 상한을
  전체 heap 상한으로 보고하지 않는다. 요청별 timeout 120초, LOAD readiness 120초, idle shutdown 확인 5초를
  초기 통합 회귀 한도로 고정하며 성능 SLO와 구별한다. 초과 시 오류/격리·소유 child 정리를 별도 10초 한도로
  수행하고 무한 join을 허용하지 않는다. 이 한도를 사후 늘린 실행은 같은 수용 arm이 아니다.
- 추가 운영 회귀는 같은 load에서 8개 요청 block을 3번 처리·회수하고 매 block 사이에 다음 요청을 재수용한다.
  현재 `active+retired≤max_requests`의 누적 상한으로는 불가능하므로 새 실행 epoch/중복 방지 수명 계약과
  구 v1 계약을 구분해 검증한다. 이전 epoch의 늦은 step/result/release 거부와 state/claim 무효과를 확인하고
  중복 방지 검사를 삭제하거나 cap을 늘려 24건만 통과시키는 구현은 불합격이다.
  `short`만 성공하고 cancel/회수/재생성이 실패하면 수용 미완이다. 출력 cap 종료는 EOS/정상 답변과 분리한다.
  conformance의 teacher-forcing 비교와 일반 생성 전문/정답·형식 검토를 따로 남긴다.
- 변경된 각 권한/한도/identity 검사는 실제 소비 반례와 독립 수정 제거 변이로 증명한다. Rust 재컴파일·binary hash,
  Python source/import 경로·bundle hash를 함께 봉인한다. feature를 쓰면 HF 활성 시험을 필수 CI/실행 명령에 넣고
  기본 빌드에서 제외됐다는 이유로 HF 검증을 통과 처리하지 않는다.
- HF adapter 폴더에 독립/통합 runner·fixture·profile·환경 lock·실행/결과 보고를 두고, P4에는 생성/광고/공통 계약 회귀와
  P4 고정 revision으로 시험을 재현하는 명령·결과 색인을 둔다. sibling 없이 시작하는 복원 시험에서는 명시 build
  bundle이 하나의 P4 commit으로 모든 내부 source를 준비해야 한다. 테스트 중 임시 path만
  우연히 존재한 빌드는 재현 가능한 배포 증거가 아니다.

HF 수용 완료는 위 통합 capability의 완료다. 소형 Qwen으로 초대형 모델 H0–H7/성능 승격을 승인하지 않는다.
물리 host/bridge source가 없으면 해당 항목은 BLOCKED/미구현이며 후속 A 성공을 기다려 수용을 대신하지 않는다.
기존 llama timer RED 등 전체 workspace 결과는 별도 보존한다. HF 때문에 새로 생긴 회귀를 기존 실패로 분류하지 않는다.

<a id="release-a-contract"></a>

## 6.2 Release A 수용 계약 (2026-09-13 계획)

제품 범위·모델·구현 진입점은 [개발 계획 Release A](external-analysis-improvement-plan.md#release-a)가 소유한다.
아래는 **새 개발의 목표 계약**이며 구현/실기 통과 기록이 아니다. 기존 T/I/H/K 및 v1.1 회귀 조건을 대체하지 않는다.
A- 접두사는 예정 시험 ID다. 실제 테스트 함수/runner와 ID 대응을 구현 보고서에 남긴다.

### A 실행 명세와 비용 확인

대상은 2026-09-15 사용자 지시에 따라 개발 계획의 Qwen3.5-122B-A10B UD-Q5_K_S 3-shard·resident8로 변경한다. 최소2물리 host를 사용하며 정확한 fleet/stage/cut은 새 PLAN·공유 pool 예산·비용 비교 후 봉인한다. 과거550B의7host/8stage를 그대로 적용하지 않는다. 이번 릴리즈의
수용 목표는 짧은 질의 streaming과 긴 문서의 비동기 분석이다. 다음 새 수치는 측정 예측이 아니라
제품 사용 한도다. 임의의 향상률 대신 사용자 대기와 작업 완료에 상한을 둔다. 달성 가능성은 A-COST에서
검사하며 불가능하면 해당 릴리즈 FAIL/범위 재심사다. 사후 수치 완화로 기존 arm을 GREEN으로 바꾸지 않는다.

| 입력/운영 항목 | 실행 전 고정할 값과 판정 |
| --- | --- |
| 제품 context/생성 | template 적용 후 short 2–8k, medium 32k급, long 100,038 input tokens. sequence context 102,400, 출력 cap 2,048. long도 input+cap이 context 내인지 검사. 정상 응답의 `length` 중도 종료는 실패 |
| 사용자 대기 상한 | class별 TTFT p95: short 60초, medium 300초, long 900초. 각 class ITL p95 ≤250ms. submit 이후 end-to-end deadline: short 600초, medium 1,200초, long 1,800초. queue 대기 포함 |
| 상한의 목적 | 100k 입력은 실시간 채팅 약속이 아니라 최대 30분 비동기 작업. 250ms/token에서 cap 2,048 생성은 약 512초이며 long TTFT 예산 900초와 합쳐 deadline 내 운영 여유를 둠. 이는 모든 토큰의 간격 보장/실측 수용량을 뜻하지 않으며 요청별 deadline은 별도 검사 |
| 초기 적재·종료 | cold LOAD/SESSION 전체 3,600초 이내, 정상 drain/idle UNLOAD는 완료된 마지막 작업 이후 30초 이내. 요청 cancel 접수/새 발행 차단 1초 이내, graceful 회수 30초 이내. native 정지가 불명하면 30초 내 failed/uncertain·quarantine으로 전환하고 정상 회수 PASS로 세지 않음 |
| fault recovery | 격리한 epoch의 결과/새 작업을 차단. 소유 프로세스 종료와 장치/예약 해소가 확인된 뒤 새 epoch의 cold LOAD/SESSION을 3,600초 내 수행하고 정상 recovery wave 재수용. 확인 불가 host는 BLOCKED 유지; 자동 원격 KV 해소 추정 금지 |
| admission | resident 8, pending queue 최대 64건/128MiB serialized request/6,553,600 input tokens, 요청당 serialized input 최대 2MiB. 초과 전에 명시 거부. UTF-8/tokenizer별 입력 bytes를 실제로 검사. 전체 max token/byte를 둘 다 적용 |
| host/device/edge/output/receipt | native PLAN이 내는 소유 weight/KV/보조 state/최대 physical result/scratch와 INSPECT의 실제 available·host 공용 pool을 사용해 **각 stage와 edge의 정수 byte 상한**을 manifest에 산출. input 예산만으로 대체 금지. count×max serialized response, pending/retained/receipt의 별도 수명과 공유 payload 중복 과금을 명시. required field 누락·무한대·음수·잔여량 초과이면 실행 전 거부 |
| 현재 관측 명세 | GPU/host 표본 주기 1초, sample coverage ≥95%, wall-clock host skew 허용 10ms 이하일 때만 host 간 세부 overlap 수치 판정. 미충족은 해당 분석 INVALID이며 0ms로 채우지 않음. 동일 host monotonic latency와 client TTFT/ITL은 별도 보존 |

위 memory byte 값은 추정 고정 상수로 장비에 밀어 넣지 않는다. PLAN→manifest materialization이
필수 구현이다. 생성된 정수 예산과 환경을 A/B 전에 봉인하며, host reserve는 OS/기존 서비스와 실제
할당을 뺀 실행 가능량에서 명시한다. 현재 available을 넘으면 진행하지 않는다. topology/quant/cut
선택을 마친 뒤 정책 A/B를 시작한다. config 안에 실험 중 자동으로 커지는 한도는 금지한다.

### A corpus와 비교 arm

1. 기존 `target/nemotron550-all-fleet/`의 100,038 input ×8·output cap 2,048·원래 deadline 및
   `target/nemotron550-mixed-waves/`의 미시작 workload를 **원본 그대로 역사 회귀 증거**로 보존한다.
   사용자가550B를 대상에서 제외했으므로 새 Release A의 선행 조건으로550B 재실행을 요구하지 않는다.
   해당 실패는 그대로 유지하며 Qwen의 통과로 대체하지 않는다. Qwen corpus는 별도 명세·tokenizer·token ID로
   재생성하고 동일 정답·입력 등급·8wave·SLO를 적용한다. 원본 유실 시 재현 불가를 명시한다.
2. 제품 cold burst: short 4건, medium 2건, long 2건을 시점 0에 함께 제출한다. sustained는 8개 wave×8건,
   매 wave 같은 길이 구성에 서로 다른 과제를 사용한다. 초기 고정 도착은 0/180/480/780/1080/1380/1680/1980초.
   예정 시각 대비 실제 송신 오차는 최대 1초. 전체 timeout은 마지막 예정 송신+1,800초이며 요청별 deadline도 검사한다.
   이 schedule에서 겹침이 안 생기면 H2에 따라 양 arm 전체를 같은 새 spec으로 다시 실행한다. 응답을 기다렸다가 송신하지 않는다.
3. 공개 문서/코드와 고정 seed의 사실/표 자료를 사용한다. short는 코드 수정/형식 답변, medium은 다문서 사실 연결,
   long은 여러 위치의 근거를 결합하는 질의로 구성한다. 무의미한 반복 padding 금지. 정확한 원문·생성기·자료 버전·
   tokenizer/template·token IDs/hash·정답/근거·기대 stop을 tracked corpus manifest에 보존한다. fixture 의미 검토는
   양 arm 측정 전에 끝낸다. 기존 `required_substrings`만으로 정답 판정을 대신하지 않는다.
4. controlled 문제는 정답·형식·출처를 기계 판정하고 일반 답변은 hash-bound 의미 검토한다. 정상 서비스의 모든 요청이
   H1을 만족해야 한다. baseline 모델도 풀지 못하는 fixture는 H1의 공통 버전화 절차로만 바꾸며 실패 원문도 보존한다.
5. recovery는 같은 load에서 최소 3개 wave block을 drain/재수용한다. 별도 fault arm은 head/intermediate/tail의 native 지연,
   return 중단, output backpressure, cancel 시점을 교차한다. overload는 최소 16건 동시 burst와 queue/byte/token 각 상한±1을
   분리 시험한다. 정상 corpus에서 거절한 건을 정상 완료로 세지 않는다.
6. 비교는 (a) 현재 소스의 spec-off 기본 정책, (b) 안전성 수정만 적용한 동일 profile, (c) 선정 batch profile을 구분한다.
   진단 baseline이 실패하면 기능 회복만 판정하고 실패한 baseline으로 H5 개선율을 계산하지 않는다. 정상 baseline과 후보의
   최적화 비교는 H5의 8 paired/4 holdout·품질·상대/절대 SLO·5%/신뢰구간 문턱을 모두 적용한다. 새로운 source/layout은
   별도 arm이다. 모든 arm 결과와 선정하지 않은 이웃 후보를 남긴다.

### A 반례와 실제 소비 경로

| 예정 ID | 실패 반례/실행 환경 | 통과 oracle와 변이 |
| --- | --- | --- |
| A-RED | 현재 `phase_pacing_actual_loop_expires_decode_wait_without_another_tail_or_input` 단독/선택 묶음. 실제 Worker와 fake native의 event 순서 수집 | timer 대기 자체는 request/flight/slot/input authority를 변경하지 않음. 정상 RELEASE가 끼었다면 선형화 지점을 고정해 구분. 관련 권한을 timer가 바꾸는 변이는 실패. 기존 assert 삭제로 수용 금지 |
| A-PLAN | 실제 OUTER→native PLAN/LOAD. available 축소, 동일 host pool 이중 사용, 불법 cut, shard/patch/backend/layout mismatch, 부분 LOAD 실패 | 예상보다 큰 allocation/다른 placement를 조용히 수용하지 않음. 이미 만든 자원까지 추적 회수. 순수 정책 6,678건 재통과는 필요 회귀지만 실기 대체 아님. 소비 경로 검사를 제거하는 변이 실패 |
| A-BYTES | 개수는 작고 bytes 큰 결과, 정확 경계±1, 중복/늦은 응답, receipt 보존 중 queue 포화, 전송 결과 불명. core는 중립 fake adapter도 시험 | reservation 전에 native/output 효과 0. 거부 전후 모든 원장/예약/credit/output 동일. valid duplicate의 효과는 1회. expiry/eviction으로 유효 duplicate 증거 유실 금지. reserve/commit/response bound 하나 제거하는 변이 실패 |
| A-LIFE | submit→issue 전/후, partial output, tail result 전/후 cancel; Drain 중 새 요청; head/mid/tail 단절; slot 재사용 후 옛 generation 반환 | cancel 선형화 전에 승인된 출력은 보존, 이후 새 발행 금지. stage별 정산·KV quiescence 확인 후만 reusable. 불명 상태 분리. 정상 recovery 3회에서 active owner/flight/미정산 회수 의무 0. 중복 판정용 retained receipt는 유효 수명 동안 보존하며 별도 byte 상한/H7 유지. fence/epoch/release 검사 제거 변이 실패 |
| A-BATCH | cap 1/8, D 수요 cap 미만/동일/초과+P, 다중 session, continuous arrival, controller on/off, prepare 거부 반복. selector와 실제 Worker 모두 실행 | 선택된 session의 eligible P는 그 session에 허용된 최대 8개 승인 issue 안에 진행. 계속 eligible인 session은 활성 session 수만큼 승인 session 선택 안에 차례를 받음. 두 bound를 합성한 전체 대기도 보고. 거부는 차례/정책 예산을 소비하지 않음. D의 실시간 SLO는 A-SERVICE로 별도 검증. equal-width/atomic verify/replay 유지. reserve-P/session 회전/거부 보존 중 실제 수정 제거 변이 실패 |
| A-COST | 같은 position/rows에서 unified KV occupancy/다른 sequence/CPU expert layout 변경. native decode→capture→return→Worker forward→client output 전체 timestamp 수집 | 실제 n_kv·phase·mask/graph·copy/network·sample·queue를 분리. 불명 시간을 compute로 몰지 않음. 실제 소비한 profile/blocked reason에 결속. n_kv 또는 return cost를 의도적으로 누락하면 비용 conformance가 실패. 계측 자체 overhead도 같은 arm으로 보고 |
| A-SERVICE | 위 target/fleet의 cold·sustained·recovery·overload·fault 전체, 실제 CLI 사용자 명령 | H0–H7 및 위 절대 SLO·자원·회수/복구 조건. 정상 요청 전문·EOS/정상 stop·품질·정산·회수 모두 통과. missed evidence/first error/cleanup error 각각 보존. failed arm을 빼거나 강제 kill을 graceful PASS로 세지 않음 |

위 round-based fairness는 **자원상 발행 가능한 작업**의 상한이다. credit/KV/native가 계속 막혀 있으면
그 시간을 빼서 사용자 SLO를 성공으로 만들지 않는다. 실제 blocked 기간을 기록하고 deadline 또는 명시
admission 거부로 종결한다. 한 요청 여러 fragment를 지원하지 않는 A에서도 독립 요청 간 전진을 증명한다.

출하 증거에는 CLI 제출/조회/취소/drain/recovery 명령, 지원 profile과 오류, machine-readable manifest와
요약 생성기를 포함한다. 전체 workspace 종료 결과·관련 native/backend·독립 변이·다중 host 승인을 따로
기록한다. raw artifact는 hash/접근 경로, corpus/실행 방법/요약은 tracked 경로에 보존한다. 필수 runner가
미구현이거나 장비가 없으면 그 gate는 미실행/BLOCKED이며 체크박스로 승인하지 않는다.

## 7. 최종 완료 체크리스트

- [ ] 모든 필수 T gate 구현·실행·mutation과 실제 소비 경로 증거가 있음.
- [ ] I gate로 층별 의존과 native/update 경계를 검증했으며 선언 backend의 실제 승격 증거가 있음.
- [ ] 사용한 영속/스냅샷 기능의 K gate를 통과했으며, 비활성 범위는 명시됨.
- [ ] 선언 native/model/backend 기능의 게이트 통과; 사용하지 않은 기능은 비활성/미완으로 표시.
- [ ] H0~H7의 최종 초대형 모델·다중 물리 컴퓨터·강한 웨이브·정상 응답 전문 증거가 있음.
- [ ] 유효 TPS와 GPU 활용 개선/한계를 같은 창에서 보고; RPC/GPU·prefill/decode·구조/의미 혼동 없음.
- [ ] 실패·제외·미실행을 숨기지 않았고, 결과가 실제 배포 이미지와 source 상태에 결속됨.
- [ ] 전체 증거를 새 세션/다른 머신에서 재열람·재현할 수 있으며, 로드맵의 다음 행동이 갱신됨.

문서 체크박스는 실행기가 아니다. 필수 gate의 자동 실행·CI wiring은 단계 산출물이며,
그것이 없으면 수동 기록으로 대체했음을 밝히고 자동 검증 완료를 주장하지 않는다.
