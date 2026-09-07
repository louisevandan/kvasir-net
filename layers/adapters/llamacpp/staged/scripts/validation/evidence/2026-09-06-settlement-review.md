# 2026-09-06 — 공유 정산 추출의 실제 범위와 미해결 반례

종류: 코드 감사와 CPU-only 결정론적 검사. GPU/원격 실행 증거가 아니다.
최초 감사 기준: `a9e1967fc59dffa6c2e458f1b91f916b1df826c1`, 최초 검사 당시 작업 트리 clean.
아래는 시간순 기록이다. **최신 상태는 마지막 후속 구현 절**이며, 과거 RED/clean을 현재 상태로 읽지 않는다.
현재 작업 순서는 [로드맵](../../../../../../../docs/distributed-batching-roadmap.md),
시험 승격 기준은 [검증 규약](../../../../../../../docs/distributed-batching-verification.md) 소유다.

## 실행한 것

- `cargo test --workspace --no-fail-fast`: 844 passed / 0 failed / 7 ignored, exit 0.
- `test/benchmarks/p4-4node`의 추적 `*.test.mjs`: 57 passed, exit 0.
- C++와 clippy는 이 감사에서 재실행하지 않았다. 이전 결과를 이번 실행으로 세지 않는다.
- 독립 임시 복사본에 기존 worker test fixture를 확장해 실제 `CapsuleSet::encode/decode`와
  `Worker::tail`, 일부 `Worker::handle`을 호출했다. full worker loop나 transport 전체를 시험한 것은 아니다.
- 아래 probe는 당시 임시 복사본에만 있었으며 **아직 저장소의 정식 회귀 시험이 아니다**.
  다음 세션은 검증 규약 T10~T13/T17로 재현·커밋해야 한다. 임시 경로는 장기 의존 대상으로 삼지 않는다.

## 수용한 변경

`v2/node/state.rs::RequestState::settle_fragment`는 요청의 outstanding과 프롬프트 경계를
검사한 뒤 변경한다. 이 함수 자체의 거부는 요청을 보존한다. 운영 `Worker::tail`과
`Simulation::advance`가 실제로 호출하므로 소스 공유는 존재한다.
cohort별 최대 미선택 간격은 마지막 구간까지 검사하고, 9회는 cohort 상한으로 주석이 축소됐다.

## R-A — 반환 이벤트 전체의 원자성은 없다

앵커: `v2/node/worker/release.rs::Worker::tail` @ a9e1967fc.

재현 절차:

1. session/request/load generation을 맞추고 prompt 길이 10, issued=4, cursor=0, outstanding=1로 둔다.
2. `AdapterState::open_batch([1])`로 실제 열린 원장 상태를 만든다.
3. execution=1의 terminal prefill capsule에 6행을 넣어 tail에 전달한다.
4. 요청 경계 오류는 반환되지만 열린 batch가 1→0으로 감소한다. 요청은 cursor=0/outstanding=1로 보존된다.

혼합 반례: A(issued4)에 4행, B(issued2)에 3행을 같은 CapsuleSet으로 반환한다.
A를 먼저 순회하도록 ID를 정하면 오류 뒤 상태는 A=(cursor4,outstanding0), B=(0,1), 열린 batch=0이다.
`Worker::handle`에 같은 과다 반환과 정상 completion publisher를 주면 오류 응답을 발행하고
`Ok(())`를 반환하면서 열린 batch=0을 유지한다. 따라서 worker 종료를 통한 fail-closed라는 해석도 틀리다.

원인: `close_execution`이 검증보다 먼저이고, 요청별 validate/write가 한 루프에서 섞인다.
요구: 반환 이벤트 전체 사전 검증과 요청·원장·출력 의도의 원자 반영(T10/T11).

## R-B — 이전 반환과 잘못된 소유/구간이 정산된다

앵커: `v2/node/state.rs::RequestState::settle_fragment`,
`v2/node/worker/release.rs::Worker::tail` @ a9e1967fc.

- F1 `[0,4)`를 정산한 뒤 F2 `[4,8)`이 유일한 in-flight인 상태를 만든다(issued8/cursor4/outstanding1).
  F1 캡슐을 새 event ID로 재전달하면 `Ok(())`, cursor8/outstanding0이 된다.
  F2 execution은 열린 원장에 남아 요청 counter와 갈라진다. 다중 fragment opt-in 없이도 성립한다.
- partial prefill 반환에 outcome 없이 sequence ID만 99로 바꾸면, 실제 request sequence0이어도 수용된다.
- 실제 issued `[0,4)`에 대해 invocation과 owner의 position을 함께 `[5,9)`로 바꾸면
  캡슐 포맷 검증을 통과하고 tail은 cursor4/outstanding0으로 수용한다.

요구: 발행 identity/range/membership와 대조하고 중복은 다음 비행을 소비하지 않음(T12/T13).
이는 보고서가 미착수로 밝힌 멱등 원장의 구체적인 위험이며, 이번 추출이 새로 만든 회귀라고 단정하지 않는다.

## R-C — 실제 simulator 경로의 공유 연결은 시험이 보호하지 않는다

앵커: `v2/simulator_tests.rs::the_worker_and_this_model_settle_through_one_transition`,
`v2/simulator.rs::Simulation::advance` @ a9e1967fc.

새 시험은 Simulation을 실행하지 않고 RequestState 함수를 직접 호출한다.
검증용 복사본에서 `advance`의 공유 함수 호출을 지우고 outstanding 감소/cursor 증가를 별도 구현하며
행 경계 검사를 제거해도 simulator tests **11/11이 통과**했다. 변이는 복사본에서 원복했다.
따라서 “모델 1개 실패”를 Simulation 실제 도착 경로의 거부 검증이라고 보고할 수 없다.
요구: malformed travelling fragment가 실제 advance를 지나 같은 거부와 상태 보존을 보이고,
공유 경계 검사를 우회하는 변이를 검출(T17).

## 다음 작업

위 반례를 정식 시험으로 보존한 뒤 발행 기록과 원자적·멱등 정산을 연결한다.
함수 추출의 완료와 전체 인플라이트/실기 성과의 완료는 다르다. 작은 카운터 함수 추출을 반복하는 대신
“발행한 바로 그 작업만 정확히 한 번 정산한다”를 다음 구현 단위로 삼는다.

## 같은 날의 문서 이관 검증

기준 HEAD는 같고 문서/문서 게이트만 수정된 작업 트리에서 수행했다. 추론 코드 변경·원격 배포·GPU 실행은 없다.

- 전체 Rust 재실행: `cargo test --workspace --no-fail-fast`, 844 passed / 0 failed / 7 ignored, exit 0.
- 하네스 재실행: 57 passed / 0 failed, exit 0.
- 문서 자체 시험: 12 passed. 기존 9건에 document-map 정상/누락/깨진 대상 3건 추가.
- docs-lint: 추적 73문서 및 신규 포함 `--all` 79문서 통과. 새 파일은 아직 Git index에 넣지 않은 상태의 별도 검사다.
- 마지막 문서 수정 후 `cargo test -p p4-agent --test docs_lint` 1 passed로 기존 cargo 연결도 확인.
- private-header 게이트: 74 source 파일, include 패턴 기준 header 부채 0 / source 부채 5.
  타입·transitive build 격리까지 성립한다는 뜻이 아니며 [계층 격리 계약](../../../../../../../docs/layer-isolation-contract.md)에 실제 빈틈을 기록했다.
- C++/GPU/원격/새 I/T/K/H 시험은 이번 문서 이관에서 실행·구현했다고 주장하지 않는다.

새 문서가 요구하는 결정론적 T, 계층 격리 I, 조건부 저장 K, 최종 실기 H는 **앞으로 구현·실행할 게이트**다.
문서 게이트 통과를 이 기능들의 통과로 세지 않는다. 커밋/푸시는 이 문서 정리 요청에 포함되지 않는다.

## 후속 반례 봉인과 계층 격리 보강 — 같은 HEAD의 작업 트리

위 844/0/7은 반례 추가 **전** 결과다. 아래 변경 뒤 녹색 기준선으로 재사용하지 않는다.
프로덕션 정산 수리는 아직 하지 않았다. 테스트 전용 simulator 주입기와 정식 실패 반례를 보존했다.

| 실제 소비 경로 / 시험 | 새 반례와 실행 범위 |
| --- | --- |
| `worker_tests.rs::t10_rejected_tail_preserves_the_open_batch_and_all_request_bookkeeping` | 과다 반환을 거부해도 open batch가 사라짐 |
| `worker_tests.rs::t11_one_bad_request_rejects_the_whole_tail_in_either_capsule_order` | A를 정산한 뒤 B를 거부. 첫 실패 때문에 뒤 permutation은 아직 이 실행에서 도달하지 않음 |
| `worker_tests.rs::t11_a_late_outcome_error_cannot_settle_any_other_request` | 뒤늦은 outcome 오류 전 A cursor/outstanding과 B generated가 이미 변경됨 |
| `worker_tests.rs::t12_old_execution_in_a_new_event_never_consumes_the_next_fragment` | F1 중복이 F2 outstanding을 소비 |
| `worker_tests.rs::t13_partial_prefill_refuses_a_different_sequence_without_an_outcome` | outcome 없는 다른 sequence 수용 |
| `worker_tests.rs::t13_partial_prefill_refuses_a_different_position_range` | 올바른 행 수의 다른 구간 수용 |
| `worker_tests.rs::t13_partial_prefill_refuses_an_unregistered_execution` | 미등록 execution 수용 |
| `worker_tests.rs::t13_partial_prefill_identity_is_checked_without_waiting_for_an_outcome` | key 변조에서 실패. 뒤 generation/phase/request/invocation 사례는 아직 실행 검증 아님 |
| `simulator_tests.rs::a_malformed_arrival_preserves_the_simulation_ledger_and_request` | 실제 run→advance에서 4행 발행→6행 도착을 거부하지만 travelling 항목을 먼저 삭제 |
| `simulator_tests.rs::a_wrong_range_arrival_cannot_settle_the_right_number_of_rows` | 실제 run→advance에서 [0,4) 발행→[1,5) 반환을 수용 |

위 worker 파일은 `v2/node/`, simulator 파일은 `v2/` 아래다. worker 선택 실행은 기존 3 passed /
신규 8 failed, simulator `arrival` 선택 실행은 0 passed / 2 failed였다. 반환은 실제 capsule decode를 지나지만
full worker loop·stage I/O·handle 후 계속 운행·외부 출력 검증까지 통과한 시험은 아니다.
fixture의 발행 등록도 아직 ID 집합뿐이며 expected membership 등록은 B1 구현과 연결해야 한다.
현재는 수정 전 반례이므로 mutation 완료도 주장하지 않는다.

### R-D — 출력 승인과 native 실행 여부의 경계

앵커: `v2/node/worker/drive.rs::Worker::emit_tail_results` @ a9e1967fc.
OUTER 토큰을 모두 발행한 뒤 head로 TAIL_BATCH를 보낸다. 따라서 head만 원자화해도 이미 발행한
출력을 되돌릴 수 없다. 이는 코드 순서로 확인한 결함이며, 이번에 전체 wire 경로에서 실증한 것은 아니다.
검증 규약 T11/T23과 격리 계약의 L1 commit + L5 effect intent 경계로 처리한다.

native `server/server_physical.cpp::Session::handle_logical_batch` @ a9e1967fc는 head 계산 이후
physical execution ID를 만든다. Rust의 발행 원장은 native 호출 전 논리 PreparedIssue와
PhysicalResult 검증 후 AcceptedIssue를 구별해야 한다. 응답 유실은 Uncertain이며 미발행으로 재시도할 근거가 없다.
logical allocation 하나가 여러 physical capsule에 걸칠 수 있으므로, 부분 capsule마다 request outstanding을
감소시키면 안 된다. 완성된 membership/연속 구간과 execution/batch 완료를 별도 대조한다.

### 계층 격리의 현재 코드 표면

- `server/CMakeLists.txt`: runtime의 `llama-common` PUBLIC link와 imported relink `_p4_inc` 전파가 남음.
- `compat/p4_llama_compat.hpp::LlamaPlan::impl` 및 public include root로 opaque 내부 접근이 열려 있음.
- `runtime/request_options_grammar.hpp::parse_grammar_triggers`는 전방 선언 common 타입을 서명에 노출.
- `v2/capsule.rs::TensorDescriptor`와 `Invocation`의 raw 정수 코드에 codec 의미/협상 대조가 필요.
- 기존 `layers/agent/tests/stays_neutral.rs`는 중립성 이름/manifest canary 기반이며 I00 전체 graph·mutation의 대체가 아님.

실제 수정 허용 범위·API 대장·target 권한·engine/common과 ggml/backend의 변경 분류는
[계층 격리 계약](../../../../../../../docs/layer-isolation-contract.md)에 단독 기록했다.
이번 보강은 그 구조의 구현 완료가 아니라 다음 구현의 계약과 시험 제약이다.

### 반례 추가 후 전체 재실행

- source: HEAD `a9e1967fc`, 작업 트리 문서/문서 gate와 위 3개 테스트 관련 Rust 파일 변경. 커밋하지 않음.
- `cargo test --workspace --no-fail-fast`: **844 passed / 10 failed / 7 ignored**, exit **101**.
  최종 종료를 확인하고 전체 57개 summary를 합산했다. 실패는 위 신규 worker 8개·실제 simulator 도착 2개다.
  미실행 외부 fixture feature는 이 집계의 pass가 아니다.
- 로컬 원본 로그: `target/layer-isolation-doc-review-workspace.log` (임시 산출물).
  장기 재현은 저장소의 반례와 위 명령에 의존하며, 이 로그 경로만으로 B1 완료를 주장하지 않는다.
- 문서 자체 시험 12 passed, 신규 포함 docs-lint 79문서 통과. private-header 패턴 gate는 74파일,
  header 부채 0 / source 부채 5로 동일. 이것은 I00~I09 전체 PASS가 아니다.
- 하네스/C++/GPU/원격은 이번 보강에서 미실행. 마지막 문서 수정 뒤 문서 gate만 다시 실행한다.
- 다음: 시험을 지우거나 skip하지 말고, 실제 발행 기록/정산/효과 소유 경계를 구현해 이 실패들을 닫는다.

## 후속 구현 — 발행 권위·정산 transaction·가짜 native seam

같은 HEAD의 **미커밋 작업 트리**에서 구현했다. 앞 절의 반례 10개는 이제 실행을 통과한다.
새로운 분산 배칭 전체, B1/B2 완료, native 의미 호환 또는 실기 성능을 승인하는 기록은 아니다.
이번 기능 변경은 `layers/adapters/llamacpp/staged/adapter/` 안에 한정했다.
P4 공통 protocol/agent의 payload 의미나 llama.cpp/CUDA 코드는 바꾸지 않았다.

### 소스와 실제 구현 경계

| 경로 / 심볼 | 구현 / 한계 |
| --- | --- |
| `v2/node/state.rs::RequestState::issue_fragment`, `settle_fragment` | 운영 발행/도착과 Simulation의 공유 부기. 검사가 끝난 후 issued/cursor/outstanding 변경 |
| `v2/node/state.rs::AdapterState::prepare_issue`, `accept_prepared_issue` | resident 권위와 후보 전체 대조, Prepared/AwaitingNative/Uncertain, 정확한 native split 승인 뒤 요청 전진. 취소 API는 아직 production 취소 소비자가 없음 |
| `v2/node/flight.rs::FlightLedger` | invocation/owner/membership 권위, partial/역순 buffering, logical fragment별 정산, receipt와 실행 ID high-water. Verify/Replay는 atomic group을 보존 |
| `v2/node/worker/release.rs::Worker::tail` | 전체 요청 후보·outcome·효과 의도 사전 검증, counter와 식별 원장 독립 대조, 원장/요청/의도 commit 후 실행 |
| `v2/node/worker/outcome.rs::apply_fragment` | prefill/decode/verify/replay의 position·생성량·stop·proposal·KV 후속 상태를 정규화. 엔진의 실제 token 생성은 대체하지 않음 |
| `v2/node/worker/effects.rs::Worker::flush_effects` | output/forward/settle/release 의도 보존. 외부 효과 실패 후 남은 의도와 fence 유지. 내구 outbox나 crash exactly-once 아님 |
| `v2/node/worker/drive.rs::Worker::emit_tail_results` | 꼬리는 head로 TAIL_BATCH만 보냄. OUTER 출력은 head 승인 뒤 한 번 발행 |
| `v2/node/worker/settlement.rs::Worker::settled` | physical outstanding과 pending KV ack 분리, sampled token·position·proposal 권위, 이벤트 전체 사전 검사 |
| `v2/node/worker/release.rs::Worker::released` | pending release 권위·슬롯·admission 후보 전부 확인한 뒤 슬롯 반환. 같은 key/slot의 새 incarnation 구분은 아직 없음 |
| `process/core.rs::ServerControl` 및 `v2/node/worker/stage_tests.rs` | 기존 trait에 Box 전달 구현. 실제 handle/drive/stage 명령/반환을 fake native로 시험. LOAD 협상과 전체 수신 루프는 우회하므로 full worker E2E 아님 |

FlightLedger의 완료 receipt 창은 64MiB/4096개로 제한하며 만료된 옛 ID는 fail-closed다.
이 수치는 active flight·edge tensor·pending queue의 메모리 상한이 아니다. crash 이후 receipt도 아니다.
기존 open-batch 표시는 ledger에서 재생성되는 관측값이며 단독 완료 권위로 쓰지 않는다.

### 실제 경로의 회귀 시험

경로 루트는 `layers/adapters/llamacpp/staged/adapter/src/`다. ID 전체 PASS가 아니라 아래 부분 입력의 증거다.

| 시험 묶음 | 이번 실행 / 증명 범위 |
| --- | --- |
| `v2/node/worker_tests.rs` | 26개. R-A/B/D, 잘못된 key/generation/phase/range/member, handle 거부 후 정상 반환, 부분/역순 정산, 중복/conflict, head 이전 출력 금지, Closed/부분 출력 후 intent 내용·join, counter 불일치 |
| `v2/node/issue_tests.rs` | 10개. resident 대비 계획 검증, issue 공유 호출, native 전 취소/불확실 이후 재발행 금지, 정확한 split, atomic capsule 분할 거부, ID 한계 |
| `v2/simulator_tests.rs` | 15개. 실제 run/issue/advance에 malformed 행·구간을 주입, 거부 후 travelling/요청 보존, 잘못된 전체 후보의 부분 commit 방지, 기존 공정성·시계·오류 cap |
| `v2/node/worker/outcome.rs` 시험 | 12개. ordinary/verify/replay 정상·거부, max_tokens/position/stop/proposal. 실제 native 샘플러 시험은 아님 |
| `v2/node/worker/settlement.rs` 시험 | 9개. 늦은 잘못된 ack에도 전체 상태 보존, proposal sampled-token 대조, 물리 outstanding이 남은 정산 거부 |
| `v2/node/worker/release_tests.rs` | 3개. 미소유/혼합 ack·잘못된 admission의 원자 거부와 정상 소유 슬롯 반환 |
| `v2/node/worker/stage_tests.rs` | 9개. 실제 drive가 fake native split을 보존, native 응답 유실/행 누락을 Uncertain으로 유지, 마지막 physical member 전 다음 issue 차단, mutating native의 잘못된 body 후 추가 호출 금지 |

fake stage는 Frame 응답을 만들 뿐 selector/정산을 다시 구현하지 않는다. 하지만 이 시험은
`Worker::run`→broker→N개의 실제 worker 전체를 연결하지 않는다. stage 수 1/2/4/8·지속 input·cancel·
재접속·종료와 native conformance는 별도 미완이다. simulator도 FlightLedger/outcome/effects 전체를
쓰지 않으므로 “engine 결과만 다른 완전한 동일 상태 머신”으로 보고하지 않는다.

### 독립 복사본 변이 검증

원본 checkout을 변경해 변이를 복구하지 않았다. 각 묶음은 작성 당시 소스 snapshot에서 수행했으며
모두 최종 소스 한 snapshot에서 수행한 것으로 합쳐 주장하지 않는다. 최종 정상 suite는 별도로 실행한다.

| 제거/오류 변이 | 실제 검출 |
| --- | --- |
| Simulation 공유 settle 우회·range 검사 제거·arrival 조기 commit | 실제 malformed arrival/range 시험 실패 |
| Simulation 공유 issue 우회·outstanding 증가 제거 | 잘못된 발행 후보 및 fragment 1/2/4 독립 원장 시험 실패 |
| worker issue 공유 호출 또는 resident authority 검증 우회 | issue 소비 시험 각 2개 실패 |
| 꼬리 OUTER 선출력 복원 | head 승인 전 출력 시험 실패 |
| effect를 성공 확인 전에 pop | 출력 거부/부분 발행 보존 시험 3개 실패 |
| 늦은 outcome 검사 전 flight commit | 이벤트 전체 보존 시험 실패 |
| request-vs-flight 독립 대조 우회 | T19 실제 worker 반례 실패 |
| KV sampled-token 대조 제거·후속 ack 검사 전 후보 commit·outstanding 허용 | 각각 해당 KV ack 시험 실패 |
| release 소유 검증 제거·부분 슬롯 조기 반환·옛 admission pop 순서 | release 시험 각각 실패 |
| max-open gate 우회·native split 누락 검사 제거 | 실제 drive의 native 호출/불완전 issue 시험 실패 |
| atomic physical membership 검사 제거 | 2+2 capsule로 찢긴 Verify/Replay 거부 시험 실패. 내부 재정렬만의 독립 변이는 미실행 |
| native SETTLE short/length/middle-proposal, RELEASE status의 사후 fence 제거 | 각각 해당 fake stage 시험 실패 |
| handle 진입 fence 제거 | 추가 오류 이벤트 효과 금지 단언 실패. 내부 stage guard는 native 재호출을 여전히 막으므로 native 호출 증가로 설명하지 않음 |

임시 상세 로그는 `target/tail-mutations-20260906-01/verification.txt`,
`target/release-mutations-20260906-01/verification.txt`,
`target/atomic-membership-mutation-20260906-01/verification.txt` 및 독립 Temp 복사본의 `results.json`에 있다.
이는 로컬 보조 증거이며 장기 재현은 위 저장소 시험/변이 위치와 명령에 의존한다.
공유 Cargo target이 baseline 바이너리를 재사용한 초기 변이 실행은 **무효로 제외**했다.
채택한 사후 fence 변이는 각 별도 target에서 실제 compile과 해당 실패를 확인했다.

### 아직 닫지 않은 결함과 다음 개발 제약

- 같은 load/session/key/slot 재사용 뒤 옛 RELEASE/RELEASED가 새 요청에 도달하는 incarnation 구멍.
  head 검사뿐 아니라 모든 홉의 native KV 효과를 보호해야 한다. wire 버전·연산 receipt가 필요하다.
- 중간 stage의 동일 physical 재전달은 head receipt보다 앞에서 native를 다시 호출할 수 있다.
  이번 중복 terminal no-op가 downstream KV/sampler 멱등성을 증명하지 않는다.
- `RequestState::clone`의 prompt/Event 복제와 effect의 cut-set 복제, 모든 열린 batch/owner 재검색.
  안전성 확보용 현재 후보 구현을 고성능 기준선으로 승격하지 않는다. 불변 입력과 작은 진행 delta,
  membership index로 바꾸면서 기존 거부·원자성 시험을 유지해야 한다.
- 계속 차는 입력을 모두 비운 후 drive하는 루프는 발행 기회를 굶길 수 있다. selector fairness와 별개다.
- selector가 plan 생성 때 cohort resume/decode_runs를 변경한다. 이번 issue counter 원자화만으로
  거부된 계획의 fairness 소비도 사라졌다고 말할 수 없다. 후보 정책 delta와 accepted commit 분리가 남아 있다.
- active row/byte/KV credit, durable/reconnect 수렴, cancel/reload/graceful drain, 제품 LOAD identity,
  common/private/transitive 격리와 declared backend conformance는 미완이다.

이 후속 작업의 상태는 **B1/B2 IN_PROGRESS**다. 다음 순서의 단독 소유는 로드맵이며, 위 목록을
새 U/P 직렬 단계표로 만들지 않는다. C++/GPU/원격/다중 머신 웨이브·성능 결과는 이번 기록에 없다.

### 최종 정상 실행과 소스 결속

- `cargo test --workspace --no-fail-fast`: **914 passed / 0 failed / 7 ignored**, 57 summary, 최종 exit 0.
  staged adapter lib는 221 passed. 초기 감사 844에서 70개 증가했다. 전체 수에는 과거 runtime 시험도
  포함되므로 914개 모두를 event worker 증거로 세지 않는다. 외부 fixture feature는 실행하지 않았다.
- 하네스 `node --test`: 57 passed / 0 failed / 0 skipped, exit 0.
- docs-lint 자체 시험: 12 passed / 0 failed. private-header 패턴 게이트: 74파일, header 0/source 5.
- 문서 gate: 추적 73개, 신규 포함 `--all` 79개 통과. 패치 뒤 혼합 EOL을 검출했으며 해당 문서만
  파일 내 CRLF로 정규화한 뒤 재검사했다. 파일 개수는 기능/의미 정확성 인증이 아니다.
- `cargo clippy -p p4-llamacpp-staged-adapter --all-targets`: exit 0, **warning은 남아 있다**.
  새 `cancel_prepared_issue`의 production 소비자 부재 경고도 있다. `-D warnings` 또는 경고 0 통과가 아니다.
- C++ CTest·실제 llama 라이브러리·GPU·원격은 미실행. commit/push/deploy 하지 않았다.
- 정상 실행 원본 로그: `target/b1-settlement-workspace.log`, clippy: `target/b1-settlement-clippy.log`.
  명령은 검증 규약에 있고, 이 로컬 로그만을 장기 재현 조건으로 요구하지 않는다.

소스는 clean HEAD가 아니다. 검증 중 Rust 변경을 동결하고 Git의 추적+비무시 미추적 파일에서
`.rs`, `Cargo.toml`, `Cargo.lock`을 중복 없이 경로 정렬했다(353파일).
각 파일의 `path + LF + raw byte length + LF + SHA256(raw bytes) + LF`를 순서대로 연결한 SHA256은
`67e7fa07aa27699dc65492934bcbdcfa07ec9f2b3bb64b668f812b517905b7b4`다.
이는 이번 Rust 소스 범위 식별자이며 문서/네이티브/전체 배포 이미지 식별자를 대신하지 않는다.
후속 소스가 이 값과 달라지면 이 결과를 새 소스 통과로 재사용하지 않는다.

## 후속 실행 소유권·정책 후보 slice — 2026-09-06

상태는 **B1/B2/B5 IN_PROGRESS**다. HEAD는 여전히 `a9e1967fc`, 아래는 미커밋 작업 트리의
후속 구현이다. 과거 절의 914/Rust-only 결과를 이 slice의 결과로 사용하지 않는다. 커밋·push·원격 배포는 하지 않았다.
계약 정의는 [배치 계약](../../../../../../../docs/adapter-batching-layers.md)의 실행 소유권 절,
레이어 권한은 [격리 계약](../../../../../../../docs/layer-isolation-contract.md), 다음 순서는 로드맵이 소유한다.

### 수정 전 실패와 실제 소비 경로

- 같은 load/session/request/slot의 두 번째 요청에 첫 번째 RELEASED가 오면 새 pending release가 지워졌고,
  중간/꼬리의 옛 RELEASE는 새 fake native KV를 삭제했다. 실제 head prefill→drive→tail→release와
  중간/꼬리 PHYSICAL→release 경로 3개에서 RED를 확인했다. 현재는 새 incarnation을 보존하며 정상 새 해제는 진행한다.
- 같은 operation의 body/kind 충돌·이전 operation은 native 이전 거부, 정확한 SETTLE/RELEASE는 receipt 재생이다.
  stage 시험은 유효 P4ID prefix와 실제 PHYSICAL warmup을 통과한 뒤 원래 short/count/role 오류를 주입한다.
  신규 guard에서 조기 거부된 것을 기존 native 응답 오류의 검출로 세지 않는다.
- 합계 receipt 예산 반례: 각개 검사는 가능했지만 한 명령의 합계는 불가능했다. 수정 전 RELEASE는
  첫 슬롯을 삭제한 뒤 거부, SETTLE은 두 native 호출/절단 뒤 commit 실패였다. 현재 두 방향 모두 첫 native
  호출 전에 거부하며 소유/flight/KV/출력 의도를 보존한다. 같은 operation을 단독으로 주면 정상 실행된다.
- old SETTLED의 incarnation/operation이 새 pending KV 장벽을 소비하지 않는 head 시험도 추가했다.
- 정책 후보는 `Scheduler::prepare_plan_with_physical_capacity`/`validate_prepared`/`commit_plan`으로 분리했다.
  실제 drive에서 발행 ID를 0으로 만들어 64번 거부해도 선택 revision/cohort/member·요청·owner·flight와
  native 호출 수가 그대로다. ID만 수리하면 원래 예정된 decode slot이 실행되고 그때만 다음 멤버로 회전한다.
- 제품 LOAD가 사용하는 `Worker::bind_loaded_identity`를 실제 lifecycle/request + fake ServerControl로
  검증했다. capability 없음/unknown·다른 echo·응답 유실·이전 generation 재사용은 슬롯 미공개/종료로 처리한다.
  **이 시험은 LOAD JSON parser와 실제 subprocess 시작까지 통과하지 않는다.** native Session 시험도 별도로 존재한다.

### 계층 경계에서 바뀐 것

P4 공용 protocol/agent에는 모델 규칙을 추가하지 않았다. 실행 소유/원장·wire는 concrete adapter에,
native guard/codec은 stage shell에 있다. guard와 공유 `utf8_text.hpp`는 llama/ggml 타입 없이 컴파일한다.
LB/PB와 제어 incarnation의 규약은 의도적 adapter wire 변경이며 upstream API의 직접 노출이 아니다.
state ABI·backend layout·ggml ordinal·common public signature·transitive include/link의 잔여를 해소한 것은 아니다.

canonical identity 부정 시험은 잘못된 session prefix, 빈 request, 추가 NUL, 손상 UTF-8을 거부하고
유효 한글/emoji를 수용한다. Rust의 유효하지 않은 key를 encoder가 먼저 거부하게 된 뒤에는 기존 worker
반례를 정상 bytes의 명시 변조로 바꿔 실제 decoder/consumer까지 유지했다. unknown request 반례는
그 다른 요청의 canonical key도 같이 바꿔 codec은 통과하고 원장 권위에서 거부되게 했다.

### 변이와 회계 한계

모두 사용자 checkout이 아닌 독립 복사본에서 실제 재컴파일했다. 소스 snapshot별 시험이므로
모든 변이를 아래 최종 전체 소스 하나에서 실행했다고 합쳐 주장하지 않는다.

| 변이 | 검출 / 보존 근거 |
| --- | --- |
| drive의 정책 commit을 issue 승인 앞으로 이동 | 실제 거부→재계획 시험 실패; 정확한 실패 원인/native 0 뒤 cursor가 달라짐. 복원 stage 13/13 |
| native control owner/body/watermark/canonical 검사 제거 | 최종 native 복사본 4종 모두 해당 assertion 실패. baseline 및 최종 CPU 별도 통과 |
| Rust ownership의 incarnation/watermark/개수·receipt 상한 제거 | 각각 소유/재사용/한계 시험 실패. 후보 payload는 Arc 공유 |
| control 합계 guard 제거, Replay 재과금, old receipt 차감 제거, 뒤 shrink 선차감, duplicate slot 또는 권위 검사 제거 | 6종 모두 실패(각 2/1/1/1/1/2개). 복원 ownership 16/16 |
| LOAD exact echo 검사 제거 또는 없는 identity revision 허용 | 각각 소비 시험 1개 실패; 복원 control 시험 6/6 |

로컬 RED/변이 보조 증거는 `target/incarnation-red-20260906-01/verification.txt`,
`target/aggregate-control-red-20260906-01/verification.txt`, `target/drive-policy-mutation-20260906-01/verification.txt`,
`target/bind-load-mutation-20260906-01/verification.txt`,
`target/native-identity-mutations-final/build/Testing/Temporary/LastTest.log`에 있다.
저장소 회귀 시험과 위 변이 정의가 재현의 기준이며 ignored target 디렉터리 존재를 새 세션의 필수 입력으로 삼지 않는다.

이는 메모리 내 제어 멱등/직렬 사전 예산 검사다. native가 실행 도중 실패하면 여러 변경을 원자 rollback하는
것이 아니며, 결과 불명은 fence한다. PHYSICAL execution 중복 계산·prefix 순서·전송 credit·지속적인
input servicing·cancel/drain·crash durable receipt는 미완이다. snapshot/restore와 bare legacy KV mutation은
이 실행 소유권과 통합되기 전 bound mode의 우회 실행으로 허용하지 않는다.

### 최종 실행 결과 — 숫자와 미실행을 분리

- `cargo test --workspace --no-fail-fast`: **956 passed / 0 failed / 7 ignored**, 57 summary, 최종 exit 0.
  staged adapter lib는 **263 passed**. 전체 수에는 과거 runtime 시험도 포함된다. 외부 fixture feature는 미실행이다.
- 하네스 `node --test`: **57 passed / 0 failed / 0 skipped**, exit 0.
- docs-lint 자체 시험 **12/12**, 공식 native builder wiring **4/4**. wiring은 production JS의 source/imported/
  no-llama target 선택을 실제 실행하되 외부 빌드 명령을 대체했다. 실제 imported relink 성공 증거가 아니다.
- 최종 native 소스로 **CPU 전체 source build 성공**. CTest 표면 집계는 13 executable exit 0이다.
  정확히는 **비모델 본문 실행 10개 + compile_test 부분 실행 1개 + 본문 전체 SKIP 2개**다.
  `request_options_test`는 첫 환경변수 검사에서 종료하여 sampling/grammar 단언도 전부 미실행,
  `mtp_ownership_test`도 첫 환경변수 검사에서 종료했다. compile_test의 real restore·batch rollback
  두 함수도 생략됐다. 따라서 SKIP 출력은 4줄이며 **실제 모델 native conformance는 미승인**이다.
  새로운 authority/codec/UTF-8 및 Session BindLoad/legacy 거부 시험은 생략된 분기에 속하지 않는다.
- private-header 패턴 gate 및 docs-lint는 통과했으나 의미 정확성/격리 전체의 인증이 아니다.
- clippy exit 0, warning 잔존. 이번 LOAD 시험의 type-complexity 경고도 있으므로 “새 경고 0”이라고 하지 않는다.
  전체 `cargo fmt --all -- --check`는 기존 비변경 entrypoint/adapter 등 formatting으로 실패했다.
  해당 파일은 임의 포맷하지 않았고 변경한 staged Rust 파일은 별도 정규화했다.
- GPU·실제 모델 load/native KV 효과·다중 컴퓨터 웨이브·성능 비회귀는 미실행이다.

