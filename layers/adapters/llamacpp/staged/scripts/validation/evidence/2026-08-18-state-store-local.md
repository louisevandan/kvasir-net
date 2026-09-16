# Native StateStore local durability evidence

> Document status (2026-09-06): **Date- and environment-scoped evidence**. These are observations for the date, commit, model and topology stated in the body. They are not evidence that the current implementation or any other distributed environment is complete.
> Current goals, status and ordering follow the [execution roadmap](../../../../../../../docs/distributed-batching-roadmap.md); document authority and reading paths follow the [document map](../../../../../../../docs/document-map.md).

Date: 2026-08-18

Scope: Linux/WSL direct compile and execution of the staged protocol, runtime
state store, and its concurrent-writer regression test. This is not Windows
ABI, Windows power-loss, or cross-file transaction evidence.

Compiler:

```text
g++ (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0
```

Command:

```text
wsl.exe bash -lc "g++ -std=c++17 -O2 -pthread -I/mnt/f/dev/linkcpp_product/apps/p4/layers/adapters/llamacpp/staged/server/src/protocol -I/mnt/f/dev/linkcpp_product/apps/p4/layers/adapters/llamacpp/staged/server/src/runtime /mnt/f/dev/linkcpp_product/apps/p4/layers/adapters/llamacpp/staged/server/src/protocol/protocol.cpp /mnt/f/dev/linkcpp_product/apps/p4/layers/adapters/llamacpp/staged/server/src/runtime/state_store.cpp /mnt/f/dev/linkcpp_product/apps/p4/layers/adapters/llamacpp/staged/server/src/runtime/state_store_test.cpp -o /mnt/f/dev/linkcpp_product/target/p4-state-store-test && /mnt/f/dev/linkcpp_product/target/p4-state-store-test"
```

Result: exit code `0`; the test produced no stderr/stdout failure output.

The test covers checksum-valid save/load/drop, metadata and corruption
rejection, eight concurrent writers for one cache key, and absence of
`.tmp.` temporary files after publication.

Source hashes at execution:

```text
state_store.cpp      ea685d9ec50377fb0952438a2891f260b518267f6ffc45a2d20db7c37cda8888
state_store_test.cpp 9c45a100f5860c0e7a4e3b1e296719f17979e39bb1a110e5036b406ec3731922
```

This evidence does not establish actual Windows `FlushFileBuffers`/
`MoveFileExW` behavior, power-loss ordering, multi-process failure injection,
coordinator/receipt/manifest atomicity, native transaction parity, or durable
replay after restart.
