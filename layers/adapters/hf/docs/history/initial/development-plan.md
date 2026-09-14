> 역사 기록: 독립 저장소 시점의 요구·상태·측정이다. 현재 배치와 사용법은 [HF 안내](../../../README.md)를 따른다. 원본 전체는 이관 시 보존한 Git bundle에 있다.

# 최초 개발 실행 계획

작성일: 2026-09-13. 지위: 구현 작업 분해안. 별도로 구현 사실을 표시하지 않은 산출물은 예정입니다.
단계 상태는 [로드맵](roadmap.md), 시험 정의는 [검증 계획](testing.md), 확정 여부는
[결정 기록](decisions.md)이 소유합니다. 각 단계의 완료는 실제 시험 증거로 판정합니다.

개발 착수 후속: S4a의 독립 IPC를 먼저 구현했고, 사용자 모델 지정 후 [Qwen 전용 스크립트](../../models/qwen3_5_0_8b/README.md)를 추가했습니다.
노드 계획·부분 적재·stage forward·로컬 worker를 구현했으며 범용 모델 framework는 만들지 않습니다.
모든 구현은 [역할별 폴더 규칙](../../structure/README.md)을 적용합니다.

## 확인한 출발점

- 이 저장소 기준 HEAD: `2ddd508a19f58921cfcbd6956d192565877eaa55`; 계획 작성 전 dirty 없음.
- 계획 작성 당시 추적 파일은 문서와 Git 설정뿐이었고 Python/Rust 패키지·CLI·시험·모델 구현은 없었습니다.
- P4 재확인 HEAD: `484b856ee7e53aea5b850b654c45da53cb0724a6`; 기존 dirty 존재.
- 해당 P4 commit의 `RetainedNodeAdapter`, node create, adapter/agent Cargo 파일은
  최초 참고 `d122125bafeaa6d32790761669f1bfa5868d8078` 대비 diff가 없습니다.
- node create는 `llamacpp`만 생성합니다. 실제 retained 소비 구현은
  `layers/agent/src/event_node/retained.rs`이며 `mod.rs`에서 재노출합니다.
- 이번 계획은 로컬 문서·코드 관측에 근거합니다. 상류 라이브러리 지원·버전·장치 호환성은 S1에서 재검증합니다.

## 첫 개발 목표와 순서

모델마다 전용 Python 코드를 작성하는 것이 개발의 중심입니다. 첫 모델의 loader·forward·cache/state·
분할·양자화를 직접 구현하며, 범용 처리기·모델 registry·공통 모델 인터페이스를 먼저 만들지 않습니다.
P4 연결과 이벤트·자원 소유권 계약을 유지하는 것과 모델 실행을 범용화하는 것은 별개입니다.

첫 개발 묶음의 목표는 **선정한 실제 모델을 담당 레이어만 적재하여 prefill과 반복 decode를 실행하고,
동일 양자화 비분산 기준선과 logits·상태를 비교할 수 있는 단일 프로세스 실행기**입니다(S1~S3).
원본 대비 품질과 실제 압축 실행도 별도로 증명합니다. 먼저 모델의 부분 실행 가능성을 고정한 뒤
Rust bridge·IPC·다중 호스트로 확장합니다.

`S1 조합 고정 → S2 기준선/부분 적재 → S3 부분 forward → S4 로컬 bridge/worker → S5 물리 분산/연속 요청 → S6 P4 통합`

S1~S3에서는 연속 레이어 PP, 한 번에 하나의 유한한 실행 스텝, 명시적 상태 소유자를 기본 제안으로 둡니다.
4bit·특정 모델·특정 OS·IPC transport를 사용자 확정값으로 취급하지 않습니다.

## 착수에 필요한 결정

| 입력 | 필요한 내용 | 없을 때 가능한 일 |
| --- | --- | --- |
| 목표 모델 | 정확한 HF ID, 원하는 가중치 또는 원본, 텍스트/필수 기능 범위 | manifest 필드와 P4 경계 감사 |
| 실행 장치 | 사용할 물리 호스트, GPU/메모리, OS, RAM, 네트워크, 접근·실행 허용 범위 | 독립 로컬 계약 설계 |
| 목표 workload | context, 동시 요청, 최대 출력, 정상 응답 기준, 우선할 품질/지연/처리량 | 평가 항목과 기록 형식 설계 |

