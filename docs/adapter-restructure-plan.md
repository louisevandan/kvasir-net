# llama.cpp 어댑터 재구성 계획

2026-08-31 개정(초판 2026-08-30, 동일자 검토 피드백 반영). llama.cpp
어댑터의 전면 재구성 — 배치 레이어링, KV 영속화, 세션 원장, 수용·축출
정책, 전송 효율, 모델 다양성 게이트 — 의 전체 계획이다. 근거 실측은
2026-08-30의 4노드 gemma-4-E2B 실행에서 나왔다.

설계 상세는 두 문서가 소유한다:

- 배치 레이어 계약: [adapter-batching-layers.md](adapter-batching-layers.md)
- 영속 저장 규약: [kv-state-store-convention.md](kv-state-store-convention.md)

## 원칙

1. **P4 코어 무변경, 어댑터+OUTER 변경 허용.** 프로토콜·에이전트·서비스는
   배치·KV·모델 개념을 모른다. 코드 변경은 `layers/adapters/llamacpp/`와
   OUTER 구현물(드라이브·플래너·하네스)에서 일어난다 — `session_key` 같은
   새 wire 필드는 어댑터 소유 content-type 안에서 늘고 OUTER가 채운다.
   P4 코어에 llama 전용 지식은 추가하지 않되, **백엔드 중립적인 2PC 정확성
   수정은 허용한다** — 현행 코디네이터(`layers/service/src/cache.rs::recover` @ df5b9ce7,
   시험 `recovered_partial_restore_is_failed_closed`는 이 현행 동작의
   고정이다)는 부분 Restore를 재시작 후 즉시 실패 처리하고 Aborting 중
   committed 영수증을 실패로 접으므로, 그대로는 P2의 all-resident/all-persisted
   수렴을 통과할 수 없다. 수렴 방향 표는 저장 규약 문서가 소유한다.
2. **llama.cpp 계층 분리 내성.** 정책 코드(전략·원장·수용)는 llama.cpp에
   링크하지 않고 HELLO 협상값·GGUF 파생값·캘리브레이션 상수만 본다 —
   "pull 후 값만 바뀐다"는 이 층에만 성립한다. native compat 계층의
   갱신은 매 pin 의미 기반 rebase이며(d7a207411→d7bd3bfc dry-run 5/24
   충돌 — 6차 회차 자체 재현), 그 비용은 U0의 per-pin 호환성 게이트와 패치 큐
   3분할이 소유한다.
3. **fail-closed 유지.** 미감사 메모리 계열(msa/dsa/dsv4/hybrid_iswa)은
   로드 거부. 모델 확장은 3축 감사(스테이지 잔존성 ∧ unified 시퀀스 분리
   ∧ backend conformance) 통과가 조건이고, 영속 정체성 완화도 P2 검증
   행렬을 통과한 항목만 내린다.
4. **메커니즘이 정책보다 먼저, 측정이 최적화보다 먼저.** 그리고 **장애
   경로가 검증되기 전에 그 위의 자동 정책을 켜지 않는다** — P2 fault gate
   통과 전 P3의 자동 TTL 축출 금지.

## 현재 상태 — 과거 관측 (재현성 주의)

아래는 2026-08-30의 로컬 관측이다. 5차 리뷰 시점의 체크아웃은 이를
재현하지 못했다 — compat 0022~0024와 manifest가 미추적이었고, upstream
checkout이 pin(d7a207411)을 벗어나(d7bd3bfc, dirty tree) 공식 prepare가
`no compatibility manifest`로 실패했다. 이번 회차에 패치 큐와 manifest를
추적했으며, upstream pin 복구와 pristine 검증은 U0 수용 기준이다.

- 4노드(3090×2 + 4080×2) gemma-4-E2B가 event-v2 경로로 40/40 완주.
  iSWA 스테이지 잔존성 옵트인 + 통과 텐서 정체성 수정(compat 0022~0024).
- 실측: parallel 20 → 121.43 tok/s, parallel 40 → 189.26 tok/s,
  동시 수용 시 TTFT p50 1.4 s.
- Persist/Restore 파일 형식, 원자적 publish, 복원 위치 검증, 2PC
  코디네이터(`service/cache.rs`)와 디스크 영수증(TransactionStore) 존재.

## 실측: upstream 이동에 대한 계층 내성 (2026-08-31)

원칙 2("정책 계층만 값-독립, native compat은 매 pin rebase")를 검증하기 위해
llama.cpp 최신 master를 받아 패치 큐를 재생했다. `bump-pipeline-upstream.mjs`가
깨끗한 worktree를 만들어 적용하므로 작업 트리를 건드리지 않고 측정된다.

| 항목 | 값 |
| --- | --- |
| pin | d7a207411 (2026-08-27) |
| 대상 | 557614e02 (2026-08-31) |
| 사이 upstream 커밋 | **69** |
| 충돌 | **24개 중 6개** (0009, 0010, 0013, 0016, 0017, 0018) |
| P4 정책 계층(Rust) 변경 | **0** |

계층별로 갈라 보면 계약이 예측한 그대로다.

| 분류 | 패치 수 | 충돌 | 해석 |
| --- | --- | --- | --- |
| `upstream_fix` (ggml 계층) | 2 | **0** | ggml 추상층은 69커밋 동안 이 패치들과 무관하게 움직였다 |
| `model_feature` | 4 | 1 (0018 mtp-tail) | 모델 기능 포트는 대체로 독립 |
| `stage_hook` (llama core 침투) | 18 | 5 | 갱신 비용은 여기 집중된다 |
| P4 정책(전략·원장·스케줄러·session_key) | — | 0 | llama.cpp에 링크하지 않으므로 구조적으로 0 |

