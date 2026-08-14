# P4 수정 계획

> 상태: **수정 지침서.** 대개편 항목을 모두 짚었고, §91가 순차 구축 순서를 정의한다. 착수 단위는 §91의 단계이며 각 단계의 관문은 §89의 `Q`다.
> 기준 구현: P4B1 v5 (`layers/protocol`, `layers/runtime`, `layers/adapters`, `tools/controller`)
> 대상: **P4B1 v6** (§90)
> 브랜치: 별도 브랜치에서 진행. 종료 조건은 **에이전트가 쓸 구상 어댑터가 `apps/p4` 밖에 없는 것** (주제 M)
> 최초 작성: 2026-08-13 · 완결: 2026-08-14

## 요약

| | |
|---|---|
| 전제 | 12개 (§1.1) |
| 주제 | A~M 13개 |
| 확인된 결함 | `D-1`~`D-78` (철회·해소 4건 포함) |
| 보완 설계 | `P-1`~`P-60` (철회 2건 포함) |
| 미결 | `Q-1`~`Q-65` 중 **48건 미결**, 17건 결정·철회·해소 |
| 외부 종속 | `Q-48`·`Q-49` — 병행 TPS 세션의 결론 대기 (§92) |
| 구축 단계 | 0~6, 순차 (§91) |

**가장 무거운 셋** — 나머지가 여기에 얹힌다.
1. **CPS 위반이 토대 계약에 박혀 있다** (`D-26`). 제어 평면 전체가 반환값 기반이며 함수 이름이 `compatibility`다
2. **주소·식별자 체계 재편** (`P-2`·`P-34`). 자기기술 주소가 에이전트 identity가 되고 중계가 큐 기본 동작이 된다
3. **체인이 P4 밖에 있다** (`D-35`). 인퍼런스 경로의 본체가 프로토콜에 표현되지 않는다

**어댑터 경계 감사 결과** — 의존 방향은 정확하고 `layers/protocol`에 백엔드 문자열이 0건이다. 새는 곳은 **계약과 문서**다: `DRAFT_REPORT`의 KV·FFN(`D-62`), `docs/model-load.md`가 llama.cpp 노브를 정식 스키마로 규범화(`D-63`), 어댑터 인터페이스가 산출물로 부재(`D-64`).

**첫 착수는 단계 0** — `P-40`. Pipeline 어댑터의 5개 화이트리스트를 걷어내는 것만으로 구조화 출력 불가 상태(`D-56`)가 풀린다. wire 변경도 다른 단계 의존도 없다.

## 0. 이 문서의 규약

### 표기

| 표기 | 뜻 |
|---|---|
| `D-n` | 확인된 결함. 근거 파일·행이 붙는다 |
| `Q-n` | 미결 결정. 답이 나오기 전에는 하위 설계를 확정하지 않는다 |
| `P-n` | 보완 제안. `Q`가 풀리면 사양으로 승격한다 |
| (초안) | 소스 대조는 끝났으나 합의 전 |
| (결정) | 회의에서 확정. 근거를 함께 남긴다 |
| (철회) | 이후 논의에서 뒤집힌 항목. 지우지 않고 남겨 이유를 보존한다 |

결함은 소스에서 확인된 것만 적는다. 추정은 `Q`로 내린다.

### 개정 방식

한 번에 전체를 기술하지 않는다. 주제 단위로 회의하고, 합의된 만큼만 채운다.
**기존 서술과 모순이 발견되면 새 절을 덧붙이지 않고 해당 절을 고친다.** `D`/`P`/`Q` 번호는 전역 연속이고 재사용하지 않는다 — 철회된 항목도 번호를 유지한 채 `(철회)`로 남긴다. 절 번호는 주제가 늘면 재배치될 수 있으므로 참조는 `D`/`P`/`Q` 번호로 한다. 개정 내역은 마지막 절에 기록한다.

**절 번호 규약:** 주제 절은 §2부터 순차로 늘어나고, 주제와 무관한 종합 절(미결·순서·처리량·파급·잔여·이력)은 **§89 이상 고정**이다. 주제가 추가되어도 종합 절 번호가 밀리지 않는다.

번호가 전역 연속이고 재배치하지 않으므로 **주제 안에서 항목 번호가 순서대로 나오지 않는다.** 나중에 추가되거나 다른 주제에서 옮겨온 항목이 있기 때문이며, 의도된 것이다. 예: 주제 A의 `P-39`, 주제 G의 `P-33`.

### 이 문서를 읽는 순서

| 목적 | 절 |
|---|---|
| 무엇을 만들려는가 | §1.2 대상 아키텍처, §1.3 식별자 소유 |
| 무엇이 잘못되었는가 | 주제 A~I의 "결함" 절 |
| 무엇을 할 것인가 | 주제 A~I의 "보완 설계" 절 |
| 무엇을 정해야 하는가 | §89 미결 결정 |
| 재작성인가 수정인가 | §91.0 판단 기록 |
| 어떤 순서로 할 것인가 | §91 구축 순서 — **착수 단위** |
| 처리량과 어떤 관계인가 | §92 — **P4는 처리량을 소유하지 않는다** |

## 1. 배경 전제와 대상 아키텍처

### 1.0 용어

**OUTER** — 외부 요청 주체. 편성 권위를 갖고 에이전트·컨트롤러에 지시하며 결과를 수신한다. 이 문서에서 지금까지 "외부"로 지칭한 주체가 OUTER다.

### 1.1 전제

회의에서 확정된 방향이다.

1. 상태 자산은 에이전트 외부에 존재한다. 어떤 노드가 있는가, 어떤 컨트롤러가 있는가, 어떤 모델이 어디에 있는가는 외부 레코드가 권위를 가진다.
   **OUTER는 인프라 사실의 소유자다.** 모든 에이전트의 접근 주소를 이미 알고 있다. 인프라 사실을 프로토콜로 발견하려 하면 "누가 먼저인가"의 선후 모순만 생기므로, **프로토콜로 알아내야 할 것과 OUTER가 이미 아는 것을 구분한다.** 주소·배치 권한 같은 인프라 사실은 후자이며 P4의 조회 대상이 아니다.
2. 함수적 형태를 지향한다. 인자로 전달될 상태는 외부에 있다고 본다.
3. 노드의 생성·모델 적재·해제는 OUTER가 지시한다. 컨트롤러는 이 지시를 **소유하지 않고 경유만** 한다 — 해석하지도, 검증하지도, 상태를 보유하지도 않는다. (경유는 소유가 아니다)
4. 모델 적재는 OUTER 요청으로 재편한다. 적재 정책은 OUTER가 수립해 프로토콜로 주입하며, 그 표현력은 구상 런타임이 실제로 제공하는 수준을 담아야 한다.
5. 적재의 진행·완료·실패는 지시한 OUTER로 돌아간다. 경로는 컨트롤러를 경유하되 컨트롤러는 통과시킬 뿐이다.
6. 적재 옵션은 P4가 해석하지 않는다. 문자열로 통과시키고 구상 어댑터가 해석한다.
7. 적재·해제는 노드의 사전 상태에 의존한다. 사전 조건을 어긴 명령은 실패 메시지가 된다. 해제가 성공하면 노드의 실체는 완전히 해지된다.
8. CPS다. 응답은 반환값이 아니라 **메시지가 되어 요청자를 호출한다.** 따라서 한 번 emit한 것은 되돌릴 수 없다.
9. **모든 프로토콜 처리에 반환값이 없다.** 워커가 할 수 있는 일은 자기 일을 하고 메시지를 큐에 넣는 것뿐이다. 응답이든 전달이든 결과는 큐를 거친다. 이를 지키지 않는 처리 경로는 전부 수정 대상이다.
10. **노드는 자신의 적재 구조를 최대한 모른다.** 추상층을 유지한다. 레이어 번호 등 배치 구조에 관한 판정은 노드의 검사에서 배제하고, 그 책임은 편성 주체인 OUTER가 진다.
12. **인퍼런스 옵션도 P4가 해석하지 않는다.** 전제 6(적재 옵션)의 대칭이다. 구상 런타임에 실제 인자로 전달되어야 하는 모든 스펙을 지원해야 하며, 어댑터가 임의로 선별하지 않는다. **미지원 옵션을 조용히 버리는 것은 금지한다.**
11. **네트워크 도달성이 제약이다.** 방화벽 밖에 컨트롤러가 있고 내부망과는 터널 하나만 열리는 배치가 일반적이다. 제약은 정확히 둘이다.
    - 컨트롤러는 **정확히 하나의 에이전트**에만 접근할 수 있다. 그 에이전트는 컨트롤러 자신의 에이전트일 수도, 다른 에이전트일 수도 있다. 이를 **진입 에이전트**라 한다
    - 진입 에이전트는 **나머지 모든 에이전트에 접근할 수 있다**

    따라서 진입 에이전트는 특정 노드를 품을 이유가 없다. 노드 생성·삭제, 모델 적재·해제, 인퍼런스 등 모든 메시지를 진입 에이전트로 보내면, 그 에이전트가 스스로 소비하거나 다른 에이전트로 전달해 수행한다. **컨트롤러를 경유하는 가장 큰 이유가 이 제약이다.**

### 1.2 대상 아키텍처 흐름

개별 결함 수정이 향하는 목적지다.

#### 위상 — 도달성이 구조를 정한다 (전제 11)

```text
        방화벽
          │                                    ┌──▶ Agent A ──▶ Node
 OUTER ───┼──▶ Controller ──▶ Entry Agent ─────┼──▶ Agent B ──▶ Node
          │                   (진입 에이전트)   └──▶ Agent C ──▶ Node
                                   │
                                   └── 자신이 노드를 품을 수도 있다
```

- 컨트롤러가 도달할 수 있는 에이전트는 **정확히 하나**다. 컨트롤러 자신의 에이전트일 수도 있다
- 그 진입 에이전트는 **나머지 모든 에이전트에 도달**한다
- 진입 에이전트는 **특정 노드를 품을 이유가 없다.** 노드 배치와 무관한 순수 진입점이며, 필요하면 자기도 노드를 가질 수 있을 뿐이다

따라서 모든 메시지는 종류를 가리지 않고 같은 관문을 지난다. **경유와 소유를 구분하는 것이 이 설계의 핵심이다** — 컨트롤러와 진입 에이전트는 메시지를 나르지만 해석하거나 소유하지 않는다.

#### 제어 경로 — 컨트롤러를 경유하되 소유하지 않는다

```text
OUTER ──inventory/capability──▶ Controller ──▶ Entry ──▶ (자기 소비 또는 대상 Agent)
OUTER ──node create/delete────▶ Controller ──▶ Entry ──▶ Agent ──▶ NodeSlot
OUTER ──model load/unload─────▶ Controller ──▶ Entry ──▶ Agent ──▶ Adapter
OUTER ◀──progress/완료/실패──── Controller ◀── Entry ◀── Agent
```

편성·적재·해제의 **지시 주체는 여전히 OUTER**이고, 컨트롤러와 진입 에이전트는 통과시킬 뿐이다. 검증도 상태 보유도 하지 않는다. (전제 3·4·5·11)

진입 에이전트가 대상 에이전트 자신이면 전달 없이 그 자리에서 소비한다.

#### 인퍼런스 경로 — 컨트롤러가 체인을 관리한다

```text
OUTER ──inference(node chain)──▶ Controller
                                    │ PREFILL(체인 전체를 담아) 1회 송신
                                    ▼
                              Node[0] ──▶ Node[1] ──▶ … ──▶ Node[n]
                                     (각 노드가 메시지의 체인을 보고 스스로 다음으로 전달)
                                                                  │
                                    ┌────────generated token──────┘
                                    ▼
OUTER ◀────────token stream──── Controller
```

**컨트롤러는 홉마다 개입하지 않는다.** 체인은 컨트롤러가 1번 노드에게 보내는 프리필 메시지 안에 담기고, 각 노드는 그 메시지로부터 다음 노드를 스스로 안다. 소스 라우팅이다.

디코드는 노드 주도로 순환한다. 컨트롤러의 최초 프리필 요청 이후 생성 반복은 **노드 간 통신**으로 이뤄진다.

```text
        ┌──────────────── next token 의뢰 ────────────────┐
        ▼                                                 │
   Node[0] ──▶ Node[1] ──▶ … ──▶ Node[n] ─────────────────┘
                                    │ token / 생성 종료
                                    ▼
                              Entry Agent ──▶ Controller ──▶ OUTER
                        (자신이 진입 에이전트면 바이패스)
```

**마지막 노드는 컨트롤러에게 직접 보고할 수 없다**(전제 11). 디코딩·생성 종료를 **진입 에이전트**로 보내고, 진입 에이전트가 컨트롤러로 넘긴다. 보고하는 노드가 이미 진입 에이전트 소속이면 중계 홉을 **바이패스해 곧장 컨트롤러로** 보낸다.

노드 배치는 자유롭다. 1번 노드도 마지막 노드도 진입 에이전트에 있을 이유가 없다.

- OUTER는 컨트롤러에게 **노드 리스트를 전달**한다. 컨트롤러는 각 노드의 구상 상태나 구조를 모른다
- 컨트롤러의 요청당 능동 관여는 **프리필 1회 송신**뿐이다. 이후는 보고 수신과 통과다
- **프리필은 상태 확인 단계이기도 하다.** 각 노드가 진입과 완료를 보고하므로 컨트롤러는 이때 비로소 노드의 실제 상태를 안다 (주제 G)
- **마지막 노드는 OUTER가 아니라 컨트롤러에게** 생성 토큰을 준다. 이후 필터링·부가 작업의 자리를 남기기 위함이며, 현재는 OUTER로 통과시키는 기능만 한다
- **스트림/비스트림 모드는 없다.** 이 시스템의 인퍼런스는 언제나 스트림이다

#### 역할 요약

| 주체 | 아는 것 | 모르는 것 | 위상 |
|---|---|---|---|
| OUTER | 하드웨어 capability, 노드 편성, 적재 계획, 체인 구성 | 실행 중 상태 | 방화벽 밖 |
| Controller | 이번 요청의 노드 리스트와 순서 | 각 노드의 구상 상태·구조 | 방화벽 밖. OUTER의 유일한 접점 |
| Entry Agent | 자기 노드(있다면) + 중계 | 편성 의도, 나르는 내용 | 컨트롤러가 도달하는 **유일한** 에이전트. 노드 배치와 무관 |
| Agent | 자기 머신의 노드 id와 수용력 | 편성 의도, 체인 | 내부망. **모든 에이전트가 중계 능력을 가진다** |
| Adapter | 구상 실체 — 적재된 레이어, 런타임 | P4 상위 의미 | 내부망 |

### 1.3 식별자 소유

**원칙: 식별자는 OUTER가 발급한다.** 예외는 실체 세대와 전송·CPS 내부 ID뿐이다. 이는 전제 1의 직접적 귀결이다 — 상태의 권위가 외부에 있으면 그 상태를 가리키는 이름도 외부가 정해야 한다.

| 식별자 | 발급 | 수명 | 비고 |
|---|---|---|---|
| **접근 주소** | OUTER (인프라 사실) | 배치 변경까지 | **에이전트의 identity** (P-2) |
| `node_id` | OUTER | create ~ delete | 이미 외부 발급 |
| `deployment_id` | OUTER | 배포 논리 단위 | |
| `binding_id` | OUTER | load ~ unload | |
| `plan_revision` | OUTER | 계획 개정 | 외부 의도의 버전 (P-8) |
| `request_id` | **OUTER** | 인퍼런스 1건 | 개별 인퍼런스마다 별도 부여 |
| `session_id` | **OUTER** | 대화·실행 세션 | 에이전트가 발급하지 않는다 (D-54) |
| `operation_id` | OUTER | lifecycle 1건 | |
| `runtime_generation` | **어댑터** | 실체 세대 | 유일한 비-OUTER 업무 ID (P-8) |
| `route_id` | 발신자 | 한 exchange | 전송 계층 correlation |
| `task_id` / `causation_id` | 런타임 내부 | Task 1건 | CPS 내부 |
| ~~`agent_id`~~ | **폐지** | — | 접근 주소가 대신한다 (P-2) |
| `ingress_id` | OUTER | 제출 ~ 승격 | `request_id`와의 중복 여부는 Q-43 |

#### 4-튜플 규칙

실행 가능한 구상 실체는 다음으로 정확히 지정된다.

```text
(에이전트 접근 주소, node_id, binding_id, runtime_generation)
```

앞의 셋은 OUTER가 발급한 이름이고, 마지막 하나만 어댑터가 발급한 실체 세대다. 넷 중 하나라도 빠지면 stale 실체에 실행될 수 있다. 체인 항목(P-25)과 실행 요청이 이 튜플을 공유한다.

---

# 주제 A. 하드웨어 조회 프로토콜

전제 1이 성립하려면 외부가 머신 스펙을 조회할 수 있어야 하고, 그 위에서 노드 편성을 한다. 현 프로토콜은 이 조회의 **계약이 없다.**

## 2. 현재 상태 (검증 완료)

`INVENTORY_QUERY`(34) → `HARDWARE_REPORT`(35) 왕복은 구현되어 있다.

- 요청: `controller_id`, `request_id`
- 응답: `agent_id`, `report_id`, `snapshot`(bounded text)
- 방향: `ExternalController` — [`catalog/mod.rs`](layers/protocol/src/catalog/mod.rs)
- 클래스: Terminal. 응답이 route를 닫는다

`snapshot`의 전체 구현은 [`domain/hardware/mod.rs`](layers/runtime/src/domain/hardware/mod.rs) 35줄이다.

```json
{ "observed_at_unix_ms": 0, "os": "windows", "arch": "x86_64",
  "cpu_physical": 16, "cpu_logical": 32,
  "gpus": ["GPU-abc…, NVIDIA RTX 4090, 24564, 23100, 550.54.14"],
  "adapters": [...], "nodes": [...] }
```

`os`는 `env::consts::OS`, 즉 **컴파일 타임 상수**다. `gpus`는 `nvidia-smi --format=csv,noheader,nounits`의 **원문 문자열 배열**이다.

## 3. 결함

### D-1. `snapshot`에 스키마 계약이 없다
P4 계층에서 `snapshot`은 256 KiB 이하 텍스트일 뿐이다. 버전도, 필수 필드도, 검증도 없다.
근거: [`message/mod.rs`](layers/protocol/src/contract/message/mod.rs) `HardwareReport.snapshot: String`

### D-2. RAM 정보가 전면 부재
total도 available도 없다. CPU offload 가능 여부, KV 예산, mmap 적합성 판단 근거가 통째로 없다. 노드 편성의 1차 입력이 빠져 있다.

### D-3. GPU 정보가 구조화되지 않았고 NVIDIA 전용
- `nvidia-smi` CSV 원문 문자열 → 외부가 규약 없이 문자열을 쪼개야 한다
- 단위 미문서화 (`nounits`는 MiB)
- AMD·Intel·Apple 경로 없음
- compute capability, PCIe bus, NVLink 피어 없음 → 다중 GPU 배치 판단 불가
- 조회마다 프로세스 spawn. 실패 시 조용히 빈 배열이라 **GPU 없음과 드라이버 오류가 구분되지 않는다**

### D-4. CPU·OS 정보가 편성에 못 미친다
코어 수 2개(`cpu_physical`, `cpu_logical`)뿐. 모델명, 클럭, 소켓/NUMA, ISA 확장(AVX-512·AMX) 없음. OS는 커널·버전·배포판 없이 계열 문자열만.

### D-5. 저장소 용량 정보 없음
모델 저장소 총량/여유가 없다. 배치 전에 적재 가능 여부를 알 수 없다.

### D-6. 에이전트가 먼저 말을 걸 수단이 없다
P4의 모든 응답은 이미 열린 `route_id` 위로만 나간다. `HARDWARE_REPORT`는 terminal이라 route를 닫는다. [`agent-link.mjs`](tools/controller/client/transport/agent-link.mjs)와 [`controller-instance.mjs`](tools/controller/client/controller-instance.mjs) 모두 요청 개시만 구현한다.

결과: 외부는 **이미 아는 엔드포인트에만** 물어볼 수 있다. "어떤 노드가 있는가"의 시작점이 외부에 없다. 전제 1에 대해 D-1~D-5보다 치명적이다.

### D-7. 프로세스 교체를 감지할 수단이 없다 (재정의)
`format!("agent-{host}-{pid}")` — [`domain/agent/mod.rs`](layers/runtime/src/domain/agent/mod.rs).

당초 "재시작하면 같은 머신이 다른 에이전트가 되므로 레코드를 키잉할 수 없다"로 적었다. **키잉 문제는 P-2에서 해소되었다** — 접근 주소가 identity이므로 키는 이미 안정적이다.

남는 실질 결함은 반대 방향이다. 주소가 안정적이기 때문에 **에이전트가 재시작해 `NodeSlot`과 바인딩을 전부 잃어도 OUTER는 그것을 알 수 없다.** 프로세스 교체를 감지할 표식이 없다(P-39).

### D-8. 불변 정보와 휘발 정보가 한 덩어리다
`cpu_physical`(하드웨어 교체 전까지 불변)과 `memory.free`(초 단위 변동)가 같은 문서에 같은 신뢰도로 들어 있다. 분리되지 않으면 "편성에 관측 여유값을 쓰지 말라"는 규칙을 강제할 방법이 없다.

## 4. 보완 설계 (초안)

### P-1. capability / occupancy 분리

| 구분 | 성격 | 내용 | 외재화 |
|---|---|---|---|
| **capability** | 하드웨어·드라이버 변경 시에만 변함 | CPU(모델·물리/논리·소켓·NUMA·ISA), RAM 총량, GPU별(uuid·vendor·모델·VRAM 총량·아키텍처·PCIe·NVLink 피어), 저장소 총량, OS/커널 버전 | 외부 레코드로 저장. `capability_revision`으로 변경 감지 |
| **occupancy** | 초 단위 변동 | free VRAM/RAM, 사용률, 온도·전력, 현재 적재 바인딩 | 저장 금지. 진단·검증용 |

**편성 규칙 (필수):** 노드 편성은 occupancy를 입력으로 쓰지 않는다. `capability 총량 − 외부 레코드가 선언한 배치`로 계산하고, 관측 여유값은 그 결과의 검증에만 쓴다.
근거: free VRAM 기준 배치는 두 컨트롤러가 같은 여유를 보고 동시에 커밋한다. 경합을 막는 것은 보고서가 아니라 외부 레코드다.

### P-2. 접근 주소가 에이전트의 identity다 (P-34으로 재작성)

**초안(철회):** `machine_id` / `boot_id` / `agent_instance_id` 3층 identity를 두자는 제안이었다. `agent_id`가 PID에 묶여 있다는 D-7의 해결책으로 적었다.

**확정:** P-34에 따라 모든 메시지가 대상 에이전트의 접근 주소를 자기기술한다. 그리고 그 주소는 OUTER가 소유하는 인프라 사실이다(전제 1). **따라서 별도의 에이전트 ID는 무의미하다 — 접근 주소 자체가 에이전트의 ID다.**

