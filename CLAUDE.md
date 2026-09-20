# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Kvasir runs large GGUF models across many machines. The engine is **p4**: agents own the
nodes on a host, **stage servers** hold contiguous slices of the model's layers, and a
stage hands its result to the next by dialing that stage's agent directly. p4 lives in its
own repository; this one holds everything around it.

```
gateway  solana/staking-service :8791   payments, settlement ledger, wallet app
   │ X-Kvasir-Service-Token
bridge   p4bridge :19000                what is loaded · who contributed · completions
   │ p4 events to each agent's advertised address
engine   p4 agents ──► stage servers ──► the model
```

Read [GATEWAY_GUIDE.md](GATEWAY_GUIDE.md) for what each layer owns and
[DEPLOY_GATE.md](DEPLOY_GATE.md) for how a host is stood up. The wiki under
`kvasir-home/src/wiki/entries.ts` is the same material written for readers.

## Build, run, test

```bash
# the site
cd kvasir-home && npm install && npx tsc -b && npm run build

# the gateway
cd solana/staking-service && npm install && node server.js

# the bridge
cd p4bridge && node --test test/
```

Serving a model is a placement plan plus a load — never a request to the bridge:

```bash
cd p4bridge
node make-plan.mjs                                    # writes load-plan.<model>.json
node load.mjs --plan load-plan.<model>.json --dry-run
node load.mjs --plan load-plan.<model>.json --confirm
node inspect.mjs                                      # every agent's node snapshots
```

## Things that will cost you a day if you do not know them

These are not style preferences. Each one was found the expensive way.

- **An event for a node that does not exist is forwarded, not refused.** The broker's rule
  for an unknown node is to send the event outbound. A LOAD addressed to the node it means
  to create therefore vanishes with no error. LOAD is addressed to the **agent**, in the
  engine's lifecycle envelope, with the adapter command as an opaque body.
- **The node generation and the load generation are one number.** The adapter compares a
  release receipt's source generation against the load it belongs to and stops the node when
  they differ — so a mismatched ring loads fine, answers one request, then loses its head and
  every later session hangs part-loaded. `load.mjs` refuses the mismatch before loading.
- **The load generation is unrecoverable.** It exists nowhere on the machines and UNLOAD
  needs it, so `load.mjs` writes it to `state/last-load.json` *before* the first command goes out.
- **An agent will not load a model without its operational journal** (`P4_EVENT_OUTBOUND_JOURNAL_DIR`).
- **Advertised addresses are not cosmetic.** A stage dials the next stage's agent at the
  address that agent advertises; loopback across hosts dials itself, silently.
- **The physical result bound must match exactly** what the stage server reports at READY, and
  it scales with `n_batch × n_ubatch`. `make-plan.mjs` derives it rather than remembering a constant.
- **Device names are backend-specific** — the HIP build names devices `ROCm0`, not `CUDA0`. A
  wrong name loads the whole model and *then* fails.
- **The agent and the native stage server are one release.** A newer agent fails at READY with a
  missing HELLO capability, again after a full load.
- **p4 applies no chat template.** The bridge renders the model's turn format from
  `prompt_format` in the catalog, and splits the reasoning block out of `content`. Skip it and
  an instruct model continues your text instead of answering it.
- **`pkill -f` matches its own command line.** `ssh host 'pkill -f server.js; ...'` kills the
  shell running it. Put the pattern in a script file, use a character class, and remember a
  process launched as bare `node server.js` has no path to match — find it by listening port.

## Where the code is

- `p4bridge/` — the OUTER. `server.js` (HTTP contract, chat template, reasoning split),
  `pipeline.js` (session install, submit, gather), `wire.js` (event framing, endpoints),
  `catalog.js` (what is loaded), `make-plan.mjs` / `load.mjs` / `inspect.mjs` (operations).
- `solana/staking-service/` — the gateway. `server.js` is the whole service: payments,
  staking, node registry, credit accounts, contribution poll, and the wallet app at `/`.
  `gatewayAuth.js` is SIWS + TOTP for the admin surface.
- `wallet/` — `desktop/` (Electron + Vite + React + TS), `ios/` (Swift), `android/`
  (Kotlin/Gradle), `shared-spec/` (constants shared by all three, and by the gateway).
- `kvasir-home/` — the public site. `src/wiki/entries.ts` and `src/tech/articles.ts` are the
  **English sources of truth**; `tr-<lang>.ts` files are merged over them per locale and must
  keep the same block sequence, because image positions are language-neutral.
- `external/llama.cpp` — vendored engine source.
- `tests/reports/`, `tests/plans/`, `benchmarks/` — **historical records**, not runnable and
  not to be rewritten. They are dated evidence of runs that happened.

## Conventions

- **Data-shape contracts cross process boundaries.** The gateway's node rows, the bridge's
  `/api/controllers` and `/api/contributions` shapes, and `wallet/shared-spec/` are read by
  the wallets and by each other. Changing a field name means changing every reader, or
  emitting both for a release.
- **Wire compatibility beats tidiness on a live deployment.** When renaming something the
  wallets or a running heartbeat send, accept both spellings before removing the old one,
  and migrate persisted rows rather than orphaning them — a renamed node id starts its reward
  history from zero.
- **The engine is unauthenticated by design.** Everything public goes through the gateway; the
  bridge binds loopback behind a service token; agents advertise on a private network. Do not
  publish an agent port or the bridge.
- **Say what is running.** The site distinguishes what serves today from what is designed —
  expert-grain sharding was demonstrated on the previous engine and is not yet on p4, and the
  wiki says so. Do not quietly upgrade a plan into a claim.
