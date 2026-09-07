# P4 staged MTP protocol

> 문서 지위 (2026-09-06): **분야 계약·구현과 구별**. 소유 분야의 계약/목표를 읽되 구현 완료로 간주하지 않는다. 현재 개발 순서와 충돌하면 로드맵의 명시적 이관을 따른다.
> 현재 목표·상태·순서는 [실행 로드맵](distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](document-map.md)를 따른다.

## 상태와 범위

이 문서는 staged P4에서 MTP를 프로토콜에 봉인하기 위한 결정과 구축 계획을 고정한다.
현재 staged 구현은 MTP parser/ownership probe 범위이며, 이 문서가 작성됐다고 해서
`mtp_execution`이 지원된다는 뜻은 아니다. 실행 capability는 아래의 선행 관문과
통합 검증을 통과한 뒤에만 광고한다.

현재 코드 기준으로 Rust staged adapter는 `HopComplete`, `SequenceAcquired`,
`SequenceReleased`를 보고하고, C++ `StageRuntime`은 일반 HOP만 production 경로로
실행한다. `execute_mtp_hop`은 prompt, `seq_id = 0`, draft 상한 1을 사용하는
ownership/test 경로이며 production MTP 실행에 연결되어 있지 않다. 따라서 이 문서는
현재 동작의 설명이 아니라, 현재 event/transport 경계를 보존하면서 execution을 여는
구축 문서다.

현재 validation 결과도 `mtp_parser=1`, `mtp_execution=0`을 기준선으로 삼는다.
`validate-mtp-speculative-capability.mjs`가 parser/ownership 지원을 확인한 것만으로
production execution을 성공으로 판정하지 않으며, 이 문서의 A·B·C와 실제 실행 검증이
끝나기 전에는 그 capability를 올리지 않는다.

범위는 다음과 같다.

- `llama.cpp`의 MTP 내부 동작을 P4 wire protocol에 노출하지 않는다.
- 기존 HOP의 다중 행 표현을 사용해 후보와 확정 토큰을 운반한다.
- 각 stage가 다음 HOP에 진입할 때 자기 speculative KV suffix를 지연 롤백한다.
- MTP 전용 `accepted_count` wire field, commit broadcast, 전역 barrier를 추가하지 않는다.

관련 현재 계약은 [protocol.md](protocol.md)의 opaque generation 계약과
[architecture.md](architecture.md)의 CPS ring 흐름을 따른다.

## 핵심 결론

MTP의 accepted boundary는 terminal이 산출한 다음 HOP의
`SequencePayload.position`에 이미 반영된다. P4의 `Sequence`/`Outcome`은
position 필드를 갖지 않는다 — position은 staged adapter 자신의
`SequencePayload`(`apps/p4/layers/adapters/llamacpp/staged/adapter/src/protocol/sequence.inc.rs`)
안에 있는 값이며, P4 경계에서는 opaque `state`/`forward` bytes로만 오간다.
따라서 정상적인 CPS 흐름에서는 accepted count를 새 필드로 전파할 필요가 없다.
실제 blocker는 각 memory backend가 speculative suffix를 부분 롤백할 수 있는지 여부다.

구축은 다음 두 선행 관문을 닫은 뒤 시작한다.

1. staged adapter `SequencePayload.position`의 의미를 문서와 테스트로 고정한다.
2. 대상 memory backend의 런타임 롤백 capability를 실측한다.

여기에 현재 P4 event 계약의 세 번째 관문이 있다. `SequencePayload.n_tokens`는 HOP
입력 행 수일 뿐이며, 현재 `HopComplete`는 sequence당 하나의 `Outcome`, `Outcome`은
하나의 optional token, `Continue`도 하나의 token만 표현한다. 그러므로 MTP가 한 랩에서
여러 외부 토큰을 방출할지, 내부에서 여러 행을 처리하되 기존 단일-token event로
투영할지 먼저 결정해야 한다.

A·B·C 중 하나라도 닫히지 않으면 `Some(1)` 제거, rollback 호출, 다중 token event
투영, `mtp_execution` 광고를
진행하지 않는다.

## 1. 위치 계약

현재 숫자 필드가 중간 stage와 terminal에서 서로 다른 권위를 갖는 점을 명문화한다.
숫자의 기본 의미는 모두 “다음 HOP 입력 행이 시작하는 위치”로 통일한다. 이 값은 P4의
`Sequence`/`Outcome`이 아니라 staged adapter의
[`SequencePayload.position`](../layers/adapters/llamacpp/staged/adapter/src/protocol/sequence.inc.rs)에
있으며, P4 경계에서는 `Sequence.state`/`Outcome.forward`의 opaque bytes로만 오간다.

| 값 | 의미 | 권위 |
|---|---|---|
| 수신 HOP의 `SequencePayload.position` | 현재 HOP 입력 행이 시작하는 위치 | 현재 HOP의 권위 있는 입력 위치 |
| terminal이 쓰는 응답 `SequencePayload.position` | terminal이 계산한 다음 HOP 입력 시작 위치 | terminal이 실제 target KV에 커밋한 행 수만큼 전진 |
| middle이 쓰는 응답 `SequencePayload.position` | terminal 결과를 알 수 없으므로 입력 위치를 그대로 echo | 진행 권위가 아님 |

중간 stage가 position을 자체적으로 전진시키면 안 된다. terminal만 이번 검증 결과를
알고 다음 HOP의 위치를 계산한다. 이 성질은 현재
`a_middle_stage_preserves_the_global_decode_position` 회귀 테스트
(`apps/p4/layers/adapters/mock/src/tests/core.rs:282`)가 지킨다. 이 테스트는 더 이상
P4 레벨 `Sequence.position`/`Outcome.position` 필드를 검사하지 않고, mock 자신의
opaque `encode_state`/`decode_state` 헬퍼로 `Sequence.state`에 실은 값이
`Outcome.forward`로 그대로 echo되는지 확인하는 방식으로 같은 성질을 지킨다. 테스트
이름과 의미는 여전히 유효하며, MTP 다중 행 경로에서도 이 성질은 유지되어야 한다.

