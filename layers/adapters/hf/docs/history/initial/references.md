> Historical record: requirements, status and measurements from the time of the standalone repository. The current layout and usage follow the [HF guide](../../../README.md). The complete original is in the Git bundle preserved during the migration.

# Reference projects and official sources

Checked on: 2026-09-13. External documentation keeps changing, so pin versions/commits at implementation time and check again.
These are the sources for the official documents actually read during the conversation and for the local code. The feature descriptions behind the links are not evidence of this project's implementation.

## P4 — read-only reference project

Location: `F:\dev\p4`.
Code reference HEAD: `d122125bafeaa6d32790761669f1bfa5868d8078`.
The working tree was dirty, so HEAD and working-copy documents are kept distinct.
During initialization, P4 HEAD moved to `484b856ee7e53aea5b850b654c45da53cb0724a6`.
The code evidence in this document is from the original baseline; observations of concurrent changes are owned by the initialization record.
Later P4 changes are not applied to this project automatically, and P4 files are not modified.

| Source | Path | What to consult |
| --- | --- | --- |
| Entry point | [README](../../../../../../README.md) | Document index and execution path distinctions |
| Current status | [roadmap](../../../../../../docs/distributed-batching-roadmap.md) | Latest status and real-hardware limits; not read as an instruction to resume P4 development |
| Verification conventions | [verification](../../../../../../docs/distributed-batching-verification.md) | Multi-computer runs, normal responses, quality/performance, failure evidence |
| Isolation contract | [layers](../../../../../../docs/layer-isolation-contract.md) | core/adapter/native/backend responsibilities |
| Document ownership | [document map](../../../../../../docs/document-map.md) | Distinguishing contracts from historical material |
| Events | [event protocol](../../../../../../docs/event-protocol-v2.md) | opaque envelope/payload, load/session, backpressure |
| Batching | [adapter batching](../../../../../../docs/adapter-batching-layers.md) | Reference for ledger/settlement/publication/state authority |
| Current trait | [node_adapter](../../../../adapter/src/node_adapter/mod.rs) | RetainedNodeAdapter and ownership return |
| event node | [event_node](../../../../../agent/src/event_node/mod.rs) | Actual retained consuming path |
| event broker | [event_broker](../../../../../agent/src/event_broker/mod.rs) | Neutral routing/delivery boundary |
| Node registration | [control.rs](../../../../../../entrypoints/agent/src/event_runtime/control.rs) | create currently supports only llamacpp |
| Assembly dependencies | [Cargo.toml](../../../../../../entrypoints/agent/Cargo.toml) | Where concrete adapters will be registered |
| Neutral crate | [adapter Cargo](../../../../adapter/Cargo.toml) | Boundary a separate bridge will reference |

The relative links work when the two folders are siblings under `F:\dev`. On another computer, align the paths and
obtain the corresponding P4 revision. No modifiable copy of P4 source or documents was vendored into this repository.

## Transformers/PyTorch ecosystem

| ID | Official source | Use/constraints confirmed in conversation |
| --- | --- | --- |
| R1 | [Continuous batching](https://huggingface.co/docs/transformers/main/en/continuous_batching) | generate_batch, manager, paged KV, chunked prefill, TP; not automatic integration with P4 PP |
| R2 | [Tensor parallelism](https://huggingface.co/docs/transformers/main/en/perf_infer_gpu_multi) | Per-model plan and communication on every layer; needs a fast interconnect |
| R3 | [Accelerate big model inference](https://huggingface.co/docs/accelerate/usage_guides/big_modeling) | device_map/offload; distinct from a multi-host PP executor |
| R4 | [Accelerate distributed inference](https://huggingface.co/docs/accelerate/usage_guides/distributed_inference) | Reference for experimental PyTorch-based PP |
| Q1 | [bitsandbytes](https://huggingface.co/docs/transformers/quantization/bitsandbytes) | Linear 4/8bit replacement, compute dtype, device/offloading constraints |
| Q2 | [HQQ](https://huggingface.co/docs/transformers/quantization/hqq) | Quantization without calibration data, per-module settings |
| Q3 | [GPTQ](https://huggingface.co/docs/transformers/quantization/gptq) | GPTQModel calibration, saving, compressed kernels, Marlin constraints |
| Q4 | [AWQ](https://huggingface.co/docs/transformers/quantization/awq) | Calibration-based candidate; recheck maintained packages/versions |
| Q5 | [Metal](https://huggingface.co/docs/transformers/quantization/metal) | MPS 2/4/8bit affine kernels, non-MPS dequantize |
| Q6 | [compressed-tensors](https://huggingface.co/docs/transformers/quantization/compressed_tensors) | Distinguishes storage format, restore at first forward and supported FP8 optimization modes |
| Q7 | [Fine-grained FP8](https://huggingface.co/docs/transformers/quantization/finegrained_fp8) | weight/activation FP8, device/kernel conditions |
| Q8 | [GGUF](https://huggingface.co/docs/transformers/main/quantization/gguf) | Limited compressed path and legacy restore; check per version/model |
| Q9 | [Quantization overview](https://huggingface.co/docs/transformers/quantization/overview) | For rechecking the device/bit/library selection table |

## Re-verification rules

For Qwen3.5-0.8B's pinned checkpoint, the actual Transformers source and the environment lock, see the [model document](../../models/qwen3_5_0_8b/README.md).

The main branch of the official docs may differ from the released package. Record the exact version and source commit at implementation time.
Check model card examples against the actually selected model revision, and pin remote model code the same way.
Library support tables are research evidence; evidence of per-model kernel execution, sustained compression and performance comes from real tests.
