#!/usr/bin/env node
/**
 * Admissions: a machine asks to join the ring, a person says yes, the ring is told.
 *
 *   bot ──POST /requests──▶ here ──Resend──▶ approver's inbox
 *                                              │ clicks
 *                                              ▼
 *                            GET /a/<id>  (a page, not an approval)
 *                                              │ presses the button
 *                                              ▼
 *                            POST /a/<id> ──▶ gate ──▶ bridge /api/admissions
 *
 * Why the click is two steps. A mail scanner or an inbox preview fetches the
 * links in a message before a human ever sees them. If the link itself admitted
 * the wallet, the scanner would do the approving, and the single-use token would
 * be spent by a machine reading mail. So GET only renders what was asked for,
 * and the state change is a POST that a prefetch does not make.
 *
 * Nothing that grants anything travels in the mail. The link carries an id and
 * a one-time token that authorise exactly one approval of exactly one request;
 * the credential that talks to the ring lives here and never leaves.
 */
import http from 'node:http';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const STATE = process.env.ADMISSIONS_STATE ?? path.join(HERE, 'state', 'requests.json');

const PORT = Number(process.env.PORT ?? 8795);
const FROM = process.env.ADMISSIONS_FROM ?? 'Kvasir admissions <admissions@reg.kvasir-ai.net>';
// reg.kvasir-ai.net is what is verified to send; the apex is not, and is a
// place to receive a reply rather than a place to send one from.
const REPLY_TO = (process.env.ADMISSIONS_REPLY_TO ?? 'tony@kvasir-ai.net').trim();
const APPROVER = (process.env.ADMISSIONS_APPROVER ?? '').trim();
const PUBLIC_URL = (process.env.ADMISSIONS_PUBLIC_URL ?? `http://127.0.0.1:${PORT}`).replace(/\/+$/, '');
const GATE = (process.env.KVASIR_GATE_URL ?? 'https://gate.kvasir-ai.net').replace(/\/+$/, '');
const RESEND_KEY = (process.env.RESEND_API_KEY ?? '').trim();
const RING_TOKEN = (process.env.P4_BRIDGE_TOKEN ?? '').trim();
const INTAKE_TOKEN = (process.env.ADMISSIONS_INTAKE_TOKEN ?? '').trim();

/** How long an approval link is worth anything. */
const TTL_MS = Number(process.env.ADMISSIONS_TTL_MINUTES ?? 30) * 60_000;
/** A base58 Solana address, which is what both askers name. */
const WALLET_RE = /^[1-9A-HJ-NP-Za-km-z]{32,44}$/;
/** How many requests may be waiting for an answer at once. The mail is the
 *  thing that grants access, so the number that matters is how many of them can
 *  be in front of the approver — not how many the disk could hold. */
const MAX_PENDING = Number(process.env.ADMISSIONS_MAX_PENDING ?? 50);
/** Approved requests are kept as a record of who was let in, but not forever;
 *  the admitted list on the bridge is the authority. */
const KEEP_APPROVED_MS = Number(process.env.ADMISSIONS_KEEP_APPROVED_DAYS ?? 30) * 86_400_000;

for (const [name, value] of Object.entries({
  ADMISSIONS_APPROVER: APPROVER, RESEND_API_KEY: RESEND_KEY,
  P4_BRIDGE_TOKEN: RING_TOKEN, ADMISSIONS_INTAKE_TOKEN: INTAKE_TOKEN,
})) {
  // Refuse to start rather than to accept a request and then discover, at the
  // moment someone is waiting, that it cannot mail or cannot admit.
  if (!value) { console.error(`admissions: ${name} must be set`); process.exit(2); }
}

// ---- state ---------------------------------------------------------------
//
// One JSON file. The volume here is a handful of requests a week, and a
// database would be a dependency to install on a box that is already running
// something else.

