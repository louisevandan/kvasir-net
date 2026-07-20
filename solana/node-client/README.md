# linkcpp node client

Connect any compute device (desktop / laptop) to your **linkcpp account** (your
wallet address) so it shows up in the wallet app's node monitoring and earns
node-operator rewards.

Zero dependencies — needs only **Node.js 18+** (uses the built-in `fetch`).

## Usage

Copy your **account address** from the wallet app (기기 연결 화면), then on each
device run:

```bash
LINKCPP_SERVICE=http://<mac-ip>:8791 \
LINKCPP_OWNER=<your_account_pubkey> \
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
| `accelerator` | `gpu` if `nvidia-smi` present or macOS, else `cpu` | `LINKCPP_ACCEL=gpu\|cpu\|npu` |
| `nodeId` | hostname | `LINKCPP_NODE_ID` |
| `label` | hostname | `LINKCPP_LABEL` |
| `deviceKind` | `computer` | `LINKCPP_DEVICE_KIND=desktop\|laptop\|tablet\|phone` |

Example (a CUDA workstation labelled explicitly):

```bash
LINKCPP_SERVICE=http://your-gateway-host:8791 LINKCPP_OWNER=<pubkey> \
LINKCPP_DEVICE_KIND=desktop LINKCPP_LABEL="RTX-4090 rig" LINKCPP_ACCEL=gpu \
node connect.js
```

## Notes

- iOS / Android phones connect from the **wallet app itself** ("이 기기 연결"),
  not this client.
- Contribution (inference work) is reported by the linkcpp hub; this client only
  establishes presence (register + heartbeat) for the devnet MVP.
