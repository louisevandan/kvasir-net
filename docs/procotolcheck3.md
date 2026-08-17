# P4 generation options protocol check 3

이 문서는 [`protocol.md`](./protocol.md)의 미결정 항목을 현재 P4 코드와
workspace 테스트 기준으로 재검수한 기록이다. 외부 backend 문서나 실행 중인
GPU를 근거로 삼지 않는다. 결론은 다음 네 가지로 구분한다.

- **확정**: 현재 코드와 테스트가 같은 의미를 보장한다.
- **모순**: `protocol.md` 또는 주석의 계약과 현재 코드가 다르다.
- **누락**: 코드가 일부 동작하지만 wire/API 계약과 검증 정책이 없다.
- **수정안**: 다음 protocol revision에서 고정해야 할 계약.

## 0. 감사 대상과 실제 구현 경계

현재 workspace는 [`apps/p4/Cargo.toml`](../Cargo.toml)의 다음 실행 adapter만
포함한다.

| 구현 | 현재 사실 | 판정 |
| --- | --- | --- |
| `p4-llamacpp-served` | `Served` 하나가 llama.cpp, vLLM, SGLang 이름으로 등록된다. | 실제 구현 |
| `p4-mock` | Internal/Staged 두 분포와 cache 동작을 검증한다. | 테스트 구현 |
| `llamacpp/staged` | C++ compatibility patch와 계획만 있고 Rust adapter crate/workspace member가 없다. | 실제 실행 미구현 |
| vLLM/SGLang 전용 adapter | 없다. `Flavour` 차이는 model-name 확인과 launch 정책뿐이다. | 별도 semantics 미검증 |

근거: [`entrypoints/agent/src/adapters/mod.rs`](../entrypoints/agent/src/adapters/mod.rs),
[`served/src/flavour/mod.rs`](../layers/adapters/llamacpp/served/src/flavour/mod.rs),
[`served/src/lib.rs`](../layers/adapters/llamacpp/served/src/lib.rs).

따라서 이 문서의 “served” 판정은 세 backend의 실제 native semantics가 아니라
공통 OpenAI-compatible HTTP surface에 대한 현재 Rust adapter의 판정이다.

### 책임 경계 보정

이 검수에서 제안한 typed common generation schema와 capability envelope은
추상 P4 protocol의 요구사항이 아니다. generation semantics와 llama-server
switch 지식은 OUTER가 소유하며, OUTER가 만든 문자열/opaque body를 P4가
보존해 adapter로 전달한다. adapter는 그 문자열을 concrete backend 요청이나
실행 switch로 해석한다. 아래의 typed schema 제안은 P4 wire 변경안이 아니라
OUTER-선택 adapter 간 별도 계약으로 읽어야 한다.

## 1. 기준 protocol.md와 현재 코드의 총괄 판정

| 영역 | protocol.md의 방향 | 현재 코드 | 판정 | 우선 수정 |
| --- | --- | --- | --- | --- |
| return identity | origin agent와 return channel 명시 필요 | `reply_to`와 chain 첫 link로 추론 | 누락 | 높음 |
| request identity | `request_id`, `sequence_id`, `hop_id` 분리 필요 | `route`가 continuation 및 backend sequence 역할을 겸함 | 모순/누락 | 높음 |
| generation options | OUTER가 만든 opaque content를 P4가 보존 | opaque JSON object를 adapter가 concrete request로 병합 | P4 transport는 충족, adapter 계약은 별도 |
| P4-owned fields | P4는 generation semantics를 소유하지 않음 | adapter 내부 merge precedence는 adapter/OUTER 계약 | P4 모순 아님 |
| sampling/decoding | OUTER-어댑터 계약의 책임 | temperature/top_p/seed를 포함한 임의 key 전달 | P4는 해석하지 않음 |
| stop/output | 종료와 token accounting 필요 | `finish_reason`와 문자열 text만 전달 | 누락/결함 | 높음 |
| grammar/structured output | 지원 여부 협상 필요 | 요청에 실릴 수 있으나 응답 metadata가 없음 | 누락 | 높음 |
| MTP/speculative | adapter capability 및 state 계약 필요 | typed field, load option, telemetry 없음 | 미구현 | 높음 |
| KV | multi-stage atomic mapping 필요 | mock verbs만 완성, served는 실패 반환 | 부분 확정 | 높음 |
| pipeline | stage credit/feed/utilization 정책 필요 | mock overlap만 검증, 실제 staged adapter 없음 | 미검증 | 높음 |
| monitoring | typed correlated status/event 필요 | human-readable status와 free-form backend report | 누락 | 높음 |

