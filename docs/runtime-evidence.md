# Runtime evidence

> 문서 지위 (2026-09-06): **증거 색인**. 각 항목의 날짜·실행 범위를 구분한다. 최신 기록이 과거 실행을 현재 HEAD 증거로 바꾸지 않는다.
> 현재 목표·상태·순서는 [실행 로드맵](distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](document-map.md)를 따른다.

## 2026-09-07: 전체 WIP 체크포인트 — ACK 서비스 통합, 회귀9개 미해결

제한된 ACK-only prepare/commit과 송신 재검증·ID 의무 대조를 통합한399입력 봉인의 전체 집계는
**1236 passed/9 failed/7 ignored**, cargo101이다. 기존 actual Full ACK 반례는 통과했지만
구체 오류와 commit 전/후 고갈을 구분하는 기존 회귀9개는 실패하므로 완료나 단계 승격이 아니다.
사용자 지시로 누적 소스·시험·문서를 모두 WIP 커밋에 포함한다. 정확한 실패 목록·다음 행동·
봉인과 범위는 [체크포인트 기록](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)을 따른다.

## 2026-09-07: 효과 보존 표현·할당 전 검사 — 당시 ACK 기아 미해결

모든 committed effect의 base를 Envelope로 이관하고, 큰 forward 본문과 관측의 소유권을
실제 flush/mailbox에서 이동·실패 복구하도록 했다. 캡슐의 불가능한 outcome/generated 선언은
Vec 예약 전에 거부한다. 최종397 Rust 입력 봉인의 전체 집계는 **1244 passed/1 failed/7 ignored**,
cargo101이며 기존 실제 completion Full ACK 기아가 유일한 실패다. 13개 추가 회귀와 소스·변이는
[정산 증거의 효과 보존 표현 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)을 따른다.
전체 RSS/예약·비동기 pump·정상 모델 응답·성능 개선을 증명한 것은 아니다. 실기는 실행하지 않았다.

## 2026-09-07: SESSION 응답 준비 — ID·표현 가능성 실패 원자화

응답을 만들 수 없는데 먼저 session 권한을 설치하는 ID 고갈/합산 envelope 반례를 각각 독립
복사본에서 재현했다. SESSION만 응답 준비→권한 설치→ID commit→기존 송신으로 이관하고,
기존 정상 wire와 Unicode를 포함한 소비 회귀12개를 유지했다. 최종396 입력 봉인의 전체 집계는
**1231 passed/1 failed/7 ignored**, cargo exit101이다. 실패는 기존 actual Full ACK 기아 그대로다.
정확한 RED/범위/소스·변이는
[정산 증거의 SESSION 응답 준비 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)을 따른다.
일반 ERROR fallback·전체 outbox/native 예산·비동기 pump는 미완이며, 모델/GPU 실기는 하지 않았다.

## 2026-09-07: head 제어의 적용·전송 권위 — 실제 ACK 기아는 미해결

pending 등록, local native 적용, 다음 stage 송신 수용을 분리했다. 조기 ACK 소비의 수정 전2개 RED와
후속6개 소비 회귀, native Frame/receipt/frontier·completion을 지나는9개 효과 시험을 구분해 보존했다.
최종 Rust396 입력 봉인의 전체 집계는 **1225 passed/1 failed/7 ignored**, cargo exit101이다.
실패는 기존 actual Worker의 completion Full ACK 기아다. 이번에 actor pump를 구현한 것은 아니다.
실제 범위·원문·변이·소스 대조는
[정산 증거의 head 제어 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)을 따른다.
모델 없는 소비 경로 증거이며 실제 llama/GPU·VRAM-only/RAM 오프로딩 웨이브나 다중 컴퓨터 증명이 아니다.

## 2026-09-07: completion Full의 실제 ACK 기아·중립 공간 통지 — 필수 RED 남음

actual worker에서 출력 공간이 없을 때 정상 해제 ACK가 입력에 들어와도 처리되지 않는 반례를 고정했다.
공간 복구 뒤 기존 출력·해제·관측 완결은 유지됐다. 중립 mailbox의 종료 wake·참조 수명은 수정하고
공간 통지와 새 회귀14개를 추가했지만 actor pump에는 아직 연결하지 않았다. 최종 원본 Rust393 봉인에서
전체1210 passed/1 failed/7 ignored이며, 실패는 새 필수 ACK 진행 시험이다. 이전 green 집계를
현재 상태로 인용하지 않는다. 소스별 결과·독립 변이·남은 한계는
[정산 증거의 completion Full 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)에 보존한다.
모델/GPU·VRAM-only/RAM 오프로딩·다중 컴퓨터 성과는 아니며 현재 순서와 다음 행동은 로드맵이 소유한다.

## 2026-09-07: OUTPUT 발행 증거·소유 관측·실제 소비 완료 — 부분 구현

새 OUTPUT v5/관측 v4 생산·소비를 함께 이관했다. actual worker의 기존 출력/KV/해제 검사를 유지하고
새 원문 캡처7건·독립 발행 기대량·actual drive의 전체 issue/stage 대조를 검사했다. 검수에서 발견한
empty-owner 미확인 실행의 거짓 완료도 실제 반례와 독립 변이로 고정했다. 원본 Rust1196/0/7 ignored,
JS90/0, 독립 복사본12개 arm·source391 봉인 및 정확한 증명 한계는
[정산 증거의 OUTPUT·관측 이관 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)을 따른다.
정상 출력/해제와 늦은 관측의 시간 분모는 분리했다. 실제 모델 tokenizer·GPU 웨이브·전체 broker/drain·
VRAM-only/RAM 오프로딩·다중 컴퓨터 성과는 아니며 단계 상태와 다음 행동은 로드맵만 소유한다.

## 2026-09-07: 제출 문자열 경계 — 실제 worker와 모델 없는 native codec

직렬화된 반환 정보/옵션의 초과 입력이 요청 등록 뒤 worker를 종료시키는 반례를 입구 거부로 수정했다.
기존 byte 한도·정상 Unicode/escape/원문 옵션·정상 출력/KV/해제 oracle를 유지하며 독립 복사본 변이를 검사했다.
Rust 전체 집계와 모델 없는 native codec의 경계·assert 활성·한도 변경 검출은
[정산 증거의 제출 문자열 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)에 보존한다.
전체 CTest/native engine·모델/GPU 웨이브 증거가 아니며 OUTPUT/관측 이관도 아직 남는다.

## 2026-09-07: 내부 발행 증거와 제출 입구 — 부분 구현

실제 L1 승인에 요청별 고정 크기 witness를 결속하고 독립 literal, 실제 worker 2/4/8-stage,
실패 원자성·포화·재전달 및 재컴파일 변이를 검증했다. 추가 NUL 제출 반례는 native 뒤 worker 종료를
재현한 뒤 입구 거부로 바꾸고 정상 재제출까지 시험했다. 소스별 집계와 정확한 증명 범위는
[정산 증거의 내부 witness 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)을 따른다.
OUTPUT/관측 완결은 미이관이며 모델·GPU·VRAM-only/RAM 오프로딩 실기 성과가 아니다.

## 2026-09-07: 보고 지표 분리와 관측 완결 감사 — 실기 미실행

실제 하네스 report 소비 경로에서 생성 토큰과 계산 행, Verify/Replay 혼합을 구분하는 버전화된
수식을 구현했다. 새 report 회귀11, 확장 JS86/0, 변경 없는 Rust379 소스에서 전체1122/0/7 ignored를
확인했다. 구 수식 RED·독립 변이·분모/품질 한계는
[정산 증거의 보고 지표 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)을 따른다.
관측 소유권·완결 witness는 코드 감사 뒤 명세만 보강했으며 아직 구현하지 않았다. 자원 단계 순서와
RAM 분류는 H0에 결속돼 있지만 모델 적재·GPU/VRAM-only/RAM 오프로딩 실기는 이번에 실행하지 않았다.

## 2026-09-07: 요청별 해제 증명·소유자 통지 — 현재 run의 정상 종료

실제 제출→terminal 승인→명시 해제 집합을 생산/소비 양쪽에서 결속했다. 다중 OUTER actual worker,
알림 포화/실패 시 intent 보존, 실제 캡처 소비와 독립 변이를 확인했다. Rust1122/0/7 ignored,
소스379 봉인·JS75/0과 증명 범위는
[정산 증거의 요청별 해제 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)을 따른다.
전체 다중 OUTER 관측·재시작 freshness·출력 없는 종료·내구 전달은 미완이며 실제 모델/GPU/
VRAM-only/RAM 오프로딩 성과가 아니다. 다음 작업과 단계 상태는 로드맵만 소유한다.

## 2026-09-07: SESSION 선언·해제 ACK 발신자 검증 — 부분 구현

중간 노드 ACK가 슬롯을 반환하는 구코드 반례를 실제 broker/worker 및 별도 3-stage run에서 고정했다.
SESSION 생산·설치와 stage source/target 검사를 함께 이관하고, 정상 ACK 재개 뒤 기존 출력/KV 검사를 유지했다.
원본 Rust 1090/0/7 ignored, 소스 374파일 봉인, 양방향 guard 변이와 OUTER 생산 변이는
[정산 증거의 SESSION 권위 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)을 따른다.
scalar 해제 집합·소유자별 통지/관측·알림 의도 보존은 미해결이다. 전송 인증이나 실제 모델/GPU·
VRAM-only/RAM 오프로딩 실기 성과로 확대하지 않는다.

## 2026-09-07: OUTPUT 예산·요청별 fresh-prefill 소비 검증 — 부분 구현

실제 drive의 출력 적용 전 sampled 예산과 최종 경계의 요청별 관측 대조를 구현했다. actual producer의
기존 wire 출력은 보존하고 실제 관측을 독립 workload 상수와 비교한다. 소비 경로 호출 제거·생산 관측
오분류를 독립 재컴파일 변이로 검증했다. 소스 373파일 봉인·1076/0/7 ignored 집계·실행 범위와 제외한
stale-EXE 시도는 [정산 증거의 OUTPUT 예산 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)을 따른다.
scalar RELEASED 멤버십은 여전히 미해결이다. Restore/LCP·독립 native 토큰화·실제 네트워크/GPU·
VRAM-only/RAM 오프로딩 또는 성능 승격 증거는 아니다.

## 2026-09-07: head 승인 출력의 생산·소비 계약 — 부분 구현

actual Worker::run의 encoded OUTPUT과 현재 producer, 실제 OUTER drive를 공용 fixture로 결속했다.
잘못된 tail/any-node 허용과 경로 검증 제거를 독립 변이로 검사한다. 전체 소스/집계·정규화 한계·
다음 소비자 반례 세 건은 [정산 증거의 head OUTPUT 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)을 따른다.
source 승인만의 수리이며 해제 집합·상한·요청별 첫 위치의 전체 승인 정확성은 아직 미완이다.
native 모델/GPU·실제 네트워크·VRAM-only/RAM 오프로딩 성과가 아니다.

## 2026-09-07: busy UNLOAD 보존·native 종료 실패 fence — 부분 구현

실제 run에서 ordinary/speculative의 미완 작업을 성공 UNLOAD로 지우던 반례와 native 종료 실패 뒤
새 SESSION을 승인하던 반례를 고정했다. 거부 뒤 정상 완주·idle 성공 및 독립 생산 변이를 검사했다.
범위·원문·소스/실행파일·전체 집계는 [정산 증거의 UNLOAD 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)을 따른다.
Cancel/Drain·OUTER 전달·실제 OS cleanup·모델/GPU 또는 VRAM-only/RAM 오프로딩 성과는 아니다.

## 2026-09-07: speculative actual run·native Replay 소비 경계 — 부분 구현

전량 수용·부분 SETTLE·checkpoint Replay를 actual Worker::run 2/4스테이지로 검사하고,
생산 소비 변이를 검출했다. fake 성공과 별개로 native logits 요청 누락을 찾았으며 실제 배치
생성 코드의 모델 없는 소비 시험과 llama 빌드 범위를 따로 기록한다. 원문·소스/실행파일·집계와
다음 UNLOAD RED는 [정산 증거의 speculative 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)을 따른다.
실제 llama sampler/checkpoint·GPU 모델 또는 VRAM-only/RAM 오프로딩 강한 웨이브 성과는 아니다.

## 2026-09-07: 유한 actor 기회·종료 실패 보존 — 부분 구현

지속 입력이 head의 계산 기회를 무한히 미루는 반례와, 계산이 입력/제어를 추월하는 경로를
실제 Worker::run으로 검사했다. 중지/EOF, 잔존 원장 조회, cleanup 실패의 정상 종료 오표시와
기존 거부 사유 유실도 회귀로 고정했다. 독립 변이·원문·소스/집계는
[정산 증거의 유한 actor 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)을 따른다.
전체 graceful drain·speculative full-loop·GPU 또는 RAM 오프로딩 웨이브 성과는 아니다.

## 2026-09-07: continuation 폭·actual run-loop·양방향 Full — 부분 구현

native proposal 폭 위반의 실제 소비자 거부, 2/4/8개의 actual Worker::run 웨이브,
중립 EventNode/broker의 양방향 포화 반례와 복구를 검증했다. 소스 동결·원문·변이·정확한
집계와 제한은 [정산 증거의 최신 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)을 따른다.
계산 엔진은 fake이며 실제 모델 품질·GPU/RAM 오프로딩 성능이나 전체 credit/drain 증명이 아니다.