로그: `target/b1-incarnation-final-workspace.log`, `target/b1-incarnation-staged.log`,
`target/b1-incarnation-harness.log`, `target/b1-incarnation-clippy.log`,
`target/native-identity-cpu/Testing/Temporary/LastTest.log`.
기존 CPU CTest가 모델 없는 early return을 Passed로 집계하는 배치는 B5/T03에서 unit/model-required를
분리해야 한다. 필수 모델 시험을 성공 숫자로 유지하는 방식으로 게이트를 고치지 않는다.

### 동결 소스 식별

이전 절과 같은 `path + LF + raw byte length + LF + SHA256 + LF` 경로 정렬 방식이다.
Rust 범위 357파일(`.rs`, Cargo.toml, Cargo.lock)의 aggregate:
`1782839851ebae20e1e69d0d1b1a5eccf175b57202850c67fb90a419b8b5b41a`.
native 범위는 staged/server 아래 `.cpp/.hpp/.h/.inc`와 CMakeLists.txt 79파일이며 aggregate:
`a4ea6508731bf6c7090d2df1c51b84530ad5d95e3b666b124664dde0f0f9cdf0`.
이는 해당 소스 범위의 식별이지 upstream 준비 트리·toolchain·전체 배포 이미지 digest가 아니다.
CPU 빌드는 `target/native-identity-cpu`의 새 source-build tree에서 수행했고 기존 imported DLL 재링크로 대체하지 않았다.
다음 개발에서 소스가 바뀌면 이 결과를 재사용하지 말고 새 digest/실행 결과를 기록한다.

## PHYSICAL 수신 receipt·opaque plan 수명 — 2026-09-07

HEAD `a9e1967fc`의 후속 **미커밋 작업 트리**다. 현재 소스에 대한 실행 결과이며 새 commit/push/deploy를
주장하지 않는다. 로드맵의 B1/B2/B5는 여전히 IN_PROGRESS다. PHYSICAL 정체성·상한·만료 계약은
[배치 계약](../../../../../../../docs/adapter-batching-layers.md)이 단독 소유한다.

### 실제 소비 경로와 수정 전 실패

- `Worker::physical`을 별도 모듈로 분리하고 `PhysicalReceiveLedger`의 prepare/begin/complete를 연결했다.
  fresh 입력만 native Frame으로 실행하고 cached 응답과 원래 이벤트 순서로 조립한다.
  전체 사전 검증 뒤 ID를 Running으로 만들며 native 응답 불명은 모든 fresh를 Uncertain/fence로 전환한다.
- 수정 전 실제 worker 시험 9개 중 정상 진행/native 사후 실패 3개는 PASS, 중복/충돌/혼합/낮은 ID의
  재전달 6개는 RED였다. fake는 native 호출마다 KV 기록과 tail sampler nonce를 바꾸므로 echo-only 대역이 아니다.
  원본 RED는 `target/physical-replay-red-20260907-01/verification.txt`와 그 raw output에 보존했다.
- 통합 후 actual SESSION→handle→PHYSICAL codec→native Frame→receipt/owner commit→mailbox 시험은
  **14/14 PASS**다. load 이후 상태는 fixture가 명시 설치하므로 LOAD JSON·subprocess 시작·Worker::run·
  실제 네트워크/llama KV/GPU를 통과했다고 하지 않는다.
- 독립 감사에서 load-global 실행 번호 가정의 결함을 추가 확인했다. 각 native head Session이 자기 번호를
  발급하는데 SESSION은 서로 다른 first를 허용한다. 설정된 first의 전체 Endpoint로 권위를 구분했다.
  다른 head의 같은 번호는 두 정상 실행, 같은 head의 다른 session/body는 conflict다.
- 실제 P4ID RELEASE로 fake live KV를 지운 뒤 같은 slot/incarnation 2를 다시 실행하고 old receipt를
  재전달해도 새 owner/KV가 보존된다. cache 만료·한도보다 큰 결과도 최초 정상 실행을 허용하며
  뒤 재전달을 새 native 작업으로 바꾸지 않는다. exact Replay에는 새 계산 span이 없다.

### 변이와 그 한계

사용자 checkout이 아닌 별도 복사본에서 보호를 제거하고 실제 재컴파일했다.
아래 변이 정의와 저장소의 시험 이름이 재현 근거이며 ignored target 경로의 영구 보존만을 요구하지 않는다.

| 변이 | 실제 검출 / 한계 |
| --- | --- |
| full issuer namespace를 첫 namespace로 합침 | `t24_distinct_head_full_endpoints...` 실패. 정상 시험은 agent/node/generation 세 축을 각각 바꾸나 이 변이는 첫 agent 차이에서 실패 |
| completed ID의 canonical body 비교 제거 | `t24_conflict_anywhere...` 실패. [fresh, 뒤 conflict]가 native를 호출해 상태를 바꾸는 것을 검출 |
| replay-only에도 old owner를 재수용 | `t24_cached_old_return...` 실패. 기존 owner guard가 takeover를 막더라도 정확한 replay를 부당 거부하는 회귀를 검출 |
| cached result를 새 계산 span으로 보고 | exact middle replay 시험 실패. early return만 지운 변이는 emit 내부 empty guard 때문에 동등하게 PASS했고 그 결과도 보존 |

원장 단위는 **18/18 PASS**다. canonical 입력 비교, naive max-seen 거부, prospective floor, 반환 owner,
oversized tombstone, 입력 byte 회계, invocation/owner membership, issuer 합치기, Seen/issuer/cache 합계
상한의 11종 변이도 해당 시험에서 실패했다. 단위 원장 시험과 위 actual worker 시험은 범위가 다르다.

단위 변이의 최초 회차는 tool 출력만 있었으므로 같은 최종 소스로 **새 보존 회차**를 재실행했다.
`target/physical-receive-mutations-20260907-01/`에 baseline/11변이/복원 raw 로그와 명령·exit·
source/binary hash를 각각 보존하고 `mutation-definitions.json`에 정확한 전후 코드·시험명을 남겼다.
13실행 모두 실제 Compiling을 확인했다. baseline/복원은 각각 18 GREEN, 변이는 각각 cargo exit 101의
시험 실패이며 컴파일 실패로 대체하지 않았다. 원본과 독립 복사본 소스는 같지만 두 정상 바이너리
hash는 달랐으므로 bit-reproducible build의 증거로 쓰지 않는다. 임시 증거 28파일을 작업공간 target에
복사하고 각 파일의 SHA256 일치를 확인했다. 원본 source는 이 과정에서 수정하지 않았다.

actual worker 변이 원자료·명령·exit·변이별 source/binary SHA256:
`target/physical-replay-mutations-20260907-01/verification.txt` 및 같은 디렉터리의 output 파일.
최종 원본/복사본의 core receipt 소스 SHA256은
`bfb777f715d300eb53a75b782658ec5cbc11b138cd1b5d0c2fce2f76b5f6b182`,
worker consumer는 `8a7e370ff817b525577339cef9f58e1cfee43c66ddd9e3b16cfd87d79046e374`,
actual worker test는 `ff3d39707909e15b3286cd2dc90ebed9416519682195c815044f1626d5c92797`다.

### opaque plan: 모델 없는 경로로도 소유권 이전 반례 유지

options E2E는 plan을 runtime으로 이동시킨 뒤 sampling 옵션을 읽고 있었다. 모델 없는 실행에서는
그 앞에서 SKIP하여 이 결함을 지나지 않았다. 이제 `consume_request_options_plan`이라는 **시험용**
공유 준비 함수가 두 sampling snapshot을 이동 전에 만든다. 실제 options E2E는 같은 함수로 실제
`runtime.load`를 호출하고, 새 `plan_lifetime_test`는 모델 없이 실제 opaque plan을 callback 안으로
소비하여 snapshot 내용·실패 시 보존·소비자 소멸 후 수명을 확인한다. production parser를 바꾸거나
27개 sampling/grammar 단언을 삭제하지 않았다.

별도 native 복사본에서 예전 순서로 되돌리면 SegFault, early-return이면 CTest의 필수 완료 문자열
부재로 실패했다. Release `assert(false)` 주입도 assertion 실패를 냈다. 이는 assertion 활성의 증거이며
assert를 제거하는 변이를 별도로 검출했다는 주장은 아니다.
근거는 `target/plan-lifetime-mutations/build/Testing/Temporary/LastTest.log`와
`target/plan-lifetime-mutations/full-native-test.log`다.

### 이번 실행 결과와 미실행

- `cargo test --workspace --no-fail-fast`: **988 passed / 0 failed / 7 ignored**, 57 summary, 최종 exit 0.
  staged adapter lib는 **295 passed**. 외부 fixture feature는 미실행이고, 전체 집계에는 과거 runtime 시험도 있다.
  로그: `target/b1-physical-workspace.log`. 중간 집계를 전체 결과로 쓰지 않았다.
- 하네스 `node --test test/benchmarks/p4-4node/*.test.mjs`: **57 passed / 0 failed / 0 skipped**.
- 공식 native builder wiring **5/5**. 빌드 프로세스를 대체하여 target 선택을 검사하므로 imported relink 실증이 아니다.
- docs-lint 자체 **12/12**, 추적 73파일/전체 79파일 clean, private-header 패턴 gate 80파일 통과.
  `git diff --check` 오류 없음. 원래 남은 common source 부채는 유지한다.
- 최종 native CPU source build 성공 후 CTest를 다시 실행했다. **14개 executable exit 0 = 비모델 본문 실행 11개
  + compile_test 부분 실행 1개 + 본문 전체 SKIP 2개**다. real restore·rollback·options E2E·MTP 네 경로의
  SKIP가 남아 있고 actual model conformance는 미승인이다. 새 lifetime 테스트는 SKIP가 아니다.
- `cargo clippy -p p4-llamacpp-staged-adapter --all-targets`: exit 0, 경고 잔존. 새 physical consumer의
  inspect_err 제안 및 테스트 type-complexity도 있으므로 경고 0/새 경고 없음으로 보고하지 않는다.
  로그: `target/b1-physical-clippy.log`. 전체 workspace fmt 통과는 주장하지 않는다.
- 문서 gate의 문자열/색인 통과는 기능/원자성/모델 성능의 증거가 아니다. GPU/실제 모델 load·
  VRAM-only 웨이브·RAM 오프로딩·다중 컴퓨터·성능 비회귀는 **미실행**이다.

동결 소스는 이전 기록과 같은 경로 정렬·raw byte length/SHA256 방식으로 집계했다.
Rust 범위 360파일 aggregate:
`0b79236846afb8c404746efd2882ebc3845352bce1137a111738b50172274a16`.
native 범위 81파일 aggregate:
`816c90838ca53b648fa67a3321dc8633ddbe445cf18a25c035cfce5443a3f08b`.
문서·모델·toolchain·배포 이미지 digest를 대신하지 않는다. A/B 수행 중인 소스를 수정한 결과도 아니다.

### H0 읽기 전용 확인 — 실행 환경과 모델 승인은 별개

사용자 지정 자원·확장 순서는 로드맵 §1에 반영했다. 여기에는 확인한 사실만 남긴다.

- 기존 신뢰된 SSH 키 경로로 조회한 M42-SERVER2는 RTX3090 24,576MiB 두 장과 host RAM
  274,561,966,080 bytes를 보고했다. 한 시점의 두 GPU 사용량은 각 350MiB/0%였으며 예약 가능량이나
  향후 독점 사용의 보장은 아니다. 두 GPU가 한 물리 호스트에 있다는 사실을 다중 호스트로 바꾸지 않는다.
- 현재 로컬 계정의 `S:\models`에는 GGUF 156파일이 있었다. 파일명/크기로 묶으면 가중치 후보 40,
  mmproj 22, embedding 1이다. split 후보 18개는 파일명상 조각이 모두 있었지만 GGUF header/내용/전체
  digest는 읽지 않았다. 156개 독립 모델이 검증됐거나 40개 모두 지원된다는 뜻이 아니다.
- 같은 SSH 비대화형 세션에는 S:가 보이지 않았다. 기존 `remote-agent.mjs`는 이 차이를 알고 interactive
  scheduled-task로 agent를 실행한다. 실제 agent 계정의 접근성은 아직 미확인이다. 경로 변경·모델 복사·
  매핑 생성·credential 변경·원격 task 시작/종료는 하지 않았다. 접속 문서의 암호/토큰은 저장소에 복사하지 않았다.
- 기존 `layer-window-memory-report.mjs`는 구형 apps 루트/누락 모듈 때문에 현재 독립 repo에서 실행 실패한다.
  기존 `spec.mjs`의 단일 ingress/GPU 강제 설정도 inventory/RAM 오프로딩/다중 host runner 완료가 아니다.

### 남은 안전성과 비용

보존된 ID의 replay를 막는 원장과 시퀀스 KV frontier는 다르다. 새 ID의 지난 위치/gap/phase,
새 Worker/native Session·재접속 후 freshness, 실제 Worker::run의 지속 입력/출력 포화·취소·drain,
edge credit/재시도 기간 결속은 남았다. cached 결과의 임의 만료는 무손실 복구를 보장하지 않는다.
begin/complete는 bounded Seen index 전체와 Arc 핸들을 복제하며, encoding/조립 임시 payload도 남아 있다.
이 cache 상한으로 전체 RSS 또는 hot-path 비용을 인증하지 않는다. 다음 순서는 로드맵 마지막 기록이 소유한다.

## 2026-09-07 후속 — stage KV frontier, 소스 동결 후 독립 변이

기준 HEAD는 여전히 `a9e1967fc59dffa6c2e458f1b91f916b1df826c1`이며 **미커밋 작업 트리**를
검증했다. 원본을 checkout/reset하지 않았고 commit/push·원격 프로세스 변경·배포·모델 load는 하지 않았다.
VRAM-only 뒤 RAM 오프로딩이라는 사용자 범위는 로드맵 §1과 검증 규약 H0가 소유한다.

### 실제 실패와 구현 범위

새 ID를 붙인 old position/gap/Prefill 회귀를 실제 `Worker::handle`로 middle/tail에 보냈다.
가짜 native는 호출할 때마다 KV를 쓰고 tail sampler nonce를 증가시킨다. 수정 전 정상 연속 진행 1개는
PASS, 사전 거부해야 할 8개는 FAIL이었다. 혼합 A 정상/B 오류를 양순서로 보내도 A가 먼저 실행됐다.
원문·당시 소스·바이너리 해시: `target/stage-frontier-red-20260907-01/verification.txt`.

`v2/node/frontier.rs::StageFrontiers`를 head 실제 발행, 중간/꼬리 PHYSICAL, SETTLE/RELEASE에
연결했다. exact receipt replay는 frontier를 움직이지 않는다. 위치·phase·생성량·options/reply·
Verify window·허가된 Replay·tail의 전체 proposal을 검사하며, candidate는 touched slot만 보유한다.
기존 `worker/outcome.rs::validate_outcome`의 본문은 순수 frontier 모듈 한 곳으로 옮겼다.
원래 검증 의미를 삭제하거나 sampler를 원장 안에서 호출하지 않았다.

정상 경로에는 partial→final Prefill→Decode, SETTLE 없는 Verify 전량 수용, direct partial SETTLE,
checkpoint 복원 후 같은 round의 정확한 Replay가 있다. 단순히 모든 Verify를 막아서 음성 시험을 통과시키지 않았다.
기존 stage control fixture가 Prefill만 한 뒤 임의 SETTLE하던 잘못된 전제도 실제 PHYSICAL Verify
warmup으로 바꿨다. 응답 손상/receipt 상한/늦은 연산 거부의 원래 목적과 native 호출·fence 단언은 유지했다.

새 시험 작성 중 fake가 여러 행 텐서를 4바이트 하나로 치환해 `InvalidTensor`를 낸 3건은 fixture 오류였다.
텐서 길이는 유지하면서 각 word에 nonce를 기록하도록 고쳤다. 새 순수 시험은 정상 membership을 가진
같은 시퀀스의 결과 청크를 역순으로 승인하는 구현 결함도 찾았다. per-slot 반환 순서 대조로 수정했다.

### 독립 변이 — 실제 재컴파일과 복원

| 제거한 검사 | 관측된 실패 |
| --- | --- |
| 순수 old/gap 위치 검사 | middle에서 허가되지 않은 행이 native KV에 추가됨 |
| 실제 `Worker::physical`의 frontier 연결 | tail에서 KV와 sampler가 다시 실행됨. 순수 함수 존재만으로는 방어가 아님 |
| exact Replay token 결속 | 잘못된 token 999가 native KV에 들어감 |
| per-slot 결과 청크 순서 | `[2..4),[0..2)` 결과가 Ready로 승인됨 |
| delta slot revision | 오래된 RELEASE 후보가 승인됨 |
| tail 정산 retain 대조 | tail이 확정한 retain 3 대신 4를 승인함 |

앞 3개는 `target/stage-frontier-mutations-20260907-01/verification.md`, 뒤 3개는
`target/stage-frontier-pure-mutations-20260907-01/verification.txt`에 raw 출력·명령·소스/바이너리
SHA256과 정확한 변이 정의가 있다. 모두 컴파일 성공 후 시험 assertion으로 실패했다(cargo exit 101).
실제 소비자 변이는 복원 후 28/28, 순수 변이는 복원 후 10/10 재통과했다. 각 변이가 for-role
루프의 첫 실패에서 멈춘 경우 뒤 role까지 그 변이로 실행했다고 확대하지 않았다.
consumer 복사본 141파일 복원 해시가 일치했고, 순수 증거 11파일은 temp에서 위 저장소 내부 위치로
복사한 뒤 파일별 해시가 전부 일치했다. 이 target 원자료는 Git 추적 배포물이라는 뜻은 아니다.

### 동결 결과와 정확한 범위

- `cargo test --workspace --no-fail-fast`: **1012 passed / 0 failed / 7 ignored**, 57 summary, 최종 exit 0.
  staged adapter lib는 **319 passed**. 로그: `target/stage-frontier-workspace.log`.
  외부 fixture feature 제외와 과거 runtime 시험도 구분한다. 이 집계가 전부 event full-loop 시험은 아니다.
- 순수 frontier **10/10**, actual PHYSICAL 소비자 **28/28**, 기존 stage 소비자 **15/15**.
- 하네스 **57/57** + native builder wiring **5/5**, 합계 **62/62**, skipped 0.
  로그: `target/stage-frontier-js.log`. native C++ 본문을 실행한 수가 아니다.
- clippy exit 0, **경고 잔존**. 신규 `map_err`/시험 배치 등의 제안도 있다.
  로그: `target/stage-frontier-clippy.log`. warnings 0 또는 새 경고 없음으로 보고하지 않는다.
- C++/CUDA/실제 모델·강한 실기 웨이브·RAM 오프로딩·다중 컴퓨터·TPS 비회귀는 **이번 회차 미실행**.
  이전 native 모델 SKIP를 이 Rust GREEN으로 채우지 않는다.

동결 Rust 361파일: Git tracked+untracked(exclude-standard) 중 `.rs`, Cargo.toml/Cargo.lock,
경로 ordinal 정렬 후 `path LF byte-length LF sha256 LF`를 합산한 SHA256은
`85d453ebbfc381225a66555861386ebfcbdcc00afc2eb2d454ea1caff72950a0`이다.
핵심 파일 SHA256:

| 파일(어댑터 src/v2/node/ 아래) | SHA256 |
| --- | --- |
| frontier.rs | `9a12ea01693a3461e4ecd6383b0ac25a19886983207d8225a7be6bdf8a6ed196` |
| worker/physical.rs | `970ad0739377e1d6ebb85f1a7d6815ae81f741dfa824d94030912e709842ec9f` |
| worker/drive.rs | `4d2769be9b39bc9ab9dee1f14534cd171ee381ed1163d10fa15936fdff5d6572` |
| worker/settlement.rs | `1e8689f497cdf68d8e3bd4c3421b5e88891ce2cfb75330ef4a2848a935ffabb5` |
| worker/release.rs | `c207e15e6025fa4d5c08f339bbade6a0c22866436edadec2c74765ef20b10578` |
| worker/physical_replay_tests.rs | `aadd2253a7839f71ffc00d2a12c23374542390006661d844cdfd32dcee98ee7f` |

### 추가 발견: GREEN 밖의 P1 두 경로

독립 감사에서 `physical_capacity=2`인데 native가 정상 형식 proposal `[23,29,31]`을 반환하면
토큰 예산 검사는 통과하고 **PHYSICAL은 TAIL_BATCH, SETTLE은 SETTLED를 발행**하는 것을 재현했다.
두 경로 모두 `handle=Ok`, fence=false다. 정상 opcode와 token budget만으로는 atomic 폭을 보증하지 않는다.

`target/proposal-cap-red-20260907-01`의 baseline 15/15와 새 거부 반례 **2/2 FAIL**을 따로 보존한다.
138파일 대조에서 변경은 독립 시험 파일 하나뿐이며 생산 137파일은 동결 원본과 같다.
`red-output.txt`, `verification.json`이 근거다. 이 반례는 원본 suite 1012개에 포함되지 않았으며,
원본 GREEN을 제품 정합성 전체 완료로 보고하지 않는다. 다음 첫 행동은 로드맵 최신 기록을 따른다.

이번 frontier는 **Rust에서 native 진입 전 검사**다. C++ 자체의 직접 호출 위치 방어, Worker::run
전체 스케줄·지속 queue 포화·종료, 재시작 신선성·credit/재시도, 모델 conformance와 실기 증명은 여전히 남았다.

## 2026-09-07 후속 — continuation 폭·actual loop·duplex Full

HEAD는 `a9e1967fc`로 같고 후속 작업 트리 변경을 검증했다. 원본 checkout 변이/복원·commit/push·
원격 배포·GPU 모델 load는 하지 않았다. C++/실제 모델·RAM 오프로딩·다중 호스트 성과가 아니다.

### T24 continuation 폭: native 뒤 실패와 head 사전 거부

지난 독립 RED를 실제 stage 소비 시험으로 이관했다. `physical_capacity=2`인데 정상 codec과
남은 token budget을 만족하는 `[23,29,31]`을 돌려준다. PHYSICAL/SETTLE 및 정상 앞부분+잘못된
뒤 결과의 Fresh 묶음은 수정 전 모두 `handle=Ok`, fence=false로 성공 TAIL/SETTLED를 발행했다.
원본 RED는 **17 passed / 3 failed**, 동일 시험의 수정 후 결과는 **20 passed**다.
`target/proposal-cap-source-red-20260907/red-output.log`와 소스 사본, 수정 후
`target/proposal-cap-source-green-20260907.log`를 보존했다.

- shared `frontier::validate_continuation_width`를 PHYSICAL 응답·SETTLE 응답·head tail 승인에 연결했다.
  token budget/phase 검사를 대체하지 않고 physical 폭을 추가 대조한다. proposal을 절단하지 않는다.
- native가 이미 실행한 뒤의 잘못된 응답은 effects fence다. PHYSICAL Fresh 묶음 전체는 Uncertain,
  성공 receipt/cache와 owner/frontier commit은 없다. SETTLE도 성공 receipt/SETTLED를 만들지 않는다.
  동일 재전달과 별도 정상 새 요청을 보내도 native 추가 호출 0이다. native KV rollback 주장은 하지 않는다.
- 폭 1과 정확한 cap=2는 실제 head 반환→다음 drive→tail에서 Decode/Verify로 계속 진행한다.
- head의 별도 회귀는 정상 A/폭 초과 B를 함께 반환하여 전 요청·flight·출력 의도가 그대로인지 본다.
  B를 cap 안으로 돌린 동일 issued identities는 각각 한 번 출력한다. 이미 있던 SETTLED의 cap 검사는 유지했다.

### T20/T21/T23 일부: 실제 Worker::run 4개 시험

`worker/loop_tests.rs`는 실제 OS worker thread, bounded input/completion mailbox, Event/Frame/
LogicalBatch/CapsuleSet codec을 지난다. native fake는 독립 KV token/position/incarnation 배열을
관리하며 production scheduler/request/flight/frontier/ownership 전이를 호출하지 않는다.
LOAD 이후 상태는 fixture가 명시 구성하고, 라우팅은 별도 bounded pump다.

1. 2-stage chunked prompt와 max_tokens 5의 정확한 토큰·위치·종료, 전 stage release 한 번, head ack.
2. 2/4/8-stage에서 첫 웨이브 6요청이 진행 중일 때 둘째 웨이브 2요청을 추가한다.
   새 짧은 요청의 첫 output이 오래된 긴 요청의 마지막 output보다 먼저 오고, 모든 요청의 정확한
   native KV 이력/토큰/position/stop·전 stage release를 확인한다.
3. completion capacity 1에서 실제 `completion_queue_full:waiting`과 계산 1회가 먼저 발생한다.
   drain을 재개하면 결과를 잃지 않고 완주한다. 단지 Full 자료구조를 직접 시험한 것이 아니다.
4. max-open=1에서 논리 batch의 physical 두 조각 중 하나만 돌려주면 다음 issue는 안 열린다.
   max-open=2는 TAIL 전에 두 번 issue하는 양성 대조이며 서로 다른 요청의 TAIL 역순도 처리한다.

**범위 제한:** N=1, 지속 유입 기아, speculative SETTLE/Replay, 취소·graceful drain은 미포함이다.
native subprocess/model/GPU와 실제 EventNode/broker/network도 이 fixture에 없다. 강제 teardown의
join escape를 정상 drain으로 세지 않는다. fixture 큐 상한을 제품 pending/RSS/credit 상한으로 확대하지 않는다.
가짜 token 문자열의 정확성을 정상 언어 모델 응답 품질로 보고하지 않는다.

### T22/T23 일부: 중립 EventNode의 양방향 진행

기존 `dispatch_or_wait`는 출력 Full이면 자기 입력을 읽지 못했다. 실제 broker와 capacity 1의
두 EventNode를 사용해 각각 상대에게 보낼 completion을 보유하게 만든 뒤 정상 입력을 채웠다.
수정 전 두 번 모두 **a=0, b=0**, 1초 timeout이었다. 첫 poll의 유일한 ready 경로를 completion으로
만들어 select 분기 운에 의존하지 않는다. 모든 이벤트는 정상 broker 검증을 통과한다.
원문과 old source/exe 해시는 `target/event-node-duplex-red-20260907-01/verification.md`에 있다.

중립 pump는 input/output을 각각 한 개 보존한다. 한쪽 Full이어도 반대쪽의 처리 기회를 유지하며,
이미 가진 방향의 queue에서는 더 꺼내지 않는다. opaque Event만 다루고 어댑터/모델 지식을 추가하지 않았다.
추가 상한 시험은 두 held + 다음 각 capacity-1 큐가 찬 상태에서 completion take=1, 셋째 양방향
offer=Full과 전체 bytes 보존을 검사한 뒤, 공간 복구 시 처음 두 개씩 정확한 Event/순서로 도착함을 본다.
목적지 Closed는 오류이며 broker가 전달 실패를 Duplicate로 기록하지 않는다.

EventNode **7/7**, core neutrality **3/3** 통과. 1ms timer는 여전히 남고 capacity waker가 아니다.
모든 adapter/queue가 Full인 일반 순환망의 교착 해소·control 예약·bounded RSS·종료는 이 수정의 보장이 아니다.

### 독립 실제 소비 변이 9종

| 변이 | 실패한 실제 소비 시험 | 복원 |
| --- | --- | --- |
| continuation helper 비활성 | stage 3개 | stage 20/20 |
| PHYSICAL의 cap 호출 제거 | stage 2개 | stage 20/20 |
| SETTLE의 cap 호출 제거 | stage 1개 | stage 20/20 |
| head의 cap 호출 제거 | head 1개 | head 1/1 |
| max-open gate 제거 | actual run-loop 1개: native issue 2 ≠ 1 | loop 4/4 |
| worker Full의 결과 보관 대신 폐기 | actual run-loop 1개: waiting/정상 복구 미성립 | loop 4/4 |
| outbound Full 독점 await 복원 | actual broker ring 1개: a=0,b=0 | EventNode 7/7 |
| held input guard 제거 | 둘째 input이 꺼내져 셋째가 잘못 수용됨 | EventNode 7/7 |
| held output guard 제거 | completion take 2 ≠ 1 | EventNode 7/7 |

각 arm에서 실제 재컴파일 후 assertion 실패(cargo exit 101)를 확인했고 최종 복원본이 통과했다.
원자료·정확한 변이·source/binary 해시·복원 대조는 다음에 보존한다.

- `target/proposal-cap-mutations-20260907/verification.json`: cap 4종, source 139파일 arm별 대조.
- `target/worker-loop-mutations-20260907-01/verification.txt`: loop 2종, final 03~06 실행의 4핵심파일 해시.
- `target/event-node-duplex-mutations-20260907-01/verification.md`: duplex 3종, snapshot 150파일 대조.

증거로 제외한 실패도 남겼다. head 시험 최초 복사 때 mtime 보존 때문에 Cargo가 옛 executable을 써
**0 tests**를 실행한 것은 무효다. 복사본 mtime 갱신→실제 compile→1개 실행을 확인하고 변이했다.
loop의 첫 gate 변이는 assertion unwind 중 fixture Drop이 다시 panic하여 비정상 abort했다.
Drop이 원래 실패를 가리지 않도록 고친 뒤 같은 변이로 정상 FAILED를 재현했고, 옛 abort를 최종 검출 수에 넣지 않았다.
target 원자료는 로컬 보존이며 Git 추적/배포되었다는 의미가 아니다.

### 최종 동결 집계

- `cargo test --workspace --no-fail-fast`: **1026 passed / 0 failed / 7 ignored**, **57 summary**, 최종 exit 0.
  staged adapter lib **330**, agent core lib **149**. 로그 `target/proposal-duplex-loop-workspace.log`.
  외부 fixture feature 제외·과거 runtime 시험을 포함한 전체 집계이며 모두 event loop 시험은 아니다.
- JS 하네스 57 + native builder wiring 5 = **62/62**, skipped 0. `target/proposal-duplex-loop-js.log`.
- clippy(staged adapter + agent core, all-targets) exit 0, 경고 잔존. 새 경고가 없다고 주장하지 않는다.
  `target/proposal-duplex-loop-clippy.log`.
- 문서 갱신 후 docs-lint tracked **73 clean** / `--all` **79 clean**, 자체 시험 **12/12**,
  `cargo test -p p4-agent --test docs_lint` **1/1** 재통과, 기본 `git diff --check` exit 0.
- C++/CUDA/native 모델·실기 웨이브·RAM 오프로딩·다중 컴퓨터·성능 비회귀는 **이번 회차 미실행**.

Rust 362파일을 이전과 같은 ordinal `path LF byte-length LF sha256 LF` 규칙으로 동결했다.
합산 SHA256: `c532a51016ba39ea97251602ecefbdbb2e9397696a75db99ce8161cdb7949f84`.
목록: `target/proposal-duplex-loop-rust-source.log`. 문서만의 후속 갱신은 이 Rust seal에 포함되지 않는다.
핵심 새 loop 시험 SHA256 `e297fe44a2ebce6ced2996b30440b95f1e89fef514614859c69e5961c4a196d2`,
EventNode SHA256 `d3711058d3a4b2ca63c2c001fac097f4b700e8b29a6ad5937842cdefce24cf3b`,
EventNode tests SHA256 `9dbce3f6fd70fcc4fd752f21b1864bfb6929006fad0cfb7b8016da8fa10d284c`.

상태·다음 행동은 로드맵 최신 절이 소유한다. 이번 GREEN을 지속 입력·전체 credit·정상 shutdown·
native 직접 호출 방어 또는 VRAM-only/RAM 오프로딩 실기 완료로 고정하지 않는다.

### 동결 GREEN 밖: 지속 입력 drain의 결정론 반례

독립 copy에서 기존 4개 loop 시험에 관측 probe 하나만 추가했다. runnable token 요청을 먼저 둔 뒤
Tokenize fake가 다음 유효 PREFILL을 worker input에 하나씩 넣어 queue nonempty를 인과적으로 유지한다.
입력 연쇄 0/16/256에 대해 첫 Logical 전에 실행한 Tokenize 수는 **0/16/256**이었다.
무한 연쇄로 늘리면 `try_recv` drain이 끝나지 않아 drive에 도달하지 못한다. 유한 연쇄를 끊은 뒤에는
정상 토큰·전 stage KV·release·head ack까지 완주했다. admission이 원인이라는 추정이나 TPS 실험이 아니라
실제 루프의 처리 순서 반례다. 고정 quantum 수치를 임의로 PASS 조건에 넣지 않았다.

`target/worker-ingress-drain-probe-20260907-01/verification.txt`에 raw/metadata/fixture-only diff,
probe 소스와 unchanged worker 소스를 보존한다. 독립 실행은 기존 4+관측 1의 5개였지만 **1026에
추가하지 않으며** 관측 probe의 종료 성공을 정상성 게이트 PASS로 세지 않는다. 원본 production loop는
이번에 바꾸지 않았고, 이어서 지켜야 할 처리 기회 계약은 검증 규약 T20, 구현 순서는 로드맵이 소유한다.

## 2026-09-07 후속 — 유한 actor 기회와 실패를 보존하는 종료

앞 절 이후 같은 HEAD `a9e1967fc`의 작업 트리를 수정했다. commit/push·배포·모델 load는 하지 않았다.
원본 소스 변이는 없으며, CPU-only Rust 검증이다. 물리 GPU·정상 언어 모델 응답·RAM 오프로딩의 성과가 아니다.

### 지속 입력과 발행의 양방향 진행

`Worker::run`의 무상한 입력 drain을 한 turn 최대 32개로 제한하고, 기존 발행 루프를 실제
`drive_one_batch` 한 번으로 나눴다. 성공하면 새 입력 없이 다음 turn도 재진행하고, 외부 해제가
필요한 gate/no-work이면 입력을 기다린다. 기존 `drive_first_batches`는 메서드 시험 전용 wrapper이며
운영 run-loop가 호출하지 않는다. 32는 고정 actor 기회 상한이지 성능 최적값이나 새 실험 옵션이 아니다.

지속 입력 시험은 실제 Event/Frame codec·bounded queue·Worker::run과 독립 native KV 모델을 사용한다.
Tokenize가 다음 정상 PREFILL을 재공급하는 유한 연쇄를 0/16/256으로 만들고, 정상 완주·출력·전 stage
KV/release를 검사한 뒤 **첫 Logical 전의 Tokenize 개수**를 독립 literal 32와 대조한다.

| 연쇄 | 수정 전 | 수정 후 |
| --- | --- | --- |
| 0 | 0 | 0 |
| 16 | 16 | 16 |
| 256 | 256 | 30 |

숫자는 Tokenize 호출 수다. SESSION과 token 입력 등 다른 handle도 같은 32-event 예산에 포함되므로
30이 나온다. 이 지표를 전체 head native 작업 시간·TTFT 또는 전체 입력 처리 건수로 바꿔 부르지 않는다.
RED는 `target/worker-ingress-quantum-regression-20260907-01/00-before-fix.stdout.log`에 보존했다.
최종 ordinary loop **5/5**는 기존 depth=2의 **입력 없는 추가 issue** 양성 대조도 유지한다.

