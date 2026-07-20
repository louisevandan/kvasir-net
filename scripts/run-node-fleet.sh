#!/usr/bin/env bash
# Launch a FLEET of native ROCm node agents on this multi-GPU host — one agent per
# GPU, each pinned to its own HIP device, RPC port, agent port, state file, and
# (optionally) owner wallet. Used to distribute one model across several nodes so
# each earns its layer-share of the inference contribution.
#
# Usage:
#   scripts/run-node-fleet.sh "<gpu_ids>" "<owners>"
#     gpu_ids : comma-separated HIP device indices, e.g. "0,1,3"
#     owners  : comma-separated wallet per gpu (recycled if fewer than gpus);
#               empty entry => falls back to the hub operator wallet.
# Example (2 GPUs, 2 owners):
#   scripts/run-node-fleet.sh "0,1" "WalletA,WalletB"
#
# Each agent: rpc port 50052+i, agent port 9101+i (i = GPU index), so they never
# collide with the existing slot-0 agent on 50052/9101 (use distinct GPU indices).
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT="$PWD"

GPUS="${1:?comma-separated GPU ids required, e.g. \"0,1,3\"}"
OWNERS="${2:-}"

RENDER_GID="$(getent group render | cut -d: -f3)"
RPC_BIN="${RPC_BIN:-${ROOT}/build-node-linux-hip/bin/ggml-rpc-server}"
[ -x "$RPC_BIN" ] || { echo "rpc bin not found/executable: $RPC_BIN" >&2; exit 2; }

IFS=',' read -r -a GPU_ARR <<< "$GPUS"
IFS=',' read -r -a OWN_ARR <<< "$OWNERS"

hub_url="${LINKCPP_HUB_URL:-http://127.0.0.1:19000}"
model_dir="${LINKCPP_MODEL_DIR:-/opt/zin-ai/models}"
vram_budget="${LINKCPP_VRAM_BUDGET:-64}"
ram_budget="${LINKCPP_RAM_BUDGET:-128}"
cores="${LINKCPP_CORES:-16}"

for idx in "${!GPU_ARR[@]}"; do
  gpu="${GPU_ARR[$idx]}"
  owner="${OWN_ARR[$idx]:-${OWN_ARR[0]:-}}"   # recycle first owner if list is short
  rpc_port=$((50052 + gpu))
  agent_port=$((9101 + gpu))
  state="$HOME/.cache/linkcpp/node-state-gpu${gpu}.json"
  wlog="$HOME/.cache/linkcpp/worker-gpu${gpu}.log"
  cache="$HOME/.cache/linkcpp/rpc-gpu${gpu}"
  mkdir -p "$(dirname "$state")" "$cache"
  echo "launching agent: GPU=$gpu rpc=$rpc_port api=$agent_port owner=${owner:-<hub operator>}"
  # Exports live INSIDE `sg render -c` (matches the known-working single-agent launch,
  # and guarantees the vars survive the group switch). HIP_VISIBLE_DEVICES pins the
  # ggml-rpc-server (spawned by the agent) to one GPU.
  setsid sg render -c "
    cd '$ROOT'
    export PYTHONPATH='$ROOT'
    export HIP_VISIBLE_DEVICES=$gpu   # pin to one GPU (do NOT also set ROCR_VISIBLE_DEVICES — double-filtering hides the device)
    export RPC_BIN='$RPC_BIN' LINKCPP_LLAMA_CPP_BACKEND=rocm
    export LINKCPP_HUB_URL='$hub_url' LINKCPP_MODEL_DIR='$model_dir'
    export LINKCPP_RPC_CACHE='$cache' LINKCPP_NODE_STATE='$state' LINKCPP_WORKER_LOG='$wlog'
    export LINKCPP_RPC_PORT=$rpc_port LINKCPP_NODE_AGENT_PORT=$agent_port
    export LINKCPP_NODE_OWNER='$owner'
    export LINKCPP_VRAM_BUDGET=$vram_budget LINKCPP_RAM_BUDGET=$ram_budget LINKCPP_CORES=$cores
    exec python3 -m uvicorn controller.nodeagent:app --host 0.0.0.0 --port $agent_port
  " > "$HOME/.cache/linkcpp/agent-gpu${gpu}.out" 2>&1 &
  sleep 1
done
echo "fleet launched (${#GPU_ARR[@]} agents). Check: curl -s localhost:<9101+gpu>/status"