| 기존 용도 | 대체 |
|---|---|
| capability 레코드의 키 | 접근 주소 |
| `is_local_bypass`의 동일성 판정 | 접근 주소 비교 |
| `HARDWARE_REPORT.agent_id` | 불필요 — 물어본 쪽이 이미 주소를 안다 |
| `Participant.agent_id` | 접근 주소 |

`machine_id`도 불필요하다. 주소는 재시작·재부팅을 넘어 안정하며, PID 유래 값보다 오히려 더 안정적이다. `boot_id`도 마찬가지다 — occupancy는 저장하지 않으므로(P-1) 유효 범위를 표시할 대상이 없다.

**단 하나 남는 것: 프로세스 화신(incarnation) 표식** — §하단 P-39 참조. 이는 identity가 아니라 세대 표식이다.

`node_id`는 이미 외부가 발급한다([`controller-instance.mjs`](tools/controller/client/controller-instance.mjs) `createNode`의 `randomUUID`) — 이 축은 이미 전제 1과 정합하다.

### P-39. 프로세스 화신 표식 (identity가 아닌 세대)

주소가 identity를 대신해도 대체하지 못하는 사실이 하나 있다. **에이전트가 재시작하면 `NodeSlot`과 바인딩이 전부 사라진다** — 현재 registry는 프로세스 메모리에만 있고 지속화되지 않는다(D-9 영역). 주소는 그대로이므로 OUTER는 자기 레코드가 무효가 된 것을 알 수 없다.

필요한 것은 ID가 아니라 **"같은 주소인데 다른 화신"을 구별하는 단조 증가 표식**이다. `runtime_generation`이 바인딩 실체에 대해 하는 일과 같은 역할을 에이전트 프로세스에 대해 한다.

D-7의 실질 해결은 여기다 — 문제는 "ID가 PID에 묶였다"가 아니라 **"프로세스가 바뀐 것을 감지할 수 없다"**였다. 지속화(§94.1)를 도입하면 필요 범위가 달라지므로 함께 판단한다(Q-42).

### P-3. `snapshot` 스키마 규범화
버전 붙은 JSON으로 규범화한다(`schema_version` 필수). wire 필드 승격은 하지 않는다 — 하드웨어 속성은 코덱보다 빨리 변하므로, 필드로 올리면 GPU 속성 하나 늘 때마다 프로토콜 버전이 올라간다. 대신 스키마를 **규범**으로 못 박고 검증한다. best-effort 성격은 occupancy 절에만 남긴다.

### P-4. agent-initiated announce (Q-3 종속)
전제 1을 끝까지 밀면 필요하다.
- 새 방향: Agent → External (현 `TaskDirection`에 없음)
- non-terminal 갱신 프레임 (현 `HARDWARE_REPORT`는 terminal)
- 등장 시 announce + capability 변경 시 재announce

**P4B1 v6 급 변경이다.** v5 호환 포기 결정이므로 Q-3에서 함께 판단한다.

---

# 주제 B. 노드 소유권과 노드의 실체

전제 3을 코드와 대조한 결과다.

## 5. 현재 상태 (검증 완료)

### 5.1 코드가 이미 전제 3과 일치하는 부분

- **노드 정의는 하드웨어 제약이지만 `NodeSlot`에는 제약이 없다.** `NodeSlot`은 `controller_id`, `adapter_id`, `max_inflight`, `admission`, `bindings`뿐 — [`registry/node/mod.rs`](layers/runtime/src/domain/agent/registry/node/mod.rs)
- **에이전트는 상세 제약을 모른다.** [`node_spec/mod.rs`](layers/runtime/src/domain/agent/lifecycle/node_spec/mod.rs) 주석 그대로 — Agent는 `p4_max_inflight` 한 필드만 읽고 나머지를 무시한다
- **실체는 모델 로딩 때 드러난다.** `MODEL_BOUND(state=ready)`일 때만 `bind()`가 `Binding{deployment_id, generation}`을 만든다
- **부분로딩 전략은 외부가 결정해 주입한다.** `stage_plan`은 Agent가 해석하지 않고 어댑터로 통과
- **언로드하면 id는 남고 실체만 사라진다.** `unbind()`는 `bindings`에서만 제거
- **`runtime_generation`은 어댑터가 발급한다.** Agent는 `MODEL_BOUND`의 값을 기록만 한다

### 5.2 코드가 전제 3과 어긋나는 부분

현재 노드는 **controller-owned**다. `NODE_CREATE`가 `controller_id`를 나르고 `NodeSlot::new(controller_id, …)`가 소유권을 각인한다. 이후 `MODEL_LOAD`, `MODEL_UNLOAD`, `HEALTH_CHECK`, `EXECUTE`가 전부 `owned_node()`를 통과한다 — [`authorization/mod.rs`](layers/runtime/src/domain/agent/authorization/mod.rs).

## 6. 결함

### D-9. 노드 제거가 프로토콜에 없다
`nodes.remove` 호출이 저장소 어디에도 없고 `NODE_DELETE` kind도 없다. **노드 수명이 Agent 프로세스 수명과 같다.** 노드 목록을 외부가 권위 있게 관리하려면 필수 결손이다.

### D-10. `NODE_CREATE` 재적용이 조용한 no-op
[`lifecycle/mod.rs`](layers/runtime/src/domain/agent/lifecycle/mod.rs)의 `entry(node_id).or_insert_with(…)`. 같은 `node_id`로 다시 만들면 새 `node_spec`이 무시되고, 호출자는 성공 응답을 받아 반영되었다고 오해한다. 갱신과 무시가 구분되지 않는다.

### D-11. `node_spec`의 하드웨어 제약이 저장되지 않는다
`create_node`는 `max_inflight`만 추출하고 원문을 버린다. 어댑터로 전달은 되지만 registry에는 남지 않는다. 제약은 실체가 아닌 정도가 아니라 **에이전트 안에서 소멸한다.** 외부가 기억하지 않으면 아무도 기억하지 않는다 — 전제 1과 정합하지만, 현재는 외부 레코드도 없으므로 그냥 유실이다.

### D-12. `controller_id` 게이트는 인증이 아니다
`controller_id`는 프레임에 적힌 **자기 신고값**이고 wire authz는 존재하지 않는다. 다른 값을 적어 보내면 그대로 통과한다. 즉 `ForeignController` 거부는 보안이 아니라 실수 방지 장치다.
**따라서 이 게이트를 걷어내는 대가는 보안 약화가 아니다.** 진짜 접근 통제가 필요하면 별도 authz 계층의 문제이지 `controller_id` 문자열이 해결할 수 있는 사안이 아니었다.

### D-13. `plan_revision`이 로그 문자열로만 쓰인다
`MODEL_LOAD`가 나르지만 저장도 비교도 되지 않고, 어댑터의 `detail` 메시지에만 삽입된다. 외부 의도의 버전을 담을 자리가 이미 있는데 비어 있다.

## 7. 보완 설계 (초안)

### P-5. 4층 모델

| 층 | 소유 | 수명 | 개입 |
|---|---|---|---|
| 편성 의도 (하드웨어 제약, 배치 계획) | **외부 레코드** | 영구 | P4 밖 |
| 노드 슬롯 (id + adapter + 수용력) | 에이전트 | create ~ delete | 외부 → 에이전트 |
| 노드 실체 (구상 어댑터, 부분로딩된 레이어) | 어댑터 | load ~ unload | 외부 → 에이전트 → 어댑터 |
| 실행 | **컨트롤러** | 요청 단위 | 컨트롤러가 처음 등장하는 지점 |

노드 슬롯은 id와 수용력만 갖는 껍데기이고, 실체는 세 번째 층에서만 존재한다. 편성 제약은 첫 층에만 있고 P4는 그것을 나르지 않는다.

### P-6. lifecycle에서 `controller_id` 제거
`NODE_CREATE`, `MODEL_LOAD`, `MODEL_UNLOAD`, `HEALTH_CHECK`에서 제거한다. `EXECUTE`의 `controller_id`는 남되 의미가 **소유권 게이트에서 relay 대상 식별·추적으로** 바뀐다(Q-6).
연동: `TaskDirection`에 외부→에이전트 방향 신설, `allows_direction` 재작성, `authorization`에서 `ForeignController` 삭제(`UnknownNode`·`Dangling`·`BindingNotReady`는 유지), `NodeSlot.controller_id` 제거.

### P-7. `NODE_DELETE` / `NODE_DELETED` 신설
D-9의 해결. 활성 바인딩·실행이 있을 때의 처리는 Q-8.

### P-8. `plan_revision` / `runtime_generation` 두 축 분리
- `plan_revision` — **외부 의도의 버전.** 외부 레코드가 발급하고 에이전트가 기록·비교한다
- `runtime_generation` — **어댑터 실체의 세대.** 현행 역할 유지. 발급 주체도 어댑터 그대로

두 축은 갱신 주기와 발급자가 다르므로 하나로 합치지 않는다.

---

# 주제 C. 모델 적재 프로토콜

전제 4. 적재 지시는 외부에서 오고, 표현력은 구상 런타임 수준을 담아야 한다.

## 8. 현재 상태 (검증 완료)

### 8.1 계약이 정의한 것

[`docs/model-load.md`](docs/model-load.md)의 `stage_plan.load_options`가 정의하는 항목은 다음이 전부다.

`flash_attention`, `mmap`, `kv_cache.{type_k, type_v, offload}`, `batching.{strategy, max_sequences, node_limits[], context_batch_tokens, context_ubatch_tokens, calculation}`, `adapter_options`(자유 객체)

### 8.2 실제 처리

| 주체 | 동작 |
|---|---|
| P4 protocol | `stage_plan`을 해석하지 않는다. bounded text |
| Pipeline 어댑터 | `load_options.batching.max_sequences` **한 필드만** 읽는다 — [`capacity/mod.rs`](layers/adapters/adapter/src/domain/capacity/mod.rs) |
| Pipeline 어댑터 (적재) | `stage_plan` 전체를 호스트 supervisor `/api/runtime-groups`로 **그대로 POST**한다. 검증하지 않는다 — [`lifecycle/load/mod.rs`](layers/adapters/adapter/src/application/lifecycle/load/mod.rs) |
| stock llama.cpp 어댑터 | `load_options` 키가 **존재하면 거부**한다(`require_process_start_compatible`). 이미 기동된 프로세스를 가리키므로 |

### 8.3 upstream 실제 표면

고정된 [`apps/llama/upstream`](../llama/upstream) 기준 `common/arg.cpp`의 `add_opt` 호출은 **347개**다. 적재 시점에 의미가 있는 것만 추려도 다음 범주가 계약에 없다.

| 범주 | 대표 플래그 |
|---|---|
| 레이어/텐서 배치 | `--n-gpu-layers`, `--override-tensor`, `--n-cpu-moe`, `--cpu-moe`, `--tensor-split`, `--main-gpu`, `--device`, `--rpc` |
| KV/컨텍스트 | `--ctx-size`, `--parallel`, `--kv-unified`, `--ctx-checkpoints`, `--checkpoint-min-step`, `--defrag-thold`, `--swa-full`, `--context-shift`, `--cache-ram`, `--cache-reuse`, `--cache-idle-slots` |
| 메모리 로딩 | `--mlock`, `--direct-io`, `--no-repack`, `--check-tensors`, `--load-mode`, `--numa` |
| RoPE/어텐션 | `--rope-freq-base`, `--rope-freq-scale`, `--rope-scaling`, `--yarn-*`(5), `--grp-attn-n/w`, `--flash-attn`, `--attention` |
| 스레드/배치 | `--threads`, `--threads-batch`, `--batch-size`, `--ubatch-size`, `--cpu-mask`, `--cpu-range`, `--cpu-strict`, `--poll`, `--prio`, `--cont-batching` |
| 투기 디코딩 | `--spec-draft-model`, `--spec-draft-ngl`, `--spec-draft-n-max/min`, `--spec-draft-device`, `--spec-draft-type-k/v`, `--eagle3`, `--mtp` |
| 부가 아티팩트 | `--lora`, `--lora-scaled`, `--control-vector`, `--control-vector-layer-range`, `--mmproj`, `--mmproj-offload` |
| 메타/템플릿 | `--override-kv`, `--chat-template`, `--jinja`, `--reasoning-budget` |

`--override-tensor`가 없다는 점이 특히 문제다. 레이어 부분 로딩의 실제 도구인데 계약에 자리가 없다.

## 9. 결함

### D-14. 적재 옵션의 표현력이 런타임의 극히 일부만 덮는다
§8.1의 항목 수와 §8.3의 표면을 비교하면 자명하다. 특히 텐서 단위 배치(`--override-tensor`), MoE 분리(`--n-cpu-moe`), 디바이스 선택(`--device`, `--tensor-split`)이 빠져 있어 부분 로딩 전략을 프로토콜로 표현할 수 없다.
**해소 경로 (결정):** 전제 6에 따라 P4는 옵션 스키마를 소유하지 않는다. 불투명 문자열로 통과시키고 구상 어댑터가 해석한다. 이 결함은 프로토콜 결함에서 **어댑터 구현 범위**로 내려간다 — 표현력 확보는 어댑터가 upstream 표면을 얼마나 덮느냐의 문제가 된다.

### D-15. `MODEL_LOAD.model`이 단일 문자열이라 다중 아티팩트를 표현할 수 없다
draft model, mmproj, LoRA, control vector는 모두 **추가 아티팩트**다. 투기 디코딩은 두 번째 모델의 적재다.
**해소 경로 (결정):** 부가 아티팩트 경로는 옵션 문자열 안에 담고 어댑터가 해석한다. `model` 필드는 주 모델을 가리키는 식별자로 남는다. 따라서 wire 변경이 필요 없다(Q-11 철회).

### D-16. 문서가 기술한 검증 주체와 구현이 다르다
`docs/model-load.md`는 "선택된 어뎁터가 `load_options`의 공통 필드와 `adapter_options`를 검증·선별하여 적용한다"고 쓰여 있으나, Pipeline 어댑터는 `batching.max_sequences` 외에는 검증하지 않고 통째로 전달한다. "미지원 옵션을 조용히 무시하면 안 된다"는 규칙을 강제하는 코드가 P4 계층에 없다.

### D-17. 같은 필드가 어댑터에 따라 전부 무시 또는 전부 거부로 갈린다
Pipeline은 통과, stock llama.cpp는 존재만으로 `ERROR`. 호출자가 어느 쪽인지 알 방법이 프로토콜에 없다. `ADAPTER_REGISTER.descriptor`가 그 자리인데 지원 옵션 집합을 선언하지 않는다.

### D-18. `load_options`는 클러스터 계획인데 `MODEL_LOAD`는 노드 단위다
`batching.node_limits[]`가 **다른 노드들의 값까지** 담는다. 그래서 "최상위 `max_sequences`는 모든 `node_limits`의 최솟값"이라는 fallback 규칙이 필요해졌다. 클러스터 전역 계획이 노드마다 중복 전송되고, 각 어댑터가 자기 몫을 골라내야 한다.

### D-19. 호스트 API에 `controller_id`라는 이름으로 `deployment_id`를 보낸다
[`lifecycle/load/mod.rs`](layers/adapters/adapter/src/application/lifecycle/load/mod.rs)의 `start.insert("controller_id", deployment)`. 필드명과 내용이 불일치하고, 전제 3·4의 컨트롤러 분리와 충돌하는 잔재다.

### D-60. 적재 시점 선언이 런타임 파생 값을 고정한다

`capacity::declare()`는 `MODEL_LOAD` 시점에 `stage_plan.load_options.batching.max_sequences`를 읽어 **deployment별 세마포어 게이트로 설치**한다 — [`capacity/mod.rs`](layers/adapters/adapter/src/domain/capacity/mod.rs). 적재 시점의 상수가 실행 시점의 admission을 지배한다.

병행 중인 TPS 세션(Mac+GB10, MI250)에서 스케줄러가 **폭을 활성 코호트와 ubatch 목표에서 파생**하고 남는 만큼 depth로 쓰는 방향이 검토되고 있다. 그 방향이 확정되면 `max_sequences`는 **런타임이 매 순간 계산하는 값을 적재 시점에 못 박아 둔 것**이 된다.

같은 파일의 주석이 경고하는 실패가 다른 층위에서 반복된다 — 프로세스 전역 상수를 게이트로 두었을 때 "스로틀이 GPU에서 세 단계 위에" 있었던 것과, 적재 시점 선언을 게이트로 두는 것은 같은 형태다.

`D-18`의 `node_limits[]`·`calculation` 기계 장치 전체가 이 정적 수치를 계산하기 위해 존재한다는 점도 함께 본다.

**이 항목은 타 세션의 스케줄러 결론에 종속된다(Q-48).** 여기서 단독으로 확정하지 않는다.

### D-61. 연결 수립 정책이 프로토콜에 없다

소스 라우팅(P-22)과 자기기술 주소(P-34)는 **홉마다 연결이 성립함**을 전제한다. 그러나 재시도·백오프·연결 예산에 대한 규정이 P4에 없다.

병행 세션에서 링 형성 중 `connect()`가 `EHOSTUNREACH`를 반환하는 현상과 연결 예산·백오프 작업이 진행 중이다. 체인이 프로토콜로 올라오면 이 정책도 계약의 일부가 되어야 한다 — 어느 홉에서 몇 번 재시도하고, 실패를 언제 `ERROR`로 종결하는가.

### D-20. 적재 보고가 요청 route에 묶여 있어 재접속 경로가 없다
`LOAD_PROGRESS`·`DRAFT_REPORT`는 non-terminal, `MODEL_BOUND`는 terminal이며 모두 요청이 들어온 `route_id`로만 나간다. **요청자가 끊기면 진행 상황도 완료 사실도 어디에도 전달되지 않는다.** 적재는 수 분 단위 작업이므로 실질적 위험이고, 전제 5(적재 보고는 요청한 외부로)가 성립해도 이 구멍은 남는다.

전제 1의 외부 레코드 관점에서는 더 나쁘다 — 완료를 놓치면 외부 기록과 실제 적재 상태가 갈라지고, 이를 복구할 조회 수단이 없다(D-6과 같은 뿌리).

## 10. 보완 설계 (초안)

### P-9. 옵션은 불투명 문자열로 통과시킨다 (결정)

전제 6. P4는 적재 옵션의 구조를 소유하지 않는다. `MODEL_LOAD`는 옵션을 bounded text로 나르고, 해석은 전적으로 구상 어댑터가 한다.

근거: upstream `add_opt`가 347개이고 계속 늘어난다. 스키마로 박으면 upstream이 움직일 때마다 P4 계약이 깨진다. 부가 아티팩트(draft·mmproj·LoRA·control vector) 경로도 이 문자열 안에 담기므로 wire 필드 승격이 불필요하다.

**따라서 폐기되는 설계:** 옵션의 3부 분리(artifacts/placement/tuning)를 P4 계약으로 규정하려던 초안. 그 구분은 유의미하지만 **어댑터와 외부 계획기가 공유하는 규약**이지 P4의 관심사가 아니다.

### P-10. 잔여 문제 — 사전 발견 수단 (축소)

P-9로 D-14~D-17의 대부분이 해소된다. 어댑터가 해석하므로 미지원 옵션에 대해 `ERROR`를 낼 수 있고, "무시 대 거부" 분기도 어댑터 구현 규칙으로 내려간다.

**남는 것 하나:** 외부 계획기가 **보내기 전에** 그 어댑터가 무엇을 지원하는지 알 방법이 없다. 현재는 보내보고 `ERROR`를 받는 시행착오뿐이다. 적재는 비싼 작업이라 실패 비용이 크다.

선택지는 두 가지다.
- (a) `ADAPTER_REGISTER.descriptor`에 지원 옵션 키 집합과 스키마 버전을 **선언만** 한다. P4는 여전히 해석하지 않고 외부에 전달만 한다
- (b) 시행착오를 수용한다. `ERROR` detail에 미지원 키를 명시하는 것으로 충분하다고 본다

(a)는 P-9와 충돌하지 않는다 — 선언은 어댑터가 만든 문자열이고 P4는 나르기만 한다. Q-9로 판단한다.

### P-13. 적재 보고의 방향 전환과 재접속 (전제 5)

**방향 전환은 재라벨링에 가깝다.** 현재도 `loadModel`을 호출한 외부 클라이언트가 같은 route로 `LOAD_PROGRESS` → `DRAFT_REPORT` → `MODEL_BOUND`를 받는다([`controller-instance.mjs`](tools/controller/client/controller-instance.mjs)). 바꿀 것은 `allows_direction`이 이들을 `NodeController`로 규정한 부분이며, P-6의 방향 재정의와 같은 변경에 포함된다.

**실질 작업은 D-20이다.** 요청 route가 끊겼을 때 진행·완료를 되찾을 수단이 필요하다. 선택지:
- (a) 진행 중인 적재를 조회하는 요청 신설 — `operation_id`로 현재 상태를 되묻는다
- (b) 적재 상태를 노드 상태 조회에 포함 — 별도 메시지 없이 D-6의 조회 수단에 얹는다
- (c) agent-initiated announce(P-4)에 적재 완료를 실어 보낸다 — Q-3 채택이 전제

Q-14로 판단한다. (b)가 새 메시지를 늘리지 않아 유력하나, 진행률의 실시간성은 포기하게 된다.

### P-44. `batching.*`를 권위 게이트에서 상한으로 강등 (조건부)

D-60의 해소안이다. **타 세션의 스케줄러 결론이 나온 뒤에 확정한다.**

| 안 | 내용 |
|---|---|
| 유지 | 현행. 적재 시점 선언이 admission 게이트 |
| **강등** | 선언은 **상한 힌트**로만 쓰고, 실제 폭은 런타임이 활성 코호트에서 파생 |
| 제거 | `batching.*`를 계약에서 빼고 전적으로 런타임 소유 |

강등안이 §92의 원칙과 가장 잘 맞는다 — P4는 처리량을 소유하지 않고 **손잡이를 넘겨줄 뿐**이다. 상한은 안전장치로서 의미가 있으나 매 순간의 폭은 GPU에 가장 가까운 층이 정해야 한다.

강등·제거 어느 쪽이든 `D-18`의 `node_limits[]`·`calculation` 구조가 함께 정리된다.

### P-11. 클러스터 계획과 노드 지시의 분리
`MODEL_LOAD`는 **그 노드가 할 일만** 싣는다. 클러스터 전역 계획은 외부 레코드에 남고, 필요한 교차 정보(전체 스테이지 수, 이웃 스테이지 식별자 등)만 명시 필드로 전달한다. D-18의 fallback 규칙과 노드별 중복 전송을 제거한다.

### P-12. 적재 지시의 방향 전환
전제 4에 따라 `MODEL_LOAD`/`MODEL_UNLOAD`를 외부→에이전트 방향으로 옮긴다(P-6과 동일 변경). D-19의 `controller_id` 잔재도 이때 제거한다.

---

# 주제 D. 적재·해제의 사전 조건과 노드 상태 기계

전제 7·8. 명령은 노드의 사전 상태에 의존하고, 실패는 메시지로 요청자를 호출한다.

## 11. 현재 상태 (검증 완료)

### 11.1 사전 조건 검사가 이미 있는 곳

