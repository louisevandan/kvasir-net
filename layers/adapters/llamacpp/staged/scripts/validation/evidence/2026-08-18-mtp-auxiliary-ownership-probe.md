# MTP auxiliary ownership probe

> 문서 지위 (2026-09-06): **날짜·환경 한정 증거**. 본문 날짜/커밋/모델/토폴로지의 관측이다. 현재 구현이나 다른 분산 환경의 완료 증거가 아니다.
> 현재 목표·상태·순서는 [실행 로드맵](../../../../../../../docs/distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](../../../../../../../docs/document-map.md)를 따른다.

Validated 2026-08-18 on Windows against the pinned compat revision
`3e3a7a416d6588597523c792fc5874a543d59ed9`.

## Scope

This is the smallest safe staged MTP slice. The compatibility loader now treats
repeating tensors with block ids in `[n_layer, n_layer_all)` as MTP/NextN
auxiliary tensors when `load_mtp` is set. The normal tail stage ending at
`n_layer` owns those tensors. Interior stages do not. This enables ownership
and load verification, plus initialization of llama.cpp's second
`LLAMA_CONTEXT_TYPE_MTP` context and `common_speculative` driver. It does not
claim MTP proposal, accept, reject, or rollback execution.

The staged server exposes the diagnostic capability as
`mtp_auxiliary_ownership=1`, while execution remains explicitly disabled:

```text
mtp_parser=1;mtp_auxiliary_ownership=1;mtp_execution=0
speculative_parser=1;speculative_execution=0
```

## Real model

```text
model: S:\models\unsloth\Qwen3.5-0.8B-MTP-GGUF\Qwen3.5-0.8B-Q8_0.gguf
size: 833,592,736 bytes
sha256: C54F8B67069C70085B98440DE696B44DA8250250AC69A961B41133DEF876E262
metadata: qwen35.block_count=25, qwen35.nextn_predict_layers=1
runtime: n_layer=24, n_layer_all=25
stage: [0,24) (tail)
```

## Commands and results

Build the patched staged server and the dedicated probe:

```powershell
node apps/p4/layers/adapters/llamacpp/staged/scripts/build-stage-server.mjs `
  --build-dir .cache/staged-mtp-build --config Release
```

The build completed through llama, llama-common, staged runtime, server, and
the dedicated test target. The capability test passed with exit code 0.

The full-model probe was run with the runtime DLL directory on `PATH`:

```powershell
$env:PATH = (Resolve-Path '.cache/staged-mtp-build/bin/Release').Path + ';' +
  (Resolve-Path '.cache/staged-mtp-build/bin').Path + ';' + $env:PATH
$env:P4_STAGED_MTP_MODEL =
  'S:\models\unsloth\Qwen3.5-0.8B-MTP-GGUF\Qwen3.5-0.8B-Q8_0.gguf'
$env:P4_STAGED_MTP_LAYER_END = '24'
& '.cache/staged-mtp-build/Release/p4_staged_mtp_ownership_test.exe'
```

The test-only execution attempt adds:

```powershell
$env:P4_STAGED_MTP_EXECUTE = '1'
& '.cache/staged-mtp-build/Release/p4_staged_mtp_ownership_test.exe'
```

Observed result:

```text
PASS: MTP auxiliary ownership probe loaded and unloaded the tail stage
PROBE_EXIT=0
```

The probe executable SHA-256 was
`3053B0CD1B5BF9D82CD401C9F394A09300F28A4D2F12605E9E39CDBAF7A4D0BD`.
The loader log reported `n_layer=24`, `n_layer_all=25`, and loaded the
`blk.24.nextn.*` auxiliary tensors before constructing and releasing the
`[0,24)` stage. It then reported
`common_speculative_init_result: creating MTP draft context against the target`
and initialized the common `draft-mtp` driver. The test asserts that the
second context is non-null before unload.

## Gate decision

**PASS: auxiliary ownership/context-init slice.** A real MTP GGUF loaded and
unloaded successfully with the auxiliary block owned by the tail stage, and
the second MTP context/common driver initialized successfully.

**NOT ENABLED: MTP execution.** The staged HOP still does not invoke the MTP
proposal/verification loop. It has no hidden-embedding HOP transport,
proposal list, acceptance/rejection result, or atomic rollback state. Ordinary
and MTP generated-token/EOS equality was therefore not claimed. The exact
next blocker was exercised through the test-only `execute_mtp_hop` path. The
target prefill succeeded, but the first `common_speculative_process` call
failed during MTP embedding handoff with the pinned staged runtime:

```text
MTP_TEST prefill_decode
MTP_TEST prefill_process
ggml/src/ggml-backend.cpp:194: GGML_ASSERT(buffer) failed
```

The process exits with Windows status `-1073740791` (`0xC0000409`). This is
before `common_speculative_draft`, target verification, or
`common_speculative_accept`; therefore no token/EOS equality is claimed. The
failure indicates that the staged graph does not provide a valid backend
buffer for the NextN embedding consumed by `common_speculative_process`.
The opt-in path is isolated behind `P4_STAGED_MTP_EXECUTE=1`; the normal CTest
probe remains a load/context-init/unload test and passes. Fixing this requires
staged graph/terminal NextN embedding residency work, followed by the target
rollback loop. It does not justify turning `mtp_execution` on.

Files changed for this slice are staged compatibility/server/build-validation
files only. `upstream`, `protocol.md`, sampler, and checkpoint files were not
modified.
