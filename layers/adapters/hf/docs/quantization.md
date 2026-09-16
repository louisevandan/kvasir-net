# Quantization design

Status: designs and candidates discussed in conversation. No quantization combination has been verified on a real model and device yet.
The official documentation sources are Q1–Q9 in [references](history/initial/references.md).

## Three axes to keep separate

| Target | What it reduces | First proposal |
| --- | --- | --- |
| Weights | Resident model memory and weight-read bandwidth | Verify 4-bit candidates per device |
| KV/recurrent state | State memory for long contexts and concurrent requests | The model's verified native precision; FP16/BF16 candidates for KV |
| Boundary activations | Inter-node transfer volume | Keep FP16/BF16 |

4-bit weights do not automatically make inputs, outputs or KV 4-bit.
W4A16 means 4-bit weights and 16-bit activations; it is not the same compute path as FP8 W8A8.
Some norm/recurrent accumulations may require FP32, so the dtype of the whole model is not forcibly unified.

```text
X(FP16/BF16) + QW(4bit) + scale/zero-point/packing metadata
  -> supported quantized matmul kernel
  -> Y(FP16/BF16)
```

Restoring compressed weights inside the kernel for computation is the normal path.
Keep it distinct from a fallback that unpacks the whole model to BF16 and holds it in global memory.

## Three ways to prepare weights

| Path | Procedure | Decision basis |
| --- | --- | --- |
| Public pre-quantized | Check the manufacturer's/publisher's checkpoint, config and revision, and load it directly | Per-model quality evidence, exact format and kernel compatibility |
| Quantize during load | Convert the original tensors into supported layers such as bitsandbytes/HQQ/Metal | Fast model support, whether calibration data is needed, load peak |
| Offline PTQ | Run GPTQ/AWQ-style calibration on representative inputs and save the quantized result | Repeated deployment, quality control, preprocessing resources and time |

Candidates are bitsandbytes' standard Linear replacement, HQQ (no calibration data needed) and calibration-based GPTQ via GPTQModel.
AWQ is also an algorithm candidate, but old AutoAWQ/AutoGPTQ install examples are not used as is.
Confirm and pin the exact model architecture, library versions and maintained backend.
Look for existing trustworthy quantized artifacts first; do not automatically choose to re-quantize yourself.
NF4 inference does not require LoRA training. Keep applying quantization itself distinct from QLoRA fine-tuning.

## Preparation flow for P4

1. Pin the original model ID/revision, weight hash, tokenizer/chat template and the manufacturer's model code.
2. Decide, per model, the quantization targets and excluded modules, bit/group size, symmetry, scale dtype and kernel layout.
3. If calibration is needed, pin a representative workload and keep it separate from the evaluation data.
4. After verifying the quantized result, save it with the layers' original names and all auxiliary tensors preserved.
5. Bind each stage's assigned layers, shared modules and quantized artifacts in the manifest, and deploy.
6. Each node reads only its assigned tensors, warms up with the real kernel, then reports peak/resident memory.
7. If the node placement changes but the quantization recipe is the same, avoid re-quantizing. If needed, only re-bundle the physical files.

The checkpoint's file shards do not match stage boundaries. A loader must be implemented that selects the required tensor set, including weights, scales, zero points, group index
and packing info. Do not load the full model on every node first and then delete the rest.

Preparing quantization first does not mean the full BF16 model must fit on a single GPU.
Check per tool whether layer-sequential processing and CPU offloading are possible, and the size of calibration activations/workspace.
Do not assume disk streaming or distributed calibration of very large models is provided automatically.
GPTQ/AWQ calibration depends on the outputs of earlier layers, so different nodes must not calibrate independently with arbitrary inputs.

## Per-model Python support

For a standard `nn.Linear`, first check the path that replaces it with a quantized layer and keeps the external input/output shapes.
MoE fused expert tensors, direct `matmul`, custom ops and packed projections may not be covered by Linear replacement alone.
Report the list of modules actually converted, the compressed bytes and the excluded modules. Do not label a model as fully 4-bit when only part of it was quantized.
embedding/lm_head tied weights, norms, routers and sensitive recurrent/state operations have per-model exclusion/sharing rules.
If steps in the quantization process, such as scale folding, change adjacent modules, bind those changed modules into the artifact as well.

## Heterogeneous nodes

With layer-wise PP, stages can connect through an agreed boundary tensor format even if their internal weight formats differ.
Example: passing BF16 tensors between GPTQ 4-bit on an NVIDIA stage and Metal 4-bit on a Mac stage.
This is a design possibility, not a verified result for that model.

| Device | Candidates to investigate | What not to assume up front |
| --- | --- | --- |
| NVIDIA CUDA | bitsandbytes 4bit, GPTQModel and supported CUDA/Marlin kernels | Same speed across all bit/group/symmetry settings and GPU generations |
| Apple MPS | Transformers MetalConfig 2/4/8bit and Metal kernels | That the same file also runs compressed on CUDA |
| AMD ROCm | Investigate the combinations actually supported by that version of GPTQModel/HQQ/torchao etc. | That CUDA kernels run as is |
| FP8-capable devices | FineGrainedFP8 or a supported compressed-tensors FP8 path | That an FP8 file alone accelerates on older GPUs |
| CPU | Verify CPU compressed ops/offloading separately | That GPU quantization settings cut CPU memory by the same ratio |

Build per-device artifacts from the same original revision and evaluate quality over the full combination.
Different quantization recipes are not the same as a simple format repack. Different bits/groups/calibration make a new numerical model.
Generate from the original weights where possible; restoring an already-lossy 4-bit and re-quantizing it to another 4-bit accumulates loss.
Weight conversion and stage moves are not approved automatically, and stages spanned by tied/shared state get special handling.

## Confirming actual compressed execution

- Measure memory not only right after load but also after the first forward and after long-context/maximum batches.
- Record the module implementation, kernel, dtype and packing actually selected, and any fallback reason.
- compressed-tensors has a storage format distinct from its execution mode. The default path in the current official docs restores at the first forward,
  and the optimization options are also limited to supported schemes/devices. Do not approve execution memory from stored bytes.
- For GGUF too, do not read read support as a guarantee of compressed execution. Check the limited GGUF kernel path in the current official docs and
  legacy dequantization per model, device and version. GGUF reuse is not a requirement for the first implementation.
- A performance-oriented LOAD is explicitly rejected if it fully dequantizes instead of using the compressed kernel the manifest requires.
  A diagnostic dense fallback is reported as a separate arm with its actual dtype.

## Capacity, performance and quality

The theoretical floor for pure weights is parameter_count × bits / 8 bytes.
For example, the pure 4-bit payload of a 100B model is 50 GB (decimal), plus scales, non-quantized modules, KV and workspace.
The active parameter count of an MoE does not substitute for the resident capacity of all weights.
The per-stage budget is weights + metadata + KV/state + activations + kernel workspace + IPC/transfer buffers + runtime headroom.
When stages share unified RAM or the same pool, do not count available capacity twice.

Capacity savings do not guarantee a TPS improvement. Measure conversion, kernel and communication costs per prefill/decode, batch and context.
Keep the quality baseline (original high precision) separate from the quantization baseline (non-distributed run of the same quantization).
Evaluate the total error of mixed-device recipes and normal-response quality versus the original separately.
Pin concrete tolerances, quality drop and SLO figures in the first model manifest before testing.
