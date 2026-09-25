# p4-bridge

The HTTP face of the p4 engine.

The settlement gateway (`solana/staking-service`) and the desktop app speak the
settlement gateway's HTTP contract: a controller catalog, `/c/{id}/v1/chat/completions`,
a runtime summary, a contribution ledger. p4 speaks none of it — it is a TCP
event protocol (OUTER) with no HTTP, no model names and no token counts in a
reply. This service is the adapter between the two, so the money path keeps its
shape while the engine underneath changes.

```
gateway ──HTTP──> p4-bridge ──OUTER/TCP──> p4 agents ──> llama.cpp staged nodes
```

## Run

```sh
P4_BRIDGE_CATALOG=./catalog.json \
P4_BRIDGE_PORT=19100 \
P4_BRIDGE_TOKEN=<shared secret> \
P4_BRIDGE_OPERATOR_WALLET=<wallet that earns for these nodes> \
node server.js
```

| variable | meaning |
| --- | --- |
| `P4_BRIDGE_CATALOG` | model → stages map (default `./catalog.json`) |
| `P4_BRIDGE_PORT` | HTTP port (default 19100) |
| `P4_BRIDGE_TOKEN` | shared secret; unset means no auth (loopback only) |
| `P4_BRIDGE_OPERATOR_WALLET` | owner credited in `/api/contributions` |
| `P4_BRIDGE_UNITS_PER_KTOKEN` | contribution units per 1k rows (default 1) |

The gateway points at it with `P4_BRIDGE_URL` and `P4_BRIDGE_TOKEN`.

## When an agent stops accepting

An agent that has leaked all 256 connection slots keeps serving on the
connections it already has and accepts no new ones, silently. Restarting it
kills its stage, so recovery is a full ring reload — the procedure, with the
measurements from the one time it has been run, is in
[RECOVERY.md](./RECOVERY.md).

## The load generation, and a mistake worth not repeating

`load_generation` is set by whoever loaded the model and is compared for **exact
equality** by the adapter. It is not in an agent snapshot and cannot be derived
from anything OUTER can see — a node's own `generation` is a different number.

Getting it wrong is not a harmless rejection. Every stage that refuses a session
records `failed:session load generation is stale` as its reported state, and
nothing sets that string back to `loaded` except loading the model again. The
weights stay resident and a session with the right value would still be
accepted, but every dashboard reading that field — this bridge included — now
says the model is not serving.

That is exactly what a convenient fallback produced here: the catalog defaulted
`load_generation` to the first stage's `generation`, which looked plausible, was
wrong, and put four production stages into that state. The fallback is gone. The
catalog must carry the value from the placement plan, or the model does not
serve.

## Driving a load — and keeping the number

`load.mjs` is the only thing here that changes what is loaded, and it exists so
the generation is never lost again:

```sh
node load.mjs --plan plan.json --dry-run     # print what would be sent
node load.mjs --plan plan.json --confirm     # load, then write the catalog
node load.mjs --unload --confirm             # unload what the record names
```

It writes the generation to `state/last-load.json` **before the first LOAD
leaves**, so a load that fails halfway still leaves you able to unload. The
catalog is only updated after every stage answers `loaded`; a partial load never
gets to claim the model serves.

For a model someone else loaded, `--unload --generation <n>` takes the number
from whoever drove it and reads the stages from the catalog. That is the only
way back for a load driven from another tool — nothing on the machines will
tell you the number.

`--confirm` is required for anything that acts. See `load-plan.example.json`
for the shape of a plan.

## The catalog, and why it exists

An agent snapshot lists node ids, generations and lifecycle state — nothing
else. p4 does not know that `step37-s0..s3` together are "Step-3.7-Flash"; that
mapping is an operator fact, so it lives in `catalog.json` and is re-checked
against a live INSPECT every 15 s. A stage that is missing, in another state or
at another generation takes the model out of the catalog: better an empty
catalog than a model that 500s on its first call.

One connection per agent. An agent answers only for its own nodes — an INSPECT
addressed to a peer times out — so a pipeline that spans machines holds a
connection to each. Replies come back on the connection the request left by,
because the envelope's return route names that connection.

## Endpoints

| method | path | notes |
| --- | --- | --- |
| GET | `/health` | 200 while at least one model is serving; no token needed |
| GET | `/api/controllers` | catalog with live stage verification |
| GET | `/api/runtime` | operator wallet, machines, GPUs, nodes |
| GET | `/api/contributions` | per-node units, rows, requests, decode tok/s |
| GET | `/c/{id}/v1/models` | the one model that controller serves |
| POST | `/c/{id}/v1/chat/completions` | OpenAI chat, JSON or SSE |
| POST | `/api/controllers/{id}/serve\|unload` | **409 `placement_is_external`** |

## What it deliberately does not do

**Load or unload a model.** p4 loads from a placement plan an operator prepares
(`tools/model-loading`) and gates the load on its own integrity checks. Serving
that from an HTTP call would put those gates behind a web request, so the bridge
answers 409 and the gateway reports it instead of retrying.

## Billing numbers

p4 returns no usage block, so the bridge derives one: completion tokens are the
output events it counted, prompt tokens come from `batch-observation-v4`
(`owned_requests[].prefill_rows`). When no observation arrived, prompt tokens are
reported as 0 rather than estimated — the gateway must never bill a guess.

## Tests

```sh
node --test test/
```

The wire tests pin the byte layout against the engine's own encoder: a field in
the wrong order still encodes, and the agent answers by closing the socket.
