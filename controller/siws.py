"""Sign-In With Solana for the hub — dependency-free.

The hub image ships no ed25519/base58 library and its build is pinned, so this
module vendors exactly what auth needs, using only the stdlib:

  - base58 decode (Solana address / signature)
  - ed25519 signature verification (RFC 8032, verify-only reference arithmetic)
  - HMAC-signed, expiring session tokens

An operator proves control of a wallet by signing a server-issued nonce; the hub
verifies the signature against an admin allowlist and issues a session token.
"""

import base64
import hashlib
import hmac
import os
import secrets
import time

# ---- base58 ---------------------------------------------------------------
_B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
_B58_MAP = {c: i for i, c in enumerate(_B58)}


def b58decode(s):
    n = 0
    for c in s:
        v = _B58_MAP.get(c)
        if v is None:
            raise ValueError("invalid base58")
        n = n * 58 + v
    body = n.to_bytes((n.bit_length() + 7) // 8, "big") if n else b""
    pad = 0
    for c in s:
        if c == "1":
            pad += 1
        else:
            break
    return b"\x00" * pad + body


# ---- ed25519 verify (RFC 8032 reference; slow but correct) ----------------
_P = 2 ** 255 - 19
_L = 2 ** 252 + 27742317777372353535851937790883648493


def _inv(x):
    return pow(x, _P - 2, _P)


_D = (-121665 * _inv(121666)) % _P
_I = pow(2, (_P - 1) // 4, _P)


def _xrecover(y):
    xx = (y * y - 1) * _inv(_D * y * y + 1)
    x = pow(xx, (_P + 3) // 8, _P)
    if (x * x - xx) % _P != 0:
        x = (x * _I) % _P
    if x % 2 != 0:
        x = _P - x
    return x


_By = (4 * _inv(5)) % _P
_Bx = _xrecover(_By)
_B = [_Bx % _P, _By % _P]


def _edwards(P, Q):
    x1, y1 = P
    x2, y2 = Q
    x3 = (x1 * y2 + x2 * y1) * _inv(1 + _D * x1 * x2 * y1 * y2)
    y3 = (y1 * y2 + x1 * x2) * _inv(1 - _D * x1 * x2 * y1 * y2)
    return [x3 % _P, y3 % _P]


def _scalarmult(P, e):
    # Iterative double-and-add (avoids deep recursion for 256-bit scalars).
    Q = [0, 1]
    while e > 0:
        if e & 1:
            Q = _edwards(Q, P)
        P = _edwards(P, P)
        e >>= 1
    return Q


def _bit(h, i):
    return (h[i // 8] >> (i % 8)) & 1


def _decodeint(s):
    return sum(2 ** i * _bit(s, i) for i in range(256))


def _isoncurve(P):
    x, y = P
    return (-x * x + y * y - 1 - _D * x * x * y * y) % _P == 0


def _decodepoint(s):
    y = sum(2 ** i * _bit(s, i) for i in range(0, 255))
    x = _xrecover(y)
    if x & 1 != _bit(s, 255):
        x = _P - x
    P = [x, y]
    if not _isoncurve(P):
        raise ValueError("point not on curve")
    return P


def ed25519_verify(signature, message, public_key):
    """True iff `signature` (64B) is a valid ed25519 sig of `message` by `public_key` (32B)."""
    try:
        if len(signature) != 64 or len(public_key) != 32:
            return False
        R = _decodepoint(signature[:32])
        A = _decodepoint(public_key)
        S = _decodeint(signature[32:])
        if S >= _L:
            return False
        # h = SHA-512(R || A || M) as a full 512-bit little-endian integer (NOT 256).
        digest = hashlib.sha512(signature[:32] + public_key + message).digest()
        h = sum(2 ** i * _bit(digest, i) for i in range(512))
        return _scalarmult(_B, S) == _edwards(R, _scalarmult(A, h))
    except Exception:
        return False


# ---- HMAC session tokens --------------------------------------------------
# Signing key: stable if provided, else random per process (sessions drop on restart).
_SECRET = (os.environ.get("LINKCPP_SESSION_SECRET") or secrets.token_hex(32)).encode()


def make_session(wallet, ttl=86400):
    exp = int(time.time()) + int(ttl)
    payload = f"{wallet}|{exp}"
    sig = hmac.new(_SECRET, payload.encode(), hashlib.sha256).hexdigest()
    return base64.urlsafe_b64encode(f"{payload}|{sig}".encode()).decode()


def verify_session(token):
    try:
        raw = base64.urlsafe_b64decode(token.encode()).decode()
        wallet, exp, sig = raw.rsplit("|", 2)
        # A scoped token (make_token: "scope|wallet|exp|sig") parses here with a
        # "|" left in `wallet` and would otherwise pass — reject it so only plain
        # sessions authenticate as full sessions.
        if "|" in wallet:
            return None
        good = hmac.new(_SECRET, f"{wallet}|{exp}".encode(), hashlib.sha256).hexdigest()
        if not hmac.compare_digest(good, sig):
            return None
        if int(exp) < time.time():
            return None
        return wallet
    except Exception:
        return None


# Scoped short-lived token — used for the "wallet signature verified, 2FA pending"
# step between /api/auth/verify and /api/auth/2fa/login (scope "pre2fa").
def make_token(wallet, scope, ttl):
    exp = int(time.time()) + int(ttl)
    payload = f"{scope}|{wallet}|{exp}"
    sig = hmac.new(_SECRET, payload.encode(), hashlib.sha256).hexdigest()
    return base64.urlsafe_b64encode(f"{payload}|{sig}".encode()).decode()


def verify_token(token, scope):
    try:
        raw = base64.urlsafe_b64decode(token.encode()).decode()
        sc, wallet, exp, sig = raw.rsplit("|", 3)
        if sc != scope:
            return None
        good = hmac.new(_SECRET, f"{sc}|{wallet}|{exp}".encode(), hashlib.sha256).hexdigest()
        if not hmac.compare_digest(good, sig):
            return None
        if int(exp) < time.time():
            return None
        return wallet
    except Exception:
        return None


# ---- nonce challenge store (in-memory, short TTL) -------------------------
_NONCES = {}   # nonce -> (wallet, expiry)
_NONCE_TTL = 300


def _sweep(now):
    for k in [k for k, (_, e) in _NONCES.items() if e < now]:
        _NONCES.pop(k, None)


def sign_in_message(wallet, nonce):
    return (
        "linkcpp hub — sign in to prove you operate this hub.\n"
        f"wallet: {wallet}\n"
        f"nonce: {nonce}"
    )


def new_challenge(wallet):
    now = time.time()
    _sweep(now)
    nonce = secrets.token_hex(16)
    _NONCES[nonce] = (wallet, now + _NONCE_TTL)
    return nonce, sign_in_message(wallet, nonce)


def consume_challenge(wallet, nonce):
    """Return the message for a valid, unexpired, matching nonce (one-time), else None."""
    entry = _NONCES.pop(nonce, None)
    if not entry:
        return None
    nw, exp = entry
    if nw != wallet or exp < time.time():
        return None
    return sign_in_message(wallet, nonce)


def verify_login(wallet, nonce, signature_b64):
    """Full check: valid nonce + ed25519 signature by `wallet` over the sign-in message."""
    message = consume_challenge(wallet, nonce)
    if message is None:
        return False
    try:
        pubkey = b58decode(wallet)
        sig = base64.b64decode(signature_b64)
    except Exception:
        return False
    return ed25519_verify(sig, message.encode("utf-8"), pubkey)