## 2. Inference identity와 return path

### 확정

현재 inference envelope은 `target`, `recipient`, `route`, `reply_to`, `chain`,
`deadline_unix_ms`를 가진다. node는 마지막 hop에서 `reply_to`로 응답하고,
직접 도달할 수 없으면 outbound pump가 chain의 첫 link를 한 번만 fallback으로
사용한다.

근거: [`layers/protocol/src/envelope/mod.rs`](../layers/protocol/src/envelope/mod.rs),
[`layers/agent/src/transport/outbound/mod.rs`](../layers/agent/src/transport/outbound/mod.rs).

관련 topology와 relay 테스트는 통과한다. 따라서 단일 OUTER listener 주소가
안정적이고 route가 충돌하지 않는다는 가정 아래의 return path는 현재 작동한다.

### 모순/누락

`protocol.md`가 요구한 `origin_agent`, `return_channel`, `request_id`,
`sequence_id`, `hop_id`는 현재 wire에 없다.

- `route`는 agent continuation key이면서 `Bodies::sequence()`가 만드는
  backend sequence ID이기도 하다.
- `reply_to`는 TCP listener 주소일 뿐 개별 OUTER connection이나 subscription을
  식별하지 않는다.
- hop 완료 event에는 hop ID가 없다.
- 재연결, route 재사용, duplicate frame, retry의 idempotency 정책이 없다.

근거: [`layers/agent/src/continuation/mod.rs`](../layers/agent/src/continuation/mod.rs),
[`layers/service/src/payload/mod.rs`](../layers/service/src/payload/mod.rs),
[`layers/adapters/adapter/src/event/report/mod.rs`](../layers/adapters/adapter/src/event/report/mod.rs).

### 수정안

다음 필드를 분리한다.

```text
request_id       한 inference 실행의 불변 식별자
sequence_id      KV/backend conversation 식별자
origin_agent     최초 ingress agent
return_channel   OUTER logical subscription/connection 식별자
hop_id           한 node에서 한 번 실행되는 hop 식별자
attempt          재시도 세대
```

`route`는 transport ordering key로 남길 수 있지만 sequence identity로 재사용하지
않는다. 모든 token, terminal, failure, cache event는 최소한
`request_id/sequence_id/hop_id`를 반환해야 한다.

## 3. Generation options

### 현재 전달 경로: 확정

`ToNode::Execute`는 `prompt`, `max_tokens`, `options`를 가진다.

- service wire가 세 값을 length-prefixed text/number로 직렬화한다.
- `Bodies::sequence()`가 options를 `Sequence.options`로 보존한다.
- 중간 agent/node는 body를 opaque하게 운반한다.
- served adapter의 `Request::body()`가 JSON object options를 HTTP request root에
  병합한다.

근거: [`layers/service/src/message/mod.rs`](../layers/service/src/message/mod.rs),
[`layers/service/src/message/wire.rs`](../layers/service/src/message/wire.rs),
[`layers/service/src/payload/mod.rs`](../layers/service/src/payload/mod.rs),
[`layers/adapters/llamacpp/served/src/chat/mod.rs`](../layers/adapters/llamacpp/served/src/chat/mod.rs).

현재 직접 테스트된 option은 `temperature`, `top_p`, `seed`이다.
테스트는 전달 여부만 확인하며 실제 backend가 의미를 적용했는지는 확인하지
않는다([`chat/tests.rs`](../layers/adapters/llamacpp/served/src/chat/tests.rs)).