### actual run 종료·제어 시험 8개

`worker/turn_tests.rs`는 head 한 개의 실제 run/codec 경로다. native Frame만 fake이며 tail은 없다.
정상 SESSION 재전달은 이벤트/ACK 인과 ID와 payload를 확인한다. 요청마다 독립 token/position도 확인한다.

1. 첫 Logical 실행 안에서 SESSION을 넣으면 둘째 Logical 시작 전에 해당 ACK가 정확히 한 개 있다.
2. 첫 native 실행 안에서 중지하면 native는 한 번뿐이며 requests 2/flight 1을 보존한다.
3. input EOF를 관측하면 native 0, requests 2/flight 0으로 abandoned를 기록한다.
4. idle EOF는 local work empty지만 native cleanup 실행 **중**에는 closing이다. cleanup 성공 뒤에만 closed다.
5. active work의 unload 실패는 최상위 cleanup failure이고 원래 abandoned와 requests 2/flight 2를 보존한다.
6. native 응답 유실+unload 실패는 원래 오류·prepared Uncertain·effects fence와 정리 오류 둘 다 남긴다.
7. idle이라도 unload 실패면 최상위 failed다. 정상 closed prefix에 실패 문자열만 덧붙이는 것으로 통과하지 않는다.
8. stale SESSION 하나를 거부한 뒤 idle EOF이면 요청 거부 사유를 previous에 남긴다. 비치명적 이벤트 거부를
   태스크 fatal로 바꾸거나 로컬 잔량을 만들어 시험하지 않는다.

첫 3개는 무수정 run에서 **0 passed / 3 failed**, 수정 뒤 통과했다.
`target/worker-turn-red-20260907-01/verification.md`에 실제 재컴파일/원문/소스·exe 해시가 있다.
추가 감사에서 새 종료 구현도 idle unload 실패에 closed prefix를 남기는 결함을 발견했다.
`target/worker-cleanup-red-20260907-01`의 **5 passed / 2 failed**를 봉인한 뒤 고쳤다.
최종 8개는 기존 단언과 pre-cleanup 관측을 모두 유지한다.

`finish_run`은 cleanup 전에 요청·pending·release/settle·prepared issue·flight·효과·owner/frontier·
수신 Running/Uncertain/fence를 각각 보존한다. `active_counts/active_slots/shutdown_status` 조회
회귀 **4/4**는 실제 원장 전이를 사용한다. completed receipt/Released tombstone은 미완 작업이 아니며,
Stopped KV는 release 전까지 미완이고 active_attempt=None인 Uncertain도 미완이다.
조회 함수 통과와 실제 종료 consumer 통과를 같은 시험이라고 합치지 않았다.

**보장하지 않는 것:** global graceful drain, 각 요청의 취소 terminal 전달, network/mailbox 잔량 0,
일반 포화 순환망의 control 진행, 이미 실행 중인 동기 native의 강제 중단. 입력 PHYSICAL/SETTLE/
RELEASE 한 이벤트 안에는 여러 native 작업과 publisher Full 대기가 있을 수 있다. turn 상한은
head의 자발적 Logical 발행 기회이며, 모든 native 호출의 시간/횟수 상한이 아니다.
중지 전에 시작한 native 결과를 승인/보존하는 것은 새 실행이 아니며, 그 결과를 버리고 rollback이라고 하지 않는다.

### 독립 변이와 복원

| 독립 copy 변이 | 검출된 실패 |
| --- | --- |
| ingress 상한을 MAX로 | 연쇄 256에서 literal 32 상한 실패 |
| 성공 issue 뒤에도 무조건 recv | terminal 전 depth=2 두 번째 발행 불가 |
| 입력 재확인 전 여러 논리 issue drain | 둘째 native가 SESSION ACK를 추월 |
| 중지/EOF/native 진입 guard 제거 묶음 | 중지 native 2≠1, EOF native 2≠0 |
| unload 오류 무시 | active 잔량/원래 native 실패와 cleanup 오류 보존 시험 2개 실패 |
| cleanup 전 최종 closed 발표 | native.shutdown 내부 관측이 closing 아님 |
| previous 삭제 | nonfatal stale SESSION 거부 사유 유실 |
| cleanup 실패의 최상위 실패 승격 제거 | idle unload 실패인데 closed prefix |
| flight/owner에 완료 history 포함 | 각 원장의 active 0 판정 실패 |
| Stopped frontier 제외 | release 전 KV 잔량 누락 |
| 수신 Uncertain 무시 | active attempt가 없어도 미완인 상태 누락 |

표의 flight/owner는 별도 변이 두 개다. 총 **12개 변이 arm**이며, 중지/EOF 묶음은 한 arm이 두 시험을
깨뜨린 것이지 각 guard를 따로 변이했다고 주장하지 않는다. 경로별 상세와 복원 대조는 다음에 있다.

- `target/worker-actor-quantum-mutations-20260907-01/verification.txt`: actor 2종, 실제 compile·133파일 대조,
  기준/복원 loop 5/5. 이 복사본 shutdown은 후속 cleanup 수정 전이며 그 수정의 검증으로 세지 않는다.
- `target/worker-turn-mutations-20260907-02/`: actor/종료 6종, arm별 실제 코드·exe·출력과 복원본.
- `target/shutdown-count-mutations-20260907/verification.json`: 조회 4종, 각 3 passed/1 failed, 복원 4/4.
  주변 actor가 동결된 별도 복사본이므로 현재 전체 workspace 시험으로 합산하지 않는다.

turn copy는 baseline/복원 **8/8**, 143파일 내용 일치다. 첫 복원은 오래된 mtime 때문에 Cargo가
0.04초에 M6 executable을 재사용해 6 passed/2 failed를 냈다. 이 실행은 승인에서 제외하고 원문을
보존했다. copy에서 내용은 원복하되 mtime을 갱신한 뒤 **실제 3.43초 재컴파일·8/8**을 확인했다.
원본에서는 네 원장 파일을 최종 format했으며, 변이 결과는 각각 봉인된 copy에, 전체 집계는 아래
최종 작업 트리에 귀속한다. 소스 일치만으로 실행 파일까지 갱신됐다고 추정하지 않는다.

### 최종 작업 트리 집계와 재개 기준

- `cargo test --workspace --no-fail-fast`: **1039 passed / 0 failed / 7 ignored**, **57 summary**, 최종 exit 0.
  staged adapter lib **343**, agent core lib **149**. `target/worker-actor-workspace.log`.
  이전 1026 대비 13개 증가: sustained ingress 1 + actual actor/exit 8 + 원장 조회 4.
  feature 제외와 과거 runtime의 시험을 구분하며 전체 1039를 current event E2E로 부르지 않는다.
- JS **62/62**, skipped 0: 하네스 57 + native builder wiring 5. `target/worker-actor-js.log`.
- staged adapter/agent core `clippy --all-targets` exit 0, 경고 잔존. `target/worker-actor-clippy.log`.
  staged lib-test는 23 warnings(13 duplicates)로 기록됐으며 무경고/새 경고 0이라고 하지 않는다.
- Rust 소스/Cargo **364파일** 합산 SHA256
  `2dbdbfe4a41b2ccc190b9188c257b08b77130cd95fc33995ae2cdb7da87e66d4`.
  `target/worker-actor-rust-source.json`과 재검증 도구에 목록·길이·원문 해시를 보존했다.
  정렬/canonical 규칙은 이전 seal과 같으며 문서 후속 수정은 포함하지 않는다.
- 문서 후속 갱신 뒤 tracked **73 clean** / `--all` **79 clean**, 자체 시험 **12/12**,
  `cargo test -p p4-agent --test docs_lint` **1/1**, 기본 `git diff --check` exit 0.
- C++/CUDA build, 실제 모델/GPU·VRAM-only/RAM 오프로딩·다중 컴퓨터·성능 비회귀는 **이번 회차 미실행**.

현재 상태·다음 첫 행동의 단독 소유자는 로드맵의 최신 진행 절이다. 이 집계를 B2 전체 완료나
최종 강한 웨이브 성과로 승인하지 않는다. target 원자료는 로컬 보존이며 commit/push된 증거가 아니다.

## 2026-09-07 후속 — speculative actual run과 native logits 소비 경계

HEAD `a9e1967fc` + 미커밋 작업 트리다. 직전 Rust 봉인 364파일을 시작 시 다시 대조한 뒤
actual run fixture를 확장했다. 기존 ordinary 5개와 위치당 1회 append oracle는 유지한다.
`worker/loop_tests/speculative.rs`의 native fake는 production 상태 전이를 호출하지 않고 literal
입력·응답·KV 이력을 소유한다. Worker::run·codec·발행·반환·제어·출력은 실제 생산 경로다.
OS 스레드/독립 라우팅 pump를 쓰며 LOAD/subprocess·EventNode/broker/network·llama 모델은 지나지 않는다.

### 정상 경로와 독립 oracle

새 시험 3개는 각각 **2·4스테이지**에서 실행한다. ordinary 5개와 합쳐 actual run **8/8**이다.

| 시나리오 | 반드시 관찰한 결과 |
| --- | --- |
| Full accept | SETTLE 0, 정확한 5 token/position/length; RELEASED 보류 중 다음 요청 미발행, 해제 뒤 같은 slot·다른 incarnation 정상 진행 |
| Direct partial | tentative KV `[10,11,12,1000,9001]`를 위치 4에서 trim; 올바른 token으로 위치 4부터 재append; 4 token/position/length |
| Checkpoint Replay | 같은 tentative KV를 **위치 3**으로 복구; retain 5는 앞으로 채울 끝; 미확정 Verify 출력 0, output=false Replay의 확인된 결과는 정상 출력; 4 token/position/length |

Direct/Checkpoint는 마지막 SETTLE 홉과 꼬리→head의 단일 SETTLED를 따로 보류한다.
별도 요청을 tokenize까지 완료해 runnable로 만들어도 global Verify fence가 logical 발행을
2회에서 유지하는지 본다. 대상 요청의 ready가 없어서 자연히 멈추는 시험이 아니다.
각 stage의 append→trim/restore→reappend→release 전체 이력과 동일 control operation ID를 대조한다.
native fake의 sampler_calls는 scripted decision 횟수이지 실제 sampler primitive 횟수 증명이 아니다.

Full의 마지막 Verify fence도 RELEASED까지 유지된다. 따라서 release 전 재사용이 없다는 end-to-end
관측은 있으나 free-slot 반환 기전만의 독립 mutation 증명이라고 하지 않는다.

### 생산 소비 변이 3종

`target/b2-speculative-run-mutations-20260907/verification.json`의 독립 3-crate/132파일 copy에서
시험과 fake를 고정한 채 생산 소비 코드만 변경했다. 각 arm 실제 compile·exe/source 해시를 보존한다.

| 변이 | actual run 결과 |
| --- | --- |
| drive의 Verify fence 검사 제거 | 6 passed / 2 failed; 별도 ready 요청으로 native logical 3≠2 |
| Replay의 실제 output intent 누락 | 7 passed / 1 failed; 최종 출력 수 부족 |
| 전량 수용 후 Verify fence 해제 누락 | 7 passed / 1 failed; 후속 발행 정지 |
| 원상 복원 후 재컴파일 | 8 passed / 0 failed; 132파일 baseline 내용 일치 |

첫 변이는 기본 assertion 뒤 fake Mutex poison에 따른 teardown 오류도 냈다. 최초 3≠2 실패와
전체 libtest summary를 모두 보존했으며 추가 teardown 오류를 별도 결함 검출 수로 세지 않는다.
원본 두 시험 파일의 SHA256:

- `loop_tests.rs`: `c6e2428dd06a9a2d6d359c475dade2fd520969274715bc29e79adf835613831d`
- `loop_tests/speculative.rs`: `5466a0265158dce0606147039204f80f57e5515d2cf8b00033b8edbf0238de3d`

### Rust 전체와 독립 native 감사의 구분

`cargo test --workspace --no-fail-fast`는 최종 exit 0, **1042 passed / 0 failed / 7 ignored**,
57 summary다. staged adapter lib는 **346**이다. `target/worker-speculative-workspace.log`에 원문을
보존했다. 이전 1039에서 위 actual run 3개만 증가했으며 전체 수를 event E2E 개수로 부르지 않는다.
Rust/Cargo **365파일**의 SHA256은
`9f1dbc577f78336bab97d088a035398c07b4799795c371dfe356913cfb93b841`이다.
`target/worker-speculative-rust-source.json`과 `worker-speculative-seal.mjs --verify`로 전체 시험
전후 내용 일치를 확인했다. clippy --all-targets는 exit 0이나 staged lib-test **24 warnings
(13 duplicates)**가 남아 있다. 새 fixture의 clone 스타일 경고 1개도 포함하며 무경고라 하지 않는다.

코드 독립 감사에서 fake 성공과 다른 native 결함을 찾았다. 실제 `execute_physical`이 Replay
wire output=false를 llama_batch.logits에 그대로 전달했지만 `sample_physical_mtp`는 Replay의
모든 행에서 logits를 읽었다. pin `0eadefebd3`의 기본 embeddings=false 경로는 해당 logits를
만들지 않는다. 같은 문제를 FIRST 배치 생성에서도 검사해야 한다. fake는 native sampler를
호출하지 않으므로 1042 GREEN이 이 오류를 반증하지 않는다. 아래 native 소비 시험은 별도 증거다.

### 별도 다음 반례 — 아직 고치지 않은 busy UNLOAD

`target/b2-unsafe-unload-red-20260907/verification.json`은 위 원본 1042 집계 밖의 독립 copy다.
기존 actual run ordinary fixture에 tail capsule-set 하나를 보류하고 정상 generation의 UNLOAD를
보냈다. 생산 코드는 변경하지 않았으며 새 반례가 있는 시험 파일만 바뀌었다.

```text
UNSAFE_UNLOAD native_shutdowns=1 rejected=0 approved=1 held_tail=1 outputs=0 snapshot=unloaded
test result: FAILED. 0 passed; 1 failed
```

실제 25.19초 재컴파일과 132파일/실행파일 해시를 보존했다. busy일 때 native shutdown 0이라는
첫 안전 단언이 1≠0으로 실패한다. 시험 뒤쪽에는 원래 작업의 정상 출력·KV·전 stage release 완주와
idle UNLOAD 성공의 양성 대조가 있지만 첫 단언에서 멈춰 **실행하지 않았다**. 수정/복원 GREEN은 없다.
현재 정상 suite GREEN으로 이 반례를 덮지 않으며 다음 첫 행동은 로드맵 최신 절이 소유한다.

### native mask 수정과 실제 소비 회귀

`llama_stage_runtime_physical.cpp`의 FIRST/downstream이 내부 logits mask를 따로 만들도록
수정했다. 원래 owner/logical/capsule mask를 쓰기 변경하지 않는다. 정책·공통 P4·상류 patch·ABI·
common 의존 권한은 변경하지 않았다. 실제 생산 변경은 17행 추가/2행 교체다.

새 `physical_logits_consumer_test.cpp`는 생산 physical.cpp 본문을 직접 컴파일한다. model/context
설정과 native API 경계만 test-only Probe로 대체하고, 실제 llama_decode/encode 호출에 도달한
mask·token·position을 관찰한다. llama 라이브러리 자체는 이 실행파일에 링크하지 않는다.
초기 mixed-mask 시험은 수정 전 FIRST에서 assertion으로 실패했다. 이 RED 이후 all-Replay를
추가했으므로 최초 RED가 나중의 12개 경우까지 실행했다고 하지 않는다.

최종 경우는 first/middle/tail × decode/encode × all-Replay/mixed **12개**다. all-Replay의
literal mask는 `[1,1]`, mixed는 `[0,1,1,1,1,1,1]`이며 downstream의 입력/반환 wire mask는
그대로다. mixed geometry·atomic preparation은 fixture이고 합법적인 recurrent 실제 shape를
검증한 것이 아니다. FIRST의 internal capture mask를 wire owner.output으로 복원하는 별도
server 소비자는 코드로 확인했지만 이 시험에서 실행하지 않았다. 실제 logits 수치·sampler·
checkpoint byte·모델 또는 backend conformance를 이 12개로 승인하지 않는다.

독립 81파일 copy의 생산 소비 변이: FIRST가 옛 row.output 사용, downstream이 계산된 mask를
버리고 옛 input.output 전달, 반환 wire에 native mask 오염 — **각각 CTest 1 failed**다.
변이 사이마다 실제 test TU 재컴파일/실행파일 해시를 확인했으며 정확한 복원 후 **1 passed**,
81파일 내용 불일치 0이다. 상세는 `target/replay-logits-consumer-mutations/verification.md`,
최초 RED는 `target/replay-logits-consumer-evidence/red-*`에 보존한다.

- 최종 생산 source: `E90AF312E7489DCA02564BCEA6E884606B9FE4C8E31872A2D798CC221F547C7E`
- 최종 consumer 시험: `D5DF40C0D51BA68BE3119EC36D66F88A5822B40308CAAA50D54415A7C00EC0C8`

### 공식 native build와 미실행 본문 분리

no-llama 공식 build는 최초 C1083으로 **시험 0개 실행**이었다. `server_hello.cpp`의 compat
include는 사용부와 달리 무조건이었고, 같은 P4_STAGED_WITH_LLAMA guard를 넣어 고쳤다.
include path/link를 넓혀 통과시키지 않았다. 수정 후 **5/5** 계약 시험이 실제 실행됐다.
최종 CMake/builder/native 동결 뒤에도 아래 두 모드를 다시 실행했다.

```powershell
node layers/adapters/llamacpp/staged/scripts/build-stage-server.mjs --backend cpu --build-dir F:/dev/p4/target/native-identity-cpu --config Release --parallel 4
node layers/adapters/llamacpp/staged/scripts/build-stage-server.mjs --no-llama --backend cpu --build-dir F:/dev/p4/target/worker-spec-native-contract --config Release --parallel 4
```

| 최종 모드 | 실행 결과와 범위 |
| --- | --- |
| llama-linked CPU source build | exit 0, CTest executable **15/15**; 모델프리 본문 **12**, 부분 실행 **1**, 전체 본문 SKIP **2** |
| no-llama build | exit 0, 계약 executable **5/5**, 해당 본문 SKIP 0 |

15/15 중 compile 시험은 실제 KV restore/HOP rollback 두 부분을 생략했고, request_options/MTP
두 실행파일은 모델이 없어 본문 전체를 생략했다. SKIP 원문 총 **4줄**을 별도 집계한다. 15개를
모델 정상성 통과라고 하지 않는다. upstream recurrent rollback의 모델 필수 별도 실행도 하지 않았다.
새 소비 시험은 target-only Rebuild로 실제 TU를 다시 컴파일한 뒤 공식 CTest에서 12-case 완료
메시지를 확인했다. Release의 /UNDEBUG 단언도 활성이다. no-llama 5개와 linked 15개는 중복
타깃을 포함하므로 20개 독립 conformance로 합산하지 않는다.

`target/worker-spec-native-contract-evidence/final-linked.*`와 `final-no-llama.*`에 명령·exit·
LastTest·실행파일 해시가 있다. native/CMake/builder/prepare/manifest/patch **111입력파일**은
각 실행 전후·두 모드 사이 동일하며 최종 원본 재해시도 불일치 0이다. 실제 production runtime
object/link 성공과 위 Probe consumer 시험의 범위를 분리한다.

공식 prepare 전후 pin `0eadefebd3f8f92a86d634a0e5b8fffc9dc792c0`, patch diff
`3cfc636181e4ee1033249b8f7ca4d50156174138bf07c2797a476f4c42ab8c47`, 재계산 tree
`c81ecd4fff7c93a1f637d63733c0387f0b6bf157` 및 patch 24개 byte hash가 manifest와 일치했다.
모델 env는 자식 빌드/시험에서 명시적으로 제외했으며 모델/GPU/원격 실행은 없었다.

### 이번 slice 최종 기타 게이트와 한계

- JS **63/63**, skipped 0: 하네스 57 + 공식 build target wiring 6. 새 target이 dead code로만
  남는 변이도 builder의 실제 명령 포착 시험이 검출한다. `target/worker-speculative-js.log`.
- private-header **81파일 clean**, common debt **0 header / 5 source**; compat manifest valid 24.
  문자열/목록 gate를 전체 transitive 의미 격리 완료로 확대하지 않는다.
- 문서 tracked **73 clean** / all **79 clean**, 자체 **12/12**, cargo 문서 gate **1/1**.
  기본 git diff --check exit 0; 파일별 EOL 일관 정규화 후 검사했다.
- Rust 소스는 위 365파일 봉인과 최종 재대조해 동일하다. 현재 whole suite의 1042 GREEN 밖에
  busy UNLOAD 독립 RED 1개가 남으며 이를 소거하지 않았다.
- CUDA build, 실제 모델의 MTP/Replay, VRAM-only/RAM 오프로딩·다중 물리 호스트 웨이브·품질/TPS는
  이번 slice에서 **미실행**이다. 원격 배포·commit/push도 하지 않았다. 원자료 target은 로컬 보존이다.

## 2026-09-07 후속 기록 — busy UNLOAD와 native 종료 실패

기준은 같은 HEAD `a9e1967fc`와 미커밋 작업 트리다. 앞 절의 busy UNLOAD RED를 원본 actual
Worker::run에 이관하고, 정상 거부와 native cleanup 사후 실패를 분리했다. P4 중립 코어·정책·native
ABI는 이 slice에서 변경하지 않았다. 의미 계약은 배치 계약의 명시적 UNLOAD 절, 판정은 T25가 소유한다.

### 수정 전 실패와 실제 소비 범위

`target/b2-busy-unload-original-red-20260907.log`의 ordinary 두 시험과
`target/speculative-unload-red/red.log`의 speculative 두 시험은 기존 명령을 실제 run에 보냈다.
각 첫 안전 단언에서 native shutdown이 0 대신 1, UNLOADED가 0 대신 1이 되어 실패했다. RED는
그 뒤 정상 완주·idle 양성 대조까지 실행한 것으로 세지 않는다. source/exe 해시와 재컴파일 원문을
각 증거 디렉터리에 보존했다. helper의 진단을 강화한 두 번째 ordinary RED도 별도 raw에 있다.

| 추가 actual-run 회귀 | 보류된 실제 작업 | 수정 뒤 검사 |
| --- | --- | --- |
| ordinary head (N=2/4) | tail capsule-set; flight batch 1, executions 2 | busy 오류 1, native/출력 불변; 같은 요청 완주 뒤 idle 성공 |
| ordinary middle (N=4) | requests/pending/flight 0이나 active owner/frontier 각 1 | head 요청 원장 없이도 KV 보존; 이후 정상 완주/idle 성공 |
| speculative head (N=2/4) | 최종 SETTLED; flight/open view 0, pending settlement 1, Verify fence | 잘못된 FIRST/LAST generation 거부; 정산 재개 뒤 literal output/release/전 stage idle 성공 |
| speculative middle (N=4) | 아직 SETTLE 안 된 tentative Verify KV; requests/pending/flight 0 | native KV·append/restore 이력 보존; checkpoint Replay 정상 완주/전 stage idle 성공 |

공통 헬퍼는 정확한 source/correlation의 오류만 허용한다. native 호출·sampler·KV·write/release 이력·
speculative 이력의 전체 스냅샷과 요청 출력을 비교하며 모든 오류를 삼키도록 pump를 바꾸지 않았다.
수동 RequestState나 공유 census 함수를 oracle로 부르는 대신 실제 이벤트로 만든 상태의 진단을
대조한다. 성공 receipt/Released history만 남은 경우에는 idle UNLOAD가 가능해야 한다.
기존 ordinary 5/speculative 3과 추가 4를 합쳐 actual pipeline loop는 **12/12**다.

생산 `control.rs::Worker::unload`는 identity 검사 후 `shutdown.rs::Worker::require_idle_unload`를
통과한다. 기존 종료용 로컬 잔량 집계를 공유하되 실패 정리를 이 guard로 막지 않는다. 이미 fenced된
worker의 fatal/Drop cleanup은 native 자원을 닫을 수 있다. 따라서 native 0은 healthy busy 사전 거부의
약속이며 모든 종료 종류의 약속이 아니다. 이 시험은 LOAD/subprocess·실제 llama/model/backend·
네트워크 broker·global drain을 지나지 않는다. 전달한 토큰을 취소하거나 사용자 출력 ACK를 증명하지 않는다.

### guard가 실제로 필요한지 — 독립 변이

`target/b2-busy-unload-mutations-20260907/verification.json`: 독립 3-crate/133소스 copy에서
시험/fake는 고정하고 생산 코드만 바꿨다. 모든 arm 실제 재컴파일·동시점 실행파일/원문 해시를 보존한다.

| 조건 | loop 통과 / 실패 |
| --- | --- |
| 기준선 | 12 / 0 |
| UNLOAD guard 호출 제거 | 8 / 4 |
| requests-only busy 판정 | 10 / 2; ordinary/speculative 중간 KV만 실패 |
| 무조건 busy 거부 | 8 / 4; 원래 요청 완료 뒤 idle 양성 대조에서 실패 |
| 정확히 복원 | 12 / 0 |

변이마다 control.rs 또는 shutdown.rs 한 파일만 달라졌다. 복원본·기준선·현재 원본 133파일의
내용이 일치했다. 테스트 합계 감소/상한 완화·골든 변경으로 변이를 통과시키지 않았다.

### 별도 발견 — idle native UNLOAD 실패 뒤 SESSION 승인

실제 run에 SESSION → idle UNLOAD → 이미 큐잉된 SESSION을 넣고 native shutdown만 실패시켰다.
`target/worker-unload-failure-regression-20260907-01/01-before-fix-compiled.stdout.log`는 실제
재컴파일 뒤 **0 passed / 1 failed**다. cleanup 1회 뒤 오류가 있어도 후속 SESSION이 ACK되었고
최종 상태는 `closed:local_work_empty`, `effects_fenced:false`였다. 앞의 00 로그는 동시 빌드 산출물을
재사용한 보조 관측이므로 01의 fresh compile 증거로 대체하지 않았다.

native cleanup Err에서 effects fence를 세워 실제 handle이 오류 응답 후 종료하게 했다. 원래 오류와
불확실 경계를 보존하며 UNLOADED·후속 SESSION ACK는 0이다. 기존 turn 시험 8개 + 새 회귀 **9/9**.
독립 copy에서 fence 한 줄만 제거하면 같은 queued-SESSION 단언으로 **0/1 실패**, 복원 **9/9**다.
135입력파일(Cargo 포함)의 기준선/복원/원본/증거 사본이 일치하며 각 단계 실제 compile·exe 해시가 있다.
`target/worker-unload-failure-regression-20260907-01/verification.txt`가 명령과 전체 범위를 기록한다.
fake shutdown 횟수는 실제 OS process 종료/소멸자 재시도의 증명이 아니며 실패 lifecycle 재로드는 구현하지 않았다.

### 최종 소스와 전체 집계

- control.rs: `50046B4A9B973D265833B3DEC65347F05ECADCC6916B5354290BB15E74D031DD`
- shutdown.rs: `3A673D5B1929DB19AE906E666525570D4DE0868650B966A610FCD0652811EB2C`
- Rust/Cargo **366파일** 봉인: `7e3257ed5f97f9ce586d6c3871f3e3621e5619f0208e1a1e960f2aa143478e23`.
  `target/worker-unload-rust-source.json`, `worker-unload-seal.mjs --verify`로 전체 실행 전후 동일성을 확인했다.

첫 `--workspace --no-fail-fast` 실행은 **1046 passed / 1 failed / 7 ignored**, exit 101이었다.
`target/worker-unload-workspace.log`의 유일한 실패는 새 배치 계약 문단의 mixed EOL 문서 gate였다.
이를 정규화하고 전체를 다시 실행한 `target/worker-unload-workspace-final.log`는 최종 exit 0,
57 summary, **1047 passed / 0 failed / 7 ignored**다. staged adapter lib **351**이며 이전 1042에서
busy 4 + native 실패 1만 증가했다. 1047개를 실기/E2E 개수로 부르지 않는다.

JS는 **63 passed / 0 failed / 0 skipped**(하네스 57 + build wiring 6), 원문은
`target/worker-unload-js.log`다. clippy staged/agent-core --all-targets exit 0이나 staged lib-test
**26 warnings (13 duplicates)**가 남는다. 새 speculative fixture clone 스타일 경고 2개도 포함한다.
private-header는 **81 clean**, common debt **0 header / 5 source**이며 compat manifest는 valid 24다.
이번 Rust-only slice에서 C++/CUDA를 다시 실행하지 않았다. 앞 slice의 모델 없는 CPU15/no-llama5를
현재 실제 모델 conformance나 새 실행으로 재표기하지 않는다.

Cancel/Drain·OUTER 소비 ACK·재시작 신선성·edge credit는 여전히 미완이다. 실제 모델/GPU·원격 배포·
VRAM-only/RAM 오프로딩 웨이브·성능 비회귀·commit/push는 이번 slice **미실행**이다. 원자료 target은
로컬 보존이며 공개된 불변 증거 번들이 아니다. 다음 작업과 승인된 실기 순서는 최신 로드맵을 따른다.

### 전체 GREEN 밖의 다음 P1 — 실제 head 출력과 OUTER의 tail 전제

`target/head-output-consumer-red-20260907-01/verification.md`의 독립 copy에서 실제 run의 출력
메일박스를 관측했다. ordinary 2-stage 기존 시험 **1 passed**, 출력 5개; checkpoint Replay 2/4-stage
기존 시험 **1 passed**, 출력 10개다. 기존 token/position/KV/release oracle는 그대로다. 이 Event 15개를
그대로 `p4_protocol::event::encode`로 보존하고 실제 event-drive `InferenceIdentity::output`에 공급했다.

ordinary 소비자는 **1 failed(5/5 거부)**, checkpoint 소비자는 **1 failed(10/10 거부)**다. 오류는 모두
`inference event source or correlation is incorrect`다. correlation/body/target/load/session은 고정하고
source만 configured tail로 바꾼 별도 대조군은 각각 **1 passed**, 15개 모두 승인됐다. 이것은 원인을
분리하는 잘못된 과거 source의 대조군이지 tail 발행을 다시 허용하라는 결론이 아니다.

생산자 fresh compile 27.02초, 소비자 fresh compile 19.65초와 EXE/367입력파일 해시, 실제 encoded
출력 15개, 두 소비 RED 원문을 보존했다. 변한 것은 복사본 시험 두 파일의 관측/소비 probe뿐이고
생산 알고리즘·원본은 무변경이다. 1047 전체 원본 GREEN에 이 신규 copy 시험이 포함됐다고 하지 않는다.
생산자의 네트워크·모델 연산은 fake이며 전체 `inference::drive` 또는 실제 GPU 웨이브는 미실행이다.
현재 strict unit test가 오히려 head 거부를 기대하므로, 소비자만 head/tail 모두 허용하도록 고쳐서는 안 된다.

최종 문서 검사도 수행했다: 추적 **73 clean**, 전체 **79 clean**, 자체 **12/12**; 전체 Rust 안의
문서 gate **1/1**. 최종 증거 문단 추가 후 lint와 기본 diff --check를 다시 확인하며 Rust 366파일 봉인은
변경하지 않는다. Git의 향후 LF→CRLF 변환 경고는 남지만 whitespace 오류/exit 실패는 없다.

## 2026-09-07 후속 구현 — head OUTPUT의 실제 생산·소비 계약

기준은 `a9e1967fc` + 이 문서의 후속 미커밋 변경이다. 앞 절의 독립 RED를 원본 기본 시험에 이관했다.
`tools/event-drive/src/run/inference_identity.rs::InferenceIdentity::output`은 configured head의
전체 endpoint를 요구한다. sampling 위치인 tail과 승인된 OUTPUT 발행자를 구분하며, 이전 load/session/
request/route/position 검사와 중복/terminal 뒤 거부를 없애지 않았다. 어댑터 생산자가 이미 head 발행을
구현하고 있었으므로 이번 production 수정은 OUTER consumer 한 파일이다. P4 중립 transport나 llama ABI는 무변경이다.

### 지속 회귀와 증거 범위

- `adapter/test-fixtures/head-approved-output-v1.json`: actual Worker::run에서 캡처한 encoded OUTPUT
  15개(ordinary 2-stage 5개, checkpoint Replay 2/4-stage 10개). test-only `head_approved_output.rs`를
  어댑터와 drive가 함께 읽는다. 새 public production API나 선택 feature는 추가하지 않았다.
- 실제 producer 기본 loop 시험은 현재 출력을 이 wire의 의미 투영과 대조한다. source/target/reply route
  전체와 protocol/class/content/adapter/correlation/deadline, Outcome JSON 전체를 비교한다. 기존 literal
  token/text/position/KV/정산/release oracle는 유지했다. loop **12/12**이며 기존 두 시험에 대조를 추가했다.
- event_id/causation_id/sequence의 실행별 값은 의미 투영에서 제외한다. 현재 실행의 ID 유일성·Event
  유효성·causation 존재·source별 sequence 증가·요청별 위치 순서를 별도로 본다. **causation 존재는 정확한
  원인 terminal의 ID 동일성 증명이 아니다.** 캡처 파일은 순서 로그가 아닌 집합이므로 소비 시 요청별
  position으로 정렬한다. producer의 live 순서 검사는 정렬 전에 수행한다.
- 실제 `InferenceIdentity`와 `inference::drive`가 같은 15개를 소비한다. 인메모리 framed EventWire와 실제
  state loop를 지나며 source/세대/route/load/session/request/위치/동일 Event 중복/terminal 뒤 출력 부정
  입력도 검사한다. 종료용 RELEASED는 **synthetic**이다. 실제 release 집합이나 network/LOAD/native/GPU
  증명이 아니며, 두 consumer 모듈의 기본 시험은 **7/7**, event-drive 전체는 **22/22**다.

원본 수정 전 fresh compile RED는 `target/head-output-consumer-original-red.log` 및 동명 snapshot에
**0 passed / 1 failed**로 남았다. 이전 tail만 허용하면 head positive가 실패한다. 앞 절 15개 전체 거부
실행과 이번 지속 기본 회귀를 구분한다.

### 독립 변이 — 양쪽 경계가 각각 실패해야 함

| 변이 | 기준선 | 잘못된 변경 | 복원 |
| --- | --- | --- | --- |
| producer를 head 대신 base의 tail source로 발행 | 12/0 | 10/2 | 12/0 |
| 같은 길이의 fixture 응답 text 오염 | 12/0 | 11/1 | 12/0 |
| consumer tail-only 회귀 | 7/0 | 3/4 | 7/0 |
| consumer가 모든 configured node 허용 | 7/0 | 5/2 | 7/0 |
| consumer route 검사 제거 | 7/0 | 5/2 | 7/0 |

표는 passed/failed다. `target/head-output-producer-mutations-20260907-01/verification.md`에 각 fresh
compile·EXE·370입력파일과 복원/원본 mismatch 0을 보존했다. producer 370에는 기존 deployment의
`fixtures.json`도 포함된다. checkpoint 변이는 2-stage iteration에서 먼저 실패하므로 그 실패 실행을
4-stage까지 수행한 것으로 세지 않는다. 정상/복원은 두 크기를 모두 수행한다.

