# KV 영속 상태 저장 규약

> 문서 지위 (2026-09-06): **분야 계약·구현과 구별**. 소유 분야의 계약/목표를 읽되 구현 완료로 간주하지 않는다. 현재 개발 순서와 충돌하면 로드맵의 명시적 이관을 따른다.
> 현재 목표·상태·순서는 [실행 로드맵](distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](document-map.md)를 따른다.

2026-08-31 계약 보정 회차 반영. 이 문서는 저장 조직, 레코드 정체성,
세션 동시성, 2PC 수렴 규칙의 **단독 소유자**다. 파일 *형식*(.lkv의
magic/버전/정체성/SHA-256/원자적 publish)은 구현이 이미 갖고 있으나,
아래 계약 중 코드에 없는 것은 각 절에 명시하고 구현 단계를
[분산 배치 로드맵](distributed-batching-roadmap.md)의 K 분기가 소유한다.
본문의 P0/P2/P3 등은 구 backlog 식별자이며 새 단계 연결은 로드맵을 따른다.

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
| 어댑터 Reconcile이 `Committing`을 `Inconsistent`로 축약 | 실재하는 영수증 상태 하나가 통째로 미정의 (`server.cpp::Session::handle` @ df5b9ce7 이 commit을 Committing 내구 기록→부작용→Committed로 수행; 정의는 `protocol.hpp::KvReceiptState`) |

## 저장 단위: 무엇이 하나의 레코드인가

**샤드 레코드 = (base_model_id × cut_id × kv_format) × (session_key ×
snapshot_key × kv_variant_id × position)** — 한 세션은 서로 다른 키의
영속 족적을 여러 개 가질 수 있다.

로딩 ID(`load_generation`)는 레코드 정체성이 아니다. 같은 모델을 같은
레이어 구성으로 재로딩한 노드는 기존 레코드를 재사용한다. 세대는
`Cache.generation`으로 연산 조율에만 결속된다.

## 정체성의 세 등급

| 등급 | 항목 | 불일치 시 |
| --- | --- | --- |
| 레이아웃 정체성 (경로에 새김) | base_model_id, 레이어 범위 [b,e), K/V 타입, v_trans/flash, 메모리 계열, **state_abi_id** | 다른 레코드. 재사용 불가 |
| 수용 조건 (복원 시 검사) | position < n_ctx_seq, 셀 여유, 슬롯 여유 | 거부하되 레코드 보존 |
| 참고 정보 (기록만) | build_identity, 저장 시 n_batch/n_ubatch/n_seq_max | 경고 로그 |

등급 인하는 계획 P2의 검증 행렬을 통과한 항목에만 적용하며, 통과 전에는
현행 완전 일치(fail-closed)를 유지한다.

build 출처와 상태 호환성은 별개의 축이다 — patch-set 전체 해시를
정체성으로 쓰면 로깅 한 줄 바뀐 빌드가 전 영속 레코드를 죽이는 D6의
재판이 된다. 넷으로 분리한다.

- `build_id` (참고 정보): upstream commit + patch_set_sha256 + backend
  provenance. 진단·증거용이지 정체성이 아니다.
- `state_abi_id` (레이아웃 정체성, 경로 파생에 포함): 시퀀스 상태
  직렬화의 호환성 세대. **compat manifest가 소유**하며 임의 수동 입력은
  금지다. 실제 상태 바이트는 `llama_state_seq_get_data_ext()` 아래 각
  upstream 메모리 계열의 `state_write/state_read`가 만들므로 **P4 패치가
  안 바뀌어도 upstream pull이 형식을 바꿀 수 있다.** 따라서 매 pin마다
  상태 호환 게이트를 통과해야 한다: 메모리 계열별로 (pin N-1 writer →
  pin N reader) 복원과 (N writer → N reader) 기준 실행을 비교 — 상태
  바이트 구조·position·후속 logits/토큰 일치. 실패하면 manifest의
  state_abi_id 증가가 강제된다. 게이트는 소형 고정 fixture 모델과
  계열별 골든 상태(저장소 시험 자산)로 실행한다.
