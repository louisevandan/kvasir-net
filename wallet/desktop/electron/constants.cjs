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
  stakingServiceUrl: 'https://kvr.prototypebench.org',
  defaultCluster: 'devnet',
}
