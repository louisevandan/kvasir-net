// Shared helpers for the linkcpp token scripts.
'use strict';

const fs = require('fs');
const path = require('path');
const { Connection, Keypair, LAMPORTS_PER_SOL, PublicKey } = require('@solana/web3.js');
const config = require('./config');

function connection() {
  return new Connection(config.rpcUrl, 'confirmed');
}

function keysDir() {
  return path.resolve(__dirname, config.keysDir);
}

// Load a keypair from solana/token/.keys/<file>, creating and persisting a fresh
// one if it does not exist yet.
function loadOrCreateKeypair(file) {
  const dir = keysDir();
  fs.mkdirSync(dir, { recursive: true });
  const p = path.join(dir, file);
  if (fs.existsSync(p)) {
    const secret = JSON.parse(fs.readFileSync(p, 'utf8'));
    return Keypair.fromSecretKey(Uint8Array.from(secret));
  }
  const kp = Keypair.generate();
  fs.writeFileSync(p, JSON.stringify(Array.from(kp.secretKey)));
  console.log(`created new keypair: ${p}`);
  return kp;
}

function adminKeypair() {
  return loadOrCreateKeypair(config.adminKeyFile);
}

async function ensureFunded(conn, pubkey, minSol) {
  const target = Math.round(minSol * LAMPORTS_PER_SOL);
  let bal = await conn.getBalance(pubkey);
  if (bal >= target) return bal;
  console.log(`balance ${bal / LAMPORTS_PER_SOL} SOL < ${minSol} SOL, requesting airdrop...`);
  try {
    const sig = await conn.requestAirdrop(pubkey, target - bal);
    const latest = await conn.getLatestBlockhash();
    await conn.confirmTransaction({ signature: sig, ...latest }, 'confirmed');
  } catch (e) {
    console.warn(`airdrop failed (devnet faucet may be rate-limited): ${e.message}`);
    console.warn(`fund ${pubkey.toBase58()} manually, e.g. https://faucet.solana.com`);
  }
  bal = await conn.getBalance(pubkey);
  console.log(`balance now ${bal / LAMPORTS_PER_SOL} SOL`);
  return bal;
}

function explorer(kind, id) {
  return `https://explorer.solana.com/${kind}/${id}?cluster=${config.cluster}`;
}

function sharedSpecPath() {
  return path.resolve(__dirname, '..', '..', 'wallet', 'shared-spec', 'token.devnet.json');
}

module.exports = {
  config,
  connection,
  keysDir,
  loadOrCreateKeypair,
  adminKeypair,
  ensureFunded,
  explorer,
  sharedSpecPath,
  PublicKey,
  LAMPORTS_PER_SOL,
};
