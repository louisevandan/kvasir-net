# Kvasir AI Network

**[kvasir-ai.net](https://kvasir-ai.net)**

Run large GGUF models — including 100B+ MoE models no single machine can hold —
across many GPUs, machines, and even phones, using stock **linkcpp** (a vendored
GGUF inference engine) as the data plane.

Kvasir AI Network is a Dockerized control hub with selectable linkcpp RPC and
Kvasir ring-proxy runtime contracts. It discovers local GPUs, keeps five fixed
RPC-backed node slots, imports nodes from other Kvasir AI Network units, links
native managed agents from Linux, macOS, Windows, iOS, and Android hosts, plans
layer (and expert) placement, launches `ggml-rpc-server` workers, starts a
GPU-less `linkcpp-server` master, and exposes OpenAI- and Anthropic-compatible
endpoints — plus a KVR pay-per-use inference gateway.

[![License: BSL 1.1](https://img.shields.io/badge/license-BSL%201.1-blue.svg)](LICENSE)
![Runtime](https://img.shields.io/badge/runtime-Docker%20hub%20%2B%20native%20nodes-blue)
![Data plane](https://img.shields.io/badge/data%20plane-linkcpp%20RPC%20%2F%20ring-6b7280)

## Why Kvasir AI Network?

linkcpp already has a capable RPC data plane. Kvasir AI Network adds the missing
control plane around it, and a **swarm** layer on top:

- **One hub UI:** configure GPUs, controllers, models, remote units, runtime
  compatibility, load plans, logs, and request testing from `http://localhost:19000`.
- **Fixed local node slots:** a unit has up to five local GPU slots mapped to
  RPC ports `50052-50056`. Slots exist from startup; you edit their GPU and
  resource budgets instead of dynamically creating arbitrary nodes.
- **Restart-safe hub configuration:** local slot names, resource budgets,
  controllers, bindings, and remote unit registrations are persisted under
  `/models/linkcpp/hub-state.json`, so a hub restart does not erase the
  configured layout.
- **Remote units:** register another running Kvasir AI Network hub, import its
  visible nodes with unit/controller metadata, and bind them through the same
  controller workflow.
- **Managed node agents:** optional `controller.nodeagent` workers can join a
  controller from Linux, macOS, or Windows, report resources and operations,
  download models, start native RPC workers, and unload through short HTTP
  control calls.
- **Mobile nodes:** phones can serve model layers (or MoE expert slices) as
  local-shard or RPC workers (OpenCL/Vulkan/CPU), managed from the non-custodial
  **Kvasir wallet**, with performance-weighted **KVR** node-operator rewards. See
  [Mobile Nodes](#mobile-nodes).
- **Expert-sharded MoE swarm:** shard a large Mixture-of-Experts model at
  *expert* granularity so a 4 GB phone can hold and compute a slice, and earn KVR
  for it. See [Expert-Sharded Swarm](#expert-sharded-swarm).
- **Runtime compatibility checks:** units, remote nodes, and managed agents
  report a protocol/runtime-pack identity plus separate backend runtime details.
  Unit, runtime-pack, engine, and RPC ABI mismatches are blocked before bind,
  plan, load, or inference. CUDA/ROCm/Metal/Vulkan/CPU backend differences are
  tracked as node capability data rather than hard protocol mismatches.
- **Gateway APIs:** each controller exposes Chat Completions, Responses, and
  Anthropic Messages routes backed by the same loaded model — plus a public,
  wallet-metered KVR inference gateway.

## Current Runtime Contract

The supported deployment is a single Docker Compose service:

```text
browser / SDK
    |
    v
Kvasir hub host :19000 -> container :9000
    |-- local node-slot-1 -> ggml-rpc-server :50052
    |-- local node-slot-2 -> ggml-rpc-server :50053
    |-- local node-slot-3 -> ggml-rpc-server :50054
    |-- local node-slot-4 -> ggml-rpc-server :50055
    |-- local node-slot-5 -> ggml-rpc-server :50056
    |-- remote unit nodes
    |-- managed node-agent nodes (Linux / macOS / Windows / iOS / Android)
    `-- GPU-less linkcpp-server master :8080+
```

Important details:

- `docker/Dockerfile` builds `ggml-rpc-server`, `linkcpp-server`, and the Python
  hub into one runtime image.
- `docker-compose.yml` starts one portable CPU `hub` service exposed on host
  TCP `19000` and listening inside the container on TCP `9000`.
- Local Docker workers use internal RPC ports. On macOS, the native Metal
  agent owns Slot 1 RPC port `50052` directly on the host; Docker must not
  publish that port range or it prevents Metal from starting.
- Models are visible in the container at `/models`.
- Staged model copies and hub configuration live under `/models/linkcpp` inside
  the same model folder mount.
- The data plane is the vendored linkcpp engine (RPC + ring runtimes).

## Quick Start

The engine is vendored in-tree (no submodules), so a plain clone is enough:

```bash
git clone https://github.com/louisevandan/kvasir-net
cd kvasir-net

docker compose up -d --build
open http://localhost:19000
```

To use a custom model folder, create `.env` from `env.example`. Set `CUDA_ARCHS`
to your local GPU architecture for faster single-machine builds, or keep the
default multi-architecture value for a more portable image:

```text
MODELS_DIR=C:/Users/admin/.lmstudio/model
CUDA_ARCHS=75;80;86;89;90;120;121
LINKCPP_UI_PORT=19000
LINKCPP_MAX_NODES=5
```

The deploy helper performs the same build/start/wait loop:

```bash
bash scripts/deploy.sh
```

## First Load

1. Open the hub UI at `http://localhost:19000`.
2. Select one of the five local node slots.
3. Assign a GPU, name, VRAM budget, RAM offload budget, and CPU core budget.
4. Create a controller.
5. Bind configured slots or imported remote nodes to the controller.
6. Choose a GGUF model from `/models`.
7. Choose **linkcpp RPC (stable)** or **Kvasir ring proxy (preview)**, then
   load the model and review the placement plan.
8. Use the built-in request test or call the controller API.

Resource assignment can only be changed while a slot is unbound. This prevents
changing the capacity contract under a running controller.

## API Endpoints

### Native OpenAI / Anthropic gateway (per controller)

Each controller gets gateway routes under `/c/{controller_id}`:

| Endpoint | Purpose |
| --- | --- |
| `GET /c/{controller_id}/v1/models` | OpenAI-compatible list of the loaded model |
| `POST /c/{controller_id}/v1/chat/completions` | OpenAI Chat Completions, including streaming |
| `POST /c/{controller_id}/v1/responses` | OpenAI Responses-style translation |
| `GET /c/{controller_id}/anthropic/v1/models` | Anthropic-compatible list of the loaded model |
| `POST /c/{controller_id}/anthropic/v1/messages` | Anthropic Messages translation |

OpenAI-compatible base URL: `http://<host>:19000/c/<controller_id>/v1`. These
routes are unauthenticated and meant for a trusted host / LAN / VPN only.

### Public KVR inference gateway (pay-per-use)

For public access, the network fronts the hub with a wallet-metered gateway
(`solana/staking-service`). Each inference is settled by an on-chain **KVR**
payment; no request reaches the model until the payment is verified.

```text
1. GET  /api/pay/models                        -> list served models
2. POST /api/pay/quote   {model, prompt}        -> requestId, priceToken, recipient, mint
3. (on-chain) transfer priceToken KVR to the recipient's token account, signed by the wallet
4. POST /api/inference   {requestId, signature} -> result + actual token usage
```

Useful hub APIs:

| Endpoint | Purpose |
| --- | --- |
| `GET /api/runtime` | Current unit runtime identity and compatibility metadata |
| `GET /api/gpus` | Visible local GPUs |
| `GET /api/nodes` | Local fixed slots plus imported or managed nodes |
| `POST /api/nodes` | Assign or edit a local slot while unbound |
| `GET /api/controllers` | Logical serving controllers |
| `POST /api/controllers/{id}/bind` | Bind a configured compatible node |
| `POST /api/controllers/{id}/load` | Plan and load a model |
| `POST /api/controllers/{id}/unload` | Stop the master and workers |
| `POST /api/remote-units` | Register another Kvasir AI Network unit |

## Remote Units

A unit is one running Kvasir AI Network hub. To use GPUs on another machine:

1. Start Kvasir AI Network on the remote GPU machine.
2. Make TCP `19000` and `50052-50056` reachable from the controller machine.
3. In the local hub UI, open a controller's **Node** tab.
4. Add the remote unit URL, for example `http://192.168.1.50:19000`.
5. Bind compatible imported nodes to the controller.

The communication boundary remains the unit: the local hub derives the data
plane endpoint from the registered unit host and the unit-exposed worker port,
not from an arbitrary node host advertised by the remote system.

## Managed Node Agents

The managed node-agent path is for worker services controlled by a hub instead
of being another full hub UI — the standard path for adding Linux, macOS, and
Windows machines as nodes.

Managed agents expose `POST /control/join`, `GET /control/status`,
`POST /control/download`, `POST /control/load`, `POST /control/unload`, and
report back with `POST /api/node-reports` over request/response HTTP (easy to
run over simple LAN or VPN routing).

Build and run a native Linux/macOS node:

```bash
bash scripts/build-node-runtime.sh auto
LINKCPP_VRAM_BUDGET=24 LINKCPP_RAM_BUDGET=64 LINKCPP_CORES=12 \
  bash scripts/run-node-agent.sh auto
```

The `auto` backend uses Metal on macOS, CUDA on Linux/Windows when `nvidia-smi`
is present, and CPU otherwise. Explicit backends are `cuda`, `rocm`, `metal`
(macOS), `vulkan`, and `cpu`. See [docs/NATIVE_NODES.md](docs/NATIVE_NODES.md).

## Expert-Sharded Swarm

The swarm goal: run a model no single owner can afford across heterogeneous,
weak, and NAT'd devices — phones included — each holding a small slice and
earning by reward.

Mixture-of-Experts models are the natural substrate. In a 122B-A10B model, most
of the weight is thousands of small, independent experts (~5 MB each), so
Kvasir shards at **expert** granularity instead of whole layers. A backbone
stage runs attention, KV, norms, the **router**, the shared expert, and the
combine; **expert workers** are pure `(hidden, local_ids) -> out` functions. The
router runs once on the backbone (its authority), dispatches the selected
experts to their owner nodes, and the backbone combines the results.

**Cross-backend numerical equivalence** is the core enabling technology.
Heterogeneous backends (CUDA / ROCm / Adreno / CPU) are not bit-identical
(~1e-3..1e-6 per op) but are numerically equivalent, and router authority turns
what would be catastrophic discrete divergence into bounded continuous error —
so a swarm of mixed hardware emits the same tokens. Verified on real 122B
across NVIDIA Blackwell (CUDA), AMD (ROCm gfx90a), and ARM/x86 CPU: the two GPU
backends match at cosine 1.0, and GPU vs CPU at cosine 0.99975.

## Mobile Nodes

Phones can join a hub as inference nodes. A device runs a native
`ggml-rpc-server` (cross-compiled with the NDK for `arm64-v8a`) or an expert
worker, and serves layers/experts to the controller; the non-custodial **Kvasir
wallet** (iOS + Android + desktop) is the control surface for onboarding,
monitoring, and rewards.

**Two node modes** (selectable in the wallet's *Mobile Node Settings* page):

- **Local shard** — the phone runs its assigned layer/expert shard locally and
  relays only activations. Fastest on mobile; avoids per-tensor RPC round-trips.
- **RPC worker** — the hub drives tensor ops over stock ggml RPC. Simpler, but
  network latency dominates on a phone.

The mobile worker supports **OpenCL** (Adreno kernels), **Vulkan**, and **CPU**
backends. A phone (Galaxy S25) has participated in real 122B inference end to
end: it autonomously downloaded its expert slice, served it, and computed one
layer's experts every token while the backbone generated — producing tokens
identical to the reference.

**Node-operator rewards.** Contribution is settled off-chain by the
`solana/staking-service` and paid in the **KVR** SPL token (devnet). Rewards are
**performance-weighted**: a node reports its measured throughput and lands in a
tier that scales its reward.

| Tier | Throughput | Reward multiplier |
| --- | --- | --- |
| S | ≥ 90 tok/s | ×1.5 |
| A | 60–89 tok/s | ×1.25 |
| B | 30–59 tok/s | ×1.0 |
| C | < 30 tok/s | ×0.7 |

Effective contribution is `rawUnits × multiplier`, so a faster node earns more
for the same work. Any desktop/laptop can also link to a wallet account and
appear in the same node monitor via the zero-dependency `solana/node-client`
(`node connect.js`). See [solana/node-client/README.md](solana/node-client/README.md).

## Planning And Loading

The planner reads GGUF metadata and estimates contiguous per-node layer
placement, `--tensor-split`, KV-cache / layer / expert VRAM, and optional expert
FFN offload to node RAM. The hub blocks adaptive loading when a node cannot
provide the resource monitoring needed for a safe plan.

## Operations

The UI includes:

- **Nodes:** fixed local slots, remote unit nodes, managed agents, resource bars,
  logs, runtime compatibility, and slot assignment.
- **Controllers:** Node and Inference tabs, bind/unbind, model load/unload,
  placement plans, operations, gateway endpoint URLs, and request testing.
- **Runtime:** unit version, runtime pack, pinned engine revision, RPC ABI,
  backend runtime details, and per-node protocol compatibility status.
- **Models:** on-device model management and inference (desktop + mobile).

Controller definitions, node bindings, local slot resources, and remote unit
registrations are restart-safe under `/models/linkcpp/hub-state.json`. Live
worker/model processes and in-flight operations remain runtime state; restarting
the container stops active serving and requires loading again.

## Development

Fast dev checks without a full rebuild:

```bash
python3 -m py_compile controller/hub.py controller/planner.py controller/protocol.py \
  controller/nodeagent.py controller/versioning.py controller/host_resources.py
node --check controller/web/hub.js
python3 -m unittest discover -s tests/proxy -t .
```

Headless UI acceptance after the hub is running:

```bash
bash scripts/acceptance.sh
```

## Project Layout

```text
apps/                small C++ app targets (expert worker, verify harness, node)
controller/          hub, planner, node agent, protocol models, and web UI
docker/              single-image CUDA runtime Dockerfile
docker-compose.yml   single hub service and published unit ports
docs/                architecture, API, deployment, and design notes
docs/CODE_MAP.md     feature-oriented code entrypoint for small-context updates
external/llama.cpp   vendored linkcpp engine source (incl. mobile ggml-RPC +
                     MoE expert-dispatch patches)
kvasir-home/         marketing site (Cloudflare Pages)
models/              optional local GGUF folder
scripts/             deploy and acceptance helpers
solana/              KVR token tooling, off-chain staking/rewards + KVR inference
                     gateway, node client
src/                 Kvasir AI Network C++ support code
tests/               unit, acceptance, and historical validation reports
wallet/              non-custodial KVR wallet + mobile node apps (iOS, Android, desktop)
```

## Security

The hub API, gateway routes, and RPC worker ports are unauthenticated today. Run
Kvasir AI Network only on a trusted host, trusted LAN, or VPN. Do not publish
port `19000` or `50052-50056` directly to the public internet — public access is
meant to go through the wallet-metered KVR gateway. Server hosts are configured
via environment (never hardcoded in source); see the `*.env.example` files.

## Roadmap

- Shared-token auth for hub and RPC-facing control paths
- On-chain (PDA-vault) settlement to replace the custodial devnet gateway
- App-native autonomous expert-slice loop (demand poll → volunteer → serve)
- 443-relay transport for cross-network / phone dispatch
- CI for Python parsing, JS syntax, Docker build, and hub smoke
- Mobile nodes: mainnet KVR settlement and iOS worker parity

## License

Business Source License 1.1. See [LICENSE](LICENSE). The vendored linkcpp /
llama.cpp / ggml engine under `external/llama.cpp` remains under its own MIT
license; the BSL applies to the Kvasir AI Network control plane and its original
components.