**비용은 선형이 아니다.** 하루 전 pin(d7bd3bfc, 1커밋 차)에서 이미 5개가
충돌했고, 69커밋을 더 받아도 6개다 — 커밋 68개가 새 충돌을 1개만 추가했다.
갱신 비용은 upstream 변화량이 아니라 **우리가 침투한 파일이 건드려졌는가**로
결정된다. 이것이 U0 ④ 패치 큐 3분할과 ③ stage ABI 격리를 정당화하는 실측
근거다: `upstream_fix`를 별도 묶음으로 두면 upstream이 그것을 흡수했을 때
전체 포팅과 함께 충돌하지 않고, `stage_hook`의 침투 표면을 좁힐수록 5라는
숫자가 줄어든다.

한계: 이 측정은 **적용 가능성**만 본다. 적용된 뒤의 의미 동등성(상태 형식,
수치 동등성)은 U0 ①의 clean-pin prepare와 매 pin 상태 호환 게이트가 따로
판정한다.

## 실측: 새 pin 채택 (2026-08-31, U0 ①)

위 내성 측정에 이어 실제로 pin을 옮겼다. 결과는 계층 분리 주장의 가장
강한 증거다.

| 단계 | 결과 |
| --- | --- |
| 큐 리베이스 (24패치) | clean 19, fuzz 3, 3-way 2 — 수동 개입은 hunk 5개 |
| pristine 재생 검증 | **24/24 clean** (`rebase-pipeline-upstream.mjs` phase 2) |
| 공식 prepare | **통과** — compatibility_id `557614e029.00e66c6b…` |
| CUDA 빌드 | **성공**. 네이티브 시험 실행 파일 11개 전부 종료 코드 0, 단 **real-model 하위 시험 4건은 SKIP**(`P4_STAGED_LLAMA_MODEL`·`P4_STAGED_MTP_MODEL` 미설정) — model-free 스위트만 증명됐다 |
| 4노드 수용 | **통과** — 동일 프롬프트에 유의미한 한국어 답, 구조 1/1·의미 1/1 |
| P4 정책 계층(Rust) 변경 | **0** |

리베이스가 실제 결함을 하나 찾아냈다. upstream이 메모리 생성 지점과 계열
둘(`llama_kv_cache_dsa_iswa`, `llama_memory_hybrid_idx`)을 추가했는데
이들이 잔존성 게이트를 우회하고 있었다. 우회는 감사되지 않은 계열에 대해
부분 스테이지를 조용히 다시 열어 주므로 fail-closed가 무너진다. 게이트
적용을 계열 이름 나열이 아니라 `llama_kv_cache*`/`llama_memory*` 생성
전체 매칭으로 바꾸고, 재생 검증에 **우회 0건**을 단언으로 넣었다 — 그
단언이 이 구멍을 드러냈다. 두 신규 계열은 의도대로 default-deny로 안착했다.

빌드 아키텍처 함정도 기록해 둔다. 스크립트 기본값이 `75;89`라 이 배치의
sm_86(3090)에는 네이티브 코드가 없었고, 그 sm_75 빌드로 돌린 로컬 실행이
19.9 → 6.15 tok/s였다. **이 수치는 그 과거 빌드의 관측이며 현행 실행의
설명이 아니다** — 현재 배포된 `ggml-cuda.dll`은 sm_86·sm_89 cubin을 모두
담고 있고(직접 검사: sm_86 15, sm_89 22, sm_75 0) 원격 해시도 동일하다.
수용 판정이 처리량이 아니라 **답의 의미**였기에 이 함정은 시험을
통과시키면서도 드러났다 — 두 판정을 분리해 둔 것이 값을 한 지점이다.

원격 smoke의 6.4 tok/s는 성능 기준선이 아니다: 요청 1건, 대부분 1행 디코드,
`mixed_batches=0`이라 배치·파이프라인 처리량을 대표하지 않는다. 기준선은
증거 체계(E0/R0)가 닫힌 뒤 동일 commit·build·model로 재측정한다.

## 결함 대장

