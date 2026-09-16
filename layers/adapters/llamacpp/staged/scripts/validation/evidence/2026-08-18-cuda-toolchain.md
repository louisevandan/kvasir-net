# CUDA 13.1 staged toolchain evidence

> Document status (2026-09-06): **Date- and environment-scoped evidence**. These are observations for the date, commit, model and topology stated in the body. They are not evidence that the current implementation or any other distributed environment is complete.
> Current goals, status and ordering follow the [execution roadmap](../../../../../../../docs/distributed-batching-roadmap.md); document authority and reading paths follow the [document map](../../../../../../../docs/document-map.md).

Observed 2026-08-18 KST on Windows with the staged server and prepared
compatibility worktree. The pinned upstream checkout was not modified.

## Host facts

| Item | Observed |
| --- | --- |
| CUDA Toolkit | `13.1.80` |
| `nvcc` | `C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.1\bin\nvcc.exe` |
| Visual Studio | Build Tools 2022 `17.14.24` at `C:\BuildTools` |
| C++ workload | found by `vswhere -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64` |
| CMake | bundled VS CMake `3.31.6-msvc6` |
| Ninja | bundled VS Ninja `1.12.1` |
| GPUs | RTX 3090 24,576 MiB; RTX 4080 16,376 MiB |

## Configure matrix

The staged CMake tree was configured with `GGML_CUDA=ON` and the prepared
`3e3a7a416d` worktree.

| Generator/toolset | Result | Evidence |
| --- | --- | --- |
| `Visual Studio 17 2022`, `-A x64`, `-T v143` | failed | `No CUDA toolset found.` from CMake 3.31.6 `CMakeDetermineCompilerId.cmake:614` |
| `Visual Studio 17 2022`, `-A x64`, `-T v143,cuda=13.1` | failed | MSBuild could not import `C:\BuildTools\MSBuild\Microsoft\VC\v170\BuildCustomizations\CUDA 13.1.props` |
| `Visual Studio 17 2022`, `-A x64`, `-T v143,cuda=C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.1` | configure passed | CMake generated the staged build files and CUDA compiler identification completed |

The first failure is therefore not missing `nvcc`, CUDA headers, a GPU, or
the MSVC C++ workload. It is a missing CUDA toolset selection in the Visual
Studio generator. The second failure shows that version-only selection points
MSBuild at the VS Build Tools customization directory, while this machine's
CUDA integration files remain under the Toolkit root. The path-form toolset
selects the correct installed integration files.

The successful configure command was equivalent to:

```text
cmake -S apps/p4/layers/adapters/llamacpp/staged/server -B <build> -G "Visual Studio 17 2022" -A x64 -T "v143,cuda=C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.1" -DP4_STAGED_BUILD_LLAMA=ON -DP4_STAGED_CUDA=ON -DGGML_CUDA=ON -DCMAKE_CUDA_ARCHITECTURES=75 -DP4_STAGED_LLAMA_SOURCE_DIR=.cache\llama-pipeline-upstream\3e3a7a416d-282f5e1fbdd7
```

## Build and memory-test status

The staged CUDA build completed successfully with the repository build script.
The first invocation exposed a staged CMake relative-source-path defect; the
staged CMake file was corrected to use `CMAKE_CURRENT_LIST_DIR`. No upstream
file was modified.

Reproduction command:

```powershell
$cmake = 'C:\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe'
node apps/p4/layers/adapters/llamacpp/staged/scripts/build-stage-server.mjs `
  --cuda `
  --cuda-root 'C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.1' `
  --cmake $cmake `
  --build-dir '.cache/staged-server-cuda-real-20260818' `
  --config Release `
  --cuda-architectures '86;89'
```

The build generated `ggml-cuda.dll`, `llama.dll`, `llama-common.dll`, and
`p4_staged_server.exe`; the staged native CTest suite passed 5/5. The cache
contains `GGML_CUDA=ON` and `P4_STAGED_CUDA=ON`, and CUDA code was compiled for
`sm_86` and `sm_89`.

Artifact paths:

```text
.cache\staged-server-cuda-real-20260818\Release\p4_staged_server.exe
.cache\staged-server-cuda-real-20260818\bin\Release\ggml-cuda.dll
```

Real GGUF validation passed. The Rust ignored two-stage test loaded
`S:\models\Qwen2.5-1.5B-Instruct-Q8_0.gguf` as `[0,14)` on GPU 0 (RTX 3090)
and `[14,28)` on GPU 1 (RTX 4080), forwarded a prefill cut-set, and received
`HopResult`. A second run used
`S:\models\unsloth\Qwen3.5-4B-MTP-GGUF\Qwen3.5-4B-Q8_0.gguf` as `[0,16)` and
`[16,32)` and also passed. During the 4B run, `nvidia-smi` observed sustained
memory usage of approximately 2,680 MiB on the 3090 and 3,970 MiB on the 4080,
within the test budgets of 23 GiB and 11 GiB. The GGUF metadata advertises 33
blocks, but the runtime's normal layer count is 32 because the extra MTP block
is not part of this normal decode range; `[0,16)`/`[16,32)` is therefore the
valid complete normal-layer split. This is device-residency evidence, not
logits-equality evidence.

Repeatable real-model command:

```powershell
$env:P4_STAGED_LLAMA_SERVER_BINARY = (Resolve-Path '.cache/staged-server-cuda-real-20260818/Release/p4_staged_server.exe').Path
$env:P4_STAGED_LLAMA_MODEL = 'S:\models\unsloth\Qwen3.5-4B-MTP-GGUF\Qwen3.5-4B-Q8_0.gguf'
$env:P4_STAGED_LLAMA_SPLIT_LAYER = '16'
$env:P4_STAGED_LLAMA_LAYER_END = '32'
$env:P4_STAGED_LLAMA_CUDA_VISIBLE_DEVICES_STAGE0 = '0'
$env:P4_STAGED_LLAMA_CUDA_VISIBLE_DEVICES_STAGE1 = '1'
$env:P4_STAGED_LLAMA_PLAN_EXTRA_ARGS = '--n-gpu-layers 99 --split-mode none --main-gpu 0'
cargo test --manifest-path apps/p4/Cargo.toml -p p4-llamacpp-staged-adapter `
  --test two_real_stages -- --ignored --nocapture
```

Using `P4_STAGED_LLAMA_LAYER_END=33` is invalid for this normal staged runtime
and reproduces the runtime assertion `end <= n_layer`; the MTP/nextn block is a
separate capability gate. This closes the prior `blocked: no completed CUDA artifact` status. Remaining
separate gates are logits equality, speculative/MTP execution, and production
P4 multi-node integration.