최초 producer 복원은 파일 mtime까지 복사해 Cargo가 변이 EXE를 재사용했다. 해당 로그는 `INVALID-*`로
보존하고 유효 복원에서 제외했다. mtime을 갱신한 실제 재컴파일/12 PASS를 따로 남겼다. 원본 checkout은
변이에 쓰지 않았다. `target/head-output-consumer-mutations-20260907/verification.json`은 소비측
161파일 기준선/복원/현재 원본 일치와 세 변이의 실제 compile·EXE·raw log를 보존한다.

### 최종 집계와 소스

- consumer production SHA256: `6DF744B8F0321FBCD007AB683FB8ED8F453C4E56F825078BBBBB852D1519FDFB`.
- shared OUTPUT JSON: `5C566577E988048335BD34CA10ECA94F8E96F7A448D7BBE126CBF7A221F40D21`.
- shared test helper: `6CCC6AC81EBD535BAA6E8D881610BCC5C4B514A8309E9E2A91E32DFFDE99CA2F`.
- 전체 실행 전후 Rust/Cargo + 새 OUTPUT JSON **369파일** 봉인:
  `b6c164b21e3131f421a195cc0d4a6c0f3d673a7b548aaa862a2aab7f367d0dff`.
  `target/head-output-rust-source.json`과 `head-output-seal.mjs --verify`를 사용했다. 이 369파일 집계는
  기존 deployment JSON·문서·compiler/registry 원문까지 포함한 모든 빌드 입력이라는 뜻은 아니다.

`target/head-output-workspace.log`: `cargo test --workspace --no-fail-fast`, 최종 exit **0**, 57 summary,
**1051 passed / 0 failed / 7 ignored**. 이전 1047 + 실제 consumer 기본 회귀 4이며 producer parity는 기존
시험 두 개를 강화했다. `target/head-output-js.log`: 하네스57 + build wiring6 = **63 passed**, 실패/생략0.
`target/head-output-clippy.log`: staged/agent-core/event-drive --all-targets exit0이지만 경고는 남는다.
staged lib-test 26(13 duplicates), event-drive bin-test 7(6 duplicates); 경고 없는 완료로 보고하지 않는다.
private-header **81 clean**, common **0 header / 5 source**, 현재 compat manifest **valid 24**다.
C++/CUDA·원격·실제 모델·VRAM-only/RAM 오프로딩 웨이브·성능 비회귀·commit/push는 이번 slice 미실행이다.

### GREEN 밖의 다음 세 소비자 RED

`target/outer-inference-consumer-probes-20260907-01/verification.txt`와 `consumer_probes.rs`에 정상
1 PASS / 거부 규약 **3 RED**를 남겼다. 권위 실행은 `03-final-identity-counterexamples.*`다.
수정된 head source/최종 formatting까지 복사하고 재컴파일했다. 원본 1051에 포함되지 않은 copy 시험이다.

실제 drive가 보낸 PREFILL을 독립 peer가 받아 검증한 뒤 current head/route의 BATCH_OBSERVATION,
OUTPUT, RELEASED를 반환한다. drive의 실제 상태→execute와 같은 관측 집계→실제 acceptance를 통과한다.
정상 대조는 프롬프트 [4,7], 첫 위치 [4,7], 각 두 토큰, 정확한 읽을 수 있는 응답, 서로 다른 해제 요청이다.

1. A만 실제 peer 해제 집합에 넣고 fresh-ID RELEASED(A,count=1)를 두 번 보내면 completed2/released2로
   승인된다. 정상 producer가 이 잘못을 한다는 증명이 아니라 consumer가 해제 집합을 증명하지 못함이다.
2. max_tokens=1 제출에 contiguous OUTPUT 두 개, 두 번째 length 종료를 보내면 sampled2인데 승인된다.
3. 프롬프트 관측 [4,7]에서 B의 위치를 전부 +1 옮겨 첫 위치 [4,8]로 보내도 선택적 공통 경계가 없으면 승인된다.

최종151소스 before/after/사본 일치와 EXE `06AA14DEDDC06EC9B158805618EE445746286E1437D9534FF5FDB6F5D65AD3D5`
및 cargo exit101을 보존했다. 원본 수리는 하지 않았다. peer의 해제 집합은 자기 payload count와 독립이나
실제 native KV 관측은 아니다. CREATE/LOAD/UNLOAD/DELETE 전 RPC나 실제 GPU 웨이브도 포함하지 않는다.
T20에 필수 완료 판정 반례를 등록했으며 구체적 다음 순서는 최신 로드맵이 단독 소유한다.

## 2026-09-07 후속 구현 — OUTPUT 예산과 fresh-prefill 관측의 실제 소비

기준은 `a9e1967fc` + 후속 미커밋 변경이다. 앞 절의 세 소비자 반례 중 출력 예산과 요청별 첫 위치를
수정했다. **해제 멤버십은 아직 수정하지 않았다.** source 승인·정상 text 존재만으로 완료를 승인하지
않도록 실제 drive/최종 acceptance 양쪽에 필요한 검사를 추가했다. P4 중립 transport/native ABI는 무변경이다.

### 구현된 경계와 아직 없는 증명

- `tools/event-drive/src/run/output_budget.rs::validate_output`: 이미 받은 sampled 개수와 들어오는
  한 OUTPUT으로 예산/terminal을 검증한다. 빈 EOS도 1개이며, `length`는 예산 끝에서만 가능하다.
  `stop`/`eos`는 조기 종료 가능, 알 수 없는 이유·상한 도달 후 비terminal은 거부한다. 실제 drive는
  response/outcomes/completion을 바꾸기 전에 호출한다. acceptance는 보존된 전체 outcomes를 다시 검사한다.
- `inference_evidence.rs::apply_observations`: 전체 후보를 검증한 후 요청별 counters를 한 번 설치한다.
  같은 observation ID의 같은 body 재전달은 기존 insert 경로에서 dedup한다. 다른 observation ID가
  같은 physical execution을 재사용하거나 unknown request/합계 overflow를 만들면 거부한다.
  모든 완료 요청은 양수 prefill 관측이 있어야 하고 first OUTPUT position이 그 요청 합계와 같아야 한다.
  뒤 요청이 실패할 때 앞 요청 counters도 미변경이라는 회귀를 포함한다.
- 현재 drive의 completed/released 최종 경계에서 검증한다. OUTPUT 뒤 관측이 오고 마지막 해제 전
  도착하는 양성은 허용한다. **그 경계 뒤 늦은 관측을 기다리는 재조정은 구현하지 않았다.**
  누락/불일치는 성공 report가 아니라 오류다. execute의 중복 후처리 집계를 제거했으며, 현재 fake-peer
  시험은 drive와 acceptance를 지나지만 CREATE/LOAD/UNLOAD/DELETE 전 RPC orchestration은 지나지 않는다.
- 이번 위치 대조는 fresh position 0 제출과 head-reported prefill 증거 사이 관계다. 독립 native tokenizer/
  실제 KV 또는 Restore/LCP의 위치 증명이 아니다. 빈 EOS의 protocol-valid와 nonempty/minimum 응답
  quality-pass도 구분한다. 기존 response 전문/최소 길이/judge는 완화하지 않았다.

### 기본 시험과 실제 생산 경로

`consumer_budget_boundary_tests.rs`의 **15개**는 실제 drive가 보낸 PREFILL을 peer가 먼저 읽어 검사한 뒤,
bounded duplex의 framed EventWire로 응답을 준다. 독립 프롬프트 행수 [4,7]·읽을 수 있는 정확한 응답을
사용한다. 예산 초과/첫 위치 변경/관측 누락/정확 재전달/ID 재사용/변형 body/역순/늦은 관측/빈 EOS/
조기 stop·eos/조기 length/unknown stop/terminal 뒤 출력을 다룬다. 시험이 request counters를 대신
채우지 않는다. 거부는 final acceptance=false만이 아니라 **실제 drive Err**여야 한다. 해제 통지는 synthetic이다.

기존 actual producer OUTPUT JSON 15개는 수정하지 않았다:
`5C566577E988048335BD34CA10ECA94F8E96F7A448D7BBE126CBF7A221F40D21`.
공용 helper는 actual loop가 받은 전체 이벤트의 BATCH_OBSERVATION에서 request/physical prefill을 읽고,
OUTPUT position에서 역산하지 않은 독립 workload 상수 ordinary=7, partial=3, fence-probe=1과 대조한다.
ordinary 2-stage 및 checkpoint Replay 2/4-stage의 기존 token/text/position/KV/release oracle는 유지한다.
producer는 fake native이므로 독립 tokenizer 증명이 아니다. 기존 fixture consumer에 새로 감싼 관측/
RELEASED는 synthetic이며, 원래 OUTPUT만 actual capture다. 실제 producer loop는 기존 **12/12**다.

초기 empty-EOS consumer 시험이 config의 `exact_response=""` 금지에 막혀 **14/15**, 전체 drive **46/47**였다.
입력 자체가 illegal한 fixture였으므로 expectation을 None으로 고쳤다. EOS token/text·min length=1·빈 응답
quality 실패 단언은 유지했다. 이 초기 RED를 생산 코드 결함이나 성공으로 세지 않는다.
`target/outer-budget-boundary-initial.log` 및 독립 기록에 원문을 남겼고 최종 drive는 **47/47**다.

### 실제 호출과 독립 변이

| 대상 / 잘못된 변경 | 기준선 | 변이 | 정확 복원 |
| --- | --- | --- | --- |
| actual drive의 budget 검사 호출만 제거 | 15/0 | 12/3 | 15/0 |
| actual drive의 observation 적용 호출만 제거 | 15/0 | 6/9 | 15/0 |
| budget의 early-length 검사 제거 | 47/0 | 44/3 | 47/0 |
| budget에서 빈 EOS 예산 예외 허용 | 47/0 | 46/1 | 47/0 |
| acceptance 첫 위치 대조 제거 | 47/0 | 46/1 | 47/0 |
| producer의 prefill 통계를 aggregate/request 모두 decode로 오분류 | 12/0 | 10/2 | 12/0 |

passed/failed 표다. 빈 EOS 변이는 pure helper 시험만, acceptance 변이는 해당 최종 대조 시험만 실패했다.
다른 layer의 방어가 남아 있으므로 모두 actual drive까지 실패했다고 하지 않는다. 반면 budget 호출 제거는
acceptance가 여전히 거부해도 drive가 잘못 완료해 3개가 실패한다. 생산 오분류는 물리 행수·native 동작·
출력을 바꾸지 않고 관측 두 카운터만 바꾼다. 기존 OUTPUT/KV oracle는 통과하되 새 prefill 대조가 실패한다.
checkpoint 변이 실패는 2-stage에서 멈추며, 기준선/복원은 2/4-stage를 모두 수행한다.

- `target/consumer-budget-boundary-regressions-20260907-01/verification.txt`: actual consumer 변이·
  원문·소스/EXE. 이전 소비자 보존본(HEAD 자체 아님)에 동일 시험을 넣은 fresh compile은 **6/9**다.
  6개는 잘못된 스트림 승인, 3개는 새 최종 counters postcondition 미충족이다. 그 3개를 과거 정상 입력
  거부라고 부르지 않는다. 권위 실행 04/05/06/10/11은 모두 실제 Compiling과 input before=after를 남긴다.
  03은 복사 mtime 때문에 old EXE를 재사용하여 **무효**다. 07은 EOL이 달라 byte-exact 복원 증거에서
  제외한다. 10은 baseline 154파일 정확 복원, 11은 최종 producer fixture 변경 후 15/0 및 원본 대응
  153파일 일치다. 사본의 나머지 1파일은 모듈 등록되지 않은 옛 probe이며 실행 coverage에 포함되지 않는다.
- `target/output-budget-mutations-20260907/verification.json`: pure/acceptance 변이 세 가지와 최종
  47/0, 167입력(원본 대응164 + 독립 workspace/Cargo 설정), 현재 대응 파일 일치, EXE/compile 원문.
  수정 전 acceptance cap RED는 `target/output-budget-acceptance-original-red.log`에 별도로 남는다.
- `target/prefill-producer-mutations-20260907-01/verification.md`: producer baseline/변이/복원 모두
  재컴파일, 372입력 source/EXE, restore/current mismatch 0. 이 copy scope는 아래 전체 봉인의 기존
  deployment JSON을 제외하므로 373과 다른 수다. 원본은 변이에 사용하지 않았다.

### 최종 소스·집계

`target/outer-budget-source.json`과 `outer-budget-seal.mjs --verify`는 Rust/Cargo와 두 literal embedded
JSON fixture **373파일**을 봉인한다. 문서·compiler/registry package 원문은 이 범위 밖이다.
SHA256 `d234508fd86df5caff045e63994a42a5f943407d36bec67d5eb9fd32567c2b71`.

- `target/outer-budget-workspace.log`: `cargo test --workspace --no-fail-fast`, 최종 exit0, **57 summary,
  1076 passed / 0 failed / 7 ignored**. 이전1051 + consumer15 + budget3 + acceptance5 + evidence2.
  producer 두 기존 시험 강화는 새 시험 수에 더하지 않는다.
- `target/outer-budget-js.log`: 하네스57 + build wiring6 = **63/0**, skipped0.
- `target/outer-budget-boundary-final-focused.log`: event-drive **47/0**.
- `target/outer-budget-clippy.log`: staged/agent-core/event-drive all-targets exit0. staged lib-test26
  (13 duplicates), event-drive bin-test7(6 duplicates) 등 경고는 남는다. warning-free로 보고하지 않는다.

C++/CUDA·실제 network·remote deployment·모델/GPU·VRAM-only/RAM offload 웨이브·성능·commit/push는
이번 slice에서 실행하지 않았다. target 원문/사본은 로컬 보존이며 공개 불변 번들이 아니다.
scalar RELEASED의 request 집합, 여러 OUTER의 소유자별 해제 통지, publish 실패 후 notification intent는
현재 GREEN 밖의 다음 감사 범위다. 구체 작업 순서와 단계 승격은 최신 로드맵만 소유한다.

### GREEN 밖의 해제 경계 RED — 실제 생산과 broker 소비를 따로 확인

아래 신규 시험은 독립 copy에만 있고 위 원본 1076 집계에는 포함되지 않는다. 원본 production 수리는
아직 하지 않았다. source·원문·입력/EXE·scope를 각각 보존했으며 반례를 fake GPU 성능으로 부르지 않는다.

**소유자 라우팅** — `target/release-owner-routing-red-20260907-01/verification.md`.
actual Worker::run의 2-stage에서 A/B가 같은 execution=1 terminal capsule에 둘 다 있다는 것을 먼저
단언했다. 원래 token/text/position/stop 및 모든 stage의 KV 해제 한 번 oracle를 통과한 뒤 새 통지 검사가
실패한다. A의 ingress42991/channel owner-a/connection11/correlation-a로 released=2, B의 독립
ingress42992/channel owner-b/connection12/correlation-b로는 0이다. 단일 OUTER 양성은 count1로 통과한다.

baseline12/0(fresh compile26.08초), 새 두 시험 후 **13/1**(6.15초), 별도 관측 캡처 시험 추가 후 **14/1**
(6.83초)이다. 최종 EXE `6EDB6F3AE20DB1653DCA92793F57E35BADA2C165C8D124D9C728A4B8186835B5`,
copy loop 시험 SHA `95DEBB54EB02E91886896A2FFBE959D841B91EE36609E634F27DC4759B64DA3A`.
372입력 봉인/원본변경0이며 copy의 loop_tests.rs만 바뀌었다. post-LOAD fake native를 사용하는 actual
run이지 LOAD negotiation·EventBroker/network 또는 실제 llama/GPU가 아니다.

실제 관측도 별도로 캡처했다. 최초 독립 correlation은 event-drive의 request_id=correlation 전제와 다르므로,
별도 actual run에서 telemetry-owner-a/b 입력의 correlation을 각 request ID로 발행했다. **생성된 wire는
사후 수정하지 않았다.** `captures/request-correlations.json`의 SHA는
`01C3E73F1CFE4AFB8E504B79B6CE1766544BF2A37D4A722249A14A3F1B8BB4F7`이다. 실제 관측 두 개는
각 route로 가지만 body는 A/B 요청 전체를 포함하고, stage span 두 개는 A route에만 간다. 이 생산 관측과
실제 소비자의 승인 여부는 별도 실행 범위로 나눠 기록한다.

**캡처 관측의 실제 소비** — `target/captured-mixed-outer-consumer-red-20260907-01/verification.txt`.
위 request-correlations 캡처를 바이트 변경 없이 protocol decode→현재 InferenceIdentity::observation에
재생했다. A/B의 독립 제출 집합은 각각 자기 요청 하나다. 두 route 모두 unknown-request 오류로 거부된다.
동일 wire를 두 요청 모두 명시 허용한 진단 대조군에 넣으면 통과한다. 이는 source/route/load/session/
correlation/숫자 형식 오류가 아니라 foreign member 문제를 분리한 대조이며, 모든 요청을 허용하라는 수리가 아니다.

fresh compile2.35초, **1 PASS/1 RED**, 입력155파일 before=after와 raw capture 불변, 실행된 consumer/
protocol6파일 원본=copy다. EXE `9B0A7EFD3C830859F5FACD076366E4CE7D4B9AC476E3AEE7B490F945D2AE8346`.
같은 함수가 actual drive에서 호출되지만 이 실행 자체는 identity 소비 지점만 통과하며 전체 drive/bootstrap/
network 실행은 아니다. 캡처의 StageSpan B 누락은 생산 관측/코드 사실이고 이 소비 시험의 coverage 단언은 아니다.

**ACK source 권위** — `target/released-source-authority-red-20260907/verification.json`.
독립 head→middle→tail topology와 정확한 pending release 두 개를 준비하고 encoded Event를 실제
EventBroker::dispatch→node receiver→Worker::handle→released에 넣었다. tail은 **1 PASS**, 같은 body의
middle·pipeline 밖 Node는 **2 RED**다. 두 RED 모두 Enqueued, ERROR0/RELEASED1이며 pending 두 개 삭제,
slot0의 waiting 요청 재수용, 남은 free=[1], Verify fence 해제가 발생한다. `next`는 이 시험에서 middle이다.

기존 release 시험 baseline3/0는32.67초 fresh compile, 신규 source 시험 **1/2**는17.66초 fresh compile다.
RED EXE `c6e7754b24a53e79653af8bd22c2c0d9d3524a975b397e47255b150daf672a0e`와 같은 소스/실행파일로
`--test-threads=1`을 재실행해 before/after 원문을 온전하게 남겼다. `red-serial-output.log` SHA는
`0744604305919a7b2a51758eba5181f16fc7125a811c7de1795c7310d47ba509`다. 보존 242입력, 원본 대응239의
production 차이0. 독립 workspace/test dev-dependency/축소 lock은 명시했으며 공유 registry 버전과 checksum은 같다.

이 시험은 pending 상태 주입 후 실제 broker/handler 소비다. 원래 native RELEASE 체인 전체 생성,
EventNode async run·TCP 인증·GPU 실행은 미실행이다. “외부”는 pipeline 밖 Node envelope이며 외부 네트워크
침입을 증명한 것이 아니다. source 역할의 모델 의미는 adapter의 SESSION 계약에 있어야 하며 중립
broker에 llama 지식을 넣는 수리는 제안하지 않는다.

**코드 감사만 완료한 별도 표면**: release.rs는 ACK 검증 뒤 pending/slot/admission을 바꾸고 직접 emit한다.
Closed 또는 이벤트 ID 고갈 뒤 notification intent를 effects에 보존하는 actual 회귀는 아직 없다.
이를 위 두 RED와 섞어 “실제로 세 결함을 재현했다”고 하지 않는다. 해제 group/attempt/terminal 기대값과
재시작 freshness도 새 wire 계약/시험 없이 scalar 교체만으로 닫을 수 없다.

문서 게이트는 추적73/전체79 clean, 자체12/12, cargo 문서 gate1/1이다. private-header81 clean,
common0 header/5 source, 현재0eadefebd manifest valid24도 다시 실행했다. 전체 준비/새 pin 의미 호환이나
native 모델 재검증은 아니다. 문서 변경 후 source373 봉인과 기본 diff 검사를 다시 확인한다.

## 2026-09-07 후속 — SESSION 권위와 해제 ACK 발신자 경계

HEAD `a9e1967fc` + 미커밋 작업 트리다. 이전 절에서 원본 밖 RED로 남긴 내부 ACK source를 수리했다.
SESSION v4 의미/제약은 배치 계약 단독 소유다. P4 중립 protocol/broker와 native/llama/backend 생산은
이 slice에서 바꾸지 않았다. staged Cargo의 agent-core 의존은 broker 시험용 dev-dependency다.

### 실제 소비와 유지한 정상 경로

- `worker/release_tests.rs`: 실제 SESSION 설치 뒤 codec→EventBroker::dispatch→Worker::handle로
  정상 terminal, next인 middle, pipeline 밖 Node, 오래된 terminal node generation, 다른 agent의
  동명 terminal을 대조한다. 거부 시 pending/slot/예약 대기/Verify fence/effects를 보존하고 같은
  본문을 정상 terminal로 다시 보내 성공한다. 기존 release/admission 3개를 포함해 **8/8**.
- `worker/loop_tests.rs`: 기존 ordinary 2/4/8 및 speculative 2/4의 token/text/position/stop·각 stage
  native KV/release oracle를 유지한다. 새 3-stage 시험은 실제 ACK들을 보류한 상태에서 middle의
  fresh-ID ACK를 주입한다. 8개 요청 뒤 대기한 9번째 요청이 구코드에서는 조기 native 발행됐고,
  수정 뒤에는 native 상태가 보존된다. 정상 ACK 재개 뒤 9개 모두 정상 완주한다. **13/13**.
  shared OUTPUT 15개 JSON와 prefill 기대값을 수정하지 않았다.
- `worker/session_tests.rs`: 설치한 세 role과 같은 선언 반복, 설치 전/후 malformed·rebind,
  local endpoint/index/target·구 wire를 검사한다. 설치 후 immutable 비교가 최초 선언 검증 누락을
  숨기지 않도록 설치 전 부정도 둔다. stage 메시지 여섯 계열은 source/target 각각 잘못된 codec 입력을
  실제 handler에 넣어 정확한 route 거부를 확인한다. **5/5**. 빈 상태의 이 행렬을 native 효과 시험으로 세지 않는다.
- 이전 fixture 5파일의 정상 metadata를 새 SESSION에 맞췄다. 기존 worker27/incarnation3/turn9/
  physical-replay28/stage20 = **87개**의 body/원장/불확실성 단언을 유지했다. target만 실제 수신자에
  맞추는 fixture wrapper는 source를 보정하지 않으며, wrong-target 증명은 새 명시 시험이 담당한다.
- OUTER `session_events`는 execute가 실제 사용하는 생산 함수다. 독립 상수의 세 노드 순서와 각
  target/index·v4 JSON·기존 Sender sequence를 고정한다. 실제 ExpectedReply/receive_exact의
  역순 ACK 양성, source/causation 부정은 bounded in-memory EventWire를 사용한다. 새 **3/3**,
  event-drive 전체 **50/50**. 이 시험은 실제 TCP CREATE/LOAD bootstrap을 실행하지 않는다.

### 구코드 반례와 독립 변이

1. `target/release-source-authority-red-20260907-01/verification.md`: 이전 봉인372파일과 동일한
   독립 구소스에 새 actual run 시험만 이관하고 SESSION literal만 구스키마로 맞췄다. fresh compile
   26.85초, **12 PASS/1 RED**. `waiting_started=true`, head logical2→3; 현 원본은 false/2→2.
   구 RED EXE `80AFD2034067AF7BE27F618BB8ED8764231F349E961CD610AA9D8441E389FCBA`.
   RED copy는 시험 파일 하나만 달랐으며 원래 보존 구소스는 변경하지 않았다.
2. `target/release-source-guard-mutations-20260907/verification.json`: 포맷 전 결과와 최종 byte 결과를
   나눴다. **final-** 5arm은 baseline8/0 → guard 허용전부4/4 → 복원8/0 → 거부전부2/6 → 복원8/0.
   각각 fresh Compiling 6.84/6.76/4.27/3.99/4.00초. 잘못된 source뿐 아니라 정상 재시도까지 검사하므로
   영구 거부도 실패한다. final-restored-deny EXE
   `e05eb7d0c0b7f00df55385cc4cf76e69ca74ad8cc3fff06e11784d17d1d7eb4d`.
   243입력 중 저장소 대응239파일은 최종 원본=복원이며 final arm 동안 원본 불변이다.
   독립 축소 workspace/lock과 원래 선언/lock을 따로 보존하고 공유 registry package checksum을 대조했다.
3. `target/session-v4-builder-mutations-20260907-01/verification.txt`: 실제 producer 인자를 v3로
   낮추면 **2/1**, middle을 생략한 head/tail 배열로 바꾸면 **2/1**, 정확 복원 **3/0**.
   154선정입력과 실행 EXE를 봉인하고 매 arm 실제 compile을 확인했다. final-restored EXE
   `1DB93BA9FBD2E46B9C9C1AC94201447BB170E3F85B104FE0097E3B77F2F5CDA3`.
   추출 전 이미 적용 중이던 v4 생산의 회귀 시험이지 builder 추출 자체의 구코드 결함 수리 주장은 아니다.

모든 변이는 독립 copy에서만 실행했다. 현재 원본의 새로운 SESSION 시험 초기 실패는 test 주소의
`tcp://` 누락과 비terminal capsule의 tensor 누락이었다. 검증기를 낮추지 않고 실제 wire 형식의 fixture로
수리했다. 해당 compile/실패 실행을 성공 수에 합치지 않는다.

### 최종 소스·집계·제외

`target/session-authority-source.json`, `session-authority-seal.mjs --verify`: Rust/Cargo와 두 literal
JSON fixture **374파일**, SHA256
`2af41250c3a3b1b05604fd7c1be7363f6f20bd3816b88613785105b32d4e1d0f`.
문서·compiler/registry package 원문은 이 봉인 밖이다. 이전373 봉인 파일은 덮어쓰지 않았다.

- `target/session-authority-workspace.log`: `cargo test --workspace --no-fail-fast` 최종 exit0,
  **57 summaries, 1090 passed / 0 failed / 7 ignored**. 이전1076 + SESSION5 + release5 + actual loop1
  + OUTER builder3. fixture 이관87개는 새 시험 수로 더하지 않는다.
- `target/session-authority-js.log`: 하네스57 + build wiring6 = **63 passed / 0 failed**, skipped0.
- 소스 동결 후 문서 변경은 별도 lint/cargo 문서 gate로 확인한다. 검사하는 문자열·색인과 실행 의미는 다르다.

actual Worker::run은 post-LOAD fake native Frame을 사용했다. 실제 모델 load·CPU/CUDA conformance·
네트워크 인증·GPU/VRAM-only/RAM 오프로딩·성능/다중 컴퓨터·배포·commit/push는 이번 slice에서 실행하지 않았다.
target 자료는 로컬 보존이지 공개 불변 evidence bundle이 아니다.

scalar RELEASED의 요청 집합, 다중 OUTER의 해제 라우팅·관측, commit 후 notification intent 보존,
새 OUTER/Worker 재시작 freshness와 fleet 전체 topology 합의는 미해결이다. SESSION의 source 대조를
해제 완료 전체나 인증으로 승격하지 않는다. 이후 행동과 단계 상태는 최신 로드맵만 소유한다.

읽기 전용 별도 감사: `event_runtime/transport.rs::serve`와 `EventBroker::dispatch`는 peer 신원과
envelope source의 인증 결속을 하지 않는다. SESSION 최초 설치는 설정자 권한을 결속하지 않고,
SESSION_READY는 전체 ordered topology를 attest하지 않는다. 이 세 가지는 code-only 열린 경계이며
위 source/target 반례의 효과 시험이나 네트워크 침입 재현 결과가 아니다. 책임/제약은 격리 계약을 따른다.

최종 문서 gate는 추적73/전체79 clean, 자체12/12, cargo docs_lint1/1이다. 편집 도중 혼합 EOL을
검출한 첫 lint 실패는 같은 여섯 문서의 CRLF 정규화로 수리했다. private-header81 clean/common0 header·
5 source, 현재 pin0eadefebd manifest valid24를 다시 확인했다. `target/session-authority-clippy.log`는
staged/event-drive all-targets exit0이나 staged lib-test26(13 duplicates) 등 경고는 남는다.
이것은 새로운 pin replay나 native 의미 호환 증명이 아니다. 문서 갱신 뒤에도 source374 봉인 동일을 확인한다.

## 2026-09-07 후속 — 요청별 해제 증명과 소유자 통지

### 소스와 실제 실행 경계

HEAD는 a9e1967fc59dffa6c2e458f1b91f916b1df826c1이며 후속 미커밋 작업 트리를 검증했다. 새로운
`completion.rs`와 `worker/release.rs`/`effects.rs`/`node/state.rs`, OUTER `run/inference.rs`/
`release_ledger.rs`가 실제 소비 경로다. wire 의미는 배치 계약만 소유한다. native C++/llama/backend와
P4 중립 프로토콜을 이번 slice에서 바꾸지 않았다. 기존 SESSION source 권위 시험도 유지한다.

actual `Worker::run`의 post-LOAD fake native에서 정상 단일 OUTER와 서로 다른 OUTER의 A/B를 한
physical terminal에 섞었다. 기대값은 원본 PREFILL의 송신 ID/전체 route, head가 발행한 PHYSICAL의
slot/incarnation, head의 RELEASE 명령 및 fake native가 받은 P4ID 원문에서 각각 대조한다. 수신
receipt를 복사하여 자기 기대값으로 쓰지 않는다. 2/4-stage에서 OUTPUT/receipt의 소유자·correlation/
deadline과 terminal 뒤 receipt 순서·이벤트 유일성·head sequence 진행을 확인한다. 첫/후속 Full은
실제 worker 루프와 mailbox를 통과하며, 원래 native release 1회·정상 토큰/text/position/stop도 유지한다.
actual loop는 **16/16**(기존13 + 새3)이다. 실제 네트워크·모델·토큰화·GPU가 실행된 것은 아니다.

직접 `Worker::released` + 실제 completion mailbox의 **7/7**은 첫/1건 뒤 Closed, MAX/MAX-1 이벤트
번호, Full 회복, 같은 OUTER의 다른 correlation/deadline, 뒤쪽 잘못된 provenance를 검사한다.
잘못된 원본/ReplySpec의 **8가지 유효 값 불일치 × A/B 양순서**에서 슬롯·pending·효과/native 호출을
보존한다. 직접 handler/method의 범위와 위 actual run을 합쳐 하나의 전체 경로라고 하지 않는다.
별도 DTO4와 실제 PREFILL source/target 소비1을 더했으며 SESSION 전체6/6이다.

새 `head-approved-output-v2.json`은 ordinary2·checkpoint Replay2/4의 **실제 PREFILL5·OUTPUT15·
receipt5**를 함께 캡처했다. 구 `head-approved-output-v1.json`은 보존했고 legacy semantic projection은
기존 전체 토큰/text/position/stop/route가 같음을 검사한다. 새 projection은 신규 필드를 포함한 전체
body를 비교한다. capture 모드가 검사를 건너뛰지 않으므로 첫 이관은 빈 새 fixture에서 의도적으로 RED였다.
실제 소비는 bounded duplex의 EventWire→drive→acceptance를 통과한다. OUTER 실제 송신의 envelope와
캡처 PREFILL을 대조하고 원문 OUTPUT/receipt bytes를 사후 수정하지 않는다. 다만 worker 입력은 explicit
tokens, OUTER 입력은 prompt이므로 **tokenization 동등성은 아니다**. 캡처 주변의 관측은 synthetic임을
유지하며 그것으로 다중 OUTER 실제 관측 생산의 미해결을 감추지 않는다.

OUTER 전체 **67/67**은 기존50 + 새17이다. actual drive budget/boundary/member27, 순수 release
원장2, 실제 send_wave 실패/등록순서1, 공유 캡처 소비5가 포함된다. final artifact의 member/승인 bool
대조는 보존 결과의 일관성 검사이지 원시 receipt의 독립 인증/재생이 아니다.

### 반례와 독립 변이

| 자료 | 고정 시험과 결과 | 해석 한계 |
| --- | --- | --- |
| `target/release-notification-provenance-red-20260907/` | 수정 전 유효한 다른 ingress를 수용: 실제 fresh compile 뒤 0/1 RED. 원본·시험·EXE 보존 | 첫 잘못된 값에서 멈춘 RED; 8축 각각의 독립 구코드 실행은 아님 |
| `target/release-receipt-producer-mutations-20260907/verification.json` | 기준13/0 → provenance 제거12/1 → 복원13/0 → 실패 front 삭제10/3 → 복원13/0 → PREFILL source/target 제거12/1 → 복원13/0 | notification7+SESSION6. 메서드/handler 증명; actual run/native KV 전체가 아님 |
| `target/release-notification-producer-mutations-20260907-01/verification.md` | actual run16/0 → 소유 그룹 하나로 축소14/2 → 복원16/0 → ACK route 통지14/2 → 복원16/0 → terminal operation 변조7/9 → 복원16/0 | route 변이는 첫 2-stage/첫 Full 하위 사례에서 실패. 4-stage/후속 Full의 개별 변이 실행을 추가 주장하지 않음 |
| `target/outer-release-membership-mutations-20260907-01/verification.txt` | 67/0 → terminal member 대조 제거61/6 → 중복 재계수64/3 → 부분 commit66/1 → OUTPUT attempt 대조 제거65/2 → send 선행66/1 → 정확 복원67/0 | 부분 commit의 상태 불변 반증1개는 순수 원장이다. 오류 뒤 actual drive의 private 원장을 관찰한 것은 아님 |

변이는 전부 독립 복사본에서 실시했다. 각 arm 실제 Compiling, source before/after, EXE·raw log 해시를
남겼고 단순 copied mtime나 원래 프로세스의 공유 EXE 재사용을 fresh 결과로 세지 않았다. source closure는
각각 247입력/원본243(+격리 workspace/lock), 379입력, 167입력이며 정확 복원/원본 불변을 확인했다.
축소 workspace의 dependency closure와 전체 workspace 시험을 구분한다. 각 변이는 baseline에서 출발하며
기대값·시험 본문을 낮추지 않았다. 잘못된 구현이 실패하는 것과 guard의 모든 predicate를 하나씩 변이한
것은 다르다. 현재 grouped provenance/PREFILL 변이는 첫 부정에서 실패한다.

최종 복원 EXE SHA256:

- method/handler: `0c169ad85cd6ae5cb17ef71953f839eb93d6cd840a758b3ea07a9e8f8fe55525`
- actual worker: `E100B55F026F25E9A1D2EAA644811C27B18186A2EF03449EADF756E261A0B3F3`
- actual OUTER: `BDF5237888734C843E3FB194EE53DB25AAF463210B14025FB6D77D1FA77D0611`

