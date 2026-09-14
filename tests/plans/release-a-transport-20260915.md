# Release A 연결 회수 검증 계획

생성일 2026-09-15 KST. 기준 P4 `530985320`, native/모델 변경 없음.
[로드맵](../../docs/distributed-batching-roadmap.md#current-status)의 A0 잔여 제어 연결 포화를 다룬다.

## 목표·환경·전제

Windows PowerShell, Rust/Cargo, 실제 로컬 TCP와 bounded mailbox. 원격 설치 앱은 유지한다.
정상 TCP half-close의 지연 응답을 보존하면서 명시적으로 끝낸 연결의 슬롯을 반환한다.
EOF는 상대가 출력을 더 받지 않는다는 증명이 아니다. 명시 FINISH도 native/request/KV 정산이 아니다.
변경 계약은 [event transport](../../docs/event-protocol-v2.md#connection-finish-candidate)를 따른다.

## 절차·기대값

1. 실제 Runtime→INSPECT→응답→종료→다음 연결을 연결 한도보다 많이 반복한다. 기준 실패를 보존한다.
2. 대기 출력 원본·비용을 유지한 채 ACK 전에 모두 전송하고 슬롯을 반환한다. half-close 지연 응답을 유지한다.
3. 이전 소켓 FINISH가 새 소켓 binding·실패 tombstone을 삭제하지 않는지 검사한다.
4. 늦은 출력과 부분 write 실패는 원본·저장 비용·실패 소유자를 보존하며 자동 재전송/정산하지 않는다.
5. Rust/Python OUTER 소비자는 종료 ACK·분할 수신·EOF·예상 밖 출력·timeout을 검증한다.
6. 독립 복사본 제거 변이, 전체 workspace, 양쪽 adapter 실제 생성·회수, 현재 호출자 및 fleet를 검증한다.

## 기록·중단

원자료 `target/release-a-transport-20260915/`: 명령별 log/exit, source-manifest.json.
의도된 기준 반례·제거 변이의 실패는 검출 증거다. 구현 검증의 실패는 컴파일·시험 작성 오류도 포함해
같은 단계에서 누적3회이면 중단한다. 중간 PASS로 횟수를 초기화하지 않는다.
사용자의 중단 조건이 기존 최대3라운드보다 엄격하게 적용된다. 중단 뒤 추가 수정·실행·배포는 하지 않고
WIP 소스·실패·미실행을 기록한다. UI 없음.