모델 revision·제조사 code revision·커널/패키지 호환 조합·합법 cut·구체적 자원 예산은
개발자가 이 입력을 바탕으로 조사합니다. 사용자가 기술 세부값을 모두 미리 정할 필요는 없습니다.
허용 오차·품질·SLO 수치는 첫 측정 전에 확정하며, 미정값이 남은 실행을 수용 시험으로 표시하지 않습니다.
목표 모델을 임의 대체하거나 P4의 fleet을 자동으로 이 프로젝트 시험 장치로 사용하지 않습니다.

## S1 — 실행 조합과 검증 계약 고정

1. 목표 revision의 제조사 forward/generation 코드를 따라 모듈·상태 지도를 작성합니다.
   embedding, position/RoPE/mask, layer index, cache mutation, norm/lm_head, sampling을 추적합니다.
   shared KV·recurrent·tied weights·MoE 등 실제 존재하는 구조의 합법/불법 cut을 정합니다.
2. 공개 양자화 아티팩트를 먼저 조사하고 원본 기반 변환 후보와 비교합니다.
   모듈 coverage, 보조 tensor, 실행 kernel, 장치 제약, 원본 reference 확보 방법을 표로 기록합니다.
3. 장치별 weight/metadata/KV/state/activation/workspace/IPC buffer와 load peak를 계산합니다.
   큰 모델의 reference는 검증 가능한 offload/다중 장치 구성을 명시합니다.
4. 공식 자료와 선택 소스에서 확인한 Python/PyTorch/Transformers/양자화 의존 조합을 고정합니다.
   Rust bridge의 P4 참조 revision과 단일 crate source 원칙도 기록합니다.
5. 첫 모델의 고정 실행 명세와 최소 검증 코드를 작성합니다. 조사 중인 설정과 봉인 manifest를 구분하고
   누락 identity·미지원 조합·예산·cut을 검사합니다. 여러 모델을 표현하는 범용 schema 엔진은 만들지 않습니다.
   모델 tensor/state 검사는 S3, 외부 입력의 frame/shape/byte 검사는 S4의 실제 소비 경로에 구현합니다.

예정 산출물: `manifests/` 첫 모델 실행 명세, `docs/models/<model>.md` 감사 결과,
첫 모델 Python 패키지의 최소 실행 설정·검증 코드, Python 의존 lock.
종료 조건: 모델/장치/커널 후보에 소스 근거가 있고 revision·cut·평가 입력·허용값·실행 상한이 고정되어야 합니다.
이 단계의 지원 판정은 정적 호환 근거이며 실제 압축 실행 승인은 S2에서 합니다.

## S2 — 기준선과 실제 부분 적재

1. 제조사 실행과 같은 tokenizer/template/position/stop을 사용하는 reference runner를 만듭니다(REF-01).
2. 동일 입력의 고정밀도와 양자화 실행에서 logits·출력 전문·상태·커널·메모리를 기록합니다(Q-01~03).
3. stage에 필요한 weight와 scale/zero point/group/packing metadata만 읽는 loader를 구현합니다(Q-04).
   전체 모델을 먼저 GPU에 올린 다음 다른 레이어를 삭제하는 방식을 쓰지 않습니다.
4. 첫 forward와 장문/최대 배치 실행 후 압축 유지·메모리 peak를 확인합니다.
   요구 커널 대신 전체 dense 복원으로 전환되면 성능용 LOAD를 거부합니다.

예정 산출물: `models/<model>/loading/`·`quantization/`·`reference/`, `artifacts/<run-id>/` 결과.
종료 조건: REF-01·Q-01~04와 원본 대비 품질 기준 충족. 커널/부분 적재가 성립하지 않으면
S1의 recipe를 수정하고 새 manifest로 다시 검증합니다. 분산 단계로 문제를 넘기지 않습니다.

## S3 — 단일 프로세스 부분 forward

1. 첫 stage의 입력 준비, 중간 stage 연산, 마지막 stage의 norm/lm_head를 모델별로 분리합니다.
2. 요청별 cache/state, 전역 layer index, position, boundary tensor schema를 구현합니다.
3. 비분산과 stage 분할에 동일한 입력 token을 공급해 prefill 및 반복 decode의 logits·상태를 비교합니다.
   이후 자유 생성 응답도 평가하여 수치 오차와 토큰 분기 영향을 구분합니다.
4. 여러 길이·padding·cache 진척·최대 context 경계, 합법 cut과 불법 cut을 검증합니다(MOD-01~02).

예정 산출물: `models/<model>/forward/`·`state/`·`boundary/`, 역할별 parity 시험, cut 검증 보고.
종료 조건: 동일 양자화 기준선과 고정된 허용 오차 내 parity, 비담당 weight/state 부재,
불법 cut의 실행 전 거부. 이것이 첫 개발 묶음의 완료 지점입니다.

