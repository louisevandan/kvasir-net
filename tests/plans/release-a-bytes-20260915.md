# Test Plan: Release A A-BYTES

## Created

2026-09-15 KST. 기준 모델은 Qwen3.5-122B-A10B UD-Q5_K_S 3-shard이고, 기준 topology는
Spark `[0,24)` / Mac20 `[24,36)` / Mac21 `[36,48)`, resident 8, total context 819,200,
batch/ubatch 128/64다.

## Goal

native 실행이 만든 physical result, adapter가 보존하는 completion, agent 간 hop receipt와 edge frame을
서로 다른 소유 비용으로 계산한다. 원인 작업 전에 필요한 보존 공간을 확보하고, 누락·무한·음수·overflow·
잔여보다 큰 구성은 실행 전에 거부한다. count를 byte로 오인하거나 2 GiB wire frame 기본값을 메모리
예산으로 쓰지 않는다.

## Environment

- 개발 기준: Windows `F:\dev\p4`, `main`, A-LOAD 기준 `9b2522dae` 이후 소스.
- native 재빌드와 모델 검증은 원격 host를 우선한다.
- 이 PC 빌드가 불가피하면 48 logical CPU의 70% 이하인 33 jobs를 `CARGO_BUILD_JOBS`, Cargo
  `--jobs`, `CMAKE_BUILD_PARALLEL_LEVEL`, `RUST_TEST_THREADS`에 적용한다.
- 이 PC 추론이 불가피하면 `CUDA_DEVICE_ORDER=PCI_BUS_ID`와
  `CUDA_VISIBLE_DEVICES=GPU-38e6dbac-fee5-ac16-62d4-cfacbe02f8ed`로 RTX 3090만 노출한다.
- 2026-09-15의 예기치 않은 종료 시각은 15:19:04와 16:18:52다. 첫 종료 3분47초 전에는 제한 없는
  `cargo test` 두 개를 동시에 시작했고 모델 실행은 없었다. 두 번째 종료 전에는 제한 없는 release build에
  이어 로컬 llama.cpp와 HF 추론을 연속 실행했다. 두 Kernel-Power 41 모두 bugcheck와 전원 버튼 시각이
  0이며 직전 WHEA/GPU 오류는 없다. 첫 사례는 빌드 부하를 독립적으로 지목하고 두 번째는 빌드·추론이
  섞였으므로, 빌드는 33 logical CPU affinity로 제한하고 로컬 추론은 지정 RTX 3090만 노출한다.

## Preconditions

1. `git status --short`가 비어 있고 HEAD와 `origin/main`이 같다.
2. 새 agent namespace는 INSPECT `nodes=[]`이며, 기존 `:52005` agent와 설치 앱은 변경하지 않는다.
3. A-PLAN의 source/patch/model/cut/shape/allocation hash가 그대로다.
4. 제품 입력 상한은 동시 요청 64, 요청 보존 128 MiB, 입력 토큰 6,553,600, 요청 하나 2 MiB,
   요청별 출력 2,048 token이다. 누적 출력 예약은 131,072 token이다.

## Steps

1. **B0 비용 단위 고정**
   - native no-alloc graph reserve가 만드는 모든 허용 최대 UBATCH graph에서 outgoing cut tensor의
     descriptor 수, alias 제거 payload bytes, physical-v4 고정/행/owner metadata 상한을 checked integer로
     합산한다.
   - PLAN과 READY가 같은 `max_physical_result_bytes`를 보고한다. 실제 result encoder와 Rust decoder가
     이 값을 넘으면 body allocation/native commit 전에 거부한다.
2. **B1 LOAD profile 결속**
   - llama adapter 소유의 versioned resource profile에 request count/bytes/input/output token 한도,
     physical result 상한, completion payload 상한을 넣는다.
   - LOAD는 profile 누락, 0, overflow, PLAN/READY 불일치, mailbox/edge/receipt 잔여 부족을 fail closed한다.
   - P4 core에는 llama.cpp tensor 타입이나 model shape를 넣지 않는다.
3. **B2 실행 전 보존 예약**
   - 실제 first/middle physical 호출 전에 completion mailbox에 forward 1개와 관측 fan-out 전체의
     count/retained-byte 상한을 하나의 group으로 예약한다.
   - 실제 Event 비용은 envelope capacity와 payload capacity를 할당 없이 계산한다. publication은 같은
     reservation을 이동하며 실제 비용이 상한 이하인지 검사한다.
   - 예약 실패 시 scheduler/flight/KV/native/ID/effect/output가 모두 불변이어야 한다.
4. **B3 수명 분리**
   - pending request, retained completion, broker dedupe receipt, hop receipt/outstanding, native response
     buffer의 snapshot을 별도 필드로 INSPECT한다.
   - destination은 실제 Event 비용을 예약한 뒤만 commit하고, source는 remote receipt 전까지 원본과
     자기 claim을 유지한다. duplicate/late/uncertain은 효과를 두 번 만들지 않는다.
5. **B4 경계 및 변이**
   - 작은 count/큰 bytes, exact boundary와 ±1, receipt를 유지한 queue full, duplicate/late receipt,
     uncertain transport, neutral fake adapter를 실제 소비 경로에서 실행한다.
   - reserve-before-native 제거, commit-before-reserve, response bound 제거, receipt 독립 비용 제거 변이를
     각각 독립 복사본에서 재컴파일하고 원래 반례가 실패하는지 확인한다.
6. **B5 회귀와 실기**
   - 표적 Rust/C++/Python 시험 뒤 `cargo test --workspace --no-fail-fast` feature off/on을 각각 끝까지
     실행한다. ignored는 별도 집계한다.
   - 소형 llama.cpp와 HF/Python adapter의 정상 생성·취소·회수·재수용을 다시 확인한다.
   - 현재 소스/바이너리로 Qwen122B 3-host LOAD 후 경계 거부와 정상 1요청을 실행하고, 모든 stage를
     UNLOAD/DELETE한 뒤 새 namespace의 nodes/listener/native/GPU 점유가 0인지 확인한다.

## Expected Results

- 모든 상한은 PLAN/codec/profile의 checked integer에서 재계산 가능하며 경험 배수나 기본 2 GiB가 없다.
- 경계 초과는 native/KV/output 전에 거부되고 request ledger, reservation, credit, receipt, output 효과가
  전후 동일하다.
- 정상 결과는 source retained claim에서 destination claim으로 이동하고 remote receipt 뒤 한 번만
  퇴역한다. uncertain은 성공으로 승격되지 않는다.
- llama.cpp와 HF/Python adapter 회귀가 모두 통과한다.
- Qwen122B 정상 응답 전 단계가 통과해야 A-BYTES를 GREEN으로 승격한다.

## Logs To Capture

- HEAD/source archive/native binary/model shard/plan/profile SHA-256.
- native PLAN/ACTUAL/READY의 result bound와 실행 result bytes.
- 거부 전후 request/completion/broker receipt/hop receipt/outstanding snapshot.
- 최초 오류, `evidence_missing`, `cleanup_error`, 종료 코드와 전체 workspace summary.
- 원격 host별 agent/native PID, 전체 명령, GPU UUID, LOAD/UNLOAD/DELETE와 최종 INSPECT.
- 변이별 source/binary hash, 실제 재컴파일 증거, 예상 실패 지점.
