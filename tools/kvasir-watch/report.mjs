#!/usr/bin/env node
/**
 * Turn a collection into the day's report: one HTML page and one short text
 * summary for the chat.
 *
 * The page is a single self-contained file — it is sent as an attachment and
 * has to open from a phone's download folder with no network. The summary is
 * what people actually read in the group, so it leads with the three tracks and
 * says plainly when a probe failed.
 *
 * Reads the JSON from stdin or a path; writes `<out>.html` and prints the
 * summary on stdout.
 *
 * Commit history is deliberately absent from both. A standing instruction says
 * the bot must not send it, and the collection still carries subjects, hashes
 * and authors — so the omission lives here, at the point where a fact becomes a
 * message, rather than in the collector where dropping it would also blind the
 * local evidence. What remains is the count: how much moved, not what moved.
 */
import { readFileSync, writeFileSync } from 'node:fs';

const source = process.argv[2];
const data = JSON.parse(source && source !== '-' ? readFileSync(source, 'utf8') : readFileSync(0, 'utf8'));
const outFile = process.argv[3] ?? null;

const TRACKS = {
  ring: { title: 'Ring hardening', blurb: 'The engine and the machines that run it' },
  gateway: { title: 'Settlement gateway', blurb: 'The money path, moving to p4' },
  client: { title: 'Client app', blurb: 'Wallet and desktop node' },
};

const byName = (probes, name) => probes.find((probe) => probe.name === name);
const named = (probes, prefix) => probes.filter((probe) => probe.name.startsWith(prefix));
const esc = (value) => String(value ?? '').replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
const gib = (bytes) => (bytes ? `${Math.round(bytes / 1024 ** 3)} GiB` : '—');

/* ---- what each track amounts to today ------------------------------------ */

function ringFacts(probes) {
  const rows = [];
  const bridge = byName(probes, 'bridge');
  if (bridge && !bridge.ok) rows.push({ state: 'bad', text: `bridge unreachable — ${bridge.error}` });
  else if (bridge) {
    rows.push({
      state: bridge.value.serving ? 'good' : 'bad',
      text: bridge.value.serving
        ? `bridge serving ${bridge.value.serving_models} model${bridge.value.serving_models === 1 ? '' : 's'}`
        : `bridge up but not serving (${bridge.value.status})`,
      note: bridge.value.inspect_error ? `inspect: ${String(bridge.value.inspect_error).slice(0, 120)}` : null,
    });
  }
  // When the bridge itself is down, every agent row would repeat "bridge
  // unreachable"; the bridge row above already says it once.
  const bridgeDown = bridge && !bridge.ok;
  // Likewise when every agent probe failed for one shared reason (no service
  // token, say): one row naming the reason, not the same line four times.
  const agentProbes = [...named(probes, 'host:'), ...named(probes, 'stages:')];
  const sharedError = agentProbes.length && agentProbes.every((p) => !p.ok && p.error === agentProbes[0].error) ? agentProbes[0].error : null;
  if (sharedError && !bridgeDown) rows.push({ state: 'unknown', text: `agents not read: ${sharedError}` });
  const skipFailed = bridgeDown || Boolean(sharedError);
  for (const probe of named(probes, 'stages:')) {
    if (!probe.ok) { if (!skipFailed) rows.push({ state: 'unknown', text: `${probe.name.slice(7)}: ${probe.error}` }); continue; }
    const { label, nodes, gpus, vramBytes } = probe.value;
    const loaded = nodes.filter((node) => node.state === 'loaded').length;
    rows.push({
      state: nodes.length && loaded === nodes.length ? 'good' : nodes.length ? 'warn' : 'idle',
      text: `${label}: ${nodes.length ? nodes.map((n) => `${n.node} ${n.state}`).join(', ') : 'no stages held'}`,
      note: `${gpus} GPU × ${gib(vramBytes)}`,
    });
  }
  for (const probe of named(probes, 'host:')) {
    if (!probe.ok) { if (!skipFailed) rows.push({ state: 'unknown', text: `${probe.name.slice(5)}: ${probe.error}` }); continue; }
    const { label, running, uptime, recentErrors } = probe.value;
    rows.push({
      state: running ? 'good' : 'bad',
      text: `${label} agent ${running ? (uptime ? `up ${uptime}` : 'connected') : 'not reachable from the bridge'}`,
      note: recentErrors.length ? recentErrors[recentErrors.length - 1].slice(0, 120) : null,
    });
  }
  // Connection slots on the host, from the gauge that runs there. A p4 agent
  // never gives a leaked slot back, so this is the one number that predicts
  // the next outage rather than reporting the last one.
  const GAUGE_STATE = { good: 'good', warn: 'warn', critical: 'bad', bad: 'bad', stale: 'unknown', unknown: 'unknown' };
  for (const probe of named(probes, 'gauge:')) {
    if (!probe.ok) { rows.push({ state: 'unknown', text: `${probe.name.slice(6)} slots: ${probe.error}` }); continue; }
    const { label, level, why, agent_sockfd, hidden } = probe.value;
    rows.push({
      state: GAUGE_STATE[level] ?? 'unknown',
      text: `${label} slots ${level}${agent_sockfd === null || agent_sockfd === undefined ? '' : ` · ${agent_sockfd} fds, hidden ${hidden}`}`,
      note: level === 'good' ? null : why,
    });
  }
  return rows;
}

