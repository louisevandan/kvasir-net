#!/usr/bin/env bash
set -euo pipefail
mode="${1:-rpc}"
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo"
case "$mode" in
    rpc)
        image="${2:-linkcpp-rpc-artifacts:local}"
        docker image inspect "$image" >/dev/null
        LINKCPP_RPC_ARTIFACT_IMAGE="$image" docker compose -f docker-compose.yml -f docker-compose.cuda.yml build hub
        ;;
    proxy)
        image="${2:-linkcpp-proxy-artifacts:local}"
        docker image inspect "$image" >/dev/null
        LINKCPP_PROXY_ARTIFACT_IMAGE="$image" docker compose -f docker-compose.yml -f docker-compose.proxy.yml build hub
        ;;
    *) echo "usage: $0 [rpc|proxy] [artifact-image]" >&2; exit 2 ;;
esac
