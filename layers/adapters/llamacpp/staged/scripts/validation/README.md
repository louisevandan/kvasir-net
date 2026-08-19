# Compat manifest validation

`validate-compat-manifest.mjs` is a read-only contract check for a staged
compatibility revision. It does not invoke Git, apply patches, create a
worktree, or modify `upstream`.

The manifest must contain:

- a full `upstream_commit` and the official repository URL;
- a non-empty `abi_revision`;
- contiguous, ordered `patches` (`0001-...patch`, `0002-...patch`, ...), with
  matching SHA-256 values and files beside the manifest; and
- non-empty, unique `required_artifact_symbols`.

Run it with:

```text
node validate-compat-manifest.mjs --manifest <compat>/manifest.json
node validate-compat-manifest.mjs --manifest <compat>/manifest.json --artifact <llama.dll>
```

The artifact check searches bytes for each declared symbol. It is a cheap
presence check, not proof of ABI calling conventions or successful linking.

The checked-in compatibility manifests predate this contract and currently do
not declare `abi_revision` or `required_artifact_symbols`; running this helper
against them intentionally reports that schema gap. Updating those manifests
is outside this helper's read-only scope.

## GGUF capacity preflight

`layer-window-memory-report.mjs` reads GGUF headers, retained metadata, tensor
offsets, shard sizes, per-layer tensor bytes, and an existing staged CMake cache.
It never calls `llama_model_load`; the window result is a tensor-byte lower
bound, not a VRAM-residency claim. The default budgets are 23 GiB for `3090`
and 11 GiB for `4080`:

```text
node layer-window-memory-report.mjs --models-dir S:\models --model-name Qwen2.5-1.5B-Instruct-Q8_0
node layer-window-memory-report.mjs --model S:\models\model-00001-of-00002.gguf --shard S:\models\model-00002-of-00002.gguf --budget 3090=23 --budget 4080=11
```

Use `--boundary-owner first|last` only when the runtime's boundary-tensor
ownership is explicitly known. Without it, boundary tensors are reported
separately and are not silently assigned to a stage.

## Short staged-server smoke

`smoke-stage-server.mjs` is intentionally bounded and uses the existing staged
server with a small GGUF. It performs one CPU load, `HELLO`, and `UNLOAD`, then
terminates. It does not prove CUDA, VRAM residency, logits equality, or a
23/11-GiB placement:

```text
node smoke-stage-server.mjs --model S:\models\Qwen2.5-1.5B-Instruct-Q8_0.gguf --executable .cache\staged-server-llama\Release\p4_staged_server.exe --timeout-ms 45000
```

The current host observation is recorded in
[`evidence/2026-08-18-s-model-qwen2.5-1.5b.md`](evidence/2026-08-18-s-model-qwen2.5-1.5b.md).

## MTP/speculative capability gate

`validate-mtp-speculative-capability.mjs` distinguishes shared-parser
acceptance from staged execution. It runs the same startup plan twice without
loading a model: `--validate-plan` must report parser support, while a normal
startup request must exit with `6` and `CAPABILITY_UNAVAILABLE`. A report with
`mtp_parser=1;mtp_execution=0` or
`speculative_parser=1;speculative_execution=0` is parser-only evidence, not an
execution claim:

```text
node validate-mtp-speculative-capability.mjs --executable <p4_staged_server.exe>
```

The CTest counterpart is `p4_staged_capability_test`. The report is also
included in the real server `HELLO` response. Normal decode remains a separate
execution capability and is covered by the short server smoke above.

## Windows CUDA toolchain gate

The CUDA path is explicit and is separate from the CPU smoke. On Windows, use
the staged build script with the CUDA Toolkit root:

```text
node apps/p4/layers/adapters/llamacpp/staged/scripts/build-stage-server.mjs --cuda --cuda-root "C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.1" --generator "Visual Studio 17 2022" --build-dir .cache\staged-server-cuda-vs
```

This maps `--cuda` to `P4_STAGED_CUDA=ON` and `GGML_CUDA=ON`, selects x64, and
passes the CMake generator toolset as:

```text
-T "v143,cuda=C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.1"
```

The path form is required on a VS Build Tools installation where CUDA's
`CUDA 13.1.props` and `.targets` files are installed below the CUDA Toolkit,
but are not copied to `C:\BuildTools\MSBuild\Microsoft\VC\v170\BuildCustomizations`.
The following outcomes are distinct:

1. `Visual Studio 17 2022` without `-T ...cuda=...` reproduces `No CUDA
   toolset found.` even when `nvcc 13.1.80` is available.
2. `-T v143,cuda=13.1` finds the version but then fails when the VS Build Tools
   `BuildCustomizations` directory lacks `CUDA 13.1.props`.
3. `-T v143,cuda=<CUDA root>` uses the installed Toolkit integration directory
   and is the reproducible VS configure form for this host.

Ninja is the alternative when MSBuild integration is intentionally unavailable.
Run it from the same script so the VS developer environment and explicit
compiler are initialized:

```text
node apps/p4/layers/adapters/llamacpp/staged/scripts/build-stage-server.mjs --cuda --cuda-root "C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.1" --generator Ninja --build-dir .cache\staged-server-cuda-ninja
```

Configure success is not an artifact result. Record a CUDA pass only when the
build creates the staged executable and CUDA backend artifacts, and the cache
contains `GGML_CUDA=ON`, `P4_STAGED_CUDA=ON`, and the expected generator. The
layer-window report above remains a GGUF tensor-byte lower bound; it is not a
GPU memory or VRAM-residency test. Run a GPU memory test only after those real
artifacts exist and the test's executable path is recorded. If no such artifact
exists, mark the GPU memory test `blocked: no CUDA artifact` rather than
substituting the CPU smoke or static tensor-byte report.

The investigation evidence for CUDA 13.1 + VS Build Tools and the completed
real-GPU staged runs is recorded in
[`evidence/2026-08-18-cuda-toolchain.md`](evidence/2026-08-18-cuda-toolchain.md).

## Exact 5000-token fixture

`make-5000-token-fixture.ps1` builds a small Windows probe against the public
header from the prepared patched source, then loads the already-built
`llama.dll` from the CUDA artifact directory with `vocab_only=true`. It uses
the GGUF's own `add_bos` setting and `parse_special=true`, searches a
deterministic UTF-8 prompt, writes a fixture, and verifies the count in the
same vocabulary process. It does not run inference, start the staged server,
or send `UNLOAD`; the process exits without an explicit model-free call after
the vocabulary-only check.

```powershell
& .\apps\p4\layers\adapters\llamacpp\staged\scripts\validation\make-5000-token-fixture.ps1
```

The default run uses the prepared Qwen3.8 GGUF, the CUDA Release artifact
under `.cache`, and writes `prompt-5000-tokens.txt`, `tokenizer-probe.txt`,
and `report.json` below `target\validation-5000-token\<run-id>`. Override
`-Model`, `-ArtifactDirectory`, or `-SourceDirectory` when reproducing on
another prepared artifact; the script rejects a non-CUDA artifact cache or a
source tree without the public header.
