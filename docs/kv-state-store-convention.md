# KV 영속 상태 저장 규약

2026-08-31 계약 보정 회차 반영. 이 문서는 저장 조직, 레코드 정체성,
세션 동시성, 2PC 수렴 규칙의 **단독 소유자**다. 파일 *형식*(.lkv의
magic/버전/정체성/SHA-256/원자적 publish)은 구현이 이미 갖고 있으나,
아래 계약 중 코드에 없는 것은 각 절에 명시하고 구현 단계를
[adapter-restructure-plan.md](adapter-restructure-plan.md)가 소유한다.

## 현행 상태와 결함

현행: `<kv_root>/<cache_key>.lkv` 평면 구조, `cache_key`는 어댑터가
`cache.sequence`를 복사한 것, 검증은 load 시점 파일 내부 대조뿐이다.

| 결함 | 결과 |
| --- | --- |
| save가 기존 파일 정체성을 읽지 않고 덮어씀 | 키 충돌 시 무단 파괴, load에서야 발견 |
| 파일명에 모델·스테이지 범위 없음 | 같은 볼륨 다중 노드가 서로 파괴 |
| 시각 필드 전무 | 유휴 TTL 판정 근거 없음 |
| 열거 구조 없음 | GC·quota·유휴 스캔 불가 |
| 레코드 128MB 단일 파일 | 노드당 ~55K 토큰 이상 세션 저장 불가 |
| `model_identity` 내용 미규정 | 문자열일 뿐, 규약 없음 |
| 영수증도 평면 구조: `<kv_root>/.p4-transactions/<operation_id>.receipt`, 스테이지 범위가 경로에 없음 | 볼륨 공유 시 4스테이지가 같은 operation_id로 충돌 |
| Prepare가 영수증만 쓰고 내구 staged 사본을 만들지 않음 (`transaction_store.cpp::TransactionStore::prepare` @ df5b9ce7) | commit 도중 손실 시 복구 근거가 없음 |
| 잠금이 operation_id 단위 (`TransactionStore::Lease`) | 같은 세션의 Persist/Restore/Discard/GC가 동시 실행 가능 |
| 어댑터 Reconcile이 `Committing`을 `Inconsistent`로 축약 | 실재하는 영수증 상태 하나가 통째로 미정의 (`server.cpp` @ df5b9ce7 은 commit을 Committing 내구 기록→부작용→Committed로 수행) |

## 저장 단위: 무엇이 하나의 레코드인가

**샤드 레코드 = (model_id × cut_id × kv_format) × (session_key × position)**

로딩 ID(`load_generation`)는 레코드 정체성이 아니다. 같은 모델을 같은
레이어 구성으로 재로딩한 노드는 기존 레코드를 재사용한다. 세대는
`Cache.generation`으로 연산 조율에만 결속된다.

## 정체성의 세 등급

| 등급 | 항목 | 불일치 시 |
| --- | --- | --- |
| 레이아웃 정체성 (경로에 새김) | model_id, 레이어 범위 [b,e), K/V 타입, v_trans/flash, 메모리 계열 | 다른 레코드. 재사용 불가 |
| 수용 조건 (복원 시 검사) | position < n_ctx_seq, 셀 여유, 슬롯 여유 | 거부하되 레코드 보존 |
| 참고 정보 (기록만) | build_identity, 저장 시 n_batch/n_ubatch/n_seq_max | 경고 로그 |

등급 인하는 계획 P2의 검증 행렬을 통과한 항목에만 적용하며, 통과 전에는
현행 완전 일치(fail-closed)를 유지한다.

## model_id 정의

- 정체성 = **GGUF 파일 전체 바이트의 SHA-256.** 분할 GGUF는 파일명
  `-%05d-of-%05d`의 part 인덱스 오름차순으로 각 part의 raw digest
  32바이트를 이어 붙인 바이트열의 SHA-256이다. 헤더·텐서 테이블·크기만으로는
  텐서 데이터가 다른 두 파일이 같은 ID가 되므로 정체성이 될 수 없다.
- 경로 성분은 전체 digest 앞 16 hex, `meta.json`과 복원 대조는 전체 digest.
- **검증 정책: load마다 전체 재계산이 기본이다.** 크기+mtime 사이드카는
  내용 증명이 아니므로 production 경로에서 재계산을 대체할 수 **없다**.
  대체가 허용되는 유일한 형태는 수집 시 전체 해시를 검증한
  content-addressed artifact manifest다.
