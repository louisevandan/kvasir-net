// Chain + service constants shared with the mobile wallets (wallet/shared-spec).
module.exports = {
  clusters: {
    devnet: { rpcUrl: 'https://api.devnet.solana.com', explorerCluster: 'devnet' },
    mainnet: { rpcUrl: 'https://api.mainnet-beta.solana.com', explorerCluster: 'mainnet-beta' },
  },
  derivationPath: "m/44'/501'/0'/0'",
  token: {
    symbol: 'KVR',
    mint: '6cuJAmqtMuGzJ7s7eWQSqfJvEFRUdTiYR3cuMmiNoCPQ',
    decimals: 6,
  },
  treasuryOwner: '8uu2gDKFVtNS79yqYyztJeerEKAh4cnZGdQytCjsYNfF',
  // Genesis gateway — served publicly via Cloudflare Tunnel (HTTPS, no port-forward).
  // Clients still auto-adopt /api/config.publicUrl if it ever changes.
  // Must match wallet/shared-spec/wallet-constants.json (iOS/Android read that
  // file directly; this file is the desktop copy). The retired
  // kvr.prototypebench.org host no longer resolves — while it was still listed
  // here, every gateway-backed screen came up blank: no model list, no credit
  // balance, no API-key minting.
  stakingServiceUrl: 'https://gate.kvasir-ai.net',
  defaultCluster: 'devnet',
  // A p4 node is reached by somebody dialling it, and a machine behind NAT has
  // no address to dial. The relay holds one on its behalf; the app keeps a
  // single outbound connection to it and listens on nothing.
  relay: { host: '34.50.62.159', port: 43000 },
  // Where a machine asks to be admitted to the ring. Separate from the gateway
  // on purpose: it has to answer when the compute fleet does not, since being
  // told "you are not admitted" is exactly the moment you need it.
  admissionsUrl: 'https://reg.kvasir-ai.net',
  // Backbone hosts expert calls can go to, measured directly (not through the
  // public gateway) when this PC joins: see electron/networkPlan.cjs. The probe
  // is a bare TCP handshake to an already-open port — nothing is sent.
  // `serving`: a p4 backbone runs there today. The MI250 hosts are reachable
  // candidates the operator can pick ahead of a backbone being placed on them.
  backbones: [
    { id: 'gb10-1', label: 'GB10-1', host: '1.220.225.155', port: 22, serving: true },
    { id: 'mi250-01', label: 'MI250-01', host: '1.214.116.122', port: 5122, serving: false },
    { id: 'mi250-02', label: 'MI250-02', host: '1.214.116.122', port: 5222, serving: false },
  ],
  // Routing shape and the ring's measured single-stream speed for the model the
  // network serves today (Step-3.7-Flash on the 2-stage GB10 ring, 2026-10-01:
  // 6.7–6.8 tok/s). Used only for the plan estimate.
  expertPlanModel: { model: 'step-3.7-flash', baseTps: 6.7, nExpert: 288, nUsed: 8, expertsPerLayer: 64, nEmbd: 4096, targetTps: 5 },
  // Below this expected speed the node screen asks the operator to connect from a faster network.
  networkWarnTps: 5,
}