- `backend_layout_id` (레코드 수준, meta에 기록·복원 시 대조): 상태가
  실제 사용한 device/buffer-type/배치의 정규화 fingerprint. llama.cpp는
  한 실행 안에서도 복수 device·CPU fallback·tensor별 buffer override를
  허용하므로 단일 `backend_family` 값으로는 표현되지 않는다.
- 복원 허용 = `state_abi_id 일치 ∧ (source_layout → target_layout)이 P2
  검증 행렬 통과`. cross-layout 이동성은 public llama.cpp 계약이 아니므로
  왕복 실증 전 fail-closed다.

`cut_id` 경로 성분은 레이아웃 정체성만으로 파생한다 — build_id가 섞이면
같은 과잉 고정이 경로에서 재발한다.

## 모델 정체성: base_model_id × kv_variant_id

단일 model_id로는 KV에 영향을 주는 아티팩트 전부를 식별하지 못한다 —
LoRA는 digest 외에 scale이, control vector는 scale·layer range가 KV를
바꾸고, llama.cpp는 context의 LoRA 집합·scale을 런타임에 바꿀 수 있다.
둘로 나눈다.

- `base_model_id` (경로 성분, 전체 64 hex): base GGUF 전체 바이트의
  SHA-256(분할 GGUF는 `-%05d-of-%05d` part 인덱스 오름차순으로 각 part
  digest 32바이트를 이은 목록의 SHA-256). tokenizer 정체성은 base 파일
  digest가 보증한다. 헤더·텐서 테이블·크기 요약은 텐서 데이터가 달라도
  같아질 수 있으므로 정체성이 될 수 없다.
- `kv_variant_id` (레코드 수준, meta 기록·복원 대조): KV에 영향을 주는
  부가 아티팩트의 canonical binary encoding에 대한 SHA-256.
  v1 = `(version u32, count u32, entry*)`,
  entry = `(role u8 ∈ {mmproj, lora, control_vector}, digest 32B,
  scale f32-bits u32, layer_begin i32, layer_end i32)`,
  role·digest 순 정렬(순서 무관 canonical). 동적 LoRA 변경이 허용되는
  배포에서 kv_variant_id는 로드가 아니라 **세션/레코드 정체성**이다.
- 검증 정책: load마다 전체 재계산이 기본이다. 크기+mtime 사이드카는 내용
  증명이 아니므로 production 경로에서 재계산을 대체할 수 없고, 수집 시
  전체 해시를 검증한 content-addressed manifest만 대체할 수 있다.
- 부정 시험(P-1 통과 조건): base 1바이트 텐서 변조(캐시 부재·조작된
  사이드카 양쪽 경로), LoRA scale 변경, mmproj 변조, control-vector range
  변경 → 복원 거부; 엔트리 순서만 바꾼 입력 → 동일 id.
## session_key 계약 (sk-v1)

- 형식은 **필수**다: `sk1:<owner>/<conversation>`.
  - 길이 한도 1..512바이트는 `sk1:` 접두를 포함한 **raw 키 전체** 기준이다.
  - 전체가 유효 UTF-8이어야 하고 C0/C1 제어문자를 금지한다.
  - 구분자는 접두 뒤 **첫 번째 `/`** 하나다. `<conversation>` 내부의 추가
    `/`는 데이터로 취급한다.
  - `<owner>`·`<conversation>` 각각 1바이트 이상이고 공백만으로 구성될 수
    없다(비공백 코드포인트 1개 이상).
  - **유니코드 정규화는 하지 않는다.** 비교와 해시는 항상 바이트 동일성이며
    NFC/NFD만 다른 두 키는 서로 다른 키다. 정규화가 필요하면 OUTER가
    키를 만들기 전에 한다.
  - 형식 위반은 경로 계산 전에 거부한다.