| # | 결함 | 증거 |
| --- | --- | --- |
| D1 | 파이프라인 깊이 1: `in_flight: bool` 하나, downstream은 CapsuleSet의 전 물리 UBATCH를 순차 실행 후 일괄 반환 | GPU 18~25%, 스텝 시간 행수 무관, 혼합 배치 0~2건 |
| D2 | 플랜에 `--kv-root` 없음 → `kv=0` | HELLO 로그 |
| D3 | 상태 저장 조직 부재: 평면 `<root>/<key>.lkv`, 무조건 덮어쓰기, 시각 없음 | state_store.cpp |
| D4 | 40요청 슬롯 재사용 position 불연속. **원인 미규명** — 로컬 seq_rm→전 스테이지 정산→슬롯 반환 순서는 이미 구현돼 있으므로(release.rs) 관찰 장부만으로는 안 고쳐진다 | 스트레스 C (증거 미보존, 재확보 필요) |
| D5 | 셀 회계 부재: 수용이 슬롯 개수만 셈 | scheduler 호출부 |
| D6 | 정체성 과잉 고정: n_batch/n_ubatch/n_seq_max·빌드 완전 일치 요구 | manifest 대조 코드 |
| D7 | 홉당 81회 개별 텐서 전송 고정비 | cut-set 31/27/23 |
| D8 | compute buffer 과할당: ubatch 512에서 1.4 GB, 실점유 3.5% | 512↔128 실측 3.66× |
| D9 | SWA V 과할당 (v_trans 시 256→512) | KV 버퍼 로그 |
| D10 | 영속 128 MB/노드 상한 | state_store 크기 검사 |
| D11 | 프리픽스 재사용 부재 | 어댑터 감사 |
| D12 | 동적 점유 텔레메트리 부재. `MEMORY_ACTUAL`은 로드 시 전체 할당 보고이지 런타임 셀 점유가 아니다 | llama_stage_runtime.cpp |
| D13 | 영수증 충돌: `<kv_root>/.p4-transactions/<op>.receipt` 평면 구조, 스테이지 범위가 경로에 없음 → 볼륨 공유 시 4스테이지가 같은 operation_id로 상충 | transaction_store.cpp:185 |
| D14 | 2PC commit 구멍: 네이티브 Commit은 publish 후 즉시 seq_rm 하므로, commit wave 중 실패 시 코디네이터의 Abort가 이미 committed 스테이지를 되살릴 수 없다. 부분 committed의 수렴 규칙 미검증 | kv 런타임 + cache.rs |
| D15 | 영속 SessionKey·LCP 증거 부재: `InferenceCommand`는 session_id/request_id뿐, 저장 meta에 토큰 이력·prefix digest 없음 → 재시작 후 "같은 대화" 판정 근거 없음 | `v2/commands.rs::InferenceCommand` @ df5b9ce7 |
| D16 | 세션 단위 직렬화 부재: 잠금이 operation_id 단위라 같은 세션의 Persist/Restore/Discard/GC가 동시 실행 가능 | `transaction_store.cpp::TransactionStore::Lease` @ df5b9ce7 |
| D17 | 레코드 번들 비원자: state/tokens/meta의 부분 조합이 가능하고 LCP 증거와 KV position의 결속을 증명할 수 없음 | 규약 문서 결함 표 |
| D18 | `Committing` 미정의: commit은 Committing 내구 기록→부작용→Committed인데 어댑터 Reconcile이 Committing을 Inconsistent로 축약; Prepare는 staged 사본 없이 영수증만 씀 | `protocol.hpp::KvReceiptState` 정의, `server.cpp::Session::handle` KvCommit 분기, `transaction_store.cpp::TransactionStore::prepare` @ df5b9ce7 |
| D19 | native compat 갱신 취약: 24개 패치·27개 upstream 파일, d7a207411→d7bd3bfc 재생에서 5개 충돌(0010, 0013, 0016, 0017, 0018 — `bump-pipeline-upstream.mjs --from d7a207411` clean-worktree 재생으로 자체 재현). 큐가 stage hook/upstream fix/model feature 미분리이고 `ggml-backend.cpp`·RPC까지 패치해 llama 계층 아래를 침범 | compat/d7a207411 큐 |
| D20 | stage server가 llama 비공개 헤더에 결합: CMake가 upstream `src/`를 PRIVATE include로 열고 `stage_memory_plan.cpp`가 `llama-cpp.h`·`llama-ext.h`를 직접 포함 → public ABI와 무관하게 내부 리팩터링마다 파손 | `server/CMakeLists.txt`(P4_STAGED_LLAMA_SOURCE_DIR/src), `runtime/stage_memory_plan.cpp` 상단 include @ 87ec1317 |
| D21 | HELLO가 실행물을 식별 못 함: upstream commit만 노출 — 같은 commit·다른 patch-set 빌드가 동일하게 보이고, stage_abi·활성 backend/device·trim_support·state_abi 부재 | `server/server_hello.cpp`(capabilities 문자열) @ 87ec1317 |
| D22 | compat manifest가 체크아웃 EOL에 종속: 0001~0021은 CRLF 바이트 해시, 0022~0024는 LF 해시로 기록돼 어떤 autocrlf 설정의 깨끗한 checkout도 검증 실패, fixture 자체 시험 14/15 | 6차 회차 수정 — `.gitattributes`(`*.patch text eol=lf`) + 전체 LF 재해시, validate 통과·자체 시험 6/6. patch_set_sha256·patched_tree 재검증은 U0 ①의 clean-pin prepare가 수행. 7차 리뷰가 clean pin 직접 적용으로 현행 aggregate 값 일치를 외부 검증 |

## 목표 구조 (요약)

```
L0 큐 → L1 원장 → L2 수용·점유 → L3 구성 전략 → L4 증명 → L5 전송
```

레코드 정체성 = (모델×컷×포맷)×(session_key×위치), 로딩ID 아님.
파이프라인 깊이 확장은 `in_flight: bool`을 풀기 전에 **fragment credit
계약**이 선행한다 — `(generation, edge, sequence, stream_epoch, fragment)` 정체성,
edge별 row·byte credit, ACK/중복/timeout에서의 idempotent 반환, 순서·취소
규칙, 큐·RSS 상한. 규범 내용은 P4.5 행이 전부 담으며, [plan.md](plan.md) §3은 역사적 근거
링크로만 남는다.

## 단계

