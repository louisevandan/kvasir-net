// linkcpp devnet token configuration.
//
// These values define the SPL token used by linkcpp for inference payment,
// node staking/rewards, and governance. Edit here, then run `npm run create-mint`.
// The resulting mint address is written to ../../wallet/shared-spec/token.devnet.json,
// which is the single source of truth shared by the iOS and Android wallets.

'use strict';

module.exports = {
  cluster: 'devnet',
  rpcUrl: process.env.LINKCPP_RPC_URL || 'https://api.devnet.solana.com',

  token: {
    name: 'Kvasir',
    symbol: 'KVR',
    // 6 decimals: matches USDC and is ample precision for a payment/utility token.
    decimals: 6,
    // Whole tokens minted to the treasury on creation. Actual on-chain base units
    // = initialSupply * 10^decimals.
    initialSupply: 1_000_000_000,
    // Off-chain metadata JSON URI (name/symbol/image). Optional; used by
    // add-metadata.js. Leave empty to skip on-chain Metaplex metadata for now.
    metadataUri: '',
  },

  // Local keypair store (gitignored). The admin key is payer + mint authority +
  // freeze authority + treasury owner for the devnet phase.
  keysDir: '.keys',
  adminKeyFile: 'admin.json',

  // Minimum admin SOL balance (in SOL) before create-mint; tops up via airdrop.
  minAdminSol: 1,
};
