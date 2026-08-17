# P4 distributed mock test plan

목표: 중앙 PC와 `m42-server2`의 RTX 3090×2 Windows 11 환경에서, 실제
llama.cpp를 사용하지 않고 `p4-mock`을 어댑터로 사용해 discovery부터
분산 load, 다중 inference, queue/backpressure, monitoring, cache, failure
return까지의 프로토콜 시나리오를 검증한다.

범위:

- 중앙 PC: `192.168.0.6`, P4 소스와 빌드 책임
- 원격 PC: `192.168.0.29`, SSH `42mob@m42-server2`
- 원격 빌드: 금지. 중앙 PC에서 만든 동일 Windows x64 바이너리만 복사
- 어댑터: `mock`, `mock-instant`, 필요 시 `p4-link`
- 실험 전제: 원격의 기존 프로세스·GPU 작업·P4 포트를 확인하고 충돌하면
  중단한다. 사용 중인 서비스는 종료하지 않는다.

## 1. 실행 산출물과 증거

각 실행은 `target/remote-mock-e2e/<run-id>/`에 다음을 남긴다.

| 산출물 | 검증 대상 |
| --- | --- |
| `manifest.json` | commit, binary hash, host, adapter, topology, limits |
| `agent-local.log`, `agent-remote.log` | 등록·discovery·load·status·failure |
| `client.jsonl` | request/stream/event order와 terminal 결과 |
| `status-before.json`, `status-during.json`, `status-after.json` | queue, in-flight, node phase, peer 상태 |
| `verdict.json` | 시나리오별 pass/fail과 실패 원인 |

바이너리는 복사 전에 중앙·원격 SHA-256을 비교한다. 원격에는 소스나
`cargo` 실행을 요구하지 않는다.

## 2. 공통 topology

```text
OUTER/controller
      |
  local ingress agent :52001
      |
  local mock stage :52002 ---- TCP link/relay ---- remote agent :52003
                                                   |
                                             remote mock stage :52004
      <--------------- reply/status ----------------┘
```

실제 테스트는 다음 두 모드를 모두 사용한다.

1. `local-only`: 모든 agent와 mock stage를 중앙 PC에 두고 protocol 오류를
   원격 네트워크와 분리한다.
2. `cross-host`: local ingress/stage와 remote stage를 분리해 frame routing,
   return anchor, peer queue, reconnect, link impairment를 검증한다.

## 3. 병렬 실행 묶음

서로 다른 포트와 `run-id`를 사용하는 다음 네 개의 worker 시나리오는
동시에 실행할 수 있다. 동일 deployment를 공유하지 않는다.

| worker | 시나리오 | 핵심 증거 |
| --- | --- | --- |
| W1 | discovery/model profile | `Inspect`, `InspectModel`, artifact/profile round trip, unsupported adapter refusal |
| W2 | sustained pipeline | 연속 prefill/decode, stage overlap, FIFO, ceiling, bounded queues |
| W3 | return/monitoring | 다중 OUTER route, ingress return, status correlation, disconnect/reconnect |
| W4 | lifecycle/cache/failure | load/unload, persist/restore/fork/discard, deadline, failed hop |

W1은 새 `InspectModel` wire와 mock profile을 먼저 smoke한다. W2~W4는
W1의 binary smoke가 통과한 뒤 parallel fan-out한다. 테스트 runner 자체는
각 worker를 별도 프로세스로 실행해 한 worker의 CPU spin이나 종료가 다른
worker의 결과를 가리지 않게 한다.

반복 실행 명령은 [`tools/scripts/e2e/run-distributed-mock.ps1`](../tools/scripts/e2e/run-distributed-mock.ps1)이다.
기본값은 4개 worker, worker당 2-stage, 128 requests, 16 tokens이며 각
worker가 독립 포트·로그·deployment를 사용한다.

## 4. 시나리오와 판정 기준

### D-01 discovery contract

OUTER가 artifact reference와 adapter 이름으로 `InspectModel`을 요청한다.
Agent는 mock profile을 반환하고, P4는 profile 문자열을 해석하거나
재작성하지 않는다.

판정:

- request/reply correlation이 유지된다.
- artifact, adapter, profile이 byte-preserving round trip한다.
- 등록되지 않은 adapter와 빈 artifact는 명시적 `Failed`가 된다.
- 기존 `Inspect`는 machine snapshot만 반환하며 model profile을 가장하지
  않는다.

### D-02 distributed placement input

local/remote agent에서 각각 model profile과 capability snapshot을 수집한
뒤 OUTER가 하나의 placement plan을 만들고, 각 node에는 opaque `Load`를
보낸다.

판정:

- 모든 stage가 동일한 model fingerprint/profile을 사용한다.
- profile을 얻기 전에는 `Load`를 보내지 않는다.
- `Internal` adapter를 staged chain의 중간 node로 사용하지 않는다.
- snapshot 불일치 또는 지원하지 않는 distribution은 load refusal이 된다.

현재 mock profile은 GGUF parser의 대체물이 아니다. D-02는 discovery
transport와 planner 입력 계약만 증명하며, 실제 GGUF metadata/tensor index
프로파일은 별도 adapter 구현에서 검증한다.

### P-01 pipeline feed-ahead

각 stage의 hop 비용을 서로 다르게 두고 64~256개의 요청을 지속적으로
주입한다. prefill이 decode를 막지 않는지와 stage 0/1이 앞선 요청과
뒤따른 요청을 겹쳐 처리하는지를 관찰한다.

판정:

- node `ceiling`을 넘는 adapter hop이 없다.
- stage별 busy 시간이 겹치며, queue가 존재하는 동안 idle gap이 지속적으로
  증가하지 않는다.
- token/event 순서는 request별 FIFO이고 서로 다른 request가 섞이지 않는다.
- ingress lane과 node queue가 모두 bounded이며 overflow 정책이 명시된다.

### Q-01 장기 도착과 메모리 압박

생산 속도를 mock 처리 속도보다 빠르게 유지하고 10분 이상 실행한다.
request 수, frame 수, queue depth, process working set을 10초 간격으로
기록한다.

판정:

- queue depth는 설정된 상한을 넘지 않는다.
- reader/worker가 무한히 block되지 않고 명시적 reject/deadline/spill 중
  하나가 관찰된다.
- working set이 요청 수에 비례해 무한 증가하지 않는다.
- 완료된 request의 route/continuation/KV 상태가 잔류하지 않는다.

### R-01 return anchor와 다중 OUTER

하나의 ingress agent에 두 개의 logical OUTER channel을 연결하고 동일한
route 문자열이 재사용되는 요청을 동시에 보낸다. 이어 ingress 연결을
끊고 재연결 정책을 실행한다.

판정:

- token과 Done은 최초 ingress/return channel로만 도착한다.
- route 재사용이 다른 channel의 응답을 소비하지 않는다.
- 재연결은 buffer, rebind, cancel 중 선언된 정책 하나로 끝난다.
- 늦은/중복/terminal 이후 event는 새 요청 상태를 오염시키지 않는다.

### M-01 monitoring transparency

유휴, load 중, prefill, decode, blocked link, failed hop, unload 직후의
status를 각각 수집한다.

판정:

- agent/node/adapter identity, request/stream/sequence/hop correlation이
  status와 event에 연결된다.
- lane depth, peer queue, in-flight, phase, last-progress, deadline이
  구분된다.
- queue depth만으로 GPU utilization을 주장하지 않고 mock busy/idle 및
  adapter report를 함께 기록한다.
- snapshot sequence와 generated time으로 stale status를 거부할 수 있다.

### K-01 cache lifecycle

각 stage에서 동일 sequence를 persist, restore, fork, discard하고, restore
실패와 deployment generation 변경을 각각 주입한다.

판정:

- 모든 stage가 같은 operation/sequence identity를 보고한다.
- 일부 stage만 restore된 상태를 inference에 노출하지 않는다.
- fork는 원본을 보존하고 새 sequence만 독립적으로 변경한다.
- model fingerprint, deployment generation, cache format이 맞지 않으면
  명시적으로 거부한다.

## 5. 실행 순서

1. `git rev-parse HEAD`, `cargo test --workspace`, binary hash를 manifest에
   기록한다.
2. local-only W1을 실행해 discovery codec와 mock hook을 확인한다.
3. 중앙 PC에서 release binary를 빌드하고 원격 Windows 경로로 복사한다.
4. 원격에서 hostname, OS/architecture, binary hash, 사용 중인 P4 port와
   기존 P4/backend process를 read-only 확인한다.
5. 충돌하지 않는 별도 port로 cross-host W1 smoke를 실행한다.
6. W1 통과 후 W2~W4를 별도 run-id로 병렬 실행한다.
7. Q-01을 별도 장시간 run으로 실행하며 작업 관리자 또는 PowerShell의
   process working set과 P4 status를 함께 샘플링한다.
8. 모든 프로세스와 listener를 run manifest와 대조해 종료 후 잔류가 없는지
   확인한다. 기존 사용자 프로세스는 종료 대상에서 제외한다.

## 6. 현재 구현의 제한과 다음 구현 단계

- `InspectModel`은 P4 wire와 adapter hook을 사용하며, mock profile은
  의도적으로 GGUF 사실을 모사하지 않는다.
- served concrete adapter는 `P4_MODEL_DIR` 또는 `LLAMA_MODEL_DIR` 아래의 상대
  artifact reference를 검증하고 실제 GGUF metadata/tensor index profile과
  fingerprint를 반환한다.