| 단계 | 내용 | 해소 | 수용 기준 |
| --- | --- | --- | --- |
| U0 | llama 호환 경계: ① clean upstream pin 복구·pristine 검증(공식 prepare 통과) ② HELLO에 stage_abi·`build_id`(patch_set_sha256 포함)·활성 backend/device·`trim_support`·`state_abi_id`·backend layout 결속 ③ server의 llama 비공개 헤더 의존 제거(public `llama.h` + P4 소유 versioned stage ABI만; 내부 접근은 compat 구현 안으로) ④ 패치 큐 3분할(stage hook / upstream fix / model feature) ⑤ backend conformance 감사 3축 등재 ⑥ backend 게이트는 release manifest의 `required_backend_set` 선언 기준이다 — 집합에는 CPU 기준선이 항상 포함되고 배포가 주장하는 production backend가 더해진다(현행 선언 = {CPU 기준선, CUDA production}). 선언된 전 backend가 plugin/version·device capability·실제 buffer placement 증거와 함께 통과해야 승격 ⑦ cross-backend Persist/Restore는 행렬 통과 전 fail-closed | D19, D20, D21, D22 | ① pin에서 공식 prepare 통과 — 양쪽 autocrlf의 깨끗한 checkout fixture 포함, patch_set_sha256·patched_tree 재검증·재기록, dirty 포트 시도는 폐기가 아니라 분기로 보존 ② 같은 upstream·다른 patch-set 두 빌드를 HELLO로 구분 + 협상값(trim_support·state_abi_id·backend layout)이 실측 능력과 일치하는 시험 ③ server include 빌드 게이트로 비공개 헤더 0건 ④ 전 패치 `stage_hook\|upstream_fix\|model_feature` 분류 — 미분류 0건, 묶음별 독립 적용 시험, upstream에 흡수된 fix 자동 검출·제거, stage hook의 허용 파일·심볼 범위 명세 ⑤ 감사 3축과 수치 동등성 기준 문서화 ⑥ `required_backend_set` 선언·검증 절차 정의 — 현행 선언 {CPU 기준선, CUDA production} 중 CPU 축은 현행 pin에서 통과; CUDA 축 통과는 U0 완료 조건이 아니라 production 승격 조건 ⑦ cross-backend 복원 거부를 부정 시험으로 확인 |
| P-1 | 고정 작업: `base_model_id × kv_variant_id` 계약 확정(정의는 규약이 단독 소유, 구현 착수는 계약 승인 후), `session_key`·prefix 증거의 계약 5요소(정규화·유일성 범위·버전·비교 규칙·부정 시험) 문서화, `session_key` wire 필드(어댑터 content-type + OUTER 전달), 하네스를 versioned `test/benchmarks/`로 이관, 증거 규약(commit·compat manifest·spec 전문·환경·요약기 버전·artifact checksum) | D15 일부 | 하네스가 저장소에서 재실행 가능; D4 실패 증거 재확보·보존; base_model_id·kv_variant_id가 load/restore 경로에 결속되고 base 1바이트 변조 시험이 **캐시 부재·조작된 사이드카 양쪽 경로에서 통과**; LoRA scale·mmproj·control-vector 변조 거부 + 엔트리 순서 무관 시험 통과; session_key가 OUTER→어댑터→저장→재시작 Restore까지 동일 값으로 왕복; 다른 request_id의 같은 session_key 재사용 성공; 같은 request_id의 다른 session_key 별칭 거부 |
| P0 | 상태 **및 영수증** 네임스페이스: `v2/<model_id>/<cut_id>/{sessions,receipts,tmp}`, 덮어쓰기 정체성 대조, meta.json(saved_at·session_key·digest 결속) + 조언적 ACCESS 분리 + CONTROL epoch CAS | D3, D13, D17 | 동일 `operation_id`로 4개 cut 동시 Prepare/Commit/Reconcile 성공; 충돌 save 거부; gen-N 번들 + MANIFEST CAS publish의 각 단계 crash 시험에서 항상 완전한 번들만 노출; 세션 lease 원자 획득·epoch fencing 동작 |
| P1a | 행동 없는 원장: 매핑·상주·슬롯 수명 **관찰**과 불변식 위반 검출 + 시퀀스별 backend position·사용 셀 텔레메트리 신설 | D12; D5는 측정 전제만(해소는 P3) | 로드 전 노드별 KV 예측 = 실할당 ±1%; 이벤트별 원장-텔레메트리 대조 일치 |
| P1b | D4 수정: 안정 재현 → 원인 규명 → 실제 수명/네이티브 상태 수정 | D4 | 스트레스 C(40요청, 슬롯 재사용) 반복 통과 |
| P2 | `kv=1` 왕복 + **fault gate**. 선행 구현: PreparePersist의 내구 staged 번들, `Committing` 증거 기반 판정(어댑터 Reconcile의 Inconsistent 축약 교체), 수렴 표의 백엔드 중립 코어 수정 — 표는 [kv-state-store-convention.md](kv-state-store-convention.md) 소유. fault gate: 각 스테이지 commit 전·중·후 실패, 부작용 완료·finalize 전 종료(=Committing 복구), 부분 committed+부분 prepared 재조정, 코디네이터 재시작 후 명시적 수렴, 세션 경합 4종(Persist↔Restore, Persist↔Discard, Restore↔GC, 이중 Persist). 정체성 완화 **행렬**: {n_batch, n_ubatch, n_seq_max, n_ctx_seq, kv_unified} × {K/V 형식, flash/v_trans} × {같은 compat 재빌드, 다른 revision} × {소스 backend × 대상 backend} — 통과 항목만 등급 인하, 나머지 fail-closed | D2, D6, D14, D16, D18 | 전 fault gate 통과(Committing 각 지점 포함); 세션 경합 시험에서 교차 손상 0; 재로딩 후 복원 성공; 행렬 결과 문서화 |
| P2.5 | n_ubatch 정적 보정: P5 측정 기준값 확정 (자동 최적화는 P7) | D8 일부 | 보정값으로 기준 워크로드 재측정·기록 |
| P3 | L2 수용·점유: 셀 인지 수용 + 스냅샷 명령 이행(Persist/`Checkpoint` 신설/`SnapshotList` 신설/RestoreInto 확장/Fork/Discard — 어휘와 의미는 규약 소유, 트리거 정책은 전부 OUTER) + 재요청 Restore + LCP(`TrimTo` 2PC 포함) + 동시성 세 축 분리(max_resident/decode_parallelism — 축 계약은 [adapter-batching-layers.md](adapter-batching-layers.md) 소유). **P2 fault gate 통과가 전제** | D5, D11 | 예측 최악 셀 선예약 — 다중 노드는 예약 2PC(규약 소유: prepared 회계 포함, 로컬 단조 TTL 회수, 멱등 release), 부족 시 대기/거절, over-admit 0(prepared 포함); 분기 프롬프트에서 전 스테이지 TrimTo attest 전 프리필 시작 0건; 헤비+숏 혼합에서 무고 세션 실패 0; TTFT 분포 개선 |
| P4 | L3 전략 크레이트 추출 + 기록 트레이스 골든 재생 | — | 기존 trace 재생 결과 동일 |
| P4.5 | fragment credit 계약 구현: `(generation, edge, sequence, stream_epoch, fragment)` 정체성, edge별 row·byte credit — `U_edge = min(producer, consumer)` 협상, `B_edge`는 모델·cut별 합의, 불일치 시 load 거부 — idempotent 반환, 순서·취소 규칙, 큐·RSS 상한 | D1 전제 | 중복·timeout·취소 fault test 통과; credit 누수 0; credit exhaustion 시험 통과; long-prompt 경계 메모리 상한 준수 실측 |
| P5 | 파이프라인 깊이>1 (프리필 청크 연속 투입부터). 스냅샷 정합 펜스는 전 파이프라인 정지가 아니라 **대상 시퀀스 드레인**으로 좁힌다(정산 증거는 credit이 아니라 stage별 `SequenceQuiesced` attest — O9) | D1 | GPU util 상승, 혼합 배치 발생, ITL 비악화, **경계 메모리·큐 깊이 상한 준수**; 시퀀스 드레인 중 타 시퀀스 스텝 지속 |
| P6 | cut-set 연속 버퍼 합치기, 통과 텐서 재전송 생략 | D7 | 스텝 고정비 감소 실측 |
| P7 | 청크 persist(D10), SWA V(D9), n_ubatch 자동 최적화, DENIED 계열 3축 감사 | D8~D10 | 계열별 감사 문서 + 로드 성공 |

