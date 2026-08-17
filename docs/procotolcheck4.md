# P4 protocol check 4: KV, identity, atomicity, and monitoring

감사 기준일: 2026-08-17. 현재 코드와 기존 [`protocol.md`](protocol.md)만
대조했다. 이 문서는 설계 제안과 현재 구현을 구분한다.

## 결론

현재 P4는 framed routing, route-hashed CPS worker, mock adapter의
`Persist/Restore/Fork/Discard` 동작은 구현되어 있다. 그러나 다음은 아직
완성된 프로토콜 계약이 아니다.

1. 실제 `llamacpp/served` adapter는 KV persistence를 지원하지 않고 항상
   실패한다.
2. `sequence`는 일반 inference에서 `Envelope.route`로 암묵적으로 파생되며,
   cache 명령에서는 body의 임의 문자열이다. `request_id`, `sequence_id`,
   cache operation id가 타입·wire 수준에서 분리되지 않는다.
3. `deployment/generation`은 일반 hop admission에는 쓰이지만 cache 작업은
   generation 검사를 우회한다. adapter의 resident/persisted map도 sequence
   하나만 키로 쓴다.
4. multi-stage cache 작업은 OUTER가 stage별 명령을 수동으로 보내야 한다.
   전 stage barrier, prepare/commit, rollback, idempotency가 없다.
5. `HopComplete`에는 hop correlation이 없고, node는 event가 오면 전체
   `in_flight`를 비운다. 늦은 event, 중복 event, 다른 generation의 event를
   안전하게 거부할 수 없다.
6. Status는 문자열 snapshot이라 cache operation, deployment/generation,
   active request/sequence/hop, event loss를 구조적으로 보고하지 않는다.

따라서 현재 상태는 “mock 기반의 KV verb 시험 구현”이지 “멀티노드에서
재시작 가능한 durable KV cache protocol”이 아니다.

## 1. 현재 경로와 식별자 대조

### 요청·응답 경로

현재 envelope에는 `target`, `recipient`, `route`, `reply_to`, `chain`이
있다. `to_next_hop()`은 route/reply address/chain을 유지하고 다음 link만
선택한다. 마지막 node의 `to_reply()`는 `reply_to`를 직접 target으로
사용한다. 직접 전송이 실패하면 outbound pump가 chain 첫 link를 한 번만
fallback으로 사용한다.

- [Envelope](../layers/protocol/src/envelope/mod.rs) (`route`, `reply_to`, `chain`)
- [Envelope::to_next_hop](../layers/protocol/src/envelope/mod.rs)
- [Envelope::to_reply](../layers/protocol/src/envelope/mod.rs)
- [Peers::hand_on](../layers/agent/src/transport/outbound/mod.rs)
- [Envelope wire encode/decode](../layers/protocol/src/envelope/wire/mod.rs)

단일 OUTER listener가 안정된 `reply_to` 주소를 유지하는 경우에는 마지막
node에서 원래 OUTER까지 돌아오는 경로가 존재한다. 그러나 `Peers`는 주소를
key로 connection을 공유하고, `Continuations`는 route 문자열 하나만 key로
쓴다. 하나의 OUTER 주소에 여러 독립 연결이 있거나 route가 재사용되면
연결별 demultiplexing이 없다.

- [Peers connection map](../layers/agent/src/transport/outbound/mod.rs)
- [Continuations registry](../layers/agent/src/continuation/mod.rs)

### 현재 필드의 실제 의미

| 개념 | 현재 구현 | 판정 |
| --- | --- | --- |
| `route` | envelope transport correlation; continuation key; OUTER reply grouping | transport id와 business request id가 혼합됨 |
| `sequence_id` | adapter contract에는 있지만 standard `Bodies::sequence()`가 `route`에서 생성 | 명시적 wire field가 아님 |
| `request_id` | 없음 | 정책 미정 |
| `reply_to` | 주소 하나 | OUTER connection/subscription 식별자가 없음 |
| `deployment_id` | chain current link의 `binding`을 adapter Work에 복사 | 문자열이며 namespace/검증 규칙 없음 |
| `generation` | chain link에 있고 일반 sequence work admission에서 비교 | cache lifecycle에는 비교되지 않음 |
| `hop_id` | 없음 | event correlation 불가 |
| cache operation id | 없음. cache reply route를 operation별로 caller가 임의 생성 | retry/idempotency 불가 |

