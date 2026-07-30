# Kvasir Gateway — architecture and operations

What the public Kvasir deployment consists of and how the pieces relate. For
the step-by-step of standing one up on a new machine, see
[DEPLOY_GATE.md](DEPLOY_GATE.md).

## The shape

Two public hostnames, one tunnel, three services:

```
browser / Electron app / mobile node
        │
        ▼  Cloudflare Tunnel — the only public path
┌──────────────────────────────────────────────────────────────┐
│ gate.kvasir-ai.net → :8791   solana/staking-service          │
│     wallet web app + /api/node/*, /api/stake, /api/pay/*,    │
│     /api/credits/*, /api/inference, /api/config              │
│                        │ x-linkcpp-service-token             │
│ hub.kvasir-ai.net  → :19000  controller/hub.py               │
│     operator auth, settlement view, linker SPA at /linker    │
│                        │ delegation over the compose network │
│                          :19001  linker  ← never published   │
│                             └─ ring stages → the model       │
└──────────────────────────────────────────────────────────────┘
```

**gate** and **hub** are two different APIs with no overlapping paths. The
wallet apps have `gate.kvasir-ai.net` compiled in, so the names are not
interchangeable — pointing one at the other service yields 404s behind a login
wall, not a working app.

**linker is never published.** It has no authentication of its own; the hub
gateway is its only client and the thing that authenticates.

## What each service owns

### gate — `solana/staking-service/` (Node/Express)

Verifies on-chain KVR transfers, tracks staking positions, the node registry,
rewards and credit accounts, and brokers inference payments. Also serves the
wallet web app (the same `wallet/desktop` React build, at `/`).

Two ways to pay for inference, both live:

- **Pay per request** — `/api/pay/quote` → client signs a KVR transfer →
  `/api/inference`. No API key; holding KVR is what grants access. This is what
  the desktop app's AI Inference screen uses.
- **Credit accounts** — `/api/credits/register` (wallet signature) →
  `/api/credits/apikey` → `Authorization: Bearer` on `/v1/chat/completions`.
  OpenAI-compatible, debited from a prepaid balance. Self-registration is
  gated by `KVR_CREDIT_OPEN_REGISTER`; spending is gated by
  `KVR_CREDIT_MIN_BALANCE`, so a key with no deposit cannot infer.

The model list is not local: `fetchModelsFrom()` polls the hub's
`/api/controllers` and surfaces only controllers that are `runtime_loaded` and
serving. It swallows connection errors and returns `[]`, so a misconfigured
`LINKCPP_HUB_URL` or a mismatched service token shows up as an empty model
dropdown with nothing in any log.

### hub — `controller/hub.py` (Python/FastAPI)

Owns authentication, the settlement view, the UI shell, and serving linker's
SPA at `/linker`. **It no longer implements the control plane**: nodes,
controllers, planning, model loading, runtime state and inference are delegated
to linker over its API (`controller/linker_client.py`).

Deliberately not delegated, and still implemented here:

- **The MoE expert market** — dispatch port allocation, relay registry,
  recruitment targets, scarcity-weighted contribution flush. Linker exposes
  same-named routes, but this hub's implementation is the one in use.
- **External-controller registration** (`/api/controllers/external`).
- **The stage/ring proxy subsystem** (`controller/proxy/`), which has its own
  module-boundary tests.

Auth is **required by default**. Without `LINKCPP_ADMIN_WALLETS` (or a KVR
balance gate) nothing can pass it — that is the safe failure for a process
fronting an unauthenticated control plane, and it is logged at startup.
Sessions carry a short idle life (`LINKCPP_SESSION_TTL`, default 300s): the UI
watches real input, slides the session forward via `/api/auth/touch`, and locks
the screen when the window elapses. A 401 on a browser navigation renders the
lock screen rather than JSON, so the `/linker` window recovers by signing in.

### linker — the `convertarchitecture` checkout (Node)

