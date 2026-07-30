# Deploying a Kvasir gate host

What it takes to bring up a public Kvasir entrance on a new machine: the wallet
/ settlement service, the linkcpp gateway, and the linker control plane behind
it. Written from the working GB10 deployment.

## What runs where

```
browser / Electron app / mobile node
        │
        ▼  Cloudflare Tunnel (only public path)
┌───────────────────────────────────────────────────────────┐
│ gate.kvasir-ai.net → :8791  kvasir-gateway                │
│     wallet UI + /api/node/*, /api/stake, /api/pay/*,      │
│     /api/credits/*, /api/config                           │
│                    │ x-linkcpp-service-token              │
│ hub.kvasir-ai.net → :19000  linkcpp gateway               │
│     operator auth, settlement view, linker SPA at /linker │
│                    │ delegation (container network)       │
│                      :19001  linker  ← never published    │
│                         └─ ring stages → the model        │
└───────────────────────────────────────────────────────────┘
```

Two hostnames because they are two different APIs with no overlapping paths,
and the wallet apps have `gate.kvasir-ai.net` compiled in. Pointing one name at
the other service produces 404s behind a login wall, not a working app.

**Linker must never be published.** It has no authentication of its own; the
linkcpp gateway is its only client and the thing that authenticates.

## Prerequisites

- Docker with the NVIDIA container runtime (for linker; the gate and gateway
  images are CPU-only).
- A Cloudflare Tunnel credentials file for the zone, and DNS records for both
  hostnames pointing at that tunnel.
- The KVR treasury signing key (`admin.json`, a 64-byte Solana secret key as a
  JSON array). **Not recoverable if lost** — it holds mint, freeze and treasury
  authority for the KVR mint. Verify it before use:

  ```bash
  python3 - <<'PY'
  import json
  d = json.load(open('admin.json')); pub = bytes(d[32:])
  A = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
  n = int.from_bytes(pub, 'big'); s = ''
  while n: n, r = divmod(n, 58); s = A[r] + s
  print(s)   # must equal the vaultOwner in wallet/shared-spec/token.devnet.json
  PY
  ```

## 1. linker (control plane)

From the `convertarchitecture` checkout.

```bash
cat >> solana/../.env <<'EOF'          # its own .env, at the compose root
MODELS_DIR=/path/to/models             # holds the GGUFs AND .kvasir state
LINKER_HOST_PORT=127.0.0.1:19001       # loopback only — see below
LINKCPP_RUNTIME_BASE_IMAGE=nvidia/cuda:13.0.0-runtime-ubuntu22.04
EOF
docker compose up -d --build linker
```

Two settings that will cost you an afternoon if missed:

- `LINKER_HOST_PORT=127.0.0.1:19001`. The compose default publishes on
  `0.0.0.0`, which puts an unauthenticated control plane on the host's public
  IP. The gateway reaches linker over the compose network by service name, so
  no host port is needed at all.
- `LINKCPP_RUNTIME_BASE_IMAGE`. The bundled native artifacts are built against
  CUDA 13; the compose default is 12.8.1, which builds fine and then fails at
  spawn with `linkcpp-node: error while loading shared libraries:
  libcudart.so.13`.

Note the Dockerfile copies `apps/linker/dist` rather than compiling, so run
`npm run build:server` in `apps/linker` before `docker compose build` or your
source change will not be in the image.

**A ring needs at least two stages.** With one node slot the ring's `--next`
points at its own `--listen`, the reset acknowledgement never comes back, and
the stage exits. Configure two slots on the same GPU and split the layers.

## 2. linkcpp gateway (auth + settlement view)

From this checkout.

```bash
cat >> .env <<'EOF'
MODELS_DIR=/path/to/models
LINKCPP_UI_PORT=19000
LINKCPP_LINKER_URL=http://linker:19001
LINKCPP_LINKER_SERVICE_TOKEN=<shared with linker>
LINKCPP_ADMIN_WALLETS=<solana address(es), comma separated>
LINKCPP_SESSION_SECRET=<openssl rand -hex 32>
LINKCPP_SESSION_TTL=300
LINKCPP_HUB_SERVICE_TOKEN=<shared with the wallet service, step 3>
EOF
docker compose up -d --build hub
```

- The gateway is **locked by default** (`LINKCPP_REQUIRE_AUTH`, default on).
  Without `LINKCPP_ADMIN_WALLETS` (or a `LINKCPP_MIN_OPERATOR_KVR` +
  `LINKCPP_KVR_MINT` balance gate) nothing can pass it; it logs that at
  startup rather than letting you discover it at the sign-in prompt.
- `LINKCPP_SESSION_SECRET` is generated per process when unset, which signs
  everyone out on restart. Pin it.
- `LINKCPP_SESSION_TTL` is the idle window in seconds. The UI locks itself and
  the server expires the session together.
- The compose publishes on `127.0.0.1` — the tunnel connects from the host, so
  there is no reason for a second unproxied door.

## 3. Wallet / settlement service (gate)

