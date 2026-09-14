# 역할별 폴더 소유권

사용자 확정 원칙: **역할이 다르면 파일 하나라도 별도 폴더로 분리합니다.**
모델 구분과 역할 구분을 함께 적용하며, 파일 이름만 달리해 같은 폴더에 섞지 않습니다.
한 역할 안의 여러 구현 파일은 함께 둘 수 있습니다. 언어·파일 수를 역할 경계의 기준으로 삼지 않습니다.

## 구현된 구조

| 폴더 | 단일 소유 역할 |
| --- | --- |
| `python/p4hfadapter/transport/framing/layout/` | wire header의 고정 byte 배치 |
| `python/p4hfadapter/transport/framing/limits/` | 명시적 frame 크기 상한 |
| `python/p4hfadapter/transport/framing/errors/` | 거절·전송 오류 분류 |
| `python/p4hfadapter/transport/framing/encoding/` | 송신 header 검증·직렬화 |
| `python/p4hfadapter/transport/framing/decoding/` | 수신 header 해석·검증 |
| `python/p4hfadapter/transport/framing/receiving/` | blocking frame 수신·수신 실패 이후 재사용 금지 |
| `python/p4hfadapter/transport/framing/sending/` | blocking frame 송신·불확실한 전송의 재시도 금지 |
| `tests/transport/framing/<role>/` | 해당 역할 검증; `process/`는 실제 프로세스 통신 시험 |
| `tests/fixtures/framing_peer/` | 전송 전용 subprocess fixture |
| `tests/fixtures/receiving_streams/` | 수신 오류 주입용 stream |
| `tests/fixtures/sending_streams/` | 송신 오류 주입용 stream |
| `scripts/testing/` | test suite 실행 진입점 |
| `scripts/verification/framing_mutation/` | 독립 복사본의 framing 변이 검증 |
| `scripts/verification/documents/` | 문서 인코딩·링크 검사 |
| `tests/plans/`, `tests/reports/framing/` | 각각 검증 의도와 실제 실행 증거 |

## 모델 구현 구조

Qwen의 실제 역할 목록은 [모델 계약](../models/qwen3_5_0_8b/README.md)의 구현 표에 있습니다.
프로세스 수명(`processes/`)과 노드 순회(`routing/`)도 서로 다른 폴더로 분리했습니다.

`python/p4hfadapter/models/<model>/`는 구상 모델의 소유 경계이며 아래 역할을 섞지 않습니다.

```text
models/<model>/
  configuration/   선택 모델의 실행 명세 검증
  loading/         담당 weight와 metadata 적재
  planning/        해당 모델의 실측 stage 후보 선택·자원 합산
  profiling/       실제 loader·cache를 새 프로세스에서 측정
  quantization/    해당 모델의 recipe 적용과 kernel 선택
  forward/         해당 모델의 부분 연산
  state/           해당 모델의 요청별 KV/recurrent 상태
  boundary/        해당 모델의 stage 입출력 표현
  reference/       제조사 실행을 보존한 비교 기준선
```

필요한 역할부터 실제 코드를 작성하며 빈 뼈대 폴더를 미리 만들지 않습니다.
새 역할은 새 폴더로 추가합니다. `common`/`utils`/`runtime`에 서로 다른 책임을 모으지 않습니다.
Rust bridge도 retained 전달·원장·IPC를 역할별 하위 폴더로 나눕니다.
역할별 폴더가 공통 모델 base class나 자동 plugin 구조를 요구하지는 않습니다.

`/models/`만 대용량 가중치 저장 위치로 Git에서 무시합니다. `python/p4hfadapter/models/`의
모델 전용 소스는 추적 대상입니다. 테스트·fixture·측정 결과를 production 폴더에 넣지 않습니다.
