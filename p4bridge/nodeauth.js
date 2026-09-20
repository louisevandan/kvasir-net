'use strict';
/**
 * Node tokens: a wallet proves it holds a key, and gets a bearer token that
 * authorises participation — polling the coverage market, downloading a shard,
 * and opening a relay. Nothing else.
 *
 * This replaces `controller/siws.py`, which was deleted with the rest of the
 * control plane while both phones kept calling it. The wire format is kept
 * exactly: the clients are already written and shipped.
 *
 * One deliberate departure. The old hub gated the challenge on
 * `_operator_authorized`: an admin wallet, or one holding at least
 * LINKCPP_MIN_OPERATOR_KVR. Operating a hub and contributing compute to one are
 * different things, and with the defaults that gate refused every phone that
 * ever asked. Participation here is open unless an operator sets a minimum, and
 * the two policies are separate knobs on purpose.
 */
const crypto = require('node:crypto');

const NONCE_TTL_MS = 5 * 60 * 1000;
const TOKEN_TTL_DAYS = Number(process.env.KVR_NODE_TOKEN_TTL_DAYS ?? 30);

/** The DER prefix for an Ed25519 SubjectPublicKeyInfo, so a raw 32-byte key
 *  from a Solana address can be handed to node:crypto without a dependency. */
const SPKI_ED25519_PREFIX = Buffer.from('302a300506032b6570032100', 'hex');

const BASE58 = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';

/** Decode a base58 Solana address to its raw bytes. Returns null if it is not
 *  base58 at all, so a caller can answer 400 rather than throw. */
function base58Decode(value) {
  if (typeof value !== 'string' || value.length === 0 || value.length > 64) return null;
  let big = 0n;
  for (const ch of value) {
    const digit = BASE58.indexOf(ch);
    if (digit < 0) return null;
    big = big * 58n + BigInt(digit);
  }
  const bytes = [];
  while (big > 0n) {
    bytes.unshift(Number(big & 0xffn));
    big >>= 8n;
  }
  // Leading '1's are leading zero bytes, and a 32-byte key may legitimately
  // start with one — dropping them would silently shorten the key.
  for (const ch of value) {
    if (ch !== '1') break;
    bytes.unshift(0);
  }
  return Buffer.from(bytes);
}

/** Verify a detached ed25519 signature made by the key a Solana address names. */
function verifyWalletSignature(wallet, message, signatureBase64) {
  const raw = base58Decode(wallet);
  if (!raw || raw.length !== 32) return false;
  let signature;
  try {
    signature = Buffer.from(signatureBase64, 'base64');
  } catch {
    return false;
  }
  if (signature.length !== 64) return false;
  let key;
  try {
    key = crypto.createPublicKey({
      key: Buffer.concat([SPKI_ED25519_PREFIX, raw]),
      format: 'der',
      type: 'spki',
    });
  } catch {
    return false;
  }
  try {
    return crypto.verify(null, Buffer.from(message, 'utf8'), key, signature);
  } catch {
    return false;
  }
}

function timingSafeEqual(a, b) {
  const left = Buffer.from(String(a));
  const right = Buffer.from(String(b));
  if (left.length !== right.length) return false;
  return crypto.timingSafeEqual(left, right);
}

class NodeAuth {
  /**
   * @param {object} options
   * @param {string} options.secret       HMAC secret for token signing.
   * @param {string} [options.serviceToken] the machine-to-machine shared secret.
   * @param {(wallet: string) => Promise<boolean>} [options.eligible]
   *        Participation policy. Defaults to "anyone who holds a key".
   */
  constructor({ secret, serviceToken = '', eligible = null } = {}) {
    if (!secret) {
      // The original generated a random secret when none was configured, which
      // reads as working and then invalidates every node token the next time
      // the process restarts — for a token with a thirty-day life, that is a
      // fleet of phones that quietly stop participating.
      throw new Error(
        'node token signing needs KVR_NODE_TOKEN_SECRET; without a stable secret '
        + 'every token issued dies at the next restart');
    }
    this.secret = secret;
    this.serviceToken = serviceToken;
    this.eligible = eligible;
    /** @type {Map<string, {wallet: string, expires: number}>} */
    this.nonces = new Map();
  }

