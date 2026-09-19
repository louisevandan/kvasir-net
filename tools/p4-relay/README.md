# kvasir-p4-relay

Gives a p4 node a dialable address it has not got.

```
protocol.mjs  → the frames, and how they are parsed off a TCP stream
auth.mjs      → proving which wallet is on the other end
relay.mjs     → the public side: one address per node, bytes forwarded
tunnel.mjs    → the node side: one outbound connection, many p4 sockets
e2e-test.mjs  → a megabyte of random bytes, three callers, one refusal
```

## Why this has to exist

p4 reaches a node by opening a TCP connection to the address in the envelope.
There is no route table, no rendezvous, no hole punching — `deliver_outbound`
dials `(address.host, address.port)` and writes. Worse, a connection an agent
opened itself is receipts-only: a `Data` frame arriving on it is answered with
`peer_closed("unexpected frame on outbound hop")`. So a machine behind NAT
cannot open a tunnel outward and be handed work down it. It cannot join at all.

That is not a gap to be filled by trying harder on the node. It is the shape of
the protocol, and the only way to change it inside p4 is to make an agent accept
inbound work on an outbound connection — an engine change we do not control.

The relay is the way round that needs no engine change. It holds a public
address; the node keeps one outbound connection to it; when somebody dials the
public address the relay carries those bytes down the connection the node
already has. Both ends of p4 see an ordinary socket to an ordinary address.

## It is also the authentication boundary, and that is the point

p4 has no authentication of any kind. No TLS, no tokens, no allowlist — any host
that can reach an agent's port may send `NODE_LOAD`, `NODE_UNLOAD` and `INSPECT`
(verified: `Cargo.lock` contains no TLS crate, and `serve()` performs no peer
check). Asking a contributor to port-forward their home machine into that would
be indefensible.

Here the node listens on nothing. It holds one outbound connection, proves a
wallet before that connection carries anything, and the relay decides who may
dial in. Reachability and authentication have the same answer, which is the
main argument for solving it this way rather than with port forwarding.

## What it refuses to be

- **It does not read p4.** Payloads are forwarded byte for byte, never parsed.
  The relay cannot tell a LOAD from an INSPECT and must not be able to.
- **It is not a scheduler.** A node is an address here and nothing else.
- **It is not proof of work.** It knows which wallet opened a tunnel, which is
  worth recording; bytes through a relay are not evidence of inference and must
  never be presented as a contribution measure.

## Running it

```sh
npm install
KVASIR_RELAY_HOST=<the public address nodes should advertise> \
KVASIR_RELAY_ALLOW_FROM=<comma-separated callers> \
node relay.mjs
```

| variable | default | meaning |
| --- | --- | --- |
| `KVASIR_RELAY_PORT` | 43000 | where nodes connect |
| `KVASIR_RELAY_BIND` | 0.0.0.0 | interface to bind |
| `KVASIR_RELAY_HOST` | 127.0.0.1 | the address handed to nodes to advertise |
| `KVASIR_RELAY_PORT_FROM/TO` | 43100–43199 | public ports, one per node |
| `KVASIR_RELAY_ALLOW_FROM` | *(empty)* | who may dial a node's port. **Empty means anyone** |
| `KVASIR_RELAY_IDLE_MS` | 90000 | drop a tunnel that has gone quiet |

Leave `KVASIR_RELAY_ALLOW_FROM` empty only on a private network. On a public
address it is the difference between "the OUTER can reach our nodes" and
"anyone on the internet can send NODE_LOAD to a contributor's machine".

## Where it runs

`mobile-coder-vm` in GCP `banya2025`, zone `asia-northeast3-a`, public
`34.50.62.159`. Installed entirely under the user's home — `~/.local/node` and
`~/kvasir-p4-relay` — so nothing system-wide changed on a machine that already
serves other things.

Inbound is admitted by a firewall rule of its own, `kvasir-p4-relay`, targeting
a tag of the same name rather than the instance's existing `banya-agent` tag.
Removing the tag removes the relay's exposure and leaves the rest of that
instance untouched. Sources are named addresses, never `0.0.0.0/0`.

```
rule    kvasir-p4-relay   tcp:43000, tcp:43100-43199
target  tag kvasir-p4-relay  (mobile-coder-vm)
source  the office, the MI250 egress, and an operator machine
```

Verified end to end on 2026-09-20 with the real topology: a Mac behind NAT
registered as a node and became reachable at `tcp://34.50.62.159:43100`, and
MI250-02 dialled that address and got 256 KiB back byte-identical in 89 ms.

### Hosts that cannot do this, and why

| host | verdict |
| --- | --- |
| MI250-01 / MI250-02 | behind NAT, LAN `192.168.20.x`. A relay there would be caught by the problem it exists to solve — the same reason the hub needs a cloudflared tunnel |
| GB10 #1 (office) | has a directly attached public address, but only port 22 is admitted. `80`, `443`, `8080`, `19001` and `43000` are all filtered above the host, so a dial never reaches the process. Opening it needs someone at the router |

## What it does not do yet

- Register the tunnel with the gateway, so `/api/node/register` knows the
  address a node is reachable at.
- Gate registration on the operator's KVR balance. `MIN_OPERATOR_KVR` lives in
  the gateway; the relay proves the wallet and should ask the gateway whether
  that wallet may run a node.
- Run under a supervisor. It is started by hand and will not survive a reboot
  of the instance.
