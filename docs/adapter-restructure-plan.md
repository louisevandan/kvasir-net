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
2. **llama.cpp 업데이트 내성.** 전략·원장·정책 코드는 llama.cpp에 링크하지
   않고 HELLO 협상값·GGUF 파생값·캘리브레이션 상수만 본다.
3. **fail-closed 유지.** 미감사 메모리 계열(msa/dsa/dsv4/hybrid_iswa)은
   로드 거부. 영속 정체성 완화도 P2 검증 행렬을 통과한 항목만 내린다.
4. **메커니즘이 정책보다 먼저, 측정이 최적화보다 먼저.** 그리고 **장애
   경로가 검증되기 전에 그 위의 자동 정책을 켜지 않는다** — P2 fault gate
   통과 전 P3의 자동 TTL 축출 금지.

## 현재 상태 — 동작하는 것

- 4노드(3090×2 + 4080×2) gemma-4-E2B가 event-v2 경로로 40/40 완주.
  iSWA 스테이지 잔존성 옵트인 + 통과 텐서 정체성 수정(compat 0022~0024).
- 실측: parallel 20 → 121.43 tok/s, parallel 40 → 189.26 tok/s,
  동시 수용 시 TTFT p50 1.4 s.
- Persist/Restore 파일 형식, 원자적 publish, 복원 위치 검증, 2PC
  코디네이터(`service/cache.rs`)와 디스크 영수증(TransactionStore) 존재.

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

## 목표 구조 (요약)

```
L0 큐 → L1 원장 → L2 수용·점유 → L3 구성 전략 → L4 증명 → L5 전송
```

레코드 정체성 = (모델×컷×포맷)×(session_key×위치), 로딩ID 아님.
파이프라인 깊이 확장은 `in_flight: bool`을 풀기 전에 **fragment credit
계약**이 선행한다 — `(generation, edge, sequence, epoch, fragment)` 정체성,
edge별 row·byte credit, ACK/중복/timeout에서의 idempotent 반환, 순서·취소
규칙, 큐·RSS 상한. 규범 내용은 P4.5 행이 전부 담으며, [plan.md](plan.md) §3은 역사적 근거
링크로만 남는다.

## 단계

