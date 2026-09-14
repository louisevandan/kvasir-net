> 역사 기록: 독립 저장소 시점의 요구·상태·측정이다. 현재 배치와 사용법은 [HF 안내](../../../README.md)를 따른다. 원본 전체는 이관 시 보존한 Git bundle에 있다.

# 다음 세션 인수인계

2026-09-14. P4 개발 계획 §0의 HF-0~3을 구현·검증했다.
현재 결과와 제한은 [수용 보고](../../../tests/reports/p4-integration/20260914_023000.md),
실행·예산·배포 계약은 [통합 명세](../../integration/README.md)가 소유한다.

## 구현과 검증

- HF가 독립 Rust bridge 및 모델별 Python을 소유한다. P4는 optional feature, 외부 의존성과 factory/INSPECT를 연결한다.
- OUTER 모델 controller는 P4 event를 사용한다. 각 stage는 담당 weight/cache와 Python 프로세스를 소유한다.
- 같은 LOAD에서 8건×3 epoch, 취소·release·이전 epoch 거부·UNLOAD/DELETE, 호환 Python A→B 교체를 검증했다.
- 실제 Qwen: 기존 v1 8조합47스텝, 최종 local event99스텝/198 stage-cache,
  두 물리 host75스텝/150 stage-cache, 응답 단절 뒤 재수용4스텝/8 stage-cache를 통과했다.
- 기존 llama.cpp는 feature on/off 모두 실제 GGUF 정상 생성·회수·UNLOAD/DELETE를 통과했다.
- P4 전체 Rust는 각1450 passed/0 failed/7 ignored, 외부 HF9개는 별도다.
  Rust 변이5종, cache/epoch3종, 기존 Qwen4종을 독립 복사본에서 검출했다.
- 정확한 source/binary/model/host/명령/실패/정리 증거는 수용 보고에 있다. 원격 push는 수행하지 않았다.

## 새 작업 시작

1. 양쪽 HEAD·dirty와 수용 보고의 runtime source 차이를 먼저 감사한다. 두 저장소는 별도 Git/workspace다.
2. [AGENTS](../../../AGENTS.md)와 [폴더 규칙](../../structure/README.md)을 읽는다.
   역할이 다르면 별도 폴더로 나누고 모델별 연산·배치·상태를 Python 안에 둔다.
3. 인접 source 복원은 통합 명세의 build bundle/standalone restore.py를 사용한다.
   optional path 특성상 feature off도 HF source가 필요하다. 환경/weight는 별도 준비한다.
4. 검증을 재실행할 때 새 출력 디렉터리와 새 node generation을 준다. 실행 중인 bundle을 덮어쓰지 않는다.
5. 현재 P4 후속 순서는 Release A다. 이번 §0 완료로 대형 모델 작업·원격 변경·push 권한을 자동 확대하지 않는다.

## 보존한 제한

RTX4080+3090 BF16 분할은 logits 기준 초과 FAIL이다. FP32 통과로 덮거나 허용 오차를 높이지 않았다.
[기존 모델 명령](../../models/qwen3_5_0_8b/README.md)은 독립 v1이며 누적 active+retired 한도를 유지한다.
v2는 drain된 epoch에서만 새 bounded 상태를 만든다.
소형 Qwen conformance는 초대형 H0–H7/성능·SLO가 아니다. 양자화, physical batching, 임의 모델,
host 전체 장애의 durable 복구는 수용 범위 밖이다.

이전 2026-09-13의 “P4 읽기 전용/미연결”은 당시 작업 범위다.
2026-09-14 사용자가 양쪽 저장소의 구현·후속 검증을 승인했다. 과거 계획·보고는 이력으로 보존한다.
모델/환경/상세 로그는 Git 제외 models/.venv/artifacts/target에 있으며 추적 manifest와 lock으로 재구성한다.