## 2026-09-07: stage KV frontier — 당시 부분 구현, 추가 P1 반례 유지

새 execution ID로 위치·phase를 우회하는 반례를 실제 워커에서 재현한 뒤, 순수 frontier와
head/중간/꼬리 소비 경로를 결속했다. 전체 집계·소스 동결·독립 변이 6종과 원본 GREEN 밖의
proposal 폭 상한 RED 두 경로는 [최신 정산 증거의 마지막 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)을 따른다.
전체 run-loop나 native/GPU 모델·RAM 오프로딩·다중 컴퓨터 실기 성과로 승인하지 않는다.

## 2026-09-07: PHYSICAL 재전달·opaque plan 수명 — 부분 구현

같은 HEAD의 후속 미커밋 작업 트리에서 actual middle/tail과 수신 원장을 연결했다.
보존 중인 정확한 재전달·발급 head 구분·Fresh 부분 실행·native 결과 불명 fence를 검사했다.
모델 없는 plan 수명 회귀 시험도 추가했다. source/변이/실행 및 모델 경로 생략은
[정산 증거의 후속 기록](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)을 따른다.
GPU/RAM 오프로딩 또는 실제 모델 웨이브 성과가 아니다. 현재 자원·다음 행동은 로드맵이 소유한다.

## 2026-09-06: B1 발행/정산과 최소 가짜 stage 연결 — 미완

HEAD `a9e1967fc` + 미커밋 어댑터/시험/문서 변경에서 CPU-only 검증했다.
전체 Rust는 914 passed / 0 failed / 7 ignored(57 summary, exit 0), 하네스 57 passed다.
발행 expected membership, 이벤트 전체 정산, head 승인 뒤 output intent, native 사후 오류 fence를
보강했다. **전 홉 멱등·요청 incarnation·전체 worker 루프·credit·native/GPU 성능 완료가 아니다.**
시험별 범위·변이·소스 식별·남은 결함은
[최신 정산 구현 증거](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)의 마지막 절을 따른다.
단계 상태/다음 행동은 [로드맵](distributed-batching-roadmap.md)에만 기록한다.

## 2026-09-06: 최초 감사 기준과 증거의 지위

현재 목표·상태·실행 순서는 [분산 배치 로드맵](distributed-batching-roadmap.md),
실기 승인 규칙은 [검증 규약](distributed-batching-verification.md)이 소유한다.
아래 수치는 해당 날짜·모델·토폴로지·소스의 관측이지 현재 초대형 다중 머신 제품의 완료 증거가 아니다.

- `a9e1967fc` 코드 감사: Rust 844 passed / 0 failed / 7 ignored, 하네스 57 passed.
- 최초 감사에서는 요청별 부기 공유만 확인했으며 이벤트 전체 정산·발행 identity/range 대조·실제 simulator 거부 시험이 미완이었다. 후속 상태는 위 최신 기록을 따른다.
- [정산 감사의 반례와 재현 조건](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md).
- 이번 기록은 GPU 실측이 아니다. 최종 다중 컴퓨터·초대형 모델·강한 웨이브의 정상 응답/성능은 아직 미증명.

## 2026-09-04: four stages on two cards costs a third to a half of the throughput

The harness has cut every model into four stages since it was written, two to a
card. Two cards give two independent execution lanes, and the fourth stage adds
a full set of per-batch fixed cost to a lane that cannot overlap. Measured by
running the same work at both depths, interleaved: the 35B gains **32.6%** at
one stage a card (175.00 against 131.96 total rows/s) and the 2B gains 42.4%
(624.98 against 438.88), with the distributions not overlapping. The same
layers cost 78 to 149 ms more per lap when cut into four - two extra
crossings at about the per-batch fixed cost measured from the width fit.

**Three corrections to how that was first written.** Every arm passed
*structurally* - 64/64 and 192/192 completed and released - but the 35B arms
scored 55 to 64 of 64 on the meaning judge, varying by run rather than by
configuration. "Every arm passing its judge" was wrong.

The 2B's 42.4% was two effects reported as one. Holding the load equal at 13
and 22 layers a card and changing only the boundary count gives **+22.3%**
(639.66 against 522.87 total rows/s); holding the boundaries at four and
rebalancing 9/26 to 13/22 gives **+19.2%** (522.87 against 438.88). Both
measured over four interleaved runs from a clean tree, all 192/192.

And re-running the 35B pair from committed source gives **+45.4%** (176.31
against 121.27) rather than 32.6%, with 20 layers a card in both arms - that
pair measures the boundary effect on its own and is the number to lean on.

GPU utilisation was identical across the 35B arms at ~33% while throughput
differed by a third, which is the fourth time that number has failed to track
the work done.

**2026-09-06 interpretation correction:** these arms compared specific process
placements on two devices. They do not establish a maximum node count or prove
that processes sharing a device can never overlap useful work. Model/KV capacity,
legal cuts and multi-host deployment determine the required topology. Keep that
topology fixed when claiming a batching-policy gain; study placement separately.

## 2026-09-04: a 35B says the small model was distorting it, and the scheduler was
## throwing work away

On gemma-4-E2B the sampler cost 2.7x the transformer layers, which is what made
parallelising it worth 10%. On a 35B over 40 layers the ratio inverts - layers
cost 1.25 to 1.6x the sampler - so that result belongs to the 2B model and is
now quoted with its condition.

The 35B also exposed a scheduler defect the small model could not. Its memory
forces equal per-sequence UBATCH widths, and the scheduler let one ready decode
row set that common width to one - so a prompt with a thousand rows ready went
one row per batch. Measured: 934.8 token rows ready at plan time against 9.85
issued. Deciding the participants before the width - decodes take their own
batch, prompts share a wide one - moves total throughput 90.13 to 127.77
rows/s, prefill batches to 367 rows mean and 512 max, and rows left behind to
32.1. Concurrent stage occupancy and GPU utilisation both fell while it did.

The parallel sampler is defaulted back to serial: every worker calls
`common_sampler_sample` on one `llama_context`, which reorders the logits
buffer in place. Passing judges is not proof that no race occurred.

Runs, the three harness faults behind the judge failures, and what is still
unmeasured are in
[`2026-09-04-35b-and-the-width-collapse.md`](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-04-35b-and-the-width-collapse.md).

## 2026-09-03: under load the batch fills and the GPU still does not

Every acceptance run before this day had arrival coalescing switched off and
an active set of 16 to 40, so a 512-row UBATCH could not be more than a few
percent full whatever the scheduler did. Raising the active set to 256 and
the arrival rate to 32 a second takes throughput from 166 to 436 tok/s and the
batch to its 512-row cap; a mixed-prefill scenario (prompts of 19 to 1,290
rows) puts prefill and decode in the same physical batch 100 times in 1,348.
GPU utilisation moves from 42% to 45% summed across both cards through all of
it. The first node is timed for the first time: its own five layers take 54
to 64 ms a batch, which scaled to 35 layers is the measured 559 ms lap. The
throughput is in the stage step, not in the batching. A field that was
reported as "the scheduler leaves nothing" turned out to be an identity and
is corrected in the record.

Runs, numbers and the correction are in
[`2026-09-03-load-and-batching.md`](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-03-load-and-batching.md).

## 2026-08-20: 768 tok/s, once a lap is batched and once generation is measured

Two cards, a 1.5B plain attention model split [0,14) and [14,28), a 32-token
prompt against 512 generated tokens so that generation is what the clock is
measuring. Every run completed with all four verdicts.

| Parallel | Aggregate | Per session | Mean hop width |
| ---: | ---: | ---: | ---: |
| 16 | **272.7 tok/s** | 52.6 | max 9 |
| 32 | **550.2 tok/s** | 54.7 | 5.89 (max 19) |
| 64 | **768.0 tok/s** | 34.0 | 10.89 (max 36) |

**The earlier numbers were a measurement fault, not a performance one.** They
were taken with a 5,000-token prompt against 24 generated tokens, where
prefill is 99% of the run and "generation tokens over the run" is not a
statement about generation. Read that way the same deployment reported 8 to 13
tok/s.

Sixteen to thirty-two is 2.02x and near linear; thirty-two to sixty-four is
1.40x with per-session throughput falling from 54.7 to 34.0, which is
concurrency starting to cost latency. Prefill, which is not batched, stays
flat across all of it — that is the control that says the difference is the
decode.

**What is still on the table is width.** Mean hop width is about a sixth of
the declared concurrency, 10.89 against 64, while the maximum reaches 36. A
node sends a lap when it arrives and in a chain laps arrive spread out. The
earlier attempt at holding a window open failed because a hop carried one
sequence however wide the window was; the table above is the proof that the
reason has inverted, and whatever gathers laps now belongs in the adapter
rather than in P4's scheduling.

Conditions and the reproduction line are in
[`2026-08-20-batched-decode-throughput.md`](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-20-batched-decode-throughput.md).

## 2026-08-20: a decode hop, taken apart

The paragraph above guesses that the throughput left on the table is in
whatever gathers laps. It is not. Holding a narrow decode window open for a
quarter of the node's own last hop raised the mean width from 10.89 to 20.48
and cost 41% of the throughput, so the hop was instrumented instead.

| | stage 0 — `[0,14)` | tail — `[14,28)` |
| --- | ---: | ---: |
| `llama_decode` submit | 11.25 ms | 12.57 ms |
| wait for outputs | 0.01 ms | 9.50 ms |
| split the cut-set | 4.45 ms | 0.00 ms |
| sample | 0.00 ms | 21.40 ms |
| **hop** | **15.76 ms** | **43.58 ms** |

Two rows cost 8.55 ms and sixty-two cost 13.57 ms, so one more sequence costs
0.04 to 0.08 ms: **batching a lap already works, and width is not the lever.**
Splitting the same model 7/21 instead of 14/14 puts the rest at about 3 to 5 ms
per call plus 0.5 to 0.7 ms per layer — an order of magnitude above what these
cards' bandwidth explains, because the CUDA graph is never armed. Pinning the
width so it can be armed cut the tail's submit by 44% and moved the aggregate
from 712.9 to 671.0 tokens per second: the time left the submit and came back
in the wait. That change was reverted.

What is left is the tail's sampler chain, 0.396 ms per row over a 151,936-entry
vocabulary, run one row after another on one thread — about a third of the ring,
and the adapter's problem rather than P4's.

Conditions, the code references and the reproduction line are in
[`2026-08-20-decode-hop-cost-decomposition.md`](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-20-decode-hop-cost-decomposition.md).

## 2026-08-20: sixty sessions on four GPUs, and the one number that explains them

A 35B model split across four cards on two machines by layer ratio, sixty
requests of a 5,000-token prompt against 5,000 tokens of answer, ten admitted
at once. Sixty of sixty completed with every verdict passing.

| | over the run | per session |
| --- | ---: | ---: |
| prefill | 19.18 tok/s | 1,996.5 tok/s |
| generation | 18.20 tok/s | 18.37 tok/s |
| combined | 37.38 tok/s | — |

Ten sessions at 18.37 tok/s each aggregate to 18.20. **Concurrency is not
becoming throughput**, and the reason is that a hop carries one sequence:
35,000 decode laps ran as 35,500 graph executions on the first stage, and
every retained sample reports one sequence per hop. The device reads a
stage's weights once per token per session.

The same deployment prefills at 1,996 tok/s per session — same cards, same
layers, and the only difference is how many tokens are in the call. That
ratio is what batching a decode lap is worth, and it is the largest single
number left on the table.

Conditions, evidence and the reproduction line are in
[`2026-08-20-four-node-35b-service-reference.md`](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-20-four-node-35b-service-reference.md).

### The frame limit that hid behind it

A prefill hop carries one F32 cut per token per sequence, so a 5,000-token
prompt at n_embd 2,048 is 39 MiB for one sequence and a ten-wide window is
391 MiB. Both the outer P4 frame and the staged local wire capped a body at
128 MiB, which refused every window past three sequences: 53 of 60 requests
failed on `HOP envelope too large` while the deployment itself was healthy.
Both are now two gibibytes. This is a P4-level change and deliberately the
only one — how large a body a frame may carry is a property of the layer, not
of a backend.

### What did not work, and why it is recorded

Two attempts at the throughput above were measured and rejected.

**Holding a decode window open to make it wider.** A node sends whatever is
waiting, so a lap that arrives alone goes alone. Making it wait 30 ms for
company changed nothing — the graph count moved from 6,208 to 6,209 and
throughput fell from 19.45 to 16.40 tok/s. Nothing joined, because arrivals
are paced by the very service being batched: a closed loop, where waiting
only slows the loop. The rule also had to be declared in the load, which put
a backend's batching sensibility into P4's protocol; that alone was reason
enough to take it back out.

**Batching the sequences of one hop inside the stage server** is the right
layer and the work continues there. It executes — widths of two and three
against a small model, no `llama_decode` failure and no ubatch split — after
three corrections that a first attempt gets wrong: a stage consuming a
cut-set needs an embedding batch rather than a token batch, positions belong
to llama.cpp because a lap knows only its own index, and a unified KV drops
the rule that a ubatch admits only consecutive sequence ids. What remains is
a chain-level defect: with batching on, some sequences stop near the end of
their generation, which the per-sequence path never does. It is behind
`P4_STAGED_DECODE_BATCH` until that is found.

