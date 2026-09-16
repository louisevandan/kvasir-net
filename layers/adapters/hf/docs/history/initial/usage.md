> Historical record: requirements, status and measurements from the time of the standalone repository. The current layout and usage follow the [HF guide](../../../README.md). The complete original is in the Git bundle preserved during the migration.

> 2026-09-14: P4 integration is being implemented under the user's §0 instruction. For the current status of the earlier unconnected/read-only descriptions, the [integration specification](../../integration/README.md) and its acceptance report take precedence.

# Current usage and planned operating flow

Status: separates the commands available now from future flows.

## Available now

Model preparation, plan checks and scenario runs for Qwen3.5-0.8B follow the [dedicated script guide](../../models/qwen3_5_0_8b/README.md).

```powershell
Set-Location F:\dev\p4hfadapter
Get-Content AGENTS.md
Get-Content HANDOFF.md
git status --short
git log -1 --oneline
git remote -v
python -B tools/testing/run.py
python -B tools/verification/framing_mutation/run.py
python -B tools/verification/documents/run.py
```

The Python commands above use only the standard library and were verified on the installed CPython 3.13.15/Windows.
The framing mutation command records the independent copy, full logs and SHA256 in a new folder under `artifacts/framing/` and does not modify the original.
The peer in the transport tests is a verification process, not a model worker.
`pip install -e .`, `cargo build`, P4 services and quantize commands do not exist yet. Qwen runs use the dedicated entry point above.
P4 documents are consulted read-only, and the existing P4 build/deploy commands are not used as if they were this project's run commands.

## Planned operating order

1. Verify the pinned model/device/quantization manifest and the version lock.
2. Obtain the original or published quantized weights, and quantize them in a separate preparation task if needed.
3. Package the tensors and metadata each stage needs, and verify their hashes.
4. OUTER composes the nodes and requests LOAD for each stage.
5. After confirming the worker's actual kernels, memory and capability, bind the session route.
6. Accept requests and proceed with prefill/decode, settlement, output and release.
7. At shutdown, confirm: stop accepting new requests → stop execution/settle → return state/reservations → unload.
8. On failure, preserve the partial results (kept distinct from normal completion), the first error, cleanup errors and remaining resources.

The CLI/REST/socket syntax for the flow above is not decided yet. Even if a web service is attached, it is an OUTER client from P4's point of view,
and model-specific execution semantics are not moved into the P4 common core.

## Environment and outputs

The Python environment is created in an independent location such as this repository's `.venv/`. P4's environment and build directories are not reused.
Packages, kernels and compilers needed for CUDA/ROCm/MPS are installed after the support table is confirmed.
The Python lock and the Rust Cargo.lock are managed as a reproducible combination, with the policy decided when the actual packages are created.

Weights go in the root `models/`, measurements in `artifacts/`, Rust output in `target/`, and local audits in ignored paths such as `.local/`.
Dedicated model source in `python/p4hfadapter/models/<model>/<role>/` is tracked by Git.
Actual large files may live on an external path, but the manifest records their digest and how to re-obtain them.
Credentials and model weights are not committed to Git. No remote URL, deployment host or access token has been set up yet.

## Planned integrated deployment

Linking the Rust bridge into the P4 binary and deploying the Python environment, models and kernels are separate deliverables.
Implement a contract that deploys both revisions and the worker package identity together and rejects LOAD on a mismatch.
The change locations and scope of the final P4 integration task follow the [architecture](architecture.md).
