# Kvasir Gateway — Public Deployment Guide

> Self-contained handoff. Everything needed to continue this on **another machine / new session**
> (e.g. the Linux server) is in this file + the repo. Nothing here depends on local machine memory.

## 0. Goal

Run the **Kvasir gateway** on a **Linux server with a public IP**, as a **Docker** container, so that:

- it is always-on and reachable from anywhere (not just LAN mDNS `.local`), and
- it serves a **web application whose UI + features are identical to the desktop app**
  (the same `wallet/desktop` React app, built for the browser and served at `/`), while also
  exposing the gateway/staking/node/inference API under `/api/*`.

Nodes (iOS / Android / desktop wallets) then point their "settlement server URL" at the public gateway.

## 1. What Kvasir is (context)

- **Kvasir** = a decentralized-inference AI project. Its Solana devnet token is **KVR**.
  - Token mint: `6cuJAmqtMuGzJ7s7eWQSqfJvEFRUdTiYR3cuMmiNoCPQ` (devnet), decimals 6, supply 1e9.
  - Treasury owner / mint+freeze authority / settlement admin: `8uu2gDKFVtNS79yqYyztJeerEKAh4cnZGdQytCjsYNfF`.
  - Treasury ATA (vault): `38kt9QVdwt8r6auHK57rF1zkHSLq1dPChhJX2FuoY4CF`.
  - Token facts live in `wallet/shared-spec/token.devnet.json` (source of truth, read by server + wallets).
- **Wallets**: native iOS (Swift), Android (Kotlin), desktop (React + Electron), all under `wallet/`.
  Internal IDs are still `ai.banya.linkcpp.*` (kept on purpose); only display/product names are "Kvasir".
- **The gateway** = `solana/staking-service/` (Node/Express). It verifies on-chain KVR transfers,
  tracks staking positions + node registry + rewards, and brokers inference payments. Binds `0.0.0.0`,
  permissive CORS. Endpoints: `/api/config`, `/api/positions/:owner`, `/api/stake`, `/api/unstake`,
  `/api/node/{register,heartbeat,remove,contribution,rewards/:owner,status/:owner,all,claim}`,
  `/api/pay/{models,quote}`, `/api/inference`, `/health`.
- **Node reward economics**: reward = `rawUnits × perfTierMult × gatewayBonus`. A node that **hosts the
  gateway** earns a **+50% bonus** (`GATEWAY_BONUS`, env `KVR_GATEWAY_BONUS`, default 1.5); the wallet
  sends `hostsGateway:true` on register/heartbeat when it manages the gateway.

## 2. Git / where to work

- Work from the writable fork **`github.com/kr-ai-dev-association/linkcpp`**.
- Work branch: **`tony`**. Pull on the Linux box with: `git clone -b tony https://github.com/kr-ai-dev-association/linkcpp.git`
  (or `git fetch fork && git checkout tony`). Push with `git push fork tony`.
- The devnet **admin secret key is NOT in git** (gitignored at `solana/token/.keys/admin.json`). You must
  copy it to the server out-of-band (scp) — see §4. It is a **devnet** key; never reuse on mainnet.

## 3. What is already DONE (this session)

The gateway is **deploy-ready**; artifacts live in `solana/staking-service/`:

- **`server.js` refactored** to be config-injectable (backward compatible):
  - `KVR_TOKEN_SPEC` → path to the token spec (default `../../wallet/shared-spec/token.devnet.json`).
  - Admin key via `KVR_ADMIN_KEY` (inline JSON array) **or** `KVR_ADMIN_KEY_FILE` (path); dev fallback to `../token/.keys/admin.json`.
  - `KVR_PUBLIC_URL` → returned in `/api/config.publicUrl` so clients learn the canonical address.
  - **Serves the web UI**: if `KVR_WEB_DIR` (default `../../wallet/desktop/dist`) has an `index.html`,
    it is served at `/` with an SPA fallback (never shadows `/api/*` or `/health`).
- **`Dockerfile`** (multi-stage): stage 1 runs `wallet/desktop` `npm run build:web` (with
  `ELECTRON_SKIP_BINARY_DOWNLOAD=1`); stage 2 = server + `--omit=dev` deps + baked token spec + the web
  bundle at `/app/web`. Non-root user, `HEALTHCHECK` on `/health`. **Build context = repo root.**
- **`docker-compose.yml`** + **`.env.example`**: ports, RPC, APR, bonus, public URL, data volume,
  admin-key mount (`./secrets/admin.json`) or inline env.
- **`.dockerignore`** (repo root) excludes node_modules/dist/build/keys; **`.gitignore`** excludes
  `secrets/` + `.env` so the key never lands in git.

