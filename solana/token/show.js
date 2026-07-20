// Print current on-chain state of the linkcpp token from the shared spec:
// mint supply and treasury balance. Verifies the mint actually exists on devnet.
//
//   npm run show
'use strict';

const fs = require('fs');
const { getMint, getAccount } = require('@solana/spl-token');
const { connection, sharedSpecPath, PublicKey } = require('./lib');

async function main() {
  const p = sharedSpecPath();
  if (!fs.existsSync(p)) {
    console.error(`no shared spec yet at ${p} — run "npm run create-mint" first`);
    process.exit(1);
  }
  const spec = JSON.parse(fs.readFileSync(p, 'utf8'));
  const conn = connection();
  const mint = new PublicKey(spec.token.mint);
  const info = await getMint(conn, mint);
  const div = 10 ** spec.token.decimals;
  console.log(`token:    ${spec.token.name} (${spec.token.symbol})`);
  console.log(`mint:     ${mint.toBase58()}`);
  console.log(`decimals: ${info.decimals}`);
  console.log(`supply:   ${Number(info.supply) / div}`);
  console.log(`authority:${info.mintAuthority ? info.mintAuthority.toBase58() : 'none'}`);
  if (spec.treasury && spec.treasury.ata) {
    const ata = await getAccount(conn, new PublicKey(spec.treasury.ata));
    console.log(`treasury: ${Number(ata.amount) / div} ${spec.token.symbol}`);
  }
  console.log(`explorer: ${spec.explorer.mint}`);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
