# Kvasir Wallet — Desktop

A non-custodial KVR wallet and distributed-inference node dashboard for the
desktop (Windows x86/x64, macOS, Linux), built with **React + Electron**. Feature
parity with the iOS/Android wallets, laid out as a full-screen dashboard.

## Features
- Non-custodial wallet — BIP39 mnemonic, SLIP-0010 ed25519 at `m/44'/501'/0'/0'`
  (same derivation as the mobile apps and Phantom). The mnemonic is encrypted at
  rest with the OS keystore via Electron `safeStorage` (DPAPI on Windows, Keychain
  on macOS) and only ever decrypted in the main process.
- Devnet/Mainnet switch, KVR + SOL balances, send/receive (QR), history.
- Staking & performance-weighted node-operator rewards (S/A/B/C tiers).
- Node monitor with tier re-scoring (raw → effective contribution).
- **Node settings** for running this PC as an inference node (GPU · CUDA / CPU,
  local-shard vs RPC-worker, resource infographic, live gauges).
- Device connect (account QR + `node connect.js` command for other machines).
- AI inference payment — estimated quote → pay KVR → actual-token usage list.
- **i18n**: 9 languages (한국어, English, 中文, Español, 日本語, Français, Deutsch,
  Nederlands, Bahasa Indonesia), switched live from Settings.

The staking/rewards and inference endpoints are the same `solana/staking-service`
the mobile apps use; on-chain transactions go through `@solana/web3.js` in the
Electron main process.

## Architecture
```
electron/main.cjs      main process — window, safeStorage key mgmt, Solana RPC + signing (IPC)
electron/preload.cjs   contextBridge → window.linkcpp
src/                   React renderer (Vite): dashboard shell + screens + i18n + services
```
The renderer also runs standalone in a browser (`npm run dev:web`) with a mock
bridge, for quick UI iteration.

## Develop
```bash
npm install
npm run dev        # Vite + Electron (hot reload)
npm run dev:web    # renderer only in the browser (mock data)
```

## Build
```bash
npm run build      # build the renderer to dist/
npm run build:win  # Windows NSIS installer, x64 + ia32 (run on Windows, or mac with wine/mono)
npm run build:mac  # macOS
```
Chain/service constants live in `electron/constants.cjs` (mirrors
`wallet/shared-spec`).