- `<owner>`는 OUTER 배포가 소유하는 안정 네임스페이스, `<conversation>`은
  그 안의 안정 대화 ID(OUTER가 생성한 무작위 ID 권장)다. 네임스페이스가
  필수인 이유: 서로 다른 대화가 같은 raw 키를 쓰면 같은 레코드가 되고 raw
  바이트 대조로는 그 의미 충돌을 검출할 수 없기 때문이다. 유일성 보장은
  OUTER의 의무이고, 어댑터는 형식만 강제할 수 있다.
- 경로 성분: `sk-v1/<sha256(raw key 바이트열) 전체 64 hex>` — 계약 버전이
  경로에 있다. 계약 변경은 `sk-v2` 경로로만 한다.
- `meta.json`에 raw 키와 전체 digest를 저장하고, 복원 전 raw 키 바이트
  일치 + base_model_id·kv_variant_id·cut_id·kv_format 전체 대조를 요구한다.
  유일성 범위는 `(base_model_id × cut_id)` 트리 내다 — 다른 모델·다른
  컷의 같은 키는 다른 레코드다.
- 부정 시험: 잘못된 UTF-8 / 빈 키 / 513바이트(접두 포함) / `sk1:` 없음 /
  구분자 없음 / 공백만의 owner 또는 conversation → 거부; conversation에
  `/`가 든 키는 수용되고 첫 `/`에서만 분할됨을 확인; NFC/NFD만 다른 두 키
  → 서로 다른 레코드; 같은 `<conversation>` 다른 `<owner>` → 다른 레코드;
  meta의 digest/raw 불일치 → 복원 거부.

## 레코드 번들과 원자성

state·tokens·meta는 셋이 하나의 레코드다. 부분 조합(새 KV + 이전
tokens 등)이 존재할 수 없도록 **불변 세대 디렉터리 + 원자 포인터**를 쓴다.

```
<kv_root>/v2/<base_model_id 64hex>/<cut_id>/
  sessions/sk-v1/<sk-digest 64hex>/
    snap-v1/<snapshot_key digest 64hex>/   # 명명된 족적. 키 문법·경로
                            # digest 규칙은 session_key 계약을 재사용
      gen-<n>/              # 불변 번들. 생성 후 내부 파일 수정 금지
      state.lkv             # (128MB 초과 시 state-<k>.part, 열린 항목)
      tokens.bin            # 프롬프트+생성 전체 토큰 ID, LE i32 나열
      meta.json             # position, record_generation=n, state/tokens 각
                            # SHA-256, raw session_key, base_model_id,
                            # kv_variant_id,
                            # cut_id, kv_format, raw snapshot_key, saved_at
      MANIFEST              # snapshot별 원자 포인터: {generation, meta_sha256}
    CONTROL                 # 세션 권위 epoch·generation, 원자 교체
    ACCESS                  # 조언적 최근 접근 시각 {last_access, epoch}, 원자 교체
    lease.json              # 세션 샤드 단일 작성자 lease
  receipts/<operation_id>.receipt
  tmp/                      # 동일 볼륨. 원자적 rename 보장
```

- publish 순서: `gen-<n>` 완성·fsync → `MANIFEST` 원자 교체
  (`(expected_epoch, expected_generation)` CAS) → 구세대 GC. 어느 시점에
  죽어도 MANIFEST는 완전한 번들만 가리킨다.
- 영수증은 `(record_generation, position, state_sha256, tokens_sha256)`을
  결속한다. LCP 증거와 KV position의 결속이 영수증 수준에서 증명된다.
- LCP 최종 판정은 `tokens.bin` 바이트 비교다(70K 토큰 ≈ 280KB). 체인
  digest는 선택 가속일 뿐이다. tokenizer 결속은 base_model_id가 보증한다.

## kv_root 토폴로지

잠금·rename·텔레메트리 계약의 전제이므로 축을 명시한다.

- **기본(권장): 노드 전용 로컬 디스크.** CONTROL·lock 상호배제는 단일
  호스트 OS 원자성(exclusive create)으로 성립하고, boot_id·pid 생존을
  로컬에서 판정할 수 있어 stale 권위 문제가 소거된다. cut_id 경로 분리는
  한 머신에 여러 스테이지가 사는 배포(현행 로컬 4노드)를 위한 것이다.
  코디네이터는 어느 토폴로지에서도 저장소 파일을 직접 읽지 않는다 —
  입력은 항상 와이어 텔레메트리다.