## 2026-08-17: the placement that won on a pipeline does nothing on a wavefront

This machine's record came partly from where the stages sat: the 4080 leading
with sixteen layers against the 3090's twenty-four, worth 271.5 tok/s where an
even split gave 213.8. The first stage carries the prefill and costs more per
layer — 5.05 seconds against 3.0 — so it is given fewer of them and the two
stages take the same time.

That reasoning is sound and it does not apply to llama.cpp's own device split.
Four GPUs over two machines, one 35B model, 64 sessions, every configuration
otherwise identical:

| Devices | Placement | Aggregate |
| ---: | --- | ---: |
| 2 | both local | 173.8 tok/s |
| 4 | remote devices in the middle | 71.8 tok/s |
| 4 | locals first, roughly even | 46.2 tok/s |
| 4 | **4080 leading, a tenth of the layers** | **45.4 tok/s** |

The 4080 held 5.7 GiB against a ceiling of 11, so the placement was what it was
asked to be. It made no difference, and the ordering that ought to have been
best measured worst.

Because a layer split is not a pipeline. llama.cpp takes a cohort through its
devices as one wavefront: device A computes its layers while B, C and D wait,
then B, and so on. Dealing the layers differently changes which term of a sum
is large; it cannot make two terms happen at once. Stage placement is worth
something only where stages overlap, and here they do not — which is also why
the same four cards idle at 0-3% while one of them works.

The layer-dealing rule is worth keeping. It belongs to the runtime that
pipelines, which is P4 owning the boundary, and the measurement of it is in
`tests/pipelining.rs` rather than here: equal layers on unequal stages leave the
stage behind waiting at 85% occupancy, and dealing by cost brings every stage
above 97%.

## 2026-08-17: crossing a machine boundary costs 3x, and not for bandwidth

Four GPUs over two machines: an RTX 4080 and an RTX 3090 here, two RTX 3090s on
a second box, joined by llama.cpp's RPC backend over 1 GbE. The link measures
81 MB/s end to end. The question was whether the second machine buys throughput
or only capacity.

| Devices | Where | Requests | Aggregate |
| ---: | --- | --- | ---: |
| 2 | both local | 64 x 500 | 173.8 tok/s |
| 2 | one across the network | 64 x 200 | 55.3 tok/s |
| 4 | two across the network | 64 x 200 | 71.8 tok/s |
| 4 | same, locals ordered first | 64 x 200 | 46.2 tok/s |

The token counts differ between the first row and the rest — the like-for-like
baseline was lost when its backend was torn down mid-run — so the exact ratio
is not to be quoted. The direction is not in doubt: one crossing costs roughly
three times, and adding a second remote device costs nothing on top of it. What
is expensive is *whether* the boundary is crossed, not how many devices sit past
it. Ordering the local devices first made it worse rather than better, which
also says the cost is not a count of crossings.

### It is not bandwidth, and the numbers are not close

Sampled on the interface during generation: **2.5 MB/s in, 3.0 MB/s out** on a
125 MB/s link, rising to 9 MB/s under the four-device run. Seven per cent, at
worst.

Nor could it be. A hidden state is one token wide: 2048 dimensions at two bytes
is 4 KB, so a 64-sequence decode step moves 256 KB across a boundary. At 81 MB/s
that is three milliseconds, and bandwidth would not bind until something over
five thousand tokens a second.

What the interface does show is **10,932 packets a second averaging 862 bytes**
— about fifteen thousand exchanges per decode step. llama.cpp's RPC backend
synchronises per graph operation rather than per boundary, so a machine boundary
costs round trips in proportion to the graph, not to the data.

### What that means for the layer

This is the case P4's staged distribution exists for. A stage keeps its own KV
and hands on one hidden state per step, so the boundary carries 256 KB once
where llama.cpp's RPC carries almost nothing fifteen thousand times. The
measurement does not make the staged adapter faster — it is still unwritten —
but it does say plainly that no amount of tuning the RPC path will close this,
and that four nodes joined this way are worth capacity rather than throughput.

## 2026-08-17: 83 to 172 tok/s, and three separate reasons it was not

Measured against this machine's own record — 271.5 tok/s, set by the earlier
staged Pipeline at 64 sessions on the same 35B MoE with 400-token prompts and
500-token answers — the same workload was run through P4 and the stock
llama.cpp adapter. It returned 83 tok/s. Three things were wrong, in three
different places, and each hid the next.

| | aggregate | what changed |
| --- | ---: | --- |
| before | ~83 tok/s | ten slots, and never more than a handful busy |
| after | **172.2 tok/s** | 64/64, 31,936 tokens in 185.4 s, all four verdicts |

Still 63% of the record, and the remaining gap is structural — see the end.

### The adapter opened a window one sequence at a time

Every sequence in a hop got its own blocking request, in sequence. A window
produces nothing until all of it is dispatched, so the first token of a window
of sixty-three arrived after the *sum* of sixty-three prefills — a minute and a
half of silence with both cards idle, and a caller watching for progress gave
up before a single token existed. Opened together, that wait is the slowest one
instead of all of them.

### The window composer starved admission

`compose` preferred decode whenever any lap was ready. One sequence decoding
always has a lap ready, so a burst behind it never got in: a node with a ceiling
of sixty-four admitted roughly one every six seconds and never reached the width
it had declared. The main queue had already learned this — it bounds its own
preference every sixteenth take, because strict priority is a veto rather than a
preference — and the composer had not.

The rule now has two halves. While the deployment has room, new work goes
first; once it is carrying its ceiling, decode wins outright, which is when
preferring it actually protects an in-flight request. Room is counted from the
decode items themselves: between hops every live sequence has exactly one lap
waiting, so the number of them *is* the width being carried.

### The batch could not hold one prompt

`-b 256` against 389-token prompts meant llama.cpp could not admit two prefills
in one batch, and slots filled one every 17-20 seconds however fast P4 offered
them. `-b 4096 -ub 512` fills all 64 at once. `-ub 1024` is not usable: it
crashes in `ggml-cuda` with an illegal memory access, in llama.cpp rather than
in anything here.

### What is left, and why it is not a tuning knob

The record came from stage *overlap* — its own report gives `sum/wall = 163.5%`,
both cards computing at the same time. llama.cpp's RPC backend splits a model
across devices but walks them in order within a token, so one card waits while
the other works, which is visible in the GPU sampling here: 91%/27%, then
21%/54%, alternating rather than both high. That ceiling does not move by
tuning; it moves by P4 owning the boundary between the pieces, which is the
staged shape and is not written — two artifacts rather than one, a C++ server
and a Rust adapter. [STAGED.md](../../../STAGED.md) is the plan.

## 2026-08-17: the queue is P4's, under arrival that outruns service

A backend has its own best width and is configured for it — `llama-server`
here runs ten slots, which is where its throughput sits. Requests do not stop
at ten. The claim this layer makes is that everything past that width waits on
*this* side of the adapter boundary, and that the waiting is arranged without
the backend's help, because a backend holding the backlog would make ordering,
cancellation and attribution its business rather than ours.

Until now that claim was only tested against the mock, and every real run had
been at or below its ceiling — so nothing had ever had to be held back.

The scenario: ten slots, a P4 ceiling of ten, and 120 requests arriving one
every 700 ms — about five times faster than they can be served — each a
1,980-token prompt against 400 tokens of answer. Arrivals are spread rather
than burst, because a burst measures a backlog draining and the case that
matters is one forming while earlier work is still running.

```
P4_DRIVE_RESULT requests=120 tokens_each=400
  completed=120 failed=0 unanswered=0 routes=124
  tokens=47880 elapsed_ms=605670 frames_per_second=79
  peak_node_queue=118 peak_in_adapter=10 peak_main_lane=6 samples=1826
  [pass] every request answered / no request failed
  [pass] every stream in order / one terminal per route
```

Read together, which is the only way they mean anything. 118 requests were
held on the node's own queue; never more than 10 were inside the adapter, which
is the declared ceiling and nothing else enforcing it; the agent's deepest lane
reached 6, so the backlog was not sitting in front of the node; and 1,826
samples say this was observed rather than assumed. The driver asked over the
same socket as everything else — the numbers are the protocol's answer, not a
log read over the machine's shoulder.

Corroborated from outside P4 entirely, sampled every few seconds through the
run: established connections from the adapter to `llama-server` stayed at 10-11
and its own `/slots` reported exactly 10 processing. llama.cpp's task queue was
never used. Had P4 forwarded everything and left the backend to sort it out,
there would have been 120 connections and 110 tasks deferred inside it.

Re-run after the node runner was split into four files, because a refactor of
the component under test is only proven by running it: 60 requests on the same
arrival pattern, `peak_node_queue=58 peak_in_adapter=10 peak_main_lane=0` over
924 samples, 60/60 and all four verdicts. Identical behaviour.

### The flag that could not answer the question

`NodeStatus.running` was a bool. "Something is running" is equally true at one
sequence and at a hundred, so an operator could not tell from it whether the
ceiling was being kept — the one thing the field existed to show. It is now a
count of what is inside the adapter, and the run above is the first that could
state its own case.

The driver read `running=` as a number before the field was one, which would
have reported a peak of zero and passed for a measurement. It did not survive
contact with a real snapshot, but only because the number was checked against
what the run was independently known to be doing.

## 2026-08-16: the same two GPUs under a real session shape

The runs below were sixty-four tokens against a three-word prompt, which is a
correctness shape, not a workload. This is the workload: a 4,983-token prompt
against a 5,000-token answer, on sessions provisioned with 15,360 tokens of
context each — enough for both plus headroom.

Provisioning changed with it. `-c 61440 --parallel 4` gives four slots of
15,360; at 80 KiB of KV per token that is 4.7 GiB of cache, up from 0.64 GiB,
so the split moved to `-ts 9,25` to keep the smaller card inside its budget.
The model is `qwen35moe`: 40 layers, 2 KV heads, head dimension 256.

| Card | Budget | In use |
| --- | ---: | ---: |
| RTX 4080 | 11 GiB | 8.6 GiB |
| RTX 3090 | 23 GiB | 18.3 GiB |

| Sessions | Tokens returned | Elapsed | Aggregate | Per session | Result |
| ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 4,998 | 85.3 s | 58.6 tok/s | 58.6 tok/s | all four verdicts |
| 2 | 9,996 | 152.4 s | 65.6 tok/s | 32.8 tok/s | all four verdicts |
| 4 | 19,992 | 241.0 s | 83.0 tok/s | 20.8 tok/s | all four verdicts |

Concurrency helps here rather than inverting as it did on the MI250, but it
helps weakly: doubling to two sessions buys 12%, and four buy 42% over one.
Each session's own rate falls almost in proportion to the number of them —
58.6, 32.8, 20.8 — which is what a scheduler stepping every session together
looks like. Whether the ceiling is the RPC hop, the decode ring's round trip
per token, or llama.cpp's batching is not answered by these three rows, and
they are not enough to claim one.

### Two defects, both in the harness

Neither was in the layer, and neither could appear until a run was long.

The driver waited a fixed three thousand polls. Ample at sixty-four tokens; at
five thousand it ran out at token 4,695 and reported a stall, while the backend
went on to finish all 5,000 normally 5 seconds later. A harness that reports the
moment its own patience expired as a defect in the thing it is measuring is
worse than no harness. Waiting is now bounded by silence — nothing arriving for
30s — and when the driver does stop, it says so above the verdicts, because
"we stopped watching" and "the deployment stopped working" are different claims.

Then routes were named `q0`, `q1`, … in every run. The abandoned inference above
was still generating into the driver's address, so the next run on the same port
collected its leftovers too: 5,232 tokens for a 5,000-token request, and an
ordering failure reported against a layer that had ordered them correctly. Route
names now carry the run that made them.

## 2026-08-16: one 35B model, two GPUs, a node on each

The thing the layer was built for. A single model too large for either card
alone, split across an RTX 4080 and an RTX 3090 in one machine, with a P4 node
standing for each share and the placement stated in the plan rather than left
to whatever llama.cpp decided.

The deployment, and where each half runs:

| Stage | Node | Role | Device | Declared | Process |
| ---: | --- | --- | --- | ---: | --- |
| 0 | `stage-0` | worker | CUDA1 (3090) | 23 GiB | `ggml-rpc-server -d CUDA1 -p 50052` |
| 1 | `tail-1` | front | CUDA0 (4080) | 11 GiB | `llama-server --rpc 127.0.0.1:50052 -dev CUDA0,RPC0 -ts 11,23` |

Model: `Ornith-1.0-35B-UD-Q5_K_S.gguf`, 23.2 GiB, one file on the NAS, loaded
in 4m32s. Nothing of llama.cpp was patched; the binaries are a stock CUDA
build and the distribution is upstream's own RPC backend.

What the split came out as, held steady across every run:

| Card | Budget | In use | Of which weights |
| --- | ---: | ---: | ---: |
| RTX 4080 | 11 GiB | 10.1 GiB | 8.0 GiB |
| RTX 3090 | 23 GiB | 15.8 GiB | 15.4 GiB |

Both inside their budget, and the 4080 is the binding one — the 3090 is under
its ceiling because the model is only 23.2 GiB and the ratio was set 11:23.

