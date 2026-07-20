import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { nodePolyfills } from 'vite-plugin-node-polyfills'

// Renderer is a standalone Vite React app. Electron loads the dev server in dev
// and dist/index.html in production. base './' keeps asset paths file://-safe.
//
// The served web build is also a REAL browser wallet: bip39 + ed25519-hd-key +
// @solana/web3.js run in the browser, which need Node globals (Buffer, process)
// and a few builtins (crypto for ed25519-hd-key's HMAC). nodePolyfills supplies them.
export default defineConfig({
  base: './',
  plugins: [
    react(),
    nodePolyfills({ globals: { Buffer: true, process: true, global: true } }),
  ],
  server: { port: 5173, strictPort: true },
  build: { outDir: 'dist', emptyOutDir: true },
})
