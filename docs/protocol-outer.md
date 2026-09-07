# OUTER 세션과 KV 수명주기

> 문서 지위 (2026-09-06): **분야 계약·구현과 구별**. 소유 분야의 계약/목표를 읽되 구현 완료로 간주하지 않는다. 현재 개발 순서와 충돌하면 로드맵의 명시적 이관을 따른다.
> 현재 목표·상태·순서는 [실행 로드맵](distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](document-map.md)를 따른다.

## 상태와 범위

이 문서는 OUTER 연결 단절, heartbeat, inference 중지, KV Persist/Restore/Discard,
보존기간 GC의 계층 경계를 정의한다. P4는 정책 엔진이나 KV 스케줄러가 아니라
메시지 브로커다. P4가 해석하지 않는 정책은 agent 상주 정책 컴포넌트가 결정하고,
실제 KV 슬롯·GPU·메모리 스케줄링은 구상 adapter가 결정한다.

현재 node-bound worker는 `target:node`를 순서 키로 사용하고 프레임 `route`는
handle로만 사용한다. 이 변경은 같은 node에 도착하는 서로 다른 route의 경쟁을
줄이지만, main queue의 Control/Response/Decode/Prefill lane 선택보다 뒤에서
적용된다. 따라서 `sequence_id` 단위의 발행 순서 보장은 아직 목표 계약이며,
현재 구현의 완료를 의미하지 않는다.

## 계층별 책임

| 계층 | 책임 | 하지 않는 일 |
| --- | --- | --- |
| Agent 상주 세션 정책 | heartbeat, 단절 판정, 요청 소유권, Persist/Restore/Discard 발행, TTL GC | KV 바이트 배치·GPU 슬롯 선택 |
| P4 agent/protocol | 메시지 전달, 식별자 보존, 세션 단위 전달 순서, 결과·거절·실패 운반 | sampling, decoding, eviction 정책, restore 시점 결정 |
| 구상 adapter | inference 경계, KV 슬롯 확보·축출·복원, 실제 동시성 및 장치 스케줄링 | P4 메시지 의미의 재정의 |

관련 구현 진입점은 [agent (`Agent`)](../layers/agent/src/agent/mod.rs), [node runner (`Node`)](../layers/agent/src/node/runner/mod.rs),
[service message vocabulary (`ToAgent`/`ToNode`)](../layers/service/src/message/mod.rs),
[cache coordinator (`CacheTransaction`)](../layers/service/src/cache.rs)다.

## 식별자

식별자는 서로 대체하지 않는다.

| 식별자 | 의미 | 수명 |
| --- | --- | --- |
| `return_channel` | OUTER의 논리 채널과 소유권 축 | 재접속에도 논리 채널로 유지 |
| `ingress_generation` | 해당 `return_channel`의 연결 세대 | 소켓 재접속마다 증가 |
| `sequence_id` | KV가 보존하는 대화 세션 | Persist 이후에도 유지 |
| `request_id` 또는 `route` | 개별 프레임·실행 handle | 메시지 하나 또는 실행 하나 |
| `operation_id` | Persist/Restore/Discard 한 번의 작업 | 해당 작업 동안 |

KV의 durable key는 `request_id`가 아니라 `sequence_id`다. 동일한
`sequence_id`의 여러 실행 요청은 하나의 대화 상태를 이어가며, 각 실행은 별도의
`request_id`와 `operation_id`를 가질 수 있다. Restore 이후 후속 inference가 어떤
세션을 이어갈지는 반드시 명시적인 `sequence_id`로 결정한다.

## P4 순서 계약

P4가 보장하는 순서는 정책이 아니라 전달 순서다.

> 동일한 `sequence_id`를 가진 모든 메시지는 각 대상 node에 발행 순서대로
> 전달된다. 서로 다른 `sequence_id` 사이에는 순서를 보장하지 않는다.

