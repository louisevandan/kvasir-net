# Real multi-stage KV save/restore/drop evidence

Observed 2026-08-18 KST with the cached CUDA staged server and
`S:\models\Qwen2.5-1.5B-Instruct-Q8_0.gguf`. The test is limited to the
staged adapter integration test and this evidence file; it does not modify
`service`, `mock`, or `upstream`.

## Test

```powershell
$env:P4_STAGED_LLAMA_SERVER_BINARY = (Resolve-Path '.cache/staged-server-cuda-real-20260818/Release/p4_staged_server.exe').Path
$env:P4_STAGED_LLAMA_MODEL = 'S:\models\Qwen2.5-1.5B-Instruct-Q8_0.gguf'
$env:P4_STAGED_LLAMA_CUDA_VISIBLE_DEVICES_STAGE0 = '0'
$env:P4_STAGED_LLAMA_CUDA_VISIBLE_DEVICES_STAGE1 = '1'

cargo test --manifest-path apps/p4/layers/adapters/llamacpp/staged/adapter/Cargo.toml `
  --test multi_stage_kv_e2e real_two_stage_kv_save_restore_drop_is_equivalent `
  -- --ignored --nocapture
```

The four-stage run uses the same command and test file with stage masks
`0,1,0,1` and the `real_four_stage_kv_save_restore_drop_is_equivalent` test.

## Results

Both real-model tests passed:

```text
two-stage: 1 passed, finished in 19.76s
four-stage: 1 passed, finished in 21.65s
```

Two-stage output:

```text
stage 0 [0,14): state_bytes=201516 file_bytes=201666
  checksum=ac93ce034f8980063d84166e3feb81161125395a99ca91c0e3724d39b54bc793
stage 1 [14,28): state_bytes=201516 file_bytes=201666
  checksum=6dae83fa42508da3a4ceb3910c4d60952641acf4d0860f997114f0531eb54d47
RESTORE_EQUAL stages=2 bytes_and_checksums=identical
DROPPED stage 0 file_exists=false
DROPPED stage 1 file_exists=false
```

Four-stage output:

```text
stage 0 [0,7):   state_bytes=201516 file_bytes=201668 checksum=f1c7b288bd3fd01674f8a09034be2afeb0219b5c688312ff9074259284598853
stage 1 [7,14):  state_bytes=201516 file_bytes=201668 checksum=c809583b80c29521c7f05e9ee4293981c1ae3a284a1da17b180b9c400d4a9c14
stage 2 [14,21): state_bytes=201516 file_bytes=201668 checksum=be87254987f90e6ae3ac20cd9e2a7b62c4e933649e5e5b70870e26bd1321e33a
stage 3 [21,28): state_bytes=201516 file_bytes=201668 checksum=906545ae17d461ae781e40b26e10d80a482b154ec8f892be9102d7c63c5af778
RESTORE_EQUAL stages=4 bytes_and_checksums=identical
DROPPED stage 0..3 file_exists=false
```

The test performs real prefill through every stage, saves each stage's llama
sequence state to its own SSD directory, restores every state, re-exports it,
and compares the resulting `KvResult` byte count and SHA-256 checksum with the
original. It then drops every file and asserts that no `.lkv` file remains.

## Boundary of the evidence

This proves multi-stage state persistence and byte-level restore equivalence.
It does not yet prove that a subsequent `HOP` decode can run after restore:
an exploratory version attempted that operation and the current runtime bridge
returned `llama_decode` status `-3`. That is a separate staged runtime defect;
the adapter-only test records the stronger state round-trip result without
claiming decode-after-restore success.
