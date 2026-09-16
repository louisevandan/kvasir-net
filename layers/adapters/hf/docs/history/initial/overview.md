> Historical record: requirements, status and measurements from the time of the standalone repository. The current layout and usage follow the [HF guide](../../../README.md). The complete original is in the Git bundle preserved during the migration.

# Purpose and scope

Status: user requirements and project scope. Implementation status is owned by the [roadmap](roadmap.md).

## Purpose

Using a specific model's vendor Python execution code and Hugging Face Transformers/PyTorch,
build a concrete P4 adapter that loads and runs parts of the model across multiple physical computers.
Model definitions and supported operations are reused; partial loading, partial forward, state and the communication boundary are implemented per model.
Writing a dedicated Python adapter for each model is the reason this project exists.
The loader, forward, cache/state, partitioning and quantization are implemented directly to fit the actual structure of the selected model.
A general-purpose processor or plugin framework that fits every model into the same interface is not a development goal.
Vendor code and verified operator libraries are reused; there is no need to rewrite every matrix-multiplication kernel.

The user's judgement is that P4 node distribution can be built as long as per-model Python code is supported,
and that the real design issue is quantized weights and how to execute them. This plan follows that premise.
The existence of a vendor Python example alone does not prove quantization, partitioning or kernel compatibility on every device.

## Confirmed scope

- Standalone repository: `F:\dev\p4hfadapter`.
- Reference repository: `F:\dev\p4`. Read its documents, current execution paths and neutral boundaries to obtain the necessary design grounds.
- Keep the P4 common specification and the upper-level node lifetime, and put model-specific semantics inside the new concrete adapter.
- Later, a separate integration task will make the final P4 executable include the new adapter bridge.
- Following the user's later instruction, independent development is done in this repository. P4 files are not modified or moved.

## Proposed first feature scope

- One exact model revision, a specified device/OS combination, and a manually verified contiguous layer split.
- 4-bit weight candidates, FP16/BF16 boundary tensors, and KV/recurrent state at the verified precision the model requires.
- Prefill, decode, EOS, output limit, cancellation, state return, node reuse.
- Proof in the order: independent reference run → partition correctness → real multiple computers → continuous waves.

4-bit is a proposal, not a value confirmed by the user. Depending on already-available quantized weights, quality and the actual kernels,
8-bit, BF16 or mixed precision may fit better. Open items follow the [decision record](decisions.md).

## Separate follow-up scope

Automatic support for every HF model, automatic model discovery and placement, cross-host TP/EP, KV save/restore, KV/communication quantization,
MTP/speculative decoding, multimodal, authenticated external services and durable recovery after restart are not automatically included in the first phase.
If the selected model's required operations are multimodal or involve special state, the model scope itself is specified first.

## What success means

Generating a token once in Python, or having several processes running, is not product completion.
With the exact model/quantization artifact, the work must prove normal responses, continuous requests, reclaim after cancellation/failure, and re-acceptance
across multiple physical computers, and must report useful generation TPS, TTFT, ITL and memory under pinned quality/SLO conditions.
Superiority over llama.cpp is not claimed until a real comparison under equal conditions exists.