`sequence_id`는 ordering key이고, `route` 또는 별도 request handle은 프레임을
식별하는 키여야 한다. 둘을 같은 필드로 사용하면 다음 문제가 생긴다.

- Restore와 후속 Hop을 같은 세션 순서로 묶을 수 없다.
- 서로 다른 worker로 분산되어 순서가 사라진다.
- node queue의 `claim`/`remove`가 여러 프레임을 같은 항목으로 오인할 수 있다.

현재 node worker 선택은 [agent dispatch](../layers/agent/src/agent/mod.rs)에서
`target:node`를 사용하고, 일반 peer traffic은 `route`를 사용한다. 이 선택은
node별 ingress를 한 worker에 직렬화하지만, [main queue](../layers/agent/src/queue/main/mod.rs)의
lane 우선순위를 없애지 않는다. 따라서 `ordering_key`와 `frame_handle`을 완전히
분리하고 발행 순서를 보장하는 변경은 별도 수용조건이다.

전달 순서가 실행 완료를 뜻하지는 않는다. 같은 세션의 후속 inference는 Restore
완료 전 실행 가능 상태가 될 수 없으며, 그 경계를 유지하는 위치는 adapter
capability에 따른다.

## OUTER heartbeat와 단절

heartbeat는 TCP 생존 확인이 아니라 application-level 메시지여야 한다.
아래는 목표 wire/policy이며, 현재 P4 코드에 구현됐다는 뜻이 아니다.

```text
Agent → Outer: Ping(nonce, issued_at)
Outer → Agent: Pong(nonce, ingress_generation)
```

구현 시 agent는 nonce와 `ingress_generation`을 검증하고, 단일 실패가 아니라 설정된
miss threshold를 넘었을 때만 `Disconnected`로 전이해야 한다. 재접속은 새 연결 세대로 등록하며,
이전 연결의 응답·ACK가 새 세션 소유권을 침범하지 않아야 한다.

## 단절과 KV 흐름

연결 단절 시 정책 컴포넌트는 해당 `return_channel`과 `ingress_generation`이 소유한
모든 실행을 찾아야 한다. 현재 구현에는 이 실행 집합을 열거하거나 집합 단위로
취소하는 primitive가 없으므로, 이는 수용 조건이다. 실행 중인 hop은 현재 P4의
취소 의미에 따라 hop 경계까지 진행될 수
있으며, 다음 hop은 시작하지 않는다.

```text
Connected
  └─ heartbeat miss threshold 초과
       └─ Disconnected
            ├─ 관련 inference 취소/중지 요청
            ├─ 진행 상태를 sequence_id에 귀속
            └─ Persist(sequence_id) 발행
```

Persist가 완료되면 세션은 `Detached`다. durable KV가 있으므로 Restore가 지금
실행되지 않아도 세션 손실은 아니다.

## Restore는 메시지 요청이다

Restore는 P4의 특별한 스케줄링 명령이 아니라 큐에 들어가는 하나의 요청이다.

```text
Detached
  └─ Restore(sequence_id)
       └─ adapter가 자신의 scheduler에서 처리
            ├─ 현재 inference 경계까지 대기
            ├─ KV 슬롯 확보 또는 축출
            ├─ KV 복원
            └─ 복원 완료 후 후속 inference 허용
```

P4는 Restore를 decode window에 끼워 넣지 않으며, 용량 예약·victim 선택·GPU
전송 방식도 정의하지 않는다. adapter는 서로 다른 세션의 Restore를 겹칠 수
있지만, 같은 `sequence_id`의 Restore와 후속 inference 순서는 지켜야 한다.

기본 adapter capability는 Restore와 inference를 같은 node barrier에서 처리하는
안전한 방식이다. 별도 capability를 광고하는 adapter만 cross-session overlap을
허용할 수 있다. 모든 stage의 Restore가 완료되기 전까지 해당 세션은 실행
가능 상태로 공개하지 않는다.

