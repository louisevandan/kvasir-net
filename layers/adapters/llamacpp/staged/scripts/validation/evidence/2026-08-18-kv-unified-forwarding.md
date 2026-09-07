# Unified KV decode regression

> 문서 지위 (2026-09-06): **날짜·환경 한정 증거**. 본문 날짜/커밋/모델/토폴로지의 관측이다. 현재 구현이나 다른 분산 환경의 완료 증거가 아니다.
> 현재 목표·상태·순서는 [실행 로드맵](../../../../../../../docs/distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](../../../../../../../docs/document-map.md)를 따른다.

Observed 2026-08-18 KST with the cached CUDA staged server and a real Qwen2.5
1.5B GGUF. The upstream checkout was not modified.

## Diagnosis

`llama_decode()` status `-3` is the upstream mapping of
`GGML_STATUS_FAILED`. The failing boundary was decode immediately after
`KV_RESTORE`, not model loading or plan parsing. State import can enqueue
backend KV uploads; a CUDA backend may complete those copies asynchronously.
If the next hop computes before the uploads are visible, the failure appears
as `llama_decode(-3)`.

## Minimal staged fix

The staged runtime synchronizes after a successful state import:

```cpp
if (llama_state_seq_set_data_ext(ctx_, state.data(), state.size(), seq, request.flags)
        != state.size()) {
    // report restore failure
}
llama_synchronize(ctx_);
```

The implementation is in
`staged/server/src/runtime/llama_stage_runtime_kv.cpp`. No upstream source was
changed. The synchronization is deliberately after the import; synchronizing
only before the import does not cover the newly queued device copies.

## Real Qwen validation

Cached server and model:

- Server: `.cache/staged-server-cuda-real-20260818/Release/p4_staged_server.exe`
- Model: `S:\models\Qwen2.5-1.5B-Instruct-Q8_0.gguf`
- Plan extra args: `--kv-unified`
- Boundaries: `0,14,28` and `0,7,14,21,28`
- Test: `real_two_stage_kv_save_restore_drop_is_equivalent`

The test performed two- and four-stage loads, prefill, KV save, KV restore,
decode after restore through every stage, tail outcome generation,
byte/checksum equality, KV drop, and process cleanup. Representative output:

```text
REAL_MULTI_STAGE_KV RESTORE_DECODE stage=0 output_descriptors=1 outcome=false
REAL_MULTI_STAGE_KV RESTORE_DECODE stage=1 output_descriptors=2 outcome=true
REAL_MULTI_STAGE_KV RESTORE_EQUAL label=two-stage stages=2 bytes_and_checksums=identical
REAL_MULTI_STAGE_KV PASS label=four-stage stages=4 boundaries=[0,7,14,21,28]
REAL_MULTI_STAGE_KV PASS label=two-stage stages=2 boundaries=[0,14,28]
test result: 2 passed; 0 failed
```

The previous restore-to-decode `-3` was not reproduced on this cached CUDA
runtime. This validates the tested two-stage unified-KV path, not all model
architectures or speculative execution.

## Scope

This closes the observed unified-KV restore-to-decode defect for the tested
Qwen/CUDA staged path. MTP and speculative execution remain explicitly
unsupported in this lane: `mtp_execution=0`, `speculative_execution=0`.
