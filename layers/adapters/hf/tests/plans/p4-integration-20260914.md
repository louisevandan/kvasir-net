# P4 HF 통합 수용 시험 계획

생성일: 2026-09-14 KST. 목표는 P4 개발 계획 §0의 HF-0/1/2/3과 기존 llama.cpp 동작을 검증하는 것이다.

## 환경·전제

Windows 인접 P4/HF clean commit, Rust/Cargo, Qwen 고정 CPython 3.13.15/torch 2.14.0+cu130/transformers 5.17.0,
고정 checkpoint 및 두 물리 호스트가 필요하다. 기존 native llama server와 정상 GGUF를 별도 기록한다.
[실행 명세](../../docs/integration/README.md)와 P4 검증 규약을 먼저 읽는다.

## 순서와 기대값

1. 양쪽 HEAD/lock/dirty, Cargo metadata의 p4-adapter/protocol 각각 하나, 실제 factory와 INSPECT의 kind 일치를 검사한다.
2. HF crate의 실제 retained+Python pipe 시험: 원본 claim 거부, held output, wake, epoch, partial/death/timeout/identity,
   실패 후 façade와 abort를 검사한다. 이전 입력/예약/효과를 보존하며 거부 후 정상 요청이 가능해야 한다.
3. 독립 복사본에서 guard/identity/reservation 변이를 재컴파일한다. baseline 통과, 변이의 해당 시험 실패,
   source/binary hash와 서로 다른 target을 기록한다.
4. 기존 v1 8개 조합/47 logits 스텝을 재실행한다. 동일 기준을 event controller에 적용하고 cache 전체 원소도 비교한다.
5. 같은 적재에서 8건×3 epoch를 처리하고 이전 epoch step/release/cancel을 거부한다. 취소된 요청과 정상 응답·해제 모두 기록한다.
6. 실제 두 호스트의 stage route, 취소/재요청, UNLOAD/DELETE를 확인한다. 호스트/PID/장치/전송 trace를 남긴다.
7. 같은 agent binary에서 Python A→B 교체와 incompatible bundle 거부, 기존 llama.cpp 정상 응답/해제/UNLOAD/DELETE를 검사한다.
8. feature off의 생성/INSPECT와 llama.cpp 실기를 실행한다. 양쪽 전체 Rust 시험과 Python 시험, docs-lint 최종 종료를 기록한다.
9. clean commit source bundle을 원본과 별도 디렉터리에 복원해 --locked 빌드를 실행한다.

필수 cache/epoch 회귀: `python scripts/verification/qwen_cache/run.py` (4개).
독립 복사본 변이: `python scripts/verification/qwen_cache/run.py --mutations artifacts/integration/cache-epoch-mutations`.
전체 cache 원소·member/shape/dtype, live epoch 거부 시 효과 보존, drain 후 동일 ID 재수용과 이전 epoch 거부를 확인한다.
Cargo graph: `python scripts/verification/package_graph/run.py --output artifacts/integration/package-graph`.
실제 root metadata를 검사하고 별도 source의 protocol을 추가한 독립 Cargo fixture를 graph gate가 거부해야 한다.
2host 응답 단절: `scripts/verification/distributed_recovery/run.py`에 matrix와 같은 plan/nodes/deployments/bundle/agent-binary,
새 output을 준다. 응답 length prefix 뒤 TCP 연결을 끊고 결과 미승인→명시 abort/delete→같은 agent 재적재를 검사한다.
이전 LOAD 명령 거부 뒤 정상 short 응답·전체 cache 비교·UNLOAD/DELETE를 요구한다. host 자체 장애 복구와 구별한다.

## 로그와 판정

artifact마다 source/hash, model/plan/scenario, PID/host, stdout/stderr, 최초 오류/cleanup 오류,
logits/greedy/cache 비교, 요청별 응답·종료 이유·release chain과 종료 상태를 보존한다.
기준을 변경해 통과시키지 않는다. 제외/미실행/ignored/실패는 전체 GREEN과 구별하며 외부 접근 불가 항목은 BLOCKED로 남긴다.
브라우저 시험은 이 CLI 범위에 해당하지 않는다.