### 실패한 중간 실행과 집계

- old fixture source가 return_route와 다르던 정상 PREFILL은 새 검사에서 거부되어 adapter 초기 전체
  **354/23**이었다. `stage_tests`20·`incarnation_tests`3의 정상 입력 생성만 고쳤고 음성 입력이나
  native/body/slot/KV 단언은 바꾸지 않았다. 기존23개를 새 시험 수에 더하지 않는다.
- actual Full 시험의 오래된 `completion_queue_full:waiting` snapshot이 다음 포화를 증명하지 못했다.
  genuine ACK 전 recv 대기·mailbox Empty를 확인한 지점의 test-only 관찰 baseline 뒤 새 Full을 기다리게
  했다. 요청/원장/native 상태를 바꾼 것은 아니다. 초기 실패 로그를 보존했다.
- 새 캡처를 쓰는 파일을 병렬 작성하는 도중 module 부재/함수 인자 미이관/Envelope 직렬화 시험 오류로
  compile이 실패한 시도는 PASS가 아니다. Envelope를 테스트 편의로 P4 Serialize 타입으로 바꾸지 않았다.
- 공유 빌드 디렉터리에서 다른 작업이 EXE를 재링크하여 한 producer 실행 직후 EXE 해시를 확정할 수
  없었던 시도는 별도 기록했다. 위 독립 최종 실행의 EXE만 정확히 결속했다.

`target/release-receipt-source.json`과 `release-receipt-seal.mjs --verify`: Rust/Cargo와 **세 literal
JSON fixture 379파일**, SHA256 `604a008d2e8b66bcf746494802fbe2382d56b133c1451271da7de84615a62dc2`.
이전 source374 파일을 덮어쓰지 않았다. 문서·JS·compiler/registry package 원문은 이 봉인 밖이다.

- `target/release-receipt-workspace.log`: 전체 `cargo test --workspace --no-fail-fast` 최종 exit0,
  **57 summaries / 1122 passed / 0 failed / 7 ignored**. 1090 + adapter15 + event-drive17이다.
- `target/release-receipt-js-final.log`: 하네스57+build wiring6+event config3+four-node config9 =
  **75/0**, skipped0. 넓힌 범위의 첫 실행66/1은 옛 `apps/p4/` import 경로 부재였다. 실제 같은 저장소
  모듈을 찾고 test import 한 줄만 수정했다. 기존 선택 범위63의 결과를 과거75 통과였다고 바꾸지 않는다.
- `target/release-receipt-clippy.log`: staged/event-drive all-targets exit0. staged lib14/lib-test26
  (13 duplicates), event-drive bin6/test8(6 duplicates) 등 경고가 남는다. warning-free 주장이 아니다.
- private-header81 clean/common0 header·5 source, 현재 pin0eadefebd manifest valid24 재확인.
  이는 native 빌드·새 pin replay·CPU/CUDA 의미 conformance가 아니다.

새 root source로 C++/CUDA/model/GPU/원격 네트워크/배포/commit/push는 실행하지 않았다. target 증거는
로컬 보존 자료이며 공개 불변 bundle이 아니다. VRAM-only 충분성 및 이후 RAM 오프로딩은 실기 게이트로
남는다. 현재 정상 종료의 증명을 출력 없는 Cancel·다중 OUTER 관측·restart freshness·재연결/내구 전달·
완전한 bounded queue/credit·graceful drain으로 확대하지 않는다. 단계 상태와 다음 첫 행동은 로드맵만 소유한다.

최종 문서 검수에서 캡처의 ‘전체 필드’ 표현을 payload 전체로 좁혔다. envelope volatile3개의 semantic
equality 제외와 별도 검사, 원본 PREFILL exact 대조를 구별한 것이며 fixture/기대값을 낮춘 수정이 아니다.
문서 gate는 추적73/전체79 clean, 자체12/12, cargo docs_lint1/1이다. 문서와 JS import의 EOL 정규화 뒤
JS75/0을 재실행했고 Rust379 source seal 동일을 다시 확인했다.

## 2026-09-07 후속 — 보고 지표 분리와 관측 완결 감사

### 이번에 실행한 범위

`test/benchmarks/p4-4node/run.mjs::buildReport`는 실제 main과 모델 없는 보고 시험이 함께 사용하는
artifact 소비 경로다. 기존 inline report 조립을 옮기고 import 시 main이 자원을 시작하지 않도록
했다. 직접 CLI의 무인자 usage 실패도 별도 시험한다. 모델/worker/native/network 실행을 이 시험이
통과한다는 뜻은 아니다. 같은 HEAD의 미커밋 트리이며 이번 생산 변경은 run.mjs, 새 시험은
report-metrics.test.mjs다. Rust/native/C++/P4 중립 코어는 이번 slice에서 변경하지 않았다.

report metrics v2는 Rust acceptance가 보존 OUTPUT을 세는 규칙에 맞췄다. 이전 decode 행 속도와
승인 생성 토큰 속도를 구분하며 정확한 필드/빈 EOS/분모/legacy 이관은 하네스 README가 기술한다.
Prefill+Verify/Replay도 혼합으로 센다. 물리 rows/fill/pacing은 원래 값이고 이전 보고서 파일은 고치지
않았다. 이 지표는 H1 품질 승인이나 H4의 마지막 terminal 시간창/유효 TPS가 아니다. Rust 요청별
logical_generation_tps와 stage span의 계산·소유권·완결은 미이관이다.

### 구코드 반례·변이

`target/report-token-metrics-20260907-01/verification.txt`와 같은 디렉터리의 원문/소스/metadata:

| 실행 | 결과 | 범위 |
| --- | --- | --- |
| `01-before-fix.stdout.log` | 0 passed / 2 failed, exit1 | 원래 수식 그대로 report 조립 소비: 혼합0≠2, speculative 출력3/2초인데 생성TPS0≠1.5 |
| `03-complete-tests.stdout.log` | 11/0, exit0 | EOS/빈 조각/첫 토큰/분모/물리 폭/증거 부재/CLI를 포함한 최종 원본 회귀 |
| `04-copy-baseline.stdout.log` | 11/0, exit0 | 독립 복사본과 실제 import 폐쇄10파일 |
| `05-copy-decode-numerator-mutant.stdout.log` | 6/5, exit1 | production 생성 분자만 generated→decode로 되돌림, 시험 bytes 그대로 |
| `06-copy-restored.stdout.log` | 11/0, exit0 | 복사본만 정확 복원, 원본과10/10 일치 |

구코드 RED 전에는 report 조립의 함수 추출/import guard만 먼저 있었으며 잘못된 수식은 그대로였다.
중간8개 GREEN을 최종11개 집계로 보고하지 않는다. 변이는 JavaScript를 매번 새 Node 프로세스에서
해석한 증거이지 컴파일/EXE 재빌드가 아니다. 세 copy 실행 모두 입력10파일의 before/after와 원본
before/after 변화0을 확인했다. 원본 checkout/reset은 사용하지 않았다. 이번에는 생성 분자 변이1개만
실행했고 혼합 predicate를 별도로 제거한 독립 변이까지 했다고 주장하지 않는다.

- 최종 run.mjs SHA256: `A7685DAB587992F18DDF6C7CDAD5137E566562041297B9B9720CFE8D2E51837E`
- 최종 시험 SHA256: `E7B71CD85E8179C556721BE0D31915630D993BB74FD1EE5BF9DE31F7F9BFADAB`
- verification.txt SHA256: `FAC6E72AE330673680AFA0584CFDF02BA466ECDD9B96648B13FD3B9D5993E1CD`
- Node v26.4.0 SHA256: `3193D7F751B8A07BD4ACC70E81946AE9C6EFDEE83E07AD1C8D0E4089DF7C5CEF`

### 최종 재실행·봉인

- `target/report-metrics-workspace.log`: `cargo test --workspace --no-fail-fast` 최종 exit0,
  **57 summaries /1122 passed /0 failed /7 ignored**. 새 Rust 시험 증가는 없다. 이전 Rust379 입력
  `release-receipt-seal.mjs --verify`의 SHA `604a008d2e8b66bcf746494802fbe2382d56b133c1451271da7de84615a62dc2`와 같다.
- `target/report-metrics-js-final.log`: 하네스68+build wiring6+event config3+four-node config9 =
  **86/0**, skipped0. 기존75에 새 report11을 더한 선택 범위이며 저장소 전체 JavaScript 시험이라는 뜻은 아니다.
- `target/report-metrics-source.json`: 하네스 .mjs와 선택 build/config 입력 **25파일**,
  SHA `41dbff05c72d143191348766d6802924611e36fdc6f1006ec9a5240f1d0a26b8`. Node/toolchain/OS/모델의
  hermetic 봉인은 아니다. 첫 봉인 도구 시도는 잘못 적은 event-config 경로 때문에 실패했고 실제
  p4-event-gate/config 경로로 고친 뒤 위 봉인을 만들었다. 그 실패를 시험 GREEN에 포함하지 않는다.
- 문서 tracked73/all79 clean, 자체12/12. 최종 문서 EOL 정리 후 cargo docs_lint1/1도 별도 재실행했다.
  문자열 gate가 미구현 관측 계약의 의미를 증명하지 않는다.

### 관측 경계 추가 감사 — 구현하지 않은 것

`observe.rs::emit_batch_observation`의 전체 owner 복사, `emit_stage_span`의 첫 owner 송신과
`inference.rs::drive`의 terminal/receipt 직후 종료를 다시 읽었다. 일부 관측/span을 한 묶음 통째로
잃으면 받은 execution에만 coverage를 요구하는 검사로는 기대 집합 자체를 알 수 없다. count/phase
합만 유지한 다른 execution/위치 대체도 별도 문제다. 이것은 **code-only 감사**이며 새 실행 RED가 아니다.
발행 승인 시의 요청별 고정 크기 증거와 terminal 결속, 소유 투영·지연 수집·실패 원자성·비용 반례는
배치 계약/검증 규약에 목표로 기록했다. 아직 SHA 의존성·새 OUTPUT·관측 wire나 완료 장벽을 구현하지 않았다.

VRAM-only→더 큰 RAM 오프로딩의 기존 사용자 순서도 유지했다. H0는 의도적인 CPU 계산이라도
실제 모델 계산/가중치/KV의 host 의존이면 VRAM-only가 아님을 명확히 했다. CPU tokenizer/sampler,
staging, mmap과 비소유 레이어 표시를 모델 offload로 오인하지 않는다. 기존 runner의 actual RAM
layout/budget은 아직 미구현이며 이번에 모델 inventory/적재·GPU·오프로딩·원격 배포·commit/push는 없다.
현재 단계와 다음 첫 행동은 로드맵의 최신 관측 감사 절만 소유한다.

## 2026-09-07 후속 — 실제 승인된 발행의 내부 witness

### 첫 봉인과 생산 경로

같은 HEAD a9e1967fc의 미커밋 작업 트리에서 `issue_witness.rs`와 `accept_prepared_issue`에 내부
발행 증거를 연결했다. 정확한 encoding·계층 귀속·제외 범위는 배치 계약의 내부 issued-work v1 절을
따른다. P4 중립 코어·native stage ABI·llama/backend는 이번 생산 변경에 포함되지 않는다. witness는
승인된 멤버십 증거이지 토큰의 의미·내구성·서명·관측 완결 또는 KV 정지점 증명이 아니다.

첫 원본 전체 실행 `target/issue-witness-workspace.log`는 최종 exit0, **57 summaries /1146 passed /
0 failed /7 ignored**였다. 이전1122에 primitive12+실제 L1 API9+실제 worker loop3을 더한 값이다.
`target/issue-witness-source.json`의 입력385파일 SHA는
`d8189658968bae94ec4181a33dba4eeb8b95cd8b8a9ce27fd1173cb3d000d94d`다. Rust/Cargo·기존3 JSON fixture·
독립 vector JSON과 생성 스크립트를 포함한다. compiler/registry source·문서·모델은 봉인 밖이다.
아래 후속 제출 사전검사 수정 이전의 소스이며 최종 소스의 전체 통과로 재사용하지 않는다.

primitive는 `generate_vectors.mjs`가 Rust 없이 선언한 입력5종/chain13단계의 bytes/digest와 대조한다.
L1 시험은 실제 준비/시작/승인 API와 committed·prepared·flight snapshot을 통과한다. actual run 시험은
post-LOAD fake native, 실제 Frame/Capsule/EventWire 및 Worker::run을 통과한다. ordinary 2/4/8-stage,
다중 OUTER 2/4-stage, 실제 completion Full, 새 EventID의 중복 terminal, 두 번째 native 실패/ID 재사용을
포함한다. native가 실제로 KV를 만진 뒤 실패한 경우에도 마지막 승인 witness가 보존됨을 검사했다.
독립 digest 외에 기존 output token/text/position/stop·KV/release oracle도 유지한다.

### 원본을 건드리지 않은 변이

`target/issued-work-mutations-20260907-01/verification.json`과 각 arm의 원문/manifest/소스/EXE에 보존했다.
시험 선택은 primitive12+L1 API9+actual loop19 = **40개**다. 19개가 모두 신규 시험이라는 뜻은 아니다.

| arm | 결과 | 변경과 검출 |
| --- | --- | --- |
| baseline | 40/0, exit0 | 원본385파일과 동일한 독립 복사본 |
| omitted-witness | 35/5, exit101 | 승인 후 witness 설치만 None으로. L1 2·actual worker 3 실패 |
| restored-omission | 40/0, exit0 | 복사본을 원본 bytes로 복원 |
| premature-commit | 35/5, exit101 | 후순위 검증/flight 등록 전에 현재 요청 witness를 먼저 변경. 거부 불변 시험 실패 |
| missing-execution-identity | 33/7, exit101 | 실행 ID를 hash 입력에서만 제거. 독립 literal 및 실제 승인/worker 비교 실패 |
| restored-final | 40/0, exit0 | 두 생산 파일 모두 원본385파일과 동일 |

모든 arm은 실제 staged crate의 **Compiling→새 EXE**를 확인했고 입력 before/after 변경0,
원본 before/after 변경0이었다. 복원 시에도 copy의 crate-root mtime만 갱신하여 이전 EXE를 재사용하지
못하게 했다. 원본 checkout/reset·원본 생산 변이·골든 수정은 없다. 각 arm의 정확한 EXE/hash는 manifest에
보존하며 실행 뒤 EXE를 복사해 다음 arm이 덮어쓰지 못하게 했다. 다른 시점의 원본 source/EXE를 같은
실행이라 주장하지 않는다. 첫 소스 봉인은 마지막 변이 뒤에도 동일하게 재확인했다.

이 증거는 OUTPUT wire/actual OUTER의 전체 관측 누락 검출을 통과한 것이 아니다. 당시 getters는
test에서만 소비되어 생산 dead_code 경고가 남았다. 경고를 숨기거나 하위 wire 완료를 주장하지 않았다.

### 추가 코드 감사가 드러낸 입구 불일치

P4 envelope/wire는 NUL을 포함한 submission event ID/OUTER channel·host를 허용하지만, 승인 OUTPUT와
내부 witness는 허용하지 않는다. `Worker::prefill`에서 이를 확인하지 않으면 native 뒤의 첫 승인에서
거부될 수 있다. 독립 코드 감사로 찾은 admission/하위 계약 불일치이며, hash 증거가 네트워크 인증을
제공하지 않는다는 제한과는 별개다. 후속 실제 반례·수정과 최종 소스 재실행은 아래 기록으로 구분한다.

### 제출 사전검사 RED → GREEN

`target/submission-identity-red-20260907-01/verification.md` 및 `red.log`의 최초 원본 실행은
**1 passed /6 failed**였다. NUL event ID/channel/ingress host 각각에 explicit tokens와 prompt를
사용했다. 원본 EventWire 왕복 후 head native logical 호출1, KV 쓰기, slot0/incarnation1과 거부될
session_key가 이미 생겼고 Uncertain·worker shutdown으로 끝났다. prompt3종은 Tokenize도1이었다.
RED raw SHA `FF5AB0D25B3AD80BF99C247C120EAFF1A18541EAF15BAE930C1E62D9C92DA08A`, 보존 EXE SHA
`68E666889636069371FD7CD71B6C94EC550A61EE0A77B29EE617208A6E05E7A6`다. 이 실행의4개 관심 파일
전후 해시와 정확한 실행파일은 agent 증거에 있고, 전체 workspace 입력 봉인이라고 확대하지 않는다.

공통 `validate_submission_identity`를 PREFILL의 원본 source/target 검사 직후 호출하게 했다.
정산용 authority 검사도 같은 공통 형식을 재사용한다. 새 slot/incarnation 또는 witness를 가짜 값으로
먼저 만들지 않는다. canonical bytes/digest 골든은 변경하지 않았다. 수정 후 bad 요청은 Tokenize·
native·원장 효과 없이 정확한 adapter 오류를 반환한다. 같은 request_id를 다른 session_key의 정상
제출로 재시도하여 slot0/incarnation1, 기존 exact output/KV/해제를 완주한다. 정상 Unicode와 별도의
correlation 양성도 유지한다. 시험 작성 중 정상 재제출의 OUTER sequence를2로 명시하고, 새 witness
fixture의 불필요한 clone 한 곳을 제거한 뒤 아래 최종 전체 소스를 다시 봉인했다.

### 최종 소스의 독립 변이·전체 재실행

`target/issue-witness-final-source.json`은 **386파일**,
SHA `ac955f7e2ce1ca7f5c73748ea7835a96a10730f430850d57faf3e46ec1cb02a2`다. 첫385 봉인을 덮어쓰지 않았다.
`target/issued-work-mutations-20260907-02/verification.json`의 최종 복사본에서 전부 다시 컴파일했다.
선택된 primitive12+L1 API9+actual loop26 = **47개**이며 시험/골든 bytes는 모든 arm에서 고정이다.

| 최종 arm | 결과 | 검출 |
| --- | --- | --- |
| baseline | 47/0, exit0 | 원본386파일과 동일 |
| omitted-witness | 42/5, exit101 | L1 2 + 실제 worker 3 |
| premature-commit | 42/5, exit101 | 후순위 오류/overflow/등록 실패의 상태 보존 |
| missing-execution-identity | 40/7, exit101 | 골든·동일량 교체·실제 승인/worker digest |
| missing-submission-guard | 41/6, exit101 | 실제 PREFILL의 NUL6종이 native 뒤 실패로 회귀 |
| restored-final | 47/0, exit0 | copy386파일 원본과 동일, 다시 Compiling·새 EXE |

매 arm의 input before/after 및 원본 before/after 변화0, 실행 로그와 보존 EXE 해시를 검증했다.
최종 restored EXE SHA `dc9020d7ef5e117320bc9b50b712953f0fbbd7ab21876fe465b2228aa7416f86`.
이 EXE는 독립 복사본의 선택47 시험이고 원본 전체 workspace의 모든 테스트 바이너리를 대신하지 않는다.
공통 검사 전체를 제거하는 추가 변이는 하지 않았으며, prefill 소비 호출 제거와 입력 멤버십 변이를 구분한다.

- `target/issue-witness-final-workspace.log`: `cargo test --workspace --no-fail-fast` 최종 exit0,
  **57 summaries /1153 passed /0 failed /7 ignored**. 이전1122 + 내부 witness24 + 입구7이다.
- `target/issue-witness-js-final.log`: 하네스68+build wiring6+event config3+four-node config9 =
  **86/0**, skipped0. 입력25파일은 이전 `report-metrics-source.json`의
  `41dbff05c72d143191348766d6802924611e36fdc6f1006ec9a5240f1d0a26b8`와 동일하다.
- `target/issue-witness-vectors-verified.json`: Node 독립 generator 출력과 literal JSON의 구조가 동일,
  authority5/chain13이다. Node26.4.0 및 generator/literal 해시도 기록했다. Rust 결과로 골든을 재생성하지 않았다.
- `target/issue-witness-final-clippy.log`: staged/event-drive all-targets exit0. staged lib15/lib-test26
  (12 duplicates), drive bin6/test8(6 duplicates)의 경고가 남는다. witness getter4개의 생산 미사용
  경고는 OUTPUT 미이관 상태 그대로이며 warning-free 주장이 아니다.
- private-header81 clean/common0 header·5 source를 재확인했다. include 문자열 게이트이며 native
  재컴파일·link 폐쇄·upstream 의미 호환 또는 backend conformance 증명이 아니다.

최종 Rust386과 JS25 봉인은 변이 후에도 동일했다. 문서 편집은 그 코드 봉인 밖이다. 최종 문서 게이트는
추적73/전체79파일 clean, 자체 시험12/12이며 `cargo test -p p4-agent --test docs_lint`도 1/1이다.
원문은 `target/issue-witness-docs-{tracked,all,self}.log`와 `target/issue-witness-final-cargo-docs.log`에
보존했다. target 파일은 로컬 보존 증거이고 immutable 배포 bundle이 아니다. 새 소스로 C++/CUDA,
모델 적재·원격 네트워크·강한 GPU 웨이브·VRAM-only/RAM 오프로딩·deploy·commit/push는 실행하지 않았다.

### 여전히 열린 경계

내부 witness는 아직 OUTPUT로 나가지 않는다. late/missing 관측을 actual OUTER가 완결 대조하는
wire/소유 투영/effect fan-out도 미이관이며 이번 GREEN으로 승인하지 않는다. 과거 RequestState 전체
프롬프트 clone, 매 issue의 ReplySpec parse/authority 재해시·현재 행 정렬 비용은 별도로 남는다.
추가 읽기 전용 감사의 serialized ReplySpec>4096 사전검사 누락은 이번 NUL 반례와 다른 열린 표면이다.
그 최대 길이 반례를 실행했다고 하지 않는다. 다음 첫 행동과 이후 순서는 로드맵의 최신 절만 소유한다.

## 2026-09-07 후속 — 제출 문자열의 실제 byte 경계

앞 절의 code-only ReplySpec 크기 표면을 이번에 실제 Worker::run으로 재현하고 수정했다. HEAD는
`a9e1967fc59dffa6c2e458f1b91f916b1df826c1`이며 미커밋 작업 트리다. 원본 commit/push·원격 배포는 하지 않았다.
현재 입력 한도의 단독 소유는 배치 계약의 제출 경계이며, 다음 행동/단계 상태는 로드맵 최신 절을 따른다.

### 반례의 원인과 수정 범위

`worker.rs::prefill`은 ReplySpec/options의 한도를 확인하지 않고 session key를 기억하고 요청을
수용했다. 이후 `LogicalBatch::encode`가 InvalidRow를 반환하면 요청 거부가 아니라 worker 종료였다.
prompt 입력은 이미 Tokenize를 실행했다. 이번 반례에서는 logical/native KV 실행 전 encode가 실패하므로
KV가 쓰였다고 하지 않는다. 앞 NUL 반례의 native 후 Uncertain과는 다른 경로다.

공유 `capsule.rs::validate_reply_options`를 actual serialized ReplySpec과 원문 options에 적용한다.
Logical/Physical도 같은 검사를 유지하며 원래 4096-byte 한도와 다른 identity/position/speculative 검사를
바꾸지 않았다. PREFILL 호출은 record/remember_session_key·Tokenize·slot/incarnation 수용보다 앞이다.
options의 JSON 의미를 Rust에 재구현하거나 strip/재직렬화하지 않는다. generic P4·llama/backend 변경은 없다.

- `submission_limits.rs`: 12 libtest 안의 정상16회/초과8회. 정상 입력 4095/4096은 ASCII·escape·UTF-8와
  tokens/prompt를 포함하며 기존 exact token/text/position/stop·KV·release oracle로 완주한다.
- 초과 4097은 native/tokenize/기록·수용 전에 명시 오류1로 거부되고 같은 request ID/다른 session key를
  원래 worker에 재제출해 정상 완주한다. incarnation1/slot0을 오염하지 않는다.
- 독립 literal ReplySpec에서 escaped correlation 원문1982bytes가 reply4097이 되고, Unicode
  correlation3962bytes/1322scalars가 reply4097이 된다. 옵션4096을 가진 외부 JSON payload4276/4326bytes는
  유효하다. 외부 payload 크기·문자 수·trim 문자열을 검사하는 대체를 금지한다.
- `logical.rs::reply_and_options_wire_limits_preserve_exact_utf8_bytes`는 별도 codec 회귀1이다.
  실제 모델 tokenizer/options parser 또는 전역 admission 예산/다른 실패의 원자성 증명은 아니다.

### 수정 전 실행 봉인과 오류 정정

`target/submission-limits-20260907-01/red.log` 첫 실행은4P/8F였으나 동시에 root가 logical codec 회귀를
추가하여 source before/after가 달랐다. **비봉인 역사 관측**으로만 보존하고 수정 전 확정 증거로 쓰지 않았다.
편집을 멈춘 뒤 `sealed-red.log`로 실제 재컴파일5.44s 후 다시4P/8F를 재현했다. 관심7파일 before/after가
동일했다. 이는 workspace 전체 봉인이 아니라 해당 소스 목록의 봉인이다.

- sealed raw SHA256 `d23d1b10bdc9efc9dbee8cc9920593b473f8da206489f057e2238e69565a74e9`
- sealed RED EXE SHA256 `00c5fcc34c4826c23e41c7a14dfb86d94e800349ea67e811e3cdae5529a70a2b`
- 8초과 모두 `LLAMA_LOGICAL_BATCH_ENCODE_FAILED/InvalidRow`, requests1·session key 등록·incarnation1/
  next2·shutdown1. prompt4종은 Tokenize1, 모든 경우 logical/physical native0·KV0.
- 수정 후 `target/submission-limits-green.log`: actual compile5.58s, 입구12+codec1 =13/13.
  후속 전체 봉인/변이/전체 suite가 이 부분 실행을 보강한다.

### 최종 Rust 소스와 독립 변이

`node target/submission-limits-proof.mjs seal`이 Rust/Cargo·adapter fixture·독립 issue vector를
**387파일**로 복사하고 봉인했다. `target/submission-limits-mutations-20260907-01/source-baseline.json`의
SHA256은 `5a60dbd09c9af3f508b0294670e640170098774ca8d309646983055a54219c38`이다.
docs·C++·compiler/registry·모델·JS 하네스는 이 봉인에 포함되지 않는다. 이전386 witness 봉인은 보존했다.

각 arm은 `node target/submission-limits-proof.mjs run <arm>`으로 **별도 복사본**의 staged adapter 전체
421시험을 실행했다. original before/after와 copy input before/after를 대조하고, 실제 Compiling·새 EXE
mtime·raw log·입력 소스·EXE hash를 arm별 보존한다. 원본 checkout/reset이나 원본 변이는 하지 않았다.

| arm | 결과 | 반증 대상 |
| --- | --- | --- |
| baseline |421/0, exit0|봉인된 정상 소스|
| missing-ingress-guard |413/8, exit101|helper가 있어도 실제 PREFILL 호출을 제거하면 실패|
| late-ingress-guard |413/8, exit101|session-key 기록 뒤 검사는 명시 오류를 내도 재제출 상태를 오염|
| character-count |418/3, exit101|UTF-8 byte 대신 chars로 세는 잘못된 수리|
| exclusive-boundary |416/5, exit101|정확4096까지 거부하는 과잉 수리|
| restored-final |421/0, exit0|전체387 source bytes 원복 일치|

`node target/submission-limits-proof.mjs verify`로 보존 입력/log/EXE와 예상 exit를 검증했다.
`check-source`도 동일387을 확인했다. 시험·골든을 변이에 맞춰 변경하지 않았다.

### 실제 C++ codec 경계 — 모델 없음

원본 변경은 `runtime/physical_wire_test.cpp`뿐이다. 최종 test SHA256은
`395d6293f9bb90d4492b0f836a54c8f99a404fbc8304c9e458c984c3de3ed921`이다. 기존 단언을 유지하고
reply/options×ASCII/3-byte UTF-8×4095/4096/4097의 **12케이스**를 추가했다. LB/PB literal 입력은
production encoder 없이 만들며 실제 두 decoder·PB encoder의 한도와 exact bytes를 대조한다.
empty options도 허용한다. Rust와 C++의 동일 nominal 경계를 독립 시험한 것이며 같은 외부 fixture를
양쪽에 직접 재생한 cross-language 전체 conformance라고 하지 않는다.

MSVC14.44.35207로 test/physical_wire_decode/physical_wire_encode/physical_authority **4 TU**를
각 arm 새로 컴파일했다. `/std:c++17 /EHsc /O2 /UNDEBUG /MD`, public llama/ggml headers만 필요하며
llama library·모델·GPU는 사용하지 않았다. actual MSVC dependencies의 source/header152개와 compiler,
commands·EXE·before/after 입력을 보존했다. CMake target/full build/CTest를 실행한 것이 아니다.

- `target/native-row-string-boundaries-final-20260907/verification.{json,md}`: baseline0 → 별도복사본의
  거짓 assert3 → exact restore0. Release assert가 실제 실행됨을 확인한다. 최초 혼합 EOL 버전의
  `target/native-row-string-boundaries-20260907`은 별도 역사 자료이며 최종 source 증거로 대체하지 않았다.
- `target/native-row-string-limit-drift-20260907/verification.{json,md}`: baseline0 → 복사본의
  `kMaxString=4097`만 변경하여 실제 LB decoder boundary assert 실패(exit3221226505) → 원복0.
  첫 단언에서 멈추므로 이 변이가 PB 모든 분기의 독립 실패까지 증명한다고 하지 않는다.
- native drift 최종 restored EXE SHA256
  `b8b6c792835bc061a5d7dd38e1194f1d1cd9b39f3188836199a715cac12150b5`.

### 최종 집계와 범위

- `target/submission-limits-workspace.log`: `cargo test --workspace --no-fail-fast` 최종 exit0,
  **57 summaries /1166 passed /0 failed /7 ignored** = 이전1153+이번13. 실행 완료를 확인했다.
- `target/submission-limits-js.log`: 하네스68+build wiring6+event config3+four-node config9 = **86/0**, skipped0.
  입력25파일은 기존 JS 봉인 `41dbff05c72d143191348766d6802924611e36fdc6f1006ec9a5240f1d0a26b8`과 동일하다.
- `target/submission-limits-clippy.log`: staged/event-drive all-targets exit0. staged lib15/lib-test26,
  drive bin6/test8 등 기존 경고는 남아 있으며 warning-free 주장이 아니다.

문서 gate는 최종 문서 편집 뒤 추적73/전체79 clean, 자체12/12, cargo docs_lint1/1이다.
`target/submission-limits-docs-{tracked,all,self,cargo}.log`에 원문을 보존했다. private-header gate는
`target/submission-limits-private-headers.log`의81 clean/common0 header·5 source이며 include 문자열
검사 범위다. Rust387·JS25 봉인은 마지막에도 동일하다. target 자료는 로컬 보존 증거이지 장기 배포 bundle이 아니다.
OUTPUT v4/관측 wire·소유자 투영/완료 조건은 아직 그대로다. 모델 tokenizer·native options 의미·CUDA/
엔진 CTest·실제 강한 GPU 웨이브·VRAM-only/RAM 오프로딩·다중 컴퓨터·deploy/commit/push는 미실행이다.

## 2026-09-07 후속 — OUTPUT 발행 증거와 소유자별 관측 완결

HEAD `a9e1967fc59dffa6c2e458f1b91f916b1df826c1`의 미커밋 작업 트리를 대상으로 했다. 앞 절의 내부
witness와 입력 한도는 유지했다. 아래 구현/시험은 이 소스 범위에 한정하며 과거 GPU 실행을 재인증하지
않는다. 계약 정의는 배치 계약, 다음 단계/승격 상태는 로드맵 최신 기록이 소유한다.

### 구현된 소비 경계와 유지한 것

- `completion.rs::ApprovedOutputPayload`는 OUTPUT v5의 엄격한 flat DTO와 terminal 전용 proof를
  검증한다. `release.rs::tail`은 RequestState를 없애기 전에 실제 승인된 witness를 복사하고 원제출
  authority digest를 다시 대조한다. raw flight 등록이나 반환 capsule에서 witness를 만들지 않는다.
- `commands.rs`의 OBS v4/SPAN v4와 `observe.rs`는 full OuterEndpoint로 수신자를 묶는다.
  서로 다른 correlation은 같은 route에서 하나의 전달로 묶되 carrier는 실제 원 ReplySpec을 사용한다.
  자기 소유 요청의 정확한 행·phase·position·발행 index와 물리 전체 크기를 구분한다. downstream은
  가지지 않은 submission ID를 만들어 넣지 않는다. pre-native 수신자 검증과 Fresh-only span을 유지한다.
- head는 승인 전 관측 후보를 만들고 `accept_prepared_issue`가 설치한 실제 witness count/ordinal과
  승인 후 대조한다. 관측을 송신했다는 이유로 발행 count를 늘리지 않는다.
- `effects.rs::flush_effects`는 실제 completion mailbox에 Forward를 넣은 직후 시각을 한 번 고정하고
  후속 Telemetry intent를 보존한다. Full 복구·Closed·이벤트 번호 고갈을 검사했다. 이는 로컬 전달
  수용 시각이며 네트워크 도착 시각/내구 재전송/자동 fence 복구를 추가한 것이 아니다.
- `event-drive`의 실제 송신 경로가 `SubmittedAuthority`를 등록한다. OUTPUT은 budget과 해제 후보를
  검사하고 증거 후보를 승인한 뒤 요청 출력/해제 기대를 commit한다. 관측은 연속해진 요청별 issue를
  한 번씩 해시하고, terminal 증거와 configured stage별 execution membership/global 크기를 대조한다.
  도착 순서 역전과 정확한 body 재전달은 허용하며 충돌은 후보 전체를 반영하기 전에 거부한다.
- 실제 drive는 terminal+release 뒤에도 Missing이면 원래 overall deadline 안에서 기다린다. 다음
  wave의 예정 시각이 deadline보다 늦은 경우도 종료한다. `elapsed_ms`는 release 경계에 latch하고
  `telemetry_complete_elapsed_ms`를 따로 보존한다. 새로운 대기 시간을 기존 TPS 분모에 합치지 않는다.
- RequestArtifact는 실제 제출 authority와 terminal proof를 보존한다. stage index는 같은 실행의
  config.json과 함께 해석한다. offline `acceptance::evaluate`는 온라인 hash 검사를 독립 재실행하지
  않는다. Invalid/Missing EOF·timeout은 Err/nonzero이며 실패 partial artifact 저장은 아직 없다.

어댑터 소유 DTO/증거 타입만 공개했다. generic P4 event/NodeAdapter·native opcode/capsule·C++·llama/
backend는 이번 slice에서 바꾸지 않았다. 정책이 native private 타입을 갖게 하지 않았으며 SHA 증거를
인증이나 KV 정지점 증명으로 승격하지 않는다. 명시 Cancel/Drain·credit는 미구현이다.

### 실제 생산 캡처와 독립 기대량

