# Kvasir gateway (staking / node-rewards / inference settlement) + bundled web UI.
#
# Build context MUST be the repo root so both the web app and the token spec are reachable:
#   docker build -f solana/staking-service/Dockerfile -t kvasir-gateway .
# or via compose:
#   cd solana/staking-service && docker compose up -d --build
#
# The web UI served at / is the SAME React app as the desktop wallet (wallet/desktop),
# built for the browser. The gateway also exposes the /api/* + /health endpoints.

# --- stage 1: build the web UI (identical to the desktop app) ---------------
FROM node:20-alpine AS web
# electron is a dev dep of the desktop app but is NOT needed for the web build;
# skip its ~200MB binary download.
ENV ELECTRON_SKIP_BINARY_DOWNLOAD=1
WORKDIR /web
COPY wallet/desktop/package.json wallet/desktop/package-lock.json* ./
RUN npm install --no-audit --no-fund
COPY wallet/desktop/ ./
RUN npm run build:web

# --- stage 2: gateway server + bundled web UI -------------------------------
FROM node:20-alpine AS gateway
WORKDIR /app
COPY solana/staking-service/package.json solana/staking-service/package-lock.json* ./
RUN npm install --omit=dev --no-audit --no-fund
COPY solana/staking-service/server.js solana/staking-service/gatewayAuth.js ./
# token spec (public source of truth) baked as a default; override with a mount + KVR_TOKEN_SPEC
COPY wallet/shared-spec/token.devnet.json /app/spec/token.devnet.json
# web UI bundle from stage 1
COPY --from=web /web/dist /app/web

ENV PORT=8791 \
    KVR_TOKEN_SPEC=/app/spec/token.devnet.json \
    KVR_WEB_DIR=/app/web \
    KVR_ADMIN_KEY_FILE=/run/secrets/admin.json

RUN addgroup -S kvasir && adduser -S kvasir -G kvasir \
 && mkdir -p /app/data && chown -R kvasir:kvasir /app
USER kvasir

EXPOSE 8791
VOLUME ["/app/data"]
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s \
  CMD wget -qO- http://127.0.0.1:8791/health || exit 1
CMD ["node", "server.js"]
