# llama.cpp Adapter implementation summary

> Document status (2026-09-06): **History / old plan**. It preserves the plans and observations of the time. Do not use it for current status, execution order or promotion criteria.
> Current goals, status and ordering follow the [execution roadmap](docs/distributed-batching-roadmap.md); document authority and reading paths follow the [document map](docs/document-map.md).

This document summarizes the current state of P4's llama.cpp adapter and staged runtime as confirmed and implemented in this session. `MTP` is excluded from the current test scope.

## Structure

```text
OUTER
  -> p4-drive / p4-agent
    -> node
      -> llamacpp adapter
        -> staged C++ server
          -> patched llama.cpp runtime
```

Main paths:

| Path | Responsibility |
| --- | --- |
| [`layers/adapters/llamacpp/staged/adapter`](layers/adapters/llamacpp/staged/adapter) | Rust `Adapter` implementation, plan delivery, server lifecycle, local protocol client |
| [`layers/adapters/llamacpp/staged/server`](layers/adapters/llamacpp/staged/server) | C++ stage server, llama.cpp calls, hop input/output, KV state handling |
| [`layers/adapters/llamacpp/staged/compat`](layers/adapters/llamacpp/staged/compat) | Versioned compatibility patch set applied to official llama.cpp |
| [`tools/scripts/e2e/run-ssh-forwarded-real-four-node.ps1`](tools/scripts/e2e/run-ssh-forwarded-real-four-node.ps1) | Real 4-node verification runner that connects the central 3090 and 4080 with the remote 3090×2 |
| [`docs/protocol.md`](docs/protocol.md) | hop, sequence, option and capability contracts already defined by OUTER/P4 |
| [`docs/buildplan.md`](docs/buildplan.md) | staged implementation and verification gates |

## Implemented behavior

### Server lifecycle

- Each llama.cpp stage runs as one independent process.
- The Rust adapter creates and connects the stage server and owns the process handle.
- A normal unload cleans up the context, model and runtime after `UNLOAD`.
- To handle abnormal parent exit, the server's stdin is not closed even after the startup plan. stdin EOF is the liveness termination signal.
- The `S:` network drive on remote Windows is not visible to non-interactive SSH sessions, so the Agent runs as a hidden Scheduled Task under the remote interactive account.

### Distributed inference

- The model plan includes per-stage `layer_begin/layer_end`, KV range, batch, ubatch, context and GPU layer count.
- Each stage owns only its own layers and KV range.
- A hop follows the P4 protocol's multi-sequence/window contract, and the staged adapter forwards each sequence's cut-set payload between stage servers.
- Stage output is synchronized once and sent after both `output_get` and `terminal_get` are processed.
- An alias descriptor sends no separate payload and allows only a valid alias range.
- The adapter's Rust code has no backend FFI; the C++ server uses the llama.cpp headers and library directly.

### Decode batching (experimental, `P4_STAGED_DECODE_BATCH`)

Even when one hop carried several sequences, the stage server called `llama_decode`
separately for each sequence. As a result, the stage weights are read again once per sequence — on the 4-GPU measurement,
35,000 decode laps produced 35,500 graph executions, one sequence per hop.

The batched path turns one lap into one `llama_batch`. Its behavior was confirmed (width 2–3,
0 `llama_decode` failures, 0 ubatch splits), and it respects three constraints established in the process.

- A stage that consumes a cut-set uses an **embedding batch** (`llama_batch_init(n, n_embd, 1)`,
  with `embd` filled with zeros). With a token batch, the graph goes looking for a token embedding
  and the input shape does not match.
- **llama.cpp decides the position.** staged knows only the lap index, and the absolute position
  includes the prompt length. Passing lap 22 as is gets rejected as KV moving backwards.
- **`--kv-unified` is required.** Without it, a ubatch accepts only consecutively increasing sequence ids,
  so a lap with scattered slots gets fragmented.

When the shape cannot be handled, this path quietly declines and falls back to the existing per-sequence path.
It is still disabled by default because, when enabled, some sequences stall late in generation,
and that defect lies in the chain accounting, not in llama.cpp.

Batching, offloading and tensor placement all end at this layer. P4 treats the plan as opaque,
so these optimizations do not change the protocol.

### Model option passing