function gatewayFacts(probes) {
  const rows = [];
  const public_ = byName(probes, 'public');
  if (public_?.ok) {
    rows.push({
      state: public_.value.ok ? 'good' : 'warn',
      text: `public endpoint ${public_.value.status} in ${public_.value.ms} ms`,
    });
  } else {
    rows.push({ state: 'bad', text: `public endpoint unreachable — ${public_?.error ?? 'no probe'}` });
  }
  return rows;
}

function clientFacts(probes) {
  const rows = [];
  const version = byName(probes, 'version');
  if (version?.ok && version.value) rows.push({ state: 'idle', text: `${version.value.name} ${version.value.version}` });
  const site = byName(probes, 'site');
  if (site?.ok) rows.push({ state: site.value.ok ? 'good' : 'warn', text: `site ${site.value.status} in ${site.value.ms} ms` });
  else rows.push({ state: 'bad', text: `site unreachable — ${site?.error ?? 'no probe'}` });
  return rows;
}

const FACTS = { ring: ringFacts, gateway: gatewayFacts, client: clientFacts };

function trackView(key) {
  const probes = data.tracks[key] ?? [];
  const commits = byName(probes, 'commits');
  const files = byName(probes, 'files');
  return {
    key,
    ...TRACKS[key],
    commits: commits?.ok ? commits.value : [],
    commitError: commits?.ok ? null : commits?.error ?? 'not collected',
    files: files?.ok ? files.value : null,
    facts: FACTS[key](probes),
    note: (data.notes ?? {})[key] || null,
    failures: probes.filter((probe) => !probe.ok).map((probe) => `${probe.name}: ${probe.error}`),
  };
}

const views = Object.keys(TRACKS).map(trackView);
const day = data.generatedAt.slice(0, 10);
const head = data.head?.ok ? data.head.value : null;

/* ---- the chat summary ---------------------------------------------------- */

const MARK = { good: '🟢', warn: '🟡', bad: '🔴', unknown: '⚪', idle: '·' };

const summary = [
  `Kvasir · ${day}`,
  head ? `head ${head.hash} on ${head.branch}` : 'head unavailable',
  '',
  ...views.flatMap((view) => [
    `${view.title} — ${view.commits.length} commit${view.commits.length === 1 ? '' : 's'}${view.files ? `, ${view.files} files` : ''}`,
    ...view.facts.slice(0, 7).map((fact) => `  ${MARK[fact.state] ?? '·'} ${fact.text}`),
    ...(view.note ? [`  note: ${view.note}`] : []),
    // Commit subjects are deliberately not sent — see the note at the top of
    // this file. The count above is the activity signal; the log is on GitHub.
    '',
  ]),
].join('\n').trim();

/* ---- the page ------------------------------------------------------------ */

const card = (view) => `
    <section class="track">
      <header>
        <h2>${esc(view.title)}</h2>
        <p class="blurb">${esc(view.blurb)}</p>
      </header>
      <ul class="facts">
        ${view.facts.map((fact) => `<li class="s-${fact.state}">
          <span class="dot"></span>
          <span>${esc(fact.text)}${fact.note ? `<em>${esc(fact.note)}</em>` : ''}</span>
        </li>`).join('\n        ')}
      </ul>
      ${view.note ? `<p class="note">${esc(view.note)}</p>` : ''}
      <div class="changes">
        <h3>${view.commits.length} commit${view.commits.length === 1 ? '' : 's'}${view.files ? ` · ${view.files} files` : ''}</h3>
        ${view.commits.length
          ? '<p class="quiet">Subjects are not listed here; read them on GitHub.</p>'
          : `<p class="quiet">${esc(view.commitError ?? 'no commits in this window')}</p>`}
      </div>
      ${view.failures.length ? `<p class="failed">could not reach: ${view.failures.map(esc).join(' · ')}</p>` : ''}
    </section>`;