근거:

- [Link: binding/generation](../layers/protocol/src/envelope/chain/mod.rs)
- [Bodies::sequence: route를 sequence로 사용](../layers/service/src/payload/mod.rs)
- [ToNode cache verbs](../layers/service/src/message/mod.rs)
- [Cache contract](../layers/adapters/adapter/src/work/cache/mod.rs)

### inference와 cache의 매핑 결함

`Execute`는 `Bodies::sequence()`에서 `frame.envelope.route`를 sequence로
사용한다. 반면 `Persist/Restore/Fork/Discard`는 body의 `sequence` 문자열을
사용하고 cache operation 자체의 envelope route와 연결하지 않는다.

이 구조는 다음 호출 규칙을 caller가 암묵적으로 지켜야만 동작한다.

```text
Execute route = S
Persist sequence = S
Restore sequence = S
후속 Execute route = S
```

`Restore sequence = S` 뒤에 다른 route `R`로 Execute하면 표준 payload에는
“R이 S에서 복원된 상태를 이어간다”는 정보가 없다. `Fork`도 `Cached` reply의
새 sequence 문자열을 caller가 다음 Execute route로 재사용해야 한다. 이를
검증하거나 강제하는 protocol invariant가 없다.

## 2. KV lifecycle 감사

### wire와 adapter contract

service wire에는 네 가지 verb와 `Cached { sequence, bytes, detail }` reply가
있다. adapter contract의 `Cache`에는 `deployment`, `sequence`, `action`만
있다. action에는 `Persist`, `Restore`, `Fork { into }`, `Discard`가 있다.

- [ToNode and Reply](../layers/service/src/message/mod.rs)
- [Binary cache encoding](../layers/service/src/message/wire.rs)
- [CacheAction](../layers/adapters/adapter/src/work/cache/mod.rs)

그러나 cache Work는 lifecycle로 분류되어 `Payload::lifecycle()`에서
`deployment`만 읽고, node의 generation admission을 우회한다.

- [Payload::lifecycle](../layers/service/src/payload/mod.rs)
- [Node::refusal](../layers/agent/src/node/runner/mod.rs)

즉 일반 hop은 `(binding, generation)`으로 stale materialisation을 거부할
수 있지만, cache 명령은 현재 node가 어느 deployment/generation을 보유하는지
확인하지 않은 채 adapter에 도달한다.

### mock adapter의 현재 의미

mock은 `produced: HashMap<String, Progress>`와
`persisted: HashMap<String, u64>`를 사용한다. 두 map 모두 key가
sequence 문자열뿐이며 deployment, generation, stage, model fingerprint,
cache format version이 없다.

- [Mock state maps](../layers/adapters/mock/src/lib.rs)
- [Mock cache implementation](../layers/adapters/mock/src/cache/mod.rs)

구현 동작은 다음과 같다.

| 동작 | 현재 구현 | 위험 |
| --- | --- | --- |
| Persist | resident map에서 remove 후 persisted map에 insert | durable commit 전 resident를 해제한다는 원자성 계약이 없음 |
| Restore | persisted map에서 remove 후 resident map에 insert | restore 실패 시 durable copy rollback 계약이 없음 |
| Fork | source bytes를 target sequence에 insert | target이 이미 있으면 조용히 overwrite |
| Discard | persisted map에서 remove | stage 전체 결과와 연결되지 않음 |

`Cache::subject()`가 Fork의 target sequence를 reply에 쓰도록 하는 점은
맞다. 하지만 reply에는 action, source sequence, deployment, generation,
operation id가 없어 OUTER가 어떤 상태 전이를 확인했는지 완전히 재구성할
수 없다.

### 실제 served adapter

`llamacpp/served`는 `sessions: HashMap<String, Session>`으로 HTTP streaming
session을 sequence 문자열 하나에 매핑한다. session에는 generation이나
deployment가 없다. `Work::Cache`는 `llama_state_seq_save_file`이 HTTP
surface에서 노출되지 않는다는 이유로 무조건 `Failed`를 emit한다.

