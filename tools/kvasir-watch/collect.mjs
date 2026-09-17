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
 * Nothing here writes anywhere. It prints one JSON document.
 */
import { execFile } from 'node:child_process';
import { readFileSync, existsSync } from 'node:fs';
import { promisify } from 'node:util';
import path from 'node:path';

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

/* ---- ring: the engine itself, on the machines that run it ---------------- */

const ssh = (host, script) => run('ssh', [
  '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=10', host, script,
], { timeout: 45_000, maxBuffer: 4e6 }).then((r) => r.stdout);

async function agentHost(agent) {
  // One round trip: liveness, uptime, recent complaints, GPU rows.
  const script = [
    'echo ---proc---',
    'ps -eo pid,etime,cmd | grep "[p]4-agent" | head -3',
    'echo ---listen---',
    'ss -ltn 2>/dev/null | grep -E "4201[0-9]" | head -3',
    'echo ---err---',
    'tail -n 5 ~/p4-envelope-*/studio-*/agent.err 2>/dev/null | tail -n 5',
    'echo ---gpu---',
    'rocm-smi --showuse --csv 2>/dev/null | head -12 || nvidia-smi --query-gpu=utilization.gpu --format=csv,noheader | head -8',
  ].join('; ');
  const out = await ssh(agent.host, script);
  const section = (name) => {
    const body = out.split(`---${name}---`)[1] ?? '';
    return body.split(/---[a-z]+---/)[0].trim();
  };
  const proc = section('proc');
  return {
    label: agent.label,
    running: Boolean(proc),
    uptime: proc.trim().split(/\s+/)[1] ?? null,
    listening: section('listen').length > 0,
    recentErrors: section('err').split('\n').filter(Boolean).slice(-3),
    gpuRows: section('gpu').split('\n').filter((line) => line.includes(',')).length,
  };
}

/**
 * What the agent says it is holding. Runs over a short-lived tunnel, because
 * the agents bind loopback — the engine is not exposed, and this must not
 * change that.
 */
async function agentStages(agent) {
  const local = 42900 + (agent.port % 100);
  const control = `/tmp/kvasir-watch-${agent.label}.sock`;
  await run('ssh', ['-o', 'BatchMode=yes', '-f', '-N', '-M', '-S', control,
    '-L', `${local}:127.0.0.1:${agent.port}`, agent.host], { timeout: 30_000 });
  try {
    const { connect } = await import('kvasir-p4-bridge/wire');
    const address = `tcp://127.0.0.1:${agent.port}`;
    const client = await connect({
      host: '127.0.0.1', port: local, address,
      channel: `watch-${Date.now().toString(16)}`,
    });
    try {
      const snapshot = await client.inspect(address, { timeoutMs: 10_000 });
      return {
        label: agent.label,
        nodes: (snapshot.nodes ?? []).map((node) => ({
          node: node.node_id, state: node.state,
          generation: node.generation, adapter: node.adapter_kind,
        })),
        gpus: (snapshot.machine?.capability?.gpus ?? []).length,
        vramBytes: snapshot.machine?.capability?.gpus?.[0]?.memory_total_bytes ?? null,
      };
    } finally { client.close?.(); }
  } finally {
    await run('ssh', ['-S', control, '-O', 'exit', agent.host]).catch(() => {});
  }
}

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
      ...config.agents.map((agent) => probe(`host:${agent.label}`, () => agentHost(agent))),
      ...config.agents.map((agent) => probe(`stages:${agent.label}`, () => agentStages(agent))),
    ]),
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
