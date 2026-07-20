#!/usr/bin/env bash
# Standard entrypoint for the single-image hub.
#   bash scripts/deploy.sh
set -euo pipefail
export MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL='*'
cd "$(dirname "$0")/.."

PORT="${LINKCPP_UI_PORT:-19000}"

echo "== 1/3 build single image =="
docker compose build hub

echo "== 2/3 start hub =="
docker compose up -d

echo "== 3/3 wait for hub =="
for _ in $(seq 1 60); do
  if curl -sf "http://localhost:${PORT}/api/gpus" >/dev/null 2>&1; then
    echo "hub ready: http://localhost:${PORT}"
    exit 0
  fi
  sleep 2
done

echo "hub did not become ready on http://localhost:${PORT}" >&2
exit 1
