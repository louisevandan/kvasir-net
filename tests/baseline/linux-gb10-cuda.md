# Linux NVIDIA GB10 CUDA acceptance baseline

This is the machine-specific acceptance baseline for the Linux aarch64 host
that runs linkcpp with its NVIDIA GB10 through NVIDIA Container Toolkit. It is
separate from the macOS Metal baseline: this deployment runs the hub and its
local RPC workers in Docker, and Docker owns the CUDA device.

## Scope

| Item | Baseline |
| --- | --- |
| Host | Linux aarch64, 20 CPU cores, 121 GiB RAM |
| GPU | NVIDIA GB10, CUDA compute capability 12.1 |
| Driver | NVIDIA 580.159.03 or a compatible newer driver |
| Build | CUDA 13.0.0, `CUDA_ARCHS=121`, 18 parallel build jobs |
| Hub | Docker Compose `hub`, host port 19000 by default |
| Runtime | CUDA `ggml-rpc-server` local slots 1--5, ports 50052--50056 |
| Slots | One physical GB10 may be split into logical slots only when their
  aggregate VRAM and CPU budgets fit the device and host budget. |

## Preconditions

1. Verify that the host has enough resources before compiling. This host has
   20 cores and about 103 GiB available RAM; reserve no less than 18 build
   jobs for its CUDA artifact build.

   ```sh
   nproc
   free -h
   docker info --format 'CPUs={{.NCPU}} Mem={{.MemTotal}}'
   ```

2. Verify GPU passthrough before building or starting the hub. Do not treat a
   successful host-only `nvidia-smi` as a Docker validation.

   ```sh
   nvidia-smi --query-gpu=name,compute_cap,driver_version --format=csv,noheader
   docker run --rm --gpus all nvidia/cuda:12.8.1-runtime-ubuntu22.04 nvidia-smi -L
   ```

   The GB10 must appear in both commands and report compute capability `12.1`.

3. Choose resource budgets conservatively. A GB10 reports unified memory on
   this host, so the sum of the logical-slot GPU budgets, the hub/master model
   mmap requirement, and normal host workload must fit at the same time.
   Never assign each logical slot the full reported memory capacity.

4. Keep a model directory available through `MODELS_DIR`. The smoke test below
   uses `Qwen2.5-7B-Instruct-Q4_K_M.gguf`; change the name only to an already
   present small GGUF and record the substitution.

## Build and start

Build the artifact with the GB10 architecture explicitly. The artifact build
requires a clean worktree because its identity includes the source revision.

```sh
CUDA_VERSION=13.0.0 CUDA_ARCHS=121 LINKCPP_BUILD_JOBS=18 \
  bash scripts/build-cuda-artifacts.sh rpc
CUDA_VERSION=13.0.0 CUDA_ARCHS=121 \
  bash scripts/build-cuda-runtime.sh rpc
docker compose -f docker-compose.yml -f docker-compose.cuda.yml up -d --no-build
curl -fsS http://127.0.0.1:19000/api/runtime
curl -fsS http://127.0.0.1:19000/api/gpus
```

For an isolated acceptance run, use a separate Compose project and UI port so
an existing serving hub is never modified:

```sh
LINKCPP_UI_PORT=19001 docker compose -p linkcpp-cuda-acceptance \
  -f docker-compose.yml -f docker-compose.cuda.yml up -d --no-build
```

## Required acceptance checks

### A. CUDA runtime identity and visibility

```sh
curl -fsS http://127.0.0.1:19000/api/runtime
curl -fsS http://127.0.0.1:19000/api/gpus
docker compose -f docker-compose.yml -f docker-compose.cuda.yml exec -T hub nvidia-smi -L
```

Pass criteria:

- The hub reports `backend_kind: cuda` and the expected unit/runtime-pack
  version.
- `/api/gpus` contains NVIDIA GB10 and the hub container can list the same GPU.
- Every configured local slot reports compatible CUDA runtime metadata.

### B. Logical slot allocation and RPC worker smoke test

Configure one or two GB10 logical slots with budgets whose total stays inside
the chosen device and host budget. Create a controller, bind the slots, then
load the small GGUF and make one short completion.

```sh
curl -fsS http://127.0.0.1:19000/api/nodes
curl -fsS http://127.0.0.1:19000/api/controllers
docker compose -f docker-compose.yml -f docker-compose.cuda.yml ps
```

Pass criteria:

- Each bound slot has a distinct RPC port in `50052`--`50056` and a running
  `ggml-rpc-server` worker.
- The planner accepts the model without exceeding either slot budget or the
  master mmap/RAM requirement.
- Model load reports `running`, a chat completion returns text, and the
  inference record includes CUDA node metrics.

### C. Headless hub UI regression

Run the existing UI acceptance suite against the isolated hub. It covers slot
configuration, controller creation, binding, and the model/chat flow when the
selected model is present.

```sh
LINKCPP_UI_PORT=19001 bash scripts/acceptance.sh
```

### D. Cleanup

Unload the controller and confirm that workers and allocations have gone away
before assigning a larger model or returning the GPU to another workload.

```sh
curl -fsS http://127.0.0.1:19000/api/controllers
docker compose -f docker-compose.yml -f docker-compose.cuda.yml exec -T hub \
  sh -lc "ps aux | grep '[g]gml-rpc-server' || true"
nvidia-smi
```

Pass criteria:

- The controller is idle after unload and local slots have no desired load.
- No stale worker remains for the unloaded controller.
- GPU memory use returns to the pre-load baseline within normal driver noise.

## Recorded run -- 2026-07-19

| Check | Result |
| --- | --- |
| Source / deployment | source `08a3265` with the current workspace changes, redeployed as unit/runtime-pack `0.0.10` |
| Host capacity before build | 20 CPU cores, 121 GiB RAM total, 107 GiB available |
| CUDA build allocation | CUDA 13.0.0, SM 12.1, 18 parallel jobs |
| Host GPU detection | NVIDIA GB10 / driver 580.159.03 / compute capability 12.1 |
| Container GPU passthrough | Passed: CUDA 13.0.0 runtime container listed GB10 at SM 12.1. |
| Hub runtime and GPU API | Passed: rebuilt hub reported `unit_version=0.0.10`, `runtime_pack_version=0.0.10`, and CUDA backend. |
| RPC worker smoke | Passed: slots 1 and 2 started `ggml-rpc-server` on ports 50052 and 50053. |
| Model load | Passed: `Qwen2.5-7B-Instruct-Q4_K_M.gguf`, 4096 context, 14/14 layer split across two 30 GiB logical slots, reached `running`. |
| Gateway inference | Passed: OpenAI-compatible chat returned `GB10 0.0.10 acceptance passed`; 12 completion tokens at 34.17 tok/s. |
| Proxy runtime | Not included in this RPC artifact deployment; `ring_proxy.available=false` is expected. |

The acceptance deployment intentionally replaced the previously running 0.0.8
hub. Its persisted controller was reloaded after the container recreation; do
not regard a persisted `running` record alone as evidence that its workers
survived a redeploy. Confirm `worker_running=true` and run a gateway request.

## Change report template

```text
Date / commit / VERSION:
Host CPU and available RAM before build:
CUDA version / CUDA_ARCHS / build jobs:
Host and container GPU detection:
Hub runtime-info and GPU report:
Slot budgets and model:
Load result / inference result / cleanup observation:
Failures, root cause, and remediation:
```
