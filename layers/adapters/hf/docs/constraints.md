# 제한

- 한 bridge의 model command는 하나이며 physical batching·양자화·임의 모델 지원은 미구현이다.
- 기존 RTX4080+3090 BF16 분할의 logits FAIL을 유지한다. FP32 통과로 대체하지 않는다.
- epoch는 drain된 상태에서만 전환한다. 오래된 generation/epoch가 새 상태를 변경해서는 안 된다.
- host 전체 장애의 durable 복구·인증은 이 in-memory 계약의 범위 밖이다.
- 작은 Qwen conformance와 디렉터리 이관 검증은 초대형 H0–H7/성능·SLO 수용이 아니다.
- Python import명 `p4hfadapter`는 유지하지만 독립 저장소·외부 path dependency는 사용하지 않는다.
- 자동 계획은 현재 Qwen3.5-0.8B 고정 revision의 FP32, CPU/CUDA만 허용한다. BF16 실험 plan의 수동 실행은 별도다.
- stage 프로필은 실제 loader/cache 실행의 관측치다. synthetic 입력의 관측 최대값이 모든 입력의 절대 메모리 상한은 아니다.
  caller reserve와 LOAD 직전 점유 재검사가 필요하다. IPC/network·동시 GPU 실행 contention은 아직 비용에 포함하지 않는다.

세부 예산/실패 의미는 [통합 계약](integration/README.md), 미검증 양자화 후보는 [양자화 설계](quantization.md)를 따른다.
