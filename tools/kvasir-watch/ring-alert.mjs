#!/usr/bin/env node
/**
 * Say something about the Ring only when the Ring changed.
 *
 * A daily line that reads "2 agents up, 2 stages loaded" every morning is a
 * line nobody reads — and then the morning it says something else, nobody
 * reads that either. So this reduces a collection to the handful of facts that
 * matter (serving or not, which agents the bridge holds, each stage's state,
 * the slot gauge's level) and compares them with the last run. Same facts:
 * silence. Different: one short message naming what moved, from what to what.
 *
 * Reads a collection JSON (path or stdin). Keeps its memory in
 * `state/ring.json`. Exit 0 when nothing changed or on the first run (the
 * baseline is recorded, not announced); exit 10 with the message on stdout when
 * something did — the same convention seed.mjs and release-check.mjs use, so
 * daily.sh and the chat timer can send it the same way.
 */
import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const HERE = path.dirname(new URL(import.meta.url).pathname);

/** The facts worth waking someone for, as a flat map of key → value. */
export function compactRing(data) {
  const probes = data?.tracks?.ring ?? [];
  const facts = {};
  const bridge = probes.find((probe) => probe.name === 'bridge');
  if (!bridge) facts['bridge'] = 'not probed';
  else if (!bridge.ok) facts['bridge'] = 'unreachable';
  else facts['bridge'] = bridge.value.serving ? `serving ${bridge.value.serving_models}` : `not serving (${bridge.value.status})`;
  if (bridge?.ok && bridge.value.inspect_error) facts['inspect'] = 'error';

  for (const probe of probes) {
    if (probe.name.startsWith('host:')) {
      const label = probe.name.slice(5);
      facts[`agent ${label}`] = !probe.ok ? 'unknown'
        : probe.value.dial?.stopped ? `stopped (${probe.value.dial.stopped})`
        : probe.value.running ? 'connected' : 'disconnected';
    } else if (probe.name.startsWith('stages:')) {
      const label = probe.name.slice(7);
      if (!probe.ok) { facts[`stages ${label}`] = 'unknown'; continue; }
      if (!probe.value.nodes.length) facts[`stages ${label}`] = 'none';
      for (const node of probe.value.nodes) facts[`stage ${node.node}`] = node.state;
    } else if (probe.name.startsWith('gauge:')) {
      const label = probe.name.slice(6);
      facts[`slots ${label}`] = !probe.ok ? 'unknown' : probe.value.level;
    }
  }
  return facts;
}

/** Lines describing what differs between two compact states. */
export function diffRing(before, after) {
  const lines = [];
  for (const key of new Set([...Object.keys(before ?? {}), ...Object.keys(after)])) {
    const was = before?.[key];
    const now = after[key];
    if (was === now) continue;
    if (was === undefined) lines.push(`${key}: ${now} (new)`);
    else if (now === undefined) lines.push(`${key}: ${was} → gone`);
    else lines.push(`${key}: ${was} → ${now}`);
  }
  return lines;
}

const MARK = (line) => (/→ (connected|loaded|serving|good)\b/.test(line) ? '🟢'
  : /(unreachable|disconnected|stopped|critical|not serving|failed|unknown)/.test(line) ? '🔴' : '🟡');

export function main(argv = process.argv, { stateFile = path.join(HERE, 'state', 'ring.json'), now = new Date() } = {}) {
  const source = argv[2];
  const data = JSON.parse(source && source !== '-' ? readFileSync(source, 'utf8') : readFileSync(0, 'utf8'));
  const after = compactRing(data);
  const previous = existsSync(stateFile) ? JSON.parse(readFileSync(stateFile, 'utf8')) : null;

  mkdirSync(path.dirname(stateFile), { recursive: true });
  writeFileSync(stateFile, JSON.stringify({ at: now.toISOString(), facts: after }, null, 2) + '\n');

  if (!previous) { process.stdout.write('ring baseline recorded; nothing to announce\n'); return 0; }
  const changes = diffRing(previous.facts, after);
  if (!changes.length) return 0;

  const since = previous.at ? ` since ${previous.at.slice(0, 16).replace('T', ' ')}Z` : '';
  process.stdout.write([
    `Ring changed${since}`,
    ...changes.map((line) => `  ${MARK(line)} ${line}`),
  ].join('\n') + '\n');
  return 10;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try { process.exitCode = main(); }
  catch (error) { console.error(error.message); process.exitCode = 1; }
}
