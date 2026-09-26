#!/usr/bin/env node
/**
 * The seed pipeline, brought within reach of the bot.
 *
 * Every programme and fund Kvasir is raising from lives in one table, kept by
 * the pipeline job on the office GB10 — who to apply to, what they want, when
 * it closes, and where each one stands. The monitoring bot could not see any of
 * it, so a question in the group about a programme we are actively in the
 * middle of ("what do we need for Nexus?") got an honest "the facts do not
 * contain that" about a fact we had written down the day before. The gap was
 * not knowledge; it was plumbing.
 *
 * The table is read, never written, from here. The pipeline job owns the
 * credentials and the row states; this host only asks it what the table says.
 * A status changes when a person changes it — not because someone asked the bot
 * a question.
 *
 * The answer is cached. Reading 71 rows over ssh on every five-minute chat poll
 * would be a standing cost for a table that changes a few times a week, so the
 * cache is refreshed on demand when it has gone stale and someone is actually
 * asking. If the refresh fails the previous answer is used and its age is
 * stated, because a week-old pipeline clearly dated is worth more than silence.
 *
 * Usage: node seed.mjs [--force]     refresh the cache and summarise it
 *        node seed.mjs --brief      the lines the daily report should carry
 */
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { readFileSync, writeFileSync, mkdirSync, existsSync, statSync } from 'node:fs';
import path from 'node:path';

const run = promisify(execFile);
const HERE = path.dirname(new URL(import.meta.url).pathname);
const CACHE = path.join(HERE, 'state', 'seed.json');

const config = JSON.parse(readFileSync(
  process.env.KVASIR_WATCH_CONFIG ?? path.join(HERE, 'config.json'), 'utf8',
));
const seedConfig = config.seed ?? {};

/**
 * What the table holds. Read-only by construction.
 *
 * Two sources. `config.seed.file`: a dump the pipeline host pushes to us (the
 * bot on GCP has no way into GB10 #1, and should not be given one for this);
 * a dump older than `staleHours` is refused, so a push that stopped reads as
 * stale rather than as an unchanging table. Otherwise `config.seed.host`: ask
 * the host over ssh, the way it worked beside the fleet.
 */
async function fetchRows() {
  if (seedConfig.file) {
    const file = seedConfig.file.replace(/^~(?=\/|$)/, process.env.HOME);
    if (!existsSync(file)) throw new Error(`seed dump ${file} not found (not pushed yet?)`);
    const ageH = (Date.now() - statSync(file).mtimeMs) / 3_600_000;
    if (ageH > Number(seedConfig.staleHours ?? 6) * 4) throw new Error(`seed dump is ${ageH.toFixed(1)} h old — the push has stopped`);
    const rows = JSON.parse(readFileSync(file, 'utf8'));
    if (!Array.isArray(rows) || !rows.length) throw new Error('the pushed dump holds no rows');
    return rows;
  }
  const host = seedConfig.host;
  if (!host) throw new Error('neither config.seed.file nor config.seed.host is set');
  const key = `${process.env.HOME}/.ssh/id_ed25519_kvasir_watch`;
  const command = seedConfig.command ?? 'cd ~/kvasir-seed && python3 sync.py dump';
  const { stdout } = await run('ssh', [
    '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=10',
    '-o', 'StrictHostKeyChecking=accept-new',
    ...(existsSync(key) ? ['-i', key] : []),
    host, command,
  ], { timeout: 60_000, maxBuffer: 8e6 });
  const rows = JSON.parse(stdout);
  if (!Array.isArray(rows) || !rows.length) throw new Error('the pipeline returned no rows');
  return rows;
}

/** What is on disk, however old. */
export function cached() {
  if (!existsSync(CACHE)) return null;
  try { return JSON.parse(readFileSync(CACHE, 'utf8')); } catch { return null; }
}

const ageHours = (entry) => (Date.now() - Date.parse(entry.at)) / 3_600_000;

/**
 * The pipeline as the bot should see it: fresh if cheaply possible, cached and
 * dated otherwise, null if we have never managed to read it at all.
 */
export async function pipeline({ maxAgeHours = Number(seedConfig.staleHours ?? 6), force = false } = {}) {
  const have = cached();
  if (have && !force && ageHours(have) < maxAgeHours) return have;
  try {
    const rows = await fetchRows();
    const entry = { at: new Date().toISOString(), rows };
    mkdirSync(path.dirname(CACHE), { recursive: true });
    writeFileSync(CACHE, JSON.stringify(entry, null, 2) + '\n');
    return entry;
  } catch (error) {
    // Keep going on what we have. The caller shows the date, so a stale answer
    // is visibly stale rather than quietly wrong.
    if (have) return { ...have, staleBecause: String(error.message ?? error).slice(0, 200) };
    return null;
  }
}

/**
 * Words that identify one programme rather than the pipeline in general.
 *
 * "capital" appears in a dozen fund names and picks out none of them; "nexus"
 * appears in one. Scoring by how rare a word is across the table is what lets a
 * question name a programme in passing and still reach the right row.
 */
function distinctiveTokens(rows) {
  const tokens = rows.map((row) => new Set(
    `${row.id ?? ''} ${row.name ?? ''}`.toLowerCase().split(/[^a-z0-9]+/).filter((t) => t.length >= 4),
  ));
  const frequency = new Map();
  for (const set of tokens) for (const token of set) frequency.set(token, (frequency.get(token) ?? 0) + 1);
  return rows.map((row, i) => ({
    row,
    keys: [...tokens[i]].filter((t) => (frequency.get(t) ?? 0) <= 3),
  }));
}