이 계약은 “마지막으로 쓴 위치”와 “다음에 쓸 위치”를 혼용하지 않도록 한다. rollback
경계 계산은 이 계약을 기준으로만 구현하며, stage마다 보유한 이전 speculative 끝과
새로 도착한 수신 HOP의 `SequencePayload.position`을 비교해 결정한다.

## 2. memory backend capability 관문

llama.cpp의 memory 지원은 정적 가정이 아니라 런타임 probe 결과로 판단한다.
현재 upstream의 분류는 다음과 같다.

| capability | 의미 | staged MTP 판단 |
|---|---|---|
| `NO` | sequence 제거를 지원하지 않음 | 실행 불가 |
| `PART` | sequence의 일부 suffix 제거 가능 | 기본 lazy rollback 후보 |
| `FULL` | 전체 sequence만 제거 가능 | 전체 checkpoint/restore 방식으로 가능하지만 비용이 크며 이번 범위에서는 제외 |
| `RS` | recurrent/hybrid 상태의 제한된 partial rollback | `n_rs_seq` 범위와 snapshot 검증 필요 |

관련 upstream 근거는
[`common.h`](../layers/adapters/llamacpp/upstream/common/common.h)의
`COMMON_CONTEXT_SEQ_RM_TYPE_*`와 `need_n_rs_seq()`, 그리고
[`common.cpp`](../layers/adapters/llamacpp/upstream/common/common.cpp)의
`common_context_can_seq_rm()` probe다. recurrent memory의 실제 rollback 범위는
[`llama-memory-recurrent.cpp`](../layers/adapters/llamacpp/upstream/src/llama-memory-recurrent.cpp)
에서 확인한다. upstream은 교체 가능한 경계이므로 Linker 전용 코드를 추가하지 않는다.

MTP/Eagle/DFlash/Dspark 계열은 draft 상한에 맞춘 `n_rs_seq`가 필요할 수 있다.
`RS`라고 판정되는 것만으로 충분하지 않고, 실제 speculative depth와 restore 결과를
대상 모델에서 검증해야 한다.

## 3. CPS lazy rollback 흐름

롤백은 별도 왕복이나 전역 commit barrier가 아니다. terminal이 만든 다음 HOP이
generating edge를 통해 링의 첫 stage로 돌아오면, 각 stage가 자기 입력 시점에
자기 suffix를 정리한다.

```text
terminal
  │ next HOP: position + KV-committed rows
  ▼
generating edge → stage 0 → stage 1 → … → stage N → terminal
                    │         │                 │
                    └─ 각 stage가 도착 시 자기 speculative suffix를 lazy rollback
```

각 stage의 처리 순서는 다음과 같다.

1. 같은 sequence에 대해 저장한 이전 speculative 끝과, `Sequence.state`가 실어 온
   staged `SequencePayload.position`을 비교한다.
2. 새 position 이후에 남은 자기 KV/state suffix를 제거하거나 snapshot을 복원한다.
3. 새 HOP 행을 처리한다.
4. 이번 HOP에서 자신이 쓴 speculative 끝을 갱신한다.

이 상태는 sequence별이어야 하며, 병렬 sequence의 suffix를 서로 잘라서는 안 된다.
롤백은 다음 HOP 진입 시점에 수행하므로 추가 broadcast나 전역 동기화가 필요 없다.

## 4. wire와 row count

MTP 전용 필드를 만들지 않는다. staged HOP에 이미 있는 `SequencePayload.n_tokens`를
`Some(1)`에서 `Some(n)`으로 확장한다. 이것은 MTP 의미를 노출하는 필드가 아니라
일반적인 HOP 입력 행 수다.

| 값 | 의미 |
|---|---|
| `None` | trailing field가 없는 legacy frame |
| `Some(0)` | corruption; encode/decode 모두 거부 |
| `Some(n > 0)` | 실제 입력 행 수 |

이번 변경은 trailing field를 추가하지 않는다. 따라서 현재의 exact trailing detection을
바꾸지 않는다. `SequencePayload`의 optional trailing field가 `n_tokens` 하나라는
불변식을 회귀 테스트로 고정하고, 다른 trailing field 추가는 별도의 wire-format 변경으로
취급한다. 자기서술형 trailing 포맷 전환은 이번 범위가 아니다.

현재 고정점은
[`hop.inc.rs`](../layers/adapters/llamacpp/staged/adapter/src/adapter/hop.inc.rs)의
Decode `input.n_tokens = Some(1)`이며, 구현 단계에서 고정된 1을 실제 row count로
대체한다. encoding/decoding의 `Some(0)` 검사는
[`hop_kv.inc.rs`](../layers/adapters/llamacpp/staged/adapter/src/protocol/hop_kv.inc.rs)
에 있다.

중요한 경계는 `n_tokens`와 외부 event cardinality가 같다고 가정하지 않는 것이다.
`n_tokens = n`은 이번 HOP이 소비하는 입력 행 수를 뜻한다. 이것만으로 `Outcome`을
n개 만들거나 `Reply::Token`을 n개 내보낼 수 있다는 뜻은 아니다. 현재 event 계약을
그대로 유지한다면 terminal은 여러 행을 내부에서 검증하되 한 랩의 외부 결과를 기존
단일 `Outcome`/`Continue`로 투영하는 정책이 필요하다. 다중 visible token을 한 랩에서
외부에 내보내는 정책을 선택하면 별도의 event/message 계약 변경과 replay·index 규칙이
필요하다.

## 5. terminal 결과와 토큰 회계

내부적으로 다음 네 수를 구분한다.

- `candidate_count`: MTP가 만든 speculative 후보 수
- `accepted_count`: 검증에서 확정된 후보 수
- `committed_count`: target KV에 실제로 커밋된 행 수
- `visible_count`: 이번 HOP이 외부 토큰 회계에 실제로 방출한 수