| Requests | Tokens each | Tokens returned | Elapsed | Result |
| ---: | ---: | ---: | ---: | --- |
| 4 | 24 | 92 | 4.4 s | 4/4, all four verdicts |
| 8 | 64 | 504 | 15.5 s | 8/8, all four verdicts |
| 4 | 48 | 188 | 6.4 s | 4/4, all four verdicts |
| 16 | 96 | 1,520 | 36.2 s | 16/16, all four verdicts |

Sampled during the 8×64 run, both cards compute: the 3090 reaches 64% and the
4080 33%, alternating rather than one idling. The answers are real English —
the driver now prints one, because four passing verdicts have twice before been
consistent with zero tokens.

### The load is a transaction over shares, and the inference is not

A backend that spreads a model internally makes the load set and the inference
chain different things. Both nodes load and both must bind; only the front
serves. A hop addressed to the worker is refused by name rather than silently
accepted, because a caller that could not tell would wait for tokens that were
never coming.

This cost the driver two pieces of vocabulary — `P4_DRIVE_PLAN_<n>` for a plan
per stage, and `P4_DRIVE_SERVE` for which stages a chain visits. Both are stated
rather than inferred: the alternative was the driver reading a plan to work out
what a stage was for, and a plan is opaque above the adapter boundary.

### The probe that could not work

The front's load first checked each declared worker by opening a TCP connection
to it. It failed on the healthy deployment: an RPC worker already serving a
front refuses further connections, and on Windows that refusal is byte-identical
to an empty port — `ConnectionRefused` either way. A check that cannot tell
"held" from "absent" is worse than no check, because it fires exactly when
nothing is wrong.

What replaced it is where the evidence actually is. llama.cpp will not start
against an RPC device it cannot reach and will not answer a token across one
that died, so a front that serves is a deployment whose workers are present.
The worker's load is the claim; the front's is the proof.

## 2026-08-16: a real inference crosses the network into a stock llama.cpp

The entry below proved the adapter against a real model but over loopback, on
the machine holding it. This is the same thing with a network in the middle,
and against a llama.cpp we have never touched: today's master cloned on the
DGX Spark, built stock with no patches, serving Qwen2.5-1.5B on CPU. The
driver ran on Windows, the agent and the backend on the Linux box.

| Requests | Tokens each | Result |
| ---: | ---: | --- |
| 4 | 16 | 4/4, 4,711 ms |
| 8 | 24 | 8/8, 6,970 ms |
| 16 | 24 | 16/16, 18,264 ms |

All four verdicts on every run, with 245 slot events in the server's own log.
The times are a 1.5B model on CPU with four slots and say nothing about this
layer; what they establish is that the concrete adapter works across a machine
boundary and against an unmodified upstream build, which together are the two
things loopback and a vendored binary could not show.

## 2026-08-16: a real llama.cpp answers through P4

The first inference this layer has carried that a model actually produced.
`llama-server` from the Metal build on `mobimacui-Macmini-2`, holding
Qwen2.5-1.5B-Instruct-Q8_0 with all layers offloaded and four slots, driven
through an agent carrying the `llamacpp` adapter.

| Requests | Tokens each | Result |
| ---: | ---: | --- |
| 4 | 16 | 4/4, 650 ms |
| 8 | 24 | 8/8, 818 ms |
| 16 | 32 | 16/16, 1,913 ms |
| 32 | 32 | 32/32, 3,296 ms |

All four verdicts on every run, and the server's own log shows the slots
working — 531 slot lines, prompts processed, sequences released on stop. The
throughput figures are a 1.5B model on a Mac mini and say nothing about this
layer; what they establish is that the path is real.

The fleet run found a defect the stub tests had not. Every token of an answer
claimed the same position, so the ordering verdict failed while everything else
passed. The adapter was reporting the position the node handed in, and a
request does not carry its progress back down — the backend is the only thing
that knows how far a sequence has got, which is why the mock counts it too.
The adapter now counts what it has delivered, and the stub test pins it.

Three things about the run are worth keeping. The prebuilt `llama-server` on
that machine could not start: its `@rpath` pointed at a build directory that no
longer exists and its `libllama-server-impl.dylib` lives in a different runtime
tree. It was run from a copy with `@loader_path` added rather than by modifying
anything installed.

And the macOS local-network grant was lost by the upgrade, which corrects the
correction in the entry below. Replacing the binary in place kept the grant
when the rebuild produced the same program; a build that genuinely differs — a
new crate linked in — arrives unapproved. The adapter was therefore proved over
loopback on the Mac itself, where the gate does not apply, with the
cross-machine path already established separately.

## 2026-08-16: the same fleet with the network made bad on purpose

Everything before this ran on an idle gigabit LAN, where a hop costs almost
nothing — which is not the network a distributed inference lives on. A relay
was put in front of each agent's port, carrying a declared latency, jitter,
width and stall, with each agent advertising its relay's address. Nothing in
P4 was changed or told: an agent behind a gateway is a case the addressing
already had.

Links: Windows 10ms ±5, both Macs 20ms ±10, GB10 35ms ±15. Four stages, so a
single token's lap crosses all four.

| Requests | Elapsed | Against one request | Frames/s |
| ---: | ---: | ---: | ---: |
| 1 | 3,031 ms | 1.00× | 8 |
| 8 | 4,255 ms | 1.40× | 45 |
| 64 | 4,696 ms | 1.55× | 327 |
| 400 | 9,731 ms | **3.21×** | 987 |

The first row is the floor and cannot be beaten: 24 tokens, four crossings
each, on links averaging 26ms. Everything after it is how well the layer
overlaps work it cannot make faster — sixty-four times the requests for 1.55×
the time, four hundred times for 3.21×. The same shape on the unimpaired path
takes 0.15s, so the link is what is being measured, not the agents.

Then each impairment alone and all of them together, on GB10's link:

| GB10's link | Requests | Result |
| --- | ---: | --- |
| 128 KB/s, no added latency | 200 × 16 | 200/200, 333 frames/s |
| seizes 250ms every 12 chunks | 200 × 16 | 200/200, 573 frames/s |
| 30ms ±20, 256 KB/s, 200ms every 20 | 200 × 16 | 200/200, 323 frames/s |

Every run passed all four verdicts. A degraded link changes how long the work
takes and nothing else about it, which is the whole claim.

### A partition, on real machines

The relay in front of GB10's agent was killed mid-session — a partition rather
than a slow link, and neither agent was touched.

| | Requests | Result |
| --- | ---: | --- |
| link up | 200 × 16 | 200/200 |
| link gone | 20 × 8 | refused: `stage 1 refused the node: no answer` |
| link back | 200 × 16 | 200/200, twice |

Nothing was restarted and nothing reconfigured between the second and third
rows; the relay came back and the layer resumed. There is no membership to
update and no reconnection to command, which is why there was nothing to do.

### The soak, and the leak it found

Twenty-four waves of 150 requests over the impaired two-machine chain — four
stages over two agents, so each also carried two nodes — with garbage bytes and
twenty abandoned connections aimed at each agent between every wave. **3,600
requests, no failure, nothing unanswered**, and the wave time did not drift
(4,218ms first, 3,563ms last).

The first soak is what found the defect. Throughput was flat and correct while
resident memory climbed by a steady amount per wave, which is the shape of a
leak rather than a fault. Every wave re-created its nodes, and a replaced node
turned out never to be freed: the node owns its own event sender, so a run loop
matching `Some(event) = events.recv()` parked on a channel that could not close
once the work channel had. `else => return` was unreachable, and every node ever
replaced or deleted stayed resident with its adapter, its queue and its
in-flight map. Nothing broke — a leaked node is inert — which is why it survived
every functional test.

Two things were fixed off the back of it: that, and the peer map, which was
append-only in the number of addresses ever spoken to.

After both, on the same soak:

| | First six waves | Last six waves |
| --- | ---: | ---: |
| Windows agent | +174 KB/wave | +32 KB/wave |
| GB10 agent | +146 KB/wave | +56 KB/wave |

Decelerating rather than linear, which is an allocator settling rather than
something being retained per request. The claim is what the numbers support: no
per-wave retention that keeps its rate, over 3,600 requests. A run of days is a
longer measurement than this one, and `peers` and `waiting` are now printed for
exactly that — either climbing for hours is a leak rather than load.

The peer retirement was watched working in the real process rather than only in
a test. With the load stopped, the count fell 5 → 4 → 3 as each idle window
expired, and settled at:

```
P4_AGENT_TRAFFIC forwarded=87032 consumed=52 to_nodes=86930 unrouted=0 peers=0 waiting=0
```

An agent that had carried eighty-seven thousand frames holding nothing open and
expecting no reply, with resident memory a little below its peak under load.

Both fixes were then rolled out to the three resident services and the fleet
re-checked on them: four stages over Windows x64, macOS arm64, Linux aarch64
and macOS arm64, 800 requests at 48 tokens, three times, 800/800 each at
78,418-81,399 frames/s.

The rollout corrected something recorded earlier in this file. Replacing the
binary at the approved path did **not** cost the macOS local-network grant —
the upgraded services answered immediately. What had been refused was a build
run from a different path while the installed copy served normally at the same
moment. The grant follows the installed executable, not the source it came
from, and a second agent stood up elsewhere for a test needs its own.

## 2026-08-16: the layer builds and runs on three operating systems and two architectures

Everything before this was Windows on x86-64. The source was copied to two Mac
minis (`mobimacui-Macmini-2`, `mobimacui-Macmini`, macOS on arm64) and to the
DGX Spark (`gx10-c044`, Linux on aarch64), built there, and joined into one
chain with this machine. All four ran `rustc 1.97.1`; nothing in the workspace
needed a conditional or a target-specific dependency.

| Machine | Platform | Build | Tests |
| --- | --- | --- | --- |
| development PC | Windows x86-64 | clean | 196 |
| Mac mini 1 | macOS arm64 | clean | 196 |
| Mac mini 2 | macOS arm64 | clean | 196 |
| DGX Spark GB10 | Linux aarch64 | clean | 196 |

The chain was `stage-0` Windows, `stage-1` macOS, `stage-2` Linux aarch64,
`tail-3` macOS — every hop crossing both a machine and an operating system, and
most of them an architecture as well.

| Shape | Requests | Tokens each | Result |
| --- | ---: | ---: | --- |
| four stages, three OSes | 800 | 48 | 800/800 three times, 80,723-81,799 frames/s |
| four stages, three OSes | 400 | 24 | 400/400, 62,807 frames/s |
| four stages, timed backend | 400 | 24 | 400/400 |
| macOS to macOS only | 400 | 24 | 400/400, 79,774 frames/s |

Four claims held on every run.

### The macOS local-network gate, which is not this layer

The first attempt failed at the macOS stage with `stage refused the node: no
answer`, and it is worth recording because the symptom points at P4 and the
cause is not.

The counters said the agent had received the request and produced its reply —
`consumed=1 forwarded=1` — and `netstat` on that machine showed no outbound
socket to the driver at all. Neither side's firewall was involved: `nc` reached
the driver from that machine while it was listening, and the same Mac running
`p4-drive` connected out to Windows and completed 100/100.

What separated the two was how the process had been started. An agent launched
with `nohup … &` over SSH is detached and becomes its own responsible process,
which on current macOS is not the one holding local-network access; its
outbound LAN connections are dropped with no error to the caller. Listening is
not gated, so it accepted work and answered into nothing. Run in the foreground
of the SSH session — same binary, same host, same ports — it completed 100/100
immediately.

So a macOS agent that receives work but whose replies never arrive should be
checked there before anything in P4 is suspected.

It was then resolved rather than worked around, and the fix was again to
inherit what the node role already had on those machines: a user LaunchAgent
(`RunAtLoad`, `KeepAlive`, `ProcessType Interactive`) bootstrapped into the GUI
domain, which is the only place the prompt can be raised. Bootstrapping alone
did not grant access — the agent ran, listened, and still failed — until
`p4-agent` was switched on in Privacy & Security → Local Network on both
machines. Both then answered immediately.

The Linux host was then given the equivalent — a systemd user service with
lingering, which needs no root and has no permission to grant, macOS's gate
having no counterpart there. Every non-Windows agent in the runs below is a
service its operating system started, not a process held open by an SSH
session.

| Shape, every agent resident | Requests | Tokens each | Result |
| --- | ---: | ---: | --- |
| four stages, three OSes | 800 | 48 | 800/800 three times, 78,867-81,316 frames/s |
| the same, GB10 under systemd | 800 | 48 | 800/800, 78,266 frames/s |
| after killing a Mac agent mid-flight | 400 | 24 | 400/400 |
| after restarting the GB10 service | 400 | 24 | 400/400 |

The last two rows are worth their own line. The supervisor revived each agent
under a new pid — `launchctl` with the macOS grant intact, `systemctl` on the
Linux host — and the chain's next run passed with no lost frame. That is the
peer-restart invariant holding against a real process death rather than a
simulated one, on two different init systems.

### More than one node per agent

Every fleet run above placed exactly one node on each agent, which is not the
shape a real placement takes. Repeating the four addresses in the chain gives
each agent a second and third node, since a node is named by its position:

| Stages over four agents | Nodes per agent | Requests | Tokens each | Result |
| ---: | ---: | ---: | ---: | --- |
| 8 | 2 | 400 | 24 | 400/400, 31,634 frames/s |
| 8 | 2 | 800 | 48 | 800/800, 34,240 frames/s |
| 12 | 3 | 200 | 16 | 200/200, 16,201 frames/s |

