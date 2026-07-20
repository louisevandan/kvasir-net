// Gateway admin auth: Sign-In With Solana (wallet-owner login) + TOTP 2FA.
//
// Mirrors the hub's auth (controller/siws.py + controller/totp.py) for the
// settlement gateway. The admin is a wallet-address owner from an allowlist; they
// prove ownership by signing a server nonce (ed25519), then — if enrolled — a TOTP
// code from an authenticator app. Backup codes are the recovery path.
//
// Dependency policy: ed25519 verification uses tweetnacl (a transitive dep of
// @solana/web3.js, pinned directly in package.json). Everything else is stdlib
// (node:crypto). No base58 library is required — base58 decode is implemented here.
'use strict';

const crypto = require('crypto');
const nacl = require('tweetnacl');

// ---- config ---------------------------------------------------------------
// Admin allowlist. EMPTY => admin auth DISABLED (the admin surface returns 503).
const ADMIN_WALLETS = new Set(
  (process.env.KVR_ADMIN_WALLETS || '').split(',').map((w) => w.trim()).filter(Boolean),
);
// Stable signing key if provided, else random per process (sessions drop on restart).
const SECRET = Buffer.from(process.env.KVR_SESSION_SECRET || crypto.randomBytes(32).toString('hex'));

function authEnabled() { return ADMIN_WALLETS.size > 0; }
function isAdmin(wallet) { return ADMIN_WALLETS.has((wallet || '').trim()); }

// ---- base58 (Bitcoin alphabet) --------------------------------------------
const B58 = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';
function b58decode(str) {
  const map = {};
  for (let i = 0; i < B58.length; i++) map[B58[i]] = i;
  const bytes = [0];
  for (const ch of String(str)) {
    const val = map[ch];
    if (val === undefined) throw new Error('invalid base58 character');
    let carry = val;
    for (let j = 0; j < bytes.length; j++) {
      carry += bytes[j] * 58;
      bytes[j] = carry & 0xff;
      carry >>= 8;
    }
    while (carry > 0) { bytes.push(carry & 0xff); carry >>= 8; }
  }
  // leading '1's => leading zero bytes
  for (let k = 0; k < str.length && str[k] === '1'; k++) bytes.push(0);
  return Uint8Array.from(bytes.reverse());
}

// ---- ed25519 verify -------------------------------------------------------
function ed25519Verify(signature, message, publicKey) {
  try {
    return nacl.sign.detached.verify(
      Uint8Array.from(message), Uint8Array.from(signature), Uint8Array.from(publicKey),
    );
  } catch { return false; }
}

// ---- login challenge (nonce) ----------------------------------------------
const NONCES = new Map(); // nonce -> { wallet, expiry }
const NONCE_TTL_MS = 300 * 1000;

function signInMessage(wallet, nonce) {
  // The exact text the wallet signs. Kept human-readable + domain-bound.
  return `linkcpp gateway admin sign-in\nwallet: ${wallet}\nnonce: ${nonce}`;
}

function newChallenge(wallet) {
  const nonce = crypto.randomBytes(16).toString('hex');
  NONCES.set(nonce, { wallet, expiry: Date.now() + NONCE_TTL_MS });
  return { nonce, message: signInMessage(wallet, nonce) };
}

function consumeChallenge(nonce) {
  const e = NONCES.get(nonce);
  if (!e) return null;
  NONCES.delete(nonce);
  if (e.expiry < Date.now()) return null;
  return e.wallet;
}

function verifyLogin(wallet, nonce, signatureB64) {
  const w = (wallet || '').trim();
  const owner = consumeChallenge((nonce || '').trim());
  if (!owner || owner !== w) return false;
  let sig;
  try { sig = Buffer.from(signatureB64 || '', 'base64'); } catch { return false; }
  if (sig.length !== 64) return false;
  let pub;
  try { pub = b58decode(w); } catch { return false; }
  if (pub.length !== 32) return false;
  const msg = Buffer.from(signInMessage(w, nonce), 'utf8');
  return ed25519Verify(sig, msg, pub);
}

// ---- HMAC session + scoped tokens -----------------------------------------
function b64url(buf) { return Buffer.from(buf).toString('base64url'); }

function makeSession(wallet, ttlSec = 86400) {
  const exp = Math.floor(Date.now() / 1000) + ttlSec;
  const payload = `${wallet}|${exp}`;
  const sig = crypto.createHmac('sha256', SECRET).update(payload).digest('hex');
  return b64url(`${payload}|${sig}`);
}

function verifySession(token) {
  try {
    const raw = Buffer.from(token, 'base64url').toString('utf8');
    const i2 = raw.lastIndexOf('|');
    const i1 = raw.lastIndexOf('|', i2 - 1);
    const wallet = raw.slice(0, i1);
    const exp = raw.slice(i1 + 1, i2);
    const sig = raw.slice(i2 + 1);
    const good = crypto.createHmac('sha256', SECRET).update(`${wallet}|${exp}`).digest('hex');
    if (!timingEqualHex(good, sig)) return null;
    if (Number(exp) < Math.floor(Date.now() / 1000)) return null;
    return wallet;
  } catch { return null; }
}

