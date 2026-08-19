# Independent non-MTP four-node regression

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