Each node was created and loaded on its own before any inference ran. The
Windows agent's counters, the one machine reporting them, show the nodes as
genuinely separate — `stage-0`, `stage-4` and `stage-8` each with their own
arrivals, hops and completions, and `lost=0 orphaned=0` on all of them across
sixty-five readings.

`layers/service/tests/many_nodes.rs` now pins this by message rather than
leaving it to a run: four nodes over two agents, loaded individually, driven by
a chain that visits each machine twice, and then by two chains at once.

### The two queues, with the backend deliberately slow

Every fleet run above used `mock-instant`, which answers with no delay — good
for routing and ordering, and useless for the claim the two-tier queue exists
to support. Repeating the four-machine chain on the timed `mock` at 600
requests of 32 tokens, with the Windows agent reporting once a second:

| | Peak over 27 readings |
| --- | ---: |
| agent lanes (control + prefill + decode + response) | **0** |
| node depth | **473** |

600 chained requests in flight across four machines, the run taking 6.1s
against 0.5s for the same shape on the instant backend, and at no point was
anything waiting in front of the adapter. That is the attribution the design
was built for, measured rather than argued: work piles up on the node, where
the slowness is, and the agent's queue drains at agent speed. A deep lane
beside an idle node would mean the opposite, and would mean P4.

## 2026-08-16: the v6 core carries an inference between two machines

First run of the rewritten communication layer off one host. An agent on
`m42-server2` (192.168.0.29, Windows x64) and one on the development machine,
with a chain spanning both: `stage-0` local, `tail-1` remote. Every prefill hop
and every decode lap crossed the physical network.

| Shape | Requests | Tokens each | Result |
| --- | ---: | ---: | --- |
| remote node only | 300 | 16 | 300/300, 45,293 frames/s |
| chain over both machines | 400 | 24 | 400/400 three times, ~36,000 frames/s |
| chain over both, direct on 52001 | 400 | 24 | 400/400 three times, 33,409-36,679 frames/s |
| three stages, local → remote → local | 800 | 48 | 800/800 twice, 42,626-48,179 frames/s |

Four claims held on every run: every request answered, none failed, every
stream in order, one terminal per route.

The first two rows went through SSH forwards, on the reasoning that the remote
host's firewall did not admit the port and opening it is a change to that
machine rather than to this project. The port was the mistake, not the
firewall. An agent stands where a node process stood, and `52001-52008` is
already admitted on every host in this fleet for exactly that role; the layer
had been given a new range of its own for no reason. Bound to 52001 the agent
is reachable directly, and the last two rows are plain LAN sockets with no
wrapper. The throughput either way is the same, which says the tunnel was never
the constraint — but the configuration that needs no tunnel is the correct one,
and the third row crosses the network on every hop in both directions.

The detour produced the run's one instructive failure. With both agents
advertising `127.0.0.1`, the chain completed its prefill and then stopped after
exactly one token each: the last node's lap named `127.0.0.1:19312`, which on
the remote host is the remote host's own loopback. Nothing listened there. The
addresses in an envelope are absolute and a relay resolves nothing, so an
advertised address that is not reachable *by its peers* is not an address —
which is why `p4-agent` takes the advertised host as an argument, and why a
fleet must pass it.

Passing it exposed a second defect in the same seam: the argument was treated
as a host and the bound port appended to it, so `192.168.0.29:52001` became
`tcp://192.168.0.29:52001:52001` — an address that parses, resolves to nothing,
and is printed by a `P4_AGENT_READY` line. Both binaries now resolve the hint
through `Address::advertised`, which takes a host or a `HOST:PORT`, refuses
what is neither, and says out loud when a process has named itself something
only its own machine can reach.

Local verification behind these runs: three agents and a driver as separate
processes, chains of one, two and three stages, twelve runs of 800 requests at
48 tokens, 53,000-86,000 frames/s, no failing verdict.

## 2026-08-15: the terminal-stage access violation is corruption, not the churn or the oversubscription ratio

The 2026-08-13 entry below recorded `exit_code=3221225477` (`0xC0000005`) on a
second identical run and left the retry condition open: explain the access
violation before trying again. This reproduces it under a controlled sweep and
narrows what actually triggers it.

Same 35B MoE, 14/24 split (4080 first, 3090 last), 10-slot capacity declared at
load, `batch 256 / ubatch 128`, 2560-token context fixture. Three concurrency
shapes were run back to back on the same binary:

| arrivals | native peak | cohort admission waves | outcome |
| --- | ---: | --- | --- |
| 10 | 10 | 1 (no replacement) | pass, 119.4 tok/s |
| 20 | 10 | 2 (one full cohort replaced mid-run) | pass, 57.9 tok/s, `verdict=partial` |
| 30 | 10 | 3 | **crash**, terminal stage `exit_code=3221225477` |

`pipeline_occupancy` in the 20-arrival run shows `peak=10, in_flight=10` sustained
through a genuine cohort replacement -- ten of the twenty sessions completed and
the P4 admission gate (`config.capacity.gate(deployment_id)`, a real semaphore
sized from the declared `max_sequences`, gating `independent_loop` in
`layers/adapters/pipeline/src/infrastructure/listener/queue.rs`) let the next ten
in. That run passed. If cohort replacement itself were the trigger, it would
have failed at 20. It did not. The trigger scales with total window volume
processed, not with whether replacement happens at all: 20 requests worth of
windows survived, 30 did not.

Two prior dumps for `linker-node.exe` (2026-08-10, presumably captured by an
earlier session with default Windows Error Reporting already active) carry a
different exception: `0xC0000409` (`STATUS_STACK_BUFFER_OVERRUN`), raised inside
`ucrtbase.dll` with `__fastfail` parameter `0x7`
(`FAST_FAIL_FATAL_APP_EXIT`) -- the CRT's own heap/stack integrity check
tripping, not a bare dereference. Both dumps were parsed by hand (no `cdb` or
`windbg` on this machine; a small Node script walked the minidump module list
and exception stream directly) since no symbol server was available. Two
different exception codes from the same load shape -- a raw access violation on
one run, a CRT-detected corruption on another -- is the signature of an
out-of-bounds write landing in different places depending on heap layout, not
two separate bugs. Static review of the terminal-stage batched path
(`handle_forward_window` in `pipeline/batch-forward.inc`) did not find an
unchecked index: `window.count`, `sequence_id`, and every array sized from
either are bounds-checked before use. The corrupting write is somewhere deeper
-- inside `engine.decode_boundary_batch` or `engine.sample_tokens_batch` -- and
was not reached by this pass.

Today's crash produced no fresh dump. Windows Error Reporting never logged an
`AppCrash` event for it (checked via `Get-WinEvent` on the Application log), so
the child-process launch path this adapter uses does not get picked up by WER
the way the 2026-08-10 session's did, even with a `LocalDumps` registry key
registered for `linker-node.exe` for this run. Capturing a fresh, symbol-mapped
dump needs either the Windows SDK debugging tools (`cdb.exe`, not installed
here) or a JIT debugger registration, neither of which was set up before this
session's time budget on the chase ran out.

What is now safe to state: the defect is real, reproducible, and is memory
corruption in the terminal-stage batched decode/sampling path, not a scheduler
race tied to session churn. The practical boundary today is `native_peak =
declared max_sequences` with no oversubscription -- both tested shapes at that
line passed, and oversubscription is unsafe at an unknown ratio between 20 and
30 arrivals against 10 slots. Retrying past this point without a symbol-mapped
dump would be guessing at the same defect the 2026-08-13 entry already declined
to guess at.

## 2026-08-13: stage overlap, and why the first-stage layer count is a session budget

The pipeline never overlapped its stages. The credit ledger issues one credit
per sequence and the scheduler refills only while `in_flight < credit_limit`,
where the limit is `max_sequences`; a window as wide as the active cohort spends
every credit at once, so exactly one physical window was ever downstream. The
identity held at every session count, which is why the session sweep could not
expose it.

`scheduler_window_target` splits the cohort into `pipeline_stage_count` equal
windows. Windows stay full, so the full-batch refill gate that protects the
boundary transport is untouched. Paired back-to-back runs on one binary, depth
forced with `LINKER_PIPELINE_WINDOW_DEPTH`, 35B MoE, 500 tokens per request,
400-token prompts, `batch 256 / ubatch 128`, 4080 leading:

| layers / sessions | depth | window | aggregate | wall | stage0 wait | sum/wall |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 20/20, 48 | 1 | 48 | 210.4 tok/s | 114.1 s | 50.0 s | 97.5% |
| 20/20, 48 | 2 | 24 | 234.4 tok/s | 102.4 s | 8.0 s | 166.2% |
| 16/24, 64 | 1 | 64 | 213.8 tok/s | 149.7 s | 80.4 s | 97.6% |
| **16/24, 64** | **2** | **32** | **271.5 tok/s** | **117.9 s** | **33.7 s** | **163.5%** |

`sum/wall` is every stage compute plus terminal sampling over wall clock. 97.5%
is serial by construction; above 100% is only reachable when stages compute at
the same time. All runs finished with zero errors and exactly 500 tokens per
request. The gain is +11.4% at 20/20 with 48 sessions and +27.0% at 16/24 with
64.

Overlap then re-opened the layer axis, which the serial regime had closed. With
no overlap the wall tracked the sum of the stages, so moving layers only traded
one stage cost for another and 20/20 measured best. With overlap the wall tracks
the max, and the first stage's layer count is really a VRAM budget: KV costs
about 188 MiB per layer per 48 sessions, so 20 layers plus 48 sessions puts the
16 GiB card at about 15.4 GiB and 64 sessions at about 16.6 GiB. That is the
64-session cliff recorded below as 8.5 tok/s, and it is a spill, not a scheduler
limit. Dropping the first stage to 16 layers buys back the room:

| layers | sessions | first-stage VRAM | aggregate |
| --- | ---: | ---: | ---: |
| 20/20 | 48 | ~15.4 GiB | 234.4 tok/s |
| 20/20 | 64 | ~16.6 GiB | spills; 8.5 tok/s in the earlier sweep |
| **16/24** | **64** | **~13.3 GiB** | **271.5 tok/s** |

The balance point moved with it. At 64 sessions the first stage costs 5.05 s per
layer and the second 3.0 s per layer plus 39.8 s of terminal sampling, which
solves to 20 first-stage layers -- and 20 is exactly what does not fit. 16/24 is
the constrained optimum for this pair of cards, and the last stage is now the
bottleneck at 111.9 s of a 117.9 s wall.

Putting the 3090 first instead was measured and abandoned: the 4080 then holds
the terminal role, which carries the output head on top of 20 layers, and the
16 GiB card spilled. The first stage sat at 0% GPU while the terminal sat at
100%, and the run was stopped after 23 minutes against a 2-minute expectation.

Two negative results bound what to try next.

Backend sampling removes the cost it targets: terminal sampling fell from 27.9 s
to 0.1 s with 23,999 backend tokens. But the profile enlarges context
construction, the run reported `kv=7,526,154,240`, and the 16 GiB first stage
spilled -- its compute went from 85.5 s to 448.9 s, of which 378.0 s was
`llama_decode` submit, and throughput fell to 50.3 tok/s. The blocker is that
reservation, not the 256-output ceiling recorded earlier.

Multi-token prefill for concurrent cohorts crashed. Removing the
`sessions.size() == 1` term made prefill windows 120 tokens instead of 24, cut
first-stage prefill compute from 32.0 s to 18.6 s and mean TTFT from 33.9 s to
21.1 s, and moved aggregate throughput -1.4%. The second run of that identical
configuration killed the terminal stage with `exit_code=3221225477`
(`0xC0000005`) and `stop_requested=false`, and every in-flight request failed
with a closed stage control pipe. The term was reinstated. Its prize is latency,
not throughput, and any retry has to explain the access violation first.

Absolute tok/s on this host is noisy: the first stage runs on the display GPU,
and two depth-1 runs of the identical 20/20 configuration measured 88.3 s and
61.3 s of first-stage compute while the second stage reproduced within 2%.
Compare depths inside one session and read `sum/wall` and downstream wait, which
are ratios and survive the clock noise.


## 2026-08-13: 246 tok/s, and stage cost follows the role rather than the device

Same 35B MoE, 32 concurrent requests capped at 1000 tokens, 400-token prompts,
`batch 256 / ubatch 128`, measured after the terminal-clearing fix below.

| Run | placement | aggregate | vs single session |
| --- | --- | ---: | ---: |
| single session | 20/20, 3090 first | 66.0 tok/s | 1.00x |
| `d3`, before any of this | 16/24, 4080 first | 74.3 tok/s | 1.13x |
| baseline | 20/20, 3090 first | 196.2 tok/s | 2.97x |
| terminal fix | 20/20, 3090 first | 206.4 tok/s | 3.13x |
| stage swap | 20/20, **4080 first** | **246.1 tok/s** | **3.73x** |

The swap is the measurement that reinterprets the earlier stage timings:

| placement | first-stage compute | last-stage compute |
| --- | ---: | ---: |
| 3090 first | 3090 **64.2 s** | 4080 39.5 s |
| 4080 first | 4080 **64.6 s** | 3090 31.7 s |

Whichever card leads costs about 64 s and whichever trails costs 32-40 s, so
the per-layer cost belongs to the stage role, not to the device. The earlier
reading that the 4080 was 1.62x faster per layer was wrong; the last stage is
simply cheaper than the first, and the 3090 is the faster card once it holds
the role that shows it, at 31.7 s against the 4080's 39.5 s.

