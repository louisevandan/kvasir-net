#!/usr/bin/env node
/**
 * Report new commits as they land, between the daily reports.
 *
 * Two ways to watch, because the repositories are not all the same shape:
 *
 *   mode "branches" (default) — read every branch head and walk the ones that
 *     moved. Works on private repositories and names the branch. Use this for
 *     a repository.
 *   mode "user" — GET /users/{login}/events, keeping PushEvents. This follows a
 *     person across every repository they push to, but GitHub only exposes
 *     PUBLIC pushes here; a private push is invisible no matter the token.
 *
 * Last-seen state lives in `state/commits.json`, one entry per watch, so a
 * restart does not replay a week of history into the group. On the very first
 * run a watch records where it is and says nothing — the point is what lands
 * from now on.
 */
import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import path from 'node:path';
import './net.mjs';

const HERE = path.dirname(new URL(import.meta.url).pathname);
const config = JSON.parse(readFileSync(
  process.env.KVASIR_WATCH_CONFIG ?? path.join(HERE, 'config.json'), 'utf8',
));
const STATE_DIR = path.join(HERE, 'state');
const STATE_FILE = path.join(STATE_DIR, 'commits.json');

const state = existsSync(STATE_FILE) ? JSON.parse(readFileSync(STATE_FILE, 'utf8')) : {};
const save = () => { mkdirSync(STATE_DIR, { recursive: true }); writeFileSync(STATE_FILE, JSON.stringify(state, null, 2)); };

/** A watch names the env var holding its token; the value never leaves here. */
async function gh(url, tokenEnv) {
  const token = (process.env[tokenEnv ?? 'GITHUB_TOKEN'] ?? '').trim();
  const response = await fetch(`https://api.github.com${url}`, {
    headers: {
      accept: 'application/vnd.github+json',
      'user-agent': 'kvasir-watch',
      ...(token ? { authorization: `Bearer ${token}` } : {}),
    },
    signal: AbortSignal.timeout(15_000),
  });
  if (!response.ok) throw new Error(`${url} → ${response.status}`);
  return response.json();
}

const short = (sha) => String(sha ?? '').slice(0, 7);
const firstLine = (message) => String(message ?? '').split('\n')[0].slice(0, 110);

/**
 * Follow every branch by its head.
 *
 * Repository events look like the obvious source — one call, all branches — but
 * on a PRIVATE repository GitHub returns the push with `commits: []`: you learn
 * that something landed, not what. So read the branch heads instead and walk
 * each one that moved. That works the same whether the repository is public or
 * private, and it names the branch a commit arrived on.
 */
async function pollRepoBranches(watch) {
  const key = `branches:${watch.repo}`;
  const previous = state[key]?.heads ?? null;
  const reported = new Set(state[key]?.reported ?? []);
  const branches = await gh(`/repos/${watch.repo}/branches?per_page=100`, watch.tokenEnv);
  const heads = Object.fromEntries(branches.map((branch) => [branch.name, branch.commit.sha]));

  if (previous === null) {                       // first sight: take the mark, stay quiet
    state[key] = { heads, reported: [...reported].slice(-500), at: new Date().toISOString() };
    return [];
  }

  const fresh = [];
  for (const [name, head] of Object.entries(heads)) {
    if (previous[name] === head) continue;
    const known = previous[name] ?? null;
    const commits = await gh(`/repos/${watch.repo}/commits?sha=${encodeURIComponent(name)}&per_page=20`, watch.tokenEnv);
    for (const commit of commits) {
      if (commit.sha === known) break;
      // A commit reachable from two branches is one event, not two.
      if (reported.has(commit.sha)) continue;
      reported.add(commit.sha);
      fresh.push({
        repo: `${watch.repo}@${name}`,
        label: watch.label ?? watch.repo,
        sha: short(commit.sha),
        author: commit.commit?.author?.name ?? commit.author?.login ?? 'unknown',
        when: commit.commit?.author?.date ?? null,
        subject: firstLine(commit.commit?.message),
        url: commit.html_url,
      });
    }
    // A brand-new branch would otherwise replay its whole history.
    if (known === null && fresh.length) fresh.splice(1);
  }

  state[key] = { heads, reported: [...reported].slice(-500), at: new Date().toISOString() };
  return fresh;
}