The control plane proper: node slots, controllers, layer placement, model
loading, and the ring runtime that actually serves the model. Consumed only
through its REST/WebSocket API and otherwise left untouched.

A ring needs **at least two stages**. With one node slot the ring's `--next`
points at its own `--listen`, the reset acknowledgement never returns, and the
stage exits — configure two slots on the same GPU and split the layers.

## Token facts

Source of truth is `wallet/shared-spec/token.devnet.json`, read by the server
and every wallet.

| | |
| --- | --- |
| Mint (devnet) | `6cuJAmqtMuGzJ7s7eWQSqfJvEFRUdTiYR3cuMmiNoCPQ`, decimals 6 |
| Treasury owner / mint + freeze authority | `8uu2gDKFVtNS79yqYyztJeerEKAh4cnZGdQytCjsYNfF` |
| Treasury ATA (vault) | `38kt9QVdwt8r6auHK57rF1zkHSLq1dPChhJX2FuoY4CF` |
| Genesis / governance wallet | `JcVYk4PpP5m2kDhGzJAmYAtD8GF7svNgNK1V1BzTrRG` |

The treasury signing key (`admin.json`) holds all three authorities and is
**unrecoverable if lost**. It is gitignored and must be provisioned per host;
see DEPLOY_GATE.md for how to verify a copy before using it.

## Reward economics

`reward = rawUnits × perfTierMult × gatewayBonus`, where `rawUnits` accrues as
`(output_tokens / 1000) × the node's layer share` — one unit per 1k tokens,
split by how much of the model a node holds. A node that hosts the gateway
earns a +50% bonus (`KVR_GATEWAY_BONUS`, default 1.5).

Metering happens where execution happens: linker credits each completion to the
participating nodes and exposes the ledger, and the hub's `/api/contributions`
is the single public settlement surface the payout service polls.

## Wallets

Native iOS (Swift), Android (Kotlin), and desktop (React + Electron) under
`wallet/`. Internal identifiers remain `ai.banya.linkcpp.*` on purpose; only
display names are "Kvasir".

The desktop app runs in three modes, selected at runtime in
`wallet/desktop/src/api.ts`: the Electron bridge when `window.linkcpp` exists,
otherwise the **browser wallet** (`browserWallet.ts` — same derivation as
Electron main, mnemonic encrypted in browser storage, signing in-page), and a
mock only for dev preview. The hosted web app at `gate.kvasir-ai.net` is
therefore a full wallet, not a read-only console.

`wallet/shared-spec/wallet-constants.json` carries the cluster endpoints. Note
its keys are `devnet` and `mainnet-beta`, while the app's `Network` type is
`'devnet' | 'mainnet'` — `rpc('mainnet')` currently resolves to `undefined` and
throws. Devnet is unaffected; fix before any mainnet switch.

## Repository

- Origin: `github.com/louisevandan/kvasir-net`, branch `kvasir-net`.
- The linker control plane lives in a separate checkout on the
  `convertarchitecture` branch of `github.com/hikaMaeng/linkcpp` and is treated
  as an external dependency.
- Secrets — `.env`, `solana/staking-service/secrets/`, tunnel credentials — are
  gitignored and provisioned per host.

## Operational notes

- **One machine per tunnel.** Cloudflare load-balances across every connected
  connector, so a second `cloudflared` on the same tunnel ID makes routing
  non-deterministic. Check for strays with `ps -eo pid,cmd | grep cloudflared`.
- **Neither service is published on `0.0.0.0`.** The tunnel connects from the
  host, so both bind loopback; linker has no host port at all.
- **The settlement ledger is a named volume** (`gateway-data`,
  `/app/data/positions.json`) holding stake positions, the node registry, credit
  accounts and API key hashes. Migrating a host means moving that volume.
- **linker's image copies `apps/linker/dist`** rather than compiling — run
  `npm run build:server` before `docker compose build`, or the change will not
  be in the image.
