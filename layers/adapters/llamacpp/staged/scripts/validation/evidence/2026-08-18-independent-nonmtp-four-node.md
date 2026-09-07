# Independent non-MTP four-node regression

> 문서 지위 (2026-09-06): **날짜·환경 한정 증거**. 본문 날짜/커밋/모델/토폴로지의 관측이다. 현재 구현이나 다른 분산 환경의 완료 증거가 아니다.
> 현재 목표·상태·순서는 [실행 로드맵](../../../../../../../docs/distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](../../../../../../../docs/document-map.md)를 따른다.

## Scope

This run intentionally excludes MTP and speculative decoding. It validates the
ordinary staged path with an already-built CUDA artifact copied to the test
nodes; this host does not rebuild the C++ runtime for the run.

## Topology and result

- model: `S:\models\Qwen2.5-1.5B-Instruct-Q8_0.gguf`
- central host: RTX 3090 + RTX 4080
- remote host: RTX 3090 x2 over SSH forwarding
- workload: four concurrent requests, one token each
- artifact: `.cache/staged-server-cuda-real-20260818\Release`
- result: 4/4 completed, 0 failed, all four stages READY, UNLOAD completed
- artifact SHA-256 was checked across the copied stage bundles
- peak node queue: 3; peak adapter queue: 1

The run proves ordinary non-MTP four-node Load/HOP/Decode/Unload and stream
completion with the copied artifact. It does not prove the 23 GiB/11 GiB VRAM
boundary, logits equality, long-run stability, or MTP/speculative execution.