- 부정 시험(P-1 통과 조건): 동일 크기 1바이트 텐서 변조 → 복원 거부를
  **캐시 부재 경로와 조작된 사이드카 존재 경로 양쪽에서** 확인한다.

## session_key 계약 (sk-v1)

- 형식은 **필수**다: `sk1:<owner>/<conversation>`. `sk1:` 접두 필수, 전체
  1..512바이트의 유효 UTF-8, 제어문자 금지, `<owner>`와 `<conversation>`
  모두 비어 있지 않아야 한다. 형식 위반은 경로 계산 전에 거부한다.
- `<owner>`는 OUTER 배포가 소유하는 안정 네임스페이스, `<conversation>`은
  그 안의 안정 대화 ID(OUTER가 생성한 무작위 ID 권장)다. 네임스페이스가
  필수인 이유: 서로 다른 대화가 같은 raw 키를 쓰면 같은 레코드가 되고 raw
  바이트 대조로는 그 의미 충돌을 검출할 수 없기 때문이다. 유일성 보장은
  OUTER의 의무이고, 어댑터는 형식만 강제할 수 있다.
- 경로 성분: `sk-v1/<sha256(raw key 바이트열) 전체 64 hex>` — 계약 버전이
  경로에 있다. 계약 변경은 `sk-v2` 경로로만 한다.
- `meta.json`에 raw 키와 전체 digest를 저장하고, 복원 전 raw 키 바이트
  일치 + model_id·cut_id·kv_format 전체 대조를 요구한다.
- 부정 시험: 잘못된 UTF-8 / 빈 키 / 513바이트 / `sk1:` 접두 없음 /
  `<owner>` 또는 `<conversation>` 공백 → 거부; 같은 `<conversation>` 다른
  `<owner>` → 다른 레코드; meta의 digest/raw 불일치 → 복원 거부.

## 레코드 번들과 원자성

state·tokens·meta는 셋이 하나의 레코드다. 부분 조합(새 KV + 이전
tokens 등)이 존재할 수 없도록 **불변 세대 디렉터리 + 원자 포인터**를 쓴다.

```
<kv_root>/v2/<model_id 16hex>/<cut_id>/
  sessions/sk-v1/<sk-digest 64hex>/
    gen-<n>/                # 불변 번들. 생성 후 내부 파일 수정 금지
      state.lkv             # (128MB 초과 시 state-<k>.part, 열린 항목)
      tokens.bin            # 프롬프트+생성 전체 토큰 ID, LE i32 나열
      meta.json             # position, record_generation=n, state/tokens 각
                            # SHA-256, raw session_key, 전체 model digest,
                            # cut_id, kv_format, saved_at, last_access
    MANIFEST                # 원자 포인터: {generation, meta_sha256}
    lease.json              # 세션 샤드 단일 작성자 lease
  receipts/<operation_id>.receipt
  tmp/                      # 동일 볼륨. 원자적 rename 보장
```

- publish 순서: `gen-<n>` 완성·fsync → `MANIFEST` 원자 교체(기대 이전
  generation을 담은 CAS) → 구세대 GC. 어느 시점에 죽어도 MANIFEST는
  완전한 번들만 가리킨다.
- 영수증은 `(record_generation, position, state_sha256, tokens_sha256)`을
  결속한다. LCP 증거와 KV position의 결속이 영수증 수준에서 증명된다.
- LCP 최종 판정은 `tokens.bin` 바이트 비교다(70K 토큰 ≈ 280KB). 체인
  digest는 선택 가속일 뿐이다. tokenizer 결속은 model_id가 보증한다.

## 세션 샤드 직렬화

- 직렬화 단위는 operation이 아니라 **(model_id, cut_id, sk-digest)** 다.
- Persist·Restore·Discard·TrimTo·GC·last_access 갱신은 모두 lease 획득
  후에만 진행한다. lease는 원자 생성(exclusive create)으로 얻고
  `{operation_id, kind, epoch, acquired_at}`를 담는다. 죽은 소유자 복구는
  **epoch 증가(fencing token)** 로만 하며, 낡은 epoch의 쓰기는 거부된다.
- `record_generation`은 단조 증가하고 모든 publish는 기대 generation
  CAS다. 늦게 끝난 낮은 position Persist는 CAS 실패로 폐기된다 —
  "supersede"는 규칙이 아니라 이 CAS의 결과다.