Verified locally (Node, no Docker): `server.js` parses, `/api/config` returns `publicUrl`, web UI serving
activates when `dist` exists, and the +50% gateway-host bonus math is correct (host node 225 eff vs 150).

## 4. Deploy on the Linux server (Docker)

Prereqs: Docker + docker compose plugin. Then:

```bash
git clone -b tony https://github.com/kr-ai-dev-association/linkcpp.git
cd linkcpp/solana/staking-service

# 1) config
cp .env.example .env
#    edit .env: set KVR_PUBLIC_URL to your address, e.g.
#      KVR_PUBLIC_URL=https://gw.example.com     (or http://<public-ip>:8791)

# 2) settlement admin key (devnet). Copy it from the machine that has it:
mkdir -p secrets
#    scp the file from the dev machine:
#      scp dev-mac:/…/linkcpp/solana/token/.keys/admin.json ./secrets/admin.json
chmod 600 secrets/admin.json

# 3) build + run  (context is the repo root; compose handles it)
docker compose up -d --build

# 4) verify
curl -s http://localhost:8791/health           # {"ok":true}
curl -s http://localhost:8791/api/config        # symbol KVR, gatewayBonus 1.5, publicUrl set
#    open http://<public-ip>:8791/  in a browser → the Kvasir web app (desktop UI)
docker compose logs -f gateway
```

Open firewall/security-group for the port (8791, or 443 if behind a proxy).

## 5. HTTPS (recommended for a public server)

Plain HTTP works for clients today (iOS ATS allows arbitrary loads; Android `usesCleartextTraffic=true`),
but a public gateway should be TLS. Easiest: put **Caddy** in front (auto Let's Encrypt):

```
# Caddyfile
gw.example.com {
    reverse_proxy 127.0.0.1:8791
}
```

Then set `KVR_PUBLIC_URL=https://gw.example.com` and point clients there.

## 6. Point the wallets at the public gateway

Each wallet has a **"정산 서버 URL / settlement server URL"** setting:
- Desktop: Settings → 정산 서버 URL.
- iOS / Android: settings / device-connect (staking URL).

Set it to your `KVR_PUBLIC_URL`. (Optional: change the shipped default in
`wallet/shared-spec/wallet-constants.json` `stakingServiceUrl` and `wallet/desktop/electron/constants.cjs`
`stakingServiceUrl`, then rebuild the apps — but per-device override is enough to test.)

## 7. Remaining work / TODO (the real functional gap)

The web app served by the gateway reaches **full desktop parity for dashboard / nodes / staking /
inference** (those go through `/api/*`). **Wallet key operations are the gap**: in a browser there is no
Electron main process, so `wallet/desktop/src/api.ts` currently falls back to a **mock** (`window.linkcpp`
is undefined). To make the hosted web app a real, full-parity wallet:

- Implement a **third `api` provider** in `wallet/desktop/src/api.ts` — a browser wallet that runs the same
  derivation as Electron main (`bip39` + `ed25519-hd-key` + `@solana/web3.js`, path `m/44'/501'/0'/0'`),
  stores the mnemonic **encrypted in IndexedDB/localStorage** (e.g. WebCrypto AES-GCM with a passphrase),
  and signs in the browser. Select provider by: `window.linkcpp` (Electron) → real bridge; else if running
  as a served web app → the new browser wallet; else (dev preview) → mock.
- Reuse the existing renderer screens unchanged (they only call `api.*`). Only the provider changes.
- Security caveats to document: browser-stored keys are weaker than OS keychain; consider read-only mode by
  default and require an explicit passphrase to unlock signing.

Until then, the hosted web app is a **live dashboard + node/staking/inference console** with the desktop UI;
add the browser wallet provider to make send/receive/stake-signing work in-browser too.

Optional niceties: mDNS auto-discovery (LAN), `KVR_PUBLIC_URL` auto-fill in the wallets from `/api/config`,
multi-node/world-map polish.

## 8. Continuation checklist (for the next session)

- [ ] On the Linux box: clone branch `tony`, `cd solana/staking-service`.
- [ ] Put `secrets/admin.json` (devnet key) + set `KVR_PUBLIC_URL` in `.env`.
- [ ] `docker compose up -d --build`; verify `/health`, `/api/config`, and the web UI at `/`.
- [ ] (Recommended) Caddy/nginx TLS in front; update `KVR_PUBLIC_URL`.
- [ ] Point one wallet at the public URL; register a node; confirm node status + rewards.
- [ ] Implement the **browser wallet provider** (§7) for full web parity; rebuild the image (web stage
      picks it up automatically).
- [ ] `git push fork tony`.
