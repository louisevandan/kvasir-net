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
}