- **공유 볼륨(SMB/NFS 등): 조건부 지원.** 해당 볼륨에서 exclusive
  create·replace rename·flush·lock 가시성을 실증하는 storage capability
  gate와, 내구 heartbeat 또는 외부 lease 서비스 같은 membership 권위를
  모두 요구한다. 없으면 미지원이며 로드를 거부한다.
- P0 장애 시험에 "분할 → stale-break → 구 writer 복귀"를 포함한다.

## 세션 샤드 직렬화

- 직렬화 단위는 operation이 아니라 **(base_model_id, cut_id, sk-digest)** 다.
- 권위 있는 epoch은 lease가 아니라 내구 `CONTROL = {epoch, generation}`
  레코드가 소유한다. **rename 단독은 CAS가 아니다** — 두 경쟁자가 같은
  epoch을 읽고 각자 원자 교체에 성공할 수 있다. CONTROL 갱신은
  `control.lock`의 exclusive create(POSIX `O_CREAT|O_EXCL`, Windows
  `CREATE_NEW`)로 상호배제한 임계구역 안에서 read → 기대값 비교 → tmp
  기록·fsync → rename으로 수행한다. lock 파일은 `{host_instance_id, boot_id, pid, operation_id, acquired_at}`을
  담는다 — 공유 볼륨에서 pid는 호스트 간 중복되고 원격 프로세스 생존은
  로컬 pid 검사로 판정할 수 없으므로, host_instance_id(노드 설치 시 고유
  난수)와 boot_id(부팅마다 갱신)가 소유자를 식별한다. stale lock은 소유
  노드의 부재·재부팅을 확인한 뒤 CONTROL epoch 검사와 함께만 파기한다
  (crash recovery 계약).
- 각 lock에는 임의 `lock_token`을 부여하고, release는 **현재 lock 파일의
  token이 내 token일 때만** 삭제한다 — stale-break 뒤 복귀한 구 소유자가
  새 소유자의 lock을 지우는 사고를 막는다.
- **stale 판정의 권위**: 소유자 식별은 누가 잡았는가만 말하고, 죽었는지
  네트워크 분할인지는 말하지 못한다. `acquired_at`은 호스트 시계 편차
  때문에 단독 권위가 될 수 없다. 권위는 배포 토폴로지가 정하며(아래
  kv_root 토폴로지), 권위가 불확실하면 자동 break는 금지다.
- 층 분리 원칙: **잠금은 liveness(경합·중복 작업 감소)용이고 안전성은
  store_epoch 결속이 보장한다.** 단 CONTROL epoch 부여 자체는 진짜
  상호배제를 요구하므로, 그 상호배제가 성립하지 않는 배포는 지원하지
  않는다(로드 거부). lease 획득은 이 임계구역에서
  epoch+1을 내구 기록한 뒤에만 성립하고, 그 다음 `lease.json`
  `{operation_id, kind, epoch, acquired_at}`을 쓴다. lease 삭제·재생성만으로는
  이전 소유자의 늦은 publish를 막지 못한다.
- **비교와 publish는 한 임계구역이다.** MANIFEST 교체·staged 승격·영수증
  finalize는 `CONTROL 읽기 → (expected_epoch, expected_generation) 비교 →
  publish`를 같은 세션 잠금 안에서 수행한다. 비교만 잠금 안에서 하고
  publish를 밖에서 하면, 그 사이 새 writer가 epoch을 올린 뒤 구 writer의
  rename이 성공하는 경합이 남는다. generation 단독 CAS로는 새 lease
  이후의 stale writer를 막지 못한다.
- P0 장애 시험에 "stale-break 후 늦게 복귀한 구 writer의 publish가
  거부됨"을 포함한다.
