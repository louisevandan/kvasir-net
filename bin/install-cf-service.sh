#!/usr/bin/env bash
# Install the linkcpp-gw Cloudflare Tunnel as a systemd service (survives reboot).
# Run with sudo:   sudo /home/kvasir/linkcpp/bin/install-cf-service.sh
# Puts config + credentials in /etc/cloudflared (root-owned, where the root service
# looks), stops any nohup-run instance, then installs + starts the service.
set -euo pipefail
CF=/home/kvasir/linkcpp/bin/cloudflared
UUID=c9c2de1d-fc91-44bd-a2ae-8a883c37f1a3
SRC=/home/kvasir/.cloudflared/${UUID}.json

[ "$(id -u)" -eq 0 ] || { echo "run with sudo"; exit 1; }
[ -f "$SRC" ] || { echo "missing tunnel credentials: $SRC"; exit 1; }

mkdir -p /etc/cloudflared
cp -f "$SRC" /etc/cloudflared/${UUID}.json
cat > /etc/cloudflared/config.yml <<YAML
tunnel: linkcpp-gw
credentials-file: /etc/cloudflared/${UUID}.json
ingress:
  - hostname: gate.kvasir-ai.net
    service: http://localhost:5173
  - service: http_status:404
YAML
echo "wrote /etc/cloudflared/config.yml"

# stop the temporary nohup-run tunnel so only the service runs it
pkill -f 'cloudflared tunnel run linkcpp-gw' 2>/dev/null || true
sleep 1

"$CF" service install || true
systemctl enable --now cloudflared
sleep 2
echo "=== service status ==="
systemctl status cloudflared --no-pager -l | head -15