function load() {
  try { return JSON.parse(fs.readFileSync(STATE, 'utf8')); } catch { return {}; }
}
function save(db) {
  fs.mkdirSync(path.dirname(STATE), { recursive: true });
  const tmp = `${STATE}.tmp`;
  fs.writeFileSync(tmp, `${JSON.stringify(db, null, 2)}\n`, { mode: 0o600 });
  fs.renameSync(tmp, STATE);
}

const now = () => Date.now();

/**
 * Drop what can no longer be acted on, and say how much is still live.
 *
 * Three kinds go: a pending request whose link has expired — the approver can
 * no longer use it and the machine is expected to ask again; a request whose
 * mail never went out, which is not waiting on anybody; and an approval old
 * enough that the bridge's admitted list is the only record worth keeping.
 */
function prune(db) {
  const t = now();
  let pending = 0;
  for (const [id, r] of Object.entries(db)) {
    if (r.state === 'approved') {
      if (t - (r.approvedAt ?? r.createdAt ?? 0) > KEEP_APPROVED_MS) delete db[id];
      continue;
    }
    if (r.state === 'mail_failed' || t > (r.expiresAt ?? 0)) { delete db[id]; continue; }
    pending += 1;
  }
  return pending;
}

const equal = (a, b) => {
  const x = Buffer.from(String(a)); const y = Buffer.from(String(b));
  return x.length === y.length && crypto.timingSafeEqual(x, y);
};

// ---- proving a wallet ----------------------------------------------------
//
// The same challenge-and-sign the bridge uses, because a client that already
// knows how to prove itself to the ring should not have to learn a second way
// to prove itself to this. Deliberately not a shared secret: a secret that has
// to ship inside a desktop app is not a secret.

const BASE58 = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';
const SPKI_ED25519_PREFIX = Buffer.from('302a300506032b6570032100', 'hex');

function base58Decode(value) {
  if (typeof value !== 'string' || !value || value.length > 64) return null;
  let big = 0n;
  for (const ch of value) {
    const digit = BASE58.indexOf(ch);
    if (digit < 0) return null;
    big = big * 58n + BigInt(digit);
  }
  const bytes = [];
  while (big > 0n) { bytes.unshift(Number(big & 0xffn)); big >>= 8n; }
  // Leading '1's are leading zero bytes, and a 32-byte key may legitimately
  // start with one — dropping them would silently shorten the key.
  for (const ch of value) { if (ch !== '1') break; bytes.unshift(0); }
  return Buffer.from(bytes);
}

function verifyWalletSignature(wallet, message, signatureBase64) {
  const raw = base58Decode(wallet);
  if (!raw || raw.length !== 32) return false;
  let signature;
  try { signature = Buffer.from(signatureBase64, 'base64'); } catch { return false; }
  if (signature.length !== 64) return false;
  try {
    const key = crypto.createPublicKey({
      key: Buffer.concat([SPKI_ED25519_PREFIX, raw]), format: 'der', type: 'spki',
    });
    return crypto.verify(null, Buffer.from(message, 'utf8'), key, signature);
  } catch { return false; }
}

const CHALLENGE_TTL_MS = 5 * 60_000;
/** `/requests/challenge` has to be open — it is how a client with nothing gets
 *  something to sign — so anyone may make this map grow. The cap turns that
 *  into a refusal rather than into this process's memory. */
const MAX_CHALLENGES = 10_000;
const challenges = new Map();

const messageFor = (wallet, nonce) =>
  `Kvasir admissions — ask to join the ring.\nwallet: ${wallet}\nnonce: ${nonce}`;

function newChallenge(wallet) {
  const t = now();
  for (const [key, entry] of challenges) if (entry.expires < t) challenges.delete(key);
  if (challenges.size >= MAX_CHALLENGES) return null;
  const nonce = crypto.randomBytes(16).toString('hex');
  challenges.set(nonce, { wallet, expires: t + CHALLENGE_TTL_MS });
  return { nonce, message: messageFor(wallet, nonce) };
}

/** Single use: taken whether or not the signature turns out to match, so a
 *  wrong signature cannot be retried against the same nonce. */