기존 `head-approved-output-v1.json`(OUTPUT v3), `v2.json`(OUTPUT v4)은 불변이다. 새로운
`head-approved-output-v3.json`은 actual Worker::run에서 받은 EventWire 원문으로 만들었다.
기존 승인 필드/원제출/해제 및 token/text/position/stop oracle는 보존한다. 버전 간 비교에서 신규 proof와
명시 version만 투영하며, 현재 v5 full payload도 새 캡처와 비교한다. 실제 live event ID의 중복·증가와
causation 존재를 검사하되 cross-run event ID/causation 값·sequence·timing/pacing은 같다고 요구하지 않는다.
따라서 causation이 특정 terminal 사건 하나를 정확히 가리킨다는 별도 증명으로 확대하지 않는다.

| 실제 case | 원제출 | OUTPUT | receipt | OBS | SPAN |
| --- | ---: | ---: | ---: | ---: | ---: |
| ordinary-2 |1|5|1|6|12|
| checkpoint-2 |2|5|2|5|10|
| checkpoint-4 |2|5|2|5|20|
| mixed-owner-2 |2|2|2|2|4|
| mixed-owner-4 |2|2|2|2|8|
| mixed-consumer-2 |2|2|2|2|4|
| mixed-consumer-4 |2|2|2|2|8|
| 합계 |13|23|13|24|66|

`loop_tests/observation_contract.rs`의 기대량은 **수신한 OBS가 아니라 실제 발행 승인 callback**의
읽기 전용 기록이다. successful native 결과 capsule bytes·원제출 Event·승인 witness·설정 stage를
기록한 뒤 별도의 수신 wire와 대조한다. old exact token/KV/release와 함께 finish barrier를 적용한다.
일반 2/4/8-stage와 mixed-owner에는 기존 독립 Node literal digest도 유지한다. 모든 speculative
시나리오에서 hash 알고리즘을 독립 재구현했다는 뜻은 아니다.

기존 mixed-owner는 correlation과 request ID가 다른 양성을 그대로 둔다. 추가 mixed-consumer는
처음부터 실제 event-drive Sender 방식으로 제출해, 실제 drive 두 개에 각 route의 **전체 캡처 Event**를
전달한다. foreign owner를 소비 직전에 body에서 지워 맞추지 않는다. checkpoint의 partial request는
logical ordinal 1/2/3/5를 사용해 count4와 last ordinal5를 구분한다. 같은 OUTER의 checkpoint 요청들은
별도 issue라 전체 Event를 골라 소비할 수 있다; 같은 OUTER의 여러 요청을 하나의 physical에 함께 담는
경우까지 이 단일요청 캡처 소비가 증명한다고 하지 않는다(생산 측 다중 요청 시험은 별도다).

이 시험들은 scripted native tokens를 사용한다. event-drive가 실제 송신한 envelope/설정과 캡처를
대조하지만, 명시 token 입력과 다른 prompt 본문이 실제 모델에서 같게 토큰화된다는 증명은 아니다.
actual worker 자체의 command/capsule codecs와 local routing, actual drive의 EventWire 소비를
연결한 회귀이지 실제 broker/네트워크/native 모델을 하나로 연결한 전체 실기는 아니다.

### 검수에서 새로 잡힌 완료 오류

정상 첫 span에 execution999, `owned_requests=[]`, rows+1만 추가해도 기존 소비 후보는 Complete였다.
`SpanRows`는 unknown1을 보류했으나 `ExecutionEvidence::missing`이 empty-owned unknown을0으로
세어 완료 조건에서 빠졌다. actual drive는 정상 출력5·issue count6·완료1/해제1을 그대로 승인했다.

`target/owner-evidence-consumer-20260907-01/04-unresolved-global-red.log`에 실제0P/1F와 성공으로
잘못 반환한 전체 결과를 보존했다. 이제 수신된 global 실행은 head 크기를 확인하기 전 Missing이다.
보지 못한 B-only span을 요구하지 않으며, span이 먼저 와도 head가 뒤에 확인하는 양성을 유지했다.
그 양성의 추가 foreign 실행은 **명시 metamorphic fixture**이지 unmodified native 캡처라고 하지 않는다.
아래 봉인 소스의 old-missing 변이로 이 오류를 다시 재현한다.

다른 실제 소비 반례는 proof의 누락/revision/count/ordinal/authority/digest, 중간 position·attempt·
issue index 교체, stage별 누락, width/owner/timestamp 충돌, legacy v4, 다른 요청의 carrier다.
마지막 decode OBS만 삭제하는 경우와 **그 OBS 및 관련 모든 stage span을 함께 삭제하는 경우**를
나누었다. 후자는 받은 execution 집합까지 없어져도 terminal 기대량이 남아 Missing requests1/stage0이다.
정상 reorder·exact replay·late telemetry와 mixed-owner 양성을 함께 실행했다.

### 봉인과 실행 범위

`node target/observation-proof.mjs seal`이 workspace Rust/Cargo·공용 캡처·독립 issue vector를
**391파일**로 복사했다. `target/observation-migration-20260907-01/source-baseline.json`의 SHA256은
`af4249da69e7447caade7182807b9a9bc970ce01b39a700ffd12280e3e5c75dc`다. docs/C++/JS/compiler/registry/
모델은 이 봉인 밖이다. 각 arm은 원본 before/after와 독립 copy before/after, 실제 재컴파일·새 EXE
mtime·source bytes·raw log·EXE hash를 보존한다. 원본 checkout/reset 또는 원본 변이는 하지 않았다.

`target/observation-migration-workspace.log`의 `cargo test --workspace --no-fail-fast`는 최종 exit0,
**57 summaries /1196 passed /0 failed /7 ignored**다. 이전1166에서 staged adapter15, drive15가 늘었다.
구 adapter/deployment 시험을 숨기거나 골든/상한을 낮추지 않았다. 단계별 실행/변이 내역은 아래에 이어진다.

`target/observation-migration-js.log`: 하네스72+build wiring6+event config3+four-node config9 =
**90/0**, skipped0. 별도 JS25 입력의 SHA256은
`039b98f651c5154823a37f2c9b9ea018f8cc1cc5efd1b0d45c937882f3703b3d`다.
`target/observation-js-20260907-01/verification.json`은 독립 copy의 report baseline15/0 → conflicting
span 허용14/1 → telemetry를 TPS 분모로 사용14/1 → exact restore15/0을 보존한다. 이것은 report
조립의 검증이며 worker/모델/네트워크 증명이 아니다. fleet의 다른 OUTER projection을 합산하는 모드도 아니다.

clippy는 `target/observation-migration-clippy.log`에서 exit0지만 warning-free가 아니다. staged lib16/
test28, drive bin8/test9 등이 남는다. production에서 안 쓰는 `approve_output` convenience와 새 type
complexity 등도 포함되며 증거 봉인 뒤 소스를 몰래 청소하지 않았다. 정리는 다음 소스 변경 때 재검증한다.

최종 문서/독립 변이 확인은 아래 기록을 따른다. 모델/GPU/전체 CTest·원격 배포·강한 실제 웨이브·
VRAM-only/RAM 오프로딩·다중 컴퓨터·commit/push는 이번 slice에서 미실행이다. 실제 tokenizer/KV 수치,
교차 호스트 clock bound·내구 전달·Cancel/Drain·restart freshness·bounded RSS·최적 배치 성능은 남는다.

### 독립 복사본의 변이와 exact restore

`node target/observation-proof.mjs run <label> [crate] [filter]`의 crate는 staged adapter 또는
event-drive다. baseline/restore는 해당 crate 전체, 두 terminal/승인 full-loop arm은
`b2_two_stage_run_loop_completes_one_request_through_real_event_and_native_codecs`, 생산 관측 arm은
`observe_tests`, 소비 arm은 `output_contract` 필터를 사용했다. 필터 제외를 실행 통과에 더하지 않는다.

| arm | 실행 passed/failed | 판정 범위 |
| --- | ---: | --- |
| baseline-adapter |436/0|독립 복사본 전체 staged adapter|
| baseline-drive |82/0|독립 복사본 전체 실제 drive 시험|
| terminal-proof-missing |0/1|terminal 복사를 제거하면 실제 run이 정상 완료하지 못함|
| accepted-witness-missing |0/1|승인 hook의 읽기 전용 기대량이 witness 설치 누락 검출|
| accepted-witness-missing-observe |6/2|읽기 hook 없는 실제 head 경로도 `accepted observation has no witness`로 실패|
| span-recipient-loss |3/5|첫 recipient만 남기면 실제 fan-out/포화 보존 검사 실패|
| telemetry-suffix-loss |3/5|Forward 성공 뒤 관측 intent를 비우면 보존/전달 검사 실패|
| drive-stage-coverage-bypass |13/4|stage 증거 누락을 완료로 취급하면 실제 소비 회귀 실패|
| drive-digest-bypass |15/2|명시 conflict 검사를 제거하면 Invalid가 Missing으로 바뀌어 실패|
| drive-unresolved-global-bypass |16/1|empty-owner 미확인 실행을 무시하는 기존 반례 재현|
| restored-adapter |436/0|391개 source bytes 원복 뒤 새 컴파일/EXE|
| restored-drive |82/0|동일 원복 소스로 새 컴파일/EXE|

모든 RED의 cargo exit는101, baseline/restore는0이다. digest arm은 최종 equality가 별도로 남아
거짓 성공이 아니라 Missing/EOF로 끝났으며, 이를 “변이 후 손상 승인”으로 보고하지 않는다. 첫 승인
누락 arm은 test-only observer가 먼저 panic하므로, 별도 observe arm으로 실제 생산 guard도 확인했다.
여러 gate가 겹쳐 있다는 사실을 숨기거나 시험 쪽 안전장치를 같이 지워 반증하지 않았다.

`node target/observation-proof.mjs verify`는 정확히12개 arm·예상 exit·실제 실행 수·실패 수·원본
before/after·각 arm의 변경 파일1개·보존 input/log/EXE hash·최종 copy exact restore를 확인했다.
`verification.json`과 각 `manifest.json`/`output.log`/source/EXE가 같은 로컬 디렉터리에 있다.
현재 root source의 `check-source`도391/동일 SHA이며 JS25 입력도 별도로 대조했다. 새 proof runner는
target 로컬 검증 도구이며 정식 저장소 gate에 자동 연결됐다고 하지 않는다.

문서 최종 대조는 `target/observation-migration-final-gates.json`과 개별 raw log에 보존한다.
추적73/전체79 문서 clean, docs 자체12/12, cargo docs_lint1/1, private-header81 clean/common0 header·
5 source다. 문서 내 EOL은 일관되게 정규화했다. 이 문자열/색인 gate를 의미 정확성·native full/relink
격리의 증명으로 부르지 않는다. Rust391 및 JS25 최종 입력 hash는 위 봉인과 일치한다.

## 2026-09-07 후속 — completion Full의 정상 ACK 기아 RED와 중립 통지

### 실제 worker의 수정 전 반례

같은 HEAD의 후속 미커밋 트리다. 앞 절의 OUTPUT/관측 소스391 봉인은 수정하지 않았다. 그 입력에
`loop_tests/effect_backpressure.rs`, 모듈 등록, `released()` commit 뒤의 읽기 전용 test hook만 더한
392개 입력으로 actual Worker::run 반례를 먼저 컴파일·실행했다. 아직 effect pump 수정은 없다.

`cargo test -p p4-llamacpp-staged-adapter completion_full_cannot_starve_a_genuine_release_acknowledgement -- --nocapture`
실행은 **0 passed / 1 failed / 436 filtered**, exit101이었다. 필터 제외를 통과에 더하지 않았다.
빌드가 실제 staged adapter를 재컴파일했고, 마지막 신규 단언에 도달하기 전 기존 복구 양성은 통과했다.

| 관측 | 수정 전 실행 |
| --- | --- |
| A의 native 해제 | 두 stage 모두 완료, 정확한 최종 RELEASED 원문 보류 |
| B의 completion Full | capacity1, 실제 B OUTPUT이 유일 큐 슬롯 점유, 새로운 Full snapshot 확인 |
| A ACK의 입력 수용 | 실제 sync input queue의 try_send 성공, event wire 왕복 원문 동일 |
| 공간을 열기 전 A 정산 | 없음; pending/slot이 남음 |
| 공간 복구 후 | 출력2, 해제 영수증2; stage별 A/B native release 각1회 |
| 기존 oracle | token/text/position/stop·KV·release provenance·관측 완결·event ID 유일성 유지 |

실패 문자열은 `completion Full starved a genuine RELEASED already accepted at input`이다.
200ms는 새 진행 단언의 관찰 창이며 포화의 근거 자체가 아니다. 테스트는 공간을 복구하고 기존
정상 완료를 먼저 검사한 뒤 마지막 단언에서 실패한다. 실제 EventNode/broker/network·Cancel/Drain,
전체 순환망의 진행이나 CPU 사용률을 증명하지 않는다.

보존 위치는 `target/effect-backpressure-red-20260907-01/`의 source-before/after.json,
red.log, seal.txt, verification.md와 시험 EXE다. 입력 manifest의 SHA-256은 양쪽 모두
`cf97679e268e3938cd287c8dfedbda28f338e131259b06bfe2af55245788830f`, 실행파일은
`69c71e97b0494c7800ccfba8ff1a270697d9fee09d997511d4aa9380a90ec748`이다.
재검수에서 입력392의 빌드 중 변경0과 앞391 봉인 대비 변경3개를 대조했다. 이후 mailbox 편집은
다른 소스 상태이며 이 RED 실행에 소급하지 않는다. target은 로컬 증거이고 영속 배포 bundle이 아니다.

### 변경 전에 추가로 확인한 안전 경계

`emit.rs`는 호출마다 next_event를 소비하고 직접 LOAD/SESSION/UNLOAD/오류 전송도 수행한다.
`effects.rs`만 Pending으로 바꾸면 ID 재발급·별도 무상한 큐·fatal ERROR 유실을 만들 수 있다.
`tail()`의 pending 설치는 native Release/Settle 및 Forward보다 앞서므로, 양보하는 pump에서는
pending 존재만으로 조기 ACK를 승인해서는 안 된다. `validate_control_batch`의 읽기 검사는
같은 native command 내부의 비선점 전제도 갖는다. 이것들은 코드 감사이지 이번 RED가 실행한
별도 결함이라고 주장하지 않는다. 목표 계약은 배치 문서가, 시험 의무는 검증 규약이 소유한다.

중립 mailbox에서도 등록된 reader가 마지막 publisher 종료에 깨어나지 않는 것과 publish가
waker lock 안에서 callback을 호출하는 것을 별도 기존 API 시험으로 재현했다. 원본 구현에서
2개 시험 모두 실패했고 실제 재컴파일 로그는
`target/mailbox-capacity-notification-20260907-01/01-before-fix.log`에 보존한다.
이 두 반례와 additive capacity 통지는 actor의 ACK 기아 해결과 구분한다.

### 중립 mailbox 구현과 검증 경계

`node_adapter/mailbox.rs`에 `CompletionPublisher::capacity_listener`와 RAII 등록을 추가했다.
동시 등록은64개로 제한하며 Closed/Exhausted를 명시 반환한다. 이 숫자는 listener 자원의 상한이지
노드 수·이벤트·바이트·전체 RSS 상한이 아니다. 등록은 공간을 예약하지 않고, drain 또는 receiver
종료 후 다시 try_publish할 이유만 제공한다. 기본 completion capacity와 Full/Closed의 Event 반환
의미는 바꾸지 않았다. 이 API의 현재 소비자는 시험이며 staged worker에는 아직 연결하지 않았다.

마지막 sender는 실제 disconnect 뒤 reader를 깨우고, 마지막 receiver는 disconnect 뒤 capacity
waiter를 깨운다. 사용자 Waker의 clone/drop/wake는 내부 mutex 밖에서 실행한다. 수신 객체가
사라지거나 poll이 Ready로 끝났을 때 남은 reader 참조도 제거한다. 검수 중 후자의 수명 표면을
추가로 확인했고, receiver 종료 후 publisher가 reader를 붙잡는 경우를 0P/1F로 재현한 뒤 수정했다.
그 로그는 `02-reader-lifetime-red.log`다. caller callback은 짧고 nonblocking/nonpanicking이어야
하며 panic은 숨기지 않는다. 내부 mutex가 poison되지 않는 시험을 전체 panic 복구/이벤트 전달
보장으로 확대하지 않는다.

1차 동결 세 파일로 실행한 `04-final-green.log`는 p4-adapter **61 passed /0 failed**다(기존47+새14).
새 회귀는 close 순서, buffered Event와 sender clone 수명, 등록/재검사 경쟁, 다중 drainer의
정확한 Event 전달, listener 상한/해제/ID 고갈, 재진입 자기 해제/재등록, reader 참조 해제,
Waker clone/drop/wake의 잠금 경계를 검사한다. 무기한 Barrier 대신 5초 제한 rendezvous와
join 완료 watchdog을 사용한다. 이것은 실기 latency·전체 스케줄링 결정성·worker ACK 처리 보장이 아니다.

그 1차 동결 구현/시험/재수출 SHA-256은 다음과 같다.

| 파일 | SHA-256 |
| --- | --- |
| `node_adapter/mailbox.rs` | `09a0a2b870d19311c29d911fcdc249e7216cc7d8563f7f0f57610da758bd1c2f` |
| `node_adapter/mailbox_tests.rs` | `6ae33daa0488b540de68885b36d3638a7c15ae64066a2cf439d438d0991bb830` |
| `node_adapter/mod.rs` | `16d2f658244ce0e6b929bf6ab570d3e03cde42b0d9003af0f12a9d2cbadbdc15` |

세 경로의 root는 `layers/adapters/adapter/src/`다. protocol envelope·NodeAdapter trait·llama/native
wire는 바꾸지 않았다. 구성요소 README에서 현재 event 경계와 과거 Work/hop 설명도 분리하고,
Cargo.toml과 반대였던 “p4-protocol 의존 없음” 서술을 실제 중립 의존성으로 정정했다.

그 상태의 원본 전체 실행은 `target/capacity-slice-20260907-01/`에 보존했다. 입력393의 SHA는
`6d77de2ab50560cd4c9cd3d12801d1b37b78a9fe21f1625b5932660c27da9ff3`이며 실행 전후 일치했다.
57개 summary 합계는 **1210 passed /1 failed /7 ignored**, cargo exit101이다. 실패는 위 actor
ACK 진행 시험 단1개다. proof runner도 suite_passed=false로 기록하며 이 집계를 green으로 바꾸지 않는다.

변이 준비 중 reentrant test callback이 정리 때 이미 사라진 mailbox를 unwrap하면 두 번째 panic으로
원래 실패를 가릴 수 있음을 확인했다. 전체 실행을 끝낼 때까지 원본을 동결한 뒤 그 시험의 Weak
upgrade만 정리 안전 guard로 바꿨다. 생산코드·핵심 단언은 무변경이다. `06-teardown-safe-green.log`의
61P/0F 이후 최종 시험 파일 SHA는
`64ac910f32097dbd59165fb91a8b5b1261425fa4a4215005c08d671e7c47b9d4`이고 나머지 두 SHA는 같다.
이 후속 상태와 첫 동결 상태를 같은 source로 합치지 않는다.

### 최종 원본 전체 집계

시험 정리 guard를 포함한 **최종393 입력**을 `target/capacity-slice-20260907-02/source/`에 보존했다.
manifest의 SHA-256은 `87e1a42d9071865b2f25850e521d76bebd472a178e3e540131f7416845a36c92`이고,
전체 실행 전후의 파일 집합·각 바이트 hash가 일치했다. 이 봉인은 Rust/Cargo/명시된 모델 없는 fixture
범위다. 문서·third-party registry/toolchain·native·모델·JS 전체 build provenance가 아니다.

`cargo test --workspace --no-fail-fast --locked`는 최종 종료까지 실행해 **57 summaries,
1210 passed /1 failed /7 ignored**, cargo exit101이었다. 실패는
`completion_full_cannot_starve_a_genuine_release_acknowledgement` 단1개다. 새14개의 중립 시험을
더하면서 actor의 필수 RED는 정상 실행에 남겼다. 런타임 skip/ignore/feature로 숨기거나 현재 대기를
정상 기대값으로 바꾸지 않았다. 이것은 **전체 suite 실패**이지 '알려진 실패를 제외하면 제품 승인'이 아니다.

`workspace.log`의 SHA-256은 `ba3b211a257dfcf83b3b770c69d22e26a6e8e961ae963a2b4662ef4d1073c9d1`이며
`workspace-result.json`에 실제 cargo exit와 suite_passed=false가 있다. 로컬 runner는
`node target/capacity-slice-verify.mjs workspace capacity-slice-20260907-02`이고 해당 runner 사본도
그 디렉터리에 보존했다. 이 명령은 기존 log를 덮어쓰지 않으므로 재실행에는 새 proof 이름을 사용한다.

현재 staged worker의 `publish_or_wait` 1ms 대기·EventNode 입력 재시도·Cancel/Drain·effect 예산/
ACK 전송 단계·실제 broker 통합은 미완이다. 중립 API가 있는 것과 실제 consumer가 통지를 사용하는
것을 구분한다. C++/JS 실기 harness·모델/GPU·원격 배포·커밋/push는 이번에 수행하지 않았다.

기본 비활성 `cross-wire-fixture`의 agent_relay/agent_relay_full/broker_registry/cross_wire/
reconnect_delivery **5개 target, 각1개 시험은 빌드·실행 제외**이며 위 passed/ignored에 넣지 않았다.
기존 외부 의존 feature 설정은 바꾸지 않았다. `cargo clippy -p p4-adapter --all-targets --locked`는
exit0이지만 Event enum/Err 크기 경고4개가 남는다. 이 결과는
`target/capacity-slice-20260907-02/clippy-neutral.log`에 보존하며 warning-free라고 하지 않는다.

### 독립 복사본 변이와 정리 실패의 구분

`target/mailbox-capacity-notification-20260907-01/copy/`는 실제 p4-protocol/p4-adapter의
59개 프로젝트 소스/의존 선언/fixture를 그대로 복사한 최소 workspace다. root workspace 전체가
아니며 workspace membership과 복사본 lockfile은 그 두 crate에 맞게 정리됐다. 원본과 복사본의
해당59개 입력은 closure-manifest.json/closure-after.json으로 전후 대조했고 최종 복원까지 일치했다.
registry/toolchain까지 포함한 hermetic build 증명으로 확대하지 않는다.

아래는 최종 시험 SHA `64ac910f...`를 고정하고 **복사본 production source만** 바꾼 결과다.
모든 유효 arm에서 실제 p4-adapter 재컴파일과 최종 libtest summary, source/EXE hash를 확인했다.
원본과 복사본 source가 같다는 것과 MSVC 링크 바이너리가 bit-identical하다는 것은 별개다.

| arm | 실행 passed/failed | 검출 조건 |
| --- | ---: | --- |
| 최종 copy 기준선 |14/0|원본과 같은 mailbox 회귀|
| M1 drain 통지 제거 |8/6|원본 Event 보존/재시도 및 실제 wake 누락|
| M2 sender disconnect 전 wake |13/1|callback 안에서 Closed를 관측하지 못함|
| M3 receiver disconnect 전 통지 |13/1|재진입 publish가 Closed 대신 Full|
| M4 등록 후 재검사 제거 |12/2|제어된 arrival/close 경쟁에서 Pending|
| M5 reader 참조 회수 제거 |11/3|Ready/receiver Drop 뒤 Weak가 살아 있음|
| M6 reader wake를 잠금 안으로 |13/1|기존 lock 재진입 반례|
| M7b capacity callback만 잠금 안으로 |12/2|자기 해제/재등록 및 mutex 비오염 조건 위반|
| M8 listener 상한 검사 제거 |13/1|65번째 등록이 Exhausted 대신 Ok|
| 정확 복원 |14/0|새 컴파일, 최종 source/test bytes 복원|

유효 변이8건은 모두 exit101이며 기준선/복원은0이다. 47개 기존 adapter 시험은 이 필터 실행에서
제외됐고, 원본 전체61/전체 workspace1210 집계와 섞지 않는다. 중간 수명 RED의 전체 소스는 별도
봉인하지 못했으므로 그 로그만으로 재구성 가능하다고 하지 않는다. 최종 M5가 같은 수명 조건을
독립 소스로 재현·고정한다.

넓은 M7은 callback뿐 아니라 마지막 Waker destructor까지 잠금 안에 남겨 정리 중 교착했다.
**FAILED summary 없이 강제 종료된 INVALID/HANG 1건**으로 따로 보존하고 위8건에서 제외했다.
경로를 확인한 복사본 시험 프로세스만 종료했다. 원본 기대값을 낮추지 않고 callback 잠금만 바꾸는
M7b로 좁혀 두 개의 정상 종료 실패를 확보했다. 실패 단언이 한 번 출력됐다는 것만으로 완료된 변이
시험이라고 보고하지 않는다.

명령·각 source/EXE SHA·raw log·도구 버전·최종 closure 대조는 같은 디렉터리의 verification.txt와
각 meta.log를 따른다. verification.txt의 SHA는
`eb8ee9456ac4855aaa99b0f0115e366af3281617887cd20a9d854f371e5f2556`이다.

최종 문서/스캐너 실행은 `target/capacity-slice-20260907-02/gates.json`에 보존했다.
추적73/전체79 문서 clean, docs 자체12/12, cargo docs1/1, private-header81 clean/common0 header·
5 source다. 이 문자열/색인/의존 패턴 검사는 의미 정확성·미구현 actor 진행·native full/relink
격리의 증명이 아니다. 최종 source check도393/`87e1a42d...`와 일치한다. 이 결과 문단을 추가한 뒤
문서 lint는 docs-final-tracked.log/docs-final-all.log로 다시 확인한다.

## 2026-09-07 후속 — head 제어의 적용·전송 권위

기준 HEAD `a9e1967fc` 위 미커밋 구현이다. 이번 변경의 범위는 `node/state.rs`의 dispatch 상태,
`worker/control_dispatch.rs`의 head 전용 검증, `effects.rs`의 실제 성공 hook 및 RELEASED/SETTLED
소비 전제다. 중립 P4 envelope/core·native wire·llama/backend는 이 slice에서 변경하지 않았다.
현재 계획/다음 행동은 로드맵, 단계 의미와 ticket 수명은 배치 계약, 시험 의무는 검증 규약 T23이 소유한다.

### 수정 전 ACK 소비 반례

`target/control-progress-red-20260907-01/`에 최초 두 시험·당시 소스·실행파일·원문을 보존했다.
실제 staged/agent-core 재컴파일 뒤 `cargo test -p p4-llamacpp-staged-adapter control_progress_tests
-- --nocapture`는 **0 passed/2 failed/437 filtered**, exit101이었다. 준비한 pending 상태에
인코딩된 정상 형식 ACK를 actual codec→Worker::handle로 넣었다. RELEASED는 미적용 슬롯 둘을
free로 반환했고, SETTLED는 미적용 direct Proposal/Replay 둘을 재개시켰다. 원장/요청/native 호출/
출력 효과까지 대조했다. 이 시험은 future yielding seam의 승인 전제이며 기존 동기 run-loop에서
같은 중간 상태가 외부에 노출된다는 공격 재현은 아니다. 실제 Full ACK 기아 RED와 합치지 않는다.

RED 소스405 파일의 manifest SHA는 `e7418f942945455a8e859a57c268bb0caab96096341f3c1163fcf42e3dd08ce0`,
실행 EXE는 `bcd325c2882da2308c532bb06dd389f67abc35275064bdb9326f0f37b89ae44c`다.
원본/보존본405개가 전후 일치했다. 이 closure는 Rust/Cargo/JSON glob 범위여서 아래396개와 파일
선정 범위가 다르다. 숫자 또는 hash가 다르다는 사실만으로 source 변경량을 추정하지 않는다.

### 실제 소비 경계와 증명 한계

후속 `control_progress_tests`6개는 미완 멤버의 양순서·Queued/LocalApplied·load/session 불일치를
whole-event 거부하고, dispatch만 수리한 동일 ACK는 정상 해제/direct Proposal/Replay로 재개하는지
검사한다. 진단 next_event만 정확히1 증가하고 나머지 업무 snapshot과 native 호출은 보존돼야 한다.
기존 소비 시험에 handcrafted ForwardAccepted를 둔 것은 그 소비 전제를 명시한 것이지 native/
송신을 시험했다는 뜻이 아니다. 새 효과 시험으로 생산 경계를 별도로 검사했다.

`control_dispatch_effect_tests`9개는 load/session/KV 초기 상태를 주입하고 실제 flush_effects→
native Frame/P4ID→owner receipt/frontier→completion을 통과한다. fake engine은 독립 byte parser로
native 변경을 먼저 반영하고 정상·변조·손실 응답을 만든다. 새 파일은 production 수정 뒤 추가됐으므로
구버전 RED를 주장하지 않는다. 전체 실행과 아래 독립 변이가 회귀의 실패 가능성을 고정한다.

| 검사 | 요구 결과 |
| --- | --- |
| RELEASE/SETTLE native 성공 | 정확한 receipt/frontier 반영 뒤 LocalApplied; Forward 권한 없음 |
| native 변경 후 손실/변조 | Queued 유지, 상태 commit 없음, fence, intent 보존, native 재시도 없음 |
| cached native replay | 추가 native0회, LocalApplied/ForwardAccepted 승격·퇴행 없음 |
| 정확한 whole-command forward | 정확한 Event/본문/target이 수용된 뒤 전 멤버 ForwardAccepted |
| Closed/ID 고갈 | LocalApplied·pending·native·미전송 intent 보존, fence |
| stale scope/identity·wrong route/class·마지막 오류 멤버 | 선행 멤버 부분 승격/송신/native 실행 없음 |

현재 local/forward ticket은 다른 private 타입이다. prepare→동기 effect→성공 callback 사이에
actor yield가 없다는 전제에서만 key가 유효하다. 이는 비동기 예약이 아니다. Full 뒤 ticket 유지,
command native 그룹 중간 yield, ACK receipt 예산 예약은 아직 구현·증명하지 않았다. 이9개는
Worker::run/가상 네트워크 전체·실제 llama·GPU 시험도 아니다.

### 최종 원본 전체 실행

`target/capacity-slice-20260907-03/source/`의 **396 Rust/Cargo/명시된 모델 없는 fixture**를 동결했다.
manifest SHA-256은 `db6f4968083b76573a0b7cda139bcaf06d913198d96055d614d30d323d61041a`이며
실행 전후 파일 집합과 바이트 hash가 일치했다. native/toolchain/registry/문서/JS 전체 provenance가 아니다.
`cargo test --workspace --no-fail-fast --locked`의 최종 **57 summaries는1225 passed/1 failed/7 ignored**,
cargo exit101이다. 신규15개와 기존 정상 경로를 모두 포함한다. 실패는 기존
`completion_full_cannot_starve_a_genuine_release_acknowledgement` 단1개이며 oracle/기본 실행을 바꾸지 않았다.

`workspace.log` SHA는 `c173cc0bb4cc90fc75e765edeb9dd2432d4ff54a8eb75f93a21fcc1347238c9a`다.
실제 명령·시작/종료·source는 workspace-result.json에 있고 **suite_passed=false**다. 로컬 runner 사본과
`node target/capacity-slice-verify.mjs workspace capacity-slice-20260907-03` 명령도 보존했다.
기존 log는 덮어쓰지 않으며 새 재실행은 새 proof 이름을 사용한다. 앞선 예비 좁은 시험6/9/전체 staged
442P1F를 이 최종 실행의 별도 추가 표본으로 합산하지 않는다.

외부 `cross-wire-fixture`5개 target/각1개는 기본 빌드·실행 제외이며 passed/ignored에 넣지 않았다.
전체 C++/JS 하네스·모델/GPU·원격 배포·커밋/push는 이번 slice에서 하지 않았다. capacity API의
production 소비·bounded outbox/미래 OUTPUT·receipt 예약·Cancel/Drain이 남아 있으므로 이 결과는
성능 단계나 실제 VRAM-only/RAM 오프로딩 웨이브 승격 증거가 아니다.

### 독립 복사본의 phase 변이8종

`target/control-dispatch-mutations-20260907-01/`은 같은396 입력과 workspace 선언/Cargo.lock을
원문 그대로 복사했다. `--locked --offline`으로 신규15개만 실행하며 매 arm 실제 staged 재컴파일,
그 시작 이후 EXE 수정시각, 실제 최종 summary/exit와 보존 EXE hash를 확인했다. 테스트와 기대값은
변경하지 않고 production 한 파일만 바꿨다. 원본396 입력은 모든 arm 전후 동일했고 복사본은 정확히 복원됐다.

| arm | passed/failed | 검출 |
| --- | ---: | --- |
| baseline |15/0|주입된 초기 상태의 두 실제 소비 경계|
| ack-phase-bypass |11/4|미적용 ACK가 상태를 소비함|
| local-hook-omitted |9/6|native 성공 뒤 LocalApplied 전이 누락|
| forward-hook-omitted |13/2|송신 수용 뒤 ForwardAccepted 전이 누락|
| forward-before-send |14/1|Release의 ID 고갈 전 조기 승격|
| replay-phase-downgrade |14/1|native replay가 전송 완료 단계를 퇴행|
| forward-route-bypass |14/1|선언된 다음 stage가 아닌 target 송신|
| pending-member-bypass |13/2|원래 제어 identity 변경을 선검증하지 않음|
| queued-forward-bypass |13/2|native 적용 없이 송신 단계로 건너뜀|
| exact restore |15/0|새 실제 컴파일과 전체 source bytes 복원|

유효 변이8개는 모두 cargo101, baseline/restore는0이다. compile failure/hang을 검출로 센 arm은 없다.
forward-before-send는 첫 Release/ID 고갈 단언에서 실패했으므로 그 변이의 Closed/Settle 분기까지
검출됐다고 확대하지 않는다. 원본 양성9개는 그 분기를 실행했다. pending-member-bypass의 실패2개 중
하나는 마지막 op999 송신 허용을 직접 검출했고, 나머지는 StageOwners가 뒤에서 거부해 예상 오류
계층이 달라진 부차 실패다. 서로 독립인 결함2개를 발견했다는 수치가 아니다.
각 arm의 source 사본·source manifest·raw output·EXE와 실제 명령은 arm 디렉터리에 있다.
`verification.json`은 모든 source/로그/EXE를 재대조한 결과다. 동일 소스에서 MSVC 링크한 EXE hash의
차이를 소스 불일치로 읽지 않는다. 기존 실제 Full ACK 시험은 이15개 필터 밖이며 원본 전체 실행에서는
그대로 실패한다. 따라서 이 변이8개가 포화 actor 문제를 해결했다는 의미가 아니다.
verification.json SHA-256은 `88060ec41b72444d68a5000fe3a7018adc8a046dae72a0ec7a6bc514e2cf0eeb`이다.

### 정적 검사와 문서 게이트