- [Served session state](../layers/adapters/llamacpp/served/src/lib.rs)
- [Served hop/session lookup](../layers/adapters/llamacpp/served/src/lib.rs)
- [Served cache refusal](../layers/adapters/llamacpp/served/src/lib.rs)

따라서 현재 실제 backend 경로에서는 mock 테스트와 달리 persistence/reload/
fork/discard가 성공하지 않는다. `llamacpp/staged`도 현재 workspace member가
아니며, README가 Rust adapter와 C++ staged server가 아직 작성되지 않은
artifact라고 명시한다.

- [P4 workspace members](../Cargo.toml)
- [Adapter implementation inventory](../layers/adapters/README.md)

## 3. deployment/generation과 ownership

chain link는 `(address, node, binding, generation)`을 운반한다. node는
`Bound::At(generation)` 하나만 저장하고 일반 sequence work에 대해 현재
generation과 일치하는지 검사한다. node 자체도 하나의 `ceiling`, 하나의
`lifecycle`, 하나의 `Bound`를 가진다.

- [Node ownership fields](../layers/agent/src/node/runner/mod.rs)
- [Bound generation check](../layers/agent/src/node/runner/bound.rs)

이것은 “한 node 인스턴스가 한 active materialisation을 가진다”는 구현에는
맞지만, cache identity를 보호하지는 않는다. cache는 generation check를
우회하고, adapter state key에는 deployment/generation이 없다. 따라서 다음
상황을 현재 protocol이 거부하지 못한다.

1. sequence `S`를 deployment A/generation 1에서 persist한다.
2. 같은 node를 deployment B/generation 2로 reload한다.
3. cache 명령이 sequence `S`만 들고 도착한다.
4. adapter가 `S`를 현재 materialisation의 상태인지 확인할 계약이 없다.

cache ownership은 다음처럼 분리되어야 한다.

```text
OUTER/origin agent: request/sequence lineage와 multi-stage operation 소유
agent: stage node로의 admission, fan-out, barrier, 결과 집계 소유
node: binding/generation과 local stage ownership 검증
adapter: 실제 KV bytes와 local persistence transaction 소유
```

KV는 node 간에 자동 이동하지 않는다. pipeline의 각 stage가 자기 shard를
가지며, 같은 logical sequence에 대해 stage-local fragment가 존재해야 한다.
node 장애 시 “KV migration”이 아니라 compatibility 검증 후 request 재시작을
기본 정책으로 해야 한다. migration은 별도의 명시적 protocol이어야 한다.

## 4. multi-stage atomicity

현재 multi-stage test는 OUTER가 head와 tail에 cache command를 각각 보내고,
각 reply를 확인한 뒤 다음 단계로 진행한다. P4 내부에서 chain을 순회해
cache operation을 fan-out하거나 all-stage barrier를 만드는 코드는 없다.

- [Manual per-stage persist/restore test](../layers/service/tests/cache_in_a_deployment.rs)
- [Cache operation helper: one target chain](../layers/service/tests/common/conversation.rs)

따라서 현재 보장되는 것은 “각 stage에 명령을 따로 보내면 mock이 각자의
local map을 조작한다”뿐이다. 다음은 보장되지 않는다.

- stage 일부만 성공한 Persist/Restore/Fork/Discard의 aggregate result
- stage 전체가 준비될 때까지 sequence를 visible하게 하지 않는 barrier
- 중간 실패 시 이미 성공한 stage의 compensating cleanup
- process crash 중 durable write와 resident free 사이의 recovery
- retry/duplicate command의 idempotency
- target sequence overwrite 방지
- 모델, tokenizer, context, adapter, cache format 호환성 검증

### 결정할 transaction state

모든 cache operation은 `cache_operation_id`를 갖고 stage별로 다음 상태를
보고해야 한다.

```text
accepted -> preparing -> prepared -> committed -> visible
                         \-> failed -> rolled_back
```

정책:

- Persist: 각 stage가 임시 durable fragment를 prepare한다. 모든 stage가
  prepared가 된 뒤 commit하고 resident KV를 해제한다.
- Restore: 각 stage가 임시 resident state로 읽고 검증한다. all-stage
  barrier 뒤에만 sequence를 visible로 publish한다.
- Fork: source는 유지하고 target fragment를 임시 이름으로 복사한다. 전
  stage commit 전에는 target sequence를 사용할 수 없다.
