import { defineConfig } from 'vite'
import { nodePolyfills } from 'vite-plugin-node-polyfills'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'

const here = dirname(fileURLToPath(import.meta.url))

// Bundle the hub wallet to a single self-contained browser IIFE exposing
// window.LinkcppHubWallet, dropped into the hub UI's static vendor dir.
export default defineConfig({
  plugins: [nodePolyfills({ globals: { Buffer: true, process: true, global: true } })],
  build: {
    lib: {
      entry: resolve(here, 'entry.js'),
      name: 'LinkcppHubWallet',
      formats: ['iife'],
      fileName: () => 'hub-wallet.js',
    },
    outDir: resolve(here, '../../../controller/web/vendor'),
    emptyOutDir: false,
    minify: true,
  },
})