같은 동결 소스의 `cargo clippy -p p4-llamacpp-staged-adapter --all-targets --locked`는 exit0이다.
로그는 `target/capacity-slice-20260907-03/clippy-staged.log`. staged lib16, lib test28(14 duplicates),
의존 adapter4/agent-core1 경고가 남으며 warning-free라고 하지 않는다. 소스 정리를 동결 실행에 섞지 않았다.
문서/의존 스캐너는 같은 디렉터리 `gates.json`에 보존했다: 추적73/전체79 clean, docs 자체12/12,
cargo docs1/1, private81/common0 header·5 source. 숫자/링크/패턴 검사는 의미·native link 격리·
미완 actor나 실제 하드웨어 승격의 증명이 아니다. 이 결과 추가 뒤 문서 lint는 별도 final 로그로 재확인한다.

### 다음 예약 slice의 SESSION 반례 — 원본 전체 집계 밖

`target/session-emission-reservation-red-20260907-01/source/`는 위396 입력의 독립 원형 복사본이다.
workspace 선언/lock/fixture는 그대로이며 새 target에 `--locked --offline` 실제 재컴파일41.04초로
기존 SESSION6개/0실패 기준선을 얻었다. 이후 복사본 `worker/session_tests.rs`에만 시험3개를 추가했다.
원본 production/test 바이트는 바꾸지 않았다. 재컴파일12.06초 뒤 **7 passed/2 failed/446 filtered**,
cargo101이다. 원본 전체1225 집계 또는 phase 변이15개에 이 probe를 합산하지 않는다.

`next_event=u64::MAX`에서 actual `Worker::session`과 actual `Worker::handle` 각각은 거부하지만
sessions가 빈 상태에서 declared-pipeline 하나로 변한다. next_event·effects=[]·effects_fenced=false·
Lifecycle::Empty·has_server=false는 보존되고 completion Event는0개다. 즉 이 반례는 모델/native를
설치하지 않고 generation/route 입력 상태를 주입한 **응답 ID 선확보 전 상태 commit** 문제다.
direct 오류는 실제로 `completion queue is full`이라고 잘못 표기된다. ID 고갈은 publish_or_wait 진입
전에 발생하므로 이 실행은 실제 Full/Closed를 관측한 증거가 아니다.

추가 정상 handle 양성은 sequence41→42, SESSION_READY v4 JSON·전체 Event 인코딩340바이트와
왕복·한 번 전달을 대조했다. ID 실패 입력만 지워 통과시키지 않는다. 실제 원문은 02-probe.log,
그 SHA-256은 `dcb7d7b0a4140c027a157dadc78153d86bd8c54950814b4619dcc9ddf04babf0`이며
보존 실행 EXE SHA는 `6784d3c8dcb0e9c87efda33641feae0a782a263440fc9c3a67a7233c26246b1c`다.
전후 source manifest와 추가 시험 diff를 같은 디렉터리에 보존했다. 원본 수정/완료는 아직 아니며,
Full 중 native 결과 보존·전체 count/byte 예산·capacity wake·actor 진행의 증명으로 확대하지 않는다.

## 2026-09-07 후속 — SESSION 응답 준비

위 ID 고갈 반례를 원본 기본 시험으로 옮기고 SESSION 소비 경계를 수정했다. 이 slice의 생산/시험
변경은 `worker/emit.rs`, `control.rs`, `session_tests.rs` 세 파일뿐이다. 기존 phase396 소스와
나머지 입력은 같으며 중립 protocol/native/llama/backend는 수정하지 않았다.

### 추가 독립 RED와 소비 의미

`target/session-envelope-red-20260907-01/`는 앞 phase396/db6f4968 봉인의 production을 그대로
사용한 독립 복사본이다. session_tests에 probe1개만 추가하고 새 컴파일39.90초 뒤 **0 passed/1 failed**,
cargo101을 얻었다. 정상 응답 양성을 먼저 실행한 뒤 원본 ID140,000바이트 입력의 실제 wire 왕복이
성공함을 확인했다. 그 입력 envelope는140,232바이트지만 응답은 ID와 causation에 원본 ID가 중복돼
280,253바이트가 된다. actual session()은 Ok·sessions0→1·ID1→2였는데 내보낸 Event는 encode 성공/
decode 실패였다. 개별 필드 검사와 합산 envelope 검사가 다르다는 실제 소비 반례다.

이 RED의 raw SHA는 `3e54bc6915945187a3b6c09bd1e77e935ddce7650ff8805245702eedc2d7bd99`,
보존 EXE SHA는 `aa8c621f3f0446fdc819713ec94cf5ef4218387d2245201f4c4167828cc7458b`다.
verification.json/md·source·raw log를 보존했고 원본 생산 바이트는 바꾸지 않았다. 세션 generation과
route를 주입한 handler 시험이지 실제 LOAD/Worker::run/네트워크/native/GPU 시험이 아니다.

현재 prepare_json_emission은 &self에서 JSON 직렬화→checked ID 후보→정확한 Event 구성→실제
encode/decode 동일성 검사를 끝낸다. 그 뒤 session 설치→next_event1회 commit→기존 동기 publish다.
SESSION만 이 helper를 소비한다. private 준비물은 동기 구간 전용으로, 다른 emitter나 await/yield가
끼어들 수 있는 예약이 아니다. 임시 encode/decode 복제는 응답 표현 가능성 검사이지 메모리 예산 확보가 아니다.

정식 SESSION12개는 기존6+신규6이다: ID 고갈 direct/handle2, 기존340바이트 응답1, 합산 envelope
direct/handle2, Unicode metadata/body1. 정확한 payload·Event route·ID·원문과 단일 전달을 유지했다.
direct 준비 실패는 session/ID/effects를 보존한다. handle의 정상 거부 루프는 진단 ID를 따로1개
소비할 수 있으며, 거대 ID의 일반 ERROR fallback은 아직 수신 불가능한 envelope를 만들 수 있다.
ID가 완전히 고갈됐으면 진단도 송신하지 못한다. 이를 정상 오류 응답 전달 성공으로 주장하지 않는다.
준비 후 Closed의 기존 state/ID commit도 rollback시키지 않았다. 그 실패 수명과 고정 outbox는 미완이다.

### 최종 원본 실행과 소스

`target/capacity-slice-20260907-04/source/`는 최종396 Rust/Cargo/명시된 모델 없는 fixture를
보존한다. SHA-256은 `9394a953b28063add0e5919c641fab6f872f8449d15bc980d3022504ee3fe8eb`이며
전체 실행 전후 파일 집합·바이트 hash가 일치했다. `cargo test --workspace --no-fail-fast --locked`의
최종57개 summary는 **1231 passed/1 failed/7 ignored**, cargo101이다. 실패는 기존
`completion_full_cannot_starve_a_genuine_release_acknowledgement` 그대로다. 원본 전체 suite는 실패다.

workspace.log SHA는 `f5aee2f6d4efd51d752a3c390777ea3d137b80fc2bf7bffcf78a1df8bec5b544`,
실제 종료/명령/source는 workspace-result.json에 있으며 suite_passed=false다. 앞선1225 집계와
source를 섞지 않는다. 외부 cross-wire-fixture5 target/각1개는 여전히 기본 빌드·실행 제외다.
clippy staged/all-targets/locked는 exit0, staged lib16/test28(14 duplicates)·adapter4 경고가 남는다.
이는 warning-free가 아니며 `clippy-staged.log`에 보존한다. C++/JS 하네스·모델/GPU 웨이브·원격 배포·
커밋/push는 이번에 수행하지 않았다. 문서와 모델 파일 stat 조사도 이 Rust source 봉인의 일부가 아니다.

### 로컬 모델 경로 예비 목록

사용자가 지정한 S:\models를 현재 로컬 계정에서 읽었다. `target/model-file-inventory-20260907-01.json`은
GGUF156파일의 경로/크기/mtime이며 파일명으로 묶으면63그룹이다. 임시 이름 분류는 모델 후보40,
embedding1, projector22이며 split 번호 누락은 없었다. metadata manifest SHA는
`a83dda1d0ca046d3be91bccbffd7ecf0a40984b3c46ab22888ff620da306fc2e`다. 실제 내용 hash가 아니며
파일명/크기/mtime가 같다는 것이 모델 정체성 또는 load 가능성의 증거는 아니다. non-GGUF·헤더 metadata·
memory family·원격 계정 접근·전체 모델/variant 감사는 미완이다. 모델을 읽어 적재하거나 실행하지 않았다.
실기 자원 단계와 승인 기준은 로드맵 §1과 검증 규약 H0를 따른다.

### SESSION 독립 변이와 최종 게이트

`target/session-preparation-mutations-20260907-01/`은 최종396/9394a953 원형 source·workspace
선언·Cargo.lock을 보존한 독립 복사본이다. 현재 SESSION12개만 `--locked --offline`로 검사했다.
각 arm의 실제 staged 재컴파일·새 EXE 시각/hash·전체 source·raw log·최종 summary를 보존했고,
원본396 입력과 테스트 기대값은 모든 실행 전후 같았다.

| arm | passed/failed | 검출 |
| --- | ---: | --- |
| baseline |12/0|현재 실제 SESSION 소비 경계|
| install-before-prepare |8/4|준비 실패 전에 session 권한 설치|
| checked-id-bypass |10/2|ID 고갈 입력을 승인|
| id-commit-omitted |10/2|정상 응답 뒤 ID41 유지|
| id-commit-twice |10/2|정상 응답 뒤 ID43으로 이중 증가|
| decode-preflight-omitted |10/2|encode는 유지했지만 합산 envelope를 받는 쪽이 거부|
| exact restore |12/0|source 전체 정확복원·새 재컴파일|

변이5개는 실제 상태/승인/ID 차이를 검출했고 단순 오류 문자열 차이만으로 센 실패는 없다.
baseline/restore는0, 변이는101이며 compile failure/timeout을 검출로 세지 않았다. 이12개 필터의
통과는 별도 원본 전체1231P/1F/7ignored나 포화 actor 통과를 대신하지 않는다. verification.json/md에
실제 실행 명령/도구 버전/각 source·EXE·로그 hash와 범위를 기록했다.

최종 문서 게이트는 `target/capacity-slice-20260907-04/gates.json`: 추적73/전체79 clean,
docs 자체12/12·cargo docs1/1, private81/common0 header·5 source다. 이 표 추가 뒤 final 문서
로그와 source check를 다시 확인한다. B1/B2/B5 또는 실기 승격은 승인하지 않는다. 남은 첫 행동은
로드맵 마지막 진행 기록이 소유한다.

## 2026-09-07 후속 — 효과 보존 표현과 할당 전 검사

### 구현과 증명 범위

코드 근거는 HEAD `a9e1967fc59dffa6c2e458f1b91f916b1df826c1` 위 미커밋 변경이며, 아래 최종
397파일 source 봉인이 이 회차의 실제 코드다. 출발 HEAD에 이미 구현돼 있었다고 읽지 않는다.

- `worker/effects.rs::CommittedEffect`와 `observe.rs::PreparedTelemetry`의 base는 모두 Envelope다.
  OUTPUT마다 TAIL payload를 복제하거나 Forward에 이전 물리 입력 본문을 별도로 보관하지 않는다.
  생산자 `release/settlement/physical/drive`와 기존 소비 시험도 함께 이관했다.
- `Worker::flush_effects`는 front 전체 clone 대신 pop으로 원본 의도를 소유하고, 실패하면 같은
  원본을 front에 복구한다. `CommittedEffect`의 Clone 파생도 제거했다. Forward의 Vec는 mailbox로
  이동하고 Closed/ID 고갈/shutdown 실패에서 원래 의도로 돌아간다. 작은 native SETTLE 후보 복제는
  응답 proposal이 원본 의도를 바꾸지 않도록 유지한다. 성공 forward 뒤에만 관측 시각을 고정하고
  telemetry를 앞에 이동한다. 후속 관측 실패 때문에 forwarding을 다시 하지 않는다.
- 이행 대상은 **동기 소비 경계**다. Full 중에는 여전히 blocking하며, 실패 뒤 남는 것은
  Envelope+DTO/body이지 재개 가능한 고정 Event 전체가 아니다. 일반 송신의 ID 소비·fence 의미는
  유지했다. 활성 입력·파싱 객체·JSON 직렬화·실행 중 effect까지 포함한 RSS 예산은 아직 없다.
- `capsule/decode.rs::read_capsule`은 outcome 헤더24바이트와 generated 최소12바이트,
  남은 outcome 헤더·proposal/replay i32 배열의 합계가 cursor 잔량 안에 들어가는지 **예약 전** 검사한다.
  곱셈 전에 나눗셈으로 범위를 확인해 overflow를 피한다. 기존 유효 wire count 상한을 줄이지 않았다.
  이는 필요한 wire-size 조건이며 전체 parsed heap/allocator/RSS 상한이나 native 의미 검증은 아니다.

### 실제 소비 회귀와 수정 전 반례

`worker/effect_representation_tests.rs`의6개는 actual prepare_outputs/flush_effects/mailbox를
호출한다. native/model/run-loop를 대신하지 않는다. 작은/큰 causal payload와 출력 수1/8을 대조하고,
32KiB nonempty Vec의 원래 allocation이 실제 mailbox에 도착하는지 검사한다. 정상 Unicode 출력의
Event ID/sequence/causation/source/target/return route/correlation/deadline·token/text/position/stop/
완료 필드와 순서·중복0을 확인한다. Closed·ID 고갈·Full-at-shutdown에서 body/중첩 telemetry/queue suffix
및 원본 allocation을 보존하고, 성공 forwarding 뒤 관측 ID 고갈에서는 고정 시각·순서·forward 재실행0을 본다.
포인터 검사는 nonempty allocation의 소유 이동에 한정되며 TPS나 RSS 수치가 아니다.

기존 head native dispatch9개와 관측8개, 해제 통지·actual loop의 출력/KV oracle는 전체 실행에서
유지했다. 기존 ReleaseReceipt의 `base.payload.is_empty()` 단언은 payload 필드가 없는 Envelope 타입
제약으로 이관하고 원래 제출 provenance/recipient/payload 단언은 유지했다.

`target/capsule-capacity-red-20260907-01/`은 기존 decoder에 테스트 전용 capacity 관측만 붙여
선언 count=3인 작은 무효 입력으로 **0 passed/2 failed**를 재현했다. 실제 컴파일9.58초이며
코드가 요청한 capacity trace가 반례다. 3개 decoder/cursor/capsule 입력은 실행 전후 같았고
manifest SHA `42d5595aefb6afce02f0312ec41dfa43c8f1a3db192441bb66ad5a0380ef2a3e`, 실제 EXE는
`054649a151630c0b77f315282b78fe45013974f1976e726eb8d00038aee03cfb`다. 이것은 부분 입력 봉인이지
전체 workspace 전이 증거가 아니다. 거대 할당·원본 production 변이는 수행하지 않았다.

decoder 새7개 회귀는 actual CapsuleSet::decode에서 zero/exact minimum/one byte short·Unicode·
혼합 capsule·checkpoint/proposal 배열과 불가능한 MAX-u32 선언을 검사한다. 활성 테스트 관측은
1024 초과 capacity가 allocator에 도달하기 전에 panic시켜 변이도 개발 호스트를 고갈시키지 않는다.
정상 production에는 이 테스트 ceiling이 없다. 중간 `green.log`의 Envelope 이관 중 compile 실패는
실행 증거에서 제외했다. 별도 focused7/0 뒤 아래 최종397 입력의 전체 실행에서 다시 통과했다.

### 최종 원본 실행

`target/capacity-slice-20260907-05/source/`는397 Rust/Cargo/명시 fixture 입력을 보존한다.
SHA-256은 `ce33c532f53e8a8c50a454303d1aa672d8883c6fb619fa3915cbd332958d7e07`이며,
전체 실행 전후 파일 집합과 바이트 hash가 같았다. `cargo test --workspace --no-fail-fast --locked`
최종57개 summary는 **1244 passed/1 failed/7 ignored**, cargo101, suite_passed=false다.
workspace.log SHA는 `11760b25b4821842881cd2d570c7c3d85d1151d73f3f9a2a47662ba880deeda0`이다.
기존 `completion_full_cannot_starve_a_genuine_release_acknowledgement`가 유일한 실패이며
시험/기대값을 수정하지 않았다. 기본 feature에서 제외된 cross-wire-fixture5개 target은 미실행이다.

clippy staged/all-targets/locked는 exit0이고 staged lib17/test29(15 duplicates), adapter4,
agent-core1 경고다. `publish_or_retain`의 owned Event 반환에 **result_large_err 경고1개가 증가**했다.
경고를 숨기거나 마지막에 boxing/인터페이스를 바꿔 검증 소스를 어긋나게 하지 않았다. 전체 warning-free가
아니다. clippy log SHA는 `871406ac44416c5f7d7dbbbdd20e859306594c2c4249c8df7bfc05840313afd0`이다.

이번 회차는 C++/JS 실기 하네스·모델/GPU·원격 배포·커밋/push를 수행하지 않았다. VRAM-only 및
RAM 오프로딩 웨이브·다중 컴퓨터 성과가 아니며, 단계 승격을 승인하지 않는다. 향후 수명/예산/ID
예약 결정은 배치 계약의 목표 절, 현재 첫 행동은 로드맵 마지막 기록이 소유한다.

### 표현 이관의 독립 변이와 EOL 정정

`target/effect-storage-mutations-20260907-01/verification.json`은 source05 전체397파일을 복사한
독립 실행이다. baseline/restored는13/0이고 동일13시험에서 front clone2건, 실패 효과 복구 누락3건,
forward body clone3건, telemetry 승격 누락1건, outcome 길이 검사 제거2건, generated 합산 검사
제거5건의 실제 assertion 실패를 검출했다. 모든 arm은 실제 새 컴파일·EXE·동일 시험 구성원을
확인했고 parser 테스트 전용 할당 감시는 유지했다. 컴파일 오류/timeout은 검출로 세지 않았다.
이는 source05의 증거이며 이후 소스로 바꿔 인용하지 않는다.

cursor.rs 행말 정규화 명령이 처음 실패해 source06은05와 같은 바이트로 다시 실행됐다
(1244/1/7). 실제 정규화 뒤 source07은397파일 SHA
`94c9f1d4ce95a069ade5fb88ae14374fc3e630a02ab1803f97a8420092f89e07`이고 전체57 summary가
1244/1/7, cargo101이었다. 05→07의 유일한 코드 차이는 cursor.rs의 EOL이며 LF 정규형 hash
`644abc79f5a2219648706fde961b04288dabd1b84e576e76a064bca8a086cac6`는 같다.
07 workspace.log SHA는 `410bb928ca4bcf9bcc30b54556e6479a5237e9ab3b36e69f83594e94e681cb17`이다.

## 2026-09-07 전체 WIP 체크포인트 — 제한된 ACK 서비스 통합

### 설계 재판정과 구현 범위

사용자 지시에 따라 장기간의 누적 변경을 일부만 남기지 않고 전체 체크포인트로 커밋한다.
임시 빌드·실행 원문·독립 복사본은 기존 target ignore 정책을 유지한다. 이 기록을 포함하는
커밋은 **중간 복원 지점이지 완료/승격 커밋이 아니다**. 이후 최초 git show로 이 기록과 소스를 대조한다.

독립 읽기 검수3건과 실제 코드의 결론은 동일했다. 현재 Full 반례의 blocked sender는 B OUTPUT이
아닌 **B RELEASE forward**이고, A ACK를 적용할 같은 worker가 송신 공간을 기다린다. 전체 RSS/
native HELLO 자원 모델은 이 한 반례 수정의 직렬 선행이 아니었다. 원본 송신 Event의 소유를
유지하며 접근 가능한 ACK를 외부효과 없는 prepare/commit으로 처리하는 한 경계로 변경했다.

- `ack_service.rs`: 한 번에 입력1개. RELEASED/SETTLED만 허용하고 handle/flush/native/drive
  재귀 호출0. non-ACK는 원본 입력1개로 보관, 첫 잘못된 ACK는 진단1개로 보관하며 두 번째 오류는
  원본 입력으로 보관하고 더 읽지 않는다. FIFO 뒤에 갇힌 ACK 진행까지 보장하지 않는다.
- `release.rs`: RELEASED prepare와 순수 commit을 분리했다. 원래 전체 검증·reply별 그룹화·
  슬롯/입장 순서·정산 observer는 보존하고 통지는 기존 FIFO 뒤에 등록한다.
- `obligations.rs`: pending release의 미래 receipt 개수, queued effect와 활성 effect의 미구체화
  suffix, 진단의 ID 의무를 합산한다. 실제 Event sequence는 송신물 구체화 시점에만 발급한다.
  이것은 count/ID 불변식이며 미래 receipt bytes 사전 할당·전체 RSS 예산 구현이 아니다.
- `emit/effects`: Full 재시도의 같은 Event를 유지하고 각 head forward offer 직전에 권위를
  다시 확인한다. 성공 offer와 callback 사이에는 ACK를 처리하지 않는다. 종료 관측에 활성 송신·
  보관 입력·진단을 추가했다. native 호출 그룹은 계속 비선점적이다.

### 현재 검증과 미완

`target/capacity-slice-20260907-08/source/`399 Rust/Cargo/fixture 입력의 SHA는
`7ba8cf32852e4d8820200ef3d6cd14a4098710b95b3326eb9a7496db51c2ab7d`다. 전체
`cargo test --workspace --no-fail-fast --locked`는 **1236 passed/9 failed/7 ignored**, cargo101,
최종57 summary이며 source before/after는 동일하다. log SHA는
`ce5bb371a1dfae71e35c46480c1e688620b510552c1b97107460edfb3a3bf578`이다.

기존 actual Worker의 Full ACK 반례는 통과했다. 그러나 전체 green 또는 이번 수정 완성은 아니다.
실패9개는 다음과 같으며 숨기거나 ignored/feature 제외로 바꾸지 않는다.

| 실패군 | 개수 | 다음 판정 |
| --- | ---: | --- |
| head forward 거부3개 |3|구체 원인/ID 무변경을 유지한 최초 preflight와 매 offer 재검증을 함께 유지|
| SESSION ID 오류 사유 |1|기존 진단 계약 보존|
| 관측/forward 후 고갈3개 |3|사전 예약과 이미 commit한 효과의 전달 실패를 혼동하지 않도록 소비 경계 분리|
| receipt·OUTPUT 고갈2개 |2|commit 전 거부 반례를 추가하고, 기존 commit 후 실패 보존 시험은 실제 그 시점에 주입|

추가 실제 루프의 잘못된 ACK→정상 ACK와 non-ACK FIFO 복구 시험은 이 체크포인트에서 아직 작성
완료되지 않았다. fixture 공통 준비·복구 helper만 추출됐다. 기존 단언은 보존했으며 신규 미실행을
통과로 세지 않는다. 추가 설계 시험·독립 변이·capacity wake·일반 byte 예산·EventNode credit·
Cancel/Drain·GPU 웨이브는 미완이다. C++/JS 하네스·모델·원격 실행·push는 이번 통합에서 수행하지 않았다.

## 두 번째 전체 체크포인트 — 제한된 ACK 진행 검증 (2026-09-07)

첫 중간 커밋 `2e9451a5cb349740982db3e7478b6c9beb1440d3`은 누적196파일 전체를 보존했고,
커밋 직후 비무시 변경/미추적0을 확인했다. 이번 기록은 그 이후 수정과 검증이며, 당시 회귀9개를
통과했다고 소급하지 않는다. 소스·시험·문서 변경은 이번에도 전부 다음 체크포인트에 포함한다.

### 수정의 논리와 경계

1. 최초 head control preflight는 구체 오류와 ID 보존을 책임지고, 매 offer 직전 검사는 Full 중
   ACK가 바꾼 현재 권위를 책임진다. 어느 하나를 다른 하나로 대체하지 않는다. ACK가 은퇴시킨
   historical control replay는 원래 본문/의도를 복구하고 fenced된다. 무해한 성공으로 흡수되지는 않는다.
2. 의무의 사전 검사는 **commit 전**이며, 이미 commit된 효과는 자기 몫을 소비한다. 후자의
   직렬화마다 전체 몫을 다시 요구하지 않는다. 그래도 실제 ID의 checked_add는 유지하여 commit
   뒤 ID 장애/손상이 나면 전달하지 않은 의도를 보존한다. 직접 응답은 미래 몫을 빌리지 못한다.
3. RELEASED의 N개 pending을 G개 원래 소유자 receipt로 전환할 때 `G <= N`이다. queued effects,
   active ForwardObserved의 아직 구체화하지 않은 관측, 미래 receipt, 보류 진단의 합을 검사한다.
   실제 Event sequence는 FIFO 발행물 구체화에서만 소비한다. 이 산식은 ID 개수이지 RAM 예약이 아니다.
4. TAIL의 반환 후보·효과를 전량 검사한 뒤 commit한다. 기존 OUTPUT/receipt 고갈 시험은 이제
   실제 commit 후에 장애를 주입하며 원래의 의도 보존/재정산 금지 단언을 유지한다. 별도 사전
   부족 시험이 전체 요청·원장·slot·effect·ID·mailbox/native 효과의 보존과 정확한 여유량의 성공을 검사한다.
5. head native 결과의 의무 검사도 prepare_issue **전**이다. 부족한 채 두 번 재시도해도 prepared
   issue/flight/요청/owner/frontier/native를 바꾸지 않으며, 정확한3개 ID 여유에서는 첫 logical ordinal1로 실행한다.

### 새 실제 소비 시험8개

모든 시험은 기본 staged lib 집합에 들어가며 ignored/feature 제외로 숨기지 않았다.

| 시험 이름 | 실제 소비와 보장 |
| --- | --- |
| `completion_full_defers_one_bad_ack_error_without_blocking_the_genuine_ack` | Worker::run, 실제 B OUTPUT으로 Full; 잘못된 ACK의 원래 provenance/JSON 진단1개를 보존하면서 정상 ACK는 공간 복구 전에 commit, native 증가0; 복구 후 정상 OUTPUT/KV/receipt 유지 |
| `completion_full_holds_a_non_ack_without_reading_past_it_then_recovers_fifo` | Worker::run, C PREFILL→정상 ACK 순서를 보관하며 Full 안에서 C 실행/ACK 추월0; 복구 후 C가 ACK보다 먼저 수용되고 모두 정상 완주 |
| `b2_completion_full_settles_both_speculative_continuations_without_native_reentry` | 실제 모든 stage의 SETTLE 후 보류한 ACK와 실제 SESSION_READY 두 건으로 Full; Direct/Checkpoint 각각 정산만 먼저 적용하고 native 이력 불변; 기존 literal 토큰/위치/KV oracle 유지 |
| `full_control_replay_revalidates_its_ticket_after_ack_retirement` | 실제 flush/native Frame/mailbox; RELEASE/SETTLE replay Full 중 ACK가 권위를 제거하면 stale 재전달0·추가 native0·원본 body allocation/intent 보존. 시작 KV와 ACK echo는 단일-worker 주입이며 다중-stage ACK 생성 증명 아님 |
| `head_id_shortage_precedes_prepared_issue_and_exact_room_still_runs` | 실제 head handle/drive/codec. 사전부족2회 상태보존과 정확한3개 ID 양성 실행 |
| `receipt_id_shortage_before_commit_preserves_ack_and_slot_authority` | 실제 RELEASED consumer. 사전 전체 거부, native/통지0, pending/slot/ID 보존 |
| `direct_responses_cannot_spend_ids_owed_to_pending_receipts` | 직접 ERROR가 pending2개의 몫을 소비하지 못함; 이어 실제 ACK가 자기 몫으로 owner별 receipt2개 발행 |
| `output_id_obligations_refuse_whole_return_before_commit_and_accept_exact_room` | 실제 TAIL decoder/flight consumer. OUTPUT2개 전체의 사전부족 원자 거부와 정확한2개 ID 성공 |

기존 `completion_full_cannot_starve_a_genuine_release_acknowledgement`의 긍정 복구·출력·정산
단언은 유지했다. Full/ACK 서비스는 새 native 계산을 발행하지 않는다. 기존 non-ACK FIFO 앞단과
두 번째 오류 뒤의 ACK 진행은 범위 밖이다. stale replay의 fenced 수렴 또한 완전한 재연결/drain은 아니다.

### 봉인된 전체 실행

- 원본: `target/capacity-slice-20260907-09/source/` 및 `source.json`, Rust/Cargo/fixture399개.
- 입력 SHA-256: `b50af68ed3760f10b04b1cb88eb6e5ccfaf0d3d7a4f1c079d082c9a6079e2ea6`.
- 명령: `cargo test --workspace --no-fail-fast --locked`.
- 결과: **1253 passed /0 failed /7 ignored**, 최종57 summary, exit0. staged lib은479/0.
- 실행 전후 소스 동일. 원문: `target/capacity-slice-20260907-09/workspace.log`.
- 원문 SHA-256: `54525d9de589023a710b47894e7f458a833c739991bc4ad0c235a3c2bf5a4715`.
- 전체 실행은2026-09-07 03:01:47~03:04:09 UTC. C++/JS 실기 하네스/모델/GPU 실행이 아니다.
- 후속 문서 게이트: tracked/all 각각79파일 clean, 자체12/12, cargo 문서1/1.
  private-header 문자열 게이트81파일 clean, common 부채0 header/5 source(기존 부채 유지).
  이는 C++ 재빌드나 의미 호환 증명이 아니다. 원문은 같은 proof의 `gates.json`과 개별 log다.

### 독립 복사본 변이5종

`target/ack-service-mutations-20260907-01/verification.json`과 각 arm의 source/log/manifest/EXE가
원문이다. 전체399입력과 Cargo.lock/fixture를 복사하고 매 arm 실제 staged 재컴파일·새 EXE 해시와
25개 시험 이름 동일성을 검사했다. 원본 변경0, 최종 복사본 exact 복원, compile 실패/timeout을
검출로 세지 않음. arm별 소스와 EXE를 보존한다. runner는 해당 proof 안 `runner.mjs`다.

| arm | passed/failed | 제거한 불변식 |
| --- | --- | --- |
| baseline |25/0|없음|
| ack-service-omitted |20/5|Full 안에서 ACK 소비|
| id-check-after-issue |24/1|ID 거부가 issue 준비보다 선행|
| future-receipt-omitted |24/1|pending receipt의 미래 몫|
| diagnostic-blocks-valid-ack |24/1|진단1개 보류 중에도 정상 ACK는 처리|
| head-recheck-omitted |24/1|각 offer 직전 현재 권위 검사|
| restored |25/0|봉인 원본으로 복원|

기본 회귀의 재현은 `cargo test -p p4-llamacpp-staged-adapter --lib --locked`로 실행한다.
변이는 독립 복사본에만 위 한 가지 변경을 적용하고 동일25시험을 유지한다. 필터는 `completion_full_`,
`full_control_replay_revalidates_`, `head_id_shortage_`, `v2::node::worker::release_notification_tests`,
`v2::node::worker_tests::t23_`, `v2::node::worker_tests::output_id_obligations_`,
`v2::node::worker::effect_representation_tests`다. 원본을 checkout/reset으로 되돌리는 방식은 금지한다.

### 승격하지 않는 것

국소 ACK 기아와 이번 ID/효과 소비 회귀를 닫았을 뿐 B1/B2/B5 전체 완료가 아니다. byte/RSS/
native 결과 공간의 예약, 통합 capacity wake, non-ACK 뒤 반환 경로, EventNode/broker credit,
graceful Cancel/Drain은 남아 있다. 최종 출력 품질/TPS/GPU 활용 또는 다중 컴퓨터 실기는 이 증거에
없다. 다음 첫 행동과 전체 순서는 실행 로드맵의 최신 진행 기록만 소유한다.

## 외부 감수 대조와 Git 포함 심사 (2026-09-07)

### 시간과 검증 범위

외부 감수의11:43~11:45 스냅샷은 첫 WIP의1236 passed/9 failed/7 ignored와 일치한다.
그 뒤 `96c90f99e`의1253/0/7 및399봉인 입력 대조와 혼동하지 않는다. ACK 서비스의
국소 GREEN과 변이5종은 앞 절에 기록돼 있지만 ResourceBudget/byte/RSS 완료는 아니다.
현재 `native_calls`와 `requests`는 Full 중 native 불변 및 보류 PREFILL의 FIFO 복구 단언에
쓰이므로 미사용이라는 옛 지적을 근거로 제거하지 않았다.

이번 코드 차이는 두 가지다. 단일 WorkerInput::Event를 이미 처리한 뒤의 도달 불가 Full(_)
분기를 제거했다. 또 `cancel_prepared_issue`를 시험 빌드로 한정했고 기존 시험 호출4개는
유지했다. 성공한 prepare_issue와 begin_native_issue 사이에 yield/일반 취소 분기는 없다.
이것은 운영 Cancel 구현이나 실제 도달 가능한 Full의 Closed 오분류 수정이 아니다.
native 시도 후 불명 상태를 취소로 되돌리는 동작도 추가하지 않았다.

중간 실행 `target/capacity-slice-20260907-10`은1253/0/7이었다. 이후 주석 표현을 정밀화한
최종 Rust399입력 SHA256은 `c56ff070e61aec2a857d6b0a6113842ab04ee650b69d3855d74be6efb90a4d23`다.
`target/capacity-slice-20260907-11`의 전체 실행은 **1252/1/7**,57summary·cargo101이다.
실패는 문서2개의 혼합 EOL이며, 문서 형식 정리 전에 전체 시험을 시작한 절차 오류다.
원문 SHA256은 `e5a43fb4b2071801bb8ca7d9f4f61f2b9ece2364eddc7cced104be9eab39f148`다.
하네스는 `node --test`에 `test/benchmarks/p4-4node/**/*.test.mjs`의 실제 파일 목록을 전달해
**72 passed/0 failed/0 skipped**를 확인했다. C++·GPU·원격 배포·push는 실행하지 않았다.
이 정리에 대한 변이5종 재실행도 주장하지 않는다. 그것은 앞 체크포인트의 별도 증거다.

문서 형식 정리 후 **요청된3라운드 상한 중 추가1라운드**를
`target/capacity-slice-20260907-12`로 실행했다. 위와 동일한 Rust399입력에서
`cargo test --workspace --no-fail-fast --locked`는 **1253 passed/0 failed/7 ignored**,
57summary·cargo0이다(04:00:53~04:02:58 UTC). 원문 SHA256은
`276c791ccdeb3fa4ca85efc5bd76e9462a5c12cf3f0743857ce0be7641a68ad1`다.
같은 묶음은 docs-lint 기본/전체79파일, 자체시험12/12, cargo docs1/1,
private-header81파일·common 부채0header/5source, 하네스72/0/0skipped를 확인했다.
Rust 입력의 실행 전후 내용은 동일하다. 실행 중에는 입력을 편집하지 않았으며 종료 뒤에는
이 결과 기록만 추가하고 문서 형식/색인을 다시 확인한다. 이번 비동작 정리는 첫 라운드에서
통과해 둘째/셋째 반복을 하지 않는다. 앞의 문서 실패 원문은 보존한다. 아직 미구현인 순환
대기 수정이나 분산 배치 전체가 이1라운드로 완료됐다는 뜻은 아니다.