`MODEL_LOAD`·`MODEL_UNLOAD`는 `exclusive()`를 통과한다 — `admission::lifecycle(slot)`이 `try_acquire_many_owned(max_inflight)`로 **슬롯의 모든 permit**을 비차단 획득한다. 실행이 하나라도 진행 중이면 즉시 실패하고 `ERROR("node … admission is full")`을 emit한다.

따라서 **"인퍼런스 참가 중이면 로드·언로드 실패"는 이미 구현되어 있다.** 전제 7 중 이 부분만 충족된다.

### 11.2 응답 전달 구조

[`forward/mod.rs`](layers/runtime/src/domain/agent/lifecycle/forward/mod.rs)의 `capture`는 어댑터의 모든 응답을 **먼저 호출자에게 그대로 흘려보내고**, 그중 terminal을 복제해 보관한다. 에이전트는 그 복제본을 보고 레지스트리 반영 여부를 뒤늦게 결정한다.

전제 8과 정면으로 충돌하는 구조다. 흘려보낸 시점에 이미 요청자가 호출되었다.

## 12. 결함

### D-21. 이미 해제된 대상의 해제가 성공으로 보고된다
[`lifecycle/unload/mod.rs`](layers/adapters/adapter/src/application/lifecycle/unload/mod.rs)가 `DELETE /api/runtime-groups/{deployment}`의 **HTTP 404를 성공으로 처리**하고 `MODEL_UNBOUND`를 emit한다. 전제 7("이미 언로드된 상태면 실패")과 어긋난다.

### D-22. 한 route에 terminal이 두 번 나갈 수 있다
D-21에 이어, 에이전트는 `MODEL_UNBOUND`를 받은 뒤 `slot.unbind()`를 호출하고 `UnknownBinding`이면 `refuse()`로 `ERROR`를 emit한다. `capture`가 이미 `MODEL_UNBOUND`를 흘려보낸 뒤이므로 **요청자는 `MODEL_UNBOUND` 다음에 `ERROR`를 받는다.**

terminal은 route를 닫는다는 계약 위반이다. 클라이언트 쪽에서는 `AgentLink`가 첫 terminal에서 route를 지우므로 뒤따르는 `ERROR`는 **조용히 버려진다** — 실패가 성공으로 관측된다.

### D-23. 이미 적재된 노드에 대한 적재가 조용히 덮어쓴다
`NodeSlot::bind()`는 `bindings.insert()`다. 사전 조건 검사가 없다. 전제 7("이미 모델이 로딩된 노드면 로드 실패")과 어긋나며, 기존 바인딩이 경고 없이 교체된다.

### D-24. 노드:바인딩이 1:N이라 "노드의 실체"가 단수로 정의되지 않는다
`NodeSlot.bindings`는 `HashMap<binding_id, Binding>`이다. 한 노드가 여러 바인딩을 동시에 보유할 수 있다.

전제 3·7의 모델("노드의 실체 = 적재된 구상 어댑터 객체", 해제하면 실체 없음)은 **0 또는 1**을 전제한다. 이 불일치가 해소되지 않으면 "이미 적재된 노드"라는 판정 자체가 성립하지 않는다. D-23의 선행 문제다.

### D-25. 해제 단위가 계층마다 다르다
P4의 `MODEL_UNLOAD`는 `binding_id` 단위인데, 어댑터는 `deployment_id` 단위로 `DELETE`한다. 같은 deployment에 여러 binding이 있으면 **하나를 해제하면서 그룹 전체를 지운다.** 전제 7의 "실체 완전 해지"가 의도한 범위보다 넓게 작동할 수 있다.

## 13. 보완 설계 (초안)

### P-14. 사전 조건은 emit 이전에 완결한다 (전제 8)

CPS에서 emit은 요청자 호출이므로 되돌릴 수 없다. 따라서:

1. 모든 사전 조건은 **어떤 메시지도 emit하기 전에** 검사한다
2. 검사를 통과하면 그 명령의 결과 메시지는 하나뿐이다 — 성공 terminal 또는 실패 terminal
3. **어댑터 응답을 흘려보낸 뒤 에이전트가 판단을 뒤집는 구조를 금지한다**

`forward::capture`의 재설계가 필요하다. 어댑터의 terminal은 에이전트가 판단을 마친 뒤에만 요청자에게 전달되거나, 에이전트가 자신의 terminal로 대체해 emit해야 한다. D-22의 근본 해결이다.

### P-15. 노드 상태 기계 명시

```text
(없음) ──NODE_CREATE──▶ empty ──MODEL_LOAD──▶ bound ──EXECUTE──▶ active
                          ▲                     │                  │
                          └────MODEL_UNLOAD─────┘◀─────완료────────┘
   empty ──NODE_DELETE──▶ (없음)
```

| 명령 | 허용 사전 상태 | 그 외 |
|---|---|---|
| `MODEL_LOAD` | `empty` | 실패 (`bound`는 이미 적재, `active`는 실행 중) |
| `MODEL_UNLOAD` | `bound` | 실패 (`empty`는 이미 해제, `active`는 실행 중) |
| `NODE_DELETE` | `empty` | Q-8 |
| `EXECUTE` | `bound` | 실패 |

교체는 단일 명령이 아니다. `MODEL_UNLOAD` → `MODEL_LOAD` 2단계로만 가능하다.
`active` 차단은 이미 `admission::lifecycle`로 구현되어 있다(§11.1). 새로 필요한 것은 `empty`/`bound` 판정이다.

### P-16. 노드:바인딩을 1:1로 좁힌다 (Q-15 종속)
P-15의 상태 기계는 노드가 최대 하나의 실체를 갖는다는 전제 위에서만 정의된다. `NodeSlot.bindings`를 `Option<Binding>`으로 좁히면 D-23·D-24가 함께 풀리고, D-25의 단위 불일치도 "노드 하나 = 실체 하나"로 정렬된다.

여러 모델을 한 노드에 올리고 싶다면 노드를 여러 개 만드는 것이 전제 3·5의 모델과 정합하다.

---

# 주제 E. CPS 전면 감사

전제 9. 반환값 기반 처리 경로를 전수 조사한 결과다.

## 14. 현재 상태 (검증 완료)

처리 경로는 두 갈래로 갈라져 있고, 한쪽만 CPS다.

### 14.1 CPS를 지키는 경로

`INGRESS_SUBMIT`, `EXECUTE`, `CANCEL`. [`dispatch/mod.rs`](layers/runtime/src/application/dispatch/mod.rs)가 후속 Task를 큐에 넣고 즉시 반환하며, 원격 응답은 `QueueResponseSink`가 `enqueue`로 큐에 재진입시킨다. `causation_id` 사슬이 이어진다.

### 14.2 반환값 기반 경로

`NODE_CREATE`, `MODEL_LOAD`, `MODEL_UNLOAD`, `HEALTH_CHECK`, `INVENTORY_QUERY`, `ADAPTER_REGISTER` — **제어 평면 전체**다. `dispatch::compatibility`가 이 경로이며, 함수 이름이 이미 성격을 인정하고 있다.

토대가 되는 세 계약이 전부 동기 완료형이다 — [`foundation/transport/mod.rs`](layers/runtime/src/foundation/transport/mod.rs).

```rust
trait P4Handler   { fn handle(&self, message, responses) -> Result<()>; }
trait P4Transport { fn dispatch(&self, message, responses) -> Result<()>; }
trait ResponseSink{ fn emit(&mut self, message) -> Result<()>; }
```

## 15. 결함

### D-26. `P4Handler`/`P4Transport`가 동기 완료 계약이다
두 trait 모두 "돌아왔으면 끝났다"를 뜻한다. 처리 중간에 큐로 빠져나갈 자리가 시그니처에 없다. 전제 9의 위반이 개별 구현이 아니라 **토대 계약에 박혀 있다.**

### D-27. `TcpTransport::dispatch`가 블로킹 RPC다
호출마다 새 `TcpStream`을 연결하고, terminal이 올 때까지 `read_message` 루프를 돈다. 지속 소켓 다중화(`peer_mux`)를 쓰지 않는다. CPS 이전에 자원 사용 측면에서도 낭비다.

### D-28. `forward::capture`가 응답을 반환값으로 돌려준다
전제 8·9의 정면 위반이며 주제 D의 `D-22`를 낳은 직접 원인이다. 모든 응답을 호출자에게 흘려보낸 뒤 terminal을 복제해 **반환**하고, 에이전트가 그 반환값으로 분기한다.

### D-29. lifecycle 여섯 핸들러가 전부 반환값으로 분기한다
`create_node`는 `NodeCreated{state=="ready"}`인지 보고 슬롯을 기록하고, `load_model`은 `ModelBound{state=="ready"}`를 보고 `bind()`하며, `unload_model`은 `ModelUnbound`를 보고 `unbind()`한다. 전부 `capture`의 반환값 검사다.

### D-30. 장기 작업이 하나의 블로킹 호출 안에 갇힌다
어댑터의 적재는 `http::json` 동기 호출이고, **600초 데드라인 폴링 루프**를 핸들러 안에서 돈다 — [`lifecycle/load/mod.rs`](layers/adapters/adapter/src/application/lifecycle/load/mod.rs). 수 분짜리 작업이 Task로 쪼개지지 않으므로 진행 상태가 큐에 나타나지 않는다.

`compatibility`가 `spawn_blocking`으로 넘기므로 **큐 워커 자체는 막히지 않는다.** 그러나 이는 블로킹 풀로 밀어낸 것이지 CPS로 만든 것이 아니며, 아래 셋이 그 대가다.

### D-31. `deadline_unix_ms`가 제어 평면에 강제되지 않는다
데드라인 검사는 admission([`agent_host/mod.rs`](layers/runtime/src/application/agent_host/mod.rs))과 `peer_mux`의 execute 경로에만 있다. 큐 워커에도, `compatibility` 경로에도 없다. 데드라인 직전에 승인된 lifecycle 작업은 **무제한으로 실행된다.**

### D-32. `CANCEL`이 제어 평면에 도달하지 못한다
`dispatch::cancel`은 `active` 맵의 relay만 `abort()`한다. 그 맵은 execute 경로만 등록한다. `spawn_blocking`으로 넘어간 lifecycle 작업은 **취소 핸들 자체가 없다.** 즉 진행 중인 모델 적재는 중단할 수 없다.

### D-33. causation 사슬이 블로킹 구간에서 끊긴다
하나의 `dispatch` 호출 안에서 일어나는 일은 Task가 아니므로 `task_id`/`causation_id`가 생기지 않는다. 적재의 어느 단계에서 멈췄는지 큐에서 관측할 수 없고, 재개 지점도 정의되지 않는다. 주제 C의 `D-20`(route 단절 시 복구 불가)과 같은 뿌리다.

## 16. 보완 설계 (초안)

### P-17. 출력 경로를 큐 하나로 통일한다
`ResponseSink`를 유일한 출력구로 삼고, 처리 함수에서 **결과를 뜻하는 반환값을 없앤다.** 반환은 "큐에 넣었다"는 수용 여부까지만 의미한다. `forward::capture`는 폐기하고, 어댑터 응답은 후속 Task로 재진입시킨다. 이미 `QueueResponseSink`가 그 형태이므로 일반화하는 작업이다.

### P-18. 어댑터 경계를 지속 소켓 다중화로 교체
`TcpTransport`를 `peer_mux`로 대체한다. D-27 해소이자 P-17의 전제 — 응답이 나중에 도착하려면 소켓이 호출과 분리되어야 한다.

### P-19. 장기 작업을 다단 Task로 분해
적재를 `시작 요청 → 진행 관측 → 완료 판정` 단계로 쪼개고, 각 단계가 다음 단계를 큐에 넣는다. 진행 폴링은 자기 자신을 재-enqueue하는 Task가 된다. D-30·D-33이 함께 풀리고, 주제 C의 `D-20` 복구 경로도 여기서 나온다.

### P-20. `deadline`과 `CANCEL`을 전 경로에 적용
Task 단위로 쪼개지면 각 단계 진입 시 데드라인을 검사할 수 있고, `CANCEL`은 다음 단계의 enqueue를 막는 방식으로 도달한다. D-31·D-32 해소.

**의존 관계:** P-17 ← P-18 ← P-19 ← P-20 순으로 쌓인다. 토대 계약(D-26)을 먼저 바꾸지 않으면 어느 것도 성립하지 않는다.

---

# 주제 F. 인퍼런스 경로와 노드 체인

§1.2의 인퍼런스 경로를 코드와 대조한 결과다.

## 17. 현재 상태 (검증 완료)

### 17.1 이미 대상 구조와 맞는 것

**스트림 단일 모드.** P4에는 스트림 여부 스위치가 없다. 인퍼런스 결과는 `TOKEN*` → `DONE`이 유일한 형태이고, 어댑터도 SSE 델타를 그대로 흘린다. "언제나 스트림"은 이미 성립하며 **유지해야 할 성질**이지 고칠 대상이 아니다.

### 17.2 어긋나는 것

`INGRESS_SUBMIT`은 `node_id` **단수**를 나른다. 체인을 표현할 자리가 없다.

현재 체인은 P4 밖에 있다. Pipeline 런타임의 deployment(`/api/runtime-groups`) 설정이 스테이지 구성을 소유하고, 스테이지 간 교환은 `linker-pipeline-inference-stream-v1`로 이뤄진다. 컨트롤러는 체인을 관리하지 않는다 — `ControllerProcessor`는 ingress를 단일 `EXECUTE`로 바꿔 한 노드에 넘길 뿐이다.

## 18. 결함

### D-34. 인퍼런스 요청이 노드 리스트를 표현할 수 없다
`INGRESS_SUBMIT.node_id`가 단수다. "OUTER가 컨트롤러에게 노드 리스트를 전달한다"는 대상 구조를 현 wire로는 표현할 수 없다.

### D-35. 체인 관리 주체가 P4 밖에 있다
스테이지 구성이 Pipeline 런타임의 deployment 설정에 박혀 있다. 대상 구조는 컨트롤러가 **요청 시점에** 체인을 편성하는 것이므로, 체인이 적재 시점의 런타임 설정에 고정되어 있으면 성립하지 않는다.

동시에 이는 전제 3과도 충돌한다 — 체인은 편성 의도(OUTER 소유)인데 지금은 노드 실체(어댑터 소유) 안에 들어 있다.

### D-36. "체인의 마지막"이라는 개념이 P4에 없다
`TOKEN`/`DONE`은 어댑터 → 에이전트 → route 소유자로 갈 뿐이다. 어느 노드가 마지막이며 그 결과가 컨트롤러로 귀환해야 하는지를 표현하는 필드가 없다.

### D-38. `EXECUTE`도 체인을 나를 수 없고, 노드 간 주소 지정이 정적 설정에 묶여 있다
`ExecutionRequest`의 `node_id`도 단수다. 소스 라우팅을 하려면 메시지가 체인 전체를 날라야 하는데 자리가 없다.

주소 지정은 더 근본적이다. 현재 노드 간 도달 수단은 `RouteProcessor`의 `HashMap<node_id, SharedTransport>`뿐이고, 이는 **기동 시 설정으로 주입되는 정적 맵**이다 — [`routing/processor/mod.rs`](layers/runtime/src/application/routing/processor/mod.rs). 요청마다 달라지는 체인을 정적 맵으로 따라갈 수 없다.

### D-39. 체인 중간 실패의 보고 경로가 없다
소스 라우팅에서 홉은 전진만 한다. `Node[2]`가 실패하면 그 사실을 컨트롤러에 알릴 역방향 간선이 없다. 현재 구조는 홉이 하나뿐이라 이 문제가 드러나지 않았다.

### D-40. `CANCEL`이 체인 전체에 도달할 수 없다
`dispatch::cancel`은 자기 `route_id`의 active relay만 중단한다. 체인의 나머지 노드는 취소 사실을 모른 채 계속 전진한다. 주제 E의 `D-32`(제어 평면 미도달)와는 다른 축의 결손이다.

### D-37. `NodeNode` 방향의 분류가 잘못되어 있었다 (정정)
이 문서는 `NodeNode`를 발행 코드가 없다는 이유로 "죽은 표면 — 구현할지 삭제할지 판단"으로 분류했다. **대상 구조에서는 필수 방향이다.** 노드 간 hidden state 전달이 인퍼런스 경로의 본체이므로 삭제 후보가 아니라 구현 대상이다. §94.3을 이에 맞게 정정했다.

## 19. 보완 설계 (초안)

### P-21. 인퍼런스 요청이 순서 있는 노드 리스트를 나른다
`INGRESS_SUBMIT`의 단일 `node_id`를 순서 있는 노드 지정으로 교체한다. 각 항목은 `(agent, node_id, binding, runtime_generation)`을 지정해야 실행 가능한 실체를 가리킨다(§1.3의 4-튜플 규칙).

컨트롤러는 이 리스트를 받아 체인을 편성하되 각 노드의 구상 상태는 조회하지 않는다. 유효성은 OUTER가 편성 시점에 보장한다.

### P-22. 체인을 메시지에 실어 소스 라우팅한다 (결정)

컨트롤러가 `Node[0]`에게 보내는 프리필 `EXECUTE`가 **체인 전체를 담는다.** 각 노드는 그 메시지에서 자신의 위치와 다음 대상을 읽어 스스로 전달한다. 컨트롤러는 홉마다 개입하지 않는다.

hidden state 자체는 계속 native 데이터 평면이 옮긴다(주제 §6.2의 구분 유지). P4가 나르는 것은 **체인·순서·correlation**이며 텐서는 프레임에 들어가지 않는다.

D-35 해소. `NodeNode` 방향이 여기서 살아난다.

### P-25. 체인 항목은 자기 완결적 주소여야 한다
정적 라우트 맵(D-38)으로는 요청마다 달라지는 체인을 따라갈 수 없다. 체인 항목은 그 자체로 도달과 실행이 가능해야 한다.

```text
chain[i] = (agent 도달 주소, node_id, binding_id, runtime_generation)
```

`binding_id`/`runtime_generation`이 없으면 홉 도착지에서 stale 실체에 실행될 수 있다(§1.3의 4-튜플 규칙과 같은 이유). 전제 2와도 정합한다 — 인자로 전달될 상태를 메시지가 들고 다닌다.

### P-26. 귀환 주소를 메시지에 싣는다 (전제 11로 대상 변경)
중간 노드의 실패 보고(D-39)와 마지막 노드의 결과 반환에 귀환 주소가 필요하다는 골자는 유지된다. 다만 **귀환 대상이 컨트롤러가 아니다.**

당초 "컨트롤러 귀환 주소"로 적었으나 전제 11에서 내부망 노드는 컨트롤러에 직접 도달할 수 없다. 귀환 주소는 **진입 에이전트**이며, 진입 에이전트가 컨트롤러로 넘긴다. 보고 노드가 이미 진입 에이전트 소속이면 한 홉을 건너뛴다(P-36).

이로써 `TOKEN`/`DONE`/`ERROR`는 체인을 거슬러 오르지 않고 **진입 에이전트로 직접** 간다. 역방향 전파는 여전히 불필요하다.

### P-27. `CANCEL`을 체인 전파형으로 정의한다
취소는 체인 전체에 도달해야 한다(D-40). 체인이 메시지에 있으므로 취소도 같은 경로를 따라 전진 전파하거나, 각 노드가 correlation 단위로 자체 중단하도록 규정한다. Q-27.

### P-23. 마지막 노드의 귀환 경로를 명시한다
체인의 마지막 노드는 생성 토큰을 **컨트롤러에게** 보낸다. 컨트롤러는 현재 그대로 OUTER로 통과시키되, 이후 필터링·부가 작업이 들어갈 자리를 계약상 확보한다.

따라서 `TOKEN`/`DONE`의 방향 규칙은 `NodeController` → `ExternalController` 2단으로 유지되며, 이 부분은 현행과 같다.

### P-24. 스트림 단일 모드를 계약으로 못 박는다
현재 사실상 그렇게 동작하지만 명문 규칙이 없다. `options` 문자열에 백엔드가 비스트림 스위치를 받아들이면 계약이 조용히 깨질 수 있으므로, 어댑터가 이를 거부하도록 규정한다.

---

# 주제 G. 스테이지 보고와 디코드 루프

프리필은 계산 단계이자 **상태 확인 단계**다. 각 노드는 진입과 완료를 보고할 책임을 진다.

## 20. 현재 상태 (검증 완료)

인퍼런스 중 노드가 내보내는 메시지는 `TOKEN`(이벤트)과 `DONE`(terminal) **둘뿐**이다. 수락 보고도 완료 보고도 없다.

사전 조건 검사는 `binding_is_ready(binding_id, deployment_id, generation)` 하나다 — 바인딩이 존재하고 세대가 일치하는지만 본다.

`Binding`이 보유한 것은 `deployment_id`와 `generation`뿐이다 — [`domain/state/mod.rs`](layers/adapters/adapter/src/domain/state/mod.rs), [`registry/node/mod.rs`](layers/runtime/src/domain/agent/registry/node/mod.rs). **레이어 범위도 컨텍스트 크기도 어디에도 기록되지 않는다.**

통계는 `DRAFT_REPORT`가 유일한데 이는 적재 시점 메모리 실측(`model_bytes`/`kv_bytes`/`layer_bytes`/`ffn_bytes`)이다. 인퍼런스 통계를 나르는 메시지는 없다.

## 21. 결함

### D-41. 스테이지 수락·완료 보고 메시지가 없다
"이 프리필을 받았고 처리할 수 있다", "무사히 마쳤다"를 표현할 kind가 없다. 홉이 하나뿐인 현 구조에서는 `DONE` 하나로 갈음되었으나, 체인에서는 **노드마다 두 시점**이 필요하다.

### D-42. 인퍼런스 통계를 나를 자리가 없다
처리 시간, 처리량, 생성된 hidden state 시퀀스 크기 등을 담을 메시지가 없다. OUTER의 모니터링은 이 정보 위에서만 성립한다.

### ~~D-43. 노드의 레이어 범위가 어디에도 없다~~ (철회)
`Binding`에도 `NodeSlot`에도 P4 메시지에도 레이어 구간이 없다는 관찰 자체는 사실이다. 그러나 **이는 결함이 아니라 의도된 추상화다.**

노드는 자신의 적재 상태를 최대한 모르는 상태로 유지한다(전제 10). 따라서 진입 검사에서 레이어 번호에 관한 판정은 배제한다. 체인 구간의 연속성·시작·종단은 그 배치를 결정한 OUTER가 편성 시점에 보장한다 — P-5의 "편성 제약은 첫 층에만 있고 P4는 그것을 나르지 않는다"와 일치한다.

### D-44. 컨텍스트 초과 판정의 통로가 없다 (축소)
당초 "바인딩의 컨텍스트 크기가 기록되지 않는다"를 결함으로 적었으나, 전제 10에 따라 **P4가 기록할 메타가 아니다.** 컨텍스트 크기는 실체의 속성이므로 어댑터가 자기 런타임에서 이미 안다.

남는 결손은 그 판정 **결과를 알릴 통로**뿐이고, 이는 D-41(진입 보고 부재)에 포함된다. 별도 항목으로 다루지 않는다.