### adapter 경계의 주의점: P4 결함이 아님

기본 HTTP body를 만든 뒤 options의 모든 key를 삽입하므로 caller가 다음을
덮어쓸 수 있다.

```text
model
messages
max_tokens
stream
```

따라서 `stream`을 항상 true로 유지한다는 adapter 테스트 의도와 구현이
일치하지 않는다. 이는 P4가 options를 해석해야 한다는 뜻이 아니라, OUTER가
선택한 adapter가 자신이 소유한 concrete request를 어떻게 구성할지 정해야
한다는 뜻이다.
`options = {"stream":false}`이면 현재 구현은 false를 보낼 수 있다. prompt와
route의 의미도 `messages` override로 깨질 수 있다.

### 누락

OUTER-어댑터 계약은 다음을 정의해야 한다. 추상 P4는 이 내용을 해석하지
않고 문자열로 보존한다.

- option 이름과 타입
- 범위/단위/기본값
- P4 field와 extension field의 precedence
- 알 수 없는 key와 지원하지 않는 key의 처리
- backend가 실제로 적용한 option의 보고
- tokenizer/chat-template 선택
- prompt가 단일 user message인지, role/message 배열인지
- input token IDs, multimodal content, tool call 입력

현재 object가 아니거나 JSON이 아닌 options는 오류가 아니라 조용히 무시된다.
이는 호출자가 “전달되지 않음”과 “backend가 거부함”을 구분할 수 없게 한다.

### 수정안: P4가 아니라 OUTER-어댑터 경계

P4 wire에 의미를 추가하지 말고, OUTER가 선택한 adapter별 generation
문자열 계약을 고정한다. P4 측 불변식은 다음이다.

```text
opaque generation content survives every hop unchanged
size and framing limits are enforced
adapter parse/apply failure is returned with request correlation
P4 never silently drops or rewrites generation fields
```

OUTER-어댑터 계약에서 필요하다면 다음 envelope을 사용할 수 있다.

```text
GenerationRequest {
  prompt/messages
  max_output_tokens
  sampling: typed common subset
  decoding: typed common subset
  stop: typed sequences or token ids
  grammar: typed grammar/json-schema reference
  backend_extensions: namespaced opaque values
  capabilities_required
  unsupported_policy: reject | warn_and_ignore | backend_default
}
```

우선순위는 다음으로 고정한다.

1. P4-owned transport/stream fields
2. typed common generation fields
3. adapter-namespaced extensions
4. backend defaults

adapter는 accepted, rejected, ignored를 request correlation과 함께 보고해야
한다. 임의 JSON key를 받는 호환층은 legacy extension으로만 남긴다.

## 4. Sampling과 decoding

### 현재 확정

현재 served adapter가 HTTP request에 직접 넣을 수 있는 공통적인 값은
`max_tokens`와 caller가 options로 제공하는 sampling key이다. seed는 request
body까지 이동하지만 determinism이나 seed scope는 protocol에 없다.

SSE 응답 parser는 다음 값만 의미 있게 보존한다.

- text delta
- `finish_reason`
- error message

[`served/src/chat/mod.rs`](../layers/adapters/llamacpp/served/src/chat/mod.rs)의
parser는 `content`, `reasoning_content`, `message.content`, `text` 중 하나를
일반 문자열로 합친다. `reasoning_content`와 answer content를 구분하지 않는다.

### 결함

1. backend chunk의 문자열 길이와 tokenizer token 수가 같다는 보장이 없다.
2. `Outcome.position`은 adapter session에서 증가하지만, stop event는
   `Sequence.position`을 사용한다.
3. `Bodies::sequence()`는 매 hop에서 position을 0으로 만든다.
4. backend가 stop을 반환한 경우 `Reply::Done.generated`가 실제 생성량과
   다르게 0이 될 수 있다.
5. stop sequence, stop token ID, length termination, backend cancellation의
   구별이 없다.
6. `logprobs`, top-logprobs, token IDs, per-choice metadata, usage가 response에서
   손실된다.

### 수정안