- The Rust adapter does not enumerate individual llama.cpp options.
- The adapter preserves the opaque option/plan delivered by OUTER/P4, and the C++ server interprets it through the llama.cpp `common` layer.
- Fine-grained offload, unified KV, sampling, reasoning budget and context/batch options are pass-through.
- Speculative decoding and MTP are separate capabilities that need staged semantic verification; MTP is currently excluded from testing.

## KV cache

- Each stage server owns its KV cache.
- KV save/restore checks the stage range, sequence, model identity, cache key and checksum.
- If the restore target and metadata do not match, nothing is restored.
- The current review found a defect: `runtime/build identity`, context parameters, KV format and token position must be included explicitly in the cache key. This needs to be reinforced before final operational approval.

## Real verification results

### What succeeded

- The central RTX 3090 and RTX 4080 were connected with the remote RTX 3090×2 through SSH forwarding.
- Model files were used from the shared `S:\models\...` path without copying them to local/remote disks.
- The remote Agent was started as a hidden Scheduled Task, and each remote port was confirmed open.
- The 4080 stage was limited to `n-gpu-layers=2` in the initial experiment.
- The runner supports artifact SHA-256 verification, VRAM sampling, stage range/GPU layer overrides, hidden processes and cleanup.

### MiniMax-M3 results

Model used:

```text
S:\models\unsloth\MiniMax-M3-GGUF\MiniMax-M3-UD-Q5_K_S-00001-of-00008.gguf
```

Stage placement attempted:

```text
ranges:   0:15,15:30,30:45,45:60
gpu:      4,2,4,4
4080 max: 10500 MiB
```

All 4 stages reached the model loader but stopped with the following error.

```text
key not found in model: minimax-m3.attention.indexer.head_count
```

This is not a distributed cut-set or remote connection failure. The early MiniMax-M3 GGUF on `S:` lacks the indexer metadata that the MSA path of the currently pinned llama.cpp requires. The official model documentation also describes that GGUF as a format based on experimental PR #24523.

The following results therefore do not exist yet:

- Real 4-node MiniMax-M3 token generation
- 5,000-token prefill
- 5,000-token generation
- prefill/generation TPS
- Parallel calibration and queue saturation

Result files:

- [`target/ssh-forwarded-four-node-e2e/m3-smoke-20260819-3/result.json`](../target/ssh-forwarded-four-node-e2e/m3-smoke-20260819-3/result.json)
- [`target/ssh-forwarded-four-node-e2e/m3-smoke-20260819-3/central-agent-52003.err.log`](../target/ssh-forwarded-four-node-e2e/m3-smoke-20260819-3/central-agent-52003.err.log)

## M3 compatibility boundary

The experimental patch in which the adapter inferred the missing MSA metadata of the early M3 GGUF has been removed. The latest official llama.cpp does not support this variant either, so if the local compatibility layer kept a per-model dense fallback, the adapter would need changes for every new model.

- Only official llama.cpp interprets GGUF and selects per-model memory/graph.
- The staged compatibility layer consumes only generic capabilities of the official memory object, not model names.
- A GGUF that official llama.cpp cannot read is rejected fail-closed; use an official-format GGUF or a new upstream revision.
- The preparation step fails if an architecture branch or a private model header is added to the compatibility patch or native runtime.

## Known issues and next gates

1. First confirm in the stock runtime that official llama.cpp can read the GGUF.
2. Confirm load and 1-token decode of the same GGUF on a single stage.
3. Confirm load and hop on the two central stages, 3090 and 4080.
4. Confirm 4-stage 1-token inference including the remote 3090×2.
5. Adjust stage range/GPU layers while keeping 4080 VRAM at 11GB or less.
6. Run the 5k/5k meaningful-prompt test, stating `batch-size=5000`, sufficient context and MTP excluded.
7. Create concurrent requests at the same parallel count, keep submitting further requests, and measure whether the node queue/adapter queue builds up.
8. Record prefill TPS, generation TPS, combined TPS and per-session mean TPS separately.
9. Update `buildplan.md` and the validation evidence only after real results pass.

## Cautions

- `apps/p4/layers/adapters/llamacpp/upstream` is the replaceable official llama.cpp boundary, so do not put Linker-specific code there.
- Keep compatibility changes in the versioned patch/prepared tree, but do not add per-model interpretation or branches.
- Do not interpret the M3 loading error as a layer placement error. Align the model format and runtime support level first.
- Run all experiments in hidden/background mode, and when stopping, clean up the central/remote Agents, SSH tunnels and VRAM sampler together.
