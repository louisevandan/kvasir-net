# P4 pipeline parallel scheduling audit

현재 코드와 [`STATUS.md`](../../../STATUS.md), [`protocol.md`](protocol.md),
P4 테스트를 대조한 read-only 감사 결과다. 이 문서는 수정안을 구현하지
않고, 현재 보장되는 동작과 아직 정책·프로토콜로 고정되지 않은 부분을
분리한다.

검증 시점: 2026-08-17. `apps/p4`에서 `cargo test --workspace` 전체 통과.
테스트 통과는 정상 mock 경로의 증거이며, 실제 staged GPU adapter와 장기
메모리 boundedness의 증거는 아니다.

## 결론

현재 P4는 다음을 정상 경로에서 보여준다.

- 한 node가 한 번에 하나의 hop만 실행한다.
- 하나의 hop은 load가 선언한 `ceiling` 이하의 sequence window를 갖는다.
- `Prefill`과 `Decode`는 별도 lane이며, decode lap은 chain을 처음부터 다시 돈다.
- 서로 다른 node의 독립적인 event loop 때문에 mock stage 간 overlap이 발생한다.
- route-hash worker가 한 route의 frame을 같은 worker에 배치한다.

그러나 “지속 요청이 GPU 유휴 순간마다 안전하게 공급되고, 모든 대기와
backpressure가 bounded하며, 실제 staged adapter에서도 pipeline overlap이
보장된다”고 판정할 수는 없다. 현재 보장은 `local node scheduling + mock
pipeline` 수준이다.

## 기존 `protocol.md`와의 판정

| 영역 | `protocol.md`의 주장 | 코드 감사 판정 |
| --- | --- | --- |
| Return path | `reply_to`와 chain 첫 link fallback | 직접 반환은 구현됐지만 OUTER connection identity와 origin agent는 없음 |
| Pipeline overlap | local window와 독립 node hop | mock에서는 입증, 실제 staged GPU에서는 미구현 |
| Queueing | agent/peer는 bounded, node ingress는 unbounded | 정확함. 단, unbounded outbox/event도 장기 압박 경로임 |
| Monitoring | human-readable status | 정확함. hop/event/queue 단계별 기계적 correlation 없음 |
| Options | opaque JSON | 전달은 되지만 validation/capability/MTP 정책 없음 |
| KV | mock의 persist/restore/fork/discard | 실제 served adapter는 HTTP surface상 persist 불가 |

## 1. Pipeline scheduling과 continuous arrivals

### 확인된 정상 경로

`Node::drain`은 `queue.is_running()`이면 새 hop을 시작하지 않고, window를
조합해 `spawn_blocking`으로 adapter를 호출한다.