`Outcome`을 문자열 token event가 아니라 다음 의미로 확장해야 한다.

```text
GeneratedDelta {
  request_id, sequence_id, hop_id
  text_delta
  token_ids or backend_token_count
  position
  channel: answer | reasoning | tool_call
  finish: none | stop | length | content_filter | error
  usage_delta
}
```

`generated`는 frame body의 position을 재사용하지 말고 adapter session의 누적
token count에서 계산한다. chunk count를 token count로 부르지 않는다.

### 필수 테스트

- options가 `stream`, `model`, `messages`, `max_tokens`를 덮어쓸 수 없음을 확인
- `temperature`, `top_p`, `top_k`, `min_p`, repetition penalties, seed의
  type/range/unsupported 정책 확인
- stop sequence와 backend finish reason의 매핑 확인
- final chunk에 text와 finish reason이 동시에 있을 때 generated count 확인
- reasoning과 answer가 모두 있는 stream에서 channel 보존 확인
- multi-choice/tool-call/logprobs/usage가 지원되지 않으면 명시적 refusal 확인

## 5. Grammar와 structured output

### 현재 판정: 누락

`options` 안에 grammar, JSON schema, response format, guided decoding 관련 key를
넣을 수는 있다. 그러나 현재 P4는 다음을 보장하지 않는다.

- backend가 그 key를 지원하는지
- grammar가 실제 sampling 단계에 적용되었는지
- schema version과 grammar identity
- 응답이 answer인지 tool call인지
- structured response metadata와 validation result

응답 parser가 text만 추출하므로 structured output을 전달해도 P4 response는
일반 문자열로 축소된다.

### 수정안

grammar는 sampling option의 임의 key가 아니라 capability-backed request로
승격한다.

```text
GrammarRequest {
  kind: grammar | json_schema | guided_json
  schema_or_grammar
  strict
  version/fingerprint
}
```

지원하지 않는 adapter는 첫 admission 단계에서 `unsupported_capability`를
반환한다. 적용 결과는 response metadata와 status에 남긴다.

## 6. MTP와 speculative decoding

### 현재 판정: protocol 미구현

현재 `Start`가 전달하는 load/launch 정보는 weights, context, slots, batch,
ubatch, patience와 device/RPC placement뿐이다. llama.cpp launch argument에도
draft model, draft device, draft layer/count, MTP mode가 없다.

request `options`로 임의 key를 넘기는 것은 다음을 해결하지 못한다.

- draft model을 어디에 load할지
- target/draft model이 어느 deployment에 속하는지
- draft KV와 target KV의 ownership
- accepted/rejected token 수
- speculative stream의 token granularity
- persist/restore/fork 시 speculative state
- vLLM/SGLang의 다른 speculative mechanism

근거: [`served/src/plan/mod.rs`](../layers/adapters/llamacpp/served/src/plan/mod.rs),
[`served/src/launch/mod.rs`](../layers/adapters/llamacpp/served/src/launch/mod.rs),
[`adapter/src/lib.rs`](../layers/adapters/adapter/src/lib.rs).

### 수정안

MTP/speculative는 `GenerationRequest`의 일반 opaque option이 아니라 adapter
capability와 load binding으로 정의한다.

```text
SpeculativeConfig {
  mode
  draft_artifact/model
  draft_deployment_id
  max_draft_tokens
  device/placement
  required
}
```

adapter load event는 target/draft materialization을 각각 보고하고, generation
event는 proposed/accepted/rejected count를 request/hop ID와 함께 보고한다.

### 필수 테스트

- capability 없음 + `required=true`가 load/admission에서 거부되는지 확인
- draft model과 target model의 deployment/generation mismatch 거부
- speculative output에서 P4 token index가 누락·중복되지 않는지 확인
- MTP 상태 persist/restore/fork가 지원되지 않으면 명시적 거부
- 실제 llama.cpp staged adapter가 생기기 전에는 MTP pipeline 성공을 주장하지 않음

## 7. KV cache mapping과 persistence

### 현재 확정

