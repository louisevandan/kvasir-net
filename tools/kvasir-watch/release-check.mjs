#!/usr/bin/env node
/**
 * Notice when a version ships, and write it down.
 *
 * The release notes page is only worth reading if it keeps up, and it only
 * keeps up if nobody has to remember to update it. So this reads the three
 * places a version actually appears — the engine binary on the machines, the
 * desktop app's manifest, the catalog of what is being served — and appends an
 * entry to the site's releases.json when one of them changes.
 *
 * What it will not do is invent a release. Every entry is built from a value
 * that was read, and the evidence lines say where each came from; a version
 * that cannot be read is left alone rather than guessed at.
 *
 * Exit codes: 0 nothing changed · 10 an entry was written (the runner sends it)
 */
import { execFile } from 'node:child_process';
import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { promisify } from 'node:util';
import path from 'node:path';

const run = promisify(execFile);
const HERE = path.dirname(new URL(import.meta.url).pathname);
const config = JSON.parse(readFileSync(
  process.env.KVASIR_WATCH_CONFIG ?? path.join(HERE, 'config.json'), 'utf8',
));

const SITE = path.join(config.repo, 'kvasir-home');
const RELEASES = path.join(SITE, 'src', 'releases.json');
const STATE_DIR = path.join(HERE, 'state');
const STATE_FILE = path.join(STATE_DIR, 'releases.json');
const today = new Date().toISOString().slice(0, 10);

const state = existsSync(STATE_FILE) ? JSON.parse(readFileSync(STATE_FILE, 'utf8')) : {};
const saveState = () => { mkdirSync(STATE_DIR, { recursive: true }); writeFileSync(STATE_FILE, JSON.stringify(state, null, 2)); };

// Beside the fleet the agent is on this host; a host named local runs the
// script here instead of over ssh (same convention collect.mjs used).
const isLocal = (host) => host === 'localhost' || host === 'local' || host === '127.0.0.1';
const ssh = (host, script) => (isLocal(host)
  ? run('/bin/sh', ['-c', script], { timeout: 30_000 })
  : run('ssh', ['-o', 'BatchMode=yes', '-o', 'ConnectTimeout=10', host, script], { timeout: 30_000 })
).then((r) => r.stdout.trim());

/* ---- what version is where ---------------------------------------------- */

/** The engine version is the agent binary the machines are actually running. */
async function engineVersion() {
  for (const agent of config.agents ?? []) {
    try {
      const out = await ssh(agent.host, 'ps -eo args | grep "[p]4-agent" | head -1');
      const match = out.match(/p4-agent-v([0-9]+\.[0-9]+\.[0-9]+)|p4-envelope-([0-9a-f]{6,})/);
      if (match) return { version: match[1] ? `v${match[1]}` : `build ${match[2].slice(0, 9)}`, host: agent.label };
      const dir = out.match(/\/(p4-[a-z-]+-[0-9a-f]{6,}[^/\s]*)\//);
      if (dir) return { version: dir[1], host: agent.label };
    } catch { /* a machine that cannot be reached is not a release event */ }
  }
  return null;
}

function walletVersion() {
  const file = path.join(config.repo, 'wallet', 'desktop', 'package.json');
  if (!existsSync(file)) return null;
  const pkg = JSON.parse(readFileSync(file, 'utf8'));
  return { version: pkg.version, name: pkg.build?.productName ?? pkg.name };
}

/** What the bridge catalog says is being served, and as how many stages. */
function servedModel() {
  const file = path.join(config.repo, 'p4bridge', 'catalog.json');
  if (!existsSync(file)) return null;
  const catalog = JSON.parse(readFileSync(file, 'utf8'));
  const model = (catalog.models ?? [])[0];
  if (!model) return null;
  const agents = new Set(model.stages.map((stage) => stage.agent));
  return { id: model.id, name: model.name ?? model.id, stages: model.stages.length, machines: agents.size };
}

/* ---- writing an entry ---------------------------------------------------- */

function appendRelease(entry) {
  const data = JSON.parse(readFileSync(RELEASES, 'utf8'));
  if (data.releases.some((release) => release.id === entry.id)) return false;
  data.releases.unshift(entry);
  data.updated = today;
  writeFileSync(RELEASES, JSON.stringify(data, null, 2) + '\n');
  return true;
}

async function main() {
  const written = [];

  const engine = await engineVersion();
  if (engine && state.engine !== engine.version) {
    const first = state.engine === undefined;
    state.engine = engine.version;
    if (!first) {
      written.push({
        id: `engine-${engine.version.replace(/[^a-z0-9.]+/gi, '-')}-${today}`,
        component: 'engine',
        version: `p4 ${engine.version}`,
        date: today,
        title: `The engine moved to ${engine.version}`,
        body: `The machines are running p4 ${engine.version}. This entry records the change; the gate results for it are published as they are measured.`,
        evidence: [`Read from the running agent process on ${engine.host}.`],
      });
    }
  }

  const wallet = walletVersion();
  if (wallet && state.wallet !== wallet.version) {
    const first = state.wallet === undefined;
    state.wallet = wallet.version;
    if (!first) {
      written.push({
        id: `wallet-${wallet.version}-${today}`,
        component: 'wallet',
        version: `${wallet.name} ${wallet.version}`,
        date: today,
        title: `${wallet.name} ${wallet.version}`,
        body: `A new version of the desktop wallet and node app.`,
        evidence: ['Version read from the app manifest in the repository.'],
      });
    }
  }

  const served = servedModel();
  const servedKey = served ? `${served.id}:${served.stages}:${served.machines}` : null;
  if (served && state.served !== servedKey) {
    const first = state.served === undefined;
    state.served = servedKey;
    if (!first) {
      written.push({
        id: `network-${served.id}-${today}`,
        component: 'network',
        version: served.name,
        date: today,
        title: `${served.name} is being served`,
        body: `${served.name} is placed as ${served.stages} stages across ${served.machines} machine${served.machines === 1 ? '' : 's'} and serves from there. Placement comes from an operator's plan, not from an HTTP call.`,
        evidence: [`Read from the bridge catalog, which is re-verified against a live INSPECT before anything is served.`],
      });
    }
  }

  saveState();

  if (!written.length) { console.log('no new release'); return; }

  const added = written.filter(appendRelease);
  if (!added.length) { console.log('release entries already present'); return; }

  const lines = [`Release notes updated · ${added.length} entr${added.length === 1 ? 'y' : 'ies'}`, ''];
  for (const entry of added) lines.push(`${entry.version} — ${entry.title}`);
  lines.push('', 'https://kvasir-ai.net/releases');
  process.stdout.write(lines.join('\n') + '\n');
  process.exitCode = 10;
}

main().catch((error) => { console.error(error.message); process.exit(1); });