`accepted_count`는 wire field가 아니다. terminal의 다음 HOP `position`에서 boundary가
드러나며, stage는 자기 이전 write 끝과 새 position으로 rollback 범위를 유도한다.
**position은 KV commit 기준이고, token event는 visible 기준이다.**
terminal은 `accepted_count`에서 commit 수를 재구성하지 않고, 실제 target KV에 쓴
행 수를 `committed_count`로 기록한다. 일반적인 불일치 경로에서는 이 값이
`accepted_count + 1`이지만, accepted 구간 안에서 EOS 같은 종료 토큰이 발생하면
추가 sample 행이 없으므로 식이 달라질 수 있다. 반면 `visible_count`는
`Reply::Token.index`의 단조 증가와 일반 `Done.generated` 회계에 반영된다.
유일한 예외는 staged native가 `position >= remaining`을 증명한 `length` terminal이다:
그 `Outcome::terminal_generated`는 request bound를 `Done.generated`으로 보존하며,
candidate/accepted count를 노출하는 필드가 아니다
([adapter boundary](adapter-boundary.md#terminal-length-accounting)).

EOS, stop sequence, grammar, cancellation 등으로 accepted 구간 전체가 외부에
방출되는 것은 아닐 수 있다. 그러므로 후보/채택/가시 토큰을 같은 수로 처리하지 말고,
terminal의 commit position 전진과 외부 token event 회계를 각각 검증한다.
- stop-string 부분 일치처럼 KV에는 커밋됐지만 외부에 아직 방출하지 않은 토큰을
  다음 HOP position에서 누락하지 않는다.

`remaining`은 마지막 adapter의 로컬 clamp로 처리한다. 이를 stage 간 wire field로
복제하지 않는다.

`HopComplete`의 `hop_id`, `deployment`, `expected`, sequence 집합 검증은 기존
P4 계약을 그대로 따른다. staged adapter는 prefill에서 필요한 경우
`SequenceAcquired`를 먼저, 해제 시 `SequenceReleased`를 해당 `HopComplete`보다
먼저 보고한다. node가 `Outcome`을 `Token`, `Done`, 다음 stage 전달 또는 `Continue`로
투영하는 순서와 `event_seq`/재시도 규칙은 [protocol.md](protocol.md)에 있는 외부
event 계약의 권위이며, MTP 내부의 candidate/accepted 수를 그대로 event로 노출하지
않는다.

## 6. 복구와 재연결

restore 또는 재연결이 발생하면 다음 상태를 함께 폐기한다.

- terminal의 MTP 후보 기억
- 모든 stage의 speculative KV suffix
- recurrent/hybrid backend의 speculative snapshot

복구 후에는 평범한 decode로 재개한다. terminal의 후보 기억만 폐기하고 앞단 stage의
유령 suffix를 남기면 이후 검증이 조용히 잘못될 수 있다.

## 7. 구축 순서

### 선행 관문 A — position contract

- staged `SequencePayload.position`의 위 의미를 코드 주석, 문서, 테스트에 고정한다.
  P4는 그 값을 읽지 않으므로 계약은 staged adapter 안에서만 성립한다.
- middle stage echo와 terminal 다중 행 전진을 별도로 검증한다.
- MTP 경로에서도 middle position을 보존하는 회귀 테스트를 유지한다.

### 선행 관문 B — backend probe

- 지원 대상 모델과 memory backend에서 `NO`/`PART`/`FULL`/`RS`를 실측한다.
- partial rollback 깊이, recurrent snapshot 복원, speculative depth 상한을 검증한다.
- probe 결과가 실행 불가이면 parser/ownership probe만 유지하고 execution capability를
  광고하지 않는다.

### 선행 관문 C — 외부 event cardinality

- 현재 `HopComplete.expected`는 sequence 집합이고, `outcomes`는 sequence당 하나이며,
  `Outcome.text`는 그 sequence가 이번 hop에서 요청자에게 내보내는 전부다. P4 경계에
  token이라는 단위는 더 이상 없다. 샘플된 token은 staged `SequencePayload`의
  `initial_tokens` 안에 있고, 그것은 벡터이므로 "정확히 하나"라는 제약은 필드 이름이
  아니라 tail이 lap마다 하나만 넣는다는 사실이 지탱한다. MTP는 바로 그 사실을 바꾼다.
- `n_tokens`를 다중 행으로 확장해도 이 계약이 자동으로 다중 token event가 되지 않음을
  명시한다.
- 구현 전에 둘 중 하나를 결정한다. (a) 여러 visible token을 기존 event/message
  계층에 순서대로 투영하는 확장, 또는 (b) MTP 랩 내부에서 여러 행을 처리하고 외부에는
  기존 한 token event만 내보내는 투영이다.
- 선택한 정책에 대해 `Reply::Token.index`, `Done.generated`, stop/EOS, replay 및
  `Continue`의 position/remaining 회계를 테스트로 고정한다.

### 구현

- Decode row count의 `Some(1)` 고정 제거.
- C에서 선택한 event cardinality 정책에 맞춰 terminal 결과를 generating edge와 token
  회계에 연결.
- 다음 HOP 진입 시 stage별 lazy rollback 구현.
- restore/reconnect 시 모든 speculative state 폐기 후 ordinary decode 재개.
- `mtp_execution` capability는 A·B·C와 실제 실행 테스트를 통과한 backend에만 광고.

### 통합 관문

최종적으로 workspace 테스트, clippy, fmt, 그리고 fleet-level driver를 통과해야 한다.
링 전체의 row 수, position, rollback, reconnect를 함께 검증해야 하므로 이 관문은
분리된 단위 작업으로 대체할 수 없다.

## 8. 병렬 구축 가능성

구축을 빠르게 하되 관문을 우회하지 않으려면, 먼저 A·B만 병렬로 실행하고 두 결과가
닫힌 뒤 C·D를 하나로 합칠지 분리할지 결정한다. lead 에이전트는 계약, worktree,
통합을 소유하고 각 작업 에이전트에는 disjoint write set을 부여한다. 따라서 에이전트
수를 처음부터 고정하지 않는다. B의 `PART/RS/FULL/NO` 결과가 rollback 구현량과
분할의 이득을 결정하기 때문이다.

### 8.1 에이전트 분할

| 에이전트 | 소유 범위 | 선행 조건 | 산출물 |
|---|---|---|---|
| A — position contract | staged `SequencePayload.position` 문서 주석, mock middle echo/terminal commit 테스트 | 없음 | position 계약 패치와 off-by-one 테스트 |
| B — capability probe | `PART/RS/FULL/NO` probe, 모델별 evidence, capability 판정 API | 없음 | probe 실행 결과, rollback depth 표, 실패 분류 |
| C — event cardinality | `HopComplete`/`Outcome`/`Continue`와 다중 row 결과의 외부 투영 정책 | A와 현재 event 계약 확인 | 단일 event 유지 또는 다중 token event 변경의 결정과 회계 테스트 |

A와 B는 서로 독립적으로 즉시 시작할 수 있다. C는 현재 outer event 처리와
`protocol.md`의 `HopComplete` 투영을 확인한 뒤 결정하고, D는 B 결과를 기다린다.
핵심 결론과 §7의 선행 관문에 따라 C와 D의 MTP 구현은
A와 B가 각각 **완료**될 때까지 MTP 구현을 시작하지 않는다. A의 초안만으로
`Some(1)` 제거를 시작하거나, B 결과 없이 rollback 추상화를 고정하거나, C 결정 없이
다중 token event 투영을 시작하지 않는다.

### 8.2 B 결과에 따른 runtime 구성

A·B·C의 산출물을 lead가 검토한 뒤, B 결과에 따라 runtime(D)의 범위를 구성한다.

| B 결과 | D 구성 | 이유 |
|---|---|---|
| `PART` | D를 단일 adapter-runtime 흐름으로 수행 | 위치 기반 suffix 제거의 경계가 좁아 별도 runtime 분할보다 통합이 빠름 |
| `RS` | D 내부를 snapshot/depth rollback과 나머지 runtime으로 분리 가능 | recurrent rollback이 runtime 작업을 크게 늘리므로 병렬화 가치가 있음 |
| `FULL` | 기본은 D 통합, 전체 checkpoint/restore를 실제 범위에 넣을 때만 분리 | FULL은 고비용이며 이번 기본 범위에서는 capability를 광고하지 않음 |
| `NO` | D를 MTP execution으로 시작하지 않음; capability rejection만 통합 | 실행 불가 backend에서 rollback 구현을 진행하지 않음 |

즉 C는 Rust `n_tokens`·event cardinality·결과 투영·visible/committed 회계를, D는
`StageRuntime` target/MTP 상태·draft/verify/accept·backend rollback·restore invalidation을
담당한다. 다만 이 역할 분할은 A·B·C 완료 후에만 활성화한다. B가 `PART`이면 D를
하나의 흐름으로 합치고, `RS`이면 D 내부를 분리해 벽시계 시간을 줄인다.

### 8.3 충돌 방지 규칙

- 한 파일은 한 에이전트만 수정한다. 공통 파일을 둘이 동시에 고치지 않는다.
- A는 semantic contract와 mock 테스트만 소유하고, C/D의 runtime 구현을 수정하지 않는다.
- B는 probe와 evidence를 소유하고 `upstream`에 Linker 코드를 추가하지 않는다.
- C는 wire row count와 P4 event projection만 소유한다. target KV 삭제나 sampler
  state를 구현하지 않는다.
- D는 C의 payload shape를 소비하지만 Rust protocol 파일을 직접 수정하지 않는다.
- capability가 확인되기 전에는 어느 에이전트도 `mtp_execution=1`을 기본값으로
  바꾸지 않는다.
- 각 에이전트는 변경 파일, 테스트 명령, 미검증 가정, 남은 blocker를 짧은 handoff
  기록으로 제출한다.

### 8.4 통합 순서

lead 에이전트는 다음 순서로 통합한다.

1. A와 B를 병렬 실행하고 각각 독립 검증을 완료한다.
2. A를 통합해 position contract와 mock 회귀 테스트를 고정한다.
3. B의 실제 모델 probe evidence를 검토하고, 실행 가능한 capability와 C·D 구성 방식을
   결정한다.
4. C에서 결정한 event cardinality를 먼저 고정한 뒤 선택된 C·D 흐름을 통합해
   `Some(1) → Some(n)`, visible/committed 회계, capability gate, stage-local rollback을
   연결한다.
5. rejection, accepted EOS, stop-string buffering, reconnect/restore를 포함한 통합
   테스트를 추가한다.
6. capability report가 실제 probe와 C의 외부 event 계약 및 실행 결과와 일치하는지
   확인한 뒤에만 `mtp_execution`을 광고한다.

`PART`/`RS` 결과에 따라 C와 D를 분리하는 경우에도 D는 C의 최종 구현을 복사하지
않고 문서에 고정된 `SequencePayload` 계약만 소비한다. 따라서 C의 파일 변경이 D의
runtime branch와 충돌하지 않는다. C·D 통합형을 선택하면 lead가 두 경계의 단일
변경을 소유한다.

### 8.5 에이전트별 검증과 최종 관문

각 에이전트는 자기 범위의 빠른 검증만 먼저 수행한다. C·D가 통합된 경우에는
아래 C/D 검증을 같은 에이전트가 순서대로 수행한다.

- A: mock position tests와 protocol contract tests
- B: `validate-mtp-speculative-capability.mjs` 및 대상 모델 evidence
- C: protocol/adapter tests, legacy frame과 `Some(n > 1)` 왕복
- D: runtime unit, full-tail MTP, rollback/restore tests

그 뒤 lead 에이전트가 분리할 수 없는 통합 관문을 실행한다.

- `cargo test --workspace`
- workspace `clippy`
- `cargo fmt --check`
- staged server/adapter C++ build
- fleet-level driver에서 terminal → stage 0 → … → terminal 실제 링 검증
- baseline full model 대비 token, logits, KV position, restore 결과 비교

통합 관문이 끝나기 전에는 병렬 에이전트의 “통과”를 최종 완료로 간주하지 않는다.
현재 문서 작성 단계에서는 병렬 에이전트를 생성하지 않았으며, 실제 구축 시에도 모든
작업은 동일 workspace의 dirty 변경을 덮어쓰지 않는 별도 worktree에서 수행한다.

## 9. 수용 기준

| 영역 | 필수 검증 |
|---|---|
| position | middle echo, terminal multi-row advance, 실제 `committed_count`, prefill/decode 경계, off-by-one |
| row count | legacy `None`, `Some(1)`, `Some(n>1)`, `Some(0)` 거부 |
| speculation | 모두 거부, 일부 수락, 전부 수락, accepted 구간 EOS, remaining clamp |
| output | token index 단조성, `Done.generated`, EOS/stop/grammar/cancel, `visible_count != committed_count`인 stop-string buffering |
| event contract | sequence당 `Outcome` cardinality, `HopComplete.expected` 검증, `SequenceAcquired/Released` 선행 순서, 단일 또는 다중 token 투영 정책 |
| rollback | `PART`, `RS`, `FULL`, `NO`별 허용/거부, sequence별 suffix 격리 |
| recovery | reconnect/restore 후 speculative state 폐기와 ordinary decode 재개 |
| CPS | terminal → generating edge → stage 0… 순방향, per-lap 전역 barrier 없음 |
| wire safety | `n_tokens` 외 trailing field 불변식과 legacy decode 유지 |

## 관련 자료

- [P4 protocol](protocol.md)
- [P4 outer/event policy](protocol-outer.md)
- [P4 architecture](architecture.md)
- [현재 llama.cpp adapter 요약](../llamaAdaper.md)
- [staged validation README](../layers/adapters/llamacpp/staged/scripts/validation/README.md)
- [MTP minimum-slice audit](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-mtp-speculative-minimum-slice-audit.md)
- [MTP ownership probe](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-mtp-auxiliary-ownership-probe.md)

## 10. 구상 어댑터 구현 상세

추상 P4가 책임지는 것은 HOP의 row count, 위치 전달, opaque cut-set 운반뿐이다.
MTP의 실제 의미와 상태 일관성은 `llamacpp/staged` 구상 어댑터가 모두 책임진다.
구현은 다음 네 경계를 넘나들지만, MTP 상태의 권위는 C++ `StageRuntime`에 둔다.

| 구상 경계 | 구현 책임 | MTP 상태를 소유하는가 |
|---|---|---|
| Rust `StagedAdapter` | HOP row count 설정, frame 왕복, 결과를 P4 event로 투영 | 아니오 |
| C++ protocol/server | `SequencePayload` 검증·dispatch, sequence별 요청 직렬화 | 아니오 |
| C++ `StageRuntime` | target/MTP context, draft·verify·accept, sampler, KV rollback | 예 |
| C++ KV bridge | durable KV save/restore/drop와 runtime identity 검증 | 영속 상태만 |

### 10.1 Rust 어댑터: HOP을 다중 행으로 바꾸기

수정 대상은
[`hop.inc.rs`](../layers/adapters/llamacpp/staged/adapter/src/adapter/hop.inc.rs)다.
현재 Decode 경로의 `input.n_tokens = Some(1)`은 모든 decode를 한 행으로 고정한다.
이를 다음 규칙으로 바꾼다.

현재 Rust 어댑터의 결과 경계도 함께 보존해야 한다. 어댑터는 sequence별 결과를
`Outcome`으로 만들고 `HopComplete` 하나로 보고한다. node는 이 결과를 다음 stage의
cut-set, 외부 `Reply::Token`/`Reply::Done`, 또는 다음 `Continue`로 투영한다.
`SequenceAcquired`와 `SequenceReleased`는 admission/release 이벤트이며 MTP 후보나
accepted count를 운반하지 않는다. 따라서 다중 row 입력을 도입해도 `Outcome` 하나에
후보 목록을 몰래 넣거나, `HopComplete.expected`의 sequence 집합을 row 집합으로
해석해서는 안 된다.

1. 새 HOP을 만들 때 `SequencePayload`의 기존 inbound `n_tokens`를 무조건 덮어쓰지
   않는다. adapter가 이번 HOP에 실제로 보낼 row 수를 계산해 명시한다.
2. 일반 decode는 계속 `Some(1)`을 사용한다.
3. MTP가 활성화된 terminal 결과를 다음 HOP으로 재투입할 때만 `Some(n)`을 사용한다.
4. `n`은 hidden tensor의 `dimensions[0]`에서 추정하지 않는다. HOP 생성자가 결정한
   논리 row 수와 descriptor/payload 수를 함께 검증한다.
5. `None`은 legacy 입력으로만 허용하고, 실행 직전에 phase별 기본값을 명시적으로
  적용한다. Decode에서 빈 row를 `Some(0)`으로 만들지 않는다.
6. C에서 단일-token 외부 투영을 유지할지 다중-token event/message를 도입할지 결정한
   뒤, 선택한 정책과 `HopComplete`/`Continue`/`Reply`의 순서를 일치시킨다.

응답을 받으면 다음을 확인한다.

- 결과 sequence id가 요청 sequence와 같은가.
- `n_tokens`가 `None`이면 legacy fallback을 적용하되, MTP 실행 중에는 조용히
  단일 행으로 downgrade하지 않는가.
- `Some(n > 0)`이 실제 descriptor/payload shape 및 terminal output과 일치하는가.
- outcome position이 terminal 결과일 때만 다음 HOP 위치로 채택되는가.
- middle 결과의 position echo를 전역 진행으로 오인하지 않는가.

현재 `outcome_from_result`는 하나의 `OutcomeMetadata`를 하나의 P4 `Outcome`으로
투영한다. MTP 구현에서는 이 경계를 명확히 나눠야 한다. 현재 코드 계약을 유지하는
경우 MTP 내부에서 여러 row를 처리하더라도 외부에는 한 `Outcome`/한 `Continue`
정책으로 투영해야 한다. 다중 visible token을 외부에 노출하기로 결정한 경우에만
`Outcome` 또는 그 이후 service message의 cardinality를 별도 변경한다.

- cut-set은 여러 row를 계속 운반한다.
- 외부 토큰은 `visible_count`만큼 token event를 순서대로 생성하거나, 저장소가
  허용하는 다중 토큰 event 표현으로 변환한다.
- `Reply::Token.index`는 기존 마지막 index 다음부터 연속 증가시킨다.
- `Done.generated`는 accepted/candidate 수가 아니라 실제 visible token 수를 더한다.
  단, 검증된 native `length` terminal은 request bound를 보고한다.
- stop/EOS/grammar/cancel 뒤의 후보 row는 외부 event로 내보내지 않는다.

즉 `Outcome` 하나에 후보 수나 accepted count를 억지로 넣지 않는다. P4 event 변환은
visible output만 보고, 검증된 length bound만 terminal 회계에 사용하며, rollback
boundary는 다음 HOP의 position으로 전달한다.

### 10.2 C++ protocol/server: transport 검증과 dispatch

관련 구조체는
[`protocol.hpp`](../layers/adapters/llamacpp/staged/server/src/protocol/protocol.hpp)의
`SequencePayload`와
[`protocol_hop.inc`](../layers/adapters/llamacpp/staged/server/src/protocol/protocol_hop.inc)의
encode/decode다.

구현할 검증은 다음과 같다.

- `n_tokens`가 없으면 legacy로 표시하되, MTP 실행 요청인지 여부는 server plan과
  runtime capability로 별도 판정한다.
- `n_tokens == 0`은 encode/decode 모두 `InvalidSequence`로 거부한다.
- `n_tokens > 0`은 `ProtocolLimits`와 loaded context의 batch/sequence 한계를 넘지
  않는지 확인한다.
- HOP envelope 안의 각 sequence는 독립적으로 row count와 position을 가진다.
- outcome metadata는 terminal에서만 생성되어야 하며 middle server는 input position을
  echo한다.
- decode된 row count와 실제 tensor descriptor 수를 혼동하지 않는다. rank-1 hidden
  tensor의 첫 dimension은 embedding width일 수 있다.

`Session::execute_hop`은 sequence별로 `StageRuntime::execute_hop`을 호출한다.
MTP 후보 생성은 여러 sequence의 전역 상태로 만들지 말고 sequence id별 runtime state로
분리한다. 한 sequence의 rollback 실패가 다른 sequence의 KV를 절단하거나 sampler를
재설정해서는 안 된다.

### 10.3 StageRuntime 상태 모델

현재 `StageRuntime`에는 `sequence_ids_`, `sequence_positions_`, `samplers_`,
`sampler_options_`가 있다. MTP 실행을 위해서는 이 상태를 다음처럼 확장해야 한다.

```text
SequenceRuntimeState {
    local_seq_id
    committed_position        // target KV가 확정한 경계
    speculative_end           // 이 stage가 마지막으로 쓴 범위의 exclusive 끝
    rollback_capability       // NO / PART / FULL / RS
    rollback_snapshot         // RS/FULL에서 필요한 native checkpoint
    sampler_state

    // terminal-only state; middle stages must not derive or advance position
    next_input_position       // base + committed_count
    candidate_tokens
    candidate_count
    accepted_count
    committed_count
    visible_count
}
```

실제 타입명은 구현 시 정할 수 있지만 다음 불변식은 고정한다.

- `candidate_tokens`와 terminal-only 회계(`candidate_count`, `accepted_count`,
  `committed_count`, `visible_count`)는 wire나 stage 간 상태가 아니다.
- `speculative_end`는 stage마다 다를 수 있으므로 terminal 결과만 저장해서는 안 된다.
- sequence id를 local `llama_seq_id`로 바꾸는 표는 기존처럼 stage runtime이 소유한다.
- sampler state는 sequence별로 유지하고, request options가 바뀌면 기존 sampler를
  폐기한 뒤 새 sampler를 만든다.
- `next_input_position`은 terminal에서만 관리하며 §10.6의 `base + committed_count`와
  같은 값이다. middle stage는 이 값을 계산하거나 position을 전진시키지 않는다.

### 10.4 load 단계: MTP 실행 전 capability 판정

현재 [`llama_stage_runtime.cpp`](../layers/adapters/llamacpp/staged/server/src/runtime/llama_stage_runtime.cpp)
는 MTP를 `mtp_ownership_probe`로만 허용하고, production execution은 거부한다.
실행을 열 때 load 순서를 다음처럼 분리한다.

1. model/context를 만든다.
2. stage가 full tail인지 판정한다. MTP auxiliary head와 target logits를 소유하는
   stage는 terminal 하나뿐이다.
3. target memory에 대해 `common_context_can_seq_rm()`과 동등한 probe를 수행한다.
4. `PART`/`RS`이면 실제 draft depth로 suffix 제거 또는 state restore를 시험한다.
5. `FULL`이면 speculative checkpoint를 저장하고 확정 경계로 복원할 수 있는지 시험한다.
6. `NO`이거나 probe가 실패하면 `mtp_execution=0`으로 유지하고 명시적인 capability
   unavailable 오류를 반환한다.
7. 모든 probe를 통과한 경우에만 `mtp_execution=1`을 capability report에 넣는다.

Probe는 “API 호출이 성공했는가”만 보지 않는다. 두 token 이상을 decode한 뒤 rejected
suffix를 제거하고, 다음 decode의 logits/KV 위치가 rejection 전 기준과 일치하는지까지
확인해야 한다. recurrent backend는 position cell이 없을 수 있으므로 snapshot 복원을
검증하지 않고 `PART`처럼 취급하면 안 된다.

### 10.5 일반 HOP 실행과 MTP 실행 분리

기존 [`llama_stage_runtime_hop.cpp`](../layers/adapters/llamacpp/staged/server/src/runtime/llama_stage_runtime_hop.cpp)의
일반 경로는 입력 tensor를 설정하고, `token_count`만큼 batch를 만들고, 한 번
`llama_decode`한 뒤 terminal sampler를 한 번 호출한다. 이 경로를 MTP 조건문으로
과도하게 오염시키지 말고 다음 두 실행기를 분리한다.

#### 일반 HOP

- input cut-set을 descriptor별로 설치한다.
- `token_count`만큼 batch를 만든다.
- sequence KV 위치를 memory에서 이어간다.
- tail에서만 일반 sampler를 호출한다.
- 하나의 visible token과 다음 position을 반환한다.

#### MTP HOP

- tail stage인지 확인한다.
- target context와 MTP context의 sequence id를 같은 logical sequence에 매핑한다.
- 현재 committed context를 기준으로 후보 생성 상한을 계산한다.
- candidate tokens를 MTP context에서 생성한다.
- target context에서 확정 token과 후보를 한 번의 verify decode batch로 계산한다.
- sampler가 candidate를 순서대로 accept/reject하고 첫 불일치에서 멈춘다.
- accepted target state를 commit하고 rejected suffix를 target/MTP 양쪽에서 정리한다.
- `visible_count`만 token event로 투영하고, 다음 HOP position은 실제 target KV
  commit 수(`committed_count`)를 기준으로 계산한다.

현재 [`llama_stage_runtime_mtp.cpp`](../layers/adapters/llamacpp/staged/server/src/runtime/llama_stage_runtime_mtp.cpp)의
`execute_mtp_hop`은 prompt와 `seq_id = 0`, draft 상한 1을 하드코딩한 ownership/test
경로이며 ordinary `execute_hop`에 연결되어 있지 않다. production 구현은 이 함수를
그대로 호출하는 방식이 아니라, sequence별 runtime state와 HOP input을 받는 실행
루틴으로 승격해야 한다. 이 경계를 닫기 전에는 현재 `mtp_execution=0`을 유지한다.

### 10.6 MTP 생성·검증 루프의 구체적 처리

MTP head가 `n`개인 경우 한 랩의 논리 흐름은 다음과 같다.

1. 이전 랩에서 확정된 context 끝을 `base`로 고정한다.
2. MTP context가 최대 `n`개의 후보를 만든다. `draft-max`, model layer limit,
   remaining, context limit를 terminal에서 clamp한다.
3. target verify batch에 `base`에서 시작하는 확정 입력과 후보 rows를 넣는다.
4. target을 한 번 decode해 필요한 logits rows를 얻는다.
5. sampler가 후보를 순서대로 비교한다.
6. 첫 불일치 전까지를 accepted로 정하고, 불일치 위치의 target sample을 새 확정
   token으로 포함한다.
7. EOS/stop/grammar/cancel이 발생하면 그 지점 이후 후보는 모두 폐기한다.
8. `accepted_count`, `candidate_count`, `committed_count`, `visible_count`를 내부
   결과로 분리한다.
9. accepted target KV와 sampler state만 다음 랩의 committed state로 남긴다.
10. 실제 target KV에 쓴 행 수를 `committed_count`로 기록하고, 다음 HOP으로 넘길
    staged `SequencePayload.position`은 `base + committed_count`로 계산한다. visible token event
    수는 별도로 회계한다. 이 내부 결과를 현재 외부 단일-token 계약으로 투영할지,
    다중-token event/message로 확장할지는 선행 관문 C의 결정에 따른다.

`llama.cpp`의 핵심 최적화는 “decode 1회 + sampler 최대 N+1회”다. 후보마다 별도
`llama_decode`를 호출하는 구현은 이 계약을 위반하고 MTP의 이점을 잃는다.

MTP context가 후보 생성 중 전진한 상태도 별도로 관리한다. rejected 후보를 단순히
다음 token으로 덮어쓴다고 가정하지 말고, target context, MTP context, sampler를
동일한 accepted boundary로 되돌릴 수 있어야 한다.

### 10.7 stage별 lazy rollback 구현

현재 [`llama_stage_runtime_hop_batch.cpp`](../layers/adapters/llamacpp/staged/server/src/runtime/llama_stage_runtime_hop_batch.cpp)의
`rollback_hop_batch()`은 실패한 multi-sequence HOP에서 새로 만든 native sequence
slot을 정리하는 경로다. 이것은 MTP speculative suffix rollback이 아니다. MTP를
구현할 때 기존 HOP 실패 정리와 candidate rejection 후의 target/MTP state rollback을
같은 성공 경로로 재사용하거나 같은 capability로 광고하지 않는다.

모든 stage는 다음 HOP을 처리하기 직전에 `prepare_hop(sequence, position)`을 수행한다.

```text
if no prior state:
    initialize committed_position = input.position
else if input.position < committed_position:
    reject as position regression
else if input.position < speculative_end:
    rollback suffix [input.position, speculative_end)
    committed_position = input.position
else if input.position == speculative_end:
    committed_position = input.position
else:
    if phase == Decode:
        reject as position gap corruption
    else:
        verify that the prefill gap is explicitly allowed
        committed_position = input.position

decode current rows
speculative_end = input.position + rows_written
```

`speculative_end`와 `position`은 모두 다음 입력 위치를 나타내는 exclusive 경계다.
따라서 llama.cpp의 `llama_memory_seq_rm(seq, p0, p1)`가 사용하는 `[p0, p1)` 규칙과
직접 대응하며, 구현 중간에 inclusive 값으로 변환하지 않는다.
각 backend adapter는 다음 동작을 구현한다.

- `PART`: `llama_memory_seq_rm(memory, seq, rollback_start, -1)`로 suffix를 제거하고,
  제거 후 `llama_memory_seq_pos_max`를 확인한다.
- `RS`: position 기반 삭제가 아니라 backend가 제공하는 제한된 rollback/snapshot
  경로를 호출하고, rollback depth가 `n_rs_seq` 이하인지 확인한다.
- `FULL`: speculative 시작 전 checkpoint를 보관하고 전체 sequence restore 후 필요한
  확정 prefix만 다시 설치한다. 부분 cut을 흉내 내서는 안 된다. 이 방식은 가능하지만
  비용이 크므로 이번 기본 구현 범위에서는 capability를 광고하지 않는다.
- `NO`: 새 HOP을 실행하지 않고 capability error를 반환한다.

rollback 실패 뒤에 새 decode를 진행하면 안 된다. stage의 메모리와 `sequence_positions_`
가 불일치한 상태가 되므로 해당 sequence를 invalid 상태로 격리하고, 복구 경로로
보내거나 요청을 실패시킨다.

동일 `input.position`의 HOP 재전송은 새 wire field 없이 idempotent retry로 취급한다.
즉 동일 position이 다시 도착하면 중복으로 조용히 무시하지 않고, 남아 있는
`[input.position, speculative_end)` suffix를 rollback한 뒤 같은 HOP을 재실행한다.
별도 hop identity가 wire에 없으므로 이 범위에서는 hop id 기반 중복 거부를 하지 않는다.

### 10.8 KV durable restore와 speculative state의 분리

[`llama_stage_runtime_kv.cpp`](../layers/adapters/llamacpp/staged/server/src/runtime/llama_stage_runtime_kv.cpp)는
현재 durable KV의 manifest, model identity, stage range, token position을 검증한다.
MTP 구현에서는 durable KV와 speculative state를 같은 것으로 취급하지 않는다.

현재 checkpoint/restore도 target prompt/KV 중심의 복구 경계이며, MTP draft context와
candidate sampler state를 함께 복구하는 계약이 아니다. 그러므로 현재 코드에서
restore가 성공했다는 사실만으로 MTP를 재개할 수 없다. MTP state를 durable하게
확장하지 않는 한, 아래처럼 speculative state를 무효화하고 ordinary decode로
재개해야 한다.

- durable restore가 성공해도 MTP candidate memory와 sampler speculative state는
  자동으로 유효해지지 않는다.
- restore 직후 `sequence_positions_`를 manifest의 token position으로 맞춘다.
- 각 stage의 in-memory speculative suffix와 snapshot을 삭제한다.
- terminal의 MTP context도 새 확정 prefix에서 다시 초기화한다.
- 다음 HOP은 ordinary decode로 시작하고, 새 MTP 후보는 그 결과 이후에 생성한다.
- `llama_synchronize` 이후에만 복원된 state로 graph를 실행한다.

### 10.9 출력·오류·관측성

구상 어댑터는 계산 결과뿐 아니라 실패 의미도 보존해야 한다.

필수 오류 조건:

- MTP가 terminal이 아닌 stage에서 요청됨
- capability probe가 실행 불가로 판정됨
- position regression 또는 rollback 범위 불일치
- row count와 batch/output descriptor count 불일치
- 현재 event cardinality 정책과 `HopComplete.outcomes`, `Outcome.text`, staged
  `SequencePayload.initial_tokens`, 외부 `Reply` 투영의 불일치
- candidate/accepted/committed/visible 회계가 음수 또는 context limit 초과
- staged `SequencePayload.position`이 실제 target KV commit position과 불일치
- sampler가 `LLAMA_TOKEN_NULL`을 반환
- restore 후 실제 KV max position이 manifest와 불일치
- 한 sequence의 rollback 실패를 다른 sequence 상태로 전파하려는 시도

관측성에는 최소한 다음을 남긴다.

- sequence id, hop id, phase
- input position, candidate count, accepted count, committed count, visible count
- rollback capability와 rollback 시작/끝
- target/MTP context별 committed position
- stop reason과 reconnect/restore 여부
- HOP row count와 실제 batch row count
- `hop_id`, `expected` sequence set, event kind와 외부 token index의 투영 결과

candidate token 자체나 prompt 원문은 기존 민감정보 정책을 따르며 기본 로그에 남기지
않는다. 현재 telemetry가 row count와 elapsed time을 기록하므로, 여기에 회계 및
rollback 결과를 추가하되 token 내용을 기록하지 않는다.

### 10.10 구상 어댑터 테스트 순서

구현은 다음 층으로 올린다.

1. **protocol unit**: `None`, `Some(1)`, `Some(n)`, `Some(0)`, trailing invariant,
   multi-sequence envelope.
2. **runtime unit**: position state machine, `PART/RS/FULL/NO` rollback adapter,
   sampler reset, restore invalidation.
3. **full-tail MTP test**: 현재 ownership test를 production-shaped sequence/HOP
   입력으로 바꾸고 draft/verify/accept 수와 position을 검증한다.
4. **multi-stage cut-set test**: terminal → stage 0 → … → terminal에서 row count와
   tensor cut-set이 보존되는지 확인한다.
5. **rejection matrix**: 전부 거부, 첫 후보 거부, 중간 거부, 전부 수락, EOS/stop.
6. **reconnect/restore**: speculative suffix와 MTP candidate state를 모두 폐기한 뒤
   ordinary decode가 baseline과 같은 logits/token을 내는지 확인한다.
7. **real model E2E**: PART/RS backend별로 baseline full model과 token/logits 및
   KV position을 비교하고, capability report와 실제 동작이 일치하는지 확인한다.

이 범위가 실제 구상 어댑터의 구현량이다. P4 추상 문서의 wire 변경이 작다는 사실은
이 내부 상태·메모리·샘플링·복구 작업을 줄이지 않는다.