adapter contract에는 `Persist`, `Restore`, `Fork`, `Discard`가 있다. mock은
persisted map을 이용해 progress를 보존하고 fork를 독립 copy로 처리한다.
multi-stage service 테스트는 모든 stage에 cache instruction을 보낸다.

근거: [`adapter/src/work/cache/mod.rs`](../layers/adapters/adapter/src/work/cache/mod.rs),
[`mock/src/cache/mod.rs`](../layers/adapters/mock/src/cache/mod.rs),
[`service/tests/cache_in_a_deployment.rs`](../layers/service/tests/cache_in_a_deployment.rs).

### 현재 제한

- `sequence_id`는 명시적 inference field가 아니라 `Envelope.route`에서 파생된다.
- `Fork`의 `into`가 다음 Execute route와 같아야 한다는 규칙이 없다.
- cache manifest에 model fingerprint, binding generation, tokenizer, stage,
  adapter, format version, checksum이 없다.
- 여러 stage의 persist/restore 원자성 barrier가 없다.
- crash 중간 상태, quota, TTL, eviction, encryption, checksum 정책이 없다.
- node 이동이나 generation 변경 후 restore compatibility가 없다.
- served adapter는 HTTP surface에서 sequence state를 저장할 수 없어 명시적으로
  실패 event를 올린다([`served/src/lib.rs`](../layers/adapters/llamacpp/served/src/lib.rs)).

### 수정안

`request_id`와 별도의 immutable `sequence_id`를 Execute, Cache, Token, Done에
공통으로 넣는다. persisted KV는 다음 manifest에 묶는다.

```text
sequence_id, deployment_id, generation, model_fingerprint
tokenizer_fingerprint, adapter_kind, stage_id, cache_format_version
created_at, last_used_at, bytes, checksum
```

restore는 모든 stage가 manifest 검증을 통과한 뒤에만 sequence를 ready로
공개한다. 일부 stage만 복원된 상태는 inference admission에 노출하지 않는다.

### 필수 테스트

- request ID와 sequence ID를 다르게 해도 token/Done correlation이 유지되는지
- fork 후 source와 branch를 독립적으로 계속 생성하는지
- route 재사용과 process restart 후 stale cache가 거부되는지
- 한 stage restore 실패 시 전체 deployment가 partial state를 노출하지 않는지
- generation/model/tokenizer mismatch를 거부하는지
- served adapter가 cache unsupported capability를 사전에 보고하는지

## 8. Actual served/staged adapter boundary

### served adapter: 확정된 실행 형태

`Served::distribution()`은 `Distribution::Internal`만 반환한다. prefill hop에서
sequence마다 `/v1/chat/completions` stream을 열고, 이후 decode hop에서 같은
session의 다음 chunk를 읽는다.

현재 실행 구조는 P4 `Hop.sequences`를 HTTP batch request로 변환하는 것이 아니다.

- prefill window는 sequence별 stream open을 병렬로 시작한다.
- 각 session의 token read는 adapter 내부에서 순차 처리한다.
- `sessions` lock을 잡은 상태로 blocking token wait를 한다.
- 실제 continuous batching은 backend server가 내부적으로 수행할 때만 발생한다.

근거: [`served/src/lib.rs`](../layers/adapters/llamacpp/served/src/lib.rs),
[`served/src/session/mod.rs`](../layers/adapters/llamacpp/served/src/session/mod.rs).

따라서 현재 P4 테스트가 증명하는 것은 “node scheduling과 mock stage overlap”이지
실제 GPU가 항상 다음 데이터를 공급받는다는 사실이 아니다.

### staged adapter: 현재 미구현

adapter contract의 `Distribution::Staged`, `Hop.phase`, `Hop.sequences`는 staged
backend를 수용할 수 있는 추상 seam이다. mock은 이를 검증한다. 그러나 실제
llama.cpp staged Rust adapter가 없으므로 다음은 아직 미검증이다.

- layer-range load
- hidden state 입력/출력 wire
- stage-local KV window
- stage-to-stage batch/credit
- 실제 multi-GPU pipeline feed
- 마지막 stage만 sampling/decoding 수행
- staged KV persist/restore

