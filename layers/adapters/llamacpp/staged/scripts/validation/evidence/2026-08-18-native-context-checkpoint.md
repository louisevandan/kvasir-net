# Native context checkpoint staged slice

> Document status (2026-09-06): **Date- and environment-scoped evidence**. These are observations for the date, commit, model and topology stated in the body. They are not evidence that the current implementation or any other distributed environment is complete.
> Current goals, status and ordering follow the [execution roadmap](../../../../../../../docs/distributed-batching-roadmap.md); document authority and reading paths follow the [document map](../../../../../../../docs/document-map.md).

Audited and implemented 2026-08-18 against pinned compat revision
`3e3a7a416d6588597523c792fc5874a543d59ed9`. The upstream checkout was not
modified, and sampler/MTP files were not changed.

## API finding

The pinned `common_prompt_checkpoint` in `common/common.h` is an in-memory
container for target and optional draft checkpoint bytes. Its target methods
call:

```text
llama_state_seq_get_size_ext
llama_state_seq_get_data_ext
llama_state_seq_set_data_ext
```

It does not provide a separate checkpoint storage engine. The staged KV SSD
path already serializes the same native sequence-state blob and restores it
with the same `llama_state_seq_*_ext` API. The SSD layer adds identity,
checksum, atomic publish, and stage metadata.

The upstream automatic context-checkpoint settings (`n_ctx_checkpoints` and
`checkpoint_min_step`) are scheduler policy. They do not expose a public
checkpoint handle that staged can send across its existing protocol.

## Safe implementation slice

`StageRuntime` now exposes two in-memory methods:

```text
save_checkpoint(sequence_id, common_prompt_checkpoint*)
restore_checkpoint(sequence_id, const common_prompt_checkpoint&)
```

They reuse `common_prompt_checkpoint` for one target sequence, record token
position metadata, import/export the native sequence state, and synchronize
after restore so asynchronous device KV copies are complete before decode.

This is deliberately not advertised as a new wire capability. It does not
include sampler state, a draft context, speculative state, or multi-stage
atomicity. Those remain outside this slice.

The implementation is in:

```text
apps/p4/layers/adapters/llamacpp/staged/server/src/runtime/llama_stage_runtime.hpp
apps/p4/layers/adapters/llamacpp/staged/server/src/runtime/llama_stage_runtime.cpp
```

The C++ runtime regression test now covers native checkpoint save, decode,
restore, decode, and restored sequence position:

```text
apps/p4/layers/adapters/llamacpp/staged/server/src/runtime/llama_stage_runtime_compile_test.cpp
```

## Validation status

The targeted CMake build was attempted against the existing
`.cache/staged-server-plan-llama` cache, but this environment has no `cmake`,
Visual Studio `msbuild`, or `ninja` executable on PATH. Therefore the newly
added checkpoint test has not been compiled or run in this turn. No existing
binary was used to claim coverage of the new methods.

The pre-existing real Qwen KV save/restore/decode test remains separate: it
proves the SSD persistence path, not the newly added in-memory wrapper.

## Boundary

This slice establishes a reusable native target-context checkpoint primitive.
It does not make MTP/speculative execution possible and does not add accepted
token lists or rollback to HOP. A later implementation must add checkpoint
identity and atomic state handling for every participating context before
advertising speculative execution.