function consumeChallenge(wallet, nonce) {
  const entry = challenges.get(nonce);
  if (!entry) return null;
  challenges.delete(nonce);
  if (entry.expires < now() || entry.wallet !== wallet) return null;
  return messageFor(wallet, nonce);
}

// ---- helpers -------------------------------------------------------------

const escape = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => (
  { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));

function send(res, status, body, type = 'application/json') {
  const text = type === 'application/json' ? JSON.stringify(body) : String(body);
  res.writeHead(status, { 'content-type': `${type}; charset=utf-8`, 'content-length': Buffer.byteLength(text) });
  res.end(text);
}

async function readBody(req, limit = 64 * 1024) {
  const chunks = []; let size = 0;
  for await (const chunk of req) {
    size += chunk.length;
    if (size > limit) throw new Error('request body too large');
    chunks.push(chunk);
  }
  return Buffer.concat(chunks).toString('utf8');
}

/** The one thing this service is trusted to do. */
async function admitToRing(wallet, by) {
  const r = await fetch(`${GATE}/api/admissions`, {
    method: 'POST',
    headers: { 'content-type': 'application/json', 'X-Kvasir-Service-Token': RING_TOKEN },
    body: JSON.stringify({ wallet, by }),
    signal: AbortSignal.timeout(15_000),
  });
  const text = await r.text();
  if (!r.ok) throw new Error(`the ring refused the admission (${r.status}): ${text.slice(0, 200)}`);
  return text;
}

async function mailApproval(request) {
  const link = `${PUBLIC_URL}/a/${request.id}?t=${request.token}`;
  const rows = Object.entries(request.profile ?? {})
    .map(([k, v]) => `<tr><td style="padding:4px 14px 4px 0;color:#667;white-space:nowrap">${escape(k)}</td>`
      + `<td style="padding:4px 0;font-family:ui-monospace,monospace">${escape(v)}</td></tr>`).join('');
  const html = `<div style="font-family:system-ui,sans-serif;max-width:560px;line-height:1.55">
<h2 style="margin:0 0 4px">A machine is asking to join the ring</h2>
<p style="margin:0 0 18px;color:#556">Approving adds this wallet to the admitted list. Nothing else changes, and it can be removed later.</p>
<table style="border-collapse:collapse;font-size:14px;margin-bottom:8px">
<tr><td style="padding:4px 14px 4px 0;color:#667">wallet</td>
<td style="padding:4px 0;font-family:ui-monospace,monospace;word-break:break-all">${escape(request.wallet)}</td></tr>
${rows}</table>
<p style="margin:22px 0"><a href="${escape(link)}"
 style="background:#2a78d6;color:#fff;padding:11px 20px;border-radius:6px;text-decoration:none;font-weight:600">Review and approve</a></p>
<p style="color:#889;font-size:13px;margin:0">The link opens a page that shows this request again; approving happens when you press the button there. It is good for ${Math.round(TTL_MS / 60000)} minutes and works once.</p>
<p style="color:#889;font-size:13px">If you did not expect this, ignore it — nothing happens until the button is pressed.</p></div>`;

  const r = await fetch('https://api.resend.com/emails', {
    method: 'POST',
    headers: { authorization: `Bearer ${RESEND_KEY}`, 'content-type': 'application/json' },
    body: JSON.stringify({
      from: FROM, to: [APPROVER], ...(REPLY_TO ? { reply_to: REPLY_TO } : {}),
      subject: `Kvasir: admit ${request.wallet.slice(0, 8)}…?`,
      html,
    }),
    signal: AbortSignal.timeout(15_000),
  });
  if (!r.ok) throw new Error(`Resend refused the message (${r.status}): ${(await r.text()).slice(0, 200)}`);
}

// ---- pages ---------------------------------------------------------------

const page = (title, body) => `<!doctype html><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1"><title>${escape(title)}</title>
<body style="font-family:system-ui,sans-serif;max-width:33rem;margin:6vh auto;padding:0 20px;line-height:1.6;color:#182028">
${body}</body>`;