It also closes the `d3` question. A 4080 in the first position cost 311.5 s of
compute then and 64.6 s now, so the 4.75x penalty was the reservation defect
against a 16 GiB card, not the role or the device.

At 3.23 s per first-stage layer and 1.59 s per last-stage layer, an even split
of work looks like 13/27 rather than 20/20. Measuring it says otherwise.

| first-stage layers (4080) | aggregate | wall | stage compute | first-stage wait |
| ---: | ---: | ---: | --- | ---: |
| 13 | 229.1 tok/s | 137.8 s | 49.7 / 51.2 | 85.9 s |
| **20** | **246.1 tok/s** | 129.0 s | 64.6 / 31.7 | 62.0 s |
| 24 | 9.1 tok/s | 3519.1 s | 3283.5 / 72.8 | 233.3 s |

Balancing the two stages made it slower. The wall tracks
`first compute + last compute + about 32 s`, so the stages do not overlap at
all and what matters is the sum, not the balance: moving seven layers to the
last stage saved 14.9 s of first-stage compute and added 23.9 s of waiting.
The 24-layer row is the other bound — 13,586 MiB of weights plus buffers
brushes the 4080's 16 GiB and the driver spills to host memory, which costs
50x. Twenty layers, about 12.1 GiB, is the practical ceiling for a 16 GiB
first stage.

Session count is the other axis the fix opened, and it is close to saturation:

| sessions | aggregate | wall | stage compute | first-stage wait |
| ---: | ---: | ---: | --- | ---: |
| 32 | 246.1 tok/s | 129.0 s | 64.6 / 31.7 | 62.0 s |
| **48** | **254.6 tok/s** | 185.1 s | 94.4 / 43.8 | 87.3 s |
| 64 | 8.5 tok/s | 7458.8 s | 7025.7 / 330.0 | 428.6 s |

Half again as many sessions bought 3.5%, and a third again after that falls
off the same cliff as 24 layers: the KV growth pushes the 4080 past 16 GiB and
the driver spills. Both axes therefore end at the same wall, the 16 GiB first
stage, and the measured optimum for this pair of cards is 48 sessions with
20/20 and the 4080 leading, at 254.6 tok/s and 3.86x the single session.

Per-step cost now grows nearly linearly with batch width, so what is left is
the 47% of wall clock the first stage spends waiting for the round trip, which
only stage overlap can recover. An earlier attempt at that — relaxing the
microbatch refill gate — broke the boundary transport, but it was made while
the first stage still carried the 10.6 GiB reservation, so it is worth
retrying now that the device has room.

## 2026-08-13: a stale graph terminal was reserving the unowned layers

`llm_graph_result::reset()` cleared `t_linkcpp_inputs`,
`t_linkcpp_input_nodes`, `t_linkcpp_outputs` and `linkcpp_tensor_layers`, but
not `t_linkcpp_terminals`. A non-final stage expands its terminals into the
freshly pruned graph, and a terminal left from an earlier build still depends
on every layer of the full graph, so it dragged the layers the stage does not
own back in. Those weights are `no_alloc` metadata tensors, so once reachable
the scheduler reserved their bytes as compute buffer.

An audit added to `apply_linkcpp_stage`, now behind
`LINKER_STAGE_REACH_AUDIT`, walks the final graph and reports every reachable
tensor with no buffer whose layer falls outside the stage range. On the 35B
MoE with `layers=[0,20)`:

| Build | unowned tensors | unowned MiB | holder |
| --- | ---: | ---: | --- |
| first | 0 | 0.00 | none |
| second | 269 | 8,525.25 | `ffn_moe_down-34` (`MUL_MAT_ID`, layer 34) |
| third | 340 | 10,837.41 | same |

The accumulation across builds is the leak, and 10,837.41 MiB matched the
stage's 10,847.68 MiB compute buffer. The final stage never expands terminals
and reported zero throughout, which is why only non-final stages carried the
term.

Adding `t_linkcpp_terminals.clear()` to `reset()` removes it:

| First stage `[0,20)` | before | after |
| --- | ---: | ---: |
| reachable unowned tensors | 340 | **0** |
| `CUDA0` compute buffer | 10,847.68 MiB | **15.27 MiB** |
| stage total | 22,169 MiB | **11,337 MiB** |

That returns 10.6 GiB to the first-stage device. A node-array aliasing
hypothesis was tested first — snapshotting the order before
`ggml_graph_clear` — and rejected: the audit reported byte-identical numbers.

## 2026-08-13: placement alone takes 32 sessions from 74 to 196 tok/s

The first multi-session measurement on the placement the single-session run
validated: `0:20` on the 3090 as first stage, `20:40` on the 4080 as last, 32
concurrent requests capped at 1000 tokens, 400-token Korean prompts,
`batch 256 / ubatch 128`. Every earlier throughput run used `0:16` on the
4080 as first stage.

| | `d3` (4080 first, 16/24) | this run (3090 first, 20/20) |
| --- | ---: | ---: |
| accepted / done / errors | 32 / 32 / 0 | 32 / 32 / 0 |
| wall clock | 417.1 s | **160.3 s** |
| **aggregate throughput** | 74.3 tok/s | **196.2 tok/s** |
| per-stream throughput | 2.32 tok/s | 6.13 tok/s |
| first-stage compute | 311.5 s | **65.5 s** |
| last-stage compute | 56.7 s | 41.0 s |
| first-stage downstream wait | 103.2 s | 91.5 s |
| native occupancy | peak 32 | `active=32 in_flight=32 peak=32` |

Aggregate throughput is 2.97x the 66 tok/s single session, so the objective in
the handoff's section 0 is met for the first time. The gain is entirely
placement: first-stage compute fell 4.75x for four *more* layers.

That also corrects an earlier reading. The "front stage costs about eight times
the last stage per layer" figure was not a property of being the front stage.
It was the 4080 holding the front role while a non-final stage reserves the
whole model's 22.17 GiB against its 16.0 GiB of VRAM. Move the front role to a
24 GiB card and the term disappears. The memory defect and the throughput
collapse are the same defect seen from two sides.

Headroom remains and it is now the pipeline bubble, not admission. With all 32
sequences admitted and alive in the native scheduler, `nvidia-smi` sampled
through generation gives the 3090 a 32.5% mean and the 4080 53.4%. Stage
compute sums to 106.5 s against a 160.3 s wall, and the first stage spends
91.5 s waiting downstream. If the stages overlapped, the wall would approach
the slower stage's 65.5 s, which is about 480 tok/s.

## 2026-08-12: three all-3090 stages reach 8-way concurrency, then the middle stage faults

Dropping the 16 GiB 4080 and running three RTX 3090 stages — local plus the two
on `192.168.0.29` — follows directly from the previous entry: a non-final stage
needs the whole model's footprint, about 22.17 GiB, which a 24 GiB card can
hold and a 16 GiB card cannot. Artifacts are under
[`target/three-node-20260812/`](../target/three-node-20260812/).

The topology is sound. All three stages loaded, `P4_HEALTH` reported ready, and
the first stage logged
`op=wavefront active=8 in_flight=8 peak=8 limit=8 capacity=8`.

That is the first concurrency observed since the arrival-axis regression: the
four-node run and every run after `p4_max_inflight` was dropped from
`node_spec` reported `peak=1`. It is not a repository first — `c1c` reached
`peak=16` and both `c3b` and `d3` reached `peak=32` before the regression. What
it establishes is narrower and still useful: the fix restores the width those
earlier runs had, across hosts, and the native scheduler was never the tier
that refused concurrency.

Then `remote-3090-a`, `stage_index=1`, exited with `exit_code=3221225477`
(`0xC0000005`, access violation) and all eight requests failed with
`pipeline stage control pipe closed`. This is the same fault class seen earlier
on an asymmetric local placement, now on a 24 GiB card, so card size alone does
not explain it. A three-stage group has two non-final stages, each reserving
about 22.17 GiB of a 24 GiB card and leaving roughly 1.8 GiB for KV cache,
context and fragmentation. The middle stage is the one that takes both the
`cut_at(begin)` input path and the `cut_at(end)` output path.

Iteration then stopped for an environmental reason worth recording. The remote
supervisor does not survive its child vanishing, which is the standing defect,
and after that first crash it would no longer stay up at all: it starts, serves
`/api/runtime` with 200, and exits within about two minutes leaving nothing in
`supervisor.log`, `launcher.out.log` or `launcher.err.log`. Restarting the
remote agent and adapter cleared their stale NodeSlot state but not this. No
native stage log was captured for the crash because the sink dies with the
supervisor; `LINKER_NATIVE_LOG_DIR` was set on the remote for later attempts
and produced no files for the same reason.

## 2026-08-12: a non-final stage reserves the weight of the layers it does not own

Loads of `Ornith-1.0-35B-UD-Q5_K_S.gguf` (qwen35moe, `n_expert=256`,
`n_expert_used=8`, `n_embd=2048`) on a local 3090 + 4080, layers `0:16` and
`16:40`, driven through the owned E2E with one request capped at one token.
Only the load path matters here. Logs are under
[`target/graph-buffer-sweep-20260812/native-logs/`](../target/graph-buffer-sweep-20260812/native-logs/).

| parallel | `n_ctx` | `n_ubatch` | first `CUDA0` | last `CUDA0` | first `CUDA_Host` |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 1,024 | 128 | **13,124.56 MiB** | 124.77 MiB | 125.73 MiB |
| 8 | 8,192 | 128 | **13,194.40 MiB** | 124.25 MiB | 134.22 MiB |
| 32 | 32,768 | 128 | **13,461.90 MiB** | 425.27 MiB | 163.33 MiB |
| 1 | 1,024 | 16 | **13,110.96 MiB** | 15.53 MiB | 16.79 MiB |

Both stages receive identical `llama_context_params`. The last stage scales
with context and micro-batch exactly as expected. The first stage does not
scale with any of them: 32x the context, 32x the sequences and an eighth of the
micro-batch all leave it within 350 MiB of the same ~13.1 GiB. It is a fixed
reservation, present with a single sequence and a 1,024-token context, and it
is 105x the last stage's allocation under the same parameters. Across 16
layers that is 819 MiB of compute buffer per layer against the last stage's
0.6 MiB.

The same sweep on `Qwen2.5-1.5B-Instruct-Q8_0` (dense) shows no such term —
first `CUDA0` moves 15.71 / 17.46 / 43.08 MiB across the same parallel values
while the last stage holds 74.94 MiB. The reservation is specific to the MoE
model on the first-stage code path.

This falsifies the earlier reading recorded in the pipeline throughput handoff,
which attributed the first stage's buffer and its `graph splits = 2` to the
boundary hidden-state copy and called it normal. At `n_ubatch=16` the boundary
frame is 64 KiB while `CUDA_Host` is 16.79 MiB and `CUDA0` is still 13.1 GiB,
so the copy cannot account for either. Because the term is independent of
batching, no admission, queue or scheduler change can remove it.

Two consequences follow. The first-stage device loses 13.1 GiB before any
weight is placed, which is why a 16 GiB 4080 in the first position is planned
with very few layers, and it is the leading suspect for the first stage costing
roughly eight times the last stage per layer during generation.

### The compute buffer is the weight of the layers the stage does not own

Holding the model, `parallel=1`, `n_ctx=1024` and `n_ubatch=16` fixed and
moving the stage boundary. Stage 0 ran on the 4080 and stage 1 on the 3090 in
every row; process-to-range mapping is taken from the supervisor spawn records,
not inferred from file order.

| stage 0 layers | stage 0 model | stage 0 compute | sum | stage 1 layers | stage 1 model | stage 1 compute |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 0:4 | 2,264.35 | 19,900.82 | 22,165 | 4:40 | 20,996.45 | 15.53 |
| 0:16 | 9,057.39 | 13,110.96 | 22,168 | 16:40 | 14,203.41 | 15.53 |
| 0:30 | 16,986.11 | 5,185.43 | 22,172 | 30:40 | 6,274.69 | 15.53 |

One layer of this model weighs 566.2 MiB, and every model-buffer figure above
is that figure times the layers the stage owns. The first stage's compute
buffer is then `566.2 MiB x (40 - owned) - 478 MiB`: it is the weight of the
layers it does not own. Model plus compute is 22.17 GiB in all three rows, so
splitting the model changes only how the first stage's memory is labelled, not
how much it takes. The last stage carries no such term and sits at 15.53 MiB
whether it holds 10 layers or 36.

The reservation is real, not a reported estimate. Sampling `nvidia-smi` through
the `0:4` run shows the 4080 holding 15.7 GiB of its 16.0 GiB for the whole
group lifetime while owning four layers, or 2.26 GiB of weights.

The dense `Qwen2.5-1.5B` control does not show it: its first stage stays at
15-43 MiB rather than reserving its unowned layers. The term therefore belongs
to the MoE expert tensors of unowned layers on the non-final stage path, where
`llama_model_loader` substitutes a metadata tensor for every tensor outside
`[linkcpp_layer_begin, linkcpp_layer_end)` and `apply_linkcpp_stage` is
expected to prune the corresponding nodes. The final stage proves the pruning
works there; the first stage shows it does not, and its debug line reports
`inputs=0 outputs=1`, so the earlier guess that a large output cut-set retained
per-layer scratch is ruled out.

