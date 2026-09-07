# P1 KV identity manifest validation

> 문서 지위 (2026-09-06): **날짜·환경 한정 증거**. 본문 날짜/커밋/모델/토폴로지의 관측이다. 현재 구현이나 다른 분산 환경의 완료 증거가 아니다.
> 현재 목표·상태·순서는 [실행 로드맵](../../../../../../../docs/distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](../../../../../../../docs/document-map.md)를 따른다.

Scope: ordinary staged KV save/restore only. Speculative execution is outside
this validation.

## Implemented

The existing `KvPayload` wire encoding is unchanged. The C++ runtime fills
process-local manifest fields before calling `StateStore`:

- llama.cpp system/build identity;
- staged runtime manifest version;
- context shape and scheduling parameters;
- KV K/V types, unified-KV mode, and state flags;
- token position at save time.

The `.lkv` internal format is version 2. Restore rejects a missing or
mismatched manifest and verifies that the imported sequence position equals the
manifest position.

## Evidence

```text
cargo test -p p4-llamacpp-staged-adapter
30 passed, 0 failed
```

```text
cmake --build .cache/staged-server-llama --config Release \
  --target p4_staged_state_store_test
p4_staged_state_store_test.exe
exit code 0
```

The state-store test covers model/context manifest mismatch, token-position
mismatch, checksum corruption, concurrent writes, drop, and round-trip load.

```text
cmake --build .cache/staged-server-llama --config Release \
  --target p4_staged_llama_runtime_compile_test
exit code 0
```

The runtime target compiled with the staged llama.cpp artifact. Its executable
reported `SKIP` because `P4_STAGED_LLAMA_MODEL` was not set; no new real-model
KV execution claim is made here.

```text
cmake --build .cache/staged-server-llama --config Release \
  --target p4_staged_protocol_test
p4_staged_protocol_test.exe
exit code 0
```

The protocol test verifies that adding process-local manifest fields does not
change the existing encoded `KvPayload` bytes.

The build identity includes the exact prepared llama.cpp commit
`3e3a7a416d6588597523c792fc5874a543d59ed9` plus `llama_print_system_info()`.
After this revision identity was made explicit, the CUDA artifact was rebuilt
and copied without a remote build. The central RTX 3090+4080 and remote RTX
3090 x2 four-node run passed with 4 requests and 8 tokens each:

```text
PASS: SSH-forwarded four-node staged E2E
evidence: target/ssh-forwarded-four-node-e2e/20260818-kv-manifest-revision
```
