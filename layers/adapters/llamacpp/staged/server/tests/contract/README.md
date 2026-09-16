# Staged local-protocol contract fixtures

> Document status (2026-09-06): **Component guide**. This is an API and structure guide for this path. The legacy service path and the current event path are distinguished by their actual callers.
> Current goals, status and ordering follow the [execution roadmap](../../../../../../../docs/distributed-batching-roadmap.md); document authority and reading paths follow the [document map](../../../../../../../docs/document-map.md).

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