- GC는 lease 획득에 실패한 세션을 건너뛴다.
- 필수 장애 시험(P2): Persist↔Restore, Persist↔Discard, Restore↔GC,
  이중 Persist — 모두 직렬화되거나 깨끗이 실패하고 교차 손상 0.

## LCP Trim 장벽

분기 프롬프트의 절단은 4스테이지 분산 변경이다.
`TrimTo(session, position, tokens_sha256_at_position, epoch)`를 2PC
연산으로 정의한다: 전 스테이지 prepare(대상 position·digest 검증) →
commit(각자 `seq_rm [position, ∞)`) → **전 스테이지 attest 후에만**
suffix 프리필을 시작한다. 일부 스테이지만 잘린 상태의 프리필은 조용한
오답을 만들므로 금지다. 구현 단계는 계획 P3이 소유한다.

## 2PC 수렴 규칙

**P2 구현 요구(전제 아님):** PreparePersist는 내구 staged 번들을 만들어야
한다. 현행 prepare는 영수증만 쓴다(`transaction_store.cpp::TransactionStore::prepare`
@ df5b9ce7). 이 요구가 구현되기 전에는 아래 Persist 복구 보장이 성립하지
않는다.

영수증 수명은 `Prepared → Committing(내구) → [부작용] → Committed(내구)`다
(`server.cpp` @ df5b9ce7). 따라서 부작용 전·중·후의 정지는 모두
`Committing`으로 관측되며, **내구 증거로 부작용 완료 여부를 판정**해야
한다. 어댑터 Reconcile이 Committing을 Inconsistent로 축약하는 현행 동작은
P2에서 이 판정으로 교체된다.

| 연산 | 관측(재시작 포함) | 판정 증거 | 수렴 |
| --- | --- | --- | --- |
| Persist | 전원 prepared | — | rollback → all-resident (staged 사본 폐기) |
| Persist | committed ≥1 + prepared 잔여 | staged 사본 존재 | **roll-forward → all-persisted** (잔여 Commit 재시도; committed는 Abort로 되살릴 수 없음) |
| Persist | Committing, 최종 번들 유효 | 번들 체크섬 일치 | roll-forward: finalize 재수행(멱등) |
| Persist | Committing, 최종 없음·staged 유효 | staged 체크섬 | roll-forward: 재승격 후 finalize |
| Persist | Committing, staged·최종 모두 무효 | — | Inconsistent(정지). staged 요구가 지켜지면 도달 불능 |
| Restore | 전원 prepared | 레코드 무손상 | rollback → all-persisted |
| Restore | committed ≥1 또는 Committing | 임포트는 휘발, 레코드 무손상 | **rollback → all-persisted** (임포트 스테이지 `seq_rm` 철회 후 전체 재시도) |
| Discard | committed ≥1 또는 Committing | 번들 잔존 여부 | roll-forward → absent (잔존 시 삭제 재수행, finalize) |
| 모든 연산 | manifest/체크섬 불일치 | — | Inconsistent: 정지, 자동 재시도 금지, 레코드 폐기 표시, 재프리필 강등 |

현행 코어 코디네이터(`layers/service/src/cache.rs::recover` @ df5b9ce7,
시험 `recovered_partial_restore_is_failed_closed`)는 부분 Restore와
Aborting 중 committed 발견을 즉시 실패로 접는다 — 그 시험은 현행 동작의
고정이지 이 표의 구현이 아니다. Persist roll-forward와 Restore rollback
경로는 llama 지식 없는 백엔드 중립 코어 수정으로 추가한다(계획 원칙 1).

## 수명 규약

- 부팅 시 `tmp/` 잔재 제거, MANIFEST가 가리키지 않는 고아 `gen-*` 재색인
  또는 GC.
- 새 publish 성공(=MANIFEST CAS 성공) 후 구세대 삭제.
- Discard(명시적) 또는 quota GC(last_access 오래된 순, lease 하에서만)로만
  삭제.

## 남는 열린 항목

- 128MB 초과 번들의 `state-<k>.part` 경계·체크섬 규약.
- 수용-조건 등급 인하 대상의 실증 행렬(계획 P2 소유).
- DENIED 메모리 계열의 보조 상태 수용은 2축 감사 통과 이후.
