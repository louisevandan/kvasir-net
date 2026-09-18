#!/usr/bin/env node
/**
 * Keep what the group said.
 *
 * Decisions happen in chat — who is applying where, what was submitted, who is
 * chasing whom — and then they are gone, or at least unsearchable. This polls
 * the bot's update feed and appends each message to a monthly JSONL file, so
 * the rest of the tooling can read the group's own record instead of asking
 * someone to remember.
 *
 * Two things to know before expecting it to work.
 *
 * **Privacy mode.** A bot added to a group sees only messages that mention it,
 * unless privacy mode is turned off in @BotFather (Bot Settings → Group
 * Privacy → Turn off) and the bot is then removed from the group and added
 * again. Until that is done this collects the handful of messages addressed to
 * it and nothing else, and says so rather than looking broken.
 *
 * **One reader.** `getUpdates` hands each update to whoever asks first and then
 * forgets it. Only this process may poll; anything else that calls getUpdates
 * with the same token will eat messages this never sees. The offset is
 * advanced only after the batch is on disk, so a crash re-reads rather than
 * drops.
 */
import { readFileSync, writeFileSync, appendFileSync, mkdirSync, existsSync } from 'node:fs';
import path from 'node:path';
import { setDefaultResultOrder } from 'node:dns';
import net from 'node:net';
import { answerQuestions } from './answer.mjs';

// The fleet hosts have no IPv6 default route, but DNS answers with an AAAA for
// api.telegram.org anyway. Node picks that address and the connection dies as a
// bare `fetch failed` with no status — while curl, which tries both families,
// succeeds every time. Ordering v4 first avoids it; autoSelectFamily makes the
// runtime fall back instead of failing if a v6 address is ever picked again.
try { setDefaultResultOrder('ipv4first'); } catch { /* older runtimes */ }
try { net.setDefaultAutoSelectFamily(true); } catch { /* older runtimes */ }

const HERE = path.dirname(new URL(import.meta.url).pathname);
const STATE_DIR = path.join(HERE, 'state');
const STATE_FILE = path.join(STATE_DIR, 'telegram.json');
const ARCHIVE_DIR = process.env.KVASIR_CHAT_ARCHIVE ?? path.join(HERE, 'archive');

const token = (process.env.TELEGRAM_BOT_TOKEN ?? '').trim();
if (!token) { console.error('TELEGRAM_BOT_TOKEN must be set'); process.exit(2); }
const onlyChat = (process.env.TELEGRAM_CHAT_ID ?? '').trim();

const state = existsSync(STATE_FILE) ? JSON.parse(readFileSync(STATE_FILE, 'utf8')) : {};

async function api(method, params = {}, attempt = 1) {
  const url = `https://api.telegram.org/bot${token}/${method}`;
  let response;
  try {
    response = await fetch(url, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(params),
      signal: AbortSignal.timeout(70_000),
    });
  } catch (error) {
    // A bare "fetch failed" with no status is the address-family problem above,
    // not Telegram refusing anything, so retry rather than report a refusal
    // that never happened.
    if (attempt < 3) {
      await new Promise((r) => setTimeout(r, attempt * 1500));
      return api(method, params, attempt + 1);
    }
    throw new Error(`${method} could not reach Telegram after ${attempt} tries: ${error.cause?.code ?? error.message}`);
  }
  const result = await response.json().catch(() => ({}));
  // 409 is the single-reader rule being broken, and it is worth naming: two
  // pollers on one token take turns eating each other's messages, and the
  // symptom is a chat that looks half-recorded rather than an error anyone
  // connects to a second process.
  if (response.status === 409) {
    throw new Error('another process is polling getUpdates with this token — only one reader may');
  }
  // A rate limit is Telegram telling us when to come back, not a refusal.
  if (response.status === 429 && attempt < 3) {
    await new Promise((r) => setTimeout(r, ((result.parameters?.retry_after ?? 2) + 1) * 1000));
    return api(method, params, attempt + 1);
  }
  // Never let Telegram's echo of the request reach a log that holds the token.
  if (!result.ok) throw new Error(`${method} failed: ${result.description ?? response.status}`);
  return result.result;
}