```bash
mkdir -p solana/staking-service/secrets && chmod 700 solana/staking-service/secrets
cp /secure/admin.json solana/staking-service/secrets/admin.json
chmod 644 solana/staking-service/secrets/admin.json   # see note

cat > solana/staking-service/.env <<'EOF'
GATEWAY_PORT=8791
LINKCPP_HUB_URL=http://hub:9000
LINKCPP_HUB_SERVICE_TOKEN=<same value as step 2>
KVR_ADMIN_WALLETS=<governance wallet>
KVR_SESSION_SECRET=<openssl rand -hex 32>
KVR_GATEWAY_OWNER=<wallet credited for this host's uptime>
KVR_HUB_OWNER=<same or another wallet>
KVR_PUBLIC_URL=https://gate.kvasir-ai.net
LINKCPP_RPC_URL=https://api.devnet.solana.com
KVR_CREDIT_OPEN_REGISTER=1
KVR_CREDIT_WHITELIST=
KVR_CREDIT_MIN_BALANCE=0
EOF
cd solana/staking-service && docker compose up -d --build gateway
```

- `LINKCPP_HUB_URL` is the **container-internal** port (`hub:9000`), not the
  host mapping. This service joins the gateway's compose network; there is no
  `host.docker.internal` in the image, and `fetchModelsFrom()` swallows
  connection errors and returns `[]`, so an unreachable hub shows up only as an
  empty model dropdown with nothing in any log.
- `LINKCPP_HUB_SERVICE_TOKEN` must match step 2 exactly, or the contribution
  poll and the model list both 401 — again silently.
- `admin.json` at mode 644: the container runs as uid 100 and cannot read a
  600 file owned by uid 1000. The parent directories (`~` at 750, `secrets/`
  at 700) are what keep it unreadable on the host; the bind mount is the only
  way in. Passing `KVR_ADMIN_KEY` inline instead exposes the key in
  `docker inspect` and the process environment.
- `KVR_CREDIT_OPEN_REGISTER=1` opens self-registration for API keys. Spending
  is still gated: `/v1/chat/completions` refuses at or below
  `KVR_CREDIT_MIN_BALANCE`, so a key with no deposit cannot infer. Leave the
  whitelist empty for open access, or list wallets to restrict it.
- The settlement ledger lives in the named volume `gateway-data`
  (`/app/data/positions.json`) — stake positions, node registry, credit
  accounts, API key hashes. A host bind mount will not work: the container is
  non-root and a host directory mounts as `root:root`. **Migrating a host means
  moving this volume**, or every staking position and issued API key is gone.

## 4. Tunnel

```yaml
# /etc/cloudflared/config.yml
tunnel: <tunnel-id>
credentials-file: /path/to/<tunnel-id>.json

ingress:
  - hostname: hub.kvasir-ai.net
    service: http://localhost:19000
  - hostname: gate.kvasir-ai.net
    service: http://localhost:8791
  - service: http_status:404
```

```bash
sudo cloudflared --config /etc/cloudflared/config.yml service install
sudo systemctl enable --now cloudflared
```

**Only one machine may run a given tunnel at a time.** Cloudflare load-balances
across every connected connector, so a second `cloudflared` on the same tunnel
ID makes routing non-deterministic — requests for one hostname land on the
other origin and 404. Stop the old host's service before starting the new one,
and check for a stray copy: `ps -eo pid,cmd | grep cloudflared`.

Running the tunnel with only the credentials file is enough. Creating a tunnel
or changing DNS records additionally needs `cert.pem` from `cloudflared tunnel
login`, or a Cloudflare API token with DNS edit rights.

## Verifying

```bash
curl -s https://hub.kvasir-ai.net/health                     # {"ok":true}
curl -s https://hub.kvasir-ai.net/api/auth/status            # enabled:true, idle_ttl
curl -s -o /dev/null -w '%{http_code}\n' \
     https://hub.kvasir-ai.net/api/nodes                     # 401 — locked, correct

curl -s https://gate.kvasir-ai.net/api/config                # mint/vault must match the spec
curl -s https://gate.kvasir-ai.net/api/pay/models            # non-empty once a model is serving

# from outside the host: both must fail
curl -s --max-time 5 http://<public-ip>:19001/health
curl -s --max-time 5 http://<public-ip>:19000/health
```

An empty `models` array with a healthy gate almost always means the wallet
service cannot reach the gateway, or the two service tokens differ. Check from
inside the container rather than the host:

```bash
docker exec kvasir-gateway sh -c \
  'wget -qO- --header="x-linkcpp-service-token: $LINKCPP_HUB_SERVICE_TOKEN" \
   http://hub:9000/api/controllers'
```

## Secrets checklist

Never committed — `.env` and `secrets/` are gitignored. Each must be provisioned
per host:

| Secret | Used by | Lost ⇒ |
| --- | --- | --- |
| `admin.json` treasury key | wallet service | mint/treasury authority gone, unrecoverable |
| `LINKCPP_HUB_SERVICE_TOKEN` | gateway + wallet service | model list and settlement poll 401 |
| `LINKCPP_LINKER_SERVICE_TOKEN` | gateway + linker | every delegated call 401 |
| `LINKCPP_SESSION_SECRET` | gateway | all operator sessions drop |
| `KVR_SESSION_SECRET` | wallet service | admin sessions drop |
| tunnel credentials JSON | cloudflared | no public entrance |

The `gateway-data` volume is not a secret but is equally unrecoverable: it is
the settlement ledger.