순서: **U0 → P-1 → P0 → P1a → P1b → P2 → P2.5 → P3 → P4 → P4.5 → P5 →
P6 → P7.** P-1의 model 해시·session_key wire·하네스 이관은 U0와 병행
가능하나, **P-1 완료 판정과 native 실측은 U0 통과가 전제**다. U0~P2가
안전·검증, P3~P4 정책, P4.5~P6 성능, P7 확장이다.

## 계약 보정 회차 (2026-08-31, 3차 리뷰)

3차 리뷰의 차단 결함 7건에 대한 처리 기록이다. "완료"는 이 저장소에서
검증 가능한 것만 말한다; 코드 구현은 해당 단계의 통과 조건이지 완료
주장이 아니다. **P-1 착수 가부는 차기 리뷰가 판정한다.**

| 차단 결함 | 계약(문서) | 구현(코드) |
| --- | --- | --- |
| Committing 수렴 미정의 | 완료 — 규약 2PC 표에 연산×Committing 행과 증거 판정 추가 | P2 |
| 세션 단위 직렬화 부재 | 완료 — 규약 "세션 샤드 직렬화"(lease/epoch fencing/generation CAS) | P0(lease)·P2(경합 시험) |
| 레코드 번들 비원자 | 완료 — 규약 "레코드 번들과 원자성"(gen-N + MANIFEST CAS, 영수증 결속) | P0 |
| LCP trim 장벽 부재 | 완료 — 규약 "LCP Trim 장벽"(TrimTo 2PC, 전 스테이지 attest) | P3 |
| model_id 사이드카 우회 | 완료 — load마다 전체 재계산 기본, 검증된 manifest만 대체 허용, 양쪽 경로 부정 시험 | P-1 |
| session namespace/version 불완전 | 완료 — `sk1:<owner>/<conversation>` 필수 형식, `sk-v1` 경로 성분, 부정 시험 목록 | P-1 |
| lint 실효성 없음 | 완료 — 재귀·README 포함·소유 주장 검사·미색인 오류화, fixture 자체 시험, `package.json` 진입점, 저장소 추적 | — |

### 4차 리뷰 반영 (2026-08-31)

| 차단 결함 | 처리 |
| --- | --- |
| 재시작 후 all-Prepared rollback 불성립 (resident는 휘발) | 규약 2PC 표 개정 — 동일 epoch resident attest ×N일 때만 Abort; attest 실패+staged 유효 → roll-forward; 둘 다 없으면 Inconsistent |
| epoch 권위 저장소 부재 | 규약 — 내구 `CONTROL {epoch, generation}` 신설, 모든 publish가 (expected_epoch, expected_generation) 결속 |
| TrimTo 부분 커밋 미정의 | 규약 2PC 표에 TrimTo 행 — 부분 절단은 동일 position roll-forward(멱등); suffix 폐기 확정 후에만 발행되므로 전진만 안전 |
| 불변 번들 vs last_access 충돌 | meta에서 제거, 조언적 `ACCESS` 파일 분리; 고아 gen은 증명 없이 노출 금지(격리/GC만) |
| model 경로 16-hex 충돌 | 경로도 전체 64 hex, Windows 장경로 전제 명시 |
| session key 문법 모순 | 512바이트=접두 포함 raw 전체, 첫 `/` 분할, 유니코드 정규화 없음(바이트 동일성), 공백-전용 금지, 부정 시험 일치화 |

게이트 주장 수위 정정: docs-lint는 문자열 canary이고 cargo test에 연결해
강제하되 의미 재서술 검출은 리뷰 몫이다. R2 자체 위반 앵커 2건(`server.cpp`
무심볼, KvReceiptState 정의 위치)을 정정했고 무심볼 앵커는 이제 lint가
거부한다. 직전 커밋 d33c2671f의 `p4-256-optimization.md` 포함은 README
색인 요구(미색인=오류)의 의도된 결과다. 동시성 세 축 계약(외부 피드백)을
batching 문서에 추가했다. **P-1 착수 가부는 여전히 차기 리뷰가 판정한다.**

### 5차 리뷰 반영 (2026-08-31)

llama.cpp의 추상층(llama/ggml 인터페이스)과 구상 백엔드(CPU/CUDA/Metal)
분리를 계획에 결속했다.

| 차단 결함 | 처리 |
| --- | --- |
| "값만 바뀐다" 과잉 주장 | 원칙 2를 두 층으로 분리 — 정책 계층만 값-독립, native compat은 매 pin rebase(5/24 충돌 관측). per-pin 게이트·큐 3분할은 U0 소유 |
| stage server의 비공개 헤더 결합 | D20 등재, U0 ③(P4 소유 versioned stage ABI 뒤로 이동) |
| HELLO 식별력 부족 | D21 등재, U0 ② 필드 목록 확정 |
| 정체성 backend 축 부재 | 규약 레이아웃 정체성에 state_format·backend_family·compatibility_id 추가, P2 행렬에 소스×대상 backend 축 |
| 동시성 소유권 오서술 | batching 문서 — 요청값(코디네이터)/물리 상한(스테이지별 min) 2층 분리, `n_seq_max`는 구성값의 echo임을 명시 |
| 모델 게이트 2축 → 3축 | backend conformance 축 추가(CPU 매 pin 필수) |
| CONTROL rename≠CAS | 규약 — `control.lock` exclusive-create 임계구역 + crash recovery 계약으로 교체 |
| TrimTo family capability | 규약 — `trim_support = arbitrary\|bounded\|none` attest, 불가 시 전체 재프리필 강등(recurrent 실코드 앵커) |
| quota GC shard-local 위험 | 규약 — 노출 레코드 삭제는 코디네이터 Discard 2PC만, 로컬 GC는 orphan 한정 |
| 재현성 | "동작하는 것" 절을 과거 관측으로 재표기, compat 큐·manifest 추적 커밋, upstream pin 복구는 U0 수용 기준 |
| lint의 untracked 오염 | 공식 모드를 `git ls-files` 추적 파일 기준으로 전환, `--all` 분리 |