Consequence: pipeline splitting currently gives a non-final stage almost no
memory relief. A four-stage group has three such stages, which is why the
planner gives the 16 GiB 4080 six layers in the four-node plan.

## 2026-08-12: four-node run completed 16/16 while every session ran alone

Local RTX 3090 + RTX 4080 and remote RTX 3090×2, four stages
`[0,11] [11,17] [17,29] [29,40]`, 16 concurrent requests capped at 16 tokens.
Artifacts are under
[`target/four-node-current-20260812/`](../target/four-node-current-20260812/).

| Observation | Result |
| --- | --- |
| planned / accepted / `DONE` / error | 16 / 16 / 16 / 0 |
| `INGRESS_ACCEPTED` latency mean | 1.852 ms — async acceptance works |
| first token mean / max | 18,082 ms / 36,061 ms |
| wall clock / generated tokens | 36,731 ms / 256 |
| aggregate throughput | **6.97 tok/s** |
| declared adapter capacity | `max_sequences=16 source=stage_plan` |
| adapter batch sizes | `requests=1`, all 16 of 16 batches |
| native occupancy | `active=1 in_flight=1 peak=1 limit=16 capacity=16` |

The three tiers agreed on a width of 16 and the run still executed one session
at a time. The cause is upstream of both the adapter and the native scheduler:
`run-pipeline-e2e.mjs` stopped sending `p4_max_inflight` when the plan-width
copy was removed, so each NodeSlot fell back to its one-permit default. The
async relay waits for that permit and holds it to the terminal response, so the
Agent released one execution at a time, the adapter's 5 ms coalescing window
had nothing to merge, and the native scheduler never saw a second sequence.

Two corrections followed. The runner now declares the arrival axis explicitly
(`P4_AGENT_SLOT_WIDTH`, defaulting to `concurrent_requests`), and every summary
carries `throughput.aggregate_tps` with an admission verdict, so a run that is
correct but serialised reports `serialized` instead of passing silently. Rerun
this configuration before quoting any four-node throughput number; the table
above measures the defect, not the topology.

## 2026-08-11: P4B1 v5 persistent-route and stock llama.cpp continuous-batch proof

The stock E2E launched unchanged `llama-server.exe` with `parallel=8`,
`ctx-size=8192` (1024 per slot), `batch-size=2048`, and `ubatch-size=512`.
One Node.js process used two logical layers of multiplexing: all eight external
ingress streams shared one Agent link, and all eight Agent execution routes
shared one adapter link. The adapter reported its independent bounded admission
as `max_inflight=256`, `max_queued=1024`.

| Observation | Result |
| --- | --- |
| requested / accepted / text / `DONE` / error | 8 / 8 / 8 / 8 / 0 |
| session IDs | eight distinct controller-issued IDs |
| completion order | `4, 0, 2, 3, 6, 7, 5, 1`; no batch barrier |
| llama-server slot evidence | slots `0..7` each logged `processing task` |
| generated text | one `안녕하세요!`; seven `안녕하세요 (Annyeonghaseyo)` |
| adapter errors | none |

The raw server proof is
[`llama-server-20260811170006.err.log`](../target/real-e2e/llama-server-20260811170006.err.log),
and the eight adapter admissions are in
[`p4-llamacpp-20260811170006.log`](../target/real-e2e/p4-llamacpp-20260811170006.log).
This proves that the enlarged adapter scheduler can fill all configured stock
server slots while preserving independently completed P4 streams. It does not
claim that eight slots or these batch sizes are optimal for another model,
device, or backend.

A follow-up run used the explicit state-machine settings `max_batch=8` and
`partial_linger_ms=1000`. All eight routes again emitted text and `DONE`; the
adapter accepted eight executions and llama-server logged eight task launches.
The deterministic scheduler tests separately held the one-second linger and
proved both transitions: the second item completed a two-item full batch and
dispatched it after about 20 ms, while an execution-completion cycle hint
released a one-item partial batch without waiting for the remaining linger.
Evidence: [`p4-llamacpp-20260811170738.log`](../target/real-e2e/p4-llamacpp-20260811170738.log)
and [`llama-server-20260811170738.err.log`](../target/real-e2e/llama-server-20260811170738.err.log).
For stock HTTP the cycle hint is request completion, not direct GPU kernel-cycle
telemetry; the scheduler exposes a separate hint/heuristic seam for that future
improvement.

## 2026-08-10: abstract Agent ingress-credit correction

The 256-session experiments showed that P4 transport retained every valid
request/response pair and that CUDA-specific work was concentrated below the
adapter boundary. The selected protocol improvement is therefore not another
llama.cpp/CUDA change: the Agent now acquires the generic NodeSlot execution
credit before it emits `INGRESS_ACCEPTED`. A saturated slot produces only
`ERROR`, so an accepted ingress is no longer a promise that can immediately be
withdrawn by admission failure.

| Verification | Result |
| --- | --- |
| ready credit | mock adapter observes `EXECUTE`; client receives `INGRESS_ACCEPTED` then `DONE` |
| saturated credit | client receives `ERROR` and no `INGRESS_ACCEPTED` |
| backend assumptions | none; test uses only P4 frames and a local TCP mock adapter |
| wire revision | unchanged (`P4B1 v3`) |

This does not claim a CUDA throughput gain. It makes multi-controller ingress
backpressure truthful at the Agent boundary and applies unchanged to llama.cpp,
vLLM, SGLang, CPU, CUDA, HIP, Vulkan, Metal, or another adapter.

## 2026-08-09: native microbatch 256×500 success

The native first-stage scheduler now groups ready sequence windows into a
physical `min(batch, ubatch)` microbatch and waits for a complete replacement
batch before refilling an in-flight wavefront. The proof used an isolated CUDA
runtime pack (build ID suffix `498d73d6b9ad`) on a temporary supervisor at port
`18083`; it did not replace the active host runtime.

| Artifact | Contents |
| --- | --- |
| [plan-20260809161709.json](../target/pipeline-e2e/plan-20260809161709.json) | 256 persisted Korean prompt requests, `max_tokens=500`. |
| [trace-20260809161709.jsonl](../target/pipeline-e2e/trace-20260809161709.jsonl) | Every P4 ingress, token, and terminal response. |
| [summary-20260809161709.json](../target/pipeline-e2e/summary-20260809161709.json) | Counts and latency distributions. |
| [report-20260809161709.md](../target/pipeline-e2e/report-20260809161709.md) | All 256 prompt/final-output pairs. |

| Measurement | Observed value |
| --- | ---: |
| planned / accepted / first token / `DONE` / `ERROR` | 256 / 256 / 256 / 256 / 0 |
| P4 text events / aggregate event rate | 56,975 / 550.206 events/s |
| concurrent wall-clock | 103,552.076 ms |
| accepted / TTFT / `DONE` p95 | 131.114 / 8,365.671 / 102,766.373 ms |
| stage-local observed maximum batch size | 128 on both stages |
| pipeline peak / terminal credit | 256 / 0 (`issued = returned = 72,093`) |