- Discard: delete intent와 commit 결과를 기록한다. 이미 삭제된 retry는
  idempotent success 또는 명시된 `not_found` 정책 중 하나로 고정한다.

manifest 최소 필드:

```text
cache_snapshot_id
sequence_id
deployment_id
generation
pipeline/model fingerprint
tokenizer fingerprint
adapter kind
stage_id and layer span
cache_format_version
context/slot/KV layout
created_at, last_used_at, bytes, checksum
```

MTP/speculative decoding을 지원하는 adapter는 sampler state, draft-model
state, extra KV/state bytes를 manifest에 포함하거나, 해당 cache snapshot을
restore 불가로 명시해야 한다.

## 5. hop correlation과 event failure

`HopComplete`는 `deployment`와 `outcomes`만 가진다. hop id, generation,
operation/request id가 없다. node는 이 event를 받으면 `in_flight` 전체를
`mem::take()`하고 각 outcome의 sequence 문자열로 carrier를 찾는다.

- [HopComplete handling](../layers/agent/src/node/runner/events.rs)
- [in_flight map](../layers/agent/src/node/runner/mod.rs)
- [HopComplete event shape](../layers/adapters/adapter/src/event/report/mod.rs)

이 구조의 결과:

1. 두 deployment 또는 두 hop에서 같은 sequence가 쓰이면 map entry가
   overwrite될 수 있다.
2. 늦은 HopComplete가 현재 hop의 in-flight를 비울 수 있다.
3. 중복 HopComplete를 식별할 수 없다.
4. outcome이 일부만 오면 나머지 carrier가 reply 없이 사라진다.
5. `Failed { deployment, sequence }`도 node가 deployment를 검증하지 않고
   sequence 또는 전체 in-flight를 실패 처리한다.
6. `Loaded`, `Unloaded`, `Cached`에도 operation correlation이 없어서 stale
   lifecycle event를 현재 lifecycle request와 연결할 수 없다.

필수 정책:

```text
hop_id = one immutable execution pass at one node
event_seq = one monotonic sequence per request/hop stream
event key = (origin_agent, request_id, sequence_id, deployment_id,
             generation, stage_id, hop_id)
```

event consumer는 위 key가 현재 active operation과 일치할 때만 상태를
전이한다. 동일 event는 idempotent하게 무시하고, old generation/old hop은
`stale_event`로 기록하되 현재 operation을 변경하지 않아야 한다.

## 6. monitoring과 typed status

현재 Status는 lanes, peer count, continuation count, node depth/running/
waiting routes, opaque backend report를 한 문자열로 만든다.

- [Status snapshot formatter](../layers/service/src/status/mod.rs)
- [NodeStatus fields](../layers/agent/src/agent/mod.rs)
- [Adapter report contract](../layers/adapters/adapter/src/lib.rs)

현재 OUTER가 알 수 없는 것:

- cache operation id/action/source/target
- deployment id와 generation
- stage별 cache fragment 상태
- request/sequence/hop correlation
- prepare/commit/barrier state
- intermediate queue, event queue, outbox depth
- stale/duplicate event, refusal, event loss의 typed reason
- adapter가 실제로 cache를 지원하는지 여부
- resident KV와 durable KV의 bytes/checksum/last-used

최소 typed status는 다음을 가져야 한다.

```text
snapshot_seq, generated_at

agent:
  agent_id, address, uptime
  accepted, forwarded, refused, completed, failed
  lane depth/capacity, peer count/queue depth

nodes[]:
  node_id, adapter_kind, distribution
  deployment_id, generation, phase
  queue_ram, queue_spill, oldest_wait_ms
  in_flight, batch_width, active hop_id
  adapter cache capability
  resident_kv_bytes, durable_kv_bytes
  event_raised, event_lost, stale_event, duplicate_event

requests[]:
  request_id, sequence_id, cache_operation_id
  origin_agent, return_channel
  deployment_id, generation, stage_id, hop_id
  state, enqueue_time, start_time, last_progress, deadline

cache_operations[]:
  operation_id, action, source_sequence, target_sequence
  expected_stages, prepared_stages, committed_stages
  state, failure_reason, bytes, checksum
```