**P-1 완료 판정과 native 실측은 U0 통과 전 불가하다.** 차기 리뷰 판정
대상이다.

### 6차 리뷰 반영 + 자체 감사 (2026-08-31)

6차부터는 지적 반영과 함께 **전체 계획 논리성 자체 감사**를 회차 산출물로
포함한다.

리뷰 5건:

| 차단 결함 | 처리 |
| --- | --- |
| compat manifest EOL 종속 | **수정 완료** — `.gitattributes` + LF 재해시(21건), validate 통과, fixture 6/6. clean-checkout 이중 autocrlf fixture와 patch_set/patched_tree 재기록은 U0 ① 수용 기준 |
| CONTROL 비교·publish 분리 경합 | 규약 — 비교와 publish를 같은 임계구역으로; lock 소유자 `{host_instance_id, boot_id, pid, operation_id}`; stale-break 늦은 writer 시험 P0 추가 |
| U0 내용·기준 불일치 | U0 수용 기준을 7항목 전부로 확장(분류 강제·미분류 0·흡수 fix 검출·허용 범위·승격 기준·cross-backend 부정 시험) |
| compatibility_id 과잉 고정 재도입 | 정체성 4분할 — build_id(참고) / state_abi_id(경로) / backend_layout_id(meta 대조) / 복원 행렬. cut_id는 build 출처 배제 |
| "deterministic logits" 판정 불능 | 수치 동등성으로 교체 — NMSE·오차 한계·greedy 토큰열 기준, 같은-backend 왕복과 cross-backend 별도 기준 |

정정: "dirty upstream이라 dry-run 자체 재현 불가"는 오류였다 — bump
스크립트는 clean worktree를 만들며, 6차 회차에서 5/24 충돌(0010, 0013,
0016, 0017, 0018)을 자체 재현했다.

자체 감사 발견(리뷰가 지적하지 않은 구멍):

| # | 구멍 | 처리 |
| --- | --- | --- |
| a | 다중 샤드 연산의 lease 획득 순서 미정 → 교착 가능 | 규약 — stage_index 오름차순 전순서, 실패 시 전부 해제 |
| b | `epoch` 용어 충돌(저장소 fencing vs P4.5 fragment) | store_epoch/stream_epoch로 분리 명명 |
| c | U0 ② 필드가 4분할 정체성과 불일치(state_format 단일값) | U0 필드 목록을 build_id/state_abi_id/backend layout으로 갱신 |
| d | model_id가 단일 GGUF 전제 — mmproj·LoRA 미포함 | 아티팩트 집합 digest로 확장, LoRA 집합 상이 = 다른 레코드 |
| e | TrimTo 후 영속 레코드가 상주보다 앞설 수 있음 → 복원이 죽은 suffix 부활 위험 | Restore에 `복원→LCP 대조→TrimTo` 순서 강제 명문화 |
| f | 코디네이터가 노드 로컬 ACCESS 파일을 읽을 수 없음 | victim 선정 입력은 와이어 텔레메트리로, ACCESS는 로컬 영속화로 역할 분리 |
| g | cut_id 파생에 build 출처 혼입 가능성 | 레이아웃 정체성만으로 파생함을 명문화 |
| h | 복원의 셀 선확보가 4노드 각각의 비동기 해제에 걸림 | P3 셀 예약을 전 노드 all-or-release로 명시(아래 P3 항목) |
| i | recurrent 앵커가 저장소 커밋 형식 | upstream pin 형식으로 정정, R2에 규칙 추가 |

### 8차 리뷰 반영 + 저장 계층 방향 (2026-08-31)

8차 판정: 승인 보류 — 정확성 차단점 3, 계약 불일치 2. O1~O8은 중복
계산되지 않았다(등재 제도가 의도대로 동작).

| 판정 | 처리 |
| --- | --- |
| Checkpoint 논거 타당하나 동사 중복 지양 | `Snapshot{after_commit: KeepResident\|ReleaseResident}` 한 형상으로 수용 |
| 무복사 분기는 조건부 | 3조건(동일 storage domain·호환 cut·전 스테이지 완료 후 절연) + read-pin(O10) 명문화 |
| fragment credit은 정지점 증거가 아님 | 확인 — plan.md §3이 credit 반환을 "peer가 인수"로 정의. `SequenceQuiesced` attest로 교체, O9 등재 |
| SnapshotList 키 교집합 부족(O8 등재 확인) | 교집합 단위를 논리 스냅샷 튜플로 정정 |
| backend set 문면 모순(미닫힘 판정) | U0 ⑥ 양쪽 셀 통일 — {CPU 기준선, CUDA production}, CUDA 통과는 승격 조건 |
| state ABI 게이트·예약 2PC 조건부 닫힘 | O3(fixture 실재)·O4(예약 수명+TTL 경합) 문구 확장으로 조건 등재 |

추가 방향 지시(저장 계층): 영속화 목적지는 디스크만이 아니다 —
`tier = durable | ram | resident`를 규약에 신설했다. ram 계층은 과부하
공정 스왑(일부 KV를 RAM으로 내리고 다른 요청 처리 후 재적재)을 위한
휘발 계층으로, 크래시 수렴은 Absent(손상 아님), cross-pin ABI 부담 없음,
호스트 바이트는 별도 수용 회계 축(O11). 스왑 정책은 전부 OUTER 명령이다.

