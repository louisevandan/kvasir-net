#!/usr/bin/env bash
# Finish setting up a named Cloudflare Tunnel that serves the Kvasir gateway at
# https://gate.kvasir-ai.net WITHOUT any router port-forward.
#
# Prereqs done by YOU first (accounts/DNS — can't be automated):
#   1. Create/login a Cloudflare account.
#   2. Add the zone `kvasir-ai.net` in Cloudflare; copy the 2 nameservers it gives.
#   3. At GoDaddy, set the domain nameservers to Cloudflare's (wait for propagation).
#   4. On this machine:  bin/cloudflared tunnel login   (authorize kvasir-ai.net)
# Then run this script. It creates the tunnel, writes the ingress config, and
# routes gate.kvasir-ai.net -> the tunnel. Finally start it (printed at the end).
set -euo pipefail
CF="${HOME}/linkcpp/bin/cloudflared"
NAME="linkcpp-gw"
HOST="gate.kvasir-ai.net"
TARGET="http://localhost:5173"
CFDIR="${HOME}/.cloudflared"

if [ ! -f "${CFDIR}/cert.pem" ]; then
  echo "Not logged in yet. Run:  ${CF} tunnel login"
  echo "(authorize the kvasir-ai.net zone), then re-run this script."
  exit 1
fi

if ! "${CF}" tunnel list 2>/dev/null | awk '{print $2}' | grep -qx "${NAME}"; then
  echo "creating tunnel ${NAME}..."
  "${CF}" tunnel create "${NAME}"
fi
UUID="$("${CF}" tunnel list 2>/dev/null | awk -v n="${NAME}" '$2==n{print $1}' | head -1)"
[ -n "${UUID}" ] || { echo "could not resolve tunnel UUID for ${NAME}"; exit 1; }

cat > "${CFDIR}/config.yml" <<YAML
tunnel: ${NAME}
credentials-file: ${CFDIR}/${UUID}.json
ingress:
  - hostname: ${HOST}
    service: ${TARGET}
  - service: http_status:404
YAML
echo "wrote ${CFDIR}/config.yml (ingress ${HOST} -> ${TARGET})"

echo "routing DNS ${HOST} -> tunnel..."
"${CF}" tunnel route dns "${NAME}" "${HOST}" || echo "(route may already exist)"

echo
echo "DONE. Start the tunnel with either:"
echo "  foreground/test:   ${CF} tunnel run ${NAME}"
echo "  persistent svc:    sudo ${CF} service install   # then: sudo systemctl start cloudflared"