opaque `adapter_report`는 보조 정보로 유지할 수 있지만 위 구조화 필드를
대체하면 안 된다.

## 7. `protocol.md`와의 모순·누락

기존 [`protocol.md`](protocol.md)의 현재 기록과 코드의 차이는 다음과 같다.

| protocol.md | 코드 대조 | 정리 |
| --- | --- | --- |
| KV verb가 mock에 존재한다고 기록 | service wire에는 존재하지만 served adapter는 무조건 실패 | “P4가 지원”이 아니라 “mock만 성공, served는 capability failure”로 표시해야 함 |
| `route`와 `sequence_id` 분리를 정책으로 제안 | standard payload는 route를 sequence로 복사하고 cache는 별도 문자열을 받음 | 제안이 아직 wire/type invariant로 구현되지 않음 |
| restore full manifest를 요구 | Cache에는 deployment/generation/model/tokenizer/cache format 필드가 없음 | 정책만 있고 검증 경로 없음 |
| all-stage restore barrier를 요구 | test가 stage별 command/reply를 caller가 수동 수행 | P4 coordinator/barrier 없음 |
| typed status schema를 제안 | 실제 Status는 human-readable string | machine-readable status 미구현 |
| hop event ordering/idempotency를 요구 | HopComplete에 hop id/event sequence 없음 | stale/duplicate event 방어 불가 |

기존 문서의 unresolved 목록은 정확한 방향이지만, 위 항목들은 구현 전
결정사항이 아니라 현재 구현의 미충족 계약으로 분류해야 한다.

## 8. protocol 문서에 확정할 정책안

1. `request_id`는 한 사용자 inference operation의 immutable id로 한다.
2. `sequence_id`는 KV lineage id로 한다. token lap과 cache action에서 유지하며
   Fork만 새 target sequence를 발급한다.
3. `route_id`는 transport/exchange id로만 사용한다. `cache_operation_id`는
   Persist/Restore/Fork/Discard transaction id로 별도 둔다.
4. `origin_agent_id`와 `return_channel_id`를 mandatory로 한다. `reply_to`
   주소만으로 다중 OUTER connection을 구분하지 않는다.
5. 모든 inference/cache reply와 event는 request, sequence, deployment,
   generation, stage, hop 또는 operation correlation을 가진다.
6. cache manifest는 deployment/generation/model/tokenizer/adapter/stage/cache
   format compatibility를 검증하기 전까지 state를 visible로 publish하지
   않는다.
7. multi-stage cache는 origin agent가 coordinator이며 all-stage prepare/
   commit barrier를 소유한다. partial success는 성공으로 보고하지 않는다.
8. adapter는 local durable KV와 resident KV의 ownership을 갖고, P4 core는
   bytes를 해석하지 않는다. 단, adapter가 지원하지 않는 cache capability는
   load/status에서 명시적으로 보고한다.
9. hop/event는 `(request_id, sequence_id, deployment_id, generation,
   stage_id, hop_id)`로 match한다. duplicate는 idempotent ignore, stale는
   상태 불변 + typed telemetry로 처리한다.
10. typed status는 queue/cache/event/request operation을 각각 구조화해
    보고한다. 문자열 backend report는 확장 필드일 뿐 핵심 상태가 아니다.

## 9. 검증 결과와 남은 테스트

실행한 검증:

```text
cargo test -p p4-service --test cache --test cache_in_a_deployment
7 passed
```

이 테스트가 증명하는 것:

- mock에서 persist 후 restore가 progress를 이어감
- mock fork가 source를 유지하고 target을 생성함
- discard 후 재 discard가 실패함
- cache가 model binding을 직접 해제하지 않음
- multi-stage에서 caller가 각 stage에 명령을 별도로 보내면 각 local
  fragment가 복원됨

이 테스트가 증명하지 않는 것:

- served llama.cpp/vLLM/SGLang의 실제 persistence
- stage 일부 실패 시 all-stage rollback/cleanup
- crash 중간의 durable commit atomicity
- deployment/generation mismatch restore 거부
- multi-OUTER route collision 방지
- duplicate/late HopComplete 격리
- typed status의 snapshot consistency
- cache quota, checksum, format migration, encryption
- MTP/speculative state의 persistence

이번 작업에서는 이 문서 외 다른 파일을 수정하지 않았다.
