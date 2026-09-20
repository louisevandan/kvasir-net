# Kvasir AI Network

**[kvasir-ai.net](https://kvasir-ai.net)**

Run large GGUF models — including 100B+ MoE models no single machine can hold —
across many GPUs, machines, and eventually phones, by cutting the model into
**stages** and giving each stage its own process on its own hardware.

The engine is **p4**: agents own the nodes on a host, stage servers hold slices
of the model's layers, and a stage hands its result to the next by dialing that
stage's agent directly. This repository holds everything around that engine —
the bridge that puts an HTTP face on it, the settlement gateway that takes
payment and credits the nodes that served, the wallets, and the public site.

[![License: BSL 1.1](https://img.shields.io/badge/license-BSL%201.1-blue.svg)](LICENSE)
![Engine](https://img.shields.io/badge/engine-p4-ff3d8b)
![Data plane](https://img.shields.io/badge/data%20plane-staged%20llama.cpp-6b7280)

> The p4 engine itself — agents, the staged llama.cpp adapter, and the
> multi-host evidence behind the site's proof numbers — lives in its own
> repository under the same BSL 1.1 terms.

## The shape

```text
browser / Electron app / mobile node
        │  Cloudflare Tunnel — the only public path
        ▼
gate.kvasir-ai.net → :8791   solana/staking-service
        │                     wallet app · payments · settlement ledger
        │  X-Kvasir-Service-Token
        ▼
                      :19000  p4bridge          ← loopback only
        │                     what is loaded · who contributed · completions
        │  p4 events to each agent's advertised address
        ▼
        p4 agents ──► stage servers ──► the model
```

Four layers with one seam each: wallets hold the keys, the gateway takes the
payment, the bridge fronts the engine, the engine runs the model. See
[GATEWAY_GUIDE.md](GATEWAY_GUIDE.md) for what each owns and
[DEPLOY_GATE.md](DEPLOY_GATE.md) for standing one up.

## Why this exists

A 428B model does not fit on one machine, and the machines that *could* hold it
are not the machines people already own. Kvasir's bet is that the interesting
hardware is distributed and mostly idle — so the work is in making a swarm of
imperfect devices behave like one endpoint:

- **No node holds the whole model.** A stage loads only its layer slice.
- **No master on the data path.** Stages pass boundaries to each other; the
  bridge submits to the head and reads from the tail.
- **Contribution is metered where execution happens.** Each stage reports the
  token rows it ran, and that is what settles to its owner's wallet.
- **Keys stay on the device.** Rewards are paid to each owner's own Solana
  address; the gateway never holds a user key.

## Quick start

Serving a model is two steps: load a placement plan onto the agents, then run
the bridge in front of them.

```bash
# 1. agents, one per compute host — listen address, then the address other
#    agents dial. Never loopback across hosts.
P4_EVENT_OUTBOUND_JOURNAL_DIR=$HOME/p4-journal \
  p4-agent 0.0.0.0:42011 tcp://10.10.10.111:42011

# 2. placement plan, then load
cd p4bridge
node make-plan.mjs
node load.mjs --plan load-plan.<model>.json --dry-run
node load.mjs --plan load-plan.<model>.json --confirm

# 3. the bridge
P4_BRIDGE_PORT=19000 P4_BRIDGE_HOST=127.0.0.1 \
P4_BRIDGE_TOKEN=<openssl rand -base64 32> \
P4_BRIDGE_OPERATOR_WALLET=<wallet credited for these machines> \
  node server.js
```

Placement is an operator artifact, not a request: the bridge answers `409` to
anyone asking it to serve. `load.mjs` writes the load generation to disk
**before** the first command leaves, because that number is unrecoverable and
UNLOAD needs it.

## API

### Inference, through the bridge

| Endpoint | Purpose |
| --- | --- |
| `GET /api/controllers` | which models are loaded, and every stage's state |
| `GET /api/runtime` | the operator wallet and the machines behind it |
| `GET /api/contributions` | per-node rows, units, requests, throughput |
| `POST /c/{model}/v1/chat/completions` | OpenAI-compatible completions |
| `GET /api/health` | unauthenticated liveness |

Everything but `/api/health` requires `X-Kvasir-Service-Token`. The bridge binds
loopback: its only authentication is that token, and anything that reaches it
can run the ring.

### Public KVR gateway (pay-per-use)

Public access goes through the wallet-metered gateway
(`solana/staking-service`). Each inference is settled by an on-chain **KVR**
payment, verified before the request reaches a model:

```text
1. GET  /api/pay/models                         -> models the swarm is serving
2. POST /api/pay/quote   {model, prompt}        -> requestId, priceToken, recipient, mint
3. (on-chain) transfer priceToken KVR to the recipient, signed by the wallet
4. POST /api/inference   {requestId, signature} -> result + actual token usage
```

There is no placeholder catalogue behind it: a model the app offers is one a
bridge is serving, or the list is empty. If the bridge then fails, the payer is
refunded from the treasury rather than charged for nothing.

OpenAI-compatible credit accounts are the other door:
`/api/credits/register` → `/api/credits/apikey` → `Authorization: Bearer` on
`/v1/chat/completions`.

## Joining as a node

The desktop installer ships the p4 agent with the app, so contributing is
install, unlock the wallet, and the node registers with the key already there.
A machine with no dialable address — a laptop behind a router, a phone behind
carrier NAT — reaches the network through the **relay**: it connects out, signs
the relay's challenge with its wallet keypair, and is reachable from then on.
That signature is what makes the node that joins and the wallet that gets paid
the same party.

## Project layout

```text
p4bridge/            the HTTP face of the p4 engine: plan, load, serve, meter
solana/              KVR token tooling, the settlement gateway, node client
wallet/              non-custodial KVR wallet + node apps (iOS, Android, desktop)
kvasir-home/         public site — wiki, tech blog, docs (Cloudflare Pages)
apps/ src/ cmake/    C++ support code and app targets
external/llama.cpp   vendored inference engine source
benchmarks/ tests/   benchmarks, acceptance runs, historical validation reports
scripts/ tools/      deploy and operations helpers
```

## Security

The engine speaks **no authentication**: p4 assumes the machines that can reach
each other are meant to. Everything public therefore converges on the gateway,
the bridge binds loopback behind a service token, and agents advertise on a
network that is not the public internet. Do not publish an agent port or the
bridge directly. Server hosts are configured via environment, never hardcoded;
see the `*.env.example` files.

## Roadmap

- Expert-grain sharding on p4, so a device can hold a fraction of a layer
- Settlement that follows weight share rather than participation, which expert
  sharding makes necessary
- On-chain (PDA-vault) settlement to replace the custodial devnet gateway
- Per-request observability from the agents, for the operator console
- Mobile nodes: mainnet KVR settlement and iOS worker parity

## License

Business Source License 1.1. See [LICENSE](LICENSE). The vendored llama.cpp /
ggml engine under `external/llama.cpp` remains under its own MIT license; the
BSL applies to Kvasir's own components.
