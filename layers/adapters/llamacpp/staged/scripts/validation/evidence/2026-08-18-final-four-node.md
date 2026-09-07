# Final four-node remote regression

> 문서 지위 (2026-09-06): **날짜·환경 한정 증거**. 본문 날짜/커밋/모델/토폴로지의 관측이다. 현재 구현이나 다른 분산 환경의 완료 증거가 아니다.
> 현재 목표·상태·순서는 [실행 로드맵](../../../../../../../docs/distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](../../../../../../../docs/document-map.md)를 따른다.

Date: 2026-08-18

The latest rebuilt Release artifacts were used:

- central RTX 3090 + RTX 4080
- remote RTX 3090 x2 over SSH forwarding
- Qwen2.5-1.5B-Instruct-Q8_0.gguf
- 4 requests, 8 tokens per request

Result:

`PASS: SSH-forwarded four-node staged E2E`

The driver completed discovery, node creation, distributed load, staged HOP,
ordered terminal responses, and unload/cleanup for all four requests.

Result directory:

`target/ssh-forwarded-four-node-e2e/20260818-final-four-node`
