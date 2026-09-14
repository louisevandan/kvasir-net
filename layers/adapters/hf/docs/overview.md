# 목적과 소유

P4의 `layers/adapters/hf/`가 Rust bridge와 모델별 Python을 함께 소유한다.
현재 구상 모델은 Qwen3.5-0.8B다. 모델별 부분 적재·forward·상태를 제조사 Transformers 구현에 맞춘다.
저장소·Cargo workspace·출하 source commit은 P4 하나다. 별도 HF checkout은 필요하지 않다.
현재 개발 순서는 [P4 로드맵](../../../../docs/distributed-batching-roadmap.md), 이관 상태는 [이관 기록](migration/README.md)을 따른다.