### D-45. 디코드 루프의 순환을 표현할 수 없다
마지막 노드가 1번 노드에게 다음 토큰 생성을 의뢰하려면 체인이 **링**이어야 한다. 현 `EXECUTE`는 단일 대상만 가리키고, 주제 F의 소스 라우팅 체인도 선형 전진만 상정했다.

### D-46. `phase=DECODE`의 처분이 확정되었다 (정정)
이 문서는 `phase=DECODE`를 "생성 코드가 없으니 삭제 판단 대상"으로 두고 P-22 이후로 미뤘다. **디코드 루프가 노드 주도로 순환하는 구조에서는 필수다.** 프리필 홉과 디코드 홉은 페이로드도 순환 형태도 다르므로 구분이 필요하다. §94.3을 정정했다.

## 22. 보완 설계 (초안)

### P-28. 스테이지 진입·완료 보고 신설
노드마다 두 시점을 보고한다.

| 시점 | 뜻 | 실패 시 |
|---|---|---|
| **진입** | 메시지를 수령했고 처리 가능한 상태다 | 사전 조건 위반 → `ERROR`, 체인 전진 중단 |
| **완료** | 자기 구간을 마쳤고 다음으로 넘겼다 | — |

진입 시 검사할 사전 조건은 **노드가 자기 적재 상태를 알지 않고도 판정할 수 있는 것**으로 한정한다(전제 10).

| 검사 | 판정 주체 |
|---|---|
| 지정된 바인딩이 존재하고 세대가 일치하는가 | 에이전트 (`binding_is_ready`) |
| 모델이 적재되어 있는가 (`bound` 상태인가) | 에이전트 (P-15 상태 기계) |
| 프롬프트가 컨텍스트를 넘지 않는가 | 어댑터 — 자기 런타임의 속성이므로 조회 없이 안다 |
| ~~레이어 구간이 자기 차례와 맞는가~~ | **배제.** OUTER가 편성 시점에 보장한다 (D-43) |

전제 8에 따라 **진입 보고는 어떤 계산도 시작하기 전에** 나가야 하고, 실패는 계산 대신 `ERROR`로 종결한다.

### P-29. 인퍼런스 통계 스키마
완료 보고가 나르는 항목: 처리 시간, 처리량, 생성된 hidden state 시퀀스 크기, 노드·바인딩 식별자, 구간 위치. 전제 6과 같은 이유로 세부 확장은 문자열에 담되, 모니터링에 필요한 최소 집합은 명시 필드로 둔다(Q-29).

컨트롤러는 이를 해석하지 않고 OUTER로 통과시킨다 — §1.2의 컨트롤러 역할과 일치한다.

### ~~P-30. 레이어 구간을 바인딩 메타로 노출~~ (철회)
`MODEL_BOUND`가 레이어 구간을 보고하고 에이전트가 `Binding`에 기록하자는 제안이었다. **전제 10과 P-5에 반한다.** 노드·에이전트가 적재 구조를 알게 되고, 편성 제약이 첫 층에만 있다는 원칙이 깨진다. P-33으로 대체한다.

### P-33. 노드는 자신의 적재 구조를 모른다 (전제 10)

노드와 에이전트가 아는 것은 **바인딩이 있다/없다와 그 세대**까지다. 어떤 레이어를 맡았는지, 몇 번부터 몇 번까지인지는 알지 않는다.

| 관심사 | 소유 |
|---|---|
| 어떤 노드가 어떤 레이어 구간을 맡는가 | **OUTER** (편성 의도, 4층 모델의 첫 층) |
| 체인 구간의 연속성·시작·종단 유효성 | **OUTER** — 편성 시점에 보장 |
| 그 구간이 실제로 적재되었는가 | 어댑터 — 실패하면 적재 자체가 실패한다 |
| 실행 시점에 이 바인딩이 유효한가 | 에이전트 — 존재와 세대만 본다 |

이로써 P4는 배치 구조를 나르지 않아도 되고, 노드는 교체 가능한 부품으로 남는다. 검증을 OUTER 단독에 맡기는 대가는 **잘못 편성된 체인이 실행 중에야 드러난다**는 것이다(Q-33).

적재 옵션이 불투명 문자열인 것(전제 6)과 같은 방향이다 — P4는 지시도 구조도 해석하지 않는다.

### P-31. 체인을 링으로 정의하고 `phase`로 홉을 구분한다
- `phase=PREFILL` — 선형 전진. `chain[i] → chain[i+1]`
- `phase=DECODE` — 순환. `chain[n] → chain[0]`, 종료 조건 충족 시 루프 이탈

마지막 노드는 두 책임을 동시에 진다 — 컨트롤러에게 토큰 또는 생성 종료를 보고하고, 1번 노드에게 다음 토큰 생성을 의뢰한다.

### P-32. KV는 요청 수명 동안 노드에 귀속된다 (외재화 예외)
디코드 루프가 성립하려면 각 노드가 그 요청의 KV를 홉 사이에 보유해야 한다. 이는 주제 A의 occupancy와 같은 성격 — **프로세스에 관한 사실이지 레코드가 아니다.**

전제 2의 예외로 명문화한다. 외재화 대상은 편성 의도와 체인이며, KV는 요청 수명에 묶인 점유 자원이다. 따라서 체인 중간 노드의 장애는 그 요청의 재시작을 뜻하지 이전(migration)이 아니다.

---

# 주제 H. 네트워크 위상과 경유

전제 11. 방화벽이 구조를 정한다.

## 23. 현재 상태 (검증 완료)

**현 구현은 평면 도달성을 가정한다.**

- `ControllerInstance`는 `endpoint`를 받아 그 에이전트에 **직접 TCP 연결**한다 — [`agent-link.mjs`](tools/controller/client/transport/agent-link.mjs). OUTER가 각 에이전트에 개별 접속하는 모델이다
- `peer_mux`는 에이전트 → 에이전트 endpoint 직결이다
- `RouteProcessor`는 `HashMap<node_id, SharedTransport>` 정적 맵으로 대상을 고른다
- `TcpTransport::dispatch`는 호출마다 목적지에 새 소켓을 연다

즉 **모든 참여자가 서로 도달 가능하다는 전제 위에 서 있다.** 방화벽으로 나뉜 배치를 표현할 수단이 하나도 없다.

## 24. 결함

### D-47. 프로토콜에 계층적 경유 개념이 없다
메시지는 발신자와 최종 대상만 안다. "이 관문을 지나 저기로"를 표현할 자리가 없다. 전제 11의 배치에서는 모든 외부 왕래가 2단 경유인데 그 구조가 wire에 나타나지 않는다.

### D-48. 컨트롤러의 relay가 정적 맵 기반이다
`ControllerProcessor::Remote(RouteProcessor)`는 기동 시 주입된 라우트 맵으로만 포워딩한다. 요청마다 대상이 달라지는 경유를 할 수 없다. 제어 경로가 컨트롤러를 지나야 하는데 그 통로가 정적이다.

### D-49. 에이전트에 중계 능력이 없다
에이전트는 자기가 수행할 메시지만 처리한다. **다른 에이전트로 단순 전달하는 처리 경로가 없다.** `AgentProcessor::handle`은 모든 kind를 자기 것으로 간주하고, 대상이 다른 에이전트임을 표현할 자리도 없다.

전제 11에서 진입 에이전트는 받은 메시지의 상당수를 **소비하지 않고 넘겨야** 한다. 나아가 중계는 진입 에이전트만의 특권이 아니라 **모든 에이전트가 가져야 할 일반 능력**이다.

### ~~D-50. 적재 경로의 관문이 정의되지 않았다~~ (해소)
"적재 시점에는 1번 노드가 없으므로 관문이 미정"이라 적었으나, 진입 에이전트가 노드 배치와 무관한 순수 진입점으로 정의되면서 소멸했다. 노드 생성·삭제, 적재·해제, 인퍼런스가 **모두 같은 진입 에이전트**를 지난다. Q-34도 함께 종결.

### ~~D-52. 에이전트가 자신의 도달 주소를 보고하지 않는다~~ (철회)
`HARDWARE_REPORT.snapshot`에 에이전트 자신의 endpoint가 없다는 관찰은 사실이나 **결함이 아니다.**

OUTER는 인프라 사실의 소유자이며 모든 에이전트의 접근 주소를 이미 안다(전제 1). 이를 프로토콜로 발견하려 들면 "주소를 알아야 물어보는데 물어봐야 주소를 안다"는 선후 모순이 생긴다. 에이전트는 **자기 주소를 알 필요조차 없다** — 전제 10이 노드에 적용한 원칙과 같은 방향이다.

따라서 capability 보고에 도달 주소를 넣지 않는다. Q-40도 함께 종결.

**남는 구분:** 어댑터의 `ADAPTER_REGISTER.endpoint`는 다르다. 어댑터 프로세스는 동적으로 등장·소멸하고 그 주소는 에이전트 내부의 사실이므로 자기 등록이 맞다. **OUTER가 소유하는 인프라 사실과 에이전트 내부의 동적 사실을 혼동하지 않는다.**

### D-54. 에이전트가 `session_id`를 발급한다
`INGRESS_SUBMIT.session_id`가 비어 있으면 에이전트가 `{controller_id}-session-{n}` 형태로 발급한다. 게다가 **발급기가 두 곳에 서로 모르는 카운터로 존재**한다 — [`ingress/mod.rs`](layers/runtime/src/domain/agent/ingress/mod.rs)와 [`routing/processor/mod.rs`](layers/runtime/src/application/routing/processor/mod.rs). 같은 controller_id가 두 경로를 타면 충돌한다.

당초 단순 중복 결함으로 기록했으나 **§1.3의 발급 원칙 위반이다.** 식별자는 OUTER가 발급하며, 에이전트가 이름을 짓는 순간 그 이름을 아는 주체가 에이전트뿐이 된다. 폐지된 `controller_id`를 이름에 박고 있다는 점(P-6)에서도 잔재다.

해결: `session_id`는 OUTER가 발급하고 빈 값을 허용하지 않는다. 두 발급기를 모두 제거한다.

### D-53. 자기기술 주소와 wire authz 부재가 겹친다
주소가 메시지에 실리면 에이전트는 **메시지가 지시하는 곳으로 접속을 연다.** 현재 P4에는 TLS도 wire authz도 없으므로(문서 §9), 프레임을 넣을 수 있는 주체는 에이전트가 임의 주소로 접속하게 만들 수 있다.

내부망이 신뢰 경계라는 전제 위에서는 감수 가능하나, **전제 11의 배치에서 진입 에이전트는 경계에 걸쳐 있다.** 설계 결정으로 기록해 두고 authz 도입 시 함께 다룬다(Q-41).

### D-51. 홉 증가가 토큰 경로에 얹힌다
`TOKEN`은 토큰마다 발생한다. 마지막 노드가 진입 에이전트와 다른 에이전트에 있으면 토큰마다 **진입 에이전트 → 컨트롤러 → OUTER**로 중계 홉이 하나 더 붙는다. 집계 TPS 목표에 직접 영향을 준다.

노드 배치가 자유로워진 만큼(전제 11 정정) 이는 편성 최적화의 문제로 남는다 — 마지막 노드를 진입 에이전트에 두면 바이패스된다.

## 25. 보완 설계 (초안)

### P-34. 대상 주소를 메시지가 자기기술한다 (결정)

메시지 표준은 대상 **객체**만이 아니라 그 객체가 **소속된 에이전트에 도달하는 정보**를 함께 실어야 한다. URL·호스트·포트 등 접속에 충분한 정보를 봉투가 자기기술한다.

이는 P-35의 **전제조건**이다. 중계 판정이 조회 테이블을 참조해야 한다면 워커 루프가 상태를 갖게 되고, 전제 2("인자로 전달될 상태는 외부에")와 전제 9(워커는 자기 일과 큐잉만)가 동시에 깨진다. **자기기술 주소여야 중계가 무상태로 성립한다.**

적용 범위는 셋이며 표기는 하나로 통일한다.

| 자리 | 내용 |
|---|---|
| 봉투의 대상 주소 | 이 메시지가 최종적으로 닿아야 할 에이전트의 접속 정보 |
| 체인 항목 (P-25) | `(에이전트 접속 정보, node_id, binding_id, runtime_generation)` |
| 귀환 주소 (P-26) | 진입 에이전트의 접속 정보 |

D-47·D-48 해소. `RouteProcessor`의 정적 맵과 `TcpTransport`의 고정 endpoint를 함께 대체한다. Q-25·Q-35가 이 결정으로 종결된다.

**frame 계층 변경이다.** 현 routed envelope는 `route_id`와 `deadline`만 나르므로 대상 주소 자리가 없다. P4B1 v6에 해당하며 Q-3(announce)과 같은 판에서 처리한다.

### P-35. 중계를 큐 디스패치의 기본 동작으로 내장한다

**핸들러 계층의 기능이 아니다.** 큐에서 메시지를 꺼내는 기초 로직이 모든 메시지에 대해 기본으로 수행하는 판정이어야 하며, 상위 코드는 중계를 인지하지 않아도 된다.

배치 위치는 워커 루프의 핸들러 호출 **직전**이다 — [`task_queue/worker/mod.rs`](layers/runtime/src/foundation/task_queue/worker/mod.rs)의 `spawn` 루프가 봉투를 꺼내 `handler.handle`로 넘기는 지점.

| 판정 | 동작 |
|---|---|
| 대상이 자신 | 핸들러로 넘긴다 — 현행 동작 |
| 대상이 다른 에이전트 | **핸들러를 거치지 않고** 다음 홉으로 큐잉한다 |

판정 근거는 봉투의 대상 주소(P-34)다. `TaskEnvelope`에는 이미 `source`/`target` Participant와 `is_local_bypass()`가 있으므로, 같은 자리에 대칭 개념을 놓는 것이다 — **bypass가 "안으로 접는" 판정이라면 중계는 "밖으로 미는" 판정**이고 둘은 같은 조건의 양면이다.

이 배치의 이점 셋.
- 핸들러는 자기 것만 안다. `AgentProcessor`에 중계 분기가 생기지 않는다
- 새 메시지 kind가 추가되어도 중계는 자동으로 따라온다. kind별 중계 규칙을 쓸 일이 없다
- **전제 9와 자연스럽게 맞는다.** 중계는 "자기 일을 하고 메시지를 큐에 넣는 것"의 가장 단순한 형태다. 반환값도 블로킹도 없다

중계 시 에이전트는 내용을 해석하지 않는다 — 전제 3의 "경유는 소유가 아니다"가 컨트롤러뿐 아니라 에이전트에도 적용된다. 따라서 "진입 에이전트"는 별도 역할이 아니라 **컨트롤러가 도달할 수 있는 위치에 있는 에이전트**를 가리키는 말일 뿐이고, `ParticipantRole`을 늘릴 필요가 없다(Q-35). 위상은 배치의 결과이지 프로토콜의 타입이 아니다.

### P-36. 동일 에이전트 바이패스
대상이 자신이면 중계 홉을 건너뛴다. 판정은 **에이전트 동일성**이며, `TaskEnvelope::is_local_bypass`가 이미 쓰는 규칙과 같다 — `source.agent_id == target.agent_id`.

기존 in-process bypass 개념을 재사용하므로 새 판정 로직이 필요 없다. P-35의 두 갈래 판정과 같은 조건이다.

### P-37. 컨트롤러의 relay 책임을 계약으로 못 박는다
컨트롤러가 경유시키는 메시지에 대해 **해석·검증·상태 보유를 금지**한다. 인퍼런스에서 컨트롤러가 하는 일(체인 미해석, 통과)과 같은 규칙을 제어 경로에도 적용한다.

이로써 P-6(소유 게이트로서의 `controller_id` 제거)과 충돌하지 않는다. 컨트롤러는 **경유 주소**로 등장하지 소유자로 등장하지 않는다.

---

# 주제 I. 인퍼런스 요청의 표현력

전제 12. 샘플링·디코딩 스펙이 구상 런타임 수준에 못 미친다.

## 26. 현재 상태 (검증 완료)

### 26.1 P4가 나르는 것

`ExecutionRequest`의 생성 관련 필드는 `max_tokens`(u32), `temperature`(f32), `prompt`(text), `options`(불투명 JSON) 넷이다. 샘플링 파라미터 중 **wire 필드로 승격된 것은 `temperature` 하나**뿐이다.

### 26.2 어댑터가 실제로 하는 일 — 정반대다

| 어댑터 | 동작 |
|---|---|
| stock llama.cpp | **전부 통과.** `model`/`messages`/`stream`만 보호하고 나머지 옵션은 그대로 전달 — [`llamacpp/.../options/mod.rs`](layers/adapters/llamacpp/src/application/options/mod.rs) |
| **Pipeline** | **5개만 화이트리스트.** `["max_tokens", "temperature", "top_p", "top_k", "seed"]` 외에는 **조용히 버린다** — [`adapter/.../execution/options/mod.rs`](layers/adapters/adapter/src/application/execution/options/mod.rs) |

두 어댑터 모두 프롬프트를 `messages: [{"role":"user","content": prompt}]`로 **하드코딩**한다.

### 26.3 upstream 실제 표면

고정된 upstream `common/common.h`의 `common_params_sampling`은 약 35개 필드와 sampler 순서 배열, grammar, logit_bias, reasoning budget을 갖는다.

```text
seed n_prev n_probs min_keep top_k top_p min_p xtc_probability xtc_threshold
typ_p temp dynatemp_range dynatemp_exponent penalty_last_n penalty_repeat
penalty_freq penalty_present dry_multiplier dry_base dry_allowed_length
dry_penalty_last_n dry_sequence_breakers adaptive_target adaptive_decay
mirostat mirostat_tau mirostat_eta top_n_sigma ignore_eos timing_per_token
samplers[] grammar grammar_lazy grammar_triggers preserved_tokens
logit_bias[] logit_bias_eog reasoning_budget_* backend_sampling
```

Pipeline 화이트리스트가 덮는 것은 이 중 `seed`, `top_k`, `top_p`, `temp` **넷**이다.

## 27. 결함

### D-55. Pipeline 어댑터가 샘플링 옵션을 조용히 버린다
`SUPPORTED` 5개 외 모든 키가 경고도 오류도 없이 사라진다. 호출자는 `min_p`나 `repeat_penalty`를 보내고 적용되었다고 믿는다. **전제 12의 "조용한 누락 금지" 정면 위반**이며, `docs/model-load.md`가 적재 옵션에 대해 선언한 규칙과도 어긋난다.

### D-56. Pipeline 경로에서 structured output이 원리적으로 불가능하다
구조화 출력은 grammar 샘플러의 **로짓 필터링**으로 동작한다. 생성 후 파싱으로 대체할 수 없다 — 모델이 애초에 그 형식만 내도록 토큰 분포를 제약하는 방식이기 때문이다.

`grammar`, `grammar_lazy`, `grammar_triggers`, `json_schema`, `preserved_tokens`가 전부 화이트리스트 밖이므로 **Pipeline 경로에서는 구조화 출력을 켤 방법이 없다.** 이는 기능 부족이 아니라 기능 부재다.

같은 이유로 `logit_bias`, `ignore_eos`, `reasoning_budget_*`도 사후처리 불가 항목이며 모두 누락되어 있다.

### D-57. 같은 `options` 필드가 어댑터마다 정반대로 처리된다
stock은 전부 통과, Pipeline은 5개만 통과. 호출자는 어느 쪽인지 알 수 없고 프로토콜에 그 차이를 표현할 자리도 없다. 주제 C의 `D-17`(적재 옵션의 무시 대 거부)과 같은 구조의 문제가 인퍼런스 쪽에도 있다.

### D-58. 프롬프트가 단일 문자열이라 대화 구조를 표현할 수 없다
`prompt: String`이 두 어댑터에서 모두 `[{"role":"user","content": prompt}]`로 하드코딩된다. **system prompt를 보낼 방법이 없고**, 다중 턴 메시지도, assistant prefill도, 멀티모달 파트도 표현할 수 없다.

`session_id`가 있으나 이는 세션 식별자일 뿐 대화 이력의 전달 수단이 아니다.

### D-59. wire 필드 승격 기준이 자의적이고 이중화되어 있다
샘플링 파라미터 중 `temperature`만 wire 필드다. `top_p`·`top_k`·`min_p`는 옵션 문자열에 있는데 `temperature`만 승격된 근거가 없다. 게다가 두 어댑터 모두 `options`의 동명 키와 wire 필드를 병합해야 해서 **우선순위 규칙이 어댑터마다 다르다** — stock은 `or_insert_with`(옵션 우선), Pipeline은 나중 삽입(wire 우선).

## 28. 보완 설계 (초안)

### P-40. 인퍼런스 옵션을 불투명 통과로 통일한다 (전제 12)
적재 옵션과 같은 규칙이다. P4는 `options`를 해석하지 않고, 어댑터가 구상 런타임에 그대로 전달한다. stock llama.cpp 어댑터의 현재 동작이 이미 정답이므로 **Pipeline 어댑터를 그 형태로 맞춘다.**

화이트리스트를 없애면 upstream이 샘플러를 추가해도 P4도 어댑터도 바뀌지 않는다.

### P-41. 조용한 누락을 금지한다
어댑터가 해석할 수 없는 키를 만나면 버리지 말고 `ERROR`로 종결한다. 전제 12의 강제 조항이며, 주제 C의 P-10(사전 발견 수단)과 같은 판단 축이다 — 어댑터가 지원 키를 선언하게 할지는 Q-9와 함께 정한다.

### P-42. 프롬프트를 대화 구조로 바꾼다
단일 문자열을 메시지 배열로 교체한다. system·user·assistant 역할과 다중 턴, 그리고 멀티모달 파트를 표현할 수 있어야 한다. D-58 해소.

전제 12에 따라 이 구조를 P4가 해석할 필요는 없다 — **표현할 수 있기만 하면 된다.** 따라서 옵션 문자열에 담는 선택지도 성립한다(Q-45).

### P-43. wire 필드를 정리한다
`temperature`·`max_tokens`가 wire 필드이면서 옵션에도 동명 키가 존재하는 이중 구조를 없앤다. 선택지는 둘이며 어느 쪽이든 **우선순위 규칙이 사라지는 것**이 목적이다(Q-44).
- 전부 옵션으로 내린다 — P4는 생성 파라미터를 하나도 모른다
- 전부 wire로 올린다 — 전제 12에 반하므로 채택하지 않는다

---

# 주제 J. 어댑터 경계

목표 계층: `P4 어댑터 인터페이스 ← 구상 어댑터 ← 구상 백엔드`. **P4는 llama.cpp를 몰라야 한다.**

## 29. 현재 상태 (검증 완료)

### 29.1 지켜지고 있는 것

**의존 방향이 정확하다.**

```text
p4-protocol   (의존 0개)
     ▲   ▲   ▲
     │   │   └── p4-llamacpp
     │   └────── p4-adapter
     └────────── p4-runtime
```

역참조가 없다. `layers/protocol/src` 전체에 `llama|gguf|ggml|cuda|vulkan|metal|rocm|nvidia` 문자열이 **0건**이다.

`adapter_kind`는 어댑터가 `"pipeline"`/`"llamacpp"`로 자기 신고할 뿐이며 protocol은 값을 해석하지 않는다. `stage_plan`·`node_spec`·`descriptor`·`options`는 bounded text로 통과한다.

