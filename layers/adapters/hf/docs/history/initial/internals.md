> Historical record: requirements, status and measurements from the time of the standalone repository. The current layout and usage follow the [HF guide](../../../README.md). The complete original is in the Git bundle preserved during the migration.

# Internal implementation plan

Status: model and bridge implementation plan. The implemented IPC folders and detailed per-role ownership follow the [folder rules](../../structure/README.md).
Qwen's actual folders are listed in the [model contract](../../models/qwen3_5_0_8b/README.md). The bridge in the table below is planned.

| Planned location | Role |
| --- | --- |
| `crates/p4-hf-adapter/src/<role>/` | Per-role bridge folders such as retained delivery, ledger and IPC |
| `python/p4hfadapter/models/<model>/<role>/` | Folders per configuration/loading/forward/state/quantization/boundary/reference |
| `python/p4hfadapter/workers/<model>/<role>/` | Folders per worker role such as lifecycle/queue/completion |
| `tests/` | Model parity, bridge consumption paths, errors/mutations, multi-host verification |
| `manifests/` | Specifications and schemas of verified execution combinations |

`<model>` is the implementation identifier of the selected model and does not promise support for the whole model family.
Code needed for the first model is written directly in the relevant role folder. Do not first build a general model base class, a registry,
automatic layer discovery/partitioning, a common cache abstraction or a general quantization backend.
Later models can also be added as separate Python implementations. Only when commonality is confirmed across two or more real implementations
is the necessary code extracted, and per-model operation semantics are never changed for the sake of code reuse.

## First per-model audit

Following the vendor's actual `forward` and generation setup, mark the owned operations in the order input token/embedding → position/RoPE/mask →
layers → final norm/lm_head → logits → sampling.
Do not partition by calling the full `generate()` on each node.

- Separate global layer indices from stage-local indices, and check index access in cache objects.
- Check whether `past_key_values` assumes the full layer count. Inserting placeholder arrays alone is not considered safe.
- Look for tied embedding/lm_head, shared KV, sliding window, recurrent cache and MoE routing/experts.
- Separate position, valid length and attention mask per request, and do not count padding as real tokens.
- Confirm cache mutation, device stream completion and reuse safety after cancellation in the actual call order.

## Loading and quantization

Review meta initialization, selective tensor reads and similar techniques to avoid allocating weights/state for unassigned layers.
Keep standard Linear replacement separate from custom module conversion, and reject LOAD if quantization auxiliary tensors are missing.
Whether a library's full-model loader can be used as a partial-model loader is confirmed with real code.
Do not hide partial-execution errors by dequantizing quantized nn.Modules or replacing them with a plain Linear.

## Batching and physical state

The worker consumes the immutable request/row membership approved by the adapter scheduler.
Initially, correctness is pinned with simple, bounded step execution; continuous admission and chunked prefill are connected afterwards.
This simple phase is not performance or product completion. When reusing the scheduler/cache of `generate_batch`,
align who decides admission, membership and cancellation with the single contract in the [architecture](architecture.md).

The default worker is a proposal to start with a single physical state owner and serial execution.
Parallel streams/samplers and multiple stages on the same GPU are enabled only after safety, resource budget and actual benefit are confirmed.
One node per card is not made a general constraint.

## IPC and errors

The [blocking binary framing](../../transport/framing/README.md) for local IPC was implemented first.
The current measurement environment is Windows; the worker execution model, deadlines and termination control are not decided yet.
Define the limits for the request queue, completion queue, serialized bytes and GPU staging buffers together.
Do not mix Python stdout logs with command/result frames.
On a worker crash, IPC timeout, partial tensor receive or result delivery failure, keep the ledger/buffer owner on record.
Do not judge that all requests went unexecuted or were released normally merely because the worker died.

## Observation

Record load peak/resident, actual kernels/fallbacks, per-stage queue/transfer/compute/settle time,
actual token length, KV/state bytes, retained bytes and per-request terminal/release.
When measuring GPU execution time, distinguish asynchronous enqueue time from device completion time.
If measurement adds an unnecessary global synchronize at every step, state that the run is a measurement arm.