const html = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Kvasir · ${day}</title>
<style>
  :root {
    --ground: #fbfaf8; --panel: #ffffff; --ink: #14171c; --muted: #6b7280;
    --line: #e7e4df; --good: #1a7f5a; --warn: #b4720d; --bad: #b3261e; --unknown: #9aa0a6;
    --accent: #2f4858;
  }
  @media (prefers-color-scheme: dark) {
    :root:not([data-theme="light"]) {
      --ground: #101215; --panel: #171a1f; --ink: #e8e6e3; --muted: #9aa0a6;
      --line: #262a30; --good: #4ec08d; --warn: #e0a23c; --bad: #ef7c72; --unknown: #6b7280;
      --accent: #9dc4d8;
    }
  }
  :root[data-theme="dark"] {
    --ground: #101215; --panel: #171a1f; --ink: #e8e6e3; --muted: #9aa0a6;
    --line: #262a30; --good: #4ec08d; --warn: #e0a23c; --bad: #ef7c72; --unknown: #6b7280;
    --accent: #9dc4d8;
  }
  * { box-sizing: border-box; }
  body {
    margin: 0; background: var(--ground); color: var(--ink);
    font: 15px/1.55 ui-sans-serif, -apple-system, "Segoe UI", Roboto, sans-serif;
    padding-block: 40px; padding-left: 20px; padding-right: 20px;
  }
  .wrap { max-width: 860px; margin: 0 auto; display: grid; gap: 28px; }
  h1 { font-size: 26px; margin: 0; letter-spacing: -0.01em; }
  .meta { color: var(--muted); font-size: 13px; margin-top: 6px; }
  .meta code { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }
  .track { background: var(--panel); border: 1px solid var(--line); border-radius: 12px; padding: 20px 22px; }
  .track h2 { font-size: 17px; margin: 0; }
  .blurb { margin: 2px 0 14px; color: var(--muted); font-size: 13px; }
  ul.facts { list-style: none; margin: 0 0 16px; padding: 0; display: grid; gap: 8px; }
  ul.facts li { display: grid; grid-template-columns: 10px 1fr; gap: 10px; align-items: start; font-size: 14px; }
  .dot { width: 8px; height: 8px; border-radius: 50%; margin-top: 7px; background: var(--unknown); }
  .s-good .dot { background: var(--good); } .s-warn .dot { background: var(--warn); }
  .s-bad .dot { background: var(--bad); } .s-idle .dot { background: var(--muted); opacity: .45; }
  ul.facts em, .changes em { display: block; font-style: normal; color: var(--muted); font-size: 12.5px; }
  .changes { border-top: 1px solid var(--line); padding-top: 14px; }
  .changes h3 { font-size: 12px; text-transform: uppercase; letter-spacing: .07em; color: var(--muted); margin: 0 0 10px; font-weight: 600; }
  .changes ol { margin: 0; padding-left: 18px; display: grid; gap: 7px; font-size: 14px; }
  .changes code { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12.5px; color: var(--accent); }
  .quiet { color: var(--muted); font-size: 13.5px; margin: 0; }
  .failed { color: var(--bad); font-size: 12.5px; margin: 12px 0 0; }
  .note { font-size: 13px; color: var(--muted); border-left: 2px solid var(--line); padding-left: 10px; margin: 0 0 14px; }
  footer { color: var(--muted); font-size: 12px; text-align: center; }
</style>
</head>
<body>
  <div class="wrap">
    <header>
      <h1>Kvasir · ${day}</h1>
      <p class="meta">${head ? `head <code>${esc(head.hash)}</code> on <code>${esc(head.branch)}</code>` : 'repository head unavailable'}<br>
      window: last ${data.sinceDays} day${data.sinceDays === 1 ? '' : 's'} · collected ${esc(data.generatedAt)}</p>
    </header>
    ${views.map(card).join('\n')}
    <footer>Kvasir Watch · probes that failed are listed, never dropped</footer>
  </div>
</body>
</html>
`;

if (outFile) writeFileSync(outFile, html);
process.stdout.write(summary + '\n');
