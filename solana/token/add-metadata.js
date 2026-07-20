// OPTIONAL / follow-up: attach on-chain Metaplex Token Metadata (name, symbol,
// logo URI) so block explorers and other wallets display the token nicely.
//
// The linkcpp iOS/Android wallets read name/symbol/decimals from
// wallet/shared-spec/token.devnet.json, so on-chain metadata is NOT required for
// the wallet to function on devnet. This is deferred to keep Phase 0 lean.
//
// To enable later:
//   1. Host a metadata JSON ({name, symbol, description, image}) and set
//      token.metadataUri in config.js.
//   2. npm install @metaplex-foundation/umi-bundle-defaults \
//        @metaplex-foundation/mpl-token-metadata @metaplex-foundation/umi
//   3. Implement createV1/updateV1 against the mint from token.devnet.json using
//      the admin keypair as the update authority.
'use strict';

const fs = require('fs');
const { config, sharedSpecPath } = require('./lib');

function main() {
  const p = sharedSpecPath();
  if (!fs.existsSync(p)) {
    console.error('run "npm run create-mint" first.');
    process.exit(1);
  }
  if (!config.token.metadataUri) {
    console.log('token.metadataUri is empty in config.js — on-chain metadata is deferred.');
    console.log('See the header of this file for how to enable it later.');
    return;
  }
  console.log('metadataUri is set, but the Metaplex step is not wired yet.');
  console.log('Follow the header instructions to add mpl-token-metadata support.');
}

main();
