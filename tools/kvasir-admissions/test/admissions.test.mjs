import test from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

/** A stand-in for Resend and for the ring, so a test never reaches either. */
function fakes() {
  const mails = []; const admitted = []; let ringFails = false;
  const server = http.createServer(async (req, res) => {
    const chunks = []; for await (const c of req) chunks.push(c);
    const body = JSON.parse(Buffer.concat(chunks).toString('utf8') || '{}');
    if (req.url === '/emails') { mails.push(body); return res.writeHead(200).end('{}'); }
    if (req.url === '/api/admissions') {
      if (ringFails) return res.writeHead(503).end('{"error":"ring down"}');
      admitted.push(body); return res.writeHead(200).end('{"ok":true}');
    }
    res.writeHead(404).end('{}');
  });
  return { server, mails, admitted, failRing: (v) => { ringFails = v; } };
}

const listen = (s) => new Promise((r) => s.listen(0, '127.0.0.1', () => r(s.address().port)));

async function boot(t, extraEnv = {}) {
  const f = fakes();
  const fakePort = await listen(f.server);
  const state = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'adm-')), 'requests.json');
  Object.assign(process.env, {
    PORT: '0', ADMISSIONS_STATE: state, ADMISSIONS_APPROVER: 'approver@example.com',
    RESEND_API_KEY: 'test-key', P4_BRIDGE_TOKEN: 'ring-secret', ADMISSIONS_INTAKE_TOKEN: 'intake-secret',
    KVASIR_GATE_URL: `http://127.0.0.1:${fakePort}`, ADMISSIONS_PUBLIC_URL: 'http://localhost',
    ADMISSIONS_TTL_MINUTES: '30', ADMISSIONS_MAX_PENDING: '50', ADMISSIONS_KEEP_APPROVED_DAYS: '30',
    ...extraEnv,
  });
  // Resend is reached by absolute URL, so point fetch at the fake for that host.
  const realFetch = globalThis.fetch;
  globalThis.fetch = (url, init) => realFetch(
    String(url).startsWith('https://api.resend.com') ? `http://127.0.0.1:${fakePort}/emails` : url, init);

  const { server } = await import(`../server.mjs?${Date.now()}`);
  const port = await new Promise((r) => (server.listening ? r(server.address().port)
    : server.once('listening', () => r(server.address().port))));
  t.after(() => { server.close(); f.server.close(); globalThis.fetch = realFetch; });
  return { ...f, port };
}

const call = async (port, method, p, { body, form, headers = {} } = {}) => {
  const r = await fetch(`http://127.0.0.1:${port}${p}`, {
    method,
    headers: { ...(body ? { 'content-type': 'application/json' } : {}),
      ...(form ? { 'content-type': 'application/x-www-form-urlencoded' } : {}), ...headers },
    body: body ? JSON.stringify(body) : form,
  });
  return { status: r.status, text: await r.text() };
};

const WALLET = '8MoUM6soLHjyeYT1trrWKboABvbGaQjtWTwGMdV5BhtL';

test('a request mails a link, and only the button admits', async (t) => {
  const { port, mails, admitted } = await boot(t);

  const made = await call(port, 'POST', '/requests', {
    headers: { 'x-admissions-token': 'intake-secret' },
    body: { wallet: WALLET, profile: { gpus: 'RTX 6000 Ada', lending: '24 GiB' } },
  });
  assert.equal(made.status, 200);
  assert.equal(mails.length, 1);
  assert.match(mails[0].html, /RTX 6000 Ada/);

  const link = /href="([^"]*\/a\/[^"]+)"/.exec(mails[0].html)[1].replace(/&amp;/g, '&');
  const url = new URL(link);

  // What a mail scanner does. It must change nothing.
  const peeked = await call(port, 'GET', url.pathname + url.search);
  assert.equal(peeked.status, 200);
  assert.match(peeked.text, /Approve/);
  assert.equal(admitted.length, 0, 'a GET must not admit');

  const token = new URLSearchParams(url.search).get('t');
  const pressed = await call(port, 'POST', url.pathname, { form: new URLSearchParams({ t: token }).toString() });
  assert.equal(pressed.status, 200);
  assert.equal(admitted.length, 1);
  assert.equal(admitted[0].wallet, WALLET);

  // Pressing again is not a second admission.
  const again = await call(port, 'POST', url.pathname, { form: new URLSearchParams({ t: token }).toString() });
  assert.match(again.text, /Already approved/);
  assert.equal(admitted.length, 1);
});