### 29.2 누출 지점

| 위치 | 내용 |
|---|---|
| `contract/message/mod.rs` | `DraftReport`의 `kv_bytes`·`layer_bytes`·`ffn_bytes` |
| `contract/execution/mod.rs` | `temperature`·`max_tokens`(D-59), `text` |
| `contract/phase/mod.rs` | `Prefill`/`Decode` |
| `docs/model-load.md` | llama.cpp 노브를 정식 스키마로 규범화 |
| `domain/hardware/mod.rs` | `nvidia-smi`(D-3) |

## 30. 결함

### D-62. 트랜스포머 내부 구조가 protocol 계약에 있다
`DRAFT_REPORT`의 `kv_bytes`·`layer_bytes`·`ffn_bytes`는 P4가 **"모델은 KV 캐시와 FFN 블록으로 이루어진다"**를 안다는 뜻이다. llama.cpp 특정은 아니나 어댑터 인터페이스가 알아야 할 것도 아니며, 구조가 다른 백엔드에서는 의미를 잃는다.

### D-63. 문서가 코드보다 더 샌다
[`docs/model-load.md`](docs/model-load.md)가 `load_options`의 **정식 스키마**로 `flash_attention`, `mmap`, `kv_cache.type_k/type_v/offload`, `context_batch_tokens`, `context_ubatch_tokens`를 규범으로 명시한다. 전부 llama.cpp 노브다.

코드는 불투명 통과인데 **문서가 P4 계층의 계약이라고 선언한다.** 전제 6·12를 확정한 지금 이 문서는 계약이 아니라 예시여야 한다.

### D-64. 어댑터 인터페이스가 명시적 산출물로 없다
어댑터가 구현해야 할 계약이 별도 아티팩트가 아니라 **P4 메시지 계약 그 자체**다. 그 설계 자체는 정당하나, 그러면 **백엔드 중립의 부담이 전부 메시지 계약에 실린다.** D-62·D-63·D-59가 그 부담을 감당하지 못하고 있는 증거다.

### D-70. 계약이 완결형 노드와 스테이지 노드를 구분하지 않는다

vLLM 도입 가능성으로 검증한 결과다.

| 종류 | 성격 | 체인 길이 | 예 |
|---|---|---|---|
| **완결형** | 모델 전체를 스스로 서빙. 내부 TP/PP는 자기 소관 | 1 | vLLM, stock llama-server |
| **스테이지** | 레이어 구간만 담당, hidden state 교환 | n | 우리 Pipeline 런타임 |

주제 F·G의 체인 설계는 **스테이지 노드를 전제**한다 — 우리가 스테이지 경계를 소유하고, hidden state를 노드 사이로 넘기며, 디코드 루프를 바깥에서 돌린다. vLLM은 파이프라인 병렬을 내부(Ray/NCCL)에서 하므로 PP 스테이지가 P4 노드로 주소 지정되지 않는다.

계약에 이 구분이 없어 **체인 길이 1이 유효한 구성인지가 명시되지 않았다.** 명시되면 완결형 백엔드가 자연스럽게 수용되고, 명시되지 않으면 체인 설계가 특정 백엔드 모양을 암묵 전제하게 된다.

**vLLM 도입 자체는 가능하다.** llamacpp 어댑터의 추론 경로 전체가 `POST /v1/chat/completions` + SSE, 즉 OpenAI 호환 API 하나이므로 vLLM이 그대로 대응한다. 적재도 프로세스 기동 시점이라 stock llama-server와 같은 제약이 적용된다. 걸리는 것은 `D-62`(KV·FFN 분해 요구), `D-58`(프롬프트 단일 문자열), `session_id`의 대응물 부재이며 **전부 vLLM 때문이 아니라 기존 결함이 드러나는 것**이다.

### D-65. 구상 어댑터가 인터페이스 이름을 점유한다
크레이트 `p4-adapter`(`layers/adapters/adapter/`)는 인터페이스가 아니라 **Pipeline 구상 어댑터**다 — `linker-pipeline-inference-stream-v1`, `/api/runtime-groups`, "Pipeline binding" 등이 박혀 있다.

[`layers/adapters/README.md`](layers/adapters/README.md)는 스스로 이 디렉터리를 **`pipeline/`이라고 부른다.** 의도한 이름이 문서에 남아 있고 실제 디렉터리만 `adapter/`다.

## 31. 보완 설계 (초안)

### P-45. `DRAFT_REPORT`를 구조 중립 보고로
바이트 항목을 트랜스포머 구조로 고정하지 않는다. 총량과 **어댑터가 정의한 분류**로 나누어, 분류 이름과 값을 어댑터가 채우는 형태로 바꾼다. 전제 6·12와 같은 방향이다 — 지시도 보고도 P4가 해석하지 않되, 보고는 구조화된다(P-30 논의 참조).

### P-46. `docs/model-load.md`의 지위를 격하
정식 스키마에서 **예시**로 내린다. 계약은 "옵션은 불투명 문자열이고 어댑터가 해석한다"(전제 6)이며, 구체 키 목록은 어댑터별 문서로 옮긴다.

### P-52. 노드 종류를 구분하고 체인 길이 1을 명시적으로 유효화

D-70의 해소안이다.

1. **체인 길이 1을 유효한 구성으로 계약에 명시한다.** 완결형 노드는 길이 1 체인이며, 그 경우 `Node[0]`이 곧 마지막 노드이므로 주제 G의 진입·완료 보고와 귀환이 그대로 성립한다
2. **노드 종류를 어댑터가 선언한다.** `ADAPTER_REGISTER.descriptor`에 스테이지 참여 가능 여부를 담는다. P-10(지원 옵션 선언)과 같은 자리이며 P4는 값을 해석하지 않고 OUTER가 편성에 쓴다
3. **OUTER의 편성 규칙:** 완결형 노드는 다른 노드와 체인을 이룰 수 없다. 이 판정은 OUTER가 하며(전제 10·P-33), 에이전트는 어긋난 지시에 실패를 보고할 뿐이다

이로써 vLLM·TGI·SGLang 같은 완결형 백엔드가 **체인 설계를 바꾸지 않고** 참여한다. 스테이지 체인은 우리가 경계를 소유하는 백엔드에만 적용된다.

### P-47. 이름 정정
`layers/adapters/adapter/` → `layers/adapters/pipeline/`, 크레이트 `p4-adapter` → `p4-pipeline`. layer README가 이미 그 이름을 쓴다. 인터페이스 자리를 비운다.

**단계 0에 넣을 수 있다.** wire 무변경이고 다른 항목과 의존이 없다.

---

# 주제 K. 메시지 디스패치 계층

목표 계층. 각 단계는 **바깥일수록 범용이고 안쪽으로 갈수록 구상**이다.

```text
에이전트 [범용 메시지큐 관리]
  └─ 다른 에이전트에게 토스 / 바이패스 판정
      └─ 워커의 범용 처리
          └─ 해당 컨트롤러 또는 노드에게 전달
              └─ (노드) 그 노드에 연결된 노드 어댑터에게 전달
                  └─ 구상 노드 어댑터 → 구상 인퍼런스 객체
```

**kind별 해석은 마지막 두 단계에서만 일어나야 한다.** 앞의 네 단계는 봉투만 보고 움직인다.

## 32. 현재 상태 (검증 완료)

| 목표 단계 | 현재 |
|---|---|
| 범용 메시지큐 관리 | `TaskQueue`가 있으나 **P4 `Message`를 안다** — `TaskEnvelope`가 `queue: message.queue_class()`로 분류를 계산한다 |
| 토스 / 바이패스 판정 | **없다.** 바이패스 판정(`is_local_bypass`)만 있고 토스가 없다 (D-49) |
| 워커의 범용 처리 | 워커는 범용이나 곧바로 kind 분기로 넘어간다 |
| 컨트롤러·노드에게 전달 | **없다.** 참여자 전달 계층이 비어 있고 에이전트가 어댑터 transport로 직행한다 |
| 노드 → 노드 어댑터 | `NodeSlot`은 수동 데이터다. 전달 주체가 아니라 `adapter_id` 문자열을 담을 뿐이며, 조회·전달을 `AgentProcessor`가 대신한다 |
| 구상 어댑터 → 구상 객체 | 정상 동작 |

## 33. 결함

### D-66. kind 분기가 계층에 흩어져 중복된다
[`dispatch/mod.rs:104`](layers/runtime/src/application/dispatch/mod.rs)의 `match task.message`와 [`agent/mod.rs:104`](layers/runtime/src/domain/agent/mod.rs)의 `match message`가 **같은 메시지를 두 번 분해한다.** 목표 계층에서 kind 해석은 마지막 두 단계의 일인데 상위 두 곳이 이미 알고 있다.

### D-67. 참여자 전달 계층이 없다
"해당 컨트롤러 또는 노드에게 전달"에 해당하는 단계가 존재하지 않는다. `AgentProcessor`가 registry 조회·권한 검사·transport 전달을 kind별 핸들러 안에서 한꺼번에 수행하고 **노드를 건너뛰어 어댑터 transport로 직행한다.**

결과로 `NodeSlot`이 전달 주체가 아니라 수동 데이터가 된다. "노드가 자기 어댑터에게 전달한다"는 계층이 코드에 없다.

### D-68. 중계에도 전체 payload 디코드가 필요하다
`read_routed_message`는 봉투를 읽자마자 `decode_payload`로 **본문을 완전히 디코드**한다. 남에게 넘길 메시지까지 내용을 해석해야 한다.

목표 계층의 두 번째 단계(토스 판정)는 봉투만 보면 되고, 그래야 "경유는 소유가 아니다"(전제 3)가 구현 수준에서도 참이 된다. 지금 구조로 중계를 붙이면 **중계 노드가 남의 메시지를 전부 해석하게 된다.**

### D-69. 큐가 P4 메시지 타입에 묶여 있다
`TaskEnvelope::new_routed`가 `message.queue_class()`를 호출해 레인을 정한다 — [`task/mod.rs:116`](layers/protocol/src/task/mod.rs). 큐가 범용이려면 **분류가 봉투에 실려 와야** 하고 큐는 그 값을 읽기만 해야 한다.

## 34. 보완 설계 (초안)

### P-48. 봉투와 본문의 디코드를 분리한다
프레임 수신 시 **봉투(route, 대상 주소, 분류, 데드라인)만 먼저 파싱**하고 본문은 지연한다. 대상이 자신일 때만 본문을 디코드한다.

D-68·D-69가 함께 풀린다. 중계 비용이 낮아지고(§92의 D-51 완화), 큐가 메시지 타입을 몰라도 된다.

전제 6·12(옵션 불투명 통과)의 자연스러운 확장이다 — **중계 경로에서는 메시지 전체가 불투명하다.**

### P-49. 참여자 전달 계층을 세운다
"컨트롤러 또는 노드에게 전달"을 명시적 단계로 만든다. 에이전트는 봉투의 대상으로 참여자를 고르고, **노드가 자기 어댑터로 전달하는 책임을 갖는다.** `NodeSlot`이 데이터에서 전달 주체로 승격된다.

D-67 해소이며 P-15(상태 기계)와 자연스럽게 붙는다 — 상태 판정과 전달이 같은 객체에 놓인다.

### P-50. kind 분기를 마지막 두 단계로 밀어낸다
`dispatch`와 `AgentProcessor`의 이중 분기를 없앤다. 상위 계층은 봉투로만 라우팅하고, kind 해석은 노드 어댑터 경계 이후에서 한다.

**단, 에이전트가 소유하는 것은 예외다** — `NODE_CREATE`/`NODE_DELETE`처럼 NodeSlot 자체를 다루는 메시지는 에이전트가 해석한다(P-5의 두 번째 층). 그 경계를 명시적으로 긋는 것이 이 항목의 실제 작업이다.

### P-51. 큐 분류를 봉투가 나른다
`queue_class`를 메시지에서 계산하지 않고 발신자가 봉투에 싣는다. 큐는 값을 읽어 레인을 고를 뿐 메시지를 모른다. D-69 해소, P-48의 전제.

---

# 주제 L. 백엔드 소유와 upstream 추적

목표: **구상 어댑터가 자기 백엔드를 소유한다.** upstream은 언제나 최신을 풀받을 수 있고, 우리가 필요한 기능만 붙여 컴파일한다. llama.cpp뿐 아니라 vLLM 등 모든 구상 어댑터에 같은 규칙을 적용한다.

## 35. 현재 상태 (검증 완료)

### 35.1 이미 목표 형태인 것

llama.cpp에 대해서는 **요청한 구조가 이미 구현되어 있다.**

| 항목 | 현재 |
|---|---|
| upstream | `apps/llama/upstream` — **pristine 서브모듈**. `.gitmodules`가 공식 `ggml-org/llama.cpp`를 가리킨다 |
| 패치 | `apps/llama/native/compat/<upstream-sha>/` — 순서 있는 패치 세트. SHA 4개분이 관리 중 |
| 검증 | `manifest.json` — upstream 커밋·날짜·subject·직전 pin, 패치별 SHA256, 패치 세트 해시, 적용된 트리 해시 |
| 적용 | `scripts/prepare-pipeline-upstream.mjs`가 무시되는 `.cache/`에 worktree를 만들어 해시 검증 후 순서대로 적용. **생성물은 커밋되지 않는다** |
| 빌드 분기 | stock 빌드는 pristine 서브모듈을 직접 컴파일. **Pipeline 빌드만 패치를 쓴다** |
| 헤더 경계 | 우리 C++는 공개 헤더만 include — `llama.h`, `ggml-backend.h`, `ggml-cuda.h` |
| 절차 | README에 6단계 갱신 절차. "빌드를 통과시키려고 공식 서브모듈을 편집하지 말 것" 명시 |

즉 "항상 최신을 풀받고 필요한 기능만 붙여 컴파일한다"는 요구는 **설계로 이미 성립해 있다.** 남은 문제는 소유 위치와 추적 비용이다.

### 35.2 패치의 성격별 분포

기준 커밋 `3e3a7a416`, 14개 패치 2,219줄.

| 성격 | 패치 | 줄 수 | 비중 | 리베이스 비용 |
|---|---|---:|---:|---|
| **upstream 결함** | `0001-ggml-backend`, `0002-ggml-rpc` | 138 | 6% | 공식 기여 시 **영구 소멸** |
| **ABI 노출** | `0003-public-pipeline-abi`, `0005`, `0007`, `0008`, `0010`, `0012` | 484 | 22% | 낮음 — 헤더 위주 |
| **내부 개조** | `0004-llama-context`(646), `0006-llama-graph`(641), `0009`, `0011`, `0013`, `0014` | 1,597 | 72% | 높음 |

**`0004`와 `0006` 둘이 1,287줄로 전체의 58%다.** 최신 추적 비용이 사실상 이 두 파일에 있다.

`0001`은 upstream이 스스로 `// FIXME: count the number of inputs instead of only checking when full`이라 표시해 둔 자리를 고친 것이다.

## 36. 결함

### D-71. 백엔드 소유가 어댑터 밖에 있다
P4의 Pipeline 어댑터는 `layers/adapters/adapter`에 있는데, 그것이 구동하는 백엔드(upstream + 패치 + 준비 스크립트 + 호스트 supervisor)는 **`apps/llama` 아래에 있다.** 둘은 HTTP로 연결된다 — `/api/runtime-groups`, `linker-pipeline-inference-stream-v1`.

어댑터와 그 백엔드가 서로 다른 앱에 흩어져 있어 **"구상 어댑터가 자기 백엔드를 소유한다"가 성립하지 않는다.** 어댑터를 추가·교체할 때 두 곳을 동시에 만져야 한다.

### D-72. 최신 추적 비용이 두 패치에 집중된다
`0004-llama-context`(646줄)와 `0006-llama-graph`(641줄)가 패치 총량의 58%다. upstream을 올릴 때마다 이 둘의 충돌 해소가 작업의 대부분을 차지한다. README의 갱신 절차 3단계("포팅")가 실질적으로 이 두 파일의 리베이스다.

### D-73. upstream 결함 수정이 우리 패치로 상주한다
`0001`은 upstream의 `FIXME` 자리를 고친 것이고 `0002`는 누락된 `<chrono>` include다. **둘 다 우리 고유 기능이 아니다.** 공식에 기여하지 않으면 upstream을 올릴 때마다 영구히 따라다니는 비용이 된다.

### D-74. 완결형·스테이지별 upstream 정책이 명문화되지 않았다
패치가 필요한 이유는 **부분 로딩**이고, 부분 로딩은 **스테이지 노드만의 요구**다(D-70).

| 노드 종류 | upstream 개조 | 현재 |
|---|---|---|
| 완결형 | **불필요.** 바이너리·패키지 의존만 | `layers/adapters/llamacpp`가 이미 이 형태 — stock llama-server에 붙고 패치를 쓰지 않는다 |
| 스테이지 | **필요.** compat 계층 필수 | Pipeline 어댑터 |

이 구분이 구조에 명시되지 않아, 새 백엔드를 붙일 때 패치가 필요한지 아닌지 판단 근거가 없다. **vLLM은 완결형이므로 upstream 개조가 0이고 pip pin만으로 끝난다** — 이것이 문서에 적혀 있지 않으면 불필요한 fork 검토가 반복된다.

## 37. 보완 설계 (초안)

### P-53. 백엔드를 어댑터 아래로 옮긴다

```text
adapters/
  llamacpp/          완결형 — upstream 개조 없음
    src/             P4 어댑터. stock llama-server에 OpenAI 호환 API로 접속
  pipeline/          스테이지 — 개조 필요
    upstream/        pristine 서브모듈
    compat/<sha>/    순서 있는 패치 + manifest
    scripts/         .cache worktree 준비·해시 검증
    src/             P4 어댑터
  vllm/              완결형 — pip pin
    src/
```

**새로 만드는 것이 아니라 이동이다.** `apps/llama`의 upstream·compat·scripts가 그대로 `adapters/pipeline/` 아래로 간다. D-71 해소이며 `P-47`(이름 정정)과 같은 작업에 포함된다.

이동이 끝나면 P4의 `apps/llama` 의존이 사라진다. 남는 것은 호스트 supervisor와의 HTTP 경계인데, 그것도 어댑터 소유가 되므로 앱 간 의존이 아니라 어댑터 내부 구조가 된다.

### P-54. upstream 정책을 노드 종류로 가른다

계약으로 명문화한다.

- **완결형 어댑터는 upstream을 개조하지 않는다.** 공식 배포물(바이너리·패키지)에만 의존하며 `compat/` 계층을 갖지 않는다
- **스테이지 어댑터만 `compat/` 계층을 갖는다.** 부분 로딩이 필요한 경우에 한한다
- 새 백엔드 도입 시 **먼저 완결형으로 가능한지 판단**하고, 불가능할 때만 스테이지를 검토한다

D-74 해소. `P-52`(노드 종류 구분)의 구조적 대응물이다.

### P-55. upstream 결함 패치를 공식에 기여해 소멸시킨다
`0001-ggml-backend`(FIXME 자리 수정), `0002-ggml-rpc`(include 누락)를 공식에 PR로 올린다. 받아들여지면 패치 세트에서 영구히 빠진다 — 138줄과 리베이스 대상 2개가 사라진다.

우리 고유 기능이 아니므로 기여에 장애가 없다. D-73 해소.

### P-56. 내부 개조를 ABI 노출로 전환한다 (검토 필요)
D-72의 근본 해소안이다. `0004`·`0006`의 1,287줄에서 **로직을 우리 코드로 끌어오고 upstream에는 훅만 남긴다.**

`0003-public-pipeline-abi.patch`(197줄)가 이미 그 방향의 시도로 보인다. 이를 극단으로 밀어 내부 개조를 ABI 노출로 수렴시킬 수 있다면, 최신 추적 비용이 **헤더 리베이스 수준**으로 떨어진다.

전환 가능 범위는 실제 패치 내용을 읽어야 판정된다(Q-56). 전부는 불가능하더라도 **비중을 줄이는 것만으로 효과가 크다** — 58%가 병목이므로.

---

# 주제 M. 저장소 종료 상태

이 작업은 **별도 브랜치**에서 이뤄진다. 종료 조건은 **"에이전트가 쓸 구상 어댑터가 `apps/p4` 밖에 없다"**이며, `apps/llama`는 실행 지식을 전부 넘기고 **계획 지식 제공자로 존속**한다(P-57·P-60).

당초 종료 상태를 "`apps/linker`와 `apps/p4`만"으로 잡았으나, 계획 지식을 OUTER 코어에 넣지 않기로 하면서(D-77 해소) 세 번째 앱이 남는다. 목표였던 **P4의 백엔드 소유**는 그대로 달성된다.

## 38. 역할 매핑

계획서가 추상적으로 쓰는 역할이 실제 산출물과 이렇게 대응한다.

| 역할 (§1.2) | 산출물 | 근거 |
|---|---|---|
| **OUTER** | `apps/linker` + `packages/linker_domain` | 토폴로지·identity·소유·카탈로그·계획 상태가 이미 여기 있다. §1.3의 식별자 발급과 P-33의 편성 소유가 곧 이 앱의 직무다 |
| **Controller** | `apps/p4/entrypoints/controller` | 이미 존재 |
| **Agent** | `apps/p4/entrypoints/agent` | 이미 존재 |
| **Node** | `apps/p4/entrypoints/node` | 이미 존재 |
| **구상 어댑터** | `apps/p4/adapters/*` | P-53의 이동 대상 |

**OUTER가 `apps/linker`라는 확정이 계획서 전반의 귀속을 정한다.** 하드웨어 capability 레코드, 노드 편성, 적재 계획, 체인 구성, 식별자 발급이 모두 `packages/linker_domain`(현재 3,041줄)으로 간다.

## 39. 현재 상태 (검증 완료)

해체 대상의 규모다.

| 구성 | 규모 | 성격 |
|---|---|---|
| `apps/llama/native/` | 134 파일 | upstream 서브모듈, `compat/<sha>/` 패치, `linker-node`, `linker-expert-worker`, `linker-device-probe`, `linker-moe-verify`, `linker-arch-fixtures` |
| `apps/llama/src/` | 56 파일 | 웹 UI + 호스트 supervisor(18082, `/api/runtime-groups`, `linker-pipeline-inference-stream-v1`) |
| `apps/llama/scripts/` | 28 파일 | `prepare-pipeline-upstream.mjs` 등 빌드·준비 |
| `packages/llama_domain` | **11,631줄** (common 6,113 / server 3,493 / front 1) | GGUF 검사, 배치 휴리스틱, 런타임 검증, Pipeline 기동 정책 |

**`packages/llama_domain`이 저장소 최대 자산이다.** `linker_domain`(3,041줄)의 약 4배다.

## 40. 결함

### D-75. `apps/llama`의 처분 계획이 없다
`P-53`은 upstream·compat·scripts의 이동만 다루고 웹 UI·supervisor·`llama_domain`은 다루지 않는다. **실행 지식과 계획 지식이 한 앱에 섞여 있어** 무엇을 넘기고 무엇을 남길지 기준이 없었다. P-60이 그 기준을 정한다.

