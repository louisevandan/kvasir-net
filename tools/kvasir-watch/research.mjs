/**
 * Reading our own material before answering.
 *
 * Asked what it thought about KVR monetisation, the bot said the facts
 * contained nothing about it. That was true and useless: the gateway guide, the
 * site copy and the KVR explorer all sit on this host, and the answer was in
 * them. The bot was not short of judgement, it was short of reading.
 *
 * So a question now goes through the repository first, and whatever it turns up
 * is handed over with the rest of the facts.
 *
 * ## This is the first time a question reaches the filesystem
 *
 * Everything before it was a fixed set of probes. A question from the group now
 * decides what gets read, and that is a door worth building narrow:
 *
 * **Fixed roots.** The places to search come from the config and never from the
 * question. No path in a message can steer this anywhere.
 *
 * **Words, not patterns.** Keywords are stripped to letters, digits and spaces
 * before they go anywhere near a search, so a message cannot smuggle in a flag,
 * a glob or a regex. They are passed as separate arguments to a program invoked
 * without a shell.
 *
 * **Read, never run.** Files are read. Nothing is executed, nothing is written,
 * and the search itself is `git grep`, which skips binaries and anything the
 * repository ignores.
 *
 * **Secrets are stripped on the way out.** Anything shaped like a key or a
 * token is replaced before the snippet reaches the model — and therefore before
 * it can reach the group.
 *
 * What comes back is quoted material with file names attached, so an opinion
 * built on it can be checked against the thing it was built from.
 */
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { readFileSync, existsSync } from 'node:fs';
import path from 'node:path';

const run = promisify(execFile);
const HERE = path.dirname(new URL(import.meta.url).pathname);
const config = JSON.parse(readFileSync(
  process.env.KVASIR_WATCH_CONFIG ?? path.join(HERE, 'config.json'), 'utf8',
));
const settings = config.research ?? {};

// Words that match everything and therefore locate nothing.
const STOP = new Set(`a an and are as at be but by can could do does for from
  get give had has have how i if in into is it its me my no not of on or our
  should so than that the their them then there these they this to too us was
  we were what when where which who why will with would you your about think
  thoughts opinion tell know like just also more most some any many much need
  want really very make made take said say says going gonna
  kvasir bot`.split(/\s+/).filter(Boolean));

/** The words in a question that are worth looking for. */
export function keywords(question, limit = 6) {
  const cleaned = String(question ?? '')
    .toLowerCase()
    .replace(/[^\p{Letter}\p{Number}\s]/gu, ' ');
  const seen = new Set();
  const out = [];
  for (const word of cleaned.split(/\s+/)) {
    if (word.length < 3 || STOP.has(word) || seen.has(word)) continue;
    seen.add(word);
    out.push(word);
    if (out.length >= limit) break;
  }
  return out;
}

// Vendored third-party source is not "our own material". llama.cpp's README
// answering a question about our token is a wrong answer that looks sourced.
const SKIP = /(^|\/)(node_modules|dist|build|\.git|coverage|target|external|vendor|third_party|\.venv)\//;
const SKIP_FILE = /(^|\/)(\.env|.*\.lock|package-lock\.json|.*\.min\.[jc]ss?|.*\.(png|jpg|jpeg|gif|svg|ico|woff2?|pdf|zip|gguf|bin))$/i;

/**
 * Anything shaped like a credential, gone before it can be quoted.
 *
 * The corpus is our own source. It should hold no secrets, and mostly does not
 * — but "mostly" is not a property to hand to a model that talks to a group
 * chat.
 */
function redact(line) {
  return line
    .replace(/\b\d{8,}:[A-Za-z0-9_-]{30,}\b/g, '<redacted bot token>')
    .replace(/\bsk-[A-Za-z0-9_-]{12,}\b/g, '<redacted key>')
    .replace(/\bgh[pousr]_[A-Za-z0-9]{20,}\b/g, '<redacted token>')
    .replace(/\b(?:api[-_]?key|secret|password|token)\s*[:=]\s*\S{8,}/gi, '$& '.replace(/.*/, '<redacted secret>'))
    .replace(/\beyJ[A-Za-z0-9_-]{20,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\b/g, '<redacted jwt>');
}

/** One root, searched for all the words at once. */
async function searchRoot(root, words) {
  if (!existsSync(root)) return [];
  const args = ['-C', root, 'grep', '-n', '-i', '-I', '--no-color', '--max-count=8'];
  for (const word of words) args.push('-e', word);
  let stdout = '';
  try {
    ({ stdout } = await run('git', args, { timeout: 25_000, maxBuffer: 8e6 }));
  } catch (error) {
    // git grep exits 1 when nothing matched; that is an answer, not a fault.
    if (error.code === 1) return [];
    throw error;
  }
  return stdout.split('\n').filter(Boolean).map((line) => {
    const [file, lineNo, ...rest] = line.split(':');
    return { root, file, line: Number(lineNo), text: rest.join(':').trim().slice(0, 300) };
  }).filter((hit) => hit.file && !SKIP.test(hit.file) && !SKIP_FILE.test(hit.file));
}

/**
 * What our own material says about this question.
 *
 * @returns {Promise<{text: string, files: string[]}>}
 */
export async function research(question, { maxFiles = Number(settings.maxFiles ?? 6), maxChars = Number(settings.maxChars ?? 9000) } = {}) {
  const roots = settings.roots ?? [];
  const words = keywords(question);
  if (!roots.length || !words.length) return { text: '', files: [] };

  let hits = [];
  for (const root of roots) {
    try { hits = hits.concat(await searchRoot(root, words)); }
    catch (error) { console.error(`research: ${root} could not be searched (${error.message})`); }
  }
  if (!hits.length) return { text: `Nothing in our own files mentions ${words.join(', ')}.`, files: [] };

  // Rank by how many different words a file answers to, not by how often one
  // word repeats — a file that speaks to the whole question beats a file that
  // says "token" forty times.
  const byFile = new Map();
  for (const hit of hits) {
    const key = `${hit.root}::${hit.file}`;
    if (!byFile.has(key)) byFile.set(key, { ...hit, lines: [], words: new Set() });
    const entry = byFile.get(key);
    entry.lines.push(hit);
    for (const word of words) if (hit.text.toLowerCase().includes(word)) entry.words.add(word);
  }
  const ranked = [...byFile.values()]
    .sort((a, b) => (b.words.size - a.words.size) || (b.lines.length - a.lines.length))
    .slice(0, maxFiles);

  const parts = [`Searched our own files for: ${words.join(', ')}.`];
  const files = [];
  let budget = maxChars;
  for (const entry of ranked) {
    const label = path.join(path.basename(entry.root), entry.file);
    const block = [`\n${label}`];
    for (const hit of entry.lines.slice(0, 6)) block.push(`  ${hit.line}: ${redact(hit.text)}`);
    const rendered = block.join('\n');
    if (rendered.length > budget) break;
    budget -= rendered.length;
    parts.push(rendered);
    files.push(label);
  }
  return { text: parts.join('\n'), files };
}
