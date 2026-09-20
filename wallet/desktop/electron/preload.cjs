'use strict'
const { contextBridge, ipcRenderer } = require('electron')

// Typed bridge exposed to the renderer as window.kvasir. Keys never leave main;
// the renderer only asks main to derive/sign. HTTP (staking/gateway) is done in
// the renderer directly via fetch.
contextBridge.exposeInMainWorld('kvasir', {
  isElectron: true,
  wallet: {
    has: () => ipcRenderer.invoke('wallet:has'),
    state: () => ipcRenderer.invoke('wallet:state'),
    create: (wordCount, passphrase) => ipcRenderer.invoke('wallet:create', wordCount, passphrase),
    preview: (wordCount) => ipcRenderer.invoke('wallet:preview', wordCount),
    commit: (mnemonic, passphrase) => ipcRenderer.invoke('wallet:commit', mnemonic, passphrase),
    import: (mnemonic, passphrase) => ipcRenderer.invoke('wallet:import', mnemonic, passphrase),
    unlock: (passphrase) => ipcRenderer.invoke('wallet:unlock', passphrase),
    lock: () => ipcRenderer.invoke('wallet:lock'),
    upgrade: (passphrase) => ipcRenderer.invoke('wallet:upgrade', passphrase),
    changePassphrase: (oldPass, newPass) => ipcRenderer.invoke('wallet:changePassphrase', oldPass, newPass),
    address: () => ipcRenderer.invoke('wallet:address'),
    mnemonic: () => ipcRenderer.invoke('wallet:mnemonic'),
    signMessage: (message) => ipcRenderer.invoke('wallet:signMessage', message),
    clear: () => ipcRenderer.invoke('wallet:clear'),
  },
  config: {
    get: () => ipcRenderer.invoke('config:get'),
    set: (patch) => ipcRenderer.invoke('config:set', patch),
  },
  solana: {
    balances: (network) => ipcRenderer.invoke('solana:balances', network),
    history: (network) => ipcRenderer.invoke('solana:history', network),
    sendSol: (args) => ipcRenderer.invoke('solana:sendSol', args),
    sendToken: (args) => ipcRenderer.invoke('solana:sendToken', args),
  },
  meta: () => ipcRenderer.invoke('meta'),
  gateway: {
    status: () => ipcRenderer.invoke('gateway:status'),
    start: () => ipcRenderer.invoke('gateway:start'),
    stop: () => ipcRenderer.invoke('gateway:stop'),
    setDir: (dir) => ipcRenderer.invoke('gateway:setDir', dir),
    setKeyFile: (file) => ipcRenderer.invoke('gateway:setKeyFile', file),
    pickKey: () => ipcRenderer.invoke('gateway:pickKey'),
    adminFetch: (url, init) => ipcRenderer.invoke('gateway:adminFetch', url, init),
  },
  models: {
    list: () => ipcRenderer.invoke('models:list'),
    remove: (name) => ipcRenderer.invoke('models:delete', name),
    dir: () => ipcRenderer.invoke('models:dir'),
    generate: (name, prompt, maxTokens) => ipcRenderer.invoke('models:generate', { name, prompt, maxTokens }),
  },
  // This machine as a p4 node: a supervised agent process, its real capability,
  // and a throughput number only after a run that actually produced one.
  node: {
    status: (opts) => ipcRenderer.invoke('node:status', opts),
    start: () => ipcRenderer.invoke('node:start'),
    stop: () => ipcRenderer.invoke('node:stop'),
    capability: (refresh) => ipcRenderer.invoke('node:capability', refresh),
    benchmark: (maxTokens) => ipcRenderer.invoke('node:benchmark', maxTokens),
  },
  openExternal: (url) => ipcRenderer.invoke('shell:open', url),
  revealPath: (p) => ipcRenderer.invoke('shell:reveal', p),
})