### D-76. `packages/llama_domain`이 세 주체의 관심사를 한 패키지에 담고 있다
계획서의 소유 규칙에 비추면 11,631줄이 셋으로 갈린다.

| 내용 | 귀속 | 근거 |
|---|---|---|
| 배치 휴리스틱 | **OUTER** | P-33 — 편성은 OUTER 단독 소유 |
| GGUF 검사 | **OUTER** (또는 공유) | 적재 계획을 세우려면 모델 구조를 알아야 한다 |
| 런타임 검증 | **어댑터** | 전제 10 — 실체 판정은 구상 어댑터의 일 |
| Pipeline 기동 정책 | **Pipeline 어댑터** | 백엔드 고유 |

지금은 이 넷이 한 패키지에 있어 `apps/llama` 해체 시 통째로 갈 곳이 없다.

### D-77. OUTER가 모델 형식을 알아야 하는지가 미정이다 (해소 경로 확정)
D-76의 GGUF 검사가 `linker_domain`으로 가면 **OUTER 코어가 GGUF를 안다.** GGUF는 llama.cpp 형식이고 vLLM은 safetensors/HF다. 완결형 백엔드가 늘면 OUTER 코어가 형식마다 검사기를 갖게 된다.

이는 `D-64`(추상 부담이 어디에 실리는가)의 OUTER 측 판본이다. 어댑터 위임은 성립하지 않는다 — **편성은 적재 이전이므로** 그 시점에 어댑터가 그 모델을 들고 있지 않다.

**해소: 계획 지식을 OUTER 코어에 넣지 않고 백엔드별 모듈로 둔다(P-60).** OUTER는 형식을 아는 게 아니라 **형식을 아는 모듈을 소비**한다. `linker_domain`은 형식 무지 상태로 남는다.

### D-78. 저장소 계약 문서가 현 구조를 기술한다
루트 `CLAUDE.md`가 `apps/linker`·`apps/llama`·`packages/linker_domain`·`packages/llama_domain` 4자 구조와 그 고정 짝을 명시하고, 포트·Docker·호스트 supervisor 배치를 규정한다. 종료 상태에서는 전부 사실과 어긋난다.

## 41. 보완 설계 (초안)

### P-60. 분할 기준을 방화벽 위치로 삼는다 (결정)

관심사가 아니라 **"방화벽 어느 쪽에서 필요한가"**로 가른다. 전제 11의 위상과 직접 맞물리는 기준이다.

| 지식 | 위치 | 소유 | 내용 |
|---|---|---|---|
| **계획 지식** | 방화벽 밖 | OUTER가 **소비** | 모델 형식 검사, 배치 휴리스틱, 용량 추정, 모델 가용성 |
| **실행 지식** | 방화벽 안 | 어댑터가 **소유** | 런타임 기동, 적재, 추론, 스테이지 제어 |

**계획 지식은 `linker_domain`에 넣지 않는다.** OUTER 코어를 형식 무지 상태로 두고, 백엔드별 계획 모듈을 OUTER가 소비한다. D-77 해소이며 백엔드가 늘어도 OUTER 코어가 부풀지 않는다.

이 기준이 기존 코드의 이음매와 이미 일치한다.

| 구분 | `llama_domain/common` | `apps/llama/src/server` 라우트 |
|---|---|---|
| 계획 | `planner` **3,182줄** (common의 52%) | `/api/models/inspect`, `/api/models/availability`, `/api/plans`, `/api/resources` |
| 실행 | `protocol` 1,864 + `pipeline-*` 976 | `/api/processes`, `/api/runtime`, `/api/runtime-groups`, `/api/rpc-runtime-groups` |

### P-57. `apps/llama`의 처분 — 계획 지식 제공자로 존속

**해체하지 않는다.** 실행 지식만 걷어내고 계획 지식 제공자로 남긴다.

| 구성 | 처분 |
|---|---|
| `native/upstream`, `native/compat`, `scripts/prepare-*` | → `apps/p4/adapters/pipeline/` (P-53) |
| `native/linker-node` 외 네이티브 | → `apps/p4/adapters/pipeline/native/` |
| 호스트 supervisor (`/api/processes`, `/api/runtime*`) | → `apps/p4/adapters/pipeline/` — 어댑터 내부 경계가 된다 |
| `/api/models/*`, `/api/plans`, `/api/resources`와 그 UI | **존속** — OUTER가 소비하는 계획 표면 |
| `backend-contract.json`, `docs/` | 처분 따라 분산 |

**종료 상태가 2앱이 아니라 3앱이 된다.** 다만 원래 목표였던 "P4가 자기 백엔드를 소유한다"는 그대로 달성된다 — `apps/llama`에는 **에이전트가 쓸 구상 어댑터가 남지 않는다.**

이름이 실체와 어긋나는 점은 남는다. 존속하는 것은 런타임 앱이 아니라 llama.cpp/GGUF **계획 제공자**다(Q-64).

### P-58. `packages/llama_domain` 분할
P-60의 기준으로 가른다.
- `planner`(3,182) 등 계획 지식 → `apps/llama` 존속분과 함께 남는다. **`linker_domain`으로 옮기지 않는다**
- `protocol`(1,864), `pipeline-*`(976), `server`(3,493) 등 실행 지식 → `apps/p4/adapters/pipeline/`

11,631줄의 분할이지만 P-60의 기준선이 기존 디렉터리 경계와 대체로 일치하므로, 당초 예상보다 절단면이 깨끗하다(Q-61).

### P-59. 저장소 계약 문서 갱신
루트 `CLAUDE.md`의 아키텍처·포트·Docker·앱↔패키지 짝 규정을 종료 상태에 맞춘다. **브랜치 병합 시점에 함께 반영한다** — 그 전에 고치면 현재 트리를 기술하지 않게 된다.

---

## 89. 미결 결정

| # | 내용 | 종속 |
|---|---|---|
| Q-1 | `snapshot`을 버전 붙은 JSON 스키마로 규범화할 것인가, wire 필드로 승격할 것인가 | P-3은 전자 전제 |
| Q-2 | capability와 occupancy를 별도 메시지로 쪼갤 것인가, 한 스냅샷 안의 별도 절로 둘 것인가 | P-1 |
| Q-3 | agent-initiated announce를 신설할 것인가 | 채택 시 v5 호환 포기 → 죽은 표면 정리를 같은 판에서 처리 |
| ~~Q-4~~ | ~~`machine_id` 산출 방식 — OS 유래 값 대 설정 주입~~ | **(해소)** `machine_id` 자체가 불필요. 접근 주소가 identity다 (P-2) |
| Q-5 | `NODE_CREATE`가 기존 id에 대해 멱등 no-op인가 갱신인가 | D-10. 활성 바인딩이 있을 때의 처리가 걸린다 |
| Q-6 | `EXECUTE`의 `controller_id`를 남길 것인가, 컨트롤러 개입을 route/session 층으로 옮길 것인가 | P-6 |
| Q-7 | `HEALTH_CHECK`의 귀속 — 외부→에이전트 진단인가, 컨트롤러 관심사인가 | P-6 |
| Q-8 | 노드 제거 시 활성 바인딩·실행이 있으면 거부인가 강제 회수인가 | P-7 |
| Q-9 | 어댑터가 지원 옵션 집합을 `descriptor`로 **선언**할 것인가, 시행착오를 수용할 것인가 | P-10. 어느 쪽이든 P4는 옵션을 해석하지 않는다 |
| ~~Q-10~~ | ~~옵션 키 이름을 llama.cpp 플래그에 맞출 것인가, 백엔드 중립 이름으로 추상화할 것인가~~ | **(철회)** 전제 6에 따라 P4 관심사가 아니다. 어댑터와 외부 계획기가 공유할 규약 |
| ~~Q-11~~ | ~~artifacts를 `MODEL_LOAD` 필드로 승격할 것인가~~ | **(철회)** 옵션 문자열 안에 담는 것으로 결정. wire 변경 없음 |
| ~~Q-12~~ | ~~부분 로딩 단위를 레이어 범위로 할 것인가 텐서 패턴까지 허용할 것인가~~ | **(철회)** 어댑터 해석 범위. P4 관심사가 아니다 |
| ~~Q-13~~ | ~~적재 옵션 검증을 어디서 하는가~~ | **(결정)** 구상 어댑터. 전제 6 |
| Q-14 | 요청 route가 끊긴 적재의 진행·완료를 어떻게 되찾는가 — 전용 조회, 노드 상태 조회에 포함, announce 중 무엇인가 | D-20, P-13 |
| Q-15 | 노드:바인딩을 1:1로 좁힐 것인가, 1:N을 유지하고 상태 판정을 다르게 정의할 것인가 | P-16. P-15 상태 기계의 선행 조건 |
| Q-16 | 해제 단위를 `binding`으로 통일할 것인가, `deployment`로 올릴 것인가 | D-25. Q-15가 1:1이면 자동 정렬된다 |
| Q-17 | `forward::capture` 재설계 방식 — 에이전트 판단 후 전달인가, 에이전트가 자기 terminal로 대체 emit인가 | P-14. 전자는 지연, 후자는 어댑터 detail 손실 |
| Q-18 | 어댑터 프로세스 경계 내부도 CPS로 만들 것인가, P4 경계까지만 요구할 것인가 | D-30. 어댑터는 별도 프로세스이고 자체 런타임을 가진다 |
| Q-19 | 장기 작업을 몇 단으로 쪼갤 것인가 — 진행 폴링을 자기 재-enqueue Task로 둘 것인가 | P-19. 폴링 주기가 큐 부하가 된다 |
| Q-20 | 반환값 제거를 토대 trait 교체로 갈 것인가, 기존 trait 위에 CPS 어댑터를 씌울 것인가 | D-26, P-17. 전자는 전면 개편, 후자는 이중 구조 존속 |
| ~~Q-21~~ | ~~체인을 요청마다 실을 것인가, 사전 등록된 체인 id를 참조할 것인가~~ | **(결정)** 요청마다 싣는다. 체인은 프리필 메시지 안에 있다 |
| ~~Q-22~~ | ~~스테이지 이동을 P4로 감쌀 것인가, 전부 native에 맡길 것인가~~ | **(결정)** P4가 체인·순서·correlation을 나르고 노드가 스스로 전달한다. hidden state는 native 유지 |
| Q-23 | 컨트롤러가 요청 동안 보유하는 상태를 OUTER 귀환 route로 한정할 것인가 | §1.2. 체인 상태는 이미 메시지로 넘어갔으므로 남는 것은 귀환 경로뿐이다 |
| ~~Q-25~~ | ~~체인 항목의 agent 도달 주소를 무엇으로 표기할 것인가~~ | **(결정)** 자기기술 접속 정보로 통일. 봉투·체인·귀환 주소가 같은 표기를 쓴다 (P-34) |
| Q-26 | 체인 항목에 체인 전체를 실을 것인가, 남은 구간만 잘라 전달할 것인가 | P-22. 전자는 관측·재시도에 유리, 후자는 프레임이 작다 |
| Q-27 | `CANCEL`을 체인 전진 전파로 할 것인가, 각 노드의 correlation 자체 중단으로 할 것인가 | P-27, D-40 |
| Q-28 | 진입 보고의 수신자는 누구인가 — 자기 에이전트인가, 컨트롤러인가, 둘 다인가 | 진술상 1번 노드는 에이전트, 2번 노드는 컨트롤러로 갈렸다. 통일 필요 |
| Q-29 | 완료 보고의 명시 필드 최소 집합을 무엇으로 할 것인가 | P-29. 나머지는 문자열 확장 |
| Q-30 | 디코드 홉에서도 전 노드가 진입·완료를 보고하는가, 마지막 노드만 보고하는가 | 전자는 토큰마다 `2×n`개 제어 메시지 — 집계 TPS에 직접 영향 |
| Q-31 | 컨텍스트 초과를 어디서 판정하는가 — 어댑터 진입 검사인가, OUTER 편성 시 사전 검증인가 | D-44. 전제 10에 따라 P4가 기록하지는 않는다 |
| ~~Q-33~~ | ~~잘못 편성된 체인이 실행 중에 드러나는 것을 수용할 것인가~~ | **(결정)** 수용한다. OUTER가 완전한 체인·적재 상태를 기억하며, 지시가 실제와 다르면 에이전트는 실패를 보고할 뿐이다. 그 대가로 **에이전트·노드는 단순한 기계적 동작을 보장받는다** |
| ~~Q-34~~ | ~~적재 경로의 관문은 누구인가~~ | **(해소)** 진입 에이전트가 노드 배치와 무관한 순수 진입점이므로 모든 메시지가 같은 관문을 지난다. D-50과 함께 종결 |
| ~~Q-35~~ | ~~중계 판정을 위한 대상 주소를 어떻게 표기할 것인가~~ | **(결정)** 자기기술 접속 정보. 조회 없이 도달 가능해야 중계가 무상태다 (P-34) |
| Q-39 | 접속 정보의 표기 형식 — URL인가 `host:port`인가, scheme·전송 종류를 담을 것인가 | P-34. 향후 TLS·다른 전송을 수용하려면 scheme이 필요하다 |
| ~~Q-40~~ | ~~에이전트의 내부망 도달 주소를 누가 정하는가~~ | **(해소)** OUTER가 인프라 사실로 이미 소유한다. 프로토콜 발견 대상이 아니다 (전제 1, D-52 철회) |
| Q-41 | 자기기술 주소를 신뢰할 범위를 어떻게 제한할 것인가 | D-53. authz 도입 시점까지 내부망 신뢰를 전제할 것인지 |
| Q-42 | 프로세스 화신 표식을 둘 것인가, 에이전트 상태 지속화로 대신할 것인가 | P-39, D-7. 지속화하면 화신 구별의 필요 범위가 줄어든다 |
| Q-43 | `ingress_id`가 `request_id`와 별개로 필요한가 | §1.3. 둘 다 OUTER 발급이고 인퍼런스 1건을 가리킨다 |
| Q-44 | `temperature`·`max_tokens`를 wire에서 내릴 것인가 | P-43, D-59. 내리면 P4는 생성 파라미터를 하나도 모른다 |
| Q-45 | 대화 구조를 wire 구조로 둘 것인가, 옵션 문자열에 담을 것인가 | P-42, D-58. 전제 12는 후자를 허용한다 |
| Q-46 | 체인에서 샘플링·grammar는 어느 노드가 수행하는가 | 파이프라인 병렬에서 로짓은 **마지막 스테이지에서만** 나온다. 옵션이 전 노드에 전달될 필요가 있는지 |
| Q-47 | Pipeline 런타임이 실제로 지원하는 샘플러 범위는 어디까지인가 | P-40. 화이트리스트를 걷어내도 하위 런타임이 못 받으면 의미가 없다 — 실측 필요 |
| Q-48 | `load_options.batching.*`를 유지·강등·제거 중 무엇으로 할 것인가 | D-60, P-44. **타 세션(Mac+GB10, MI250)의 스케줄러 결론에 종속.** 여기서 단독 확정하지 않는다 |
| Q-49 | 연결 수립 정책(재시도·백오프·연결 예산)을 프로토콜 계약에 넣을 것인가 | D-61. 소스 라우팅은 홉마다 연결을 전제한다 |
| Q-50 | `Phase`(Prefill/Decode)와 `text`를 계약에 남길 것인가 | D-64. 실행 계약의 일부로 정당화 가능하나 백엔드 중립은 아니다 |
| Q-51 | 어댑터 인터페이스를 명시적 산출물로 만들 것인가 | D-64. 만들지 않으면 중립 부담이 메시지 계약에 남는다 |
| Q-52 | 봉투가 나를 최소 필드는 무엇인가 — route·대상·분류·데드라인이면 충분한가 | P-48·P-51. 중계가 본문을 안 보려면 봉투가 자족해야 한다 |
| Q-53 | 에이전트가 직접 해석하는 kind의 경계를 어디로 긋는가 | P-50. `NODE_CREATE`/`NODE_DELETE`는 에이전트 소유이나 나머지는 통과 대상 |
| Q-54 | 완결형 노드와 스테이지 노드의 구분을 `descriptor` 선언으로 둘 것인가, `adapter_kind`로 둘 것인가 | P-52, D-70. 전자는 P4 무해석, 후자는 계약이 종류를 안다 |
| Q-55 | 완결형 백엔드(vLLM 등)를 실제 도입 대상으로 삼을 것인가 | D-70. 삼는다면 P-52가 단계 5에 들어간다 |
| Q-56 | `0004-llama-context`·`0006-llama-graph`의 내부 개조를 ABI 노출로 전환할 수 있는가 | P-56, D-72. 패치 내용 분석이 선행되어야 판정된다. 전체의 58% |
| Q-57 | `compat/` 계층의 소유를 Pipeline 어댑터 하나로 한정할 것인가 | P-53·P-54. 다른 스테이지 백엔드가 생기면 각자 갖는다 |
| Q-58 | vLLM을 pip pin으로 둘 것인가 서브모듈로 둘 것인가 | P-53. 완결형이면 pin으로 충분하다 |
| ~~Q-59~~ | ~~`apps/llama`의 나머지는 어디로 가는가~~ | **(주제 M으로 이관)** P-57이 배치표를 정의한다 |
| Q-60 | 존속하는 계획 UI(`/api/models/*`·`/api/plans`)를 `apps/llama`에 둘 것인가 `apps/linker`로 합칠 것인가 | P-57. 합치면 2앱이 되나 OUTER 코어가 형식별 화면을 갖는다 |
| Q-61 | `llama_domain`의 절단면이 실제로 깨끗한가 — `planner`가 `protocol`·`pipeline-*`에 의존하는가 | P-58. 의존이 있으면 분할 비용이 커진다 |
| ~~Q-62~~ | ~~OUTER가 모델 형식을 직접 아는가, 검사를 위임하는가~~ | **(해소)** 둘 다 아니다. **형식을 아는 모듈을 소비**한다. OUTER 코어는 형식 무지 (P-60, D-77) |
| Q-64 | 존속하는 `apps/llama`의 이름을 유지할 것인가 | P-57. 실체는 런타임 앱이 아니라 llama.cpp/GGUF 계획 제공자다 |
| Q-65 | vLLM 도입 시 계획 모듈을 형제로 둘 것인가 | P-60. safetensors/HF 검사가 필요하면 같은 자리에 선다 |
| Q-63 | 브랜치 병합 시점과 `CLAUDE.md` 갱신을 어떻게 묶을 것인가 | P-59, D-78 |
| ~~Q-36~~ | ~~1번 노드를 게이트웨이에 배치하도록 편성 제약을 명문화할 것인가~~ | **(철회)** 진입 에이전트가 노드를 품을 이유가 없어 편성 제약이 불필요하다. 마지막 노드 배치는 성능 최적화로만 남는다(D-51) |
| Q-37 | 컨트롤러가 진입점이 여럿인 배치(내부망 여러 개)를 상대해야 하는가 | 전제 11은 단일 진입점을 명시한다. 확장 필요 여부만 확인 |
| Q-38 | 중계 메시지도 큐 레인을 소비하는가, 별도 경로인가 | P-35. 중계량이 많으면 Control 레인 예산을 잠식한다 |
| Q-32 | 체인 중간 노드 장애 시 재시작 단위는 무엇인가 — 요청 전체인가, 프리필부터인가 | P-32. KV가 노드 귀속이므로 이전은 불가능하다 |
| ~~Q-24~~ | ~~적재 시점 구성과 요청 시점 체인이 불일치하면 어떻게 하는가~~ | **(결정)** OUTER 책임. 에이전트는 자기 상태와 맞지 않는 지시에 실패를 보고할 뿐 대조·보정하지 않는다 (Q-33과 같은 근거) |

## 90. wire 버전 결정

**P4B1 v6으로 간다. v5 호환은 포기한다.** 개별 판단이 아니라 누적된 결과다 — 아래 중 어느 하나만 채택해도 frame 또는 필드 구조가 바뀐다.

| 근거 | 항목 |
|---|---|
| routed envelope에 대상 주소 자리가 없다 | P-34 |
| `Participant.agent_id`·`HardwareReport.agent_id` 제거 | P-2 |
| lifecycle에서 `controller_id` 제거, 방향 재정의 | P-6 |
| `NODE_DELETE`/`NODE_DELETED` 신설 | P-7 |
| 스테이지 진입·완료 보고 신설 | P-28 |
| `EXECUTE`가 체인·귀환 주소를 나른다 | P-21·P-22·P-26 |
| `prompt` 단일 문자열 → 대화 구조 | P-42 |
| agent-initiated announce (채택 시) | P-4 |

따라서 "v5와의 호환을 위해"라는 이유로 남겨둘 표면은 없다. 미사용 표면(§94.3)도 같은 판에서 정리한다.

**v6에서 사라지는 것:** `agent_id`, lifecycle의 `controller_id`, `machine_id`(도입되지 않음), 에이전트의 `session_id` 발급.
**v6에서 생기는 것:** 대상 주소, 체인, 귀환 주소, 노드 제거, 스테이지 보고, 대화 구조.

## 91. 구축 순서

### 91.0 재작성 대 수정 (판단 기록)

"기존 구조를 버리고 새 프로젝트에서 구현 코드만 참고해 재작성"을 검토한 결과다. **결론: 프로젝트를 새로 만들지 않는다. 재작성 대상은 `layers/runtime` 하나다.**

**코드 분포**

| 계층 | 줄 수 | 계획서가 요구하는 변경 |
|---|---:|---|
| `layers/protocol` | 1,330 | 필드 수술 — 코덱 원시 연산은 존속 |
| **`layers/runtime`** | **3,489** | **거의 전면** — 주제 E + K가 사실상 재작성 |
| `layers/adapters/adapter` | 2,159 | `P-40` 한 파일, `P-45` 필드, 이름 정정 |
| `layers/adapters/llamacpp` | 986 | 거의 무변경 |
| `tools` | 1,400 | v6 대응, 구조 유지 |

파괴적 변경은 `layers/runtime`(전체의 약 29%)에 몰려 있다. 어댑터 3,145줄은 거의 그대로 살아남는다.

**재작성을 권하지 않는 근거**

1. **버릴 것을 다시 쓰려고 지킬 것까지 버리는 거래가 된다.** `apps/p4`+`apps/llama/native` 커밋 61개 중 22개가 fix·revert로 **36%**다. 그 흉터는 runtime이 아니라 **어댑터와 네이티브**에 있다 — `capacity/mod.rs`의 "스로틀이 GPU에서 세 단계 위에" 주석, 스테이지 터미널 수정, 마이크로배치 게이트 revert. 그리고 그 지식은 코드 모양이 아니라 **주석·문서·커밋 메시지**에 있다. "구현 코드만 참고"가 정확히 그 층을 버리는 방식이다
2. **재작성이 주는 자유를 순차 수정이 이미 갖고 있다.** v6 파괴가 허용되어 있고 외부 소비자가 우리 도구뿐이라 호환 부담이 없다
3. **미결 40건 중 셋(`Q-47`·`Q-48`·`Q-49`)이 다른 세션의 측정에 종속된다.** 재작성은 아무것도 돌기 전에 전부 결정해야 한다. 순차 수정은 단계 0이 `Q-47`을, TPS 세션이 `Q-48`·`Q-49`를 답하는 동안 진행된다. **재작성은 질문을 없애지 않고 답하는 시점만 앞당기며, 그동안 "돌려보고 안다"는 검증 수단을 잃는다**
4. **병행 TPS 세션이 같은 트리의 네이티브를 고치고 있다.** 지금 포크하면 가장 나쁜 시점에 갈라진다
5. **추상층 판정은 "대체로 맞다"였다.** vLLM 검증에서 의존 방향이 깨끗했고 누출은 계약 필드 3~4개·문서 하나·크레이트 이름 하나·디스패치 계층이었다

