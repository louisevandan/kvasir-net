# MiniMax-M2.7 multi-shard partial-stage validation

Date: 2026-08-18

This is a bounded real-model lane, separate from the MTP implementation lane.
It uses the existing CUDA staged-server artifact and does not rebuild C++.

## Model and policy

Model directory:

```text
S:\models\unsloth\MiniMax-M2.7-GGUF
```

The model is `minimax-m2`, 62 layers, 256 experts, and five GGUF shards. The
five model shards total 148.1118 GiB. `mmproj-F32.gguf` was excluded from the
planner input because this lane tests the text model.

Physical GPU inventory at test time:

```text
0, NVIDIA GeForce RTX 3090, 0 MiB used, 24576 MiB total
1, NVIDIA GeForce RTX 4080, 539 MiB used, 16376 MiB total
```

The staged plan used only `[0,1)`. The planner reports 2.37 GiB for that
transformer layer and 1.2163 GiB of boundary tensors when the boundary is
assigned to the tail. This is within both requested 23 GiB (3090) and 11 GiB
(4080) budgets; the full model is not within those budgets.

## Planner command and result

```powershell
node apps/p4/layers/adapters/llamacpp/staged/scripts/validation/layer-window-memory-report.mjs `
  --model S:\models\unsloth\MiniMax-M2.7-GGUF\MiniMax-M2.7-UD-Q5_K_S-00001-of-00005.gguf `
  --shard S:\models\unsloth\MiniMax-M2.7-GGUF\MiniMax-M2.7-UD-Q5_K_S-00002-of-00005.gguf `
  --shard S:\models\unsloth\MiniMax-M2.7-GGUF\MiniMax-M2.7-UD-Q5_K_S-00003-of-00005.gguf `
  --shard S:\models\unsloth\MiniMax-M2.7-GGUF\MiniMax-M2.7-UD-Q5_K_S-00004-of-00005.gguf `
  --shard S:\models\unsloth\MiniMax-M2.7-GGUF\MiniMax-M2.7-UD-Q5_K_S-00005-of-00005.gguf `
  --budget 3090=23 --budget 4080=11 --boundary-owner last
```

Result:

```text
status=ok
architecture=minimax-m2
layers=62
experts=256
shards=5
total_gib=148.1118
boundary_gib=1.2163
window_status=does_not_fit_tensor_bytes
best_two-way_split=after layer 43
3090 [0,43) required=101.7755 GiB fits=False
4080 [43,62) required=46.3363 GiB fits=False
```

The planner also reports layer 0 as 2.37 GiB, making `[0,1)` a feasible
bounded partial-stage candidate.

## Real CPU partial-stage load

Artifact:

```text
F:\dev\linkcpp_product\.cache\staged-server-cuda-real-20260818\Release\p4_staged_server.exe
```

Command:

```powershell
node apps/p4/layers/adapters/llamacpp/staged/scripts/validation/smoke-stage-server.mjs `
  --model S:\models\unsloth\MiniMax-M2.7-GGUF\MiniMax-M2.7-UD-Q5_K_S-00001-of-00005.gguf `
  --executable .cache\staged-server-cuda-real-20260818\Release\p4_staged_server.exe `
  --layer-begin 0 --layer-end 1 --n-gpu-layers 0 --timeout-ms 180000
```

Result: `status=passed`, `HELLO operation=1`, `UNLOAD operation=9`, process
exit `code=0`, elapsed `1309 ms`. The loader reported 50 metadata entries and
809 tensors; the CPU-mapped buffer was 3046.43 MiB. The runtime reported
`linkcpp graph stage: layers=[0,1) inputs=0 outputs=1` and `READY`.

## Real CUDA partial-stage loads

Both runs used `--n-gpu-layers 999 --device CUDA0`. `CUDA_VISIBLE_DEVICES=0`
was the physical RTX 3090 and `CUDA_VISIBLE_DEVICES=1` was the physical RTX
4080; `CUDA0` is therefore the per-process visible device in each run.

3090 command:

```powershell
$env:CUDA_VISIBLE_DEVICES='0'
node apps/p4/layers/adapters/llamacpp/staged/scripts/validation/smoke-stage-server.mjs `
  --model S:\models\unsloth\MiniMax-M2.7-GGUF\MiniMax-M2.7-UD-Q5_K_S-00001-of-00005.gguf `
  --executable .cache\staged-server-cuda-real-20260818\Release\p4_staged_server.exe `
  --layer-begin 0 --layer-end 1 --n-gpu-layers 999 --device CUDA0 --timeout-ms 180000
```

Result: `status=passed`, `HELLO operation=1`, `UNLOAD operation=9`, process
exit `code=0`, elapsed `26589 ms`. CPU-mapped buffer was 622.76 MiB and the
runtime reported `[0,1)` graph-stage lines and `READY`.

4080 command:

```powershell
$env:CUDA_VISIBLE_DEVICES='1'
node apps/p4/layers/adapters/llamacpp/staged/scripts/validation/smoke-stage-server.mjs `
  --model S:\models\unsloth\MiniMax-M2.7-GGUF\MiniMax-M2.7-UD-Q5_K_S-00001-of-00005.gguf `
  --executable .cache\staged-server-cuda-real-20260818\Release\p4_staged_server.exe `
  --layer-begin 0 --layer-end 1 --n-gpu-layers 999 --device CUDA0 --timeout-ms 180000
```

Result: `status=passed`, `HELLO operation=1`, `UNLOAD operation=9`, process
exit `code=0`, elapsed `26489 ms`. CPU-mapped buffer was 622.76 MiB and the
runtime reported `[0,1)` graph-stage lines and `READY`.

After both runs, no `p4_staged_server` process remained. Existing unrelated
GPU processes were not terminated.

## Nonclaims

- This proves bounded five-shard model discovery/loading and a real `[0,1)`
  staged load/HELLO/unload path; it does not prove full MiniMax-M2.7 loading.
- It does not prove MoE expert dispatch, token generation, logits equality,
  KV restore, MTP, speculative decoding, or multi-node execution.
- The harness output does not sample peak `nvidia-smi` memory during the load;
  the GPU policy conclusion is therefore based on the planner lower bound and
  successful bounded CUDA load, not a peak-VRAM measurement.