/** Rows the question names, by a word that belongs to few other rows. */
export function rowsNamedIn(question, rows) {
  const asked = ` ${question.toLowerCase().replace(/[^a-z0-9]+/g, ' ')} `;
  return distinctiveTokens(rows)
    .filter(({ keys }) => keys.some((key) => asked.includes(` ${key} `)))
    .map(({ row }) => row);
}

const DAY = 86_400_000;

/** A deadline that is a real date and lands inside the window. */
function closingWithin(row, days) {
  const at = Date.parse(row.deadline ?? '');
  if (Number.isNaN(at)) return false;                 // "Rolling", "확인 불가"
  const away = (at - Date.now()) / DAY;
  return away >= -7 && away <= days;
}

/** Everything about one programme, on a few lines. */
function full(row) {
  const lines = [`- ${row.name} [${row.status ?? 'unknown'}] — ${row.type ?? ''} · ${row.base ?? ''} · deadline ${row.deadline ?? 'unstated'}`];
  if (row.focus) lines.push(`    focus: ${row.focus}`);
  if (row.apply_mode || row.apply_url) lines.push(`    apply: ${[row.apply_mode, row.apply_url].filter(Boolean).join(' — ')}`);
  if (row.note) lines.push(`    what it needs: ${row.note}`);
  if (row.owner_note) lines.push(`    where we stand${row.status_at ? ` (${row.status_at})` : ''}: ${row.owner_note}`);
  return lines.join('\n');
}

/**
 * The pipeline section of the bot's facts.
 *
 * Not the whole table: the live ones, the ones closing soon, and whichever the
 * question names. Seventy-one rows of prose would crowd out the monitoring
 * facts and buy nothing — nobody asks about a fund nobody has contacted.
 */
export function pipelineText(entry, question = '') {
  if (!entry) return 'The seed pipeline could not be read, and no earlier copy is stored.';
  const rows = entry.rows ?? [];
  const counts = new Map();
  for (const row of rows) counts.set(row.status ?? 'unknown', (counts.get(row.status ?? 'unknown') ?? 0) + 1);

  const head = [
    `Seed pipeline as of ${entry.at.slice(0, 16).replace('T', ' ')} UTC` +
      (entry.staleBecause ? ` (could not refresh: ${entry.staleBecause})` : ''),
    `${rows.length} programmes tracked — ${[...counts].map(([s, n]) => `${n} ${s}`).join(', ')}.`,
  ];

  const shown = new Map();
  const add = (row) => { if (row?.id) shown.set(row.id, row); };
  for (const row of rows) if (row.status && row.status !== 'Not started') add(row);
  for (const row of rows) if (closingWithin(row, 30)) add(row);
  for (const row of rowsNamedIn(question, rows)) add(row);

  if (!shown.size) return [...head, 'Nothing is live and nothing closes in the next 30 days.'].join('\n');
  return [...head, '', 'Live, closing soon, or asked about:', ...[...shown.values()].map(full)].join('\n');
}

const SEEN = path.join(HERE, 'state', 'seed-seen.json');

/**
 * What the group should be told this morning without having to ask.
 *
 * Two things qualify: a deadline close enough to act on, and a status someone
 * changed since the last report. Everything else is a table that has not moved,
 * and a daily line about a table that has not moved is how a report becomes
 * something people stop reading.
 */
function brief(entry, { withinDays = 14 } = {}) {
  const rows = entry.rows ?? [];
  const seen = existsSync(SEEN) ? JSON.parse(readFileSync(SEEN, 'utf8')) : null;

  const changed = [];
  if (seen) {
    for (const row of rows) {
      const before = seen[row.id];
      if (before && before !== row.status) changed.push(`${row.name}: ${before} → ${row.status}`);
    }
  }
  mkdirSync(path.dirname(SEEN), { recursive: true });
  writeFileSync(SEEN, JSON.stringify(Object.fromEntries(rows.map((r) => [r.id, r.status ?? null])), null, 2) + '\n');

  const closing = rows
    .filter((row) => closingWithin(row, withinDays))
    .map((row) => ({ row, days: Math.round((Date.parse(row.deadline) - Date.now()) / DAY) }))
    .sort((a, b) => a.days - b.days);

  const lines = [];
  if (closing.length) {
    lines.push('Seed pipeline — closing soon');
    for (const { row, days } of closing) {
      const when = days < 0 ? `${-days}d ago` : days === 0 ? 'today' : `${days}d`;
      lines.push(`  ${row.name} — ${row.deadline} (${when}) · ${row.status}`);
    }
  }
  if (changed.length) {
    lines.push(lines.length ? '' : '');
    lines.push('Seed pipeline — moved since the last report');
    for (const line of changed) lines.push(`  ${line}`);
  }
  // The first run has nothing to diff against, only a table to remember.
  if (!seen && !closing.length) return [];
  return lines;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const entry = await pipeline({ force: true });
  if (!entry) { console.error('the seed pipeline could not be read'); process.exit(1); }
  if (process.argv.includes('--brief')) {
    const lines = brief(entry);
    if (!lines.length) { console.error('nothing closing and nothing moved'); process.exit(0); }
    process.stdout.write(lines.join('\n') + '\n');
    process.exit(10);                     // 10 = there is something worth reading
  }
  const counts = new Map();
  for (const row of entry.rows) counts.set(row.status ?? 'unknown', (counts.get(row.status ?? 'unknown') ?? 0) + 1);
  console.log(`pipeline ${entry.rows.length} rows @ ${entry.at.slice(0, 16)} — ${[...counts].map(([s, n]) => `${n} ${s}`).join(', ')}` +
    (entry.staleBecause ? ` (stale: ${entry.staleBecause})` : ''));
}
