# Remote RTX 3090 x2 / four-node staged evidence

> Document status (2026-09-06): **Date- and environment-scoped evidence**. It records observations for the date, commit, model and topology in the body. It is not completion evidence for the current implementation or for other distributed environments.
> Current goals, status and ordering follow the [execution roadmap](../../../../../../../docs/distributed-batching-roadmap.md); document authority and reading paths follow the [document map](../../../../../../../docs/document-map.md).

Observed 2026-08-18 KST. The central Windows host used already-built
artifacts; the remote Windows host did not run Cargo, CMake, or a source build.
The pinned llama.cpp upstream checkout was not modified.

## Topology

```text
central 192.168.0.6 RTX 3090  CUDA 0  stage [0,7)   agent :52003
central 192.168.0.6 RTX 4080  CUDA 1  stage [7,14)  agent :52004
remote  192.168.0.29 RTX 3090  CUDA 0  stage [14,21) agent :52001
remote  192.168.0.29 RTX 3090  CUDA 1  stage [21,28) agent :52002
```

Model: `Qwen2.5-1.5B-Instruct-Q8_0.gguf`. The central host used
`S:\models\Qwen2.5-1.5B-Instruct-Q8_0.gguf`; the remote host used the same
artifact from `D:\models\Qwen2.5-1.5B-Instruct-Q8_0.gguf`.

## Artifact transfer

The remote runtime directory contained `p4-agent.exe`,
`p4_staged_server.exe`, `ggml*.dll`, `llama*.dll`, and the CUDA runtime
dependencies `cublas64_13.dll` and `cublasLt64_13.dll`. The first Load attempt
failed on the remote stages because the CUDA runtime DLLs were absent. After
copying those two DLLs, the same test loaded all four nodes.

SHA-256 provenance:

```text
p4-agent.exe       A1B5BF6CBEBE49FB6C42EA3D2413D9816586D59037A067F4A0306CD9BBAFE91A
p4_staged_server.exe
                    59B33EFF352E41FDA8314138106D58B05F33E99685928E6865BB18B43712F11D
remote cublas64_13.dll
                    101AE2B98BE62704EC96E90A3C49373B76122FC6B502497A7B6FAE9AB0F01564
remote cublasLt64_13.dll
                    517B6A69AC9FAA7354CFFCBD92179AEC0CC18A8F6237D36B28D9ADFE8C912D8D
```

The build script now copies those CUDA runtime dependencies into the Release
artifact directory so a future bundle does not repeat this failure.

## P4 result

The actual `p4-drive` path was used with `P4_DRIVE_DISCOVER=1` and adapter
`llamacpp-staged`, not the direct Rust stage-server test. The four-stage run
reported:

```text
P4_DRIVE_DISCOVERY models=4 artifact=Qwen2.5-1.5B-Instruct-Q8_0.gguf
P4_DRIVE_NODES created=4
P4_DRIVE_LOADED nodes=4
P4_DRIVE_RESULT requests=1 tokens_each=1
  completed=1 failed=0 unanswered=0 routes=9
```

A second run used four requests and eight requested tokens:

```text
P4_DRIVE_LOADED nodes=4
P4_DRIVE_RESULT requests=4 tokens_each=8
  completed=4 failed=0 unanswered=0 routes=12
  peak_node_queue=2 peak_in_adapter=1
```

Both runs passed the driver checks for every request answered, no request
failed, stream order, and one terminal per route. The driver unconditionally
issued `UNLOAD`; the run ended with no staged server process. The four agent
processes remained only as the explicitly started node services and were then
stopped separately.

## CUDA evidence

The remote staged server was also held alive with its stdin plan pipe open on
GPU 0. Its stderr reported:

```text
ggml_cuda_init: found 1 CUDA devices ... NVIDIA GeForce RTX 3090
llama_prepare_model_devices: using device CUDA0 ...
load_tensors: offloaded 29/29 layers to GPU
load_tensors: CUDA0 model buffer size = 332.04 MiB
sched_reserve: CUDA0 compute buffer size = 60.00 MiB
READY port=5555
```

