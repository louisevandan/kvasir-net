# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

linkcpp is a **control plane** around llama.cpp's RPC data plane. It runs large GGUF
models across multiple GPUs and machines using *stock* `ggml-rpc-server` / `llama-server`
binaries. The data plane (llama.cpp) is unmodified except for one pinned mobile GPU-over-RPC
patch in the submodule. Everything linkcpp adds is orchestration: GPU discovery, node slots,
layer placement planning, worker launch, and OpenAI/Anthropic-compatible gateways.

The active runtime is a **single Docker image** running the Python FastAPI hub
(`controller.hub:app`) plus the two llama.cpp binaries baked in.

## Build, run, test

Everything runs through Docker Compose — the C++ binaries are only ever built inside the image.

```bash
docker compose up -d --build      # build image + start hub on http://localhost:19000
bash scripts/deploy.sh            # same build/start/wait loop as a helper
bash scripts/acceptance.sh        # headless Playwright UI acceptance (hub must be running)
```

Config lives in `.env` (copy from `env.example`): `MODELS_DIR` (host GGUF folder bind-mounted
to `/models`), `CUDA_ARCHS` (set to your GPU arch for faster single-machine builds),
`LINKCPP_UI_PORT` (default 19000), `LINKCPP_MAX_NODES` (default 5).

Fast dev checks without a full rebuild:

```bash
python3 -m py_compile controller/hub.py controller/planner.py controller/protocol.py \
  controller/nodeagent.py controller/versioning.py controller/host_resources.py \
  controller/stage_protocol.py controller/llama_updater.py controller/runtime_modes.py \
  controller/proxy/*.py controller/runtimes/*.py
node --check controller/web/hub.js
python3 -m unittest tests.unit.test_control_protocol tests.unit.test_runtime_modes \
  tests.unit.test_host_resources tests.unit.test_llama_updater
python3 -m unittest discover -s tests/proxy -t .   # proxy/ring runtime suite
```

Requires Python ≥3.9 with `pydantic`, `fastapi`, `httpx` importable (the stage-protocol
tests moved to `tests/proxy/test_stage_protocol.py`).

`tests/acceptance/*.py` are Playwright scripts (run via the Docker image in `acceptance.sh`),
not plain unittest. `tests/reports/` and `tests/plans/` are historical validation records, not runnable.

Native worker nodes (Linux/macOS/Windows) build/run outside Docker:

```bash
bash scripts/build-node-runtime.sh auto   # backend: auto|cuda|metal|vulkan|cpu
LINKCPP_VRAM_BUDGET=24 LINKCPP_RAM_BUDGET=64 LINKCPP_CORES=12 bash scripts/run-node-agent.sh auto
```

## Architecture

Request flow: browser/SDK → hub `:19000` (container `:9000`) → per-controller GPU-less
`llama-server` master `:8080+` → `ggml-rpc-server` workers on nodes (local slots `:50052-50056`,
remote-unit nodes, or managed agents).

Three ways a machine's GPUs join a hub:
- **Local node slots** — five fixed slots per hub mapped to RPC ports `50052-50056`. Slots always
  exist; you edit their GPU + VRAM/RAM/CPU budgets rather than creating arbitrary nodes. Resources
  are editable only while a slot is **unbound** (protects the capacity contract under a running controller).
- **Remote units** — register another running linkcpp hub; import its visible nodes. The data-plane
  endpoint is always derived from the registered *unit* URL + unit-exposed worker port, never from a
  node host the remote system advertises.
- **Managed node agents** (`controller/nodeagent.py`) — worker-only services that join a controller
  over request/response HTTP (`/control/join|status|download|load|unload`) and report via `POST /api/node-reports`.
  Deliberately not a persistent stream, to survive simple LAN/VPN routing.

**Runtime compatibility gating is a first-class concept.** Every unit/node/agent reports a
protocol/runtime-pack identity plus separate backend runtime details (`controller/versioning.py`).
Unit, runtime-pack, llama.cpp revision, and RPC ABI mismatches are **hard-blocked before bind/plan/load/infer**.
Backend differences (CUDA/Metal/Vulkan/CPU) are tracked as node capability data, not hard mismatches.
Adaptive loading is also blocked when a node can't provide the resource monitoring a safe plan needs.

**Planner** (`controller/planner.py`) reads GGUF metadata and produces contiguous per-node layer
placement, `--tensor-split`, KV-cache/layer/expert VRAM estimates, and optional expert-FFN offload to RAM.

**Persistence:** local slot names/budgets, controllers, bindings, and remote-unit registrations are
restart-safe in `/models/linkcpp/hub-state.json`. Live worker/model processes and in-flight operations
are runtime-only — a container restart stops serving and requires reloading.

**Gateway:** each controller exposes `/c/{id}/v1/chat/completions`, `/v1/responses`, `/v1/models`,
and `/anthropic/v1/messages|models`, all backed by the same loaded model.

### Where the code is

- `controller/hub.py` — **the monolith (~4200 lines)**, the only active hub runtime surface. Broadly
  sectioned (see `docs/CODE_MAP.md` for the section list and the per-feature file/function index).
  Start any change from the CODE_MAP row for your feature, not by reading top-to-bottom.
- `controller/nodeagent.py` — managed-agent worker service (**not** legacy).
- `controller/{planner,protocol,stage_protocol,versioning,host_resources,model_catalog}.py` — supporting modules.
- `controller/web/hub.{html,js,css}` — the active hub UI (vanilla JS). `hub.js` render/wire functions
  pair with hub endpoints per the CODE_MAP.
- `docker/Dockerfile`, `docker-compose.yml` — single CUDA image (builds `ggml-rpc-server` + `llama-server`
  + Python hub), one `hub` service.
- `external/llama.cpp` — pinned submodule with the mobile ggml-RPC GPU patch. `apps/linkcpp-node`, `src/`,
  `cmake/`, `CMakeLists.txt` — linkcpp C++ support (gated by `LINKCPP_BUILD`, OFF in the Docker build).
- `wallet/` — non-custodial LKC wallet + mobile node apps: `desktop/` (Electron + Vite + React + TS),
  `ios/` (Swift), `android/` (Kotlin/Gradle), `shared-spec/` (shared constants).
- `solana/` — LKC SPL token tooling, off-chain `staking-service` (Express, devnet), zero-dep `node-client`.

### Legacy — do not treat as active runtime

`controller/app.py`, `controller/nodehost.py`, `controller/web/{index.html,app.js,nodehost.html,style.css}`
are the older split controller/node-host architecture. Only touch them for deliberate compatibility work.

## Conventions

- **Docs update rule** (from `docs/CODE_MAP.md`): when a feature changes, update `docs/CODE_MAP.md`
  (if entrypoints/contracts/data shapes move), `docs/ARCHITECTURE.md` (if runtime boundaries change),
  `docs/blueprint/11-api-reference.md` (if endpoints/bodies change), and `README.md` (only for
  user-visible setup/workflow/API changes).
- **Preserve data-shape contracts**: `node_view(n)` (rows in `/api/nodes`), `ctrl_view(c, full=)`
  (`/api/controllers`), and the planner result stored on the controller are UI/API contracts — the
  `hub.js` renderers depend on their exact fields. `docs/CODE_MAP.md` lists the fields to keep.
- The hub, gateway routes, and RPC ports are **unauthenticated** — designed for trusted host / LAN / VPN
  only. Do not expose `19000` or `50052-50056` publicly.
