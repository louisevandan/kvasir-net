#!/usr/bin/env bash
# Headless Playwright acceptance test for the single-image hub.
#   bash scripts/acceptance.sh
set -euo pipefail
export MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL='*'
cd "$(dirname "$0")/.."

IMG=mcr.microsoft.com/playwright/python:v1.47.0-jammy
PORT="${LINKCPP_UI_PORT:-19000}"

echo "== hub UI =="
docker run --rm --add-host=host.docker.internal:host-gateway \
  -e "LINKCPP_HUB_URL=http://host.docker.internal:${PORT}" \
  -v "$(pwd)/tests/acceptance:/t" "$IMG" \
  bash -lc "pip install -q playwright==1.47.0 2>/dev/null; python /t/test_hub.py"