### 방향 확정: 스냅샷 명령 모델 (2026-08-31)

영속화 트리거를 TTL로 좁혔던 것을 정정했다. 분기 워크로드(기존 KV를
영속화·복사해 새 세션으로 트리 분기)가 일상 연산이므로, 어댑터는 명령
어휘(Persist/Checkpoint/SnapshotList/RestoreInto/Fork/Discard/Unload)의
이행만 소유하고 **모든 트리거 정책은 OUTER**가 소유한다. 이 과정에서
기존 계약과의 실제 충돌 하나를 확인했다: `CacheAction::Persist`의
"한 동사" 논증은 상주를 유지하는 족적(Checkpoint) 용례를 보지 못했다.
`Fork {into}`는 계약이 이미 분기를 예견한 부분이다. 상세는 규약의
"스냅샷 명령 모델" 절이 소유한다.

### 7차 리뷰 반영 + 자체 감사 (2026-08-31)

EOL 수리는 승인되었고, 리뷰가 clean pin 직접 적용으로 aggregate
(patch_set_sha256·patched_tree) 일치까지 외부 검증했다.

리뷰 6건:

| 차단 결함 | 처리 |
| --- | --- |
| state_abi_id가 upstream 상태 형식 변화를 못 봄 | 규약 — manifest 소유·수동 입력 금지, 매 pin 상태 호환 게이트(N-1→N 계열별 복원, 바이트·position·logits 비교, 실패 시 증가 강제) |
| stale 판정 권위 부재 | 규약 — lock_token·release token 대조, 권위는 토폴로지가 결정(신설 kv_root 토폴로지 절), 불확실 시 자동 break 금지 |
| model_id 불완전 + plan-규약 R1 불일치 | base_model_id × kv_variant_id 분리(role·scale·range 정규 인코딩), P-1 정합화, lint에 정의 소유 needle 추가 |
| Restore→LCP 역순(자체 감사 e의 방향 오류) | 판정 사다리로 교체 — import 전 tokens.bin LCP, trim 불가 시 Restore 생략. 역순 문구는 폐기 목록 등재 |
| all-or-release 비분산 | 예약 2PC 신설(prepared 회계·로컬 단조 TTL·멱등·Reconcile·장애 시험) |
| production backend CUDA 고정 | required_backend_set 선언 기준으로 일반화, 현행 target은 CUDA 한정 명시 |

자체 감사 재판정 수용: 완료 2(b, c) / 부분 6(a, d, f, g, h, i) / 방향
오류 1(e — 이번 회차에 정정). 부분 항목 중 d·h는 위 리뷰 항목으로 승격
처리됐다.

신규 자체 감사(리뷰 미지적):

| # | 구멍 | 처리 |
| --- | --- | --- |
| α | 상태 호환 게이트를 실서비스 모델로 돌리면 pin마다 비용 폭발 | 소형 fixture 모델 + 계열별 골든 상태를 저장소 시험 자산으로 명시 |
| β | 예약 TTL을 절대 시각으로 두면 시계 편차 문제 재도입 | 수신 시점 기준 로컬 단조 시계로 정의 |
| γ | e-역순 충돌의 근본 원인은 실행 흐름의 문서 간 중복 | batching의 Persist/Restore 흐름 블록 제거(링크만) — 중복 클래스 자체 소거 |
| δ | 잠금에 완전성을 요구하는 계층 오류 | liveness(잠금)/safety(store_epoch) 분리 원칙 명문화; 상호배제 불성립 배포는 로드 거부 |
| ε | prepared 예약이 회계에 안 보이면 over-admit 재발 | 텔레메트리 reserved_cells 필드 추가 |
| ζ | kv_root가 공유인지 노드 로컬인지 미정 — 잠금 계약 전체가 조건부였음 | 토폴로지 축 신설: 기본=노드 전용 로컬(권위 문제 소거), 공유 볼륨은 storage capability gate + membership 권위 필수 |

**U0·P-1 완료는 여전히 주장하지 않는다.** 병행 가능 범위는 session_key
wire·하네스 이관이며, base×variant 구현은 이 계약의 리뷰 승인 후다.

session_key wire의 현재 실증 범위는 **OUTER→어댑터 편도**다. 3090×2 원격
4노드 실행에서 하네스가 발급한 키와 어댑터가 admission 시점에 보유한 키가
같음을 어댑터 자신의 트레이스로 대조했고, 같은 request_id가 다른 키로
재등장하면 admission이 거부한다. **저장·재시작 Restore 구간은 아직 없다** —
그 왕복은 상태 네임스페이스(P0)와 스냅샷 명령(P3)이 생긴 뒤에야 성립하며,
P-1 인수 기준의 나머지 절반은 그때까지 미충족으로 남는다.

## known open surface

다음 리뷰가 지적할 것으로 스스로 예상하는 미해결 표면이다. 여기 등재된
항목의 지적은 "새 발견"이 아니라 "등재 확인"으로 판정한다.