function reviewPage(request) {
  const rows = Object.entries(request.profile ?? {})
    .map(([k, v]) => `<tr><td style="padding:5px 16px 5px 0;color:#667">${escape(k)}</td>`
      + `<td style="padding:5px 0;font-family:ui-monospace,monospace">${escape(v)}</td></tr>`).join('');
  return page('Admit this machine?', `<h1 style="font-size:1.5rem">Admit this machine?</h1>
<p style="color:#556">Approving adds the wallet below to the ring's admitted list.</p>
<table style="border-collapse:collapse;font-size:14px;margin:18px 0">
<tr><td style="padding:5px 16px 5px 0;color:#667">wallet</td>
<td style="padding:5px 0;font-family:ui-monospace,monospace;word-break:break-all">${escape(request.wallet)}</td></tr>
${rows}</table>
<form method="post" action="/a/${escape(request.id)}">
<input type="hidden" name="t" value="${escape(request.token)}">
<button type="submit" style="background:#2a78d6;color:#fff;border:0;padding:11px 22px;border-radius:6px;font-size:15px;font-weight:600;cursor:pointer">Approve</button>
</form>`);
}

// ---- server --------------------------------------------------------------

const server = http.createServer(async (req, res) => {
  const url = new URL(req.url, 'http://admissions.local');
  const route = url.pathname.replace(/\/+$/, '') || '/';

  try {
    if (req.method === 'GET' && (route === '/health' || route === '/')) {
      return send(res, 200, { ok: true, service: 'kvasir-admissions' });
    }

    // How a client with no secret gets something to sign. Open by necessity.
    if (req.method === 'POST' && route === '/requests/challenge') {
      let body;
      try { body = JSON.parse((await readBody(req)) || '{}'); }
      catch { return send(res, 400, { error: 'request body is not JSON' }); }
      const wallet = String(body.wallet ?? '').trim();
      if (!WALLET_RE.test(wallet)) {
        return send(res, 400, { error: 'wallet must be a base58 Solana address' });
      }
      const challenge = newChallenge(wallet);
      if (!challenge) return send(res, 503, { error: 'too many challenges outstanding; try again shortly' });
      return send(res, 200, challenge);
    }

    // Intake. Two ways to be allowed to ask, for two kinds of asker.
    //
    // The bot speaks for whoever messaged it and has no wallet of its own, so
    // it presents a shared secret. An app has the wallet in its hands and no
    // safe place to keep a secret — one shipped inside a desktop build is not a
    // secret — so it signs a challenge instead. Neither grants admission;
    // both only put a request in front of a person.
    if (req.method === 'POST' && route === '/requests') {
      let body;
      try { body = JSON.parse((await readBody(req)) || '{}'); }
      catch { return send(res, 400, { error: 'request body is not JSON' }); }

      const wallet = String(body.wallet ?? '').trim();
      if (!WALLET_RE.test(wallet)) {
        return send(res, 400, { error: 'wallet must be a base58 Solana address' });
      }

      const presented = (req.headers['x-admissions-token'] ?? '').toString();
      if (!equal(presented, INTAKE_TOKEN)) {
        const nonce = String(body.nonce ?? '').trim();
        const signature = String(body.signature ?? '');
        const message = nonce && signature ? consumeChallenge(wallet, nonce) : null;
        if (!message || !verifyWalletSignature(wallet, message, signature)) {
          return send(res, 401, {
            error: 'prove the wallet: POST /requests/challenge, sign the message, '
              + 'and send {wallet, nonce, signature} — or present the intake token',
          });
        }
      }

      const db = load();
      const pending = prune(db);

      // Asking twice about one machine must not put a second copy of the same
      // decision in front of the approver — two mails, one of which silently
      // stops working when the other is used, is how a person learns to ignore
      // both. The live request is returned instead.
      const waiting = Object.values(db).find(
        (r) => r.state === 'pending' && r.wallet === wallet && now() <= r.expiresAt);
      if (waiting) {
        save(db);
        console.log(`admissions: ${wallet} already waiting (${waiting.id})`);
        return send(res, 200, { ok: true, id: waiting.id, expiresAt: waiting.expiresAt, reused: true });
      }

      if (pending >= MAX_PENDING) {
        save(db);
        console.warn(`admissions: refused ${wallet} — ${pending} requests already waiting`);
        return send(res, 429, {
          error: `too many requests are already waiting (${pending}); try again once some are answered or expire`,
        });
      }

      const id = crypto.randomUUID();
      const request = {
        id,
        wallet,
        profile: body.profile && typeof body.profile === 'object' ? body.profile : {},
        requestedBy: String(body.requestedBy ?? '').slice(0, 120),
        token: crypto.randomBytes(32).toString('base64url'),
        createdAt: now(),
        expiresAt: now() + TTL_MS,
        state: 'pending',
      };
      db[id] = request;
      save(db);

      try { await mailApproval(request); }
      catch (e) {
        request.state = 'mail_failed';
        request.error = e.message;
        save(db);
        return send(res, 502, { error: e.message, id });
      }
      console.log(`admissions: asked about ${wallet} (${id})`);
      return send(res, 200, { ok: true, id, expiresAt: request.expiresAt });
    }

    const match = /^\/a\/([0-9a-f-]{36})$/.exec(route);
    if (match) {
      const db = load();
      const request = db[match[1]];
      if (!request) return send(res, 404, page('Not found', '<h1>No such request</h1>'), 'text/html');

      const token = req.method === 'GET'
        ? (url.searchParams.get('t') ?? '')
        : new URLSearchParams(await readBody(req)).get('t') ?? '';
      if (!equal(token, request.token)) {
        return send(res, 403, page('Not valid', '<h1>That link is not valid</h1>'), 'text/html');
      }
      if (request.state === 'approved') {
        return send(res, 200, page('Already approved',
          `<h1>Already approved</h1><p style="color:#556">This machine was admitted on ${new Date(request.approvedAt).toISOString()}. Nothing more to do.</p>`), 'text/html');
      }
      if (now() > request.expiresAt) {
        return send(res, 410, page('Expired',
          '<h1>That link has expired</h1><p style="color:#556">Ask the machine to send its request again.</p>'), 'text/html');
      }

      // A GET only ever renders. Whatever fetched it — a person, a scanner, a
      // preview — leaves the request exactly as it found it.
      if (req.method === 'GET') return send(res, 200, reviewPage(request), 'text/html');

      if (req.method === 'POST') {
        try { await admitToRing(request.wallet, `approved by ${APPROVER}`); }
        catch (e) {
          console.error(`admissions: ${request.wallet} not admitted — ${e.message}`);
          return send(res, 502, page('Not admitted',
            `<h1>The ring did not accept it</h1><p style="color:#556">${escape(e.message)}</p>`
            + '<p style="color:#556">Nothing was changed. The link still works; try again once the ring is reachable.</p>'), 'text/html');
        }
        request.state = 'approved';
        request.approvedAt = now();
        save(db);
        console.log(`admissions: admitted ${request.wallet}`);
        return send(res, 200, page('Approved',
          `<h1>Approved</h1><p style="color:#556">The wallet is on the admitted list. The machine joins on its next request — it does not need to be restarted.</p>
<p style="font-family:ui-monospace,monospace;font-size:13px;word-break:break-all;color:#667">${escape(request.wallet)}</p>`), 'text/html');
      }
    }

    return send(res, 404, { error: `no such endpoint: ${req.method} ${route}` });
  } catch (e) {
    console.error(`admissions: ${e.message}`);
    return send(res, 500, { error: 'internal error' });
  }
});

server.listen(PORT, '0.0.0.0', () => {
  console.log(`kvasir-admissions on :${PORT}`);
  console.log(`  approver ${APPROVER}`);
  console.log(`  from     ${FROM}`);
  console.log(`  ring     ${GATE}/api/admissions`);
  console.log(`  links    ${PUBLIC_URL}/a/<id>`);
});

export { server };