- Persist·Restore·Discard·TrimTo·GC·ACCESS 갱신은 lease 하에서만 진행한다.
- 다중 샤드 연산은 세션의 전 cut lease를 **stage_index 오름차순**으로
  획득하고, 하나라도 실패하면 획득분을 전부 해제한 뒤 재시도한다 — 획득
  순서의 전순서가 교착을 막는다. "세션당 코디네이터 하나"는 OUTER 단일
  권한 원칙에서 오는 전제이고, 위반 시의 방어선이 이 잠금 순서와 epoch
  fencing이다.
- 용어: 이 문서의 epoch은 세션 저장소 fencing epoch(`store_epoch`)이다.
  파이프라인 fragment 정체성의 `stream_epoch`(계획 P4.5)과는 다른
  개념이며 이름을 공유하지 않는다.
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
오답을 만들므로 금지다. TrimTo는 세션 lease와 epoch 아래에서만 발행되고
영수증에 (position, tokens_sha256, epoch)를 기록하며, 부분 커밋의 수렴은
2PC 표의 TrimTo 행(roll-forward)이 소유한다. 구현 단계는 계획 P3이
소유한다.

TrimTo는 모든 메모리 계열에서 가능한 연산이 아니다. recurrent 상태는
임의 suffix 절단이 불가능하고(`llama-memory-recurrent.cpp::seq_rm` @ upstream d7a20741 —
"can't have a state partially erased at the end"), 부분 롤백은 보관된
스냅샷 깊이 안에서 단일 사용으로만 성공하며 그 밖은 false를 반환한다.
따라서 스테이지는 `trim_support = arbitrary | bounded:<depth> | none`을
capability로 보고하고, TrimTo prepare는 각 스테이지의 trim_support와 남은
롤백 범위를 attest한다. 하나라도 대상 position을 감당할 수 없으면
TrimTo를 발행하지 않고 **전체 재프리필로 강등**한다.

영속 레코드는 상주보다 앞서 있을 수 있고(레코드 position > 마지막 절단
position), `tokens.bin`은 상태 import 없이 읽을 수 있다. 따라서 복원
판정은 **import 전에** 끝낸다:

1. meta·tokens 검증(체크섬·정체성 대조) 후 요청 프롬프트와 LCP 계산
2. LCP = 레코드 position(정확한 접두) → 셀 예약 → Restore → suffix 프리필
3. LCP < 레코드 position(분기) ∧ 전 스테이지 trim 가능 → 예약 → Restore
   → TrimTo(LCP) → suffix 프리필
4. 분기 ∧ 어느 스테이지든 trim 불가 → **Restore 자체를 생략**하고 전체
   재프리필 — 죽은 suffix를 import했다 지우는 낭비와 부분 실패 경로를
   없앤다
5. 어떤 경로든 LCP 판정 없이 복원된 suffix 위에서 디코드 시작 금지

## 2PC 수렴 규칙

**P2 구현 요구(전제 아님):** PreparePersist는 내구 staged 번들을 만들어야
한다. 현행 prepare는 영수증만 쓴다(`transaction_store.cpp::TransactionStore::prepare`
@ df5b9ce7). 이 요구가 구현되기 전에는 아래 Persist 복구 보장이 성립하지
않는다.

영수증 수명은 `Prepared → Committing(내구) → [부작용] → Committed(내구)`다
(`server.cpp::Session::handle` KvCommit 분기 @ df5b9ce7; 상태 정의는
`protocol.hpp::KvReceiptState`). 따라서 부작용 전·중·후의 정지는 모두
`Committing`으로 관측되며, **내구 증거로 부작용 완료 여부를 판정**해야
한다. 어댑터 Reconcile이 Committing을 Inconsistent로 축약하는 현행 동작은
P2에서 이 판정으로 교체된다.