### 수정안

adapter capability를 다음처럼 분리한다.

```text
distribution: internal | staged
prefill: supported/unsupported
decode: supported/unsupported
batching: backend_owned | p4_owned
max_in_flight, max_batch_width
kv: resident | persistable | restorable | forkable
sampling_owner: final_stage | backend_internal
speculative: unsupported | supported(config)
```

P4는 `batching=backend_owned`인 served adapter와 `batching=p4_owned`인 staged
adapter의 의미를 같은 것으로 취급하지 않는다. GPU utilization은 queue depth가
아니라 adapter/device telemetry로 검증한다.

### 필수 pipeline 테스트

- 연속 request가 stage별로 겹치고 각 stage가 starvation 없이 busy한지
- prefill/decode lane 혼합이 backend contract에 맞는지
- stage credit가 고갈될 때 bounded backpressure 또는 명시적 refusal인지
- one hop/one node 실행과 duplicate hop 방지
- 실제 staged adapter에서 input/output hidden state와 KV identity가 대응하는지
- served adapter에서는 “P4 pipeline parallel”을 주장하지 않는지

## 9. Monitoring과 correlated events

현재 Status는 agent lane depth, peer count, node depth, running flag, waiting
routes, adapter free-form report를 문자열로 보낸다([`service/src/status/mod.rs`](../layers/service/src/status/mod.rs)).

이는 운영자가 대략적인 위치를 보는 데는 확정이지만 다음을 protocol 수준에서
보장하지 않는다.

- request별 state transition
- queue별 capacity/oldest wait
- request/sequence/hop correlation
- event sequence와 duplicate/stale rejection
- adapter accepted/ignored option
- KV residency와 cache generation
- MTP acceptance
- backend batch width와 device utilization

`Event::HopComplete`도 deployment와 outcomes만 가지며 hop correlation ID가
없다([`adapter/src/event/report/mod.rs`](../layers/adapters/adapter/src/event/report/mod.rs)).
현재 node가 한 번에 하나의 hop만 실행해서 mock에서는 통과하지만, 늦은 event나
재시작 후 event를 안전하게 분리하는 계약은 아니다.

### 수정안

최소 typed snapshot/event를 추가한다.

```text
Snapshot {
  snapshot_seq, generated_at, agent, nodes[], requests[]
}

RequestState {
  request_id, sequence_id, origin_agent, return_channel
  node_id, stage_id, hop_id, phase
  state, enqueue_at, start_at, last_progress_at, deadline
}

AdapterDecision {
  accepted_options, ignored_options, rejected_options
  capability_state, batch_width, in_flight, kv_state
}
```

모든 event는 request/hop/generation을 포함하고, receiver는 이미 종료된 hop의
event를 idempotently 무시하거나 명시적 stale error로 기록해야 한다.

## 10. 확정해야 할 최종 정책

1. `request_id`, `sequence_id`, `route`, `hop_id`를 별도 필드로 둘 것인가.
2. `origin_agent`와 `return_channel`을 wire에 강제할 것인가.
3. P4-owned field와 backend extension의 precedence를 어떻게 고정할 것인가.
4. unsupported option을 reject, warning, backend default 중 무엇으로 처리할 것인가.
5. grammar와 structured output의 response metadata를 어떤 형태로 보존할 것인가.
6. MTP/speculative를 load capability로 협상할 것인가.
7. sampling owner가 final staged node인지 backend 내부인지 어떻게 표시할 것인가.
8. KV restore를 multi-stage barrier로 강제할 것인가.
9. served continuous batching을 P4 보장으로 볼 것인가, adapter capability로 볼 것인가.
10. queue/event/status를 typed schema로 승격할 것인가.

이 정책과 테스트가 추가되기 전까지 P4는 “opaque generation options를 운반하는
framed routing layer와 mock pipeline”으로는 확정할 수 있지만, 세 backend에
공통인 완전한 generation/MTP/KV/pipeline protocol로 판정할 수 없다.
