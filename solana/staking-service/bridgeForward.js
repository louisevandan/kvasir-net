'use strict';
/**
 * Forward one OpenAI chat completion to the bridge and relay the answer.
 *
 * Moved out of server.js so it can be tested without a wallet, a chain or a
 * keypair: everything it needs from the gateway is passed in.
 *
 * Not every failure is an outage. ringOutage() reloads the ring, which cuts
 * every user on it, and tells the caller to retry in a minute — right for a
 * ring that is down, wrong for most of what else can go wrong:
 *
 *   400 413 422 429      the request itself (400 context_length_exceeded, say).
 *   503 ring_busy        the ring is healthy and full.
 *                        → passed through: status, body, Retry-After.
 *   401 403              the gateway's own service token is wrong. Telling the
 *                        caller "unauthorized" would make them doubt a key that
 *                        is fine, and a reload fixes nothing.
 *                        → 502 gateway_misconfigured, logged loudly.
 *   404                  the bridge does not know this controller: a reload, or
 *                        a controller id that changed. The pool is read live
 *                        from the bridges on every request (resolveModels has
 *                        no cache), so the next request already sees the new
 *                        list and there is nothing to invalidate; what is left
 *                        is the outage path it always had.
 *                        → ringOutage, as before.
 *   other 4xx            the bridge answered something this gateway does not
 *                        expect. → 502 bad_upstream_response.
 *   our own timeout      the bridge was slow, not down: it may queue a request
 *                        75 s and then generate for up to 300 s.
 *                        → 504 upstream_timeout; mid-stream, an error frame.
 *   no response at all   (refused, reset, unreachable) → ringOutage.
 *
 * A stream is relayed byte for byte, SSE comments included: the bridge sends
 * `: keepalive` through a long prefill so Cloudflare, in front of this gateway,
 * does not cut a reply that is merely slow. scan() reads only `data:` lines, so
 * a comment is never mistaken for a frame, and only a usage frame is debited.
 *
 * Billing follows who ended the reply. A complete answer is debited, even when
 * the caller hung up before the last frame: the stream is still read to its
 * end for the usage, or anyone could cut just before it and pay nothing. A
 * reply the upstream side cut short (our deadline, a broken bridge stream) is
 * not debited and does not mark the ring healthy; whether partial output
 * should be billed is a policy question left open, and the cut is logged.
 */

/**
 * How long the gateway waits for one completion, end to end. It has to cover
 * the bridge's queue wait (queue_timeout_s, 75; it was 120) plus its request timeout
 * (request_timeout_s, 300) plus a margin, or a request the bridge is still
 * honestly serving is cut off here and reported as something it is not.
 */
const BRIDGE_TIMEOUT_MS = Number(process.env.KVR_BRIDGE_TIMEOUT_MS || 450_000);

const PASS_THROUGH = new Set([400, 413, 422, 429]);

/**
 * What a non-2xx bridge answer is: 'pass', 'busy', 'misconfigured',
 * 'missing', 'unexpected' or 'outage'.
 */
function classifyFailure(status, raw) {
  if (PASS_THROUGH.has(status)) return 'pass';
  if (status === 401 || status === 403) return 'misconfigured';
  if (status === 404) return 'missing';
  if (status >= 400 && status < 500) return 'unexpected';
  if (status === 503) {
    try {
      if (JSON.parse(raw)?.error?.type === 'ring_busy') return 'busy';
    } catch { /* not JSON: an outage */ }
  }
  return 'outage';
}

/** Works on Express and on a bare http.ServerResponse. */
function sendJson(res, status, body, headers = {}) {
  res.statusCode = status;
  res.setHeader('Content-Type', 'application/json; charset=utf-8');
  for (const [name, value] of Object.entries(headers)) res.setHeader(name, value);
  res.end(JSON.stringify(body));
}

/** Send the bridge's own answer on, unchanged. */
function passThrough(res, upstream, raw) {
  res.statusCode = upstream.status;
  res.setHeader('Content-Type', upstream.headers.get('content-type') || 'application/json; charset=utf-8');
  const retryAfter = upstream.headers.get('retry-after');
  if (retryAfter) res.setHeader('Retry-After', retryAfter);
  res.end(raw);
}

const timedOut = (res, ms) => sendJson(res, 504, { error: {
  message: `the model did not finish within ${Math.round(ms / 1000)} s; the ring is up but busy — retry later or ask for fewer tokens`,
  type: 'upstream_timeout', code: 'upstream_timeout',
} });

/**
 * The Anthropic surface's error type for an OpenAI-route failure. 401 means
 * the caller's key; a gateway_misconfigured 502 must not, or a user with a
 * good key goes looking for a problem that is ours.
 */