**실행 형태:** `layers/runtime` 아래 새 모듈을 세우고 옮겨 붙인 뒤 기존을 삭제한다. 단계 1과 주제 K를 합치면 그것이 곧 runtime 재작성이며, **wire 불변이라 그동안 시스템이 돈다.**

**재작성으로 뒤집을 조건**
- 단계 1·2가 실제로 wire 불변이 아닌 것으로 드러날 때
- 미결이 어댑터까지 무효화하는 방향으로 결론날 때 (예: `Q-51`에서 어댑터 인터페이스가 지금 어댑터와 근본적으로 다른 형태로 결정될 때)
- 병행 TPS 작업이 종료되어 네이티브가 얼어붙을 때. 포크 비용이 사라진다

---

**순차 구축이다.** 각 단계는 그것만으로 완결되고, 끝난 시점에 시스템이 동작하며, 뒤 단계의 존재를 전제하지 않는다. 단계 경계는 **wire 호환성**으로 긋는다 — v5를 유지한 채 할 수 있는 것을 모두 먼저 끝내고, 그 다음에 v6로 넘어간다.

| 단계 | 성격 | wire | 끝난 시점의 상태 |
|---:|---|---|---|
| 0 | 어댑터 단독 | v5 | 구조화 출력이 동작한다 |
| 1 | 런타임 내부 | v5 | 제어 평면이 CPS가 된다 |
| 2 | 의미 정리 | v5 | 노드 상태 기계가 성립한다 |
| 3 | frame 확장 | **v6** | 주소가 자기기술되고 중계가 동작한다 |
| 4 | 소유·방향 | v6 | OUTER→에이전트 제어가 성립한다 |
| 5 | 표현력 | v6 | 런타임 스펙을 온전히 전달한다 |
| 6 | 체인 | v6 | 인퍼런스 경로가 프로토콜에 표현된다 |

---

### 단계 0 — 어댑터 화이트리스트 제거

| 항목 | 내용 |
|---|---|
| P-40 | Pipeline 어댑터의 5개 화이트리스트 제거, 불투명 통과 |
| P-41 | 미지원 키를 조용히 버리지 않고 `ERROR` |
| P-46 | `docs/model-load.md`를 정식 스키마에서 예시로 격하 |
| P-47 | `adapter` → `pipeline` 이름 정정 (크레이트·디렉터리) |
| P-53 | 백엔드(upstream·compat·scripts)를 어댑터 아래로 이동 |
| P-54 | 완결형·스테이지별 upstream 정책 명문화 |
| P-55 | upstream 결함 패치 2건을 공식에 기여 |

`P-53`은 `P-47`과 같은 이동 작업이므로 함께 처리한다. `P-55`는 외부 반영 시점이 우리 통제 밖이므로 착수만 이 단계에서 한다.

**wire 변경 없음. 다른 단계와 의존 없음.** 파일 하나(`adapter/.../execution/options`)이며 stock llama.cpp 어댑터가 이미 정답 형태이므로 그것을 따른다.

**이 단계만으로 D-56(구조화 출력 원리적 불가)이 풀린다.** 순서상 가장 먼저 둘 이유가 여기 있다.

**관문:** Q-47(하위 런타임의 실제 수용 범위). 걷어낸 뒤 `ERROR`가 늘면 런타임 쪽 작업이 뒤따른다.

---

### 단계 1 — 제어 평면을 CPS로 (wire 불변)

**wire를 건드리지 않는다.** 외부에서 본 프로토콜은 그대로이고 내부 처리 구조만 바뀐다. 따라서 기존 클라이언트·벤치 도구가 그대로 동작한다.

순서가 있다.

**1-a. 어댑터 경계를 지속 다중화로 (P-18)**
`TcpTransport`를 `peer_mux` 기반으로 교체한다. 현재 호출마다 새 소켓을 여는 것(D-27)을 없앤다. 프레임 형식은 v5 그대로이고 어댑터 측 listener도 이미 routed frame을 처리하므로 **양쪽 모두 무변경**이다.

**1-b. 출력 경로를 큐로 통일 (P-17·P-14)**
`forward::capture`를 폐기한다. lifecycle 여섯 핸들러가 어댑터 응답을 반환값으로 받아 분기하던 것을, 후속 Task로 재진입시키는 형태로 바꾼다. 사전 조건은 emit 이전에 완결한다.

이 시점에 `D-22`(한 route에 terminal 두 번)와 `D-28`이 해소된다. `dispatch::compatibility` 경로를 제거한다.

**1-c. 장기 작업을 다단 Task로 (P-19)**
적재를 `시작 → 진행 관측 → 완료 판정`으로 쪼갠다. 600초 블로킹 폴링 루프(D-30)가 사라지고 진행 상태가 큐에 나타난다.

**1-d. 데드라인·취소를 워커 루프에 (P-20)**
Task 단위로 쪼개졌으므로 각 단계 진입 시 데드라인을 검사할 수 있고, `CANCEL`은 다음 단계 enqueue를 막는 방식으로 도달한다. `D-31`·`D-32`·`D-33` 해소.

**관문:** Q-20(토대 trait 교체 대 어댑터 씌우기), Q-18(어댑터 내부까지 CPS로 할 것인가), Q-19(폴링 단 수).

---

### 단계 2 — 노드 의미 정리 (wire 불변)

필드 구조는 그대로이고 **동작 규칙만** 바뀐다. 호출자에게는 거부가 늘어나는 형태로 나타난다.

| 항목 | 내용 | 관문 |
|---|---|---|
| P-16 | `NodeSlot.bindings`를 `HashMap` → `Option<Binding>` | **Q-15** |
| P-15 | 상태 기계 `empty`/`bound`/`active` 강제 | Q-5, Q-8 |
| P-8 | `plan_revision` 기록·비교 활성화 | — |
| D-21 | 어댑터의 HTTP 404 성공 처리 제거 | — |
| D-23 | 적재된 노드에 대한 적재를 실패로 | Q-15 |
| D-54 | `session_id` 빈 값 금지, 두 발급기 제거 | — |

단계 1의 `P-14`가 선행되어야 한다. 사전 조건 검사가 emit 이전에 끝나지 않으면 상태 기계가 "성공 통지 후 거부"를 낳는다.

**Q-15가 이 단계 전체의 선행 조건이다.** 1:1이 아니면 "이미 적재됨" 판정이 정의되지 않는다.

---

### 단계 3 — v6 frame: 자기기술 주소와 중계

**여기서 wire가 깨진다.** 클라이언트(`agent-link.mjs`, `controller-instance.mjs`)와 벤치 도구를 같은 단계에서 함께 옮긴다.

**3-a. 봉투에 대상 주소 (P-34)**
routed envelope에 대상 에이전트 접속 정보를 추가한다. 이 시점에는 모든 대상이 자기 자신이므로 **동작은 변하지 않는다.** 필드만 자리를 잡는다.

**3-b. `agent_id` 폐지 (P-2)**
`Participant.agent_id`·`HardwareReport.agent_id`·`agent-{host}-{pid}` 생성을 제거하고 주소로 대체한다. `is_local_bypass`가 주소 비교가 된다.

**3-c. 봉투·본문 디코드 분리 (P-48·P-51)**
봉투(route·대상·분류·데드라인)만 먼저 파싱하고 본문은 지연한다. 큐 분류를 봉투가 나른다. 중계가 본문을 해석하지 않게 하는 전제다.

**3-d. 중계를 큐 디스패치에 내장 (P-35·P-36)**
워커 루프의 핸들러 호출 직전에 판정을 넣는다. 대상이 자신이면 핸들러로, 아니면 다음 홉으로 큐잉한다. 3-a와 3-c가 끝나 있어야 판정 근거가 존재하고 본문을 건드리지 않는다.

**이 단계가 끝나면 전제 11의 방화벽 배치가 성립한다.** 진입 에이전트를 거쳐 내부망 에이전트에 도달할 수 있다.

**관문:** Q-39(주소 표기 형식), Q-38(중계가 레인 예산을 잠식하는가), Q-42(화신 표식).

---

### 단계 4 — 소유와 방향 재편

| 항목 | 내용 | 관문 |
|---|---|---|
| P-6 | lifecycle에서 `controller_id` 제거, `TaskDirection` 재정의 | Q-6, Q-7 |
| P-37 | 컨트롤러·에이전트의 relay 책임 계약화 | — |
| P-7 | `NODE_DELETE`/`NODE_DELETED` 신설 | Q-8 |
| P-12·P-13 | 적재 지시·보고의 방향 전환 | — |
| D-19 | 호스트 API의 `controller_id` 잔재 제거 | — |
| D-20 | route 단절 시 적재 진행·완료 복구 | Q-14 |

단계 3의 중계가 있어야 "OUTER→진입 에이전트→대상 에이전트" 경로가 실제로 성립한다. 단계 2의 상태 기계가 있어야 `NODE_DELETE`의 사전 조건이 정의된다.

---

### 단계 5 — 표현력 (병행 가능)

단계 3 이후 서로 독립적이며 병행할 수 있다.

| 항목 | 내용 | 관문 |
|---|---|---|
| P-42·P-43 | 대화 구조, wire 생성 필드 정리 | Q-44, Q-45 |
| P-9·P-10 | 적재 옵션 불투명 통과, 사전 발견 수단 | Q-9 |
| P-1·P-3 | capability/occupancy 분리, 스냅샷 스키마 | Q-1, Q-2 |
| P-4 | agent-initiated announce | Q-3 |

---

### 단계 6 — 체인

가장 마지막이다. 앞의 모든 단계를 전제한다 — 자기기술 주소(3), 중계(3), 방향 규칙(4), 다단 Task(1).

**6-a. 체인을 메시지에 (P-21·P-22·P-25·P-26)**
`EXECUTE`가 순서 있는 노드 리스트와 귀환 주소를 나른다. 각 항목은 4-튜플(§1.3)로 자기 완결적이다.

**6-b. 스테이지 보고 (P-28·P-29)**
진입·완료 보고와 통계. OUTER의 상태 관측이 여기서 성립한다.

**6-c. 디코드 루프 (P-31·P-27·P-32)**
링 구조, `phase` 구분, 체인 취소 전파, KV 귀속 명문화.

**네이티브 측 작업:** Pipeline 런타임이 요청마다 체인을 받아야 한다. 현재는 적재 시점 deployment에 스테이지 구성이 고정된다(D-35). 다만 **에이전트·노드는 기계적으로 남는다** — 지시가 자기 상태와 맞지 않으면 실패를 보고할 뿐이고, 체인의 정합성은 OUTER가 책임진다(P-33, Q-24·Q-33 결정).

**관문:** Q-26, Q-27, Q-28, Q-29, Q-30, Q-32, Q-46.

---

### 단계 간 되돌림

단계 0~2는 wire 불변이므로 개별 되돌림이 가능하다. 단계 3부터는 v6이므로 **단계 3 이전으로 되돌리려면 클라이언트도 함께 되돌려야 한다.** 실질적 되돌림 경계는 단계 2와 3 사이 하나다.

## 92. 처리량과의 관계

### 처리량의 소재 — P4가 소유하지 않는다

이 저장소의 판단 기준은 단일 세션 대비 **집계 TPS**다. 그러나 **처리량은 P4 계층에서 결정되지 않는다.** 최적화가 일어나는 곳은 노드 큐와 그 아래 물리 구상층이다.

| 층 | 소재 | 무엇을 결정하는가 |
|---|---|---|
| 노드 큐 | [`listener/queue.rs`](layers/adapters/adapter/src/infrastructure/listener/queue.rs) | 배치 합치기(`batch_coalesce_ms`), 디코드 credit, 동적 배치 구성 |
| 용량 게이트 | [`capacity/mod.rs`](layers/adapters/adapter/src/domain/capacity/mod.rs) | deployment별 동시 시퀀스 상한 |
| 물리 구상층 | `apps/llama` 네이티브 | 마이크로배치, 스테이지 중첩, 스테이지 터미널, 레이어 점유 |

`capacity/mod.rs`의 주석이 이 경계를 직접 서술한다 — 프로세스 전역 상수로 게이트를 두었을 때 "스로틀이 GPU에서 세 단계 위에" 있었고, 그래서 네이티브가 `limit=50`을 보고하는데 실제로는 16개만 도착했다.

**따라서 이 개편은 처리량 노력과 배치되지 않는다.** 서로 다른 층을 만지며 경쟁하지 않는다.

### P4의 의무는 둘뿐이다

처리량을 **올리는** 것이 P4의 일이 아니다. P4가 지는 책임은 다음 둘이다.

**1. 처리량을 소유한 층이 필요로 하는 선언을 온전히 전달한다.**
현재 이 의무가 깨져 있다. 용량 게이트는 `stage_plan.load_options.batching.max_sequences`를 읽는데(capacity/mod.rs), 인퍼런스 옵션은 Pipeline 어댑터가 5개만 통과시킨다(D-55). **배치·투기 디코딩처럼 처리량에 직결되는 옵션이 프로토콜 중간에서 사라진다.**

`P-40`·`P-9`가 이 의무의 이행이며, 이것이 개편과 처리량이 만나는 **유일하고 정확한 접점**이다. 프로토콜이 TPS를 올리는 게 아니라, 올릴 수 있는 층에 손잡이를 온전히 넘겨준다.

**2. 토큰 경로를 불필요하게 무겁게 하지 않는다.**
아래 "비용" 항목이 이에 해당한다.

### 부수적 이득

| 항목 | 효과 |
|---|---|
| P-18 | `TcpTransport`가 **호출마다 새 소켓을 연다**(D-27). 지속 다중화로 제어 경로의 연결 비용이 사라진다 |
| P-19 | 적재가 블로킹 풀을 점유하지 않게 되어 동시 적재의 확장성이 생긴다 |

### 비용

| 항목 | 비용 | 판단 |
|---|---|---|
| D-51 | 마지막 노드가 진입 에이전트 밖이면 토큰마다 중계 홉 +1 | 편성으로 회피 가능 — 마지막 노드를 진입 에이전트에 배치 |
| P-22 | 스테이지 이동마다 P4 `EXECUTE` 홉 | hidden state는 native 유지이므로 제어 프레임만 |
| Q-26 | 체인 전체를 매 홉 복제하면 프레임이 커진다 | 잔여 구간만 전달하는 선택지 |
| Q-38 | 중계가 Control 레인 예산을 잠식 | 레인 분리 |

### 스테이지 보고는 비용으로 보지 않는다 (결정)

`P-28`의 진입·완료 보고를 토큰 경로 부하로 계상하지 않는다. 근거 둘.

- 매우 작은 신호다. hidden state도 토큰도 아니고 correlation과 통계뿐이다
- **이것이 없으면 OUTER는 각 노드의 상태와 동작에 대한 통계를 수집할 수 없다.** 관측 가능성이 사라지는 대가가 신호 비용보다 크다

프리필 파이프라이닝(코호트 윈도 분할, 프리필 중 도착률 유지)이 최근 최적화된 경로이므로 보고가 그 위에 얹히는 것은 사실이나, 신호 크기를 고려하면 실질 간섭으로 보기 어렵다. Q-30은 여전히 열려 있으나 "부하 때문에 줄인다"는 근거로는 판단하지 않는다.

### 측정이 필요한 것

- **Q-47** — Pipeline 런타임이 실제로 받는 샘플러 범위. 화이트리스트를 걷어내도 하위가 못 받으면 `ERROR`만 늘어난다. 단계 0의 직후 확인 대상이다
- **Q-26** — 체인 복제의 프레임 증가량. 주소 표기 길이에 비례하므로 Q-39 확정 후 산출한다

### 병행 세션과의 접점

처리량 최적화는 별도 세션에서 진행 중이다 — Mac+GB10 조합과 MI250에서 각각 실측을 취합한다. 이 개편은 그 층을 만지지 않지만 **두 곳에서 만난다.**

| 접점 | 내용 | 방향 |
|---|---|---|
| `D-60`·`P-44`·`Q-48` | 적재 시점 `max_sequences` 선언이 런타임 파생 값을 게이트로 고정한다 | **타 세션 결론을 기다린다.** 스케줄러가 폭을 코호트에서 파생하면 이 선언은 강등 대상 |
| `D-61`·`Q-49` | 링 형성 중 연결 실패·백오프·연결 예산 | 체인이 프로토콜로 올라오면 연결 정책도 계약이 된다 |

**둘 다 이 문서가 단독으로 확정하지 않는다.** 프로토콜이 처리량 층의 결정을 앞질러 못 박는 것이 D-60이 지적하는 실패 형태이므로, 같은 실수를 계획 단계에서 반복하지 않는다.

### 이 문서가 다루지 않는 것

노드 큐의 배치 정책, 마이크로배치 크기, 스테이지 중첩, 레이어 점유는 **이 개편의 대상이 아니다.** 처리량 작업은 그 층에서 독립적으로 진행되며, 두 작업은 서로를 막지 않는다.

역으로 **처리량 문제를 프로토콜 변경으로 풀려 하지 않는다.** 스로틀이 GPU에서 멀어질수록 실제 도착량과 보고된 한도가 어긋난다는 것이 이미 확인된 사실이고(capacity/mod.rs), P4에 게이트를 추가하는 것은 그 실수를 반복하는 일이다.

## 93. 파급 범위 (Q 확정 후 상세화)

| 대상 | 예상 변경 |
|---|---|
| [`domain/hardware/mod.rs`](layers/runtime/src/domain/hardware/mod.rs) | 전면 재작성. vendor 중립 probe 분리, RAM·저장소·NUMA 추가 |
| [`contract/message/mod.rs`](layers/protocol/src/contract/message/mod.rs) | Q-2·Q-3 신규 kind, `NODE_DELETE`/`NODE_DELETED`, lifecycle의 `controller_id` 제거, Q-11에 따라 artifacts 필드 |
| [`catalog/mod.rs`](layers/protocol/src/catalog/mod.rs) | 신규 kind의 class/queue/direction, `allows_direction` 재작성 |
| [`task/mod.rs`](layers/protocol/src/task/mod.rs) | `TaskDirection` 확장 (외부→에이전트, Q-3 채택 시 에이전트→외부) |
| [`registry/node/mod.rs`](layers/runtime/src/domain/agent/registry/node/mod.rs) | `NodeSlot.controller_id` 제거, 제거 연산 추가 |
| [`authorization/mod.rs`](layers/runtime/src/domain/agent/authorization/mod.rs) | `ForeignController` 삭제, 게이트 축소 |
| [`lifecycle/mod.rs`](layers/runtime/src/domain/agent/lifecycle/mod.rs) | 재생성 의미 확정(Q-5), `plan_revision` 기록 |
| [`adapters/adapter/.../lifecycle/load`](layers/adapters/adapter/src/application/lifecycle/load/mod.rs) | 옵션 검증 도입(Q-13), `controller_id` 잔재 제거 |
| [`adapters/llamacpp/.../model_load_options`](layers/adapters/llamacpp/src/application/model_load_options/mod.rs) | 전부 거부에서 선별 해석으로. 미지원 키는 명시적 `ERROR` |
| [`adapter/.../execution/options`](layers/adapters/adapter/src/application/execution/options/mod.rs) | **5개 화이트리스트 제거.** 불투명 통과로 전환 (P-40, D-55) |
| [`contract/execution/mod.rs`](layers/protocol/src/contract/execution/mod.rs) | `prompt` 단일 문자열 → 대화 구조, wire 생성 필드 정리 (P-42, P-43) |
| `ADAPTER_REGISTER.descriptor` 계약 | Q-9 채택 시 지원 옵션 키 집합 선언 (P-10) |
| [`controller-instance.mjs`](tools/controller/client/controller-instance.mjs) | 조회·lifecycle·적재 surface 갱신 |
| [`docs/model-load.md`](docs/model-load.md) | 전면 개정. D-16의 서술 불일치 포함 |
| [`docs/message-pairs.md`](docs/message-pairs.md) | pair 표 갱신 |
| `apps/llama` 네이티브 Pipeline 런타임 | 요청마다 체인을 수용 (단계 6). 스테이지 구성이 적재 시점 deployment에 고정된 것을 요청 단위로 (D-35) |
| `apps/llama/upstream` · `native/compat` · `scripts` | 어댑터 아래로 이동 (P-53). upstream은 pristine 유지, 패치·manifest·준비 스크립트가 함께 간다 |
| `native/compat/<sha>/0004`·`0006` | ABI 노출 전환 검토 (P-56, Q-56). 패치 총량의 58% |
| [`tools/controller/evidence`](tools/controller/evidence) | v6 전환 시 벤치 도구를 단계 3과 함께 이동 |
| [`foundation/transport/mod.rs`](layers/runtime/src/foundation/transport/mod.rs) | **토대 계약 교체.** `P4Handler`/`P4Transport`의 동기 완료 시그니처 제거 (D-26, Q-20) |
| [`task_queue/worker/mod.rs`](layers/runtime/src/foundation/task_queue/worker/mod.rs) | **중계 판정을 워커 루프에 내장** — 핸들러 호출 직전 (P-35). 데드라인 검사도 같은 지점(P-20) |
| [`protocol/task/mod.rs`](layers/protocol/src/task/mod.rs) | `is_local_bypass`의 대칭으로 중계 판정 추가, `Participant.agent_id`를 접근 주소로 대체 (P-2, P-34) |
| [`contract/message/mod.rs`](layers/protocol/src/contract/message/mod.rs) | `HardwareReport.agent_id` 제거 (P-2) |
| [`domain/agent/mod.rs`](layers/runtime/src/domain/agent/mod.rs) | `agent-{host}-{pid}` 생성 제거. 필요하면 화신 표식으로 대체 (P-39, Q-42) |
| [`lifecycle/forward/mod.rs`](layers/runtime/src/domain/agent/lifecycle/forward/mod.rs) | 폐기. 후속 Task 재진입으로 대체 (P-17) |
| [`domain/agent/lifecycle/mod.rs`](layers/runtime/src/domain/agent/lifecycle/mod.rs) | 여섯 핸들러를 다단 Task로 재작성 (D-29, P-19) |
| [`application/dispatch/mod.rs`](layers/runtime/src/application/dispatch/mod.rs) | `compatibility` 경로 제거, 데드라인·취소를 전 경로에 (P-20) |
| [`adapters/adapter/.../lifecycle/load`](layers/adapters/adapter/src/application/lifecycle/load/mod.rs) | 600초 폴링 루프를 단계 Task로 분해 (D-30, Q-18) |

## 94. 잔여 주제

### 94.1 상태 외재화 — 경계 확정 (결론)

회의 전체에 걸쳐 결정이 누적되어 이 주제는 **경계가 확정되었다.** 별도 주제로 열 필요가 없다.