| 연산 | 관측(재시작 포함) | 판정 증거 | 수렴 |
| --- | --- | --- | --- |
| Persist | 전원 prepared, 전 스테이지가 동일 epoch의 resident를 attest | resident attest ×N | rollback → all-resident (Abort, staged 폐기) |
| Persist | 전원 prepared이나 resident attest 실패(재시작 포함 — resident는 휘발) | staged 사본 유효 | **roll-forward → all-persisted**: 재시작 후 Abort로는 resident를 되살릴 수 없다 |
| Persist | prepared, resident 소실 + staged 무효/부재 | — | Inconsistent(정지) → 재프리필 강등 |
| Persist | committed ≥1 + prepared 잔여 | staged 사본 존재 | **roll-forward → all-persisted** (잔여 Commit 재시도; committed는 Abort로 되살릴 수 없음) |
| Persist | Committing, 최종 번들 유효 | 번들 체크섬 일치 | roll-forward: finalize 재수행(멱등) |
| Persist | Committing, 최종 없음·staged 유효 | staged 체크섬 | roll-forward: 재승격 후 finalize |
| Persist | Committing, staged·최종 모두 무효 | — | Inconsistent(정지). staged 요구가 지켜지면 도달 불능 |
| Restore | 전원 prepared | 레코드 무손상 | rollback → all-persisted |
| Restore | committed ≥1 또는 Committing | 임포트는 휘발, 레코드 무손상 | **rollback → all-persisted** (임포트 스테이지 `seq_rm` 철회 후 전체 재시도) |
| Discard | committed ≥1 또는 Committing | 번들 잔존 여부 | roll-forward → absent (잔존 시 삭제 재수행, finalize) |
| TrimTo | prepared만(어느 스테이지도 미절단) | — | rollback (Abort) |
| TrimTo | 부분 commit(일부 스테이지만 `seq_rm`) | 영수증의 (position, tokens digest, epoch) | **roll-forward**: 잔여 스테이지에 동일 position 절단 재시도(멱등). TrimTo는 코디네이터가 분기 suffix의 폐기를 확정한 뒤에만 발행되므로 전진만이 안전하다 |
| 모든 연산 | manifest/체크섬 불일치 | — | Inconsistent: 정지, 자동 재시도 금지, 레코드 폐기 표시, 재프리필 강등 |

현행 코어 코디네이터(`layers/service/src/cache.rs::recover` @ df5b9ce7,
시험 `recovered_partial_restore_is_failed_closed`)는 부분 Restore와
Aborting 중 committed 발견을 즉시 실패로 접는다 — 그 시험은 현행 동작의
고정이지 이 표의 구현이 아니다. Persist roll-forward와 Restore rollback
경로는 llama 지식 없는 백엔드 중립 코어 수정으로 추가한다(계획 원칙 1).

## 스냅샷 명령 모델 (2026-08-31 방향 확정)

영속화의 트리거는 타임아웃 하나가 아니다. 분기 에이전트 워크로드에서
"기존 KV를 영속화해 복사한 새 세션으로 트리를 분기"하는 것은 일상
연산이고(LM Studio mlx-engine agentic workloads, llama.cpp/LM Studio의
Context Checkpoints가 같은 방향의 선례), **정책 — 언제·무엇을·왜 — 은
전부 OUTER 소유**다. 어댑터·노드는 자발적으로 영속화·언로드하지 않으며,
다음 명령 어휘를 이행만 한다. TTL·quota·분기 시점은 모두 OUTER가 이
어휘로 표현하는 정책이다.

| OUTER 명령 | `CacheAction` 대응 | 상태 |
| --- | --- | --- |
| 세션 S를 키 K로 영속화하고 **상주 해제** | `Persist` (+2PC Prepare) | 있음 — cache_key를 시퀀스 복사가 아니라 OUTER 지정 snapshot key로 바꾸는 수정 필요(`cache_direct.inc.rs::cache_key` @ 87ec1317) |
| 세션 S를 키 K로 영속화하되 **상주 유지**(족적 남기고 계속) | **`Checkpoint` 신설 필요** | 없음 — 현행 `Persist` 주석은 "resident로 두는 persist는 아무것도 해제하지 않으므로 한 동사"라고 논증하는데(`work/cache/mod.rs::CacheAction::Persist` @ 87ec1317), 이 논증은 분기 족적 용례를 보지 못했다 |
| 세션 S의 영속 키 목록 | **`SnapshotList` 신설 필요** | 없음 — `Reconcile`은 단일 operation 영수증 조회다. 교집합 단위는 키가 아니라 **논리 스냅샷**이다 — `{snapshot ref generation, operation_id, position, tokens_digest, state_abi_id, kv_variant_id}` 전 필드 일치. 필드 불일치·부분 존재는 Inconsistent(사용 불가) |
| 세션 S의 키 K로 **세션 T를 로딩**(디스크 분기) | `Restore`를 target 지정으로 확장 | 부분 — 현행 Restore는 "같은 id로"다. target은 상주 상태가 비어 있어야 하며, 복원 판정 사다리가 T의 요청 프롬프트에 대해 그대로 적용된다 |
| 상주 세션의 즉시 분기(메모리 복사) | `Fork { into }` | **있음** — "copies rather than aliases" 계약 그대로 |
| 세션 언로드(영속 없이 해제) | 기존 release 경로 | 있음 |
| 키 K 폐기 | `Discard` (+2PC) | 있음 |