The prior 256×500 artifact took 302,396.880 ms for 57,439 events. This new run
is a comparable harness result, not a controlled repeated experiment, but it
reduced wall-clock by 65.8% and raised aggregate event rate by about 2.9× while
preserving all request/response pairs. The runtime is documented by
[`apps/llama` scheduler internals](../../llama/docs/internals.md#multi-token-prefill).

## 2026-08-09: 500-token concurrency sweep, 100/50/10/2/1

Five fresh two-GPU Pipeline runs used the same model, 1,024 context tokens per request, a 500-token output cap, deterministic prefixes of one prompt family, and full plan/trace/report artifacts. Every planned request was accepted, streamed text, and ended in `DONE`; no run had a P4 `ERROR`. The complete linked evidence is [sweep-20260809150430.md](../target/pipeline-e2e/sweep-20260809150430.md) and [sweep-20260809150430.json](../target/pipeline-e2e/sweep-20260809150430.json).

| Concurrent sessions | P4 events/s | TTFT p50 / p95 ms | `DONE` p50 / p95 ms | Natural stop / length cap |
| ---: | ---: | ---: | ---: | ---: |
| 100 | 247.311 | 3,437.945 / 3,592.087 | 18,761.322 / 25,505.134 | 99 / 1 |
| 50 | 243.363 | 2,132.378 / 2,202.624 | 7,869.504 / 9,268.574 | 50 / 0 |
| 10 | 178.148 | 1,101.835 / 1,117.138 | 1,882.353 / 2,068.411 | 10 / 0 |
| 2 | 89.067 | 452.436 / 506.389 | 829.396 / 880.565 | 2 / 0 |
| 1 | 14.705 | 433.752 / 433.752 | 2,440.899 / 2,440.899 | 1 / 0 |

The observed aggregate-event-rate peak is 100 sessions, but it is only 1.62% above 50 sessions while its p95 TTFT is 63.1% higher and p95 completion latency is 175.2% higher. For this model and host, use 50 as the current balanced batch setting, use 100 only when maximizing aggregate throughput outweighs tail latency, and use 10 or fewer for latency-sensitive traffic. The rise from 1→50 is consistent with better GPU utilization from concurrent sequence work; the 50→100 plateau with rising latency is consistent with a saturated shared Pipeline/native execution path and increased queueing. This is one deterministic sweep with different prompt-prefix sizes and naturally varying output lengths, so repeat it before turning the operating guidance into a hard policy.

## 2026-08-09: scripted 256×500 request/response proof

The owned Pipeline E2E first generated and persisted exactly 256 input requests, then opened all 256 ingress streams concurrently with `max_tokens=500`. It persisted the frame-level response trace and generated the report only after all streams terminated. Plan and trace `request_id` sets both contain 256 unique IDs and match exactly.

| Artifact | Contents |
| --- | --- |
| [plan-20260809144839.json](../target/pipeline-e2e/plan-20260809144839.json) | Exact 256 prompts and fixed sampling request fields before ingress. |
| [trace-20260809144839.jsonl](../target/pipeline-e2e/trace-20260809144839.jsonl) | Every `INGRESS_ACCEPTED`, `TOKEN`, and terminal `DONE`/`ERROR` per request. |
| [summary-20260809144839.json](../target/pipeline-e2e/summary-20260809144839.json) | Machine-readable count and latency distributions derived from the trace. |
| [report-20260809144839.md](../target/pipeline-e2e/report-20260809144839.md) | All 256 input/final-output pairs and session-level first-token/completion latency. |

| Measurement | Observed value |
| --- | ---: |
| planned / accepted / first token / `DONE` / `ERROR` | 256 / 256 / 256 / 256 / 0 |
| maximum output tokens per request | 500 |
| natural stop / output-length stop | 228 / 28 |
| P4 text events / native generated tokens | 57,439 / 57,439 |
| concurrent request wall-clock | 302,396.880 ms |
| model load to ready binding | 20,968.571 ms |
| native KV allocation | 7,985,976,320 B |
| total / per-request context | 262,144 / 1,024 tokens |

| Per-request event latency | min | mean | p50 | p95 | p99 | max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| ingress accepted | 128.288 | 133.993 | 133.745 | 138.224 | 138.731 | 138.848 |
| first text token | 532.918 | 7,656.837 | 7,702.827 | 8,146.075 | 8,183.449 | 8,190.316 |
| `DONE` | 917.304 | 166,673.330 | 144,088.042 | 302,172.541 | 302,173.294 | 302,173.666 |

The report preserves the exact prompt/final-streamed-text relationship. Text answer quality is separate from transport correctness; 28 responses stopped at the configured 500-token cap and 228 stopped naturally.

## 2026-08-09: traceable `parallel=256` request/response proof

The maximum-native-capacity test was repeated with per-session evidence. The runner wrote the submitted `INGRESS_SUBMIT` data, the `INGRESS_ACCEPTED`, every `TOKEN`, terminal `DONE`/`ERROR`, and the joined final streamed text for each concurrent request. The result contains 256 JSONL rows: 256 accepted, 256 `DONE`, zero `ERROR`, and 4,084 P4 text events.

| Artifact | Contents |
| --- | --- |
| [trace-20260809144038.md](../target/pipeline-e2e/trace-20260809144038.md) | All 256 prompt → final-text pairs, session IDs, and terminal reasons. |
| [trace-20260809144038.jsonl](../target/pipeline-e2e/trace-20260809144038.jsonl) | Full per-session P4 request/response event sequence, including every streamed text chunk. |
| [client-20260809144038.log](../target/pipeline-e2e/client-20260809144038.log) | Lifecycle, aggregate timing, resource draft, and trace artifact paths. |

| Input prompt | Final streamed text | Terminal |
| --- | --- | --- |
| `Rust 언어를 한국어로 간단히 설명해. 핵심 특징을 한 문장으로 포함해.` | `Rust은 고성능이고 안전한 프로그래밍 언어` | `length`, 16 |
| `C 언어를 한국어로 간단히 설명해. 핵심 특징을 한 문장으로 포함해.` | `C 언어는 간단하고 효율적인 프로그래밍 언` | `length`, 16 |
| `C++ 언어를 한국어로 간단히 설명해. 핵심 특징을 한 문장으로 포함해.` | `C++는 객체지향 언어로, 데이터와 함수를 분리` | `length`, 16 |
| `C# 언어를 한국어로 간단히 설명해. 핵심 특징을 한 문장으로 포함해.` | `C#은 객체지향 언어로, 변수와 함수를 쉽게 사용` | `length`, 16 |
| `Java 언어를 한국어로 간단히 설명해. 핵심 특징을 한 문장으로 포함해.` | `Java는 객체지향 프로그래밍 언어로, 복잡` | `length`, 16 |

Each request used the 16-token output cap, so `reason=length` and incomplete sentences are expected. This is complete transport evidence, not answer-quality evidence.

## 2026-08-09: native Pipeline capacity ceiling and `parallel=256` proof

The planner estimated that `parallel=1000` with `1,024` context tokens per request would fit GPU memory, but the measured native load failed before KV allocation: `llama_init_from_model: failed to initialize the context: n_seq_max must be <= 256`. This is a native Pipeline/llama runtime ceiling, not a P4 controller or Agent queue result, and means VRAM cannot establish a higher usable session count for the currently linked binary.

The owned E2E then set the actual maximum, `parallel=256`, `ConcurrentRequests=256`, `P4_AGENT_MAX_INFLIGHT=256`, and `262,144` total context tokens. It generated 256 distinct Korean prompts in the form `<language> 언어를 한국어로 간단히 설명해. <style>` from programming and human language names. Every stream emitted text and reached `DONE`; no session was queued for later execution.

| Measurement | Observed value |
| --- | ---: |
| failed native probe | `parallel=1000`, `n_seq_max <= 256` |
| completed concurrent ingress requests | 256 / 256 |
| distinct prompts | 256 |
| output cap per request | 16 tokens |
| aggregate P4 streamed text events | 4,083 |
| concurrent request wall-clock | 22,631.502 ms |
| aggregate streamed-event rate | 180.41 events/s |
| native KV allocation | 7,985,976,320 B |
| total / per-request context | 262,144 / 1,024 tokens |
| model load to ready binding | 20,890.301 ms |

The final native request summary is a last-request sample, not a percentile: 57 prompt tokens, 56 prefill tokens, 8,414.878 ms native TTFT, and 2,741.520 ms queue wait. The runner removed its exact runtime group and P4 processes. Raw artifact: `target/pipeline-e2e/client-20260809141921.log`.

## 2026-08-09: native Pipeline `parallel=20` simultaneous ingress

Command:

```powershell
.\scripts\run-pipeline-e2e.ps1 -P4ListenPort 29221 -Prompt '러스트에 대해 한국어로 설명하라.' -MaxTokens 32 -Parallel 20 -ConcurrentRequests 20
```

The planner and Pipeline runtime both received `parallel=20`. Because native context is divided among the parallel slots, the E2E calculated a total context of `20,480` tokens to preserve `1,024` tokens per request. The Agent gave the bound NodeSlot `p4_max_inflight=20`, opened 20 ingress streams concurrently, and every stream emitted text and reached `DONE`.

| Measurement | Observed value |
| --- | ---: |
| concurrent ingress requests completed | 20 / 20 |
| output cap per request | 32 tokens |
| aggregate P4 streamed text events | 637 |
| concurrent request wall-clock | 2,751.773 ms |
| aggregate streamed-event rate | 231.49 events/s |
| native KV allocation | 623,924,224 B |
| context total / per request | 20,480 / 1,024 tokens |

The final post-stress request also completed with `DONE(reason=length)`. The script removed its exact runtime group and P4 processes. Raw artifact: `target/pipeline-e2e/client-20260809140406.log`.

## 2026-08-09: Tokio Agent admission and two-controller proof

`p4-agent` accepted ingress through a Tokio multi-thread I/O runtime at `127.0.0.1:29221`; its relay pool is bounded by `P4_AGENT_MAX_INFLIGHT=64` by default. The owned Pipeline E2E created two distinct NodeSlots concurrently from two ControllerInstances (`pipeline-2gpu` and a marker slot), then loaded only `pipeline-2gpu` as a two-GPU Pipeline deployment. While one execution held that slot's default `p4_max_inflight=1` permit, a second execution received an immediate admission `ERROR`; it was not queued. The held stream completed, and the requested 500-token-cap Korean Rust prompt then completed with 267 streamed text events and `reason=stop`.

| Measurement | Observed value |
| --- | ---: |
| concurrent controllers / created NodeSlots | 2 / 2 |
| model-load to ready binding | 20,947.912 ms |
| P4 ingress to first text event | 149.015 ms |
| P4 ingress to `DONE` | 2,829.497 ms |
| native stage 0 / stage 1 compute | 1,293.745 / 176.339 ms |
| stage 0 → 1 hidden state | 311 frames / 1,008,884 B |
| stage 1 → 0 sampled token | 272 frames / 3,185 B |

The final adapter error log was empty and the script removed its owned P4 processes and Pipeline group. Raw artifact: `target/pipeline-e2e/client-20260809135821.log`.

## 2026-08-08: P4B1 v3 agent inventory, NodeSlot, binding, and ingress

The owned stock E2E started `p4-agent` at `127.0.0.1:29111` before the self-registering stock adapter. The external Node.js controller received `HARDWARE_REPORT` with one registered adapter and two NVIDIA GPUs, created model-free `gpu-0`, bound the configured model at generation `1`, then submitted ingress without a session ID. ControllerProcessor returned `nodejs-controller-example-session-1`, streamed `12` events, and removed only the binding; the process-owned llama-server remained available.

The owned two-GPU Pipeline E2E used the same lifecycle at `127.0.0.1:29211`: one registered adapter, two discovered GPUs, `NODE_CREATED(ready)` for `pipeline-2gpu`, progress and `DRAFT_REPORT`, `MODEL_BOUND(generation=1)`, ingress-issued session, `16` streamed events, and `MODEL_UNBOUND`. The model deployment was independently deleted in cleanup while the NodeSlot contract remained valid.

| Measurement | Observed Pipeline value |
| --- | ---: |
| GGUF model / layer / KV bytes | 1,640,622,080 / 1,392,656,384 / 15,619,072 B |
| native decode / P4 streamed events | 16 / 16 |
| time to first token | 328.686 ms |
| stage 0 -> 1 hidden-state traffic | 52 frames / 168,688 B |
| stage 1 -> 0 sampled-token traffic | 16 frames / 167 B |
| host-observed average transfer rate | 52,209,223.15 B/s |

This proves agent-side lifecycle separation and ingress relay, not durable controller registry, public authentication, or a throughput guarantee.

## 2026-08-08: P4B1 v2 adapter-owned execution options

`cargo test --workspace` passed the P4B1 v2 `EXECUTE` codec round trip and both adapter policy tests: stock llama.cpp preserves a backend option such as `top_p` while restoring P4-owned `model` and `stream`; Pipeline copies `top_p`, `top_k`, and `seed` while omitting an unsupported `repeat_penalty`.

The owned stock E2E used `run-inference.mjs`, whose `infer()` call supplied `options: { top_p: 0.9, top_k: 20, seed: 7 }`, at agent listener `127.0.0.1:29101`. The model streamed `12` P4 token events and ended with `P4_DONE`.

The owned two-stage Pipeline E2E used the same options at `127.0.0.1:29201` with prompt `러스트에 대해 설명하라.` and a `16` token cap. The current Pipeline parser accepted its supported subset and returned:

| Measurement | Observed value |
| --- | ---: |
| GGUF model / layer / KV bytes | 1,640,622,080 / 1,392,656,384 / 15,619,072 B |
| P4 streamed text events / native decode tokens | 16 / 16 |
| finish reason | `length` |
| time to first token | 342.563 ms |
| stage 0 -> 1 hidden-state traffic | 52 frames / 168,688 B |
| stage 1 -> 0 sampled-token traffic | 16 frames / 167 B |
| host-observed average transfer rate | 49,731,132.08 B/s |

The exact temporary Pipeline group and spawned P4 relay processes were removed by the scripts' `finally` blocks. This validates v2 transport and adapter filtering, not model-answer quality or a universal llama.cpp option set.

## 2026-08-08: real two-stage Pipeline request

Command:

```powershell
.\scripts\run-pipeline-e2e.ps1 -Prompt '러스트에 대해 설명하라.' -MaxTokens 128
```

The current `S:\models` inventory contained one GGUF file, `Qwen2.5-1.5B-Instruct-Q8_0.gguf`; this run did not select among multiple model sizes. The test created one P4 controller identity and routed its logical `pipeline-2gpu` node to a native Pipeline group with these stages:

| Stage | Logical node | GPU |
| --- | --- | --- |
| 0 | `p4-gpu-3090` | RTX 3090 (`GPU-38e6dbac-fee5-ac16-62d4-cfacbe02f8ed`) |
| 1 | `p4-gpu-4080` | RTX 4080 (`GPU-79caabbe-c843-631f-3cea-9c01e652c78c`) |

P4 received progress `0..99`, then `DRAFT_REPORT`, then `LOAD_PROGRESS=100` only after the group became `running`. Its health response was `ready=true`.

| Measurement | Observed value |
| --- | ---: |
| GGUF model bytes | 1,640,622,080 B |
| GGUF layer bytes | 1,392,656,384 B |
| native KV allocation bytes | 15,619,072 B |
| FFN allocation bytes | unavailable (`0`, not measured zero) |
| prompt / prefill tokens | 37 / 36 |
| native decode-token budget/count | 128 / 128 |
| P4 streamed text events | 122 |
| time to first token | 340.844 ms |
| stage 0 prefill / decode compute | 98.208 ms / 1,944.521 ms |
| stage 1 prefill / decode compute | 110.016 ms / 104.062 ms |
| stage 0 -> 1 hidden-state traffic | 164 frames, 532,016 B |
| stage 1 -> 0 sampled-token traffic | 128 frames, 1,421 B |
| host-observed average transfer rate | 54,666,666.67 B/s |

The generated response was truncated by the requested token cap and included factual inaccuracies. It proves transport, resource reporting, and cleanup behavior; it does not certify model answer quality.

After the run, the exact `p4-adapter-e2e-*` group was deleted and no listeners remained on P4 ports 19201-19203 or Pipeline ports 52221-52222. The retained raw client artifact is `target/pipeline-e2e/client-20260808190619.log`.

## 2026-08-08: combined `p4-agent` reconstruction verification

The relay implementation was rebuilt into a policy-free listener/frame-forwarding base (`layers/runtime/src/foundation/transport/mod.rs`) and independent ControllerProcessor/NodeProcessor policies (`layers/runtime/src/application/routing/processor/mod.rs`). The default `p4-agent` invokes the latter two in-process; only the adapter boundary remains a P4 TCP connection.

`tools/scripts/e2e/stock/run-real-e2e.ps1` then completed an owned stock CPU `llama-server` request through `p4-agent` with 12 streamed tokens and `P4_DONE`.

`tools/scripts/e2e/pipeline/run-pipeline-e2e.ps1 -Prompt 'P4 agent 경로가 동작하는지 한 문장으로 답하라.' -MaxTokens 16` completed the actual two-GPU Pipeline path through the same combined agent:

| Measurement | Observed value |
| --- | ---: |
| P4 health | `ready=true` |
| GGUF model / layer / KV bytes | 1,640,622,080 / 1,392,656,384 / 15,619,072 B |
| prompt / prefill / native decode tokens | 47 / 46 / 16 |
| P4 streamed text events | 16 |
| time to first token | 350.292 ms |
| stage 0 prefill / decode compute | 100.878 ms / 591.312 ms |
| stage 1 prefill / decode compute | 117.116 ms / 37.466 ms |
| stage 0 -> 1 hidden-state traffic | 62 frames, 201,128 B |
| stage 1 -> 0 sampled-token traffic | 16 frames, 193 B |
| host-observed average transfer rate | 77,297,463.49 B/s |

The exact owned Pipeline group and all P4 relay processes are removed by the scripts' `finally` blocks; the raw artifact is retained under `target/pipeline-e2e/`.

## 2026-08-08: startup-selected listener ports

The rebuilt agent accepts its sole `LISTEN_ENDPOINT` argument and prints the resolved listener address. The owned stock E2E completed at `127.0.0.1:29017` with 12 streamed tokens. The owned 2-GPU Pipeline E2E completed at `127.0.0.1:29201` with `LOAD`, `DRAFT_REPORT`, `HEALTH ready=true`, 16 P4 token events, and `DONE`.

For the Pipeline run, the host reported 362.875 ms TTFT, 207,616 B of stage-0-to-stage-1 hidden-state traffic, 172 B of sampled-token return traffic, and 82,980,015.99 B/s average observed transfer rate. This proves that the listener port is a startup configuration, not a protocol constant.

## 2026-08-08: controller-supplied dynamic node route

The default agent was started with only `p4-agent 127.0.0.1:29019`; it received no node, GPU, model, or backend argument. The stock E2E first sent `LOAD` with `p4_agent.adapter_endpoint=127.0.0.1:19103`, received 0% then 100% progress, and completed 12 streamed tokens.

The two-GPU Pipeline E2E started the agent with only `p4-agent 127.0.0.1:29202`, sent the same control envelope with its Adapter endpoint and opaque Pipeline `adapter_request`, then completed `LOAD`, `DRAFT_REPORT`, `HEALTH ready=true`, 16 token events, and `DONE`. Its observed TTFT was 357.091 ms; hidden-state traffic was 62 frames / 201,128 B, sampled-token return traffic was 16 frames / 200 B, and host-observed transfer rate was 44,675,255.44 B/s.

This proves that concrete inference topology is protocol-supplied runtime state, not agent startup configuration.
