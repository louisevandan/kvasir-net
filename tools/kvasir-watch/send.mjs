#!/usr/bin/env node
/**
 * Put the day's report in the Telegram group.
 *
 * Two parts, deliberately: the summary goes in as a message, because a report
 * nobody opens is a report nobody reads, and the full page goes in as an
 * attachment for whoever wants the detail.
 *
 * Credentials come from the environment (TELEGRAM_BOT_TOKEN, TELEGRAM_CHAT_ID)
 * and are never printed — not in an error, not in a log line.
 */
import { readFileSync, existsSync } from 'node:fs';
import path from 'node:path';
import { setDefaultResultOrder } from 'node:dns';
import net from 'node:net';

// The fleet hosts have no IPv6 default route, but DNS answers with an AAAA for
// api.telegram.org anyway. Node picks that address and the connection dies as a
// bare `fetch failed` with no status — while curl, which tries both families,
// succeeds every time. Ordering v4 first avoids it; autoSelectFamily makes the
// runtime fall back instead of failing if a v6 address is ever picked again.
try { setDefaultResultOrder('ipv4first'); } catch { /* older runtimes */ }
try { net.setDefaultAutoSelectFamily(true); } catch { /* older runtimes */ }

const token = (process.env.TELEGRAM_BOT_TOKEN ?? '').trim();
const chatId = (process.env.TELEGRAM_CHAT_ID ?? '').trim();
if (!token || !chatId) {
  console.error('TELEGRAM_BOT_TOKEN and TELEGRAM_CHAT_ID must be set');
  process.exit(2);
}

const summaryFile = process.argv[2];
const htmlFile = process.argv[3];
const summary = summaryFile && summaryFile !== '-' ? readFileSync(summaryFile, 'utf8') : readFileSync(0, 'utf8');

const api = (method) => `https://api.telegram.org/bot${token}/${method}`;

/** Telegram rejects a message over 4096 characters; keep the head, say so. */
function clamp(text, limit = 3900) {
  if (text.length <= limit) return text;
  return `${text.slice(0, limit)}\n… truncated; the full report is attached`;
}

async function call(method, body, isForm = false, attempt = 1) {
  let response;
  try {
    response = await fetch(api(method), isForm ? { method: 'POST', body } : {
      method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body),
    });
  } catch (error) {
    // Reaching Telegram at all. The form body is a Blob built from memory, not
    // a stream, so undici re-serializes it on each attempt and the attachment
    // retry actually re-sends something.
    if (attempt < 3) {
      await new Promise((r) => setTimeout(r, attempt * 1500));
      return call(method, body, isForm, attempt + 1);
    }
    throw new Error(`${method} could not reach Telegram after ${attempt} tries: ${error.cause?.code ?? error.message}`);
  }
  const result = await response.json().catch(() => ({}));
  // A rate limit is Telegram telling us when to come back, not a refusal.
  if (response.status === 429 && attempt < 3) {
    await new Promise((r) => setTimeout(r, ((result.parameters?.retry_after ?? 2) + 1) * 1000));
    return call(method, body, isForm, attempt + 1);
  }
  // Telegram echoes the request; never let that reach a log that holds a token.
  if (!result.ok) throw new Error(`${method} failed: ${result.description ?? response.status}`);
  return result.result;
}

await call('sendMessage', { chat_id: chatId, text: clamp(summary), disable_web_page_preview: true });

if (htmlFile && existsSync(htmlFile)) {
  const form = new FormData();
  form.set('chat_id', chatId);
  form.set('document', new Blob([readFileSync(htmlFile)], { type: 'text/html' }), path.basename(htmlFile));
  await call('sendDocument', form, true);
}

console.log('sent');
