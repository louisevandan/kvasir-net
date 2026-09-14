> 역사 기록: 독립 저장소 시점의 요구·상태·측정이다. 현재 배치와 사용법은 [HF 안내](../../../README.md)를 따른다. 원본 전체는 이관 시 보존한 Git bundle에 있다.

> 2026-09-14: 사용자 §0 지시로 P4 통합 구현을 진행한다. 이전 미연결/읽기 전용 설명의 현재 상태는 [통합 명세](../../integration/README.md)와 그 수용 보고가 우선한다.

# 검증 계획

지위: 아래 전체 제품 수용 시험은 미완입니다. 독립 Qwen 모델 스크립트의 검증은
[Qwen 계획](../../../tests/plans/qwen3_5_0_8b-20260913.md)과 [실행 보고](../../../tests/reports/qwen3_5_0_8b/20260913_220709.md)를 따릅니다.
하위 로컬 IPC framing 시험도 실행했습니다.
그 범위와 결과는 [계획](../../../tests/plans/framing-20260913.md)과
[보고](../../../tests/reports/framing/20260913_174516.md)에 있습니다. WIRE-01 전체 통과로 계산하지 않습니다.
초기 문서 점검은 [초기화 기록](bootstrap-evidence.md)을 참조합니다.

## 비교 기준 분리

| 기준선 | 확인할 것 |
| --- | --- |
| 제조사 고정밀도 실행 | 원본 의미와 정상 응답 품질 |
| 동일 양자화의 비분산 실행 | 양자화 자체의 오차/품질/커널/메모리 |
| 동일 양자화의 단일 프로세스 stage 분할 | 부분 forward와 cache/index/cut 오류 |
| 동일 아티팩트의 다중 프로세스/호스트 | codec·순서·정산·장치 조합의 추가 영향 |
| 기존 llama.cpp의 동등 비교 arm | 실제 대안 비교; format 차이는 명시하고 분산 오차 시험과 분리 |

모델이 너무 커서 한 장치 기준 실행이 불가능하면 검증된 reference의 offload/다중 장치 실행을 명시합니다.
작은 모델로만 통과시킨 뒤 목표 초대형 모델 정확성까지 승인하지 않습니다.
허용 logits 오차·품질 하락·성능/SLO 기준은 모델 manifest에서 시험 전에 고정합니다.
샘플링의 미세 수치 차이가 토큰을 갈라놓을 수 있으므로 logits/상태 비교와 응답 의미 평가를 함께 사용합니다.

## 예정 시험 대장

| ID | 실제 소비 경로와 반례 | 필요한 증거 |
| --- | --- | --- |
| REF-01 | 제조사 예제와 동일 tokenizer/template/stop의 기준 실행 | 입력·출력·logits·version/hash |
| Q-01 | 모든 대상 모듈의 압축 적용 및 제외 모듈 | 실제 module/kernel·가중치/metadata byte |
| Q-02 | 첫 forward에서 전량 복원되는 fallback 감지 | load/첫 실행/장문 peak·실제 dtype·명시 거부 |
| Q-03 | 보정과 독립 평가, 원본 대비 품질 | 데이터 digest·응답 전문·고정 judge·오차 |
| Q-04 | stage 부분 적재에서 비담당 weight/state 부재 | tensor/allocator 목록·peak·누락 metadata 거부 |
| MOD-01 | 단일 프로세스 분할의 prefill/decode parity | 여러 길이·cache position·logits/상태 대조 |
| MOD-02 | shared KV/recurrent/tied/MoE의 불법 cut | 효과 전 거부와 합법 cut의 reference parity |
| BR-01 | retained 이벤트 Full/Closed/peek/take 실소비 | 원본/예약 소유권 보존·중복 소비 없음 |
| BR-02 | worker crash/IPC timeout/partial tensor | 최초 오류·uncertain·잔존 상태·정리 결과 |
| WIRE-01 | dtype/shape/length/version/identity 오염 | native 실행·출력·상태 효과 0 |
| LIFE-01 | LOAD/SESSION/요청/해제/UNLOAD 전체 흐름 | 실제 state/메모리 반환과 다음 정상 요청 |
| LIFE-02 | 취소·EOS·상한·timeout 후 지연 completion/RELEASE | 새 incarnation 보호·예약 보존/반환 |
| SET-01 | 잘못된 membership/receipt, 출력 전송 실패 | 원장 승인 전 출력 0·effect pending/uncertain 보존 |
| BATCH-01 | 다른 길이·생성/프리필 혼합·도중 합류 | stage별 동일 membership·순서·진척·상한 |
| DIST-01 | 최소 두 실제 물리 컴퓨터의 목표 모델 실행 | host identity·stage hash·정상 응답·반환 |
| HET-01 | 다른 장치/양자화 recipe stage의 연결 | boundary dtype·전체 품질·실제 커널·메모리 |
| WAVE-01 | 반복 강한 연속 웨이브·장단문·긴 context | 정상 응답·TTFT/ITL·유효 TPS·공정성·자원 상한 |
| INT-01 | 미래 실제 P4 entrypoint 소비와 기존 adapter 회귀 | 양쪽 고정 revision·빌드·중립성·실제 이벤트 시험 |

미지원 모델 기능의 시험은 N/A 이유를 명시합니다. 필수 시험을 feature/ignored로 숨겨 통과를 주장하지 않습니다.
HET-01은 이기종 지원을 주장할 때 필수입니다. INT-01은 P4 변경 권한과 통합 단계에 도달한 뒤 실행합니다.

## 실패 반례와 변이

기능 수정 전에 실패 반례를 확보하고 실제 worker/bridge 소비 경로에서 재현합니다.
수정 제거 또는 핵심 검사 제거 변이는 독립 복사본/검증용 checkout에서만 실행합니다.
소스/바이너리/환경 hash와 재컴파일 여부를 기록해 공유 cache의 baseline 재사용을 제외합니다.
실패를 통과시키기 위해 입력·judge·허용 오차·상한을 사후 완화하지 않습니다.
Python import/bytecode와 worker 실행 경로도 변이 소스에 결속합니다.

## 성능 산식과 기록

- 고정 측정 구간의 정상 품질·완료 조건을 통과한 요청의 생성 토큰만 유효 TPS 분자에 포함합니다.
- 분모는 선언한 전체 서비스 측정 wall time입니다. 웨이브 사이 idle/대기도 임의로 제거하지 않습니다.
- raw TPS와 유효 TPS를 분리하고 실패·미완료·length·품질 탈락·분류 불가 요청을 모두 보고합니다.
- TTFT는 예정 송신/실제 송신/수용/첫 토큰 시각을 구분하고, ITL은 실제 연속 출력 간격으로 계산합니다.
- 모델·양자화·kernel·host/cut·batch/context·샘플링·workload·warmup/측정 경계를 봉인합니다.
- GPU 관측 구간은 측정 arm에 맞추고 transfer/compute/wait·실제 배치 인구를 함께 봅니다.
- 가장 좋은 결과는 '시험한 조건 중 최선'이며 탐색 범위 밖 최적성을 주장하지 않습니다.

## 실패 실행의 증거 보존

timeout/crash 때 소유 worker와 자원을 명시한 한도 안에 정리하고 가능한 부분 결과를 보존합니다.
요청 원문·출력 전문·token count·terminal/release·최초 오류·cleanup 오류·evidence_missing을 구별합니다.
artifact 복사 실패가 실행 정리를 무한정 늦추거나 실패 원문을 덮어쓰지 않도록 합니다.
최종 보고에는 실행 명령·exit code·전체 summary·미실행/ignored/실패와 소스 기준을 포함합니다.