- [`runner/mod.rs#L162-L205`](../layers/agent/src/node/runner/mod.rs#L162-L205)
- [`queue/mod.rs#L73-L105`](../layers/agent/src/node/queue/mod.rs#L73-L105)

`compose`는 현재 decode sequence 수를 `carrying`으로 세고, 아직 ceiling에
도달하지 않았으면 fresh prefill을 먼저 채운다. ceiling에 도달하면 decode가
우선이다.

- [`window/mod.rs#L33-L80`](../layers/agent/src/node/window/mod.rs#L33-L80)

따라서 연속 요청이 충분히 들어오면 stage 0이 다음 prefill을 실행하는 동안
stage 1은 앞선 요청을 실행할 수 있다. 이 동작은 mock 기반
[`pipelining.rs#L125-L208`](../layers/agent/tests/pipelining.rs#L125-L208)와
[`queues.rs#L260-L319`](../layers/agent/tests/queues.rs#L260-L319)에서
검증된다.

### 보장하지 않는 부분

현재 scheduler는 각 node의 local queue와 local ceiling만 본다. 다음 계약이
없다.

- downstream stage readiness
- stage 간 credit 또는 window depth
- downstream queue가 수용 가능한 batch width
- 최소 batch width와 partial batch 정책
- adapter가 실제로 허용하는 max batch/max concurrency
- stage별 blocked 상태와 upstream feed 중단 시점

그러므로 stage 0은 stage 1이 느리거나 downstream send가 막혀도 다음 hop을
계속 생산할 수 있다. 그 결과 “GPU를 계속 공급한다”가 아니라 “다음 stage로
보낼 frame을 메모리에 계속 쌓는다”로 변할 수 있다.

`protocol.md`가 요구한 global admission/credit와 feed 보장은 아직 문서
정책에 머물며, wire나 adapter contract에는 없다.

## 2. Node window, ceiling, prefill/decode

### 보장되는 것

- window 폭은 `take(ceiling)`으로 제한된다.
- 하나의 node는 정상 `HopComplete`까지 다음 hop을 시작하지 않는다.
- `to_next_lap`은 chain을 restart하고 lane을 `Decode`로 바꾼다.
- `NodeQueue.running`은 현재 adapter에 넘긴 sequence 수를 기록한다.

근거:

- [`window/mod.rs#L54-L80`](../layers/agent/src/node/window/mod.rs#L54-L80)
- [`envelope/mod.rs#L71-L80`](../layers/protocol/src/envelope/mod.rs#L71-L80)
- [`queue/mod.rs#L15-L28`](../layers/agent/src/node/queue/mod.rs#L15-L28)

### ceiling의 의미가 좁다

`ceiling`은 Load message가 선언한 값이며, adapter capability와 협상하지
않는다. `Adapter`에는 max batch, max concurrent sequences, readiness,
backpressure, cancellation 계약이 없다.

- [`adapter/src/lib.rs#L27-L37`](../layers/adapters/adapter/src/lib.rs#L27-L37)
- [`service/src/message/mod.rs#L49-L66`](../layers/service/src/message/mod.rs#L49-L66)
- [`payload/mod.rs#L64-L69`](../layers/service/src/payload/mod.rs#L64-L69)

따라서 P4가 보장하는 것은 “선언된 ceiling을 넘겨 호출하지 않음”이지,
“구상 adapter의 실제 처리 제한에 맞음”이 아니다. 특히 served adapter는
window 안의 각 sequence마다 blocking HTTP 요청용 OS thread를 만든다.

- [`served/src/lib.rs#L163-L188`](../layers/adapters/llamacpp/served/src/lib.rs#L163-L188)

ceiling이 과도하게 선언되면 P4의 worker 수가 아니라 adapter 쪽 thread와
backend connection 수가 먼저 머신을 압박할 수 있다.

### 현재 구현의 prefill payload 불일치

adapter contract는 later stage의 `prompt`가 absent이고 decode가 이전
position/state를 이어받는 형태를 설명한다.

- [`adapter/src/work/hop/mod.rs#L35-L48`](../layers/adapters/adapter/src/work/hop/mod.rs#L35-L48)

하지만 service payload는 모든 stage와 모든 lap에서 다음 값을 재생성한다.

- `sequence = envelope.route`
- `position = 0`
- `prompt = Some(prompt)`
- `remaining = max_tokens`

근거: [`service/src/payload/mod.rs#L15-L37`](../layers/service/src/payload/mod.rs#L15-L37)

현재 mock과 served adapter는 자체 session/progress로 이를 보정하므로 테스트가
통과한다. 그러나 staged adapter가 contract의 `position`, later-stage prompt,
remaining을 신뢰하면 prefill 재실행 또는 잘못된 decode 위치가 된다.

## 3. 실제 pipeline parallel 구현 여부

`STATUS.md`는 staged adapter가 아직 Rust로 구현되지 않았고, 현재 four-GPU
deployment는 하나의 P4 node 뒤에 있다고 명시한다.

- [`STATUS.md#L120-L128`](../../../STATUS.md#L120-L128)

현재 served adapter도 `Distribution::Internal`만 반환한다.

- [`served/src/lib.rs#L270-L273`](../layers/adapters/llamacpp/served/src/lib.rs#L270-L273)

따라서 다음은 입증되지 않았다.

- llama.cpp staged layer-range load
- node 간 hidden-state 전달
- stage-local KV window
- 실제 GPU 간 prefill/decode overlap
- 실제 GPU utilization이 idle gap 없이 유지되는지
- staged adapter의 MTP/speculative state 전달

현재 overlap 수치의 직접 근거는 mock adapter다. `STATUS.md`도 이를
“mocks rather than cards”로 구분한다.

## 4. Worker와 CPS

### 잘 된 부분

adapter의 blocking `start`는 agent worker 안에서 직접 실행되지 않고
`spawn_blocking`으로 분리된다.

- [`runner/mod.rs#L196-L205`](../layers/agent/src/node/runner/mod.rs#L196-L205)

node-bound worker는 frame을 node queue로 넘기고 끝나는 형태를 의도한다.

### 실제 모순

forward path는 worker 내부에서 peer queue가 비워질 때까지 기다린다.

- [`agent/mod.rs#L220-L223`](../layers/agent/src/agent/mod.rs#L220-L223)
- [`agent/mod.rs#L294-L305`](../layers/agent/src/agent/mod.rs#L294-L305)
- [`transport/outbound/mod.rs#L77-L103`](../layers/agent/src/transport/outbound/mod.rs#L77-L103)

즉 “worker는 기다리지 않는다”는 [`constraints.md#L7-L15`](constraints.md#L7-L15)의
계약과 실제 `Peers::send().await`가 충돌한다. 한 peer가 막히면 그 peer로
hash된 worker와 그 worker에 배정된 다른 route가 head-of-line blocking을
겪는다. async wait이므로 OS thread를 직접 잠그지는 않지만 CPS의 짧은
procedure 경계는 깨진다.

또한 `Budget::in_flight` semaphore는 acquire/release되지 않고 worker 수를
계산하는 데만 사용된다.

- [`agent/mod.rs#L288-L321`](../layers/agent/src/agent/mod.rs#L288-L321)

따라서 이름 그대로의 in-flight message budget이 아니라 worker-count
힌트다. 각 worker inbox의 `WORKER_DEPTH=64`도 status에 나타나지 않는다.

## 5. Fairness

### 존재하는 fairness

agent main receiver는 control/response/decode/prefill를 우선 선택하되 매
16번째 take에서 우선순위를 내려놓는다.

- [`queue/main/mod.rs#L42-L43`](../layers/agent/src/queue/main/mod.rs#L42-L43)
- [`queue/main/mod.rs#L122-L158`](../layers/agent/src/queue/main/mod.rs#L122-L158)

node select도 event와 work를 biased하지 않아 event 폭주가 arrivals를
무기한 가리지 않도록 한다.

- [`runner/mod.rs#L109-L144`](../layers/agent/src/node/runner/mod.rs#L109-L144)

### 결정되지 않은 fairness

node window에는 요청별 aging, deadline-aware fairness, weighted fairness,
최대 대기시간 또는 round-robin이 없다. 하나의 decode cohort가 계속 살아
있고 ceiling이 가득 차면 fresh prefill은 계속 뒤로 밀린다.

이것은 기존 정책상 의도된 “full이면 decode 우선”이지만, 장기 요청에서
새 요청의 starvation 상한을 보장하지 않는다. 반대로 prefill이 ceiling을
채우는 동안 decode가 늦어지는 경우의 공정성도 별도 계약이 없다.

필요한 정책 선택:

1. 기존 KV를 가진 decode의 우선순위를 유지하되 fresh prefill의 최대 대기시간을 둔다.
2. deadline이 가까운 route를 우선할지 명시한다.
3. window 안 route 순서와 stage 간 route 순서를 별도로 정의한다.
4. worker hash로 인한 worker별 head-of-line blocking을 허용할지 금지할지 정한다.

## 6. Queue와 backpressure

### bounded인 경로

- agent lanes: Tokio bounded channel
- peer queue: `PER_PEER_DEPTH=4096`
- connection count: semaphore

근거: [`queue/main/mod.rs#L58-L78`](../layers/agent/src/queue/main/mod.rs#L58-L78),
[`transport/outbound/mod.rs#L24-L39`](../layers/agent/src/transport/outbound/mod.rs#L24-L39),
[`transport/inbox/mod.rs#L21-L41`](../layers/agent/src/transport/inbox/mod.rs#L21-L41)

### unbounded인 핵심 경로

node spawn 시 work, adapter event, node outbox가 모두 unbounded다.

- [`runner/mod.rs#L45-L48`](../layers/agent/src/node/runner/mod.rs#L45-L48)
- [`runner/mod.rs#L67-L69`](../layers/agent/src/node/runner/mod.rs#L67-L69)
- [`runner/handle.rs#L17-L24`](../layers/agent/src/node/runner/handle.rs#L17-L24)

실제 `NodeQueue`도 capacity가 없는 `VecDeque`다.

- [`queue/mod.rs#L13-L16`](../layers/agent/src/node/queue/mod.rs#L13-L16)

outbox pump가 bounded agent queue에 기다리는 동안 node event loop는 다음
hop을 계속 끝내고 unbounded outbox에 frame을 넣을 수 있다. 그러므로
“backpressure가 producer까지 전달된다”는 문서 설명과 달리, 그 앞에
unbounded memory reservoir가 있다.

socket lane refusal은 현재 protocol reply가 아니라 stderr 로그다.

- [`transport/inbox/mod.rs#L65-L97`](../layers/agent/src/transport/inbox/mod.rs#L65-L97)

### 필요한 정책

최소 정책은 다음 중 하나가 아니라 조합이어야 한다.

`bounded RAM FIFO + bounded optional durable spill + explicit queue_full`

각 queue에 대해 다음을 wire/status에 노출해야 한다.

- capacity와 현재 depth
- RAM depth와 spill depth
- oldest enqueue time와 deadline expiry count
- refused count와 refusal reason
- producer가 blocked인지 adapter가 blocked인지
- spill quota, checksum, recovery state

## 7. Monitoring과 protocol identity

현재 status는 문자열 snapshot이다.

- [`service/src/status/mod.rs#L18-L50`](../layers/service/src/status/mod.rs#L18-L50)

노출되는 것은 agent lane, peer count, node depth/running/routes/backend text다.
다음은 없다.

- `request_id`, `sequence_id`, `hop_id`
- origin agent와 OUTER return channel
- queue별 intermediate depth와 capacity
- worker inbox/outbox/event backlog
- lifecycle/ready/prefill/decode/blocked phase
- snapshot sequence와 generated timestamp
- event loss와 socket refusal 누적값
- stale/duplicate event 판정 결과

`reply_to`는 직접 반환 주소이고, 직접 전송이 실패하면 chain 첫 link로 한
번 fallback한다.

- [`envelope/mod.rs#L84-L109`](../layers/protocol/src/envelope/mod.rs#L84-L109)
- [`envelope/mod.rs#L112-L132`](../layers/protocol/src/envelope/mod.rs#L112-L132)

이는 단일 OUTER listener의 정상 경로에는 충분하지만, 여러 OUTER connection이
같은 address를 쓰거나 NAT/reconnect가 발생하면 route만으로 어느 connection에
보낼지 결정할 수 없다. `protocol.md`의 origin agent/return channel 결정은
아직 미결이다.

## 8. Hop event correlation 결함

adapter `HopComplete`는 deployment와 outcomes만 운반하고 hop identity를
운반하지 않는다.

- [`adapter/src/event/report/mod.rs#L33-L38`](../layers/adapters/adapter/src/event/report/mod.rs#L33-L38)

node는 `HopComplete`를 받으면 현재 `in_flight` 전체를 꺼내 outcome sequence와
맞춘다.

- [`runner/events.rs#L52-L74`](../layers/agent/src/node/runner/events.rs#L52-L74)

따라서 늦은 event, duplicate event, 다른 generation/deployment의 event를
현재 hop과 구분할 수 없다. 정상 mock은 한 hop이 한 completion만 내므로
테스트가 이 결함을 드러내지 않는다.

더 심각한 경로가 있다. served adapter는 한 window의 일부 sequence가
prefill에 실패하면 `Failed`를 먼저 raise하고, 이후 `HopComplete`를 raise한다.

- [`served/src/lib.rs#L195-L218`](../layers/adapters/llamacpp/served/src/lib.rs#L195-L218)
- [`served/src/lib.rs#L139-L149`](../layers/adapters/llamacpp/served/src/lib.rs#L139-L149)

node의 `Failed` 처리 는 `queue.finished()`와 `drain()`을 즉시 호출한다.

- [`runner/events.rs#L76-L102`](../layers/agent/src/node/runner/events.rs#L76-L102)

그 시점에는 원래 `spawn_blocking`의 adapter hop이 아직 `HopComplete`를
발행하지 않았을 수 있으므로, 다음 hop이 앞선 adapter call과 겹칠 수 있다.
“node는 한 hop만 실행한다”는 불변식은 정상 completion에서만 성립한다.

필수 정책:

- `hop_id`는 node/deployment/generation마다 새로 발급한다.
- event는 `deployment_id + generation + hop_id + sequence_id`를 가진다.
- terminal event는 한 번만 적용한다.
- stale/duplicate/unknown event는 상태를 변경하지 않고 counter에 기록한다.
- partial sequence failure와 whole-hop failure를 구분한다.

## 9. Generation options, MTP, sampling

`Execute`는 `prompt`, `max_tokens`, `options: String`만 가진다.

- [`service/src/message/mod.rs#L59-L66`](../layers/service/src/message/mod.rs#L59-L66)

served adapter는 기본 request를 만든 뒤 options object의 key로 전체 object를
덮어쓴다.

- [`served/src/chat/mod.rs#L12-L40`](../layers/adapters/llamacpp/served/src/chat/mod.rs#L12-L40)

따라서 `options`가 `model`, `messages`, `max_tokens`, `stream`을 다시 쓰는
precedence가 정의되어 있지 않다. JSON이 아니거나 object가 아니면 조용히
무시된다. 현재 테스트는 이 동작을 검증하지만, 정책상 안전한 동작인지는
결정하지 않았다.

현재 공통 protocol에는 다음이 없다.

- typed sampling subset와 field precedence
- stop strings/IDs, top-k/min-p, penalties, grammar/JSON schema, logit bias
- tokenizer/template 선택
- unsupported option의 reject/warn/default 정책
- adapter capability negotiation
- speculative decoding/MTP의 token granularity와 accepted-token accounting
- MTP KV state의 ownership/persistence

MTP는 임의 JSON key를 전달한다고 지원되는 것이 아니다. 토큰 수, hop 결과,
KV ownership과 completion accounting을 바꾸므로 adapter capability와 명시적
협상이 필요하다.

## 10. KV cache와 request mapping

service는 backend `SequenceId`를 `Envelope.route`에서 파생한다.

- [`service/src/payload/mod.rs#L25-L36`](../layers/service/src/payload/mod.rs#L25-L36)

이는 간단한 단일 실행에는 동작하지만 `route`, `request_id`,
conversation/sequence identity가 분리되어 있지 않다. route 재사용, fork 후
새 request, reconnect 후 동일 conversation 재개에 대한 정책이 없다.

mock은 persist/restore/fork/discard를 구현하고 multi-stage test도 있다. 반면
served adapter는 HTTP surface로 KV를 persist할 수 없다고 명시적으로 실패한다.

- [`adapter/src/work/cache/mod.rs#L23-L70`](../layers/adapters/adapter/src/work/cache/mod.rs#L23-L70)
- [`served/src/lib.rs#L295-L305`](../layers/adapters/llamacpp/served/src/lib.rs#L295-L305)

아직 정해지지 않은 정책:

- 모든 stage restore 성공 전 공개 금지 여부
- stage별 KV와 global conversation의 ownership
- model fingerprint, tokenizer fingerprint, generation compatibility
- persist/free와 durable commit 사이 crash recovery
- cache format version, checksum, encryption, quota, eviction
- MTP/speculative state의 저장 형식

## 11. 누락 테스트

다음 테스트가 추가되기 전에는 pipeline scheduling을 안정된 protocol
contract로 판정하지 않는다.

1. **Partial hop failure:** 한 window의 sequence 하나가 `Failed`를 raise한
   뒤 `HopComplete`를 내도록 하고 adapter peak hop이 1인지 검증한다.
2. **Stale/duplicate event:** 잘못된 deployment/generation/hop과 duplicate
   completion이 현재 in-flight route를 오염시키지 않는지 검증한다.
3. **Downstream saturation soak:** node queue, work channel, event channel,
   outbox, worker inbox, peer queue별 peak와 RSS를 지속 arrival로 측정한다.
4. **Worker HOL:** 한 peer queue를 막고 같은 worker에 배정된 다른 route가
   다른 peer/노드로 진행 가능한지 검증한다.
5. **Fairness bound:** endless decode cohort와 fresh prefill의 최대 대기시간,
   deadline 우선 정책, 반대 방향 starvation을 검증한다.
6. **Payload progression:** later stage prompt omission, position 증가,
   remaining 감소, route와 sequence 분리를 검증한다.
7. **Options safety:** reserved field override, malformed JSON, unsupported
   option, capability mismatch, MTP/speculative output accounting을 검증한다.
8. **Return identity:** 같은 listener의 여러 OUTER connection, reconnect,
   route collision, origin agent fallback을 검증한다.
9. **Atomic KV:** 한 stage restore 실패, process crash, generation mismatch,
   all-stage barrier와 partial cleanup을 검증한다.
10. **Real staged E2E:** 실제 layer-range adapter, hidden-state boundary,
    stage-local KV와 GPU utilization을 mock과 별도로 측정한다.

## 최종 판정

P4는 framed routing, local window composition, normal mock pipeline overlap,
per-route ordering의 구현체로는 동작한다. 그러나 현재 protocol은

`request identity → origin/return channel → hop correlation → stage credit →
bounded queue → adapter capability → KV ownership`

전체를 하나의 명시적 계약으로 연결하지 못한다. 따라서 현재 문서상의
정확한 표현은 “working framed routing and mock-pipeline implementation”이며,
완전한 durable multi-OUTER real-GPU pipeline protocol로 판정해서는 안 된다.