function anthropicErrorType(status, body) {
  const type = body?.error?.type;
  if (type === 'ring_busy' || status === 429) return 'overloaded_error';
  if (type === 'gateway_misconfigured') return 'api_error';
  if (status === 401) return 'authentication_error';
  if (status === 403) return 'permission_error';
  if (status === 400 || status === 413 || status === 422) return 'invalid_request_error';
  return 'api_error';
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
 * @param {number} [o.timeoutMs] end-to-end limit; BRIDGE_TIMEOUT_MS by default
 * @param {(line:string)=>void} [o.log]
 * @param {typeof fetch} [o.fetchImpl]
 */
async function forwardCompletion({
  res, m, fwd, wantStream, headers, ringOutage, debit, markOk,
  timeoutMs = BRIDGE_TIMEOUT_MS, log = (line) => console.error(line), fetchImpl = fetch,
}) {
  const deadline = AbortSignal.timeout(timeoutMs);
  let upstream;
  try {
    upstream = await fetchImpl(`${m.bridgeUrl}/c/${m.cid}/v1/chat/completions`, {
      method: 'POST', headers, body: JSON.stringify(fwd), signal: deadline,
    });
  } catch (e) {
    if (deadline.aborted) return timedOut(res, timeoutMs);
    return ringOutage(res, m, `upstream unreachable: ${e.message}`);
  }

  // The rest of a body; null when our own deadline cut it off.
  const readAll = async () => {
    try { return await upstream.text(); } catch { return deadline.aborted ? null : ''; }
  };

  // The bridge refuses before it streams (its queue and its size check both
  // answer JSON with a status), so one check serves both paths.
  if (!upstream.ok) {
    const raw = await readAll();
    if (raw === null) return timedOut(res, timeoutMs);
    const kind = classifyFailure(upstream.status, raw);
    if (kind === 'pass' || kind === 'busy') return passThrough(res, upstream, raw);
    if (kind === 'misconfigured') {
      log(`GATEWAY MISCONFIGURED: bridge ${m.bridgeUrl} answered ${upstream.status} to the gateway's service token `
        + '— P4_BRIDGE_TOKEN here does not match P4_BRIDGE_TOKEN on the bridge');
      return sendJson(res, 502, { error: {
        message: 'the gateway could not authenticate to the model server; this is an operator problem, not your key',
        type: 'gateway_misconfigured', code: 'gateway_misconfigured',
      } });
    }
    if (kind === 'unexpected') {
      return sendJson(res, 502, { error: {
        message: `the model server answered ${upstream.status}, which the gateway does not expect`,
        type: 'bad_upstream_response', code: 'bad_upstream_response', detail: String(raw).slice(0, 200),
      } });
    }
    return ringOutage(res, m, raw || `upstream ${upstream.status}`);
  }

  if (!wantStream) {
    const raw = await readAll();
    if (raw === null) return timedOut(res, timeoutMs);
    let d; try { d = JSON.parse(raw); } catch { return ringOutage(res, m, 'non-JSON upstream response'); }
    if (!d || !d.choices) return ringOutage(res, m, 'upstream returned no choices');
    markOk();
    if (d.usage) debit(d.usage);
    return sendJson(res, 200, d);
  }

  if (!upstream.body) return ringOutage(res, m, `upstream ${upstream.status} with no body`);
  // Peek the first chunk: a down ring answers 200 with data:{"error":...}. Catch
  // it BEFORE committing SSE headers so we can return a typed 503 + auto-reload.
  const reader = upstream.body.getReader();
  const dec = new TextDecoder();
  let first;
  try { first = await reader.read(); } catch (e) {
    if (deadline.aborted) return timedOut(res, timeoutMs);
    return ringOutage(res, m, `stream read: ${e.message}`);
  }
  const firstText = first && first.value ? dec.decode(first.value, { stream: true }) : '';
  const errMsg = sseFirstError(firstText);
  if (errMsg) return ringOutage(res, m, errMsg);

  res.statusCode = 200;
  res.setHeader('Content-Type', 'text/event-stream; charset=utf-8');
  res.setHeader('Cache-Control', 'no-cache');
  res.setHeader('Connection', 'keep-alive');
  let usage = null, buf = '', requestId = null, bytes = 0, frames = 0;
  const scan = (text) => {
    buf += text;
    let nl;
    while ((nl = buf.indexOf('\n')) >= 0) {
      const line = buf.slice(0, nl).trim(); buf = buf.slice(nl + 1);
      if (!line.startsWith('data:')) continue;
      const payload = line.slice(5).trim();
      if (payload === '[DONE]') continue;
      try {
        const obj = JSON.parse(payload);
        frames += 1;
        if (obj && obj.id && !requestId) requestId = obj.id;
        if (obj && obj.usage) usage = obj.usage;
      } catch { /* partial */ }
    }
  };
  // A caller that hangs up does not stop the read: see the header comment.
  const write = (text) => {
    if (res.destroyed || res.writableEnded) return;
    try { res.write(text); } catch { /* the client went away */ }
  };
  const take = (text) => { bytes += Buffer.byteLength(text); write(text); scan(text); };
  if (firstText) take(firstText);
  if (!(first && first.done)) {
    for (;;) {
      let chunk;
      try {
        chunk = await reader.read();
      } catch (e) {
        // Cut off upstream of the caller: by our deadline, or the bridge's
        // stream broke. Not a complete answer, so not billed and not proof
        // of a healthy ring.
        const type = deadline.aborted ? 'upstream_timeout' : 'upstream_interrupted';
        log(`stream cut (${type}) request=${requestId ?? '?'} bridge=${m.bridgeUrl} model=${m.cid} `
          + `bytes=${bytes} frames=${frames}: ${e.message}; not debited`);
        write(`data: ${JSON.stringify({ error: { message: deadline.aborted
          ? `the model did not finish within ${Math.round(timeoutMs / 1000)} s`
          : `the model server stream broke: ${e.message}`, type, code: type } })}\n\n`);
        if (!res.writableEnded) res.end();
        return undefined;
      }
      if (chunk.done) break;
      take(dec.decode(chunk.value, { stream: true }));
    }
  }
  markOk();
  if (!res.writableEnded) res.end();
  if (usage) debit(usage);
  return undefined;
}

module.exports = { forwardCompletion, classifyFailure, anthropicErrorType, sseFirstError, BRIDGE_TIMEOUT_MS };
