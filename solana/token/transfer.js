// Transfer LKC from the treasury to any devnet address, using the admin key.
// Useful for funding test wallets during Phase 1 (public devnet airdrop is
// IP rate-limited, so we move real LKC from the treasury instead).
//
//   npm run transfer -- <recipientPubkey> <amountWholeTokens>
'use strict';

const fs = require('fs');
const {
  getOrCreateAssociatedTokenAccount,
  getAssociatedTokenAddress,
  transferChecked,
} = require('@solana/spl-token');
const { connection, adminKeypair, ensureFunded, config, explorer, sharedSpecPath, PublicKey } = require('./lib');

async function main() {
  const recipientArg = process.argv[2];
  const amountArg = process.argv[3];
  if (!recipientArg || !amountArg) {
    console.error('usage: npm run transfer -- <recipientPubkey> <amountWholeTokens>');
    process.exit(1);
  }
  const spec = JSON.parse(fs.readFileSync(sharedSpecPath(), 'utf8'));
  if (!spec.token || !spec.token.mint) {
    console.error('no mint in shared spec — run "npm run create-mint" first');
    process.exit(1);
  }
  const conn = connection();
  const admin = adminKeypair();
  const mint = new PublicKey(spec.token.mint);
  const decimals = spec.token.decimals;
  const recipient = new PublicKey(recipientArg);
  const baseUnits = BigInt(Math.round(Number(amountArg) * 10 ** decimals));

  // Admin pays fees + rent for the recipient ATA.
  await ensureFunded(conn, admin.publicKey, config.minAdminSol);

  const source = await getAssociatedTokenAddress(mint, admin.publicKey);
  console.log(`treasury ATA: ${source.toBase58()}`);
  console.log(`creating/looking up recipient ATA for ${recipient.toBase58()}...`);
  const destAcct = await getOrCreateAssociatedTokenAccount(conn, admin, mint, recipient);
  console.log(`recipient ATA: ${destAcct.address.toBase58()}`);

  console.log(`transferring ${amountArg} ${spec.token.symbol} (${baseUnits} base units)...`);
  const sig = await transferChecked(
    conn,
    admin,           // payer
    source,          // source ATA (treasury)
    mint,
    destAcct.address,// dest ATA
    admin,           // owner/authority of source
    baseUnits,
    decimals,
  );
  console.log(`tx: ${sig}`);
  console.log(`explorer: ${explorer('tx', sig)}`);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