- 실제 모델 파일에 대한 parser smoke는
  `Qwen2.5-1.5B-Instruct-Q8_0.gguf`로 통과했다.
- capability snapshot ID, model fingerprint, expiry를 Load plan에 묶는
  필드는 아직 추가해야 한다.
- Agent lane, node ingress, adapter event channel, node outbox는 모두
  `Budget.depth` 기반의 bounded RAM 경로다. 초과 node work는 기다리지
  않고 명시적 `Failed`로 반환되며, 느린 downstream은 bounded outbox를
  통해 event loop까지 역압을 전파한다. 아직 disk spill, FIFO paging,
  retry quota 정책은 구현하지 않았다.
- 실제 staged GPU adapter가 없으므로 이 계획의 mock overlap 결과는 GPU
  utilization 증거가 아니다.

관련 계약:

- [protocol.md](protocol.md#11-discovery-is-required-before-distributed-loading)
- [testing.md](testing.md)
- [service message](../layers/service/src/message/mod.rs)
- [service wire](../layers/service/src/message/wire.rs)
- [adapter contract](../layers/adapters/adapter/src/lib.rs)
- [mock adapter](../layers/adapters/mock/src/lib.rs)

## 7. 실행 결과

2026-08-18 중앙 PC와 `192.168.0.29`에서 release binary만 사용해 cross-host
mock smoke를 수행했다.

| 항목 | 결과 |
| --- | --- |
| binary | 중앙 release build 후 `p4-agent.exe`, `p4-drive.exe`만 원격 복사 |
| hash | 중앙/원격 `p4-agent.exe` `48406259...1F79DE6`, `p4-drive.exe` `67BD4B41...D999E40` 일치 |
| topology | local stage + SSH-forwarded remote stage, 2 stages |
| load | nodes 2, mock, ceiling 8 |
| inference | 32 requests × 8 tokens |
| result | completed 32, failed 0, unanswered 0, tokens 224 |
| timing | 659 ms, 388 frames/s |
| queue | peak node 25, peak adapter 8, peak main lane 1 |
| ordering | every stream in order, one terminal per route |

직접 `192.168.0.29:52001` 경로는 원격 agent가 정상 기동했지만 중앙에서
원격 TCP listener에 연결할 수 없어 30초 node creation timeout이 발생했다.
방화벽을 변경하지 않고 SSH `-L 52101`과 `-R 52003` 양방향 forwarding을
사용해 재실행했고 통과했다. 첫 tunnel 시도는 remote agent가 SSH 세션 종료와
함께 사라지는 문제가 있어 PTY 세션으로 agent를 유지했다. 이 결과는 원격
Windows 운영 시 방화벽·프로세스 수명·forward/reply 경로를 모두 manifest에
기록해야 한다는 증거다.

2026-08-18에는 `run-distributed-mock.ps1`로 4개 worker를 병렬 실행했다.
각 worker는 독립적인 2-stage mock deployment에서 128 requests × 16 tokens를
처리했다. 네 worker 모두 `completed=128`, `failed=0`, `unanswered=0`,
stream order 통과를 기록했다. worker별 peak node queue는 83~87,
peak in-adapter는 16, main lane은 1~3이었다. 이 결과는 여러 agent/driver가
동시에 동작해도 request별 FIFO와 terminal correlation이 유지됨을 검증한다.

bounded release를 원격에 재복사한 뒤 2026-08-18 cross-host tunnel smoke도
재실행했다. 중앙 stage와 원격 stage의 2-stage topology에서 32 requests ×
8 tokens가 `completed=32`, `failed=0`, `unanswered=0`, stream order 통과로
끝났고 peak node queue 25, peak in-adapter 8, peak main lane 1이었다. 실행
후 중앙·원격 P4 listener는 모두 정리됐다.

최신 bounded event/outbox release로 4개 worker에 각각 1024 requests × 32
tokens를 다시 실행했을 때도 모두 완료했고, worker별 peak node queue는
728~744, peak in-adapter는 16이었다. 6000 requests × 1 token overflow run도
`completed=6000`, `failed=0`, `unanswered=0`으로 끝났고 peak node queue는
4, peak main lane은 2였다. 즉 생산자는 lane admission에서 조절되고 node
queue/event/outbox는 무제한으로 증가하지 않았다. 최신 release를 원격에 복사해
동일한 SSH 양방향 forwarding 경로로 32 requests × 8 tokens를 재실행했고
`completed=32`, `failed=0`, `unanswered=0`, stream order 통과, 569 ms,
450 frames/s, peak node 26, peak adapter 8, peak main lane 1을 확인했다.
이 결과는 RAM 상한 자체를 증명하는
것은 아니므로 process working-set 샘플을 포함한 별도 장기 검증이 필요하다.
