# kvasir-node

A headless Kvasir expert node for Linux servers.

It lends GPU memory to host MoE expert shards: it signs in to the bridge with
your wallet key, takes assignments from the expert market, downloads each
shard, runs `linkcpp-expert-worker` on it, and reports what it holds. Rewards
are credited to the key's wallet.

It is the desktop app's node without the app. The market, shard and worker
logic is required from `wallet/desktop/electron/` (`participation.cjs`,
`expertHost.cjs`, `executors.cjs`), not copied, so servers and desktops run the
same code.

## Requirements

- Node.js 18 or newer
- An NVIDIA GPU and a `linkcpp-expert-worker` built for it, from
  `apps/linkcpp-expert-worker`:

  ```sh
  cmake -S . -B build/worker -DCMAKE_BUILD_TYPE=Release \
    -DLINKCPP_EXPERT_WORKER_ONLY=ON -DGGML_CUDA=ON -DCMAKE_CUDA_ARCHITECTURES=native
  cmake --build build/worker --target linkcpp-expert-worker
  ```

- A checkout of this repository. The CLI needs the `wallet/desktop/electron`
  sources, but not the desktop app's dependencies.

## Setup

```sh
cd node-cli && npm install          # ws and tweetnacl, nothing else

# A key for the node. Rewards go to its address. An existing Solana CLI
# keypair (what `solana-keygen new` writes) works too; keep it mode 600.
node kvasir-node.cjs keygen --out ~/.config/kvasir/node-key.json
```

The key is read from a file on purpose. A key or mnemonic on the command line
ends up in shell history and in `ps` output that every user can read. A key
file that other users can read is refused.

## Run

```sh
node kvasir-node.cjs run \
  --key ~/.config/kvasir/node-key.json \
  --worker /opt/kvasir/linkcpp-expert-worker \
  --budget 16
```

| flag | |
|---|---|
| `--key` | Keypair file (JSON array of 64 bytes), mode 600. Required. |
| `--budget` | GPU memory to lend, in GiB. Defaults to half of what the GPU reports. **Required on unified-memory machines** (GB10, Grace), where the GPU memory is the host's RAM and there is no card total to take half of. |
| `--worker` | The worker binary, or set `KVASIR_EXPERT_WORKER`. |
| `--name` | This machine's part of the node id. Defaults to the hostname. Two servers on one wallet need different names. |
| `--gateway` | Defaults to `https://gate.kvasir-ai.net`. |
| `--data` | Shards and the node token. Defaults to `~/.local/share/kvasir-node`. |

The node id is `server-<first 8 of the wallet>-<name>`. A budget larger than one
shard holds several shards as slots: one worker process each, with ids `<id>`,
`<id>-2`, and so on, all paying the same wallet. The number of slots is capped
at 8.

Stop it with SIGTERM or Ctrl-C. It stops every worker and leaves the market.

## As a service

`kvasir-node.service` is an example systemd unit. Copy it to
`/etc/systemd/system/`, adjust the user, paths and budget, then
`systemctl enable --now kvasir-node`. Keep `Restart=on-failure`: the bridge
drops a node from the market after 120 s without a heartbeat.

## A shared GPU

The budget is a promise to the rest of the machine. If the GPU also serves
something else (a vLLM server, say), set `--budget` to what it can spare. On
unified memory, the expert worker and the other service draw from the same
RAM, and nothing isolates them from each other.

## Tests

```sh
npm test
```

The tests drive a whole node (key file, sign-in, two slots, per-slot coverage)
against a loopback bridge with a fake worker.