### 보관 결정

약1.06MB·36파일의 생성 묶음과 특정 커밋 전용104줄 보관 도구를 만들었지만 **Git 포함을 철회**했다.
대부분은399소스 원장의 반복이며, 원문 해시 검사는 당시 EXE·환경·경로 독립 재실행을 복구하지 않는다.
이것을 배치 개발의 새 공용 도구로 확장하지 않는다. 삭제 없이
`target/unpublished-ack-archive-20260907-01/`로 옮겼으며 기존 `/target/` 무시 규칙을 확인했다.
보존된 `bundle/manifest.json` SHA256은
`35fc1d91f0405f6f8c69c52209ec6a4d755477efbc3565a2dace2fe66581563e`다.
원 실행 결과와 변이 원자료 경로는 앞 절 그대로다. 이들은 **로컬 보존일 뿐 장기 증거가 아니다**.
다른 경로에서35원문/399Git내용 대조와 사본 변조 거부를 확인했어도 시험 재실행이 아니므로,
검증 규약의 다른 머신 재열람/재현 항목은 미충족이다. 외부 저장소로 업로드하지 않았다.
Git에는 실제 소스·회귀 시험과 이 간결한 기록을 유지한다. 새 JSON/보관 도구/원문 복제는 넣지 않는다.

### 다음 반례의 코드상 후보 — 실행된 RED 아님

근거는 `layers/agent/src/event_node/mod.rs::EventNode::run` @ 96c90f99e의 보류 출력 뒤
completion 수신 제한, `layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/ack_service.rs::Worker::service_blocked_ack` @ 96c90f99e의
nonACK 보류 뒤 수신 제한이다. `entrypoints/agent/src/event_runtime/control.rs::create` @ 96c90f99e는
broker 입력과 worker 입력을 각각 같은 선언 용량으로 만든다. 기존 duplex 시험의
`layers/agent/src/event_node/tests.rs::DuplexProbeAdapter::try_offer` @ 96c90f99e는 항상 성공한다.

후보 입력은 두 노드H/T, 각 큐 용량1, 정상 요청R의 RELEASE, 정상 요청Q의 PHYSICAL,
추가 정상 PREFILL H1~H4, 동일한 설치 내용의 SESSION 재전달 C1~C6이다. R의 유한 native
RELEASE 중 T 입력을 C1~C3으로 채워 Q의 송신을 보류시키고, H 입력을 H1~H4로 채운다.
R의 진짜 RELEASED도 H 앞에서 보류된다. T가 C1 응답으로 완료큐를 채우고 C2 응답에서
Full이 되면 C3을 보류하며 C4~C6으로 나머지 입력 공간을 채우는 유한 순서가 후보이다.

| 공간 | H | T |
| --- | --- | --- |
| EventNode 보류 출력 | PHYSICAL(Q)→T | RELEASED(R)→H |
| broker 입력 / EventNode 보류 입력 | H4 / H3 | C6 / C5 |
| worker 입력 / worker 보류 입력 | H2 / H1 | C4 / C3 |
| 완료큐 / Full 송신 | Q 관측 / Q StageSpan | C1 SESSION_READY / C2 SESSION_READY |

정상 OUTER 소비와 모든 태스크의 공정한 재개 뒤에도 내부에서 공간을 만들 수 있는지가
검사할 질문이다. 강제 종료나 시험이 외부에서 여유를 주는 것으로 정상 해제 완료를 대체하지 않는다.
아직 이 순서를 실제 EventNode·broker·Worker로 실행하지 않았으며, 순수 PREFILL 웨이브만으로
같은 상태에 도달한다고 증명한 것도 아니다. 구현 전에 도달성·정상 진행 oracle부터 고정한다.

## 실제 actor 순환 반례 — 수정 전 봉인 (2026-09-07)

### 실행 전 고정한 범위와 oracle

기준 HEAD `f13e2560b`에 test-only actor_ring.rs와 Cargo dev 배선만 추가한다. 운영 코드는
바꾸지 않는다. 두 시험은 같은14개 원본 입력(SESSION8개, 정상 추론6개)을 실제 broker/node/
adapter/worker에 전달한다. native Frame 처리와 유한 지연만 fake이며 post-LOAD 상태에서 시작한다.
이것은 자연어/llama/GPU/remote 실기 또는 모든 스케줄에 대한 교착 자유 증명이 아니다.

| 필수 시험 | 수정 전 예상 | 독립 판정 |
| --- | --- | --- |
| `event_actor_ring_saturated_normal_ingress_must_progress_without_external_dequeue` | 마지막 정상 진행 단언 RED | cap1의 실제 보류 소유자/Full, genuine RELEASED의 pending operation 일치, OUTER 계속 배출, native 진행0 |
| `event_actor_ring_same_normal_ingress_completes_with_capacity_eight` | GREEN | 같은 입력·동일 출력 oracle, 외부 completion dequeue 없이 완주 |

cap1의 보류 소유자 표와 유한 입력 순서는 바로 앞 후보 절 그대로다. 시험은 실제 수용/Full 반환의
전체 Event 동등성, source/target/load/session/slot/incarnation/operation, R 슬롯 미반환과 H1~H4
미수용 상태를 대조한다. 단순 완료 수만 검사하지 않는다. 별도 외부 복구 뒤에는 입력14개 전부
실제 adapter에서 정확히 한 번 수용, 요청별 token1000/position1/text/stop=length, native 입력
[(0,10)] 한 번, 두 stage의 각 KV 해제 한 번과 잔량0, SESSION_READY8개를 검사한다.

**정상 진행과 외부 복구는 분리**한다. 공정한 node polling100회 뒤 정상 완료 여부를 먼저 고정한다.
실제 OS worker와1ms 타이머를 쓰므로100회는 논리 시계 독립 증명이 아니다. 실제 대기 고리의 소유자
관측·native 상태 불변·OUTER 배출 및 정상 대조를 함께 읽는다. 이후 알려진 C1~C6 SESSION_READY만
최대6개 외부에서 꺼내 원본 그대로 broker로 전달할 수 있다. 다른 correlation의 PHYSICAL/TAIL/
관측을 꺼내 순서를 바꾸지 않는다. 이 복구가 성공해도 정상 진행 RED를 GREEN으로 바꾸지 않는다.
보강 전 외부 감수의 “C1 한 개로 복구”를 이번 소스의 관측으로 인용하지 않는다.

native gate의10초 만료는 sticky 실패이며 각 poll/완료에서 별도 fixture-expired 단언으로 검사한다.
setup/finish3초 초과, gate 만료, EventNode 종료 또는 컴파일 실패는 예정된 liveness RED가 아니다.
Drop은 모든 native gate를 먼저 열고 실제 adapter worker를 join하지만 graceful 분산 Drain은 아니다.

### 검증 묶음과 시행착오 점검

요청한3라운드 상한 중 앞 실행12가 첫째, 이번13이 둘째다. 실행 전에 위 정상·포화·복구 oracle,
입력 및 문서 형식을 고정하고 Rust/Cargo/fixture와 문서 소스를 봉인한다. 전체 실행 명령은
`cargo test --workspace --no-fail-fast --locked`이며 actor 두 시험도 기본 목록에 포함된다.
예상 밖 실패는 분리 기록하고, 기대값을 바꿔 같은 묶음을 다시 돌리지 않는다. 이번은 운영 fix가
없으므로 fix 제거 변이를 주장하지 않는다. 원자료는 무시 경로의 로컬 증거로 보존하고 Git에는
회귀 시험·필수 배선·계약·이 기록만 넣는다. 장기 외부 재열람은 계속 미충족이다.

### 두 번째 라운드 실제 결과 — 예정된 RED 한 건

`target/capacity-slice-20260907-13/`에400개 Rust/Cargo/fixture 입력을 봉인했다. source SHA256은
`4a353e02335162a53371df334f8bd55b53742745392178cfb99ec1dc8ddb49eb`다. 실행 전후 입력 목록과
전체 bytes/hash가 동일하며 함께 읽힌 변경 문서4개의 SHA256도 전후 동일했다. 종료 뒤 이 결과
기록과 색인만 추가한다. 로그는 `workspace.log`, 집계는 `workspace-result.json`이다.

- 명령: `cargo test --workspace --no-fail-fast --locked`.
- 시간: 2026-09-07 04:29:42~04:32:06 UTC. cargo exit101,57summary.
- 전체 **1254 passed/1 failed/7 ignored**. staged lib480/1, 실행2.21초.
- 유일한 실패는 위 cap1 시험의 `actor_ring.rs` 마지막 `normal_progress` 단언이다. cap8 대조는 PASS.
- native gate 만료·setup/finish timeout·EventNode 종료·컴파일 오류는 없었다. 복구 뒤 전체14입력/
  6결과/native 해제 oracle는 마지막 단언 전에 전부 통과했다. 외부 복구는 실제로 C1~C6 **6개**였다.
- 로그 SHA256: `a2889eef868a1b68ab732df88c7afd46ff4aa38416b0ad31848886eb318f9ce7`.
- 실제 staged lib 재컴파일 로그와 실행 EXE `p4_llamacpp_staged_adapter-60c4f56e88389385.exe`를 대조했다.
  EXE SHA256: `78aa35288537e0960ebe0a6dbe1e85ca3a9dd9ef325e916f81288cdfcf08c26d`.
  검증 당시 EXE는 proof 디렉터리에 별도 보존한다. source/로그/EXE 모두 로컬 증거이며 장기 보존은 아니다.

로컬 집계기의 `expected_red_only`는 **이전 국소 ACK 시험 이름**을 찾는 필드라 false다. 이번에는
`workspace-any`로 전체 실패/exit를 그대로 보존했고 위 실제 유일 실패 이름·단언을 직접 대조했다.
false를 PASS로 바꾸거나 실패 시험을 ignore하지 않았다. 집계기 수정이나 재실행은 하지 않는다.
하네스·C++·GPU·remote·변이 재실행은 이 전체 Rust 결과에 포함하지 않는다.

### 코드 판정과 체크포인트

`EventNode::run`은 held_output 뒤 completion 수신을 멈추고 held_input 뒤 broker 수신을 멈춘다.
`Worker::service_blocked_ack`는 held non-ACK 뒤 수신을 멈춘다. 이 소유 관계에서 두 방향이 모두
포화하면 타이머 wake만 반복해도 어느 소비자도 공간을 만들지 못한다. 위 실행은 유한 정상 입력으로
그 상태의 도달·정지·보존 복구를 관측한 반례이며 무손실 또는 교착 자유의 전역 증명은 아니다.

수정 위치는 기존 B2/B3 목표의 원인 작업별 후속 공간 보장이다. 단일 ACK 예외 확장은 선택하지 않는다.
ID 사전 거부/사후 intent 보존 구분은 배치 계약의 기존 소유 절에, actor 시험의 dev-only 중립 API/
tokio 사용 범위는 격리 계약에 명시했다. production normal/build 의존은 바뀌지 않았다.
**운영 수정 없이 필수 RED를 별도 전체 커밋**으로 보존하며 후속 운영 수정은 이 커밋 뒤에 시작한다.
세 라운드를 새 이름으로 초기화하지 않고 마지막 후보 확인은 재설계가 닫힌 뒤에만 한다.

## 로컬 완료 저장소 예약 기반 — 실행 전 WIP (2026-09-07)

기준 HEAD는 `393a6c23e`다. source/binary 봉인 또는 새 실행 결과가 아닌 **구현/정적 검토 기록**이다.
이 체크포인트는 검증 전 진행 보존이며 RED를 GREEN으로 바꾸었다는 보고가 아니다.

### 변경과 정적 판정

- 중립 `node_adapter/event_cost.rs`는 Event/Envelope/Endpoint/Address를 exhaustive 분해한다.
  inline 및 독립 String/Vec capacity의 checked 합산이며 직렬화나 clone은 하지 않는다.
- `node_adapter/mailbox.rs`는 실제 사전 할당 큐와 ordinary/reserved/owned의 count/bytes를
  동일 원장에 연결한다. move-only 예약, 원본+예약 거부 반환, dequeue 이후 claim 유지,
  새 실제 저장소 수용 후 책임 이전, Event 먼저 폐기 후 claim 반환을 구현했다.
- 단일 비용 초과는 TooLarge, 단일 산술 overflow는 CostOverflow, 다른 소유물 때문에 현재
  부족한 경우는 Full이다. 실제 worker publication match도 영구 오류를 재시도하지 않고
  원본 Event를 호출자로 돌려준다. 그 이후 기존 호출자의 실패/보존 한계는 이번에 일반 해결하지 않았다.
- lock 순서는 Storage→Budget이며 reserve는 Budget을 해제한 뒤 Storage에 들어간다.
  waker 호출/소멸·Event/claim 소멸은 잠금 밖이다. 이는 두 검토자의 정적 경로 확인이며 실행 증명이 아니다.
- RELEASE의 native 전 ID 고갈·기존 prefix 의무·마지막 ID 정상 전달·native 부분 실패 oracle를
  기존 실제 handle/native fixture에 추가했다. RELEASE 운영 코드 자체는 이번에 수정하지 않았다.

### 작성했지만 실행하지 않은 시험

| 범위 | 작성 수 | 판정할 계약 |
| --- | --- | --- |
| Event 보존 비용 | 5 | inline 한 번, 모든 필드 capacity, 중첩 독립 할당, spare Vec, checked overflow |
| 실제 mailbox 예약 | 14 | count/bytes·취소·wrong receiver·too-small·close·owned/transfer·lost wake·경쟁 |
| 실제 Worker publication | 1 | 영구 초과는 원본 allocation을 반환하며 Full/shutdown으로 오분류하지 않음 |
| 실제 RELEASE handle | 4 | 거부 전 native0/보존, 동일 Event 정상 대조, 정확한 successor, 부분 실패 fence |

Worker 영구 오류 시험은 shutdown guard로 잘못된 Full 구현도 유한하게 종료시킨다. 영구 오류와
shutdown abandonment의 snapshot을 구분하므로 잘못된 재시도 분기는 단언 실패가 되어야 한다.
RELEASE의 retained-prefix 경우는 직접 method 경로이며 현 동기 run loop가 flush 도중 그 명령을
수용한다는 주장이 아니다. 정상 대조는 정렬되지 않은 두 owner와 exact Event/wire를 검사한다.

기존 mailbox_tests.rs·actor_ring.rs의 입력/기대는 변경0이다. 기존 1ms worker 대기·원격 serve/pump·
wire version·native 결과 상한·product byte 설정은 그대로다. count-only 생성자는 제품의 byte 한도
선언이 아니다. reserved front를 legacy 소비자로 전달하면 정지하므로 새 예약 생산자는 운영에서
활성화하지 않았다. 단일 작업의 다중 결과를 cap1 슬롯 전부에 선예약하는 것으로 진행을 보장할 수도 없다.

### 검증 지위와 보존

이번 변경에 대한 컴파일/단위/전체/변이/docs-lint/C++/GPU 실행은 **모두 미실행**이다. 실행 전
계약과 코드 대조·서식 정리만 했다. 마지막 고정 검증 라운드는 사용하지 않았고 현재 통과 수를 만들지 않는다.
기존 실행13의1254/1/7은 그 봉인 소스의 결과다. 후보 전체의 수용/반환 연결이 완성되기 전에 마지막
라운드를 이 기반 API 확인용으로 사용하지 않는다. 이번에는 결과 로그/해시 생성물을 추가하지 않았다.
사용자 지시에 따라 전체 비무시 소스·시험·관련 문서를 **미검증 WIP 체크포인트**로 함께 커밋한다.
현재 순서와 다음 첫 행동은 로드맵만 소유하며 B3/전체 교착/최종 실기 완료를 주장하지 않는다.

## 고정 committed 송신물 — 정적 검토 WIP (2026-09-07)

기준 HEAD는 `7f402aba5`다. 현재 변경의 **컴파일·시험·변이·docs-lint는 미실행**이며 새 source/binary
봉인이나 통과 수가 없다. 서식 정리와 코드 대조만 했다. 실행13의1254/1/7은 수정 전 소스의 결과다.

### 변경 범위와 결정 근거

- `effects.rs::Worker::flush_effects`는 FIFO 선두를 완전한 Event로 한 번 만들고 최종 실패에도
  envelope/ID/sequence/payload와 after-action을 함께 보존한다. 기존 Full loop 자체는 이미 원 Event를
  유지했다. Closed/shutdown 뒤 body만 돌리거나 DTO만 남던 경계를 이관한 것이지 Full 동작 발견이 아니다.
- `obligations.rs::CommittedEffect::event_count`는 아직 발급하지 않은 몫이다. ForwardObserved의1+N이
  materialize 뒤 next_event+1과 미할당N으로 바뀐다. Full 중 활성N을 다시1감산하면 진단이 관측 몫을
  쓸 수 있으므로 감산하지 않는다. 이 합 보존과 head ticket의 매 시도 재검증을 독립 정적 검토했다.
- broker는 목적지의 실제 슬롯을 확보한 뒤 수용용 복사를 한다. Full은 원본 Event의 spare capacity와
  allocation까지 돌려준다. 성공 시 큐/ledger 복사·Closed 원본 비반환·전역 순서 영역은 그대로다.
- ID/직렬화 실패는 원 DTO 보존, Event 생성 후 전송 실패는 완전한 Publication 보존으로 표현을 나눴다.
  기존 시험의 입력·ID 경계·native/slot/fence·실제 수신물 기준은 바꾸지 않았다. 이관 중 전체 telemetry
  대조와 Output envelope 대조가 빠질 수 있음을 독립 검토자가 지적해 실행 전에 둘 다 보강했다.
- release_notification 시험은 Closed 뒤 첫 항목이 고정 Event임을 명시하고 원 pending provenance로 만든
  기대 Event 전체와 wire를 대조한다. ID 발급 전 실패는 여전히 원 DTO만 허용한다. 둘 중 아무 표현이나
  허용하는 느슨한 단언으로 바꾸지 않았다. 원래 조기 거부와 post-commit 거부는 그대로 구분한다.

### 작성한 신규 시험과 예정된 제거 변이 — 전부 미실행

실제 `flush_effects`→completion mailbox의 `publication_tests.rs` 6개와 broker 시험2개다.
시험용 fence 해제/수신기 교체는 운영 재연결 API가 아니며 native 완료를 합성하지 않는다.

| 시험/범위 | 고정한 판정 | 나중에 제거할 동작 |
| --- | --- | --- |
| final_forward_failures | Closed/Full-at-shutdown/TooLarge 원 Event·ID·allocation·FIFO, 수동 재개 뒤 정확한 한 번 전달 | 실패 뒤 Event 대신 DTO 재생성 |
| reply_publications | Output/Receipt/Telemetry 전체 reply/envelope/payload, 이미 발급한 ID 재소비0 | frozen Event의 ID 재발급 |
| unallocated_id_failure | 발급 전 의도 전체 보존, Observed 미할당1+N→N | Publication을 다시1+N으로 회계 |
| active_frozen_forward | 실제 Full/invalid ACK에서 후속 observation N개 몫을 진단이 소비하지 못함 | 활성N에서 잘못된1감산 |
| accepted_forward | forward 성공 뒤 observation 실패/재개에 무재forward·고정 timestamp/ID/allocation | observation 실패에서 forward 복원/시각 재설정 |
| native_precondition | native 사전 권한 거부에서 원 intent·미생성 suffix·fence 유지 | 거부 시 intent 제거/후속 materialize |
| broker Full | 같은 원본 allocation3회 반환 후 실제 수용·Duplicate | Full에서 원본 대신 clone 반환 |
| broker order domain | 같은 source/correlation의 다른 목적지에도 순서 위반 거부; 동일 입력의 올바른 순서 성공 | 목적지별 순서 원장으로 변경 |

Full/ACK 시험은 실제 소비 뒤 test-only 관측점에서 mailbox를 닫는다. 누락된 관측점/회귀가 무한
대기가 되지 않도록5초 guard를 두고, 실제 관측점 도달을 별도 단언한다. 이는 시계 독립 liveness 증명이
아니다. 기존 실제 native 성공/실패/불명 결과/ACK 퇴역 검사는 control_dispatch_effect_tests가 유지한다.

기존 실패 표현 이관은 effect_representation/observe/control_dispatch_effect/release_notification의
4개 시험 파일에만 적용했다. 전체 DTO Debug 대신 필요한 wire Event 전체와 아직 미생성 suffix/관측
전체를 비교한다. 변경 전부터 있던 ID/전달순서/원본 allocation/정산/native oracle를 지우지 않는다.

### 아직 연결되지 않은 경계와 증명 제한

이 코드는 동기 publication이고 byte/native 결과 예약이나 actor 입력 양보는 아니다. SESSION/error
직접 응답·EventNode raw 소비·remote serve/pump·전달 grant·Cancel/Drain은 여전히 별도다.
actor_ring.rs의 cap1/cap8·14입력/6결과·full recovery oracle는 변경하지 않았다. 정상 수용을 모두
차단하거나 completion 큐를 늘려 RED를 숨기지 않는다. 제품 예약 생산자도 아직 활성화하지 않는다.

정적 fan-out 계산에서 SESSION은1, head issue는PHYSICAL+BatchObservation+Span의3, tail physical은
TAIL+Span의2를 만든다. head TAIL은 stopped owner별OUTPUT와 native RELEASE 및 command를 만들고,
RELEASED 뒤에는 원 제출별 receipt가 필요하다. cap1 전달 슬롯에 이 전체를 선예약하면 정상 작업도
시작하지 못하므로 effect 보존 공간과 transfer 슬롯을 분리해야 한다. 이 값은 해당 fake2stage fixture의
경로 계산이며 일반 모델/실제 배치 수/byte 상한을 증명하지 않는다.

같은 source/correlation의 PHYSICAL와 뒤의 OUTER 관측은 목적지가 달라도 순서를 공유한다.
따라서 destination 우회나 ACK 우선 lane만으로 해결할 수 없다. 다음 구현 순서는 로드맵이 소유한다.
마지막 검증 라운드는 아직 사용하지 않았으며, 부분 API를 확인하기 위해 새 라운드를 만들지 않는다.
전체 비무시 변경은 미검증 WIP로 커밋한다. 모델·원문로그·일회성 도구·바이너리를 Git에 추가하거나
원격/GPU/C++/push를 실행한 것은 아니다.

## 전달 큐와 필수 결과 보존 공간 — 정적 검토 WIP (2026-09-07)

기준 HEAD는 `bcbadf101`이다. 이 절의 수정은 **컴파일·실행시험·변이·docs-lint 미실행**이며,
수정 전 봉인 실행13의1254/1/7을 이번 소스 결과로 재사용하지 않는다. 마지막 검증 회차도 미사용이다.
기존 actor cap1/cap8·14입력/6결과·native/회복 oracle는 변경하지 않았다. 새 장기 원자료도 생성하지 않았다.

### 코드 변경과 정적 결정

- 실제 mailbox의 delivery queue_capacity와 retained count/bytes를 분리했다. 기존 두 생성자는
  count 상한을 양쪽에 동일하게 적용하며 새 `completion_mailbox_with_limits`만 별도 한도를 받는다.
  reserved publication도 실제 queue Full에서는 원 Event allocation과 선형 예약을 그대로 반환한다.
  owned dequeue는 queue slot만 반환하고 보관 claim을 유지한다. 수신 성공 전 transfer 실패는 양쪽
  claim을 보존한다. 이것은 수신 측 원격 grant나 actor 순환의 전체 수용 계약이 아니다.
- ordinary Full에서 임시 예약을 만들었다 파기하면 자신을 깨워 재시도하는 loop가 생길 수 있으므로
  queue admission과 일반 claim 확보를 Storage→Budget 순서로 같은 push 구간에 뒀다. 영구적인
  단일 Event byte 초과는 queue Full보다 먼저 판정한다. 새 큐 크기나 실험 threshold를 선택하지 않았다.
- `mailbox_group.rs`는 알려진 Event footprint 목록의 count+bytes를 같은 임계구역에서 확보한다.
  각 항목·합산·실제 배열 backing 크기를 checked 연산하고, 준비 후 Closed/경합을 다시 검사한다.
  budget commit 전에는 active Claim을 만들지 않아 사전 실패가 다른 소유자의 공간을 반환하지 않는다.
  commit 이후에는 이미 확보된 배열에 claim만 설치하며 fallible allocation이나 caller callback이 없다.
- 배열 capacity를 claim과 별도로 과금한다. 그룹 항목을 꺼내도 배열은 남기 때문에 count-only에는
  새 group API를 허용하지 않았다. 동시 준비 중 임시 배열은 이 성공한 보존 공간 한도 밖이며 RSS
  예산 완료라 하지 않는다. Event 미래 상한의 정확성은 호출자 계약이고 실제 생산자 연결은 아직 없다.
- 독립 정적 감수에서 그룹 항목별 알림 중 첫 panic→unwind의 다음 알림이라는 이중 호출 반례를
  발견했다. 미사용 항목/배열 회계를 먼저 반환하고 마지막 한 번만 통지하도록 고정했다. 새 owned
  dequeue의 동일 경로도 확인해 Claim은 unwind 중 회계만 반환한다. 첫 panic은 숨기지 않는다.
  callback 위반 뒤 전달/진행이나 임의 RawWaker destructor까지 안전하다는 보장은 하지 않는다.

### 신규 회귀 oracle와 제거 변이 계획 — 실행 전

| 실제 경로 | 작성 수 | 판정 / 제거하면 실패해야 할 동작 |
| --- | --- | --- |
| group 예약·cap1 순차 전달 | 1 |3개 결과를 실제 저장소에 원자 예약, Full 무변이, 독립 retirement, 빈 배열 비용 유지 / queue와 retained 재결합 |
| group 입력/용량 거부 | 3 |빈 목록·마지막 overflow·permanent TooLarge·count-only·일시 Full 무변이와 같은 입력 재수용 / 부분 claim 설치 또는 배열 과금 누락 |
| group 경합/닫힘 | 3 |최종 commit 전 close·ordinary 경쟁·동시 group1개만 승인 / 최종 검사 제거 |
| group 정리/owned dequeue callback | 3 |잠금 밖 한 번 통지, 다른 소유 claim 보존, 첫 panic 전파·재호출0 / quiet cleanup·unwind guard 제거 |
| queue/retained 실제 전달 | 3 |queue1/retained3, dequeue wake paired control, destination Full의 양쪽 claim / Full에서 claim 반환 또는 원 Event 교체 |
| ordinary 거부·생성자 | 3 |Full3회 snapshot/wake0, Full이어도 영구 TooLarge, queue/retained0 거부 / 임시 claim 자기 wake·검사 순서 역전 |

새 시험은 `mailbox_group_tests.rs`10개와 `mailbox_queue_storage_tests.rs`6개다. 원본 Event equality와
payload allocation, permit identity와 실제 storage snapshot을 함께 대조한다. callback 반례는
첫 호출만 panic하게 해 제거 변이가 프로세스 abort가 아닌 재호출 횟수 단언 실패로 드러나게 했다.
동시 group 시험의5초 채널 guard는 실행 실패를 유한하게 보고하기 위한 것이지 시계 독립 진행 증거가 아니다.
표의 변이도 **예정**이며 아직 실행하거나 통과를 보고한 것이 아니다.

### 현재 한계와 Git 포함 판단

변경은 backend 중립 `p4-adapter` 저장소와 그 실제 경로 시험, 소유 계약/로드맵/증거 색인에 한정한다.
새 API를 사용하는 제품 producer/owned consumer·broker dedupe byte 비용·원격 수용 grant·native
가변 결과 bound·통합 input/capacity/shutdown pump는 미완이다. wire/llama/native/모델은 변경하지 않았다.
현재 HELLO row/seq limit와 frame 수신 cap을 native 출력 사전 메모리 bound로 오독하지 않는다.
전체 순환 교착 해결이나 최종 웨이브 성과로 승격할 수 없다. 다음 순서는 로드맵의 최신 기록만 따른다.

유지할 구현·회귀 시험·간결한 계약/진행 기록만 전체 WIP 체크포인트에 포함한다. 생성 로그·중복
manifest·일회성 도구·모델·바이너리는 기존 ignore 경로에 남긴다. 새 시험 파일은 필수 oracle이므로
ignore하지 않으며, 다른 머신에서 원자료를 재열람하는 B8 조건은 여전히 미충족이다.

## PREFILL 수용 연결의 선행 원자성 — 정적 검토 WIP (2026-09-07)

기준 HEAD는 `d8fff7d2712de2bd90daed4c0de8292662761246`이다. 실제 producer/owned consumer 연결을
읽으며 발견한 **코드상의 거부 전이**를 먼저 분리했다. 이번에는 컴파일·시험·변이·docs-lint를 실행하지
않았다. 새 봉인 소스/바이너리/실행 로그와 통과 수는 없다. 검증2회 사용·마지막1회 미사용 상태다.
실행13의1254/1/7은 그때 봉인한 수정 전 소스에만 귀속한다.

### 수정 전 경로와 수정 지점

`worker.rs::Worker::prefill` @ `d8fff7d27`은 session key를 기억한 뒤 Tokenize/context/incarnation을
검사했다. 요청 삽입·incarnation 증가·pending 추가 뒤 `admit_pending`이 거부하는 경로도 있었다.
따라서 일반 요청 처리 함수를 그대로 포화 중 수용 경로에 연결하면 실패한 admission이 기억/요청을
남긴다. 이 절의 반례는 **실행한 RED가 아니라 정적 경로 분석**이다. 실행 전/제거 변이 증명은 남아 있다.

이번 코드는 새 요청을 기존 pending 뒤에 가상으로 붙여 이번 배정 접두 전체를 검사하고, incarnation·
Tokenize·context까지 통과한 뒤 첫 admission 쓰기를 한다. 기존 ACK의 prefix 검사와 같은 validator를
쓰되 ACK의 입력/검사 범위를 넓히지 않았다. 확정 구간에는 새 Result 거부/handler/yield/publication이
없으며 ADMITTED 기록은 확정 뒤다. 오류 우선순위 변경과 보장하지 않는 자원 범위는
[L2 소유 계약](../../../../../../../docs/adapter-batching-layers.md#l2-수용점유-admission)을 따른다.

### 작성한 실제 consumer oracle7개 — 실행·변이는 아직 없음

`worker/session_tests.rs`의 기존 SESSION fixture 아래 `prefill_admission_tests.rs`를 연결했다.
native가 설치되지 않은 fixture이므로 Tokenize 요청 거부 외에는 token 입력으로 실제 prefill/handle을
지난다. Event는 기존 wire codec으로 encode/decode한다. 기존 시험 내용/기대값은 변경하지 않았다.

| 시험 | 거부/정상 입력과 고정한 판정 |
| --- | --- |
| context_refusal | context32에서 prompt32+max1 거부; 동일 request를 prompt31+max1 및 다른 session key로 정상 수용 |
| zero_or_exhausted_incarnation | incarnation0/MAX 모두 무변이 거부; 값만7로 고쳐 동일 입력 수용 |
| invalid_free_slot | 정상 기존 pending 뒤 새 후보, 범위 밖/중복 free id가 접두 전체를 거부; 슬롯만 고쳐 FIFO 배정 |
| a_free_slot_that_is_still_owned | 기존 요청 소유 id를 free에 다시 넣어도 이중 배정0; free 수정 뒤 기존/새 요청의 소유 분리 |
| an_invalid_later_pending_member | 정상 첫 pending 뒤 missing/이미 소유한 두 번째가 있으면 첫 요청도 미배정; 원인 수정 뒤3개 FIFO 배정 |
| tokenize_failure | 실제 tokenize→Empty lifecycle.request 거부에서 admission 보존; 같은 request의 token 입력 수용 |
| two_available_slots | pending2개+새1개, free2개일 때 기존2개 먼저 배정하고 새 요청만 대기, 원본 Event·incarnation 유지 |

각 거부는 direct 호출의 admission/원장 snapshot 및 next_event 불변, 실제 handle의 정확한 ERROR
Event1개와 next_event+1, 원인 수정 뒤 재제출을 함께 검사한다. 일반 handle의 상태 문자열은
`failed:<detail>`로 바뀌므로 **전체 Worker 불변**이라 하지 않는다. private session_key_order와
기록 파일 출력 자체는 검사하지 않으며 전역 환경변수를 조작하지 않는다. Tokenize 사례는 native
parser/GPU 실패 주입이나 Loaded 복구가 아니다. 성공 token 경로도 native 없는 fixture 그대로다.

예정된 변이는 session key 기억/ADMITTED를 검증 앞으로 되돌림, 요청 삽입 뒤 FIFO 검증으로 되돌림,
배정 접두의 후기 구성원 검사 제거, 기존 pending 대신 신규 요청을 먼저 배정하는 것이다. 기록 파일을
관측하지 않는 현 oracle가 ADMITTED 출력 시점만의 변이까지 검출한다고 주장하지 않는다. 최소한 key
조기 기억과 post-insert 거부 변이가 실제 consumer의 상태 보존 단언에서 실패해야 승격할 수 있다.

### 실제 연결 경로 감사와 범위

독립 정적 검토에서 테스트 모듈 가시성/타입, ERROR의 envelope·ID·detail, Empty Tokenize의 lifecycle
상태와 동일 접두 검사를 대조했다. 이는 컴파일러나 실행 결과의 대체 증거가 아니다.

- raw Event 수신은 broker/node만이 아니라 `entrypoints/agent/src/event_runtime/{mod,control,transport}.rs`,
  transport ConnectionSender와 adapter WorkerInput/held_input에도 남아 있다. producer만 예약형으로
  바꾸거나 중간 raw 다리에서 claim을 버리면 소비자 보존 공간은 추적되지 않는다.
- broker는 성공 시 큐용 사본과 exact 중복 원장을 만든다. 중복 원장에 source claim을 붙이면 정상
  count-window 퇴역까지 producer가 묶인다. 독립 중복 비용과 callback의 ledger 잠금 밖 retirement가
  필요하다. count-window 이후에도 byte가 영구 부족한 경우를 destination Full로 기다리게 하지 않는다.
- 실제 remote serve는 dispatch 실패 시 연결을 끝내고, outbound/outer pump 및 connection writer는
  로컬 owned claim/원격 acceptance로 아직 이관되지 않았다. socket write 완료는 수신 공간 증거가 아니다.
- 현재 수정은 byte grant·원인별 필수 출력 예약·blocked worker pump·native 결과 사전 bound를 만들지
  않는다. actor cap1/cap8의 입력·완료·native/외부 복구 oracle는 그대로이며 교착 해결을 주장하지 않는다.

다음 구현 순서의 단독 소유자는 로드맵이다. 이번 변경은 운영 함수2개 파일·새 필수 회귀/배선·관련
소유 계약/진행/증거만 전체 미검증 WIP로 보존한다. generated proof·모델·바이너리·임시 도구는 기존
ignore 경로에 둔다. remote/GPU/C++ 실행이나 push를 하지 않았고 최종 웨이브 성과 승격은 없다.