- `Persist`/`Checkpoint`는 별도 상태기가 아니라
  `Snapshot { after_commit: KeepResident | ReleaseResident }` 한 형상으로
  구현한다 — durable publish까지 동일하고 후조건만 다르다(8차 리뷰 권고
  수용; llama.cpp 업데이트 표면 최소화).
- 스냅샷은 **불변**이다. 같은 키로의 재영속화는 그 키의 gen-N 체인이
  받는다(supersede = 키 내부 CAS). 서로 다른 키는 서로를 대체하지 않는다.
- 디스크 분기(RestoreInto)는 레코드를 복사하지 않는다 — 조건부다:
  ① 같은 storage domain에서 읽고 ② cut·정체성 호환이 성립하며 ③ **전
  스테이지 import 완료 후에만** 원본과의 관계를 끊는다. 그 전의 원본
  Discard는 read-pin이 막아야 하고(O10), 노드 이동·cross-domain은 복사
  경로다. 이후 T의 영속화는 T의 세션 디렉터리에 쓴다.
- 배치 결합: Checkpoint·Persist·Fork는 대상 시퀀스가 **정지점**(in-flight
  행 없음, 전 스테이지 정산)에 있을 때만 실행된다. 펜스와 삽입 일정은
  batching 계약([adapter-batching-layers.md](adapter-batching-layers.md)
  불변식 11)이 소유한다.
- 세션 lease·CONTROL·epoch는 세션 수준 그대로다 — 같은 세션의 서로 다른
  스냅샷 연산도 직렬화된다(단순함 우선; 병목이 실측되면 그때 키 단위로
  세분한다).
### 저장 계층 (tier)

스냅샷 명령은 `tier` 인자를 갖는다 — 목적지는 디스크만이 아니다.

| tier | 매체 | 생존성 | 용도 |
| --- | --- | --- | --- |
| durable | SSD — 이 규약의 파일 기계장치 전체 적용 | 프로세스·재부팅 생존 | 장기 족적, 세션 이동 |
| ram | 호스트 CPU 메모리 — 파일 기계장치 미적용, 프로세스 내 보관 | **휘발**: 프로세스 종료로 소멸 | 과부하 공정 스왑: 일부 세션 KV를 RAM으로 내리고 다른 요청을 처리한 뒤 재적재 |
| resident | backend(VRAM) 내 체크포인트 | 컨텍스트 수명 | 고속 롤백·분기 (O7, capability 협상 후) |

- ram 계층: `llama_state_seq` 바이트를 파일 대신 호스트 메모리에 보관한다
  — llama-server의 `--cache-ram`/idle-slot offload가 같은 방향의 선례다.
  크래시 수렴은 **Absent이지 Inconsistent가 아니다**: 휘발 계층의 소실은
  손상이 아니라 부재다. 재시작을 건너 살지 않으므로 cross-pin state ABI
  부담도 없다. 영수증은 tier를 기록하고, ram 레코드를 가리키는 영수증은
  재시작 후 Absent로 해소된다.