This is direct proof that the copied CUDA staged server can load and bind a
partial range on a remote RTX 3090. The remote `nvidia-smi` WDDM accounting
reported a 229 MiB baseline during this observation, so that number is not
used as the resident-weight measurement; the llama runtime log is the
authoritative device/buffer evidence here. All observed allocations were far
below the requested 23 GiB per-3090 budget.

## Latest four-node evidence

The original smoke gate was extended and rerun with the copied Release
artifact. `target/ssh-forwarded-four-node-e2e/20260818090647/result.json`
records `passed=true`, `completed=4`, `failed=0` for four concurrent requests
and eight tokens each. The observed overlap was
`peak_node_queue=2` and `peak_in_adapter=3`.

The run uses the central RTX 3090 and RTX 4080 plus two remote RTX 3090s. The
plan explicitly fixes `--device CUDA0`, `--n-seq-max`, and `--flash-attn 0`.
The SSH tunnel now also reverse-forwards central agent ports 52003 and 52004
so a remote tail can return to the central chain.

This closes the real remote four-node Load/HOP/Unload, concurrent request, and
stage-overlap gate. It does not close actual MTP/speculative execution or
multi-stage logits equivalence for a large model.

## Reproduction script

The manual run is now reproducible with the scoped PowerShell runner:

```powershell
& F:\dev\linkcpp_product\apps\p4\tools\scripts\e2e\run-ssh-forwarded-real-four-node.ps1 `
  -SshTarget '42mob@192.168.0.29' `
  -ArtifactDirectory 'F:\dev\linkcpp_product\.cache\staged-server-cuda-real-20260818\Release' `
  -LocalModel 'S:\models\Qwen2.5-1.5B-Instruct-Q8_0.gguf' `
  -RemoteModel 'D:\models\Qwen2.5-1.5B-Instruct-Q8_0.gguf'
```

The script performs no remote build. It copies only `p4-agent.exe`, the
staged server, and the required `ggml*.dll`, `llama*.dll`, and `cublas*.dll`
runtime files, then verifies SHA-256 manifests on both hosts before starting
the agents. The remote model is only checked at the supplied path; it is not
copied by the script.

SSH forwarding is deliberately split by direction:

```text
local 53001 -> remote 127.0.0.1:52001   remote stage agent 0
local 53002 -> remote 127.0.0.1:52002   remote stage agent 1
remote 127.0.0.1:52000 -> local 127.0.0.1:52000   driver replies
```

The central agents listen on `127.0.0.1:52003` and `:52004`; the driver
chains `52003,52004,53001,53002`. Each agent receives its own
`CUDA_VISIBLE_DEVICES` value. The remote agents run in foreground SSH
channels, so the local script owns and can terminate those channels together
with the tunnel; it also performs a remote port-based cleanup as a second
guard. Unless
`-KeepRemoteArtifacts` is supplied, it also removes the copied remote files
and run log directory; the source model is never removed.

### Network prerequisites

- SSH public-key or batch authentication from the central host to
  `42mob@192.168.0.29`; the script intentionally uses `BatchMode=yes`.
- The SSH account must be allowed to create both local forwards and a remote
  loopback reverse forward (`AllowTcpForwarding` and `PermitOpen` policy).
- No inbound firewall rule for the remote GPUs is required when SSH forwarding
  is allowed. The central host must allow local loopback ports `52000`,
  `52003`, `52004`, `53001`, and `53002` to be unused; the SSH client opens
  the two `-L` and one `-R` endpoints.
- If the SSH daemon or host firewall blocks reverse forwarding, the script
  fails before Load with `ExitOnForwardFailure`; do not interpret that as a
  model or CUDA failure. The saved `ssh-tunnel.err.log` is the authoritative
  diagnostic.
- Remote CUDA driver/runtime compatibility is still required. The remote
  machine must have two visible RTX 3090 devices and the copied CUDA DLLs must
  match the staged server artifact manifest.

The script writes `artifact-manifest.json`,
`remote-artifact-manifest.json`, `ssh-tunnel*.log`, per-agent logs,
`drive*.log`, and `result.json` below
`target\ssh-forwarded-four-node-e2e\<run-id>` for audit and reruns.