/** One line per message: enough to read back, nothing the chat did not say. */
function record(message, kind) {
  const from = message.from ?? {};
  return {
    at: new Date((message.date ?? 0) * 1000).toISOString(),
    kind,
    chat_id: message.chat?.id ?? null,
    chat: message.chat?.title ?? message.chat?.username ?? null,
    message_id: message.message_id ?? null,
    from: [from.first_name, from.last_name].filter(Boolean).join(' ') || from.username || null,
    username: from.username ?? null,
    // The numeric id, because a display name and a @handle both change and
    // neither is an identity. Anything that ever gates on "who asked" needs it.
    user_id: from.id ?? null,
    is_bot: Boolean(from.is_bot),
    reply_to: message.reply_to_message?.message_id ?? null,
    text: message.text ?? message.caption ?? null,
    // Something was said that is not text — a file, a photo, a poll. Name it so
    // a gap in the transcript is visible instead of silent.
    attachment: message.text || message.caption
      ? null
      : Object.keys(message).find((k) => ['document', 'photo', 'video', 'voice', 'audio', 'sticker', 'poll', 'location'].includes(k)) ?? null,
    // Joins, leaves, renames, pins. Telegram delivers these as messages with no
    // text, and without naming them the archive shows blank rows that look like
    // a collector fault rather than the group changing shape.
    event: describeEvent(message),
  };
}

function describeEvent(message) {
  const who = (u) => [u?.first_name, u?.last_name].filter(Boolean).join(' ') || u?.username || 'someone';
  if (message.new_chat_members?.length) return `joined: ${message.new_chat_members.map(who).join(', ')}`;
  if (message.left_chat_member) return `left: ${who(message.left_chat_member)}`;
  if (message.new_chat_title) return `renamed to ${message.new_chat_title}`;
  if (message.pinned_message) return 'pinned a message';
  if (message.group_chat_created || message.supergroup_chat_created) return 'chat created';
  if (message.migrate_to_chat_id) return `migrated to chat ${message.migrate_to_chat_id}`;
  return null;
}

async function main() {
  const offset = state.offset ?? 0;
  // A long poll costs one request per minute when the group is quiet, and
  // returns the moment someone speaks.
  const updates = await api('getUpdates', {
    offset,
    limit: 100,
    timeout: Number(process.env.KVASIR_CHAT_POLL_SECONDS ?? 50),
    allowed_updates: ['message', 'edited_message', 'channel_post'],
  });

  if (!updates.length) {
    console.log('no new messages');
    return;
  }

  const rows = [];
  const inbound = [];
  for (const update of updates) {
    const kind = update.message ? 'message'
      : update.edited_message ? 'edited'
      : update.channel_post ? 'channel_post' : null;
    const message = update.message ?? update.edited_message ?? update.channel_post;
    if (!message || !kind) continue;
    if (onlyChat && String(message.chat?.id) !== onlyChat) continue;
    rows.push(record(message, kind));
    // Only fresh messages are answerable. An edit is archived as a record of
    // the edit, but re-answering it would let a question be rewritten after it
    // was answered.
    if (kind === 'message') inbound.push(message);
  }

  if (rows.length) {
    mkdirSync(ARCHIVE_DIR, { recursive: true });
    const month = rows[0].at.slice(0, 7);
    const file = path.join(ARCHIVE_DIR, `${month}.jsonl`);
    appendFileSync(file, rows.map((r) => JSON.stringify(r)).join('\n') + '\n');
  }

  // Only now: the batch is durable, so a crash before this re-reads it.
  state.offset = updates[updates.length - 1].update_id + 1;
  state.at = new Date().toISOString();
  state.kept = (state.kept ?? 0) + rows.length;
  mkdirSync(STATE_DIR, { recursive: true });
  writeFileSync(STATE_FILE, JSON.stringify(state, null, 2) + '\n');

  console.log(`kept ${rows.length} of ${updates.length} update(s); total ${state.kept}`);

  // Answering comes after the archive is durable: a question is recorded
  // whether or not the answer succeeds.
  if (inbound.length && onlyChat) {
    try {
      const me = await api('getMe');
      const sent = await answerQuestions(inbound, {
        botUsername: me.username,
        botId: me.id,
        chatId: onlyChat,
        send: (text, replyTo) => api('sendMessage', {
          chat_id: onlyChat,
          text,
          reply_to_message_id: replyTo,
          disable_web_page_preview: true,
        }),
      });
      if (sent.length) console.log(`answered ${sent.length} question(s)`);
    } catch (error) {
      console.error(`answering failed: ${error.message}`);
    }
  }
}

main().catch((error) => { console.error(error.message); process.exit(1); });
