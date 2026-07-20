#!/usr/bin/env bash
# Run a native managed node-agent on Linux or macOS.
# Usage:
#   scripts/run-node-agent.sh [auto|cuda|metal|vulkan|cpu]
# The hub links this agent from its controller Node tab; no hub URL is needed
# to start the local agent.
set -euo pipefail
cd "$(dirname "$0")/.."

backend="${1:-${LINKCPP_LLAMA_CPP_BACKEND:-auto}}"
system="$(uname -s | tr '[:upper:]' '[:lower:]')"

if [[ "$backend" == "auto" ]]; then
  if [[ "$system" == "darwin" ]]; then
    backend="metal"
  elif command -v nvidia-smi >/dev/null 2>&1; then
    backend="cuda"
  else
    backend="cpu"
  fi
fi

build_dir="${LINKCPP_NODE_BUILD_DIR:-build-node-${system}-${backend}}"
if [[ "$build_dir" = /* ]]; then
  build_root="$build_dir"
else
  build_root="${PWD}/${build_dir}"
fi
rpc_bin="${RPC_BIN:-${build_root}/bin/ggml-rpc-server}"
venv="${LINKCPP_NODE_VENV:-.venv-linkcpp-node}"
if [[ "${LINKCPP_SKIP_VENV:-0}" != "1" ]]; then
  python3 -m venv "$venv"
  # shellcheck disable=SC1091
  source "$venv/bin/activate"
  python -m pip install --upgrade pip >/dev/null
  python -m pip install fastapi "uvicorn[standard]" httpx gguf numpy python-multipart >/dev/null
  # Native macOS machines often have only Xcode's compiler.  Keep the managed
  # node self-contained when it must build the paired proxy runtime.
  if ! command -v cmake >/dev/null 2>&1 || ! command -v ninja >/dev/null 2>&1; then
    python -m pip install cmake ninja >/dev/null
  fi
else
  python_bin="${PYTHON:-python3}"
fi

stage_bin="${LINKCPP_STAGE_BIN:-${build_root}/apps/linkcpp-node/linkcpp-node}"
server_bin="${LINKCPP_SERVER_BIN:-${build_root}/apps/linkcpp-server/linkcpp-server}"
if [[ ! -x "$rpc_bin" || ! -x "$stage_bin" || ! -x "$server_bin" ]]; then
  # RPC_BIN is commonly a fixed LaunchAgent path into the generated build
  # directory.  It is expected to be absent after a stale cache reset, so it
  # must trigger a rebuild rather than preventing one.
  bash scripts/build-node-runtime.sh "$backend"
fi

export PYTHONPATH="${PWD}${PYTHONPATH:+:${PYTHONPATH}}"
export RPC_BIN="$rpc_bin"
export LINKCPP_STAGE_BIN="$stage_bin"
export LINKCPP_SERVER_BIN="$server_bin"
export LINKCPP_LLAMA_CPP_BACKEND="$backend"
export LINKCPP_MODEL_DIR="${LINKCPP_MODEL_DIR:-${PWD}/models}"
export LINKCPP_RPC_CACHE="${LINKCPP_RPC_CACHE:-${HOME}/.cache/linkcpp/rpc}"
export LINKCPP_RPC_PORT="${LINKCPP_RPC_PORT:-50052}"
# Wallet that owns this node (earns its inference-contribution rewards). Optional.
export LINKCPP_NODE_OWNER="${LINKCPP_NODE_OWNER:-}"

host="${LINKCPP_NODE_HOST:-0.0.0.0}"
port="${LINKCPP_NODE_AGENT_PORT:-9101}"
# Multiple logical slots on one host must not share persisted identity or logs.
export LINKCPP_NODE_STATE="${LINKCPP_NODE_STATE:-${HOME}/.cache/linkcpp/node-${port}.json}"
export LINKCPP_WORKER_LOG="${LINKCPP_WORKER_LOG:-${HOME}/.cache/linkcpp/worker-${port}.log}"

mkdir -p "$LINKCPP_MODEL_DIR" "$(dirname "$LINKCPP_NODE_STATE")" "$(dirname "$LINKCPP_WORKER_LOG")" "$LINKCPP_RPC_CACHE"

echo "node agent backend=${backend} rpc=${RPC_BIN} api=http://${host}:${port} rpc_port=${LINKCPP_RPC_PORT}"

if [[ "${LINKCPP_SKIP_VENV:-0}" == "1" ]]; then
  exec "$python_bin" -m uvicorn controller.nodeagent:app --host "$host" --port "$port"
fi
exec python -m uvicorn controller.nodeagent:app --host "$host" --port "$port"