  /** Whether this wallet may participate. Open by default; see the file note. */
  async allows(wallet) {
    if (!this.eligible) return true;
    try {
      return await this.eligible(wallet);
    } catch {
      return false;
    }
  }

  /** The text a wallet signs. Any change here is invisible to clients — they
   *  sign whatever the challenge returns — but it must match what the token
   *  endpoint reconstructs. */
  static messageFor(wallet, nonce) {
    return `Kvasir bridge — prove you operate this node.\nwallet: ${wallet}\nnonce: ${nonce}`;
  }

  newChallenge(wallet) {
    const now = Date.now();
    for (const [key, entry] of this.nonces) {
      if (entry.expires < now) this.nonces.delete(key);
    }
    const nonce = crypto.randomBytes(16).toString('hex');
    this.nonces.set(nonce, { wallet, expires: now + NONCE_TTL_MS });
    return { nonce, message: NodeAuth.messageFor(wallet, nonce) };
  }

  /** Consume a nonce. Single use: a replayed signature must not mint a second
   *  token, so the entry is removed whether or not it turns out to be valid. */
  consumeChallenge(wallet, nonce) {
    const entry = this.nonces.get(nonce);
    if (!entry) return null;
    this.nonces.delete(nonce);
    if (entry.expires < Date.now()) return null;
    if (entry.wallet !== wallet) return null;
    return NodeAuth.messageFor(wallet, nonce);
  }

  makeToken(wallet, ttlDays = TOKEN_TTL_DAYS) {
    const exp = Math.floor(Date.now() / 1000) + Math.round(ttlDays * 86400);
    const body = `node|${wallet}|${exp}`;
    const mac = crypto.createHmac('sha256', this.secret).update(body).digest('hex');
    return {
      token: Buffer.from(`${body}|${mac}`, 'utf8').toString('base64url'),
      expiresIn: exp - Math.floor(Date.now() / 1000),
    };
  }

  /** Returns the wallet a token speaks for, or null. Stateless: nothing about
   *  an issued token is stored, so a restart does not disconnect a fleet. */
  verifyToken(token) {
    if (typeof token !== 'string' || !token) return null;
    let decoded;
    try {
      decoded = Buffer.from(token, 'base64url').toString('utf8');
    } catch {
      return null;
    }
    const parts = decoded.split('|');
    if (parts.length !== 4) return null;
    const [kind, wallet, expText, mac] = parts;
    if (kind !== 'node' || !wallet) return null;
    const exp = Number(expText);
    if (!Number.isFinite(exp) || exp < Math.floor(Date.now() / 1000)) return null;
    const expected = crypto
      .createHmac('sha256', this.secret)
      .update(`${kind}|${wallet}|${expText}`)
      .digest('hex');
    if (!timingSafeEqual(mac, expected)) return null;
    return wallet;
  }

  /**
   * Who is asking, from the headers a request carries.
   *
   * Both clients put the node token in `X-Kvasir-Service-Token` as well as in
   * `Authorization`, so that header is checked against the machine-to-machine
   * secret with a constant-time compare and, failing that, tried as a node
   * token. It must never be enough that a value merely appears there, or a node
   * token would escalate itself to the service grant.
   */
  identify(req) {
    const header = (name) => {
      const value = req.headers[name];
      return (Array.isArray(value) ? value[0] : value ?? '').toString().trim();
    };
    const service = header('x-kvasir-service-token');
    if (this.serviceToken && service && timingSafeEqual(service, this.serviceToken)) {
      return { kind: 'service', wallet: null };
    }
    const bearer = header('authorization').replace(/^Bearer\s+/i, '');
    for (const candidate of [bearer, service]) {
      const wallet = candidate ? this.verifyToken(candidate) : null;
      if (wallet) return { kind: 'node', wallet };
    }
    return { kind: 'none', wallet: null };
  }
}

module.exports = { NodeAuth, base58Decode, verifyWalletSignature };
