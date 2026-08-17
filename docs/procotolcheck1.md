# P4 protocol check 1: routing and return-path audit

이 문서는 [protocol.md](protocol.md)와 현재 P4 코드만을 기준으로 수행한 읽기 전용 검수 기록이다. 범위는 `routing`, `return path`, `OUTER identity`, `stream continuation`이며, 정상 경로에서 동작하는 부분과 다수 OUTER·재연결·실패 상황에서 보장되지 않는 부분을 분리한다.

## 1. 최종 판정

현재 구현은 다음 범위까지 확정된다.

| 영역 | 판정 | 근거 |
| --- | --- | --- |
| 고정 TCP 주소의 frame forward | 확정 | `Envelope::is_mine`, `target`, `recipient`로 매 hop 결정한다. [`envelope/mod.rs:23-52`](../layers/protocol/src/envelope/mod.rs#L23-L52) |
| 순차 chain 진행과 decode lap | 확정 | `advance()`와 `restart()`가 chain position을 이동시킨다. [`chain/mod.rs:92-109`](../layers/protocol/src/envelope/chain/mod.rs#L92-L109) |
| 여러 node의 mock pipeline overlap | 확정 | 서로 다른 stage의 hop이 겹치는 테스트가 있다. [`pipelining.rs:125-159`](../layers/agent/tests/pipelining.rs#L125-L159) |
| 단일 OUTER의 직접 reply | 조건부 확정 | `reply_to` 주소가 접근 가능하고 route가 충돌하지 않을 때만 성립한다. [`envelope/mod.rs:84-109`](../layers/protocol/src/envelope/mod.rs#L84-L109) |
| 최초 OUTER 연결 agent의 식별 | 미정 및 결함 | `origin_agent` 또는 ingress connection ID가 envelope에 없다. |
| 다수 OUTER의 논리 채널 분리 | 미정 | 주소와 route만으로는 동일 listener 안의 여러 요청자를 분리할 수 없다. |
| 재연결 후 기존 reply 복구 | 결함 | inbound 연결을 reply channel로 보존하지 않고, outbound delivery ACK도 없다. |
| streaming continuation | 결함 | `FnOnce` continuation이 첫 Response 뒤 제거되지만 inference는 `Token* + Done`을 보낸다. |

따라서 현재 P4는 `working framed routing and mock-pipeline implementation`으로는 설명할 수 있지만, 다수 OUTER와 durable reply recovery를 포함한 완전한 inference protocol로 판정할 수 없다.

## 2. 현재 routing 계약

### 2.1 확정된 frame 경로

Envelope의 routing 필드는 `target`, `recipient`, `lane`, `route`, `deadline_unix_ms`, `reply_to`, `chain`이다. [`envelope/mod.rs:23-43`](../layers/protocol/src/envelope/mod.rs#L23-L43)

매 agent는 주소가 자기 것인지 먼저 판단하고, 아니면 body를 해석하지 않은 채 target으로 forward한다. 자기 주소이면 `Recipient::Agent` 또는 `Recipient::Node`로 소비 대상을 결정한다. [`agent/mod.rs:226-267`](../layers/agent/src/agent/mod.rs#L226-L267)

이 구조는 다음 정상 경로를 보장한다.

```text
origin agent -> chain[0] -> chain[1] -> ... -> chain[last]
```

chain의 link는 `address`, `node`, `binding`, `generation`을 가진다. [`chain/mod.rs:21-32`](../layers/protocol/src/envelope/chain/mod.rs#L21-L32)

### 2.2 모순: chain 첫 링크와 최초 ingress agent가 다를 수 있음

`relay_home()`은 chain의 첫 링크를 “caller가 처음 보낸 agent”로 간주한다. [`envelope/mod.rs:112-132`](../layers/protocol/src/envelope/mod.rs#L112-L132)

하지만 topology 테스트는 chain 밖의 `door`가 요청을 받고 chain은 worker부터 시작하는 구조를 사용한다.

```text
OUTER -> door -> worker
chain = [worker]
```

근거는 chain이 worker만 명명하고, 요청을 `door.enqueue()`로 넣는 테스트다. [`topology.rs:39-56`](../layers/agent/tests/topology.rs#L39-L56)

따라서 현재 fallback은 실제 origin agent를 복원하지 못할 수 있다.

### 2.3 수정안: forward chain과 return anchor를 분리

`chain`은 모델 stage의 forward path로 유지하고, 다음 필드를 별도로 둔다.

```text
request_id       한 inference 실행의 immutable ID
origin_agent     OUTER 요청을 처음 수락한 agent ID/address
return_channel   OUTER logical subscription/connection ID
sequence_id      backend/KV conversation ID
hop_id           한 node의 한 execution pass ID
```

최소 불변식은 다음과 같다.

1. `origin_agent`는 chain 첫 stage와 같다고 추론하지 않는다.
2. `return_channel`은 TCP 주소만으로 대체하지 않는다.
3. forward frame은 `request_id`, `sequence_id`, `hop_id`를 보존한다.
4. reply는 `origin_agent + return_channel`을 기준으로 귀환한다.
5. 재연결 시 새 channel에 기존 `request_id`를 rebind할지, buffer할지, cancel할지를 명시한다.

## 3. Return path 및 실패 처리

### 3.1 직접 reply는 주소 기반이다

`to_reply()`는 `reply_to`를 target으로 복사하고 response lane으로 바꾸며, reply의 `reply_to`는 제거한다. chain은 fallback을 위해 남긴다. [`envelope/mod.rs:84-109`](../layers/protocol/src/envelope/mod.rs#L84-L109)

`Address` 자체는 scheme, host, port만 표현한다. [`address/mod.rs:17-35`](../layers/protocol/src/envelope/address/mod.rs#L17-L35)

즉 다음 조건에서만 직접 reply가 확정된다.

```text
reply_to 주소가 현재도 유효함
reply_to listener가 해당 route를 올바른 OUTER에게 demux함
route가 다른 요청과 충돌하지 않음
```

### 3.2 inbound socket은 return channel이 아님

inbox는 연결 수만 제한하고 socket reader task를 만들며, frame을 queue에 넣는다. 연결의 writer, connection ID, OUTER subscription은 보존하지 않는다. [`inbox/mod.rs:21-41`](../layers/agent/src/transport/inbox/mod.rs#L21-L41) [`inbox/mod.rs:45-77`](../layers/agent/src/transport/inbox/mod.rs#L45-L77)

outbound는 별도 connection을 열고, peer가 자기 connection으로 응답한다고 가정한다. [`outbound/mod.rs:328-350`](../layers/agent/src/transport/outbound/mod.rs#L328-L350)

따라서 OUTER가 origin agent에 연결되어 있다는 사실만으로 reply가 그 연결을 통해 돌아간다는 보장이 없다.

### 3.3 전송 성공과 전달 성공이 분리됨

`Peers::send()`는 peer별 bounded queue에 frame이 들어가면 성공한다. [`outbound/mod.rs:78-104`](../layers/agent/src/transport/outbound/mod.rs#L78-L104)

write 실패 시에는 최대 한 번 `relay_home`을 시도하고, relayed frame이 다시 실패하면 로그 후 종료한다. [`outbound/mod.rs:216-239`](../layers/agent/src/transport/outbound/mod.rs#L216-L240) [`outbound/mod.rs:261-285`](../layers/agent/src/transport/outbound/mod.rs#L261-L285)

이미 write된 frame도 중복 hop을 피하기 위해 replay하지 않는다. [`outbound/mod.rs:169-175`](../layers/agent/src/transport/outbound/mod.rs#L169-L175)

현재 정의되지 않은 정책:

| 상황 | 현재 동작 | 필요한 결정 |
| --- | --- | --- |
| target connect 실패 | origin request에 terminal failure가 보장되지 않음 | `DELIVERY_FAILED`를 어느 agent가 생성하는가 |
| reply_to OUTER 실패 | chain 첫 링크로 한 번 relay | origin agent/channel 재시도 여부 |
| relay도 실패 | log/drop | buffer, retry, cancel 중 무엇인가 |
| OUTER 주소 변경 | 기존 frame은 옛 주소 유지 | request rebind 가능 여부 |
| process restart | in-memory queue와 continuation 소실 | durable recovery 여부 |

## 4. OUTER identity 및 다수 OUTER

### 4.1 현재 구현이 전제로 삼는 것

현재 `reply_to`는 listener address이고 `route`는 correlation key다. service 테스트의 OUTER는 하나의 agent와 하나의 `Outer` accumulator로 모델링된다. [`service/tests/common/mod.rs:25-50`](../layers/service/tests/common/mod.rs#L25-L50)

요청 builder도 `reply_to`에 OUTER address만 넣는다. 별도의 origin/session/channel 필드는 없다. [`service/tests/common/mod.rs:117-150`](../layers/service/tests/common/mod.rs#L117-L150)

### 4.2 누락된 identity 계약

다음 경우의 demux 규칙이 없다.

1. 하나의 OUTER listener에 여러 논리 client가 존재함.
2. 여러 OUTER가 동일 route 문자열을 사용함.
3. 한 OUTER가 재연결했지만 기존 주소를 유지하지 못함.
4. 동일 주소의 이전 connection과 새 connection이 동시에 살아 있음.
5. `route`가 이전 inference의 KV sequence로 재사용됨.

`route`는 continuation key일 뿐 아니라 service에서 sequence ID로도 사용된다. [`payload/mod.rs:16-37`](../layers/service/src/payload/mod.rs#L16-L37)

또한 continuation registry는 route 하나당 handler 하나만 저장하며 duplicate register는 기존 handler를 조용히 덮어쓴다. [`continuation/mod.rs:17-37`](../layers/agent/src/continuation/mod.rs#L17-L37)

### 4.3 수정안

다음 식별자를 의미상 분리한다.

```text
request_id    매 실행 및 모든 reply의 correlation
stream_id     하나의 streaming response 집합
sequence_id   KV/backend 상태의 지속 identity
outer_id      OUTER 프로세스 또는 logical client identity
channel_id    OUTER와 origin agent 사이의 subscription/connection identity
```

`route`는 기존 호환성을 위해 transport shard key로만 남기거나, 명시적으로 `request_id`의 별칭임을 선언해야 한다. 현재처럼 request/sequence/worker/cancel을 동시에 의미해서는 안 된다.

## 5. Stream continuation

### 5.1 현재 continuation은 one-shot이다

handler 타입이 `FnOnce`이고 registry는 `HashMap<String, Handler<T>>`이다. [`continuation/mod.rs:14-19`](../layers/agent/src/continuation/mod.rs#L14-L19)

`resolve()`는 handler를 먼저 제거한 뒤 한 번 호출한다. [`continuation/mod.rs:39-53`](../layers/agent/src/continuation/mod.rs#L39-L53)

agent도 Response frame 하나를 resolve한 뒤 즉시 반환한다. [`agent/mod.rs:255-266`](../layers/agent/src/agent/mod.rs#L255-L266)

### 5.2 inference reply 계약과 모순

service reply에는 여러 `Token`과 최종 `Done`이 있다. [`message/mod.rs:94-117`](../layers/service/src/message/mod.rs#L94-L117)

따라서 continuation이 등록된 inference는 다음처럼 처리될 수 있다.

```text
Token(0) -> continuation 제거 및 호출
Token(1..N) -> 같은 continuation 없음
Done -> 같은 continuation 없음
```

이는 `FnOnce` continuation과 streaming inference의 의미가 충돌하는 확정 결함이다.

### 5.3 수정안

continuation을 terminal one-shot과 stream subscription으로 분리한다.

```text
TerminalContinuation:
  Accepted/Bound/Failed/Done 중 terminal 하나에서 close

StreamContinuation:
  Token/Progress를 0..N회 수신
  Done/Failed/Cancelled에서 close
```

각 stream에는 다음 정책이 필요하다.

- `stream_id`별 sequence ordering
- duplicate token의 idempotency 기준
- gap 감지 및 replay 가능 여부
- disconnect 시 buffer/rebind/cancel
- deadline 및 idle TTL
- terminal 이후 late frame 처리
- handler backpressure와 bounded response queue

## 6. Correlation 및 event consistency

adapter의 `HopComplete`는 deployment와 outcomes만 전달하고 hop ID를 갖지 않는다. [`event/report/mod.rs:33-38`](../layers/adapters/adapter/src/event/report/mod.rs#L33-L38)

node는 현재 `in_flight: HashMap<String, Frame>`을 두고 sequence 문자열로 carrier를 찾는다. [`runner/mod.rs:35-48`](../layers/agent/src/node/runner/mod.rs#L35-L48)

`HopComplete` 처리 시 현재 in-flight 전체를 꺼낸다. [`events.rs:51-75`](../layers/agent/src/node/runner/events.rs#L51-L75)

따라서 늦은 duplicate event가 새 hop 이후 도착하는 경우를 판별할 protocol field가 없다. 최소한 `request_id`, `sequence_id`, `hop_id`, `deployment_generation`을 event와 outcome에 넣고, 이전 hop/generation event는 idempotently reject해야 한다.

## 7. 검증된 테스트와 누락된 테스트

### 7.1 현재 코드에서 확인되는 테스트

| 테스트 | 확인 내용 | 한계 |
| --- | --- | --- |
| `two_process.rs:92-124` | 두 agent chain의 token 순서와 Done | OUTER ingress는 직접 `enqueue()`로 주입 |
| `pipelining.rs:125-159` | 여러 stage의 mock hop overlap | 실제 adapter GPU utilization/credit 아님 |
| `topology.rs:39-66` | chain 밖 relay를 통한 forward | 이 구조가 `relay_home`의 origin 가정을 반증함 |
| `topology.rs:205-253` | agent 간 partition heal 후 새 요청 | OUTER reply가 끊긴 동안 생성된 token의 복구는 검증하지 않음 |
| `outbound/tests.rs:232-303` | target 및 relay 실패 시 loop 방지 | 최종 OUTER가 reply를 받았는지는 검증하지 않음 |
| `continuation/tests.rs:21-38` | one-shot handler와 duplicate terminal 방지 | Token* + Done stream은 검증하지 않음 |

service 테스트의 초기 요청은 대부분 `Peers::send()`가 아니라 origin agent의 `enqueue()`를 직접 호출한다. [`service/tests/common/mod.rs:117-150`](../layers/service/tests/common/mod.rs#L117-L150)

### 7.2 추가해야 할 검증 테스트

#### A. ingress/origin identity

1. `OUTER -> door -> worker`에서 chain에는 worker만 포함한다.
2. worker가 reply_to에 연결하지 못하게 한다.
3. reply가 door의 실제 logical channel로 돌아오는지 검증한다.
4. chain 첫 링크로 잘못 relay되지 않는지 검증한다.

#### B. 다수 OUTER

1. 하나의 origin agent에 OUTER A/B를 동시에 연결한다.
2. A/B가 동일 route를 각각 사용한다.
3. token과 Done이 올바른 OUTER/channel로만 도착하는지 검증한다.
4. 같은 listener의 두 logical client도 분리되는지 검증한다.

#### C. reconnect/failure

1. inference 중 OUTER connection을 끊는다.
2. 마지막 node가 Token과 Done을 생성하게 한다.
3. OUTER가 새 connection/channel로 재접속한다.
4. 정책에 따라 buffer/rebind/cancel 중 정해진 결과가 나오는지 검증한다.
5. target 및 relay 실패 때 origin OUTER가 terminal `Failed` 또는 명시적 timeout을 받는지 검증한다.

#### D. streaming continuation

1. 하나의 `stream_id`에 Token 0..N과 Done을 보낸다.
2. 모든 frame이 같은 handler/stream subscriber에 도착하는지 검증한다.
3. duplicate Token, gap, late Done, terminal 뒤 Token을 검증한다.
4. disconnect와 TTL 후 registry가 남지 않는지 검증한다.

#### E. hop correlation

1. hop 1 완료 뒤 hop 2를 시작한다.
2. hop 1의 늦은 duplicate `HopComplete`를 주입한다.
3. hop 2 carrier가 소비되거나 reply가 중복되지 않는지 검증한다.

## 8. 수정 우선순위

1. `origin_agent`와 `return_channel/channel_id`를 명시한다.
2. `request_id`, `stream_id`, `sequence_id`, `hop_id`를 분리한다.
3. streaming continuation과 terminal continuation을 분리한다.
4. delivery ACK/failure 및 reconnect 정책을 wire contract로 고정한다.
5. chain validation을 추가한다: 현재 target과 chain position의 일치, origin anchor 정책, generation/route scope.
6. 다수 OUTER·실패 reply·재연결 stream 테스트를 추가한다.
7. 상태 snapshot에 request/channel/hop/delivery state를 구조화해 노출한다.

이 문서는 검수 결과와 수정 설계만 기록하며, 구현 수정은 포함하지 않는다.
