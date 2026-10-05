'use strict';
/**
 * Forward one OpenAI chat completion to the bridge and relay the answer.
 *
 * Moved out of server.js so it can be tested without a wallet, a chain or a
 * keypair: everything it needs from the gateway is passed in.
 *
 * Not every non-2xx from the bridge is an outage. ringOutage() reloads the
 * ring, which cuts every user on it, and tells the caller to retry in a
 * minute — right for a ring that is down, and exactly wrong for:
 *
 *   4xx              the request itself (400 context_length_exceeded, say).
 *                    Retrying it in 60 s fails again; reloading fixes nothing.
 *   503 ring_busy    the ring is healthy and full. A reload would empty it.
 *
 * Those are passed through with their status, body and Retry-After, and
 * nothing is debited, because nothing was generated. Everything else keeps
 * the outage path it had.
 */

/** 'client', 'busy' or 'outage', for a bridge response that was not 2xx. */
function classifyFailure(status, raw) {
  if (status >= 400 && status < 500) return 'client';
  if (status === 503) {
    try {
      if (JSON.parse(raw)?.error?.type === 'ring_busy') return 'busy';
    } catch { /* not JSON: an outage */ }
  }
  return 'outage';
}

/** Send the bridge's own answer on, unchanged. Works on Express and bare http. */
function passThrough(res, upstream, raw) {
  res.statusCode = upstream.status;
  res.setHeader('Content-Type', upstream.headers.get('content-type') || 'application/json; charset=utf-8');
  const retryAfter = upstream.headers.get('retry-after');
  if (retryAfter) res.setHeader('Retry-After', retryAfter);
  res.end(raw);
}

// A bridge "ring down" shows up either as a non-2xx, or as a 200 SSE whose FIRST
// chunk is a data:{"error":...} — treat both as an outage.
function sseFirstError(text) {
  for (const line of String(text || '').split('\n')) {
    const t = line.trim();
    if (!t.startsWith('data:')) continue;
    const p = t.slice(5).trim();
    if (p === '[DONE]') return null;
    try { const o = JSON.parse(p); return o && o.error ? (o.error.message || 'upstream error') : null; } catch { return null; }
  }
  return null;
}

/**
 * @param {object} o
 * @param {import('http').ServerResponse} o.res
 * @param {{bridgeUrl:string,cid:string}} o.m the resolved model
 * @param {object} o.fwd the body to send the bridge
 * @param {boolean} o.wantStream
 * @param {object} o.headers request headers for the bridge
 * @param {(res, m, detail:string)=>any} o.ringOutage
 * @param {(usage:object)=>void} o.debit
 * @param {()=>void} o.markOk
 * @param {typeof fetch} [o.fetchImpl]
 */
async function forwardCompletion({ res, m, fwd, wantStream, headers, ringOutage, debit, markOk, fetchImpl = fetch }) {
  let upstream;
  try {
    upstream = await fetchImpl(`${m.bridgeUrl}/c/${m.cid}/v1/chat/completions`, {
      method: 'POST', headers, body: JSON.stringify(fwd), signal: AbortSignal.timeout(300000),
    });
  } catch (e) { return ringOutage(res, m, `upstream unreachable: ${e.message}`); }

  // The bridge refuses before it streams (its queue and its size check both
  // answer JSON with a status), so one check serves both paths.
  if (!upstream.ok) {
    const raw = await upstream.text().catch(() => '');
    if (classifyFailure(upstream.status, raw) !== 'outage') return passThrough(res, upstream, raw);
    return ringOutage(res, m, raw || `upstream ${upstream.status}`);
  }

  if (!wantStream) {
    const raw = await upstream.text().catch(() => '');
    let d; try { d = JSON.parse(raw); } catch { return ringOutage(res, m, 'non-JSON upstream response'); }
    if (!d || !d.choices) return ringOutage(res, m, 'upstream returned no choices');
    markOk();
    if (d.usage) debit(d.usage);
    res.statusCode = 200;
    res.setHeader('Content-Type', 'application/json; charset=utf-8');
    return res.end(JSON.stringify(d));
  }

  if (!upstream.body) return ringOutage(res, m, `upstream ${upstream.status} with no body`);
  // Peek the first chunk: a down ring answers 200 with data:{"error":...}. Catch
  // it BEFORE committing SSE headers so we can return a typed 503 + auto-reload.
  const reader = upstream.body.getReader();
  const dec = new TextDecoder();
  let first;
  try { first = await reader.read(); } catch (e) { return ringOutage(res, m, `stream read: ${e.message}`); }
  const firstText = first && first.value ? dec.decode(first.value, { stream: true }) : '';
  const errMsg = sseFirstError(firstText);
  if (errMsg) return ringOutage(res, m, errMsg);

  res.statusCode = 200;
  res.setHeader('Content-Type', 'text/event-stream; charset=utf-8');
  res.setHeader('Cache-Control', 'no-cache');
  res.setHeader('Connection', 'keep-alive');
  let usage = null, buf = '';
  const scan = (text) => {
    buf += text;
    let nl;
    while ((nl = buf.indexOf('\n')) >= 0) {
      const line = buf.slice(0, nl).trim(); buf = buf.slice(nl + 1);
      if (!line.startsWith('data:')) continue;
      const payload = line.slice(5).trim();
      if (payload === '[DONE]') continue;
      try { const obj = JSON.parse(payload); if (obj && obj.usage) usage = obj.usage; } catch { /* partial */ }
    }
  };
  try {
    if (firstText) { res.write(firstText); scan(firstText); }
    if (!(first && first.done)) {
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        const text = dec.decode(value, { stream: true });
        res.write(text);
        scan(text);
      }
    }
  } catch { /* client/stream aborted */ }
  markOk();
  res.end();
  if (usage) debit(usage);
  return undefined;
}

module.exports = { forwardCompletion, classifyFailure, sseFirstError };
