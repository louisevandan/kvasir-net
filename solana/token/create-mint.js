// Create the linkcpp SPL token mint on devnet and mint the initial supply to the
// treasury. Writes wallet/shared-spec/token.devnet.json as the shared source of
// truth for the iOS and Android wallets.
//
//   npm install
//   npm run create-mint
'use strict';

const fs = require('fs');
const {
  createMint,
  getOrCreateAssociatedTokenAccount,
  mintTo,
} = require('@solana/spl-token');
const { config, connection, adminKeypair, ensureFunded, explorer, sharedSpecPath } = require('./lib');

async function main() {
  const conn = connection();
  const admin = adminKeypair();
  console.log(`cluster:  ${config.cluster}`);
  console.log(`rpc:      ${config.rpcUrl}`);
  console.log(`admin:    ${admin.publicKey.toBase58()}`);

  await ensureFunded(conn, admin.publicKey, config.minAdminSol);

  const { decimals, symbol, name, initialSupply } = config.token;

  // Admin is payer + mint authority + freeze authority for the devnet phase.
  console.log('creating mint...');
  const mint = await createMint(
    conn,
    admin,
    admin.publicKey, // mint authority
    admin.publicKey, // freeze authority
    decimals,
  );
  console.log(`mint:     ${mint.toBase58()}`);

  console.log('creating treasury ATA...');
  const treasury = await getOrCreateAssociatedTokenAccount(conn, admin, mint, admin.publicKey);
  console.log(`treasury: ${treasury.address.toBase58()}`);

  const baseUnits = BigInt(initialSupply) * 10n ** BigInt(decimals);
  console.log(`minting ${initialSupply} ${symbol} (${baseUnits} base units)...`);
  const sig = await mintTo(conn, admin, mint, treasury.address, admin, baseUnits);
  console.log(`mintTo tx: ${sig}`);

  const spec = {
    cluster: config.cluster,
    rpcUrl: config.rpcUrl,
    token: {
      name,
      symbol,
      decimals,
      mint: mint.toBase58(),
      initialSupply,
    },
    treasury: {
      owner: admin.publicKey.toBase58(),
      ata: treasury.address.toBase58(),
    },
    authorities: {
      mintAuthority: admin.publicKey.toBase58(),
      freezeAuthority: admin.publicKey.toBase58(),
    },
    explorer: {
      mint: explorer('address', mint.toBase58()),
      treasury: explorer('address', treasury.address.toBase58()),
    },
    createdAtIso: new Date().toISOString(),
  };

  const outPath = sharedSpecPath();
  fs.mkdirSync(require('path').dirname(outPath), { recursive: true });
  fs.writeFileSync(outPath, JSON.stringify(spec, null, 2) + '\n');
  console.log(`\nwrote ${outPath}`);
  console.log(`explorer: ${spec.explorer.mint}`);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