| 단계 | 내용 | 해소 | 수용 기준 |
| --- | --- | --- | --- |
| P-1 | 고정 작업: `model_id` = 전체 GGUF SHA-256 확정, `session_key`·prefix 증거의 계약 5요소(정규화·유일성 범위·버전·비교 규칙·부정 시험) 문서화, `session_key` wire 필드(어댑터 content-type + OUTER 전달), 하네스를 versioned `test/benchmarks/`로 이관, 증거 규약(commit·compat manifest·spec 전문·환경·요약기 버전·artifact checksum) | D15 일부 | 하네스가 저장소에서 재실행 가능; D4 실패 증거 재확보·보존; model_id 해시가 load/restore 경로에 결속되고 1바이트 변조 시험이 **캐시 부재·조작된 사이드카 양쪽 경로에서 통과**; session_key가 OUTER→어댑터→저장→재시작 Restore까지 동일 값으로 왕복; 다른 request_id의 같은 session_key 재사용 성공; 같은 request_id의 다른 session_key 별칭 거부 |
| P0 | 상태 **및 영수증** 네임스페이스: `v2/<model_id>/<cut_id>/{sessions,receipts,tmp}`, 덮어쓰기 정체성 대조, meta.json(saved_at·session_key·digest 결속) + 조언적 ACCESS 분리 + CONTROL epoch CAS | D3, D13, D17 | 동일 `operation_id`로 4개 cut 동시 Prepare/Commit/Reconcile 성공; 충돌 save 거부; gen-N 번들 + MANIFEST CAS publish의 각 단계 crash 시험에서 항상 완전한 번들만 노출; 세션 lease 원자 획득·epoch fencing 동작 |
| P1a | 행동 없는 원장: 매핑·상주·슬롯 수명 **관찰**과 불변식 위반 검출 + 시퀀스별 backend position·사용 셀 텔레메트리 신설 | D12; D5는 측정 전제만(해소는 P3) | 로드 전 노드별 KV 예측 = 실할당 ±1%; 이벤트별 원장-텔레메트리 대조 일치 |
| P1b | D4 수정: 안정 재현 → 원인 규명 → 실제 수명/네이티브 상태 수정 | D4 | 스트레스 C(40요청, 슬롯 재사용) 반복 통과 |
| P2 | `kv=1` 왕복 + **fault gate**. 선행 구현: PreparePersist의 내구 staged 번들, `Committing` 증거 기반 판정(어댑터 Reconcile의 Inconsistent 축약 교체), 수렴 표의 백엔드 중립 코어 수정 — 표는 [kv-state-store-convention.md](kv-state-store-convention.md) 소유. fault gate: 각 스테이지 commit 전·중·후 실패, 부작용 완료·finalize 전 종료(=Committing 복구), 부분 committed+부분 prepared 재조정, 코디네이터 재시작 후 명시적 수렴, 세션 경합 4종(Persist↔Restore, Persist↔Discard, Restore↔GC, 이중 Persist). 정체성 완화 **행렬**: {n_batch, n_ubatch, n_seq_max, n_ctx_seq, kv_unified} × {K/V 형식, flash/v_trans} × {같은 compat 재빌드, 다른 revision} — 통과 항목만 등급 인하, 나머지 fail-closed | D2, D6, D14, D16, D18 | 전 fault gate 통과(Committing 각 지점 포함); 세션 경합 시험에서 교차 손상 0; 재로딩 후 복원 성공; 행렬 결과 문서화 |
| P2.5 | n_ubatch 정적 보정: P5 측정 기준값 확정 (자동 최적화는 P7) | D8 일부 | 보정값으로 기준 워크로드 재측정·기록 |
| P3 | L2 수용·점유: 셀 인지 수용 + TTL Persist + 재요청 Restore + LCP(`TrimTo` 2PC 포함) + 동시성 세 축 분리(max_resident/decode_parallelism — 축 계약은 [adapter-batching-layers.md](adapter-batching-layers.md) 소유). **P2 fault gate 통과가 전제** | D5, D11 | 예측 최악 셀 선예약, 부족 시 대기/거절, over-admit 0; 분기 프롬프트에서 전 스테이지 TrimTo attest 전 프리필 시작 0건; 헤비+숏 혼합에서 무고 세션 실패 0; TTFT 분포 개선 |
| P4 | L3 전략 크레이트 추출 + 기록 트레이스 골든 재생 | — | 기존 trace 재생 결과 동일 |
| P4.5 | fragment credit 계약 구현: `(generation, edge, sequence, epoch, fragment)` 정체성, edge별 row·byte credit — `U_edge = min(producer, consumer)` 협상, `B_edge`는 모델·cut별 합의, 불일치 시 load 거부 — idempotent 반환, 순서·취소 규칙, 큐·RSS 상한 | D1 전제 | 중복·timeout·취소 fault test 통과; credit 누수 0; credit exhaustion 시험 통과; long-prompt 경계 메모리 상한 준수 실측 |
| P5 | 파이프라인 깊이>1 (프리필 청크 연속 투입부터) | D1 | GPU util 상승, 혼합 배치 발생, ITL 비악화, **경계 메모리·큐 깊이 상한 준수** |
| P6 | cut-set 연속 버퍼 합치기, 통과 텐서 재전송 생략 | D7 | 스텝 고정비 감소 실측 |
| P7 | 청크 persist(D10), SWA V(D9), n_ubatch 자동 최적화, DENIED 계열 2축 감사 | D8~D10 | 계열별 감사 문서 + 로드 성공 |

순서: **P-1 → P0 → P1a → P1b → P2 → P2.5 → P3 → P4 → P4.5 → P5 → P6 → P7.**
P0~P2가 안전·검증, P3~P4 정책, P4.5~P6 성능, P7 확장이다.

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

## 검토 수렴 규약

반복 리뷰가 같은 종류의 결함을 다시 찾지 않게 하는 규칙이다.

- R1 **주장 단일 소유**: 하나의 계약·순서·결함 귀속은 한 문서만 소유하고
  다른 문서는 링크한다. 중복 서술이 낡는 것이 이번 회차 충돌의 원인이었다.
- R2 **코드 앵커 의무**: 코드 행동을 서술하는 문장은 `path::symbol @
  short-commit` 앵커를 달거나, 검증 전이면 "목표 계약"으로 명시한다.
  line 번호 단독 앵커는 코드 이동으로 부패하므로 쓰지 않는다.
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
  리뷰의 몫이다. CI/pre-commit 연결은 아직 없다.
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