| # | 표면 | 예정 소유 |
| --- | --- | --- |
| O1 | 텔레메트리 채널 자체의 계약 부재 — reserved_cells·last_access·세션 점유를 어느 이벤트로, 어떤 주기·순서 보장으로 나르는지 | P1a |
| O2 | U0 ③의 stage ABI가 "제거하라"뿐 — P4 소유 versioned ABI의 실제 표면(함수·타입) 미정의 | U0 설계 산출물 |
| O3 | 상태 게이트 fixture의 계열 커버리지 — kv_cache/iswa/hybrid/recurrent 각각의 소형 골든 모델 실재 미확인 | P2 준비 |
| O4 | 예약 2PC와 세션 lease의 관계 — 예약이 lease를 전제하는지, 두 조율 계층의 획득 순서, 예약 수명 `Prepared→Committed→Consumed/Released`와 TTL 만료·Commit 경합의 승자 규칙 | P0 |
| O5 | ~~session_key wire 확장이 정말 어댑터 content-type 안에서 끝나는지~~ — 해소: 필드는 어댑터의 `InferenceCommand` 안에서만 살고 P4 프로토콜은 불변, 4노드 원격 실행이 OUTER가 발급한 값과 어댑터가 보유한 값의 동일성을 증명 | P-1 |
| O6 | `Snapshot{after_commit}`·`SnapshotList`·RestoreInto의 계약 문면 — 방향은 확정(스냅샷 명령 모델), p4-adapter 동사의 정확한 시그니처·2PC 결합·**Fork와 snapshot key의 immutable-ID/mutable-ref 선택** 미작성 | P-1 계약, P3 구현 |
| O7 | resident 계층 체크포인트(tier)의 capability 협상 | P7 |
| O8 | 코디네이터의 스냅샷 원장 복구 — 교집합 단위는 키가 아니라 논리 스냅샷 튜플(규약 정정 완료), OUTER 재시작 후 재구성 절차 자체는 미작성 | P3 |
| O9 | Sequence quiescence — credit와 별개의 stage별 `SequenceQuiesced` attest 계약(credit 반환은 인수 증거이지 compute/KV 완료가 아님, plan.md §3) | P4.5 |
| O10 | Snapshot storage domain·read pin — source·target lease, 동시 Discard 차단, 노드 이동·cross-domain 복사 경로 | P3 |
| O11 | durable/ram-byte admission — 노드별 디스크·호스트 RAM 예약, ENOSPC partial-prepare 수렴, OUTER 가용량 텔레메트리 | P3 |
| O12 | 한 실행을 마친 원격 에이전트가 두 번째 실행을 받지 못한 관측 1건 — 스테이지가 뜨지 않고 에이전트 로그에 수신 흔적도 없이 드라이브가 timeout_ms까지 대기(2026-09-01, 재기동 후 동일 시나리오는 정상). 원인 미확정: 로드 세대 전환의 어댑터 상태인지 터널·연결 수명인지 분리되지 않음. 현재 하네스는 실행마다 에이전트를 재기동해 회피한다 | 미배정 |

## 검토 수렴 규약

반복 리뷰가 같은 종류의 결함을 다시 찾지 않게 하는 규칙이다.

- R1 **주장 단일 소유**: 하나의 계약·순서·결함 귀속은 한 문서만 소유하고
  다른 문서는 링크한다. 중복 서술이 낡는 것이 이번 회차 충돌의 원인이었다.
- R2 **코드 앵커 의무**: 코드 행동을 서술하는 문장은 `path::symbol @
  short-commit` 앵커를 달거나, 검증 전이면 "목표 계약"으로 명시한다.
  line 번호 단독 앵커는 코드 이동으로 부패하므로 쓰지 않는다. upstream
  파일 앵커는 저장소 커밋이 아니라 **upstream pin 커밋**으로 적는다.
  lint는 `@` 형태만 검사하므로 앵커 없는 신규 주장의 검출은 리뷰 몫이다.
- R3 **이름에는 계약**: 새 식별자·키·digest는 정규화, 유일성 범위, 버전,
  비교 규칙, 부정 시험의 5요소가 정의되기 전에는 이름만 올릴 수 없다.
- R4 **단계 동사 규율**: 단계-결함 연결은 측정/재현/수정/검증 중 하나의
  동사로만 표기한다. "관찰" 단계가 "해소"를 주장할 수 없다.
- R5 **기계 검사**: `npm run docs-lint`가 vendored/build 제외 전 프로젝트
  Markdown을 재귀 검사한다 — 파일별 혼합 EOL, 폐기 문구(README 포함),
  소유 주장 재서술, docs/ 미색인(README의 실제 링크 형태 요구), 심볼 없는
  코드 앵커를 오류로 거부하며, `cargo test --workspace`가 이 검사를
  실행한다(entrypoints/agent/tests/docs_lint.rs). 한계도 계약이다:
  이것은 **문자열 canary**라서 의미를 바꾼 재서술은 잡지 못하고, 그 검출은
  리뷰의 몫이다. 공식 모드는 `git ls-files` 기준 추적 Markdown만 검사해
  무관한 untracked 초안이 게이트와 커밋 범위를 오염시키지 않게 하고,
  `--all`이 파일시스템 전체 검사다. CI/pre-commit 연결은 아직 없다.
- R6 **피드백 변환 규칙**: 리뷰 지적은 (a) 앵커 달린 주장 수정, (b) 실행
  가능한 시험·게이트, (c) 소유 단계가 있는 열린 결정 중 하나로 변환해서만
  닫는다. 텍스트 수정만으로 닫지 않는다.

## 측정 하네스

현행 `target/gemma4-4node/`는 git-ignored라 기준이 될 수 없다. P-1에서
versioned `test/benchmarks/` 아래로 이관하고, 모든 수용 판정 증거에
commit·compat manifest·spec 전문·환경·요약기 버전·원본 artifact checksum을
남긴다. 현재 보존된 스트레스 아티팩트는 2/2 성공본이며 D4의 40요청 실패
증거가 아니다 — P-1에서 재확보한다.

## 문서 지도

| 문서 | 소유 |
| --- | --- |
| 이 문서 | 재구성 전체 계획·결함 대장·단계 |
| [adapter-batching-layers.md](adapter-batching-layers.md) | L0~L5 계약, 불변식, 전략 모듈 |
| [kv-state-store-convention.md](kv-state-store-convention.md) | 레코드 정체성, 디렉터리, 수명 |
| [llamacpp-stage-memory.md](llamacpp-stage-memory.md) | 스테이지 메모리 소유권, 합법 절단 |
| [event-protocol-v2.md](event-protocol-v2.md) | 이벤트 계약, 게이트 증명 순서 |
| [plan.md](plan.md) | 2026-08-21 계획(역사 문서; §3 Edge credit 규범 내용은 P4.5 행으로 이관 완료) |