**권위 모델:** OUTER 단독 권위다(전제 1). "외부 저장소 권위 + 에이전트 조정" 대 "에이전트 권위 + 외부 투영"의 선택지는 소멸했다 — 식별자 발급(§1.3), 편성 의도(P-5), 배치 구조(P-33)가 모두 OUTER로 확정되었기 때문이다.

**외재화되는 것:** 노드 목록, 배치, 적재 계획, 체인 구성, 모든 업무 식별자, 에이전트 접근 주소.
**외재화되지 않는 것과 그 이유:**

| 항목 | 이유 | 근거 |
|---|---|---|
| occupancy (free VRAM 등) | 프로세스에 관한 사실이지 레코드가 아니다 | P-1 |
| KV | 요청 수명 동안 노드 귀속 | P-32 |
| 실행 credit (세마포어) | 물리 점유 | 주제 A §admission |
| 소켓·전송 핸들 | 프로세스 로컬 | P-17 |
| 프로세스 화신 | 세대 표식이지 상태가 아니다 | P-39 |

**처리량 가드레일:** 전제 11의 결과로 자동 충족된다. OUTER는 애초에 실행 경로에 없다 — 실행 중 OUTER가 관여하는 것은 토큰 반환뿐이고, 편성·적재는 모두 실행 이전이다. "외부 접근을 실행 경로에서 배제한다"는 규칙을 따로 강제할 필요가 없다.

**남은 미결:** Q-42(에이전트 상태 지속화 여부)와 그 경우의 재시작 조정 절차. 이는 외재화 범위의 문제가 아니라 **에이전트 측 복구**의 문제다.

### 94.2 연속성(continuation) 외재화 — 범위 축소 (결론)

`route_id` 키 in-process 맵 5개(`recipients`, `ingress_routes`, `prepared`, `active`, `deliveries`)를 봉투로 옮기는 문제였다. 단계 1·4를 거치면 **상당 부분이 소멸한다.**

| 맵 | 처분 |
|---|---|
| `prepared` | P-19의 다단 Task화로 소멸. 준비 상태가 Task 사이에 머물 이유가 없어진다 |
| `active` | P-20의 취소 재설계로 대체. 취소는 다음 단계 enqueue를 막는 방식이 된다 |
| `deliveries` | CPS 내부 추적이므로 프로세스 로컬로 남는다 |
| `recipients`·`ingress_routes` | **남는다.** route 소유권은 살아 있는 연결이며 직렬화 대상이 아니다 |

`AsyncExecution`을 "직렬화 가능한 실행 의도"와 "프로세스 로컬 핸들"로 분리해야 한다는 요구는 유효하며, P-17·P-19가 그 작업이다.

**결론적 비대칭 하나:** KV가 노드 귀속이므로(P-32) **인퍼런스 연속성은 외재화할 수 없다.** 체인 중간 노드가 죽으면 그 요청은 재시작이지 재개가 아니다(Q-32). 외재화 대상이 되는 것은 **제어 연속성**뿐이다 — 적재의 진행·완료를 route 단절 후 되찾는 문제(D-20, Q-14).

이 비대칭을 인정하면 continuation 외재화는 별도 대개편 주제가 아니라 단계 1·4의 부수 효과다.

### 94.3 미사용 표면 정리 (D-37·D-46으로 정정 완료)
이 절은 더 이상 "삭제 판단" 대상이 아니다. 둘 다 **구현 대상**으로 확정되었다.
- **`NodeNode` 방향** — §1.2 인퍼런스 경로의 본체 (D-37, P-22)
- **`phase=DECODE`** — 노드 주도 디코드 루프에 필수. 프리필 홉과 순환 형태가 다르다 (D-46, P-31)

### 94.4 아직 주제로 열지 않은 것

대개편 항목은 모두 짚었으나 다음은 별도 판단이 남아 있다.

- **`INGRESS_ACCEPTED` 의미의 3분기** (검증 전 / credit 전 / credit 후). 단계 1의 P-14로 구조적 원인은 사라지지만, **어느 시점을 계약으로 삼을지**는 정해야 한다. Q-43(`ingress_id` 존치)과 함께 보면 `INGRESS_ACCEPTED` 자체의 필요성까지 재검토 대상이다
- **wire authz와 TLS.** D-53이 자기기술 주소와 겹치는 위험을 기록했으나 이 개편의 범위 밖이다. 도입 시 Q-41을 함께 판단한다
- **다중 컨트롤러·다중 진입점.** Q-37. 전제 11은 단일 진입점을 명시하므로 현 개편은 이를 가정한다
- **에이전트 상태 지속화.** Q-42. §94.1의 남은 미결

※ `session_id` 발급 문제는 D-54로 격상되어 이 절에서 빠졌다.

## 95. 개정 이력

| 날짜 | 내용 |
|---|---|
| 2026-08-13 | 초판. 주제 A(하드웨어 조회) — D-1~D-8, P-1~P-4, Q-1~Q-4 |
| 2026-08-13 | 주제 B(노드 소유권) 추가 — D-9~D-13, P-5~P-8, Q-5~Q-8. 전제 3 추가 |
| 2026-08-13 | **철회:** 초판의 "`runtime_generation`을 외부 desired-state 레코드의 버전으로 재정의" 항목. 노드 실체가 어댑터에 있다는 전제 3과 모순된다. P-8의 두 축 분리로 대체 — 외부 의도는 `plan_revision`, 실체 세대는 `runtime_generation` |
| 2026-08-13 | 주제 C(모델 적재) 추가 — D-14~D-19, P-9~P-12, Q-9~Q-13. 전제 4 추가. upstream `common/arg.cpp`(고정 커밋 `3e3a7a4`, 347 `add_opt`) 대조 |
| 2026-08-13 | 전제 5·6 추가. **결정:** 적재 옵션은 불투명 문자열 통과, 해석은 구상 어댑터 — P-9 재작성, Q-13 결정, Q-10·Q-11·Q-12 철회, D-14·D-15에 해소 경로 기재. P-10은 "사전 발견 수단" 잔여 문제로 축소 |
| 2026-08-13 | 전제 5(적재 보고의 외부 전송) 반영 — P-13 추가. 방향 전환 자체는 재라벨링이며 실질 작업은 route 단절 시 복구(D-20, Q-14) |
| 2026-08-13 | 주제 D(사전 조건과 상태 기계) 추가 — D-21~D-25, P-14~P-16, Q-15~Q-17. 전제 7·8 추가. `admission::lifecycle`로 `active` 차단은 이미 구현되어 있음을 확인 |
| 2026-08-13 | 주제 E(CPS 전면 감사) 추가 — D-26~D-33, P-17~P-20, Q-18~Q-20. 전제 9 추가. 절 번호 중복(주제 D 삽입 시 발생) 정정, 주제 E 이후를 §17~§20으로 재배치 |
| 2026-08-13 | **OUTER 명명**과 §1.2 대상 아키텍처 흐름 추가 — 제어 경로와 인퍼런스 경로를 분리 기술. 주제 F(인퍼런스 경로와 체인) 추가 — D-34~D-37, P-21~P-24, Q-21~Q-24 |
| 2026-08-13 | 체인 전달 방식 확정 — 컨트롤러가 `Node[0]`에 보내는 프리필 메시지가 체인 전체를 담고 노드가 스스로 전달하는 **소스 라우팅**. §1.2 도식 수정, P-22 재작성, P-25~P-27 추가, D-38~D-40 추가, Q-21·Q-22 결정 처리 |
| 2026-08-13 | 주제 G(스테이지 보고와 디코드 루프) 추가 — D-41~D-46, P-28~P-32, Q-28~Q-32. 프리필을 상태 확인 단계로 규정. 디코드 루프의 노드 주도 순환을 §1.2 도식에 반영. KV의 노드 귀속을 전제 2의 외재화 예외로 명문화(P-32) |
| 2026-08-13 | **전제 11 정정 — 진입 에이전트는 노드 배치와 무관한 순수 진입점.** "1번 노드의 에이전트가 관문"이라는 초안을 폐기. 제약은 ① 컨트롤러가 도달하는 에이전트는 정확히 하나 ② 그 에이전트는 나머지 전부에 도달, 둘뿐이다. §1.2 위상·인퍼런스 도식과 역할 표 재작성, P-26·P-35·P-36 수정, D-49를 "중계 능력 부재"로 재정의, D-50·Q-34 해소, Q-36 철회, Q-38 추가 |
| 2026-08-13 | 중계를 **모든 에이전트의 일반 능력**으로 확정 — 자기가 수행할 메시지 외에 다른 에이전트로 단순 전달하는 처리 경로. P-35 재작성. "진입 에이전트"는 역할이 아니라 위상상의 위치이므로 `ParticipantRole`을 늘리지 않는다 |
| 2026-08-14 | **분할 기준을 방화벽 위치로 확정(P-60).** 관심사가 아니라 "방화벽 어느 쪽에서 필요한가"로 가른다 — 계획 지식(모델 형식 검사·배치 휴리스틱·용량 추정)은 방화벽 밖에서 OUTER가 **소비**하고, 실행 지식(기동·적재·추론)은 방화벽 안에서 어댑터가 **소유**한다. **`apps/llama`는 해체하지 않고 계획 지식 제공자로 존속**한다(P-57 재작성) — 에이전트가 쓸 구상 어댑터만 넘긴다. 이로써 **D-77 해소**: 계획 지식을 `linker_domain`에 넣지 않으므로 OUTER 코어가 형식 무지로 남고, 백엔드가 늘어도 부풀지 않는다. Q-62 해소, Q-64·Q-65 추가. 종료 상태가 2앱에서 3앱으로 바뀌나 목표(P4의 백엔드 소유)는 그대로 달성된다. 기준선이 기존 이음매와 일치함을 확인 — `planner` 3,182줄(common의 52%) 대 `protocol`+`pipeline-*` 2,840줄, `/api/models\|plans\|resources` 대 `/api/processes\|runtime*` |
| 2026-08-14 | 주제 M(저장소 종료 상태) 추가 — D-75~D-78, P-57~P-59, Q-60~Q-63. 브랜치 종료 시 **`apps/linker`와 `apps/p4`만 남고 `apps/llama`는 해체**된다. **역할 매핑 확정: OUTER = `apps/linker` + `packages/linker_domain`** — 이로써 capability 레코드·편성·적재 계획·체인 구성·식별자 발급의 귀속이 정해진다. Controller/Agent/Node는 `apps/p4/entrypoints/*`에 이미 존재. 해체 규모는 native 134파일·src 56파일·scripts 28파일과 **`packages/llama_domain` 11,631줄**이며 후자는 배치 휴리스틱·모델 검사(→OUTER)와 런타임 검증·기동 정책(→어댑터)으로 3분할된다. Q-59는 주제 M으로 이관 |
| 2026-08-14 | 주제 L(백엔드 소유와 upstream 추적) 추가 — D-71~D-74, P-53~P-56, Q-56~Q-59. **검증 결과 "항상 최신 풀받고 필요한 기능만 붙여 컴파일"은 이미 구현되어 있다** — pristine 서브모듈, `native/compat/<sha>/`의 해시 검증된 순서 패치, `.cache/` worktree 적용(생성물 미커밋), stock은 무패치 빌드, 우리 C++는 공개 헤더만 include. 남은 문제는 **소유 위치**(어댑터와 백엔드가 다른 앱에 분산)와 **추적 비용**(패치 2,219줄 중 `0004`·`0006`이 1,287줄로 58%)이다. 완결형은 upstream 개조 0(vLLM은 pip pin으로 끝난다), 스테이지만 `compat/` 필요라는 정책을 P-54로 명문화. upstream 결함 패치 2건(138줄)은 공식 기여로 소멸 대상 |
| 2026-08-14 | **§91.0 재작성 대 수정 판단 기록.** 신규 프로젝트 재작성을 검토하고 **권하지 않는 것으로 결론.** 파괴적 변경은 `layers/runtime`(3,489줄, 전체의 29%)에 몰려 있고 어댑터 3,145줄은 거의 존속한다. 흉터(커밋 61개 중 fix·revert 22개 = 36%)는 runtime이 아니라 어댑터·네이티브에 있으며 코드 모양이 아니라 주석·문서·커밋 메시지에 담겨 있어 "구현 코드만 참고"가 정확히 그 층을 버린다. 미결 3건이 타 세션 측정에 종속되므로 재작성은 답하는 시점만 앞당기고 검증 수단을 잃는다. 실행 형태는 `layers/runtime` 모듈 단위 재작성이며 뒤집을 조건 3가지를 함께 기록 |
| 2026-08-14 | vLLM 도입 가능성으로 어댑터 추상을 검증 — D-70, P-52, Q-54·Q-55. **단일 노드로는 가능하다** (llamacpp 어댑터의 추론 경로 전체가 OpenAI 호환 `POST /v1/chat/completions`+SSE 하나). 걸리는 것은 D-62·D-58·`session_id` 대응물 부재이며 전부 기존 결함이 드러나는 것이다. **체인 스테이지로는 불가능**하고 이는 vLLM이 PP를 내부에서 하기 때문이지 P4 결함이 아니다. 이로써 **완결형 노드와 스테이지 노드**의 구분이 계약에 없다는 것이 드러났다 — 체인 길이 1의 유효성을 명시하면 완결형 백엔드가 설계 변경 없이 수용된다 |
| 2026-08-14 | 주제 K(메시지 디스패치 계층) 추가 — D-66~D-69, P-48~P-51, Q-52·Q-53. 목표 계층은 **바깥일수록 범용, 안쪽일수록 구상**이며 kind 해석은 마지막 두 단계의 일이다. 현재는 kind 분기가 `dispatch`와 `AgentProcessor`에 이중으로 있고(D-66), 참여자 전달 계층이 없어 에이전트가 노드를 건너뛰어 어댑터로 직행하며(D-67), 중계에도 본문 전체 디코드가 필요하고(D-68), 큐가 P4 메시지 타입에 묶여 있다(D-69). 단계 3에 `3-c`(봉투·본문 분리) 신설. **절 번호 규약 변경 — 종합 절을 §89 이상 고정으로 이전**해 주제 추가 시 번호가 밀리지 않게 함 |
| 2026-08-14 | 주제 J(어댑터 경계) 추가 — D-62~D-65, P-45~P-47, Q-50·Q-51. **감사 결과: 의존 방향은 정확하다** — `p4-protocol` 의존 0개, 역참조 없음, `layers/protocol/src`에 백엔드 문자열 0건. 누출은 계약과 문서에 있다 — `DRAFT_REPORT`의 KV·FFN, `docs/model-load.md`의 llama.cpp 노브 규범화(코드는 불투명한데 문서가 계약을 선언), 어댑터 인터페이스의 산출물 부재, 구상 어댑터가 `p4-adapter`로 인터페이스 이름을 점유(layer README는 이미 `pipeline/`이라 부른다). P-46·P-47을 단계 0에 추가. 절 번호 재배치(§90~§95) |
| 2026-08-14 | 병행 TPS 세션(Mac+GB10, MI250)과의 접점 기록 — D-60(적재 시점 `max_sequences` 선언이 런타임 파생 값을 게이트로 고정), D-61(연결 수립 정책 부재), P-44(`batching.*` 강등안), Q-48·Q-49. **둘 다 이 문서가 단독 확정하지 않는다** — 프로토콜이 처리량 층의 결정을 앞질러 못 박는 것이 D-60이 지적하는 실패 형태이므로 계획 단계에서 반복하지 않는다 |
| 2026-08-14 | **§92를 "처리량과의 관계"로 재작성.** 처리량은 P4가 아니라 **노드 큐와 물리 구상층**에서 결정된다 — 배치 합치기·디코드 credit(`listener/queue.rs`), 용량 게이트(`capacity/mod.rs`), 네이티브의 마이크로배치·스테이지 중첩. 따라서 이 개편은 처리량 노력과 배치되지 않으며 서로 다른 층을 만진다. P4의 의무는 **① 그 층이 필요로 하는 선언을 온전히 전달하고 ② 토큰 경로를 무겁게 하지 않는 것** 둘뿐이며, `P-40`·`P-9`가 ①의 이행이자 개편과 처리량의 유일한 접점이다. "처리량 문제를 프로토콜 게이트로 풀지 않는다"를 명시 — 스로틀이 GPU에서 멀어질 때의 실패가 이미 기록되어 있다 |
| 2026-08-14 | **§91을 순차 구축 순서로 재작성.** "단계 1은 분리 불가"라는 이전 서술은 **오류였다** — `P-35`(중계)가 `P-34`(주소)를 필요로 하는 것과 CPS 토대가 주소를 필요로 하는 것을 혼동했다. `P-17`·`P-18`은 wire 불변이며 단독으로 선다. 단계 경계를 **wire 호환성**으로 다시 긋고 0~6으로 재구성: v5를 유지한 채 가능한 것(0~2)을 모두 끝낸 뒤 v6(3~6)로 넘어간다. 되돌림 경계는 단계 2/3 사이 하나. **결정:** Q-33·Q-24 — 체인·적재 상태는 OUTER가 기억하고 에이전트는 불일치에 실패만 보고한다. 그 대가로 에이전트·노드가 기계적으로 단순해진다. **결정:** 스테이지 보고를 토큰 경로 비용으로 계상하지 않는다 — 신호가 작고, 없으면 OUTER의 통계 수집이 불가능하다. 파급 범위에 네이티브 런타임과 벤치 도구 추가 |
| 2026-08-14 | **조사 완결.** 요약 절과 읽는 순서 신설. §90 wire 버전 결정(v6 확정과 그 근거 8건), §91 개편 순서(6단계 의존 그래프와 단계별 관문 Q), §92 처리량 영향 평가(이득 3·비용 5·측정 필요 3) 신설. §94.1 상태 외재화를 "미완 절"에서 **경계 확정 결론**으로, §94.2 연속성 외재화를 **범위 축소 결론**으로 승격 — 인퍼런스 연속성은 KV 귀속(P-32) 때문에 외재화 불가이며 제어 연속성만 대상이라는 비대칭 확정. §94.4를 "아직 주제로 열지 않은 것"으로 재정의. 절 번호 §90~§95 재배치 |
| 2026-08-13 | 주제 I(인퍼런스 요청의 표현력) 추가 — D-55~D-59, P-40~P-43, Q-44~Q-47. 전제 12(인퍼런스 옵션 불투명 통과 + 조용한 누락 금지) 추가. upstream `common_params_sampling`(약 35필드) 대조. **Pipeline 어댑터가 샘플링 옵션을 5개로 화이트리스트하고 나머지를 조용히 버리는 것**이 핵심 결함이며, structured output은 로짓 필터링이라 사후처리가 불가능하므로 Pipeline 경로에서 원리적으로 사용 불가(D-56). 절 번호 재배치(§89~§92) |
| 2026-08-13 | **§1.3 식별자 소유 신설.** 개별 인퍼런스의 `request_id`는 OUTER가 부여한다는 확정을 계기로, 누적된 발급 주체 결정을 한 표로 정리하고 **"식별자는 OUTER가 발급한다"**를 원칙으로 명문화. 예외는 `runtime_generation`(어댑터)과 전송·CPS 내부 ID뿐. P-21·P-25가 참조하던 "§1.3의 4-튜플 규칙"이 실제 절 없이 걸려 있던 것을 여기서 해소. D-54(에이전트의 `session_id` 발급) 격상, Q-43 추가 |
| 2026-08-13 | **에이전트 ID 폐지(P-2 재작성).** 모든 메시지가 접근 주소를 자기기술하므로 별도 에이전트 ID가 무의미하다 — **접근 주소 자체가 에이전트의 ID**다. `machine_id`·`boot_id`·`agent_instance_id` 3층 초안 철회, Q-4 해소. `HardwareReport.agent_id`·`Participant.agent_id`·`agent-{host}-{pid}` 제거 대상. D-7을 "프로세스 교체 감지 불가"로 재정의하고 P-39(화신 표식)로 분리, Q-42 추가 |
| 2026-08-13 | **철회(D-52·Q-40):** "에이전트가 도달 주소를 보고하지 않는다"는 결함이 아니다. OUTER가 인프라 사실의 소유자이며 주소를 이미 안다. 프로토콜 발견은 선후 모순을 낳는다. 전제 1에 **"프로토콜로 알아낼 것과 OUTER가 이미 아는 것을 구분한다"**를 명문화하고, capability 항목에서 도달 주소를 제거. 어댑터의 `ADAPTER_REGISTER.endpoint`는 에이전트 내부의 동적 사실이므로 자기 등록 유지 |
| 2026-08-13 | **주소의 자기기술 확정(P-34)** — 메시지가 대상 객체뿐 아니라 소속 에이전트의 접속 정보(URL·포트 등)를 자기기술한다. 중계가 무상태로 성립하기 위한 전제조건. 봉투·체인 항목·귀환 주소가 같은 표기를 공유. Q-25·Q-35 결정 처리, Q-39~Q-41 추가. D-52(에이전트 도달 주소 미보고) 추가 및 주제 A의 capability 항목에 반영, D-53(자기기술 주소와 authz 부재) 추가. frame 계층 변경이므로 v6 |
| 2026-08-13 | 중계의 **배치 위치 확정** — 핸들러 계층이 아니라 큐에서 메시지를 꺼내는 기초 디스패치에 내장한다. 워커 루프의 `handler.handle` 호출 직전이며, `is_local_bypass`의 대칭 판정으로 둔다. 상위 코드는 중계를 인지하지 않는다. P-35 재작성, 파급 범위에 `task_queue/worker`·`protocol/task` 추가 |
| 2026-08-13 | **전제 11(네트워크 도달성) 추가 — 방화벽 제약.** 전제 3·5 재작성: 제어 지시가 컨트롤러를 **경유**하되 소유하지 않는다. §1.2에 위상 절 신설, 제어 경로 도식 전면 수정, 인퍼런스 반환 경로를 게이트웨이 경유로 수정, 역할 요약에 위상 열 추가. P-26 대상 변경(컨트롤러 → 게이트웨이). 주제 H 추가 — D-47~D-51, P-34~P-37, Q-34~Q-37 |
| 2026-08-13 | 전제 10(노드는 자기 적재 구조를 모른다) 추가. **철회:** D-43(레이어 범위 부재)은 결함이 아니라 의도된 추상화 — 진입 검사에서 레이어 판정 배제. **철회:** P-30(레이어 구간을 바인딩 메타로 노출)은 P-5의 "편성 제약은 첫 층에만"과 모순이었다. P-33으로 대체. D-44는 D-41에 흡수되어 축소. Q-33 추가 |
| 2026-08-13 | **정정(D-46):** `phase=DECODE`의 삭제 판단 보류를 종결. 노드 주도 디코드 루프에 필수이므로 구현 대상으로 확정. §94.3을 "삭제 판단" 절에서 "구현 확정" 절로 다시 씀 |
| 2026-08-13 | **정정(D-37):** `NodeNode` 방향을 "죽은 표면 — 삭제 후보"로 분류한 것은 오류. 대상 구조의 인퍼런스 경로 본체이므로 구현 대상이다. §94.3을 그에 맞게 다시 씀. `phase=DECODE`의 삭제 판단도 P-22 확정 이후로 연기 |
