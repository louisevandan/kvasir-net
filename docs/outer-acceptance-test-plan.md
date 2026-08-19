# OUTER 인수테스트 3턴 계획

이 문서는 OUTER 세션·취소·복원·KV 수명주기의 인수테스트 순서와 판정 기준을
고정한다. 테스트 케이스를 임의로 추가하거나 순서를 바꾸지 않는다. 총 세 턴만
허용한다.

## 변경·커밋 규칙

| 턴 | 허용 변경 | 커밋 경계 | 테스트 범위 |
| --- | --- | --- | --- |
| 1차 | 계획 문서 커밋 후, 인수테스트에서 발견된 결함만 수정 | 계획 커밋 → 결함 수정 커밋 | 전체 핵심 계약과 상호작용 |
| 2차 | 1차 결함 수정 및 재검증. 새 구조·새 시나리오 추가 금지 | 2차 결함 수정 커밋 | 1차 실패 재현 + 전체 회귀 |
| 3차 | 미세조정, flaky 제거, 로그·판정 보완만 허용 | 최종 수정 커밋 | 최종 smoke·전체 게이트 |

각 턴에서 테스트 전에 `git status`, 바이너리 hash, 포트, state root를 기록한다.
테스트 산출물은 `target/outer-acceptance/<turn>-<run-id>/`에 저장한다.

## 격리 토폴로지

llama.cpp 테스트 세션과 충돌하지 않도록 OUTER 인수테스트는 별도 프로세스와
상태 디렉터리를 사용한다.

```text
OUTER driver :52102
        │
agent A :52100 ── agent B :52101
        │              │
   mock stage A   mock stage B
```

- adapter: `mock` 또는 `mock-instant`
- agent: `p4-agent.exe` 두 프로세스
- OUTER: `p4-drive.exe` 별도 프로세스
- 상태: `target/outer-acceptance/<turn>-<run-id>/state-*`
- 로그: agent stdout/stderr, drive stdout/stderr, process exit code
- 포트가 사용 중이면 기존 프로세스를 종료하지 않고 다른 run을 시작하지
  않는다. 충돌을 기록하고 해당 실행은 실패 처리한다.

## 병렬 실행 묶음

각 묶음은 독립 agent·port·state root·deployment를 사용하므로 1차와 2차에서
동시에 실행한다. 묶음 내부의 단계 순서는 고정한다.

| 묶음 | 검증 축 | 핵심 증거 |
| --- | --- | --- |
| A | 연결·재접속·generation | stale ACK/event 거부, 새 generation 수용, journal replay |
| B | 요청 순서·복원 barrier | 동일 sequence의 Restore 순서, Restore 완료 전 Hop 차단, partial residency 거부 |
| C | 취소·terminal | queued 취소, active hop 경계, duplicate Cancel, terminal 1회·replay |
| D | KV transaction | 2-stage/4-stage Persist·Restore·Discard, commit 실패 보상, restart recovery |
| E | 수명정책 | heartbeat miss threshold, retain-until 경계, 중복 GC 후보 제거 |

묶음 A~E는 병렬 실행하지만, 모든 묶음이 같은 agent나 state root를 공유하지
않는다. 병렬 실행 결과는 묶음별 manifest로 합친다.

## 1차: 전체 계약과 논리 관계 확인

### 1차 사전 게이트

1. 계획 커밋의 commit id 기록
2. `cargo fmt --all -- --check`
3. `cargo clippy --workspace --all-targets -- -D warnings`
4. `cargo test --workspace`
5. release `p4-agent.exe`, `p4-drive.exe` hash 기록
6. reserved ports가 비어 있고 기존 llama.cpp 세션이 살아 있음을 확인

사전 게이트가 실패하면 인수테스트를 시작하지 않는다.

### 1차 케이스

각 케이스는 요청 ID, sequence ID, operation ID, route, return channel,
ingress generation을 로그와 대조한다.

| ID | 시나리오 | 반드시 확인할 관계 |
| --- | --- | --- |
| A1 | 연결 후 event 수신·ACK | channel + stream + event sequence |
| A2 | 연결 단절 후 재접속 | 이전 generation의 ACK/event가 새 연결을 침범하지 않음 |
| A3 | 동일 sequence Restore 두 건 | 발행 순서와 adapter 실행 순서 일치 |
| A4 | Restore 중 후속 Hop | 모든 stage 완료 전 Hop이 실행되지 않음 |
| A5 | 한 stage Restore 실패 | partial resident가 executable로 공개되지 않음 |
| A6 | queued request Cancel | 대상 request만 terminal 처리 |
| A7 | 같은 Cancel 재전송 | terminal 추가 생성 없음, 동일 결과 replay |
| A8 | active hop Cancel | 강제중단을 주장하지 않고 경계 이후 종료 |
| A9 | 4-stage Persist → restart → Restore | 각 shard와 position 보존 |
| A10 | commit 중 한 stage failure | abort/reconcile 후 성공 상태를 가장하지 않음 |
| A11 | 만료 경계 전후 GC | `now < retain_until` 보존, `now >= retain_until` 폐기 후보 |
| A12 | 동시 서로 다른 sequence | 서로 다른 sequence는 병렬, 동일 sequence는 순서 보존 |

### 1차 판정

- 위 A1~A12 중 하나라도 실패하면 즉시 테스트를 중단하고 실패 manifest와
  관련 로그를 보존한다.
- 원인 분석 후 구현을 수정한다. 수정 범위는 실패를 직접 설명하는 코드와
  해당 회귀테스트로 제한한다.
- 수정 후 `outer-acceptance-1-fix` 커밋을 만든다.

## 2차: 1차 결함 재현과 전면 회귀

2차는 1차에서 실패한 케이스를 먼저 같은 입력으로 재현한다. 재현되지 않으면
수정 완료로 판정하지 않는다. 이후 A~E 병렬 묶음 전체와 `cargo test --workspace`
를 재실행한다.

판정 기준:

- 1차 실패 케이스 0건
- A1~A12 전체 0건
- 병렬 묶음 간 cross-talk 0건
- terminal 누락·중복 0건
- 종료 후 agent/drive/listener 잔류 0건
- 1차에서 변경하지 않은 기존 테스트 회귀 0건

새로운 실패가 발견되면 2차 수정 커밋만 허용한다. 테스트 범위를 넓히거나
프로토콜 의미를 바꾸지 않는다.

## 3차: 최종 미세조정

3차는 2차에서 통과한 계약을 유지한 채 다음만 수행한다.

- timing margin과 polling 안정화
- 로그·manifest의 누락 필드 보완
- deterministic ordering과 flaky 원인 제거
- 최종 포맷·Clippy·diff·workspace test

3차에서 의미 있는 프로토콜·상태·스케줄링 변경이 필요하면 최종판으로 인정하지
않고 별도 변경으로 되돌린다.

## 최종 산출물

최종 커밋에는 다음이 함께 있어야 한다.

- 1차/2차/3차 manifest
- 묶음 A~E별 pass/fail과 process exit code
- agent/drive 로그 경로
- 실행 바이너리 hash
- 포트·state root 목록
- ignored된 실 llama.cpp E2E와 그 이유
- `protocol-outer.md`의 구현 상태 갱신

실 llama.cpp adapter 테스트는 이 계획의 대체물이 아니다. OUTER 계약은 Mock
adapter로 먼저 확정하고, llama.cpp 세션에서는 같은 wire·cache·cancel 입력을
실 backend에 연결하는 별도 acceptance로 검증한다.
