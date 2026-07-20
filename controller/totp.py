"""TOTP (RFC 6238) + backup codes for hub 2FA — dependency-free (stdlib only).

Layered on top of Sign-In With Solana ([[siws]]): the wallet signature is the
first factor (something you have: the key); a TOTP code from an authenticator app
is the second factor (something you have: the enrolled device). Backup codes are
the recovery path if the authenticator is lost.
"""

import base64
import hashlib
import hmac
import secrets
import struct
import time
from urllib.parse import quote


# ---- TOTP -----------------------------------------------------------------
def new_secret(nbytes=20):
    """A fresh base32 TOTP secret (no padding) — what the authenticator stores."""
    return base64.b32encode(secrets.token_bytes(nbytes)).decode().rstrip("=")


def _b32decode(s):
    s = s.strip().replace(" ", "").upper()
    return base64.b32decode(s + "=" * ((8 - len(s) % 8) % 8))


def _hotp(secret_b32, counter, digits=6):
    key = _b32decode(secret_b32)
    h = hmac.new(key, struct.pack(">Q", counter), hashlib.sha1).digest()
    o = h[-1] & 0x0F
    code = (struct.unpack(">I", h[o:o + 4])[0] & 0x7FFFFFFF) % (10 ** digits)
    return str(code).zfill(digits)


def now_code(secret_b32, t=None, step=30, digits=6):
    return _hotp(secret_b32, int((t if t is not None else time.time()) // step), digits)


def verify_code(secret_b32, code, t=None, step=30, digits=6, window=1):
    """Constant-time verify with a ±window step tolerance for clock skew."""
    code = (code or "").strip()
    if not (code.isdigit() and len(code) == digits):
        return False
    counter = int((t if t is not None else time.time()) // step)
    for w in range(-window, window + 1):
        if hmac.compare_digest(_hotp(secret_b32, counter + w, digits), code):
            return True
    return False


def otpauth_uri(secret_b32, account, issuer="linkcpp-hub"):
    """otpauth:// URI to render as a QR for Google Authenticator / Authy / etc."""
    label = quote(f"{issuer}:{account}", safe="")
    return (f"otpauth://totp/{label}?secret={secret_b32}"
            f"&issuer={quote(issuer, safe='')}&algorithm=SHA1&digits=6&period=30")


# ---- backup codes ---------------------------------------------------------
def new_backup_codes(n=10):
    """Human-typeable one-time recovery codes (shown once, stored only as hashes)."""
    codes = []
    for _ in range(n):
        raw = base64.b32encode(secrets.token_bytes(7)).decode().rstrip("=").lower()[:10]
        codes.append(f"{raw[:5]}-{raw[5:]}")
    return codes


def hash_backup(code):
    return hashlib.sha256((code or "").strip().lower().replace("-", "").encode()).hexdigest()


def verify_and_consume_backup(hashes, code):
    """If `code` matches a stored hash, return the remaining hashes (consumed); else None."""
    h = hash_backup(code)
    for i, stored in enumerate(hashes or []):
        if hmac.compare_digest(stored, h):
            return hashes[:i] + hashes[i + 1:]
    return None