### 목 어댑터 우선과 라마 지연 허용선

프로토콜·agent·service의 복원 정책은 목 어댑터로 먼저 고정한다. 실제 라마
어댑터의 KV 파일 포맷, GPU 슬롯 회수, 장치 전송 최적화는 다음 조건을 지키는
동안 지연할 수 있다.

- 목 어댑터가 `Adapter::start`의 비동기 이벤트 경계와 `Work::Cache` 수명주기를
  통과한다.
- `sequence`를 durable key로 사용하고, `operation_id`와 `generation`을 섞거나
  추측하지 않는다.
- `PreparePersist/Restore/Discard` 뒤에는 반드시 `Commit` 또는 `Abort`가 오며,
  `Reconcile`은 상태를 바꾸지 않는다.
- Restore가 완료되기 전에는 후속 Hop을 성공시킬 수 없고, 처리할 수 없는
  용량·세대·무결성 상태는 `Refused` 또는 `Failed`로 보고한다.

이 선을 넘지 않는 한 라마 어댑터가 아직 실제 KV를 저장·복원하지 않아도 P4의
순서, 단절, 재시도, GC 정책 테스트를 완료할 수 있다. 라마 어댑터를 완료로
판정하려면 이 계약을 실제 KV와 장치 슬롯에 연결한 별도 acceptance가 필요하다.

## 결과와 재시도

정책 컴포넌트가 재시도 여부를 결정할 수 있도록 cache 결과는 최소한 다음을
구분해야 한다.

| 결과 | 의미 | 정책 |
| --- | --- | --- |
| `Cached` | cache 작업이 완료됨 (`bytes` 포함) | 다음 inference 또는 GC 상태 갱신 |
| `Accepted` | 일반 명령이 접수됨 | 해당 명령의 후속 결과 대기 |
| `Done` | inference 생성이 종료됨 | 다음 inference 진행 |
| `Refused` | 지금은 처리할 수 없으나 durable 상태는 보존됨 | adapter가 제시한 재시점에 재시도 |
| `Failed` | 손상·세션 없음 등 영구 실패 | 자동 재시도 금지 |
| `Inconsistent` | receipt와 실제 상태를 확정할 수 없음 | fail-close, reconciliation |

현재 `CacheStatus.state`는 reconciliation 경로에서 `Inconsistent`를 기계적으로
전달할 수 있다. 반면 `CacheFailed.detail`은 자유 텍스트이므로 실패 경로에서
`Refused`와 영구 `Failed`를 구분할 수 없다. 실패 코드와 선택적 재시점 필드는
추가 계약으로 남는다. 이 구분은 eviction 정책을 P4에 넣기 위한 것이 아니라,
adapter의 backpressure와 정책 계층의 재시도를 안전하게 운반하기 위한 계약이다.

## 30일 보존과 GC

30일 보존은 OUTER가 아니라 agent에 상주하는 정책 컴포넌트가 실행한다. OUTER가
사라진 뒤에도 보존기간이 진행되어야 하기 때문이다.

- Persist 완료 시 `retain_until`을 durable manifest/receipt에 기록한다.
- 기본 정책은 `retain_until = persisted_at + 30일`이다.
- 성공한 Restore 또는 정책상 사용 시 보존기간을 갱신할 수 있다.
- agent scheduler는 하루 한 번 만료 목록을 조회해 `Discard(sequence_id)`를 발행한다.
- Discard는 idempotent해야 하며, 완료 receipt 확인 전 로컬 목록을 삭제하지 않는다.
- agent 재시작 후에도 GC가 가능하도록 adapter는 cache 목록을 열거할 수 있어야 한다.

권장 관리 메시지는 다음 형태다.

```text
ListCaches(deployment_id)
  → sequence_id, retain_until, bytes, receipt_state
```