// Scoped short-lived token for the "wallet verified, 2FA pending" step.
function makeToken(wallet, scope, ttlSec) {
  const exp = Math.floor(Date.now() / 1000) + ttlSec;
  const payload = `${scope}|${wallet}|${exp}`;
  const sig = crypto.createHmac('sha256', SECRET).update(payload).digest('hex');
  return b64url(`${payload}|${sig}`);
}

function verifyToken(token, scope) {
  try {
    const raw = Buffer.from(token, 'base64url').toString('utf8');
    const parts = raw.split('|');
    if (parts.length !== 4) return null;
    const [sc, wallet, exp, sig] = parts;
    if (sc !== scope) return null;
    const good = crypto.createHmac('sha256', SECRET).update(`${sc}|${wallet}|${exp}`).digest('hex');
    if (!timingEqualHex(good, sig)) return null;
    if (Number(exp) < Math.floor(Date.now() / 1000)) return null;
    return wallet;
  } catch { return null; }
}

function timingEqualHex(a, b) {
  const ba = Buffer.from(String(a)); const bb = Buffer.from(String(b));
  if (ba.length !== bb.length) return false;
  return crypto.timingSafeEqual(ba, bb);
}

// ---- TOTP (RFC 6238) ------------------------------------------------------
const B32 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';

function newSecret(nbytes = 20) {
  const buf = crypto.randomBytes(nbytes);
  let bits = '';
  for (const b of buf) bits += b.toString(2).padStart(8, '0');
  let out = '';
  for (let i = 0; i + 5 <= bits.length; i += 5) out += B32[parseInt(bits.slice(i, i + 5), 2)];
  return out;
}

function b32decode(s) {
  const clean = String(s).replace(/=+$/, '').replace(/\s/g, '').toUpperCase();
  let bits = '';
  for (const ch of clean) {
    const idx = B32.indexOf(ch);
    if (idx < 0) throw new Error('invalid base32');
    bits += idx.toString(2).padStart(5, '0');
  }
  const bytes = [];
  for (let i = 0; i + 8 <= bits.length; i += 8) bytes.push(parseInt(bits.slice(i, i + 8), 2));
  return Buffer.from(bytes);
}

function hotp(secretB32, counter, digits = 6) {
  const key = b32decode(secretB32);
  const buf = Buffer.alloc(8);
  buf.writeBigUInt64BE(BigInt(counter));
  const h = crypto.createHmac('sha1', key).update(buf).digest();
  const o = h[h.length - 1] & 0x0f;
  const code = ((h.readUInt32BE(o) & 0x7fffffff) % 10 ** digits);
  return String(code).padStart(digits, '0');
}

function nowCode(secretB32, step = 30, digits = 6) {
  return hotp(secretB32, Math.floor(Date.now() / 1000 / step), digits);
}

function verifyCode(secretB32, code, { step = 30, digits = 6, window = 1 } = {}) {
  const c = String(code || '').trim();
  if (!/^\d+$/.test(c) || c.length !== digits) return false;
  const counter = Math.floor(Date.now() / 1000 / step);
  for (let w = -window; w <= window; w++) {
    const good = hotp(secretB32, counter + w, digits);
    if (timingEqualHex(good, c)) return true;
  }
  return false;
}

function otpauthUri(secretB32, account, issuer = 'linkcpp-gateway') {
  const label = encodeURIComponent(`${issuer}:${account}`);
  return `otpauth://totp/${label}?secret=${secretB32}`
    + `&issuer=${encodeURIComponent(issuer)}&algorithm=SHA1&digits=6&period=30`;
}

// ---- backup codes ---------------------------------------------------------
function newBackupCodes(n = 10) {
  const codes = [];
  for (let i = 0; i < n; i++) {
    const raw = crypto.randomBytes(7).toString('hex').slice(0, 10);
    codes.push(`${raw.slice(0, 5)}-${raw.slice(5)}`);
  }
  return codes;
}

function hashBackup(code) {
  return crypto.createHash('sha256')
    .update(String(code || '').trim().toLowerCase().replace(/-/g, '')).digest('hex');
}

function verifyAndConsumeBackup(hashes, code) {
  const h = hashBackup(code);
  for (let i = 0; i < (hashes || []).length; i++) {
    if (timingEqualHex(hashes[i], h)) return hashes.slice(0, i).concat(hashes.slice(i + 1));
  }
  return null;
}

// ---- cookie helpers -------------------------------------------------------
function parseCookies(req) {
  const out = {};
  const raw = req.headers.cookie || '';
  for (const part of raw.split(';')) {
    const i = part.indexOf('=');
    if (i < 0) continue;
    out[part.slice(0, i).trim()] = decodeURIComponent(part.slice(i + 1).trim());
  }
  return out;
}

module.exports = {
  authEnabled, isAdmin,
  newChallenge, verifyLogin,
  makeSession, verifySession, makeToken, verifyToken,
  newSecret, nowCode, verifyCode, otpauthUri,
  newBackupCodes, hashBackup, verifyAndConsumeBackup,
  parseCookies,
};
