// Airdrop devnet SOL to a pubkey (defaults to the admin keypair). Useful for
// funding wallet test accounts during Phase 1.
//
//   npm run airdrop -- <pubkey> [sol]
'use strict';

const { config, connection, adminKeypair, ensureFunded, PublicKey, LAMPORTS_PER_SOL } = require('./lib');

async function main() {
  const conn = connection();
  const arg = process.argv[2];
  const sol = Number(process.argv[3] || config.minAdminSol);
  const target = arg ? new PublicKey(arg) : adminKeypair().publicKey;
  console.log(`airdropping ${sol} SOL to ${target.toBase58()} on ${config.cluster}`);
  await ensureFunded(conn, target, sol);
  const bal = await conn.getBalance(target);
  console.log(`final balance: ${bal / LAMPORTS_PER_SOL} SOL`);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
