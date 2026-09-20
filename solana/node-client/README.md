# Kvasir node client

Connect any compute device (desktop / laptop) to your **Kvasir account** (your
wallet address) so it shows up in the wallet app's node monitoring and earns
node-operator rewards.

Zero dependencies — needs only **Node.js 18+** (uses the built-in `fetch`).

## Usage

Copy your **account address** from the wallet app, on its device-linking screen, then on each
device run:

```bash
KVR_SERVICE=http://<mac-ip>:8791 \
KVR_OWNER=<your_account_pubkey> \
node connect.js
```

or positionally:

```bash
node connect.js http://<mac-ip>:8791 <your_account_pubkey>
```

It registers the device and sends a heartbeat every 30s so the app shows it as
**online**.

## Detected / overridable fields

| Field | Auto-detected | Override env |
| --- | --- | --- |
| `os` | macOS / Windows / Linux (from the OS) | — |
| `accelerator` | `gpu` if `nvidia-smi` present or macOS, else `cpu` | `KVR_ACCEL=gpu\|cpu\|npu` |
| `nodeId` | hostname | `KVR_NODE_ID` |
| `label` | hostname | `KVR_LABEL` |
| `deviceKind` | `computer` | `KVR_DEVICE_KIND=desktop\|laptop\|tablet\|phone` |

Example (a CUDA workstation labelled explicitly):

```bash
KVR_SERVICE=http://your-gateway-host:8791 KVR_OWNER=<pubkey> \
KVR_DEVICE_KIND=desktop KVR_LABEL="RTX-4090 rig" KVR_ACCEL=gpu \
node connect.js
```

## Notes

- iOS / Android phones connect from the **wallet app itself**, with its "link this device" action,
  not this client.
- Contribution (inference work) is reported by the bridge; this client only
  establishes presence (register + heartbeat) for the devnet MVP.