P4는 목록의 보존기간을 해석하지 않고 요청과 응답만 운반한다. 실제 KV를 가진
adapter가 목록과 receipt를 제공하는 편이 coordinator journal과 실물 KV의
불일치를 줄인다.

## 현재 구현과 수용 조건

현재 구현에서 확인된 기반:

- [ToNode](../layers/service/src/message/mod.rs)는 Persist/Restore/Discard와
  Prepare/Commit/Abort를 운반한다.
- [CacheTransaction](../layers/service/src/cache.rs)은 여러 stage의
  cache receipt를 operation 단위로 조정한다.
- node runner는 Cache를 hop과 섞지 않고 lifecycle 작업으로 실행한다.
- Cancel은 `request_id`, `stream_id`, `return_channel`, `ingress_generation`을
  함께 검증한다. 중복 Cancel은 멱등적으로 접수하며, 이미 terminal인 요청에는
  새 terminal을 추가하지 않는다. 이미 시작한 hop의 강제 중단은 보장하지 않고
  hop 경계까지의 협력적 처리를 명시한다.
- 취소된 요청에는 원래 `return_channel`로 replay 가능한 terminal `Failed`를
  요청당 한 번 보내며, 체인의 추가 queued carrier는 폐기한다. 추가 폐기 수는
  별도 관측값으로 계수해야 하며 현재 status 계약에는 없다.

목 어댑터 우선 구현의 필수 실패 시나리오는 [testing.md](testing.md)의
`Cache contract` 표를 따른다. 이 표가 통과하면 라마 어댑터 지연은 프로토콜
구현의 차단 사유가 아니다.

이 문서의 완성을 주장하려면 다음을 별도로 검증해야 한다.

| 조건 | 검증 레벨 |
| --- | --- |
| `sequence_id`가 `request_id` 파생이 아닌 독립적인 durable wire 식별자가 됨 | wire/adapter contract |
| `sequence_id` ordering key와 프레임 handle 분리 | Pure: queue tests; Simulated: `layers/agent/tests/network.rs` |
| 동일 sequence의 Restore 전후 전달 순서 | Pure: queue tests; Simulated: `layers/service/tests/cache_in_a_deployment.rs` |
| Restore 완료 전 후속 inference 차단 | Simulated: `layers/service/tests/cache_in_a_deployment.rs`; Fleet: `tools/drive` |
| heartbeat miss threshold와 `return_channel`/`ingress_generation` fencing | Pure: `layers/service/src/outer_policy.rs`; transport reconnect integration remains |
| 연결별 실행 집합의 열거와 집합 단위 취소 | Simulated: `layers/service/tests/protocol_in_flight.rs`; Fleet: reconnect run |
| 요청당 replay 가능한 terminal `Failed` 1회와 추가 carrier 폐기 계수 | Simulated: `layers/service/tests/protocol_in_flight.rs`; Fleet: status snapshot |
| adapter가 Restore를 bounded하게 거절하고 재시점 또는 실패 코드를 전달함 | Simulated: adapter tests; Fleet: `tools/drive` |
| adapter의 `Refused`/`Failed`/`Inconsistent` 결과 전달 | Pure: wire tests; Simulated: cache tests |
| 재시작 후 cache 열거와 30일 GC | Pure retention policy: `layers/service/src/outer_policy.rs`; adapter enumeration/scheduler integration remains |
| 다단계 Restore 중 partial residency가 실행 가능 상태로 노출되지 않음 | Simulated: `layers/service/src/cache.rs`, `layers/service/tests/cache_barrier.rs`, mock recovery tests |

이 항목들은 P4가 KV 정책을 해석한다는 뜻이 아니다. P4가 브로커로서 순서,
식별자, 결과 전달을 잃지 않아야 adapter와 agent 정책이 각자의 책임을 수행할
수 있다는 뜻이다.

관련 문서: [protocol.md](protocol.md), [architecture.md](architecture.md),
[api.md](api.md), [testing.md](testing.md).