test('intake needs its token, and the wallet must look like one', async (t) => {
  const { port, mails } = await boot(t);
  const noToken = await call(port, 'POST', '/requests', { body: { wallet: WALLET } });
  assert.equal(noToken.status, 401);
  const wrongToken = await call(port, 'POST', '/requests', {
    headers: { 'x-admissions-token': 'guess' }, body: { wallet: WALLET } });
  assert.equal(wrongToken.status, 401);
  const notAWallet = await call(port, 'POST', '/requests', {
    headers: { 'x-admissions-token': 'intake-secret' }, body: { wallet: 'nope' } });
  assert.equal(notAWallet.status, 400);
  assert.equal(mails.length, 0);
});

test('a wrong token approves nothing, and a ring refusal leaves the link usable', async (t) => {
  const { port, mails, admitted, failRing } = await boot(t);
  await call(port, 'POST', '/requests', {
    headers: { 'x-admissions-token': 'intake-secret' }, body: { wallet: WALLET } });
  const link = /href="([^"]*\/a\/[^"]+)"/.exec(mails[0].html)[1].replace(/&amp;/g, '&');
  const url = new URL(link);
  const token = new URLSearchParams(url.search).get('t');

  const forged = await call(port, 'POST', url.pathname, { form: 't=not-the-token' });
  assert.equal(forged.status, 403);
  assert.equal(admitted.length, 0);

  failRing(true);
  const refused = await call(port, 'POST', url.pathname, { form: new URLSearchParams({ t: token }).toString() });
  assert.equal(refused.status, 502);
  assert.equal(admitted.length, 0);

  // The approval was not consumed by a failure that was not the approver's.
  failRing(false);
  const retried = await call(port, 'POST', url.pathname, { form: new URLSearchParams({ t: token }).toString() });
  assert.equal(retried.status, 200);
  assert.equal(admitted.length, 1);
});

test('asking twice about one machine does not mail twice', async (t) => {
  const { port, mails } = await boot(t);
  const ask = () => call(port, 'POST', '/requests', {
    headers: { 'x-admissions-token': 'intake-secret' }, body: { wallet: WALLET } });

  const first = JSON.parse((await ask()).text);
  const second = JSON.parse((await ask()).text);

  assert.equal(mails.length, 1, 'the approver sees one decision, not two');
  assert.equal(second.id, first.id);
  assert.equal(second.reused, true);
});

test('the approver cannot be buried: waiting requests are capped', async (t) => {
  const { port, mails } = await boot(t, { ADMISSIONS_MAX_PENDING: '2' });
  const wallets = [
    '8MoUM6soLHjyeYT1trrWKboABvbGaQjtWTwGMdV5BhtL',
    'EDJ7XqLSDNkWnNHt3SrbjGjdaiT1ktgWNm23hVN1Kvmu',
    '3oEmoqj33egWUVNqcVvuGxMs6xnE1LTLmVeZ7Q8pxRLq',
  ];
  const results = [];
  for (const wallet of wallets) {
    results.push(await call(port, 'POST', '/requests', {
      headers: { 'x-admissions-token': 'intake-secret' }, body: { wallet } }));
  }
  assert.deepEqual(results.map((r) => r.status), [200, 200, 429]);
  assert.equal(mails.length, 2, 'the refused request must not send mail');
  assert.match(JSON.parse(results[2].text).error, /already waiting/);
});

test('an expired request stops occupying the queue', async (t) => {
  // 0.01 minutes: the link is dead almost immediately, which is what a request
  // nobody answered looks like by the time the next one arrives.
  const { port, mails } = await boot(t, { ADMISSIONS_TTL_MINUTES: '0.01', ADMISSIONS_MAX_PENDING: '1' });
  const first = await call(port, 'POST', '/requests', {
    headers: { 'x-admissions-token': 'intake-secret' }, body: { wallet: WALLET } });
  assert.equal(first.status, 200);

  await new Promise((r) => setTimeout(r, 700));

  // Same cap, but the slot is free again because the first one can no longer
  // be acted on — and a second wallet is not held up by a dead request.
  const second = await call(port, 'POST', '/requests', {
    headers: { 'x-admissions-token': 'intake-secret' },
    body: { wallet: 'EDJ7XqLSDNkWnNHt3SrbjGjdaiT1ktgWNm23hVN1Kvmu' } });
  assert.equal(second.status, 200);
  assert.equal(mails.length, 2);
  assert.notEqual(JSON.parse(second.text).id, JSON.parse(first.text).id);
});
