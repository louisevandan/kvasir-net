# Kvasir Gateway — architecture and operations

What the public Kvasir deployment consists of and how the pieces relate. For
the step-by-step of standing one up on a new machine, see
[DEPLOY_GATE.md](DEPLOY_GATE.md).

## The shape

One public hostname, one tunnel, two services in front of the ring:

```
browser / Electron app / mobile node
        │
        ▼  Cloudflare Tunnel — the only public path
┌──────────────────────────────────────────────────────────────┐
│ gate.kvasir-ai.net → :8791   solana/staking-service          │
│     wallet web app + /api/node/*, /api/stake, /api/pay/*,    │
│     /api/credits/*, /api/inference, /api/config              │
│                        │ X-Kvasir-Service-Token              │
│                        ▼                                     │
│                          :19000  p4bridge  ← loopback only   │
│     /api/controllers, /api/runtime, /api/contributions,      │
│     /c/<model>/v1/chat/completions                           │
│                        │ p4 events to each agent's           │
│                        ▼ advertised address                  │
│          p4 agents ──► stage servers ──► the model           │
└──────────────────────────────────────────────────────────────┘
```

**The bridge is never published.** Its only authentication is a shared service
token, and anything that reaches it can run the ring. It binds loopback and the
gateway is its only client.

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

There is no local model catalogue. `fetchModelsFromBridge()` polls each
bridge's `/api/controllers` and surfaces only controllers that are
`runtime_loaded` and serving; when none is reachable the pool is empty rather
than falling back to placeholders. It swallows connection errors and returns
`[]`, so a misconfigured `P4_BRIDGE_URL` or a mismatched service token shows up
as an empty model list with nothing in any log.

Placement is not the gateway's to drive. When the ring watchdog finds a model
not serving it says so once and stops asking: the bridge answers `409`, because
which layers sit on which GPU at which load generation is decided by a
placement plan an operator wrote.

### p4 bridge — `p4bridge/` (Node)

The HTTP face of the p4 engine, and the whole contract the gateway speaks:

| Route | What it answers |
| --- | --- |
| `/api/controllers` | which models are loaded, and every stage's state |
| `/api/runtime` | the operator wallet and the machines behind it |
| `/api/contributions` | per-node rows, units, requests, throughput |
| `/c/<model>/v1/chat/completions` | inference |
| `/api/health` | unauthenticated liveness |

It is an OUTER in p4 terms: it installs a session across the stages, submits to
the head, and gathers the token stream. Two jobs p4 deliberately leaves to it:

- **The chat template.** p4 hands the stage server an opaque prompt and applies
  no template of its own. The bridge renders the model's turn format from
  `prompt_format` in the catalog. Without it an instruct model continues your
  text instead of answering and never emits its end-of-turn token.
- **The reasoning block.** A reasoning model opens its reply with `<think>`.
  The bridge returns it as `reasoning_content`, separate from `content`, and
  honours `chat_template_kwargs.enable_thinking: false` by closing the block in
  the prompt — a reasoning pass that eats the whole token budget would
  otherwise leave `content` empty and bill the payer for a blank reply.

### p4 agents and stage servers

The agent owns a host's nodes; a stage server is one process holding a slice of
the model's layers. A stage hands its result to the next by asking its own
agent to dial that stage's agent **at the address that agent advertises** — so
the advertised address must be reachable from the other hosts, and should be
the fastest network they share.

A pipeline needs **at least two stages**; the session command refuses a
one-stage pipeline.

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

`reward = rawUnits × perfTierMult × gatewayBonus`, where a node's `rawUnits`
accrue as `rows / 1000` — one unit per thousand token-rows it processed. A node
that hosts the gateway earns a +50% bonus (`KVR_GATEWAY_BONUS`, default 1.5).

Metering happens where execution happens: each stage reports the rows it ran,
the bridge accumulates them per node, and the gateway polls
`/api/contributions` every 30 s and credits the owner the bridge names.

Two properties of this worth knowing before tuning it:

- **In a pipeline every stage sees the same rows**, so four stages of a
  four-stage ring earn equally no matter how many layers each holds. Credit
  follows participation, not weight share. Expert sharding, where nodes hold
  different amounts of a layer, will need this revisited.
- **Contribution counters live in the bridge's memory.** A restart loses
  whatever the gateway had not yet polled, and the gateway rebaselines rather
  than double-counting when the counter goes backwards.

A node whose owner the bridge does not know is skipped **silently** — set
`P4_BRIDGE_OPERATOR_WALLET`, or the machines appear to have earned nothing.

## Wallets

Native iOS (Swift), Android (Kotlin), and desktop (React + Electron) under
`wallet/`. Internal identifiers remain `ai.banya.linkcpp.*` on purpose — they
are published application identifiers and changing one is a new install, not a
rename; only display names are "Kvasir".

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
- The p4 engine is a separate checkout (our fork); the agent and the native
  stage server must be built from the same tree, or the ring fails at READY
  with a missing HELLO capability after loading the whole model.
- Secrets — `.env`, `solana/staking-service/secrets/`, tunnel credentials — are
  gitignored and provisioned per host.

## Operational notes

- **One machine per tunnel.** Cloudflare load-balances across every connected
  connector, so a second `cloudflared` on the same tunnel ID makes routing
  non-deterministic. Check for strays with `ps -eo pid,cmd | grep cloudflared`.
- **Nothing is published on `0.0.0.0`.** The tunnel connects from the host, so
  the gateway binds loopback and the bridge has no public door at all.
- **The settlement ledger is a named volume** (`gateway-data`,
  `/app/data/positions.json`) holding stake positions, the node registry, credit
  accounts and API key hashes. Migrating a host means moving that volume.
- **`p4bridge/state/last-load.json` is the ring's equivalent.** It holds the
  load generation, which exists nowhere on the machines and which UNLOAD
  requires. Lose it and a loaded model cannot be taken down.
