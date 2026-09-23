# Deploying a Kvasir gate host

What it takes to bring up a public Kvasir entrance: the p4 ring that runs the
model, the bridge that puts an HTTP face on it, and the wallet / settlement
service in front. Written from the working MI250 deployment.

## What runs where

```
browser / Electron app / mobile node
        │
        ▼  Cloudflare Tunnel (only public path)
┌────────────────────────────────────────────────────────────────┐
│ gate.kvasir-ai.net → :8791  kvasir-gateway                     │
│     wallet UI + /api/node/*, /api/stake, /api/pay/*,           │
│     /api/credits/*, /api/config                                │
│                    │ X-Kvasir-Service-Token                    │
│                    ▼                                           │
│                      :19000  p4 bridge   ← loopback only       │
│     /api/controllers, /api/runtime, /api/contributions,        │
│     /c/<model>/v1/chat/completions                             │
│                    │ p4 events (P4E3) to each agent's          │
│                    ▼ advertised address                        │
│      p4 agents ──► stage servers ──► the model                 │
└────────────────────────────────────────────────────────────────┘
```

The bridge is the only thing the gateway talks to, and it speaks one contract:
which models are loaded, who contributed how much, and completions. It never
decides placement — see step 2.

**The bridge must not be published directly.** It runs inference on the ring
for anyone who reaches it, gated only by a service token. Bind it to loopback
and let the tunnel be the door.

## Prerequisites

- ROCm (or CUDA) on the compute hosts, and a GGUF of the model on each host
  that will hold stages.
- A Cloudflare Tunnel credentials file for the zone, and a DNS record for
  `gate.kvasir-ai.net` pointing at that tunnel.
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

## 1. p4 agents (one per compute host)

```bash
p4-agent 0.0.0.0:42011 tcp://10.10.10.111:42011
```

The two arguments are the listen address and the address the agent **advertises
to other agents**. They are not interchangeable:

- A stage hands its result to the next stage by asking its own agent to dial
  that stage's agent at the advertised address
  (`entrypoints/agent/src/event_runtime/transport.rs`). Advertise `127.0.0.1`
  and a two-host ring dials itself; nothing arrives and nothing says why.
- Advertise the fastest network the hosts share. On the MI250 rack that is the
  InfiniBand link (`10.10.10.0/24`, ~0.14 ms), not the office LAN.

**The operational journal is mandatory.** A model LOAD is refused outright
without it (`operational journal is required before model LOAD`):

```bash
P4_EVENT_OUTBOUND_JOURNAL_DIR=$HOME/p4-journal p4-agent 0.0.0.0:42011 tcp://10.10.10.111:42011
```

The agent and the native stage server are one release. An agent built from a
tree newer than the installed `p4_staged_server` fails at READY with
`stage server HELLO omits <capability>` — after loading the whole model. Build
both from the same checkout.

## 2. Placement plan and load

Placement is an **operator artifact**, not something the gateway or the bridge
can ask for: the bridge answers `409` to `/api/controllers/<id>/serve`. Write a
plan, load it, and the catalog then names what is serving.

```bash
cd p4bridge
node make-plan.mjs                                   # writes load-plan.<model>.json
node load.mjs --plan load-plan.<model>.json --dry-run
node load.mjs --plan load-plan.<model>.json --confirm
```

Four things in a plan are easy to get wrong and expensive to discover:

- **The node generation and the load generation are one number.** The adapter
  compares a RELEASE receipt's source node generation against its
  `load_generation` and stops the node when they differ. A ring loaded with two
  different numbers serves exactly one request and then loses its head; the next
  session hangs at `3/4 stages ready`. `load.mjs` refuses the mismatch before
  loading. Use a fresh value per load — reusing one collides with a node
  registration a failed attempt left behind (`StaleNode`).
- **`n_batch` sets the physical result bound**, which the adapter compares for
  exact equality against the stage server's READY report, and which must fit the
  agent's 256 MiB retained stores. The bound grows with `n_batch × n_ubatch`; at
  2048 × 512 it is 34 GB and no load can ever be admitted. 128 rows is the
  production shape.
- **The device name is backend-specific.** The HIP build's ggml names its
  devices `ROCm0`, not `CUDA0`. A CUDA plan loads the entire model and only then
  fails to find the device.
- **The chat template is OUTER's job.** p4 hands the stage server an opaque
  prompt and applies no template. State the model's turn format in the plan
  (`prompt_format`, `reasoning`) or the model will continue your text instead of
  answering it, and never emit its end-of-turn token.

`load.mjs` writes the generation to `state/last-load.json` **before** the first
LOAD, because it is the only way back: UNLOAD needs the same number and it
exists nowhere on the machines.

## 3. p4 bridge

```bash
cd p4bridge
P4_BRIDGE_PORT=19000 \
P4_BRIDGE_HOST=127.0.0.1 \
P4_BRIDGE_TOKEN=<openssl rand -base64 32> \
P4_BRIDGE_OPERATOR_WALLET=<wallet credited for these machines> \
node server.js
```

