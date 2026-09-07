# Staged local-protocol contract fixtures

> 문서 지위 (2026-09-06): **구성요소 안내**. 해당 경로의 API·구조 안내다. 과거 service 경로와 현재 event 경로는 실제 호출자로 구분한다.
> 현재 목표·상태·순서는 [실행 로드맵](../../../../../../../docs/distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](../../../../../../../docs/document-map.md)를 따른다.

This directory is deliberately independent of the Rust adapter and the C++
stage server. It is an executable, language-neutral contract harness for the
adapter/server boundary.

Run it from the repository root with:

```text
python apps/p4/layers/adapters/llamacpp/staged/server/tests/contract/contract_test.py
```

The test uses the wire values currently defined by the staged adapter:

- `LCP4`, revision `1`, little-endian header
- `HELLO` is the only operation allowed before the session is ready
- exactly one mutating operation is in flight
- `HOP_RESULT` completes the current `HOP`
- `CANCEL` applies only to the current hop
- `KV_*` and `UNLOAD` are legal only after the current hop has completed
- the startup plan is `u32le byte_length + UTF-8 bytes`; stdin remains open
  after the plan and EOF is the abnormal-parent liveness signal

The JSON fixture is intentionally simple so Rust and C++ tests can consume the
same scenarios without importing this Python implementation.
