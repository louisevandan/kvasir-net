#!/usr/bin/env node
/**
 * Gather what is true about Kvasir today.
 *
 * Three tracks are in flight at once — hardening the Ring, reworking the
 * settlement gateway, advancing the client app — and a daily note that shows
 * only one of them misreports the project. So every probe here is tagged with
 * the track it belongs to, and each one records what it could NOT reach rather
 * than quietly dropping it: a report that omits a failed probe reads like good
 * news.
 *
 * Nothing here writes anywhere. It prints one JSON document. Ring facts come
 * from the p4 bridge's HTTP endpoints, never from an agent socket (ring.mjs).
 */
import { execFile } from 'node:child_process';
import { readFileSync, existsSync } from 'node:fs';
import { promisify } from 'node:util';
import path from 'node:path';
import './net.mjs';
import { ringProbes } from './ring.mjs';

const run = promisify(execFile);
const HERE = path.dirname(new URL(import.meta.url).pathname);
const SEP = String.fromCharCode(1);          // field separator git will not emit

const config = JSON.parse(readFileSync(
  process.env.KVASIR_WATCH_CONFIG ?? path.join(HERE, 'config.json'), 'utf8',
));

/** Every probe returns a value or an explained absence; never a silent gap. */
async function probe(name, fn) {
  try { return { name, ok: true, value: await fn() }; }
  catch (error) { return { name, ok: false, error: String(error.message ?? error).slice(0, 300) }; }
}

const git = async (args) => (await run('git', ['-C', config.repo, ...args], { maxBuffer: 8e6 })).stdout.trim();

/** Commits in the last `days` touching any of `paths`. */
async function commits(paths, days = 1) {
  const out = await git([
    'log', `--since=${days}.days.ago`, '--date=short',
    `--pretty=%h${SEP}%ad${SEP}%an${SEP}%s`, '--', ...paths,
  ]);
  if (!out) return [];
  return out.split('\n').map((line) => {
    const [hash, date, author, subject] = line.split(SEP);
    return { hash, date, author, subject };
  });
}

async function fileCount(paths, days = 1) {
  const out = await git(['log', `--since=${days}.days.ago`, '--name-only', '--pretty=format:', '--', ...paths]);
  return new Set(out.split('\n').filter(Boolean)).size;
}

/* ---- ring: the engine itself, read through the bridge ------------------- */

// The bot does not dial agents any more. It did until 2026-09-26, over an ssh
// tunnel with one INSPECT and a close — and a close without FINISH costs a p4
// agent one of its 256 connection slots for good. The bridge already holds a
// live connection to every agent; ring.mjs reads the bridge. See that file.
/* ---- gateway and client -------------------------------------------------- */

async function httpProbe(url, { timeoutMs = 8000 } = {}) {
  const started = Date.now();
  const response = await fetch(url, { signal: AbortSignal.timeout(timeoutMs) });
  const body = await response.text();
  return { status: response.status, ok: response.ok, ms: Date.now() - started, body: body.slice(0, 300) };
}

function desktopVersion() {
  const file = path.join(config.repo, 'wallet', 'desktop', 'package.json');
  if (!existsSync(file)) return null;
  const pkg = JSON.parse(readFileSync(file, 'utf8'));
  return { version: pkg.version, name: pkg.build?.productName ?? pkg.name };
}

async function main() {
  const since = Number(process.env.KVASIR_WATCH_DAYS ?? 1);
  const [ring, gateway, client] = await Promise.all([
    Promise.all([
      probe('commits', () => commits(config.tracks.ring, since)),
      probe('files', () => fileCount(config.tracks.ring, since)),
    ]).then(async (fixed) => [...fixed, ...await ringProbes(config)]),
    Promise.all([
      probe('commits', () => commits(config.tracks.gateway, since)),
      probe('files', () => fileCount(config.tracks.gateway, since)),
      probe('public', () => httpProbe(`${config.gatewayUrl}/health`)),
    ]),
    Promise.all([
      probe('commits', () => commits(config.tracks.client, since)),
      probe('files', () => fileCount(config.tracks.client, since)),
      probe('version', async () => desktopVersion()),
      probe('site', () => httpProbe(config.site)),
    ]),
  ]);

  const head = await probe('head', async () => ({
    branch: await git(['rev-parse', '--abbrev-ref', 'HEAD']),
    hash: await git(['rev-parse', '--short', 'HEAD']),
    subject: await git(['log', '-1', '--pretty=%s']),
    at: await git(['log', '-1', '--date=iso', '--pretty=%ad']),
  }));

  process.stdout.write(JSON.stringify({
    generatedAt: new Date().toISOString(),
    sinceDays: since,
    repo: config.repo,
    notes: config.notes ?? {},
    head,
    tracks: { ring, gateway, client },
  }, null, 2) + '\n');
}

main().catch((error) => { console.error(error); process.exit(1); });
