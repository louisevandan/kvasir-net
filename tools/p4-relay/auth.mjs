/**
 * Proving which wallet is on the other end of a tunnel.
 *
 * The scheme is deliberately the one the gateway already uses for operator
 * sign-in (`solana/staking-service/gatewayAuth.js`): a server nonce, a
 * human-readable domain-bound message, a base58 Solana address decoded to an
 * ed25519 public key, and a detached signature checked with tweetnacl. Reusing
 * it means a node proves the same identity the rewards ledger is keyed on, and
 * there is one signature convention in this project rather than two.
 *
 * What this establishes and what it does not. It establishes that whoever is
 * connecting holds the private key for the wallet they named. It does not say
 * that wallet is allowed to run a node, hold a stake, or earn anything — that
 * is policy, it belongs to the gateway, and the relay asks about it separately.
 */
import crypto from 'node:crypto';
import nacl from 'tweetnacl';

const NONCE_TTL_MS = 120_000;
const nonces = new Map();          // nonce -> expiry

/** base58 -> bytes. Solana addresses only; no length assumptions here. */
export function b58decode(str) {
  const ALPHABET = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';
  const bytes = [];
  for (const ch of String(str)) {
    const value = ALPHABET.indexOf(ch);
    if (value < 0) throw new Error('not base58');
    let carry = value;
    for (let i = 0; i < bytes.length; i++) {
      carry += bytes[i] * 58;
      bytes[i] = carry & 0xff;
      carry >>= 8;
    }
    while (carry > 0) { bytes.push(carry & 0xff); carry >>= 8; }
  }
  for (let k = 0; k < String(str).length && String(str)[k] === '1'; k++) bytes.push(0);
  return Uint8Array.from(bytes.reverse());
}

/**
 * The exact text a node signs.
 *
 * Bound to the relay host and the node id as well as the nonce, so a signature
 * made for one relay cannot open a tunnel on another, and one made for a node
 * cannot be replayed as a different one.
 */
export const message = (relayHost, nodeId, owner, nonce) =>
  `kvasir p4 relay\nrelay: ${relayHost}\nnode: ${nodeId}\nwallet: ${owner}\nnonce: ${nonce}`;

export function newChallenge(relayHost) {
  const nonce = crypto.randomBytes(16).toString('hex');
  nonces.set(nonce, Date.now() + NONCE_TTL_MS);
  // Opportunistic sweep: a relay that is dialled constantly should not grow a
  // map of nonces nobody answered.
  if (nonces.size > 1000) {
    const now = Date.now();
    for (const [key, expiry] of nonces) if (expiry < now) nonces.delete(key);
  }
  return { nonce, relayHost };
}

/** True once, for a live nonce and a signature that matches. */
export function verify({ relayHost, nodeId, owner, nonce, signature }) {
  const expiry = nonces.get(nonce);
  // Consumed whether or not it verifies: a nonce that survives a failed attempt
  // is a nonce an attacker can keep guessing against.
  nonces.delete(nonce);
  if (!expiry || expiry < Date.now()) return { ok: false, error: 'challenge expired' };
  if (!nodeId || !owner) return { ok: false, error: 'nodeId and owner required' };

  let sig;
  try { sig = Buffer.from(String(signature ?? ''), 'base64'); } catch { return { ok: false, error: 'signature is not base64' }; }
  if (sig.length !== 64) return { ok: false, error: 'signature must be 64 bytes' };

  let pub;
  try { pub = b58decode(owner); } catch { return { ok: false, error: 'owner is not base58' }; }
  if (pub.length !== 32) return { ok: false, error: 'owner is not a 32-byte key' };

  const text = Buffer.from(message(relayHost, nodeId, owner, nonce), 'utf8');
  let good = false;
  try { good = nacl.sign.detached.verify(Uint8Array.from(text), Uint8Array.from(sig), pub); }
  catch { good = false; }
  return good ? { ok: true, owner } : { ok: false, error: 'signature does not match owner' };
}