- `P4_BRIDGE_TOKEN` is not optional in a deployment. Unset, the bridge
  authenticates nobody and announces so at startup.
- `P4_BRIDGE_OPERATOR_WALLET` is what makes contribution count. The gateway
  skips any contribution row with an empty owner **silently**, which reads as
  "the nodes earned nothing" rather than "the wallet is unset".
- Contribution counters are in memory. A bridge restart loses whatever the
  gateway had not yet polled (default every 30 s).

## 4. Wallet / settlement service (gate)

```bash
mkdir -p solana/staking-service/secrets && chmod 700 solana/staking-service/secrets
cp /secure/admin.json solana/staking-service/secrets/admin.json
chmod 644 solana/staking-service/secrets/admin.json   # see note

cat > solana/staking-service/.env <<'EOF'
GATEWAY_PORT=8791
P4_BRIDGE_URL=http://127.0.0.1:19000
P4_BRIDGE_TOKEN=<same value as step 3>
KVR_ADMIN_WALLETS=<governance wallet>
KVR_SESSION_SECRET=<openssl rand -hex 32>
KVR_GATEWAY_OWNER=<wallet credited for this host's uptime>
KVR_BRIDGE_OWNER=<same or another wallet>
KVR_PUBLIC_URL=https://gate.kvasir-ai.net
KVR_RPC_URL=https://api.devnet.solana.com
KVR_CREDIT_OPEN_REGISTER=1
KVR_CREDIT_WHITELIST=
KVR_CREDIT_MIN_BALANCE=0
EOF
cd solana/staking-service && docker compose up -d --build gateway
```

- `P4_BRIDGE_URL` must be reachable **from inside the gateway container**. A
  bridge on the same host is not at `127.0.0.1` there; publish it to the docker
  bridge address or give the container `host.docker.internal` via
  `extra_hosts`. `fetchModelsFromBridge()` swallows connection errors and
  returns `[]`, so an unreachable bridge shows up only as an empty model list
  and nothing in any log.
- `P4_BRIDGE_TOKEN` must match step 3 exactly, or the model list and the
  contribution poll both 401 — again silently.
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

## 5. Tunnel

```yaml
# /etc/cloudflared/config.yml
tunnel: <tunnel-id>
credentials-file: /path/to/<tunnel-id>.json

ingress:
  - hostname: gate.kvasir-ai.net
    service: http://localhost:8791
  - service: http_status:404
```

```bash
sudo cloudflared --config /etc/cloudflared/config.yml service install
sudo systemctl enable --now cloudflared
```

Only `gate` is published. When the bridge lives on a different machine from the
gateway, give it its own hostname and keep the service token on it; do not move
the bridge to a public bind.

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
# on the bridge host
curl -s http://127.0.0.1:19000/api/health                    # service:"p4-bridge", serving_models >= 1
curl -s -H "X-Kvasir-Service-Token: $P4_BRIDGE_TOKEN" \
     http://127.0.0.1:19000/api/controllers                  # serving:true, every stage "loaded"
curl -s -o /dev/null -w '%{http_code}\n' \
     http://127.0.0.1:19000/api/controllers                  # 401 without the token — correct

curl -s https://gate.kvasir-ai.net/api/config                # mint/vault must match the spec
curl -s https://gate.kvasir-ai.net/api/pay/models            # non-empty once a model is serving

# from outside the host: must fail
curl -s --max-time 5 http://<public-ip>:19000/api/health
```

An empty `models` array with a healthy gate almost always means the gateway
cannot reach the bridge, or the two service tokens differ. Check from inside the
container rather than the host:

```bash
docker exec kvasir-gateway sh -c \
  'wget -qO- --header="X-Kvasir-Service-Token: $P4_BRIDGE_TOKEN" \
   "$P4_BRIDGE_URL/api/controllers"'
```

`contributedUnits: 0` on nodes that clearly ran work means
`P4_BRIDGE_OPERATOR_WALLET` is unset: the gateway drops contribution rows with
no owner without saying so.

## Secrets checklist

Never committed — `.env` and `secrets/` are gitignored. Each must be provisioned
per host:

| Secret | Used by | Lost ⇒ |
| --- | --- | --- |
| `admin.json` treasury key | wallet service | mint/treasury authority gone, unrecoverable |
| `P4_BRIDGE_TOKEN` | bridge + wallet service | model list and settlement poll 401 |
| `KVR_SESSION_SECRET` | wallet service | admin sessions drop |
| tunnel credentials JSON | cloudflared | no public entrance |

The `gateway-data` volume is not a secret but is equally unrecoverable: it is
the settlement ledger. `p4bridge/state/last-load.json` is the same kind of
thing for the ring: without the load generation the model cannot be unloaded.
