/* Shared helpers for the GitHub star-gate Functions. Underscore-prefixed so it
   is a module, not a route. HMAC-signed stateless session cookie (no token
   stored client-side; re-checking a star just re-runs the OAuth flow). */
const enc = new TextEncoder();

function b64url(bytes) {
  let s = "";
  for (const b of bytes) s += String.fromCharCode(b);
  return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}
function b64urlDecode(str) {
  str = str.replace(/-/g, "+").replace(/_/g, "/");
  while (str.length % 4) str += "=";
  const bin = atob(str);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

async function hmac(data, secret) {
  const key = await crypto.subtle.importKey(
    "raw",
    enc.encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"]
  );
  const sig = await crypto.subtle.sign("HMAC", key, enc.encode(data));
  return b64url(new Uint8Array(sig));
}

export async function signSession(payload, secret) {
  const data = b64url(enc.encode(JSON.stringify(payload)));
  return `${data}.${await hmac(data, secret)}`;
}

export async function verifySession(token, secret, maxAgeMs = 7 * 864e5) {
  if (!token || !secret || token.indexOf(".") < 0) return null;
  const [data, sig] = token.split(".");
  if ((await hmac(data, secret)) !== sig) return null;
  try {
    const obj = JSON.parse(new TextDecoder().decode(b64urlDecode(data)));
    if (!obj.iat || Date.now() - obj.iat > maxAgeMs) return null;
    return obj;
  } catch {
    return null;
  }
}

export function parseCookies(request) {
  const out = {};
  const raw = request.headers.get("cookie") || "";
  for (const part of raw.split(";")) {
    const i = part.indexOf("=");
    if (i > 0) out[part.slice(0, i).trim()] = decodeURIComponent(part.slice(i + 1).trim());
  }
  return out;
}