- ram 바이트는 GPU 셀과 별개의 수용 회계 축이다(호스트 바이트;
  MEMORY_ACTUAL host 항목과 텔레메트리로 보고). durable 디스크 바이트
  예산과 함께 O11이 소유한다.
- 스왑 정책은 전부 OUTER 명령이다: 스왑 아웃 =
  `Snapshot{tier=ram, ReleaseResident}`, 스왑 인 = `Restore(ram)`. 배치
  정합 펜스(불변식 11)가 동일하게 적용된다.
- 이로써 `max_resident`는 논리적으로 초과 가능해진다 — GPU 셀 상한은
  "동시 상주" 상한이지 "살아있는 세션" 상한이 아니게 되고, 스왑 왕복
  비용과 TTFT 영향의 트레이드오프는 OUTER 정책의 몫이다.

## 예약 2PC

다중 노드 셀 예약과 다중 샤드 lease 획득의 "실패 시 전부 해제"는
네트워크가 정상일 때만 성립하는 문장이다. 예약은 2PC로 정의한다.

- Prepare: `(reservation_id, session_key, requested_cells, ttl)`.
  TTL은 절대 시각이 아니라 **각 노드가 Prepare를 수신한 시점 기준의 로컬
  단조 시계**로 평가한다 — 호스트 시계 편차를 권위에서 배제한다.
- 전 스테이지 Prepared → Commit. Abort/Release는 멱등이다.
- **Prepared 예약도 수용 회계에 포함**되며 텔레메트리의 `reserved_cells`
  필드로 코디네이터에 보인다 — 안 보이면 over-admit이 예약 경로로 재발한다.
- 코디네이터 재시작은 Reconcile로 미결 예약을 수렴시키고, 통신 단절 시
  각 노드는 로컬 TTL로 자동 회수한다.
- 장애 시험: partial prepare, release 유실, 코디네이터 사망 — 어느
  경우에도 예약 잔류 0(TTL 회수 확인).

## 수명 규약

- 접근 시각은 불변 번들 밖의 `ACCESS` 파일이 담는다(원자 교체, 단조
  최대값, 조언적). 불변 `meta.json` 안에 두면 접근마다 불변성이 깨지거나
  접근 회계가 내용 세대와 뒤섞인다. GC는 ACCESS를 읽고, 손상 시 가장
  오래된 것으로 간주한다. 단 ACCESS는 **노드 로컬 디스크**에 있으므로
  코디네이터의 victim 선정 입력은 파일 읽기가 아니라 어댑터가 와이어로
  올리는 세션별 `{last_access, position, bytes}` 텔레메트리다(D12의 점유
  보고와 같은 채널). ACCESS 파일은 그 텔레메트리의 재시작 생존용 로컬
  영속화일 뿐이다.
- 부팅 시 `tmp/` 잔재 제거. MANIFEST 또는 유효 영수증이 증명하지 않는
  고아 `gen-*`은 **노출 없이 격리 또는 GC**한다 — publish되지 않은 세대를
  재색인으로 살리는 것은 금지다.
- 새 publish 성공(=MANIFEST CAS 성공) 후 구세대 삭제.
- 어댑터는 어떤 스냅샷도 **자발적으로 만들거나 지우지 않는다** — TTL
  판단을 포함한 모든 트리거는 OUTER의 명령이다.
- 노출된 레코드의 삭제는 항상 **코디네이터가 발행하는 4-스테이지 Discard
  2PC**다. 세션 레코드는 여러 cut의 샤드 집합이므로 각 노드가 ACCESS만
  보고 독립 삭제하면 partial-absent 상태를 만든다. TTL·quota victim
  선정은 코디네이터 소유이며 ACCESS는 그 입력이다.
- 로컬 GC는 MANIFEST가 노출하지 않는 고아 세대와 tmp 잔재 정리로
  한정한다.

## 남는 열린 항목

- 128MB 초과 번들의 `state-<k>.part` 경계·체크섬 규약.
- 수용-조건 등급 인하 대상의 실증 행렬(계획 P2 소유).
- DENIED 메모리 계열의 보조 상태 수용은 3축 감사(backend conformance
  포함) 통과 이후.