## S4 — 독립 Rust bridge와 Python worker

| 순서 | 구현 묶음 | 완료 증거 |
| --- | --- | --- |
| S4a | worker Inspect/Load/BindSession/ExecuteStep/Release/Unload, bounded IPC와 tensor codec | 실제 별도 Python 프로세스 왕복, WIRE-01, 정상 LIFE-01 |
| S4b | `crates/p4-hf-adapter/` retained trait·원장·출력 승인, 독립 event test host | P4 중립 node/broker 소비 경로의 BR-01·SET-01 |
| S4c | Cancel/Quiesce, generation fencing, crash/timeout/partial frame, 정리와 재수용 | BR-02·LIFE-02, 실패 이후 LIFE-01, 수정 제거/변이 검증 |

IPC는 S1의 지원 OS와 S3의 tensor 크기를 기준으로 한 방식을 선택하고 길이 제한·큐/byte 예약을 먼저 정의합니다.
stdout 로그와 wire frame을 분리하고 pickle/raw GPU pointer를 wire로 사용하지 않습니다.
초기 worker는 직렬 장치 실행으로 시작하며 수용·장치 완료·정산·출력·상태 반환을 각각 기록합니다.
늦은 completion/RELEASE는 새 incarnation에 효과를 내지 못해야 합니다.

독립 test host는 이 저장소에 둡니다. P4의 중립 crate는 고정 revision으로 참조하며,
한 빌드에서 path/Git 혼용으로 타입이 중복되지 않게 확인합니다. target·lock·환경은 이 저장소가 소유합니다.
mock은 오류 주입을 보조하지만 실제 worker/모델 소비 경로 검증을 대체하지 않습니다.

## S5~S6 — 분산 수용과 제품 통합

| 순서 | 작업 | 진입/종료 조건 |
| --- | --- | --- |
| S5a | 최소 두 실제 물리 호스트에서 고정 stage·단일 요청 PP | 장치/실행 권한 확보, DIST-01과 목표 모델 품질·반환 |
| S5b | 다른 길이·도중 합류·prefill/decode 혼합, 전 stage 동일 membership | BATCH-01, 상한 준수, 취소 후 재수용 |
| S5c | 장단문·긴 context·강한 연속 웨이브, 주장할 이기종 recipe 조합 | WAVE-01, 해당 시 HET-01, 고정 품질/SLO와 자원 상한 |
| S6 | P4 composition root의 kind 등록·의존·worker 배포 연결 | 별도 통합 지시 후 INT-01·기존 adapter 회귀·실제 제품 소비 |

S5의 노드 간 tensor는 adapter payload로 P4 event 전달·backpressure 경계를 통과시킵니다.
Python worker가 별도 직접 송신 경로로 이를 우회하지 않습니다. 최적화는 정확성 통과 후 측정에 근거해 선택합니다.
성능은 [검증 계획](testing.md)의 전체 wall time 분모와 정상 완료 토큰 기준을 따릅니다.
실패·미완료·length·품질 탈락·분류 불가를 포함하고 실제 측정 구간의 GPU/전송/대기를 함께 기록합니다.

## 변경 단위와 재개 기록

- 구현 묶음마다 관련 실패 반례 → 실제 소비 경로 구현/검증 → 독립 복사본 변이 순서로 증거를 남깁니다.
- 첫 커밋 묶음은 S1 조합/manifest, S2 reference/loader, S3 stage parity 순서로 구분하되,
  각각 복원 가능한 작은 단위로 나눕니다. 이후 S4a~c와 S5a~c에도 같은 원칙을 적용합니다.
- 매 복원 지점에 source/환경/artifact hash, 실제 명령·exit code·시험 결과, 첫 오류·cleanup 오류·
  evidence_missing, 미완 항목과 다음 첫 행동을 남깁니다. 원본 artifact는 무시 경로, 재현 계약/요약은 Git에 둡니다.
- 단계 통과 여부는 roadmap에만 갱신하고 HANDOFF에는 다음 실행과 해당 기록을 연결합니다.
- 목표 모델/장치 확정 전에는 기간·성능 수치를 약속하지 않습니다. S1의 모델 감사·reference 자원 산정 후
  S2~S3 작업량과 장치 확보 일정을 추정합니다.

다음 첫 행동: 위 세 입력을 확보하고 S1의 모델 연산·상태·양자화 coverage 표부터 작성합니다.