async function pollRepo(watch) {
  const key = `repo:${watch.repo}`;
  const seen = state[key]?.sha ?? null;
  const commits = await gh(`/repos/${watch.repo}/commits?per_page=20${watch.branch ? `&sha=${watch.branch}` : ''}`, watch.tokenEnv);
  if (!commits.length) return [];
  const fresh = [];
  for (const commit of commits) {
    if (commit.sha === seen) break;
    fresh.push({
      repo: watch.repo,
      label: watch.label ?? watch.repo,
      sha: short(commit.sha),
      author: commit.commit?.author?.name ?? commit.author?.login ?? 'unknown',
      when: commit.commit?.author?.date ?? null,
      subject: firstLine(commit.commit?.message),
      url: commit.html_url,
    });
  }
  const first = seen === null;
  state[key] = { sha: commits[0].sha, at: new Date().toISOString() };
  return first ? [] : fresh;     // first sight sets the mark, it does not shout
}

async function pollUser(watch) {
  const key = `user:${watch.user}`;
  const seen = state[key]?.id ?? null;
  const events = await gh(`/users/${watch.user}/events?per_page=30`, watch.tokenEnv);
  const pushes = events.filter((event) => event.type === 'PushEvent');
  if (!pushes.length) return [];
  const fresh = [];
  for (const event of pushes) {
    if (event.id === seen) break;
    for (const commit of (event.payload?.commits ?? []).slice().reverse()) {
      fresh.push({
        repo: event.repo?.name ?? '?',
        label: watch.label ?? watch.user,
        sha: short(commit.sha),
        author: commit.author?.name ?? watch.user,
        when: event.created_at,
        subject: firstLine(commit.message),
        url: `https://github.com/${event.repo?.name}/commit/${commit.sha}`,
      });
    }
  }
  const first = seen === null;
  state[key] = { id: events[0]?.id ?? pushes[0].id, at: new Date().toISOString() };
  return first ? [] : fresh;
}

async function main() {
  const watches = config.commitWatches ?? [];
  const found = [];
  const failures = [];
  for (const watch of watches) {
    try {
      const poll = watch.user ? pollUser
        : (watch.mode ?? 'branches') === 'branches' ? pollRepoBranches
        : pollRepo;
      found.push(...await poll(watch));
    } catch (error) {
      // A watch that cannot be read is worth saying out loud once a day, not
      // every twenty minutes; the daily report carries it.
      failures.push(`${watch.label ?? watch.repo ?? watch.user}: ${error.message}`);
    }
  }
  save();

  if (!found.length) {
    if (failures.length) console.error(`no commits; unreachable: ${failures.join(' · ')}`);
    else console.log('no new commits');
    return;
  }

  // Group by repository so a push of nine commits reads as one event.
  const groups = new Map();
  for (const commit of found) {
    const list = groups.get(commit.repo) ?? [];
    list.push(commit);
    groups.set(commit.repo, list);
  }
  const lines = [`New commits · ${found.length}`];
  for (const [repo, list] of groups) {
    lines.push('', `${list[0].label} — ${repo}`);
    for (const commit of list.slice(0, 10)) lines.push(`  ${commit.sha} ${commit.subject} — ${commit.author}`);
    if (list.length > 10) lines.push(`  … and ${list.length - 10} more`);
  }
  process.stdout.write(lines.join('\n') + '\n');
  process.exitCode = 10;          // 10 = there is something to send
}

main().catch((error) => { console.error(error.message); process.exit(1); });
