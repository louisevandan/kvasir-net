/**
 * The Ring, as the bridge sees it.
 *
 * The bot used to dial each agent itself: an ssh tunnel, one INSPECT, close.
 * That close was the problem. A p4 agent keeps a connection's slot until the
 * client sends FINISH, and the bot never did — so on the GB10 ring every daily
 * run would have taken one of 256 slots from each agent for good. Yesterday's
 * outage was exactly that leak, from other clients, and it took a day to find
 * (p4bridge/DEFECT-agent-slot-leak.md).
 *
 * So the bot no longer talks to an agent at all. The bridge already holds one
 * long-lived connection per agent and INSPECTs over it every 15 s; everything
 * the report needs is in `/health` and `/api/runtime`. Reading those costs the
 * ring nothing. Nothing in this file opens a socket to port 42011, and nothing
 * should be added here that does.
 *
 * What the bridge does not know — the slot count on each host — comes from the
 * gauge that runs on the host itself (a systemd timer, one line per 5 min). It
 * is read as a file, when the bot runs on that host or has been given a copy.
 *
 * Everything is injectable so the shapes can be tested against fixtures
 * without a bridge.
 */
import { readFileSync, existsSync } from 'node:fs';
import os from 'node:os';

const expand = (file) => file.replace(/^~(?=\/|$)/, os.homedir());

/* ---- thresholds ---------------------------------------------------------- */

// From the defect report's workaround section: warn at 128 held socket fds,
// critical at 192. `hidden` is the leak itself and should be 0.
export const SOCKFD_WARN = 128;
export const SOCKFD_CRITICAL = 192;
// A gauge that has not written for this long is dead, not quiet: the timer
// fires every 5 minutes and always writes one line.
export const GAUGE_STALE_MIN = 15;

/* ---- the bridge ---------------------------------------------------------- */

async function getJson(fetchFn, url, { token, timeoutMs = 8000 } = {}) {
  const headers = token ? { 'x-kvasir-service-token': token } : {};
  const response = await fetchFn(url, { headers, signal: AbortSignal.timeout(timeoutMs) });
  const text = await response.text();
  let body = null;
  try { body = JSON.parse(text); } catch { /* not JSON: reported by status below */ }
  return { status: response.status, body };
}

/**
 * Read `/health` and `/api/runtime`. `/health` answers without a token and
 * 503 when nothing is serving — that is a live bridge saying "not serving",
 * not a dead one, and the two must never be conflated (the host gauge did,
 * and logged a recovering bridge as unreachable). `/api/runtime` needs the
 * service token; without it the bridge answers 401 and the per-agent probes
 * say so instead of pretending the agents are unknown.
 */
export async function readBridge(bridge, { fetch: fetchFn = fetch, env = process.env } = {}) {
  const base = bridge.url.replace(/\/$/, '');
  const token = bridge.tokenEnv ? (env[bridge.tokenEnv] ?? '') : '';

  const health = await getJson(fetchFn, `${base}/health`);
  if (!health.body || typeof health.body !== 'object') {
    throw new Error(`bridge /health answered ${health.status} without a JSON body`);
  }

  let runtime = null;
  let runtimeError = null;
  try {
    const r = await getJson(fetchFn, `${base}/api/runtime`, { token });
    if (r.status === 401) {
      runtimeError = `service token required for /api/runtime (${bridge.tokenEnv ?? 'no tokenEnv configured'} ${token ? 'was rejected' : 'is not set'})`;
    } else if (r.status !== 200 || !r.body) {
      runtimeError = `/api/runtime answered ${r.status}`;
    } else {
      runtime = r.body;
    }
  } catch (error) {
    runtimeError = String(error.message ?? error).slice(0, 200);
  }

  return { base, health: health.body, healthStatus: health.status, runtime, runtimeError };
}

/* ---- the catalog --------------------------------------------------------- */

/**
 * The load generation is not in `/api/runtime` (nodes there carry no
 * generation). The bridge's catalog holds the one it verified against, so the
 * report can at least say which generation the bridge believes it is serving.
 * That is the bridge's belief, not the agent's word; it is labelled as such.
 */
export function readCatalog(file, { readFile = readFileSync, exists = existsSync } = {}) {
  if (!file) return null;
  const path = expand(file);
  if (!exists(path)) return { file: path, error: 'not found' };
  try {
    const catalog = JSON.parse(readFile(path, 'utf8'));
    const models = (catalog.models ?? []).map((model) => ({
      id: model.id,
      load_generation: model.load_generation ?? null,
      nodes: (model.stages ?? []).map((stage) => stage.node_id ?? stage.node ?? null).filter(Boolean),
    }));
    return { file: path, models };
  } catch (error) {
    return { file: path, error: String(error.message ?? error).slice(0, 200) };
  }
}

/* ---- the host gauge ------------------------------------------------------ */

const GAUGE_PAIR = /(\w+)=("[^"]*"|\S+)/g;

/**
 * Parse one gauge line. Only lines carrying `unique=` count: earlier versions
 * of the gauge wrote a `hidden` with a different meaning (fds minus what `ss`
 * shows, which counts dup fds as leaks). A line without `unique=` is ignored,
 * never misread.
 */
export function parseGaugeLine(line) {
  if (!line || !/\bunique=/.test(line)) return null;
  const at = line.split(/\s+/, 1)[0];
  const fields = {};
  for (const [, key, raw] of line.matchAll(GAUGE_PAIR)) {
    fields[key] = raw.startsWith('"') ? raw.slice(1, -1) : raw;
  }
  const num = (key) => (fields[key] === undefined || fields[key] === 'na' ? null : Number(fields[key]));
  return {
    at,
    agent_pid: num('agent_pid'),
    agent_sockfd: num('agent_sockfd'),
    unique: num('unique'),
    hidden: num('hidden'),
    close_wait: num('close_wait'),
    // Gauge v6 writes one of: ok · not_serving · http_<code> · unreachable(<class>).
    // Only the last means the bridge is dead; the others are a live bridge.
    // The bot takes serving/not-serving from /health itself, so this is kept
    // as the gauge's own word, not rated.
    bridge: fields.bridge ?? null,
    bridgeDead: (fields.bridge ?? '').startsWith('unreachable'),
  };
}

/** Rate a parsed gauge line. `level` is what the report colours on. */
export function rateGauge(parsed, now = Date.now()) {
  if (!parsed) return { level: 'unknown', why: 'no v5 gauge line' };
  const ageMin = Math.round((now - Date.parse(parsed.at)) / 60_000);
  if (!Number.isFinite(ageMin)) return { level: 'unknown', why: `unparseable time ${parsed.at}`, ageMin: null };
  if (ageMin > GAUGE_STALE_MIN) return { level: 'stale', why: `last line ${ageMin} min ago`, ageMin };
  if (parsed.agent_sockfd === null) return { level: 'bad', why: 'no agent process', ageMin };
  if (parsed.agent_sockfd >= SOCKFD_CRITICAL) return { level: 'critical', why: `${parsed.agent_sockfd} socket fds`, ageMin };
  if (parsed.agent_sockfd >= SOCKFD_WARN) return { level: 'warn', why: `${parsed.agent_sockfd} socket fds`, ageMin };
  if (parsed.hidden > 0) return { level: 'warn', why: `${parsed.hidden} hidden (leaked) sockets`, ageMin };
  return { level: 'good', why: `${parsed.agent_sockfd} fds, hidden 0`, ageMin };
}

export function readGauge(file, { readFile = readFileSync, exists = existsSync, now = Date.now() } = {}) {
  const path = expand(file);
  if (!exists(path)) throw new Error(`gauge log ${path} not found`);
  // Last line that carries the current format. The file is append-only and
  // small; reading it whole is fine and avoids a seek dance.
  const lines = readFile(path, 'utf8').split('\n').filter(Boolean);
  const last = [...lines].reverse().find((line) => /\bunique=/.test(line)) ?? null;
  const parsed = parseGaugeLine(last);
  return { ...(parsed ?? {}), ...rateGauge(parsed, now), file: path };
}

/* ---- the probes the rest of the bot reads -------------------------------- */

/**
 * Build the ring probes in the shapes `report.mjs`, `readiness.mjs` and
 * `answer.mjs` already read (`host:<label>`, `stages:<label>`), plus one
 * `bridge` probe and one `gauge:<label>` per configured gauge file. Keeping
 * the old shapes means the renderers did not have to change to stop the bot
 * dialling agents; what changed is where the facts come from.
 *
 * Fields that used to come from a shell on the host — process uptime, the
 * agent's own log tail — are `null` now rather than guessed. The bridge does
 * not know them.
 */
export async function ringProbes(config, deps = {}) {
  const bridge = config.bridge;
  if (!bridge?.url) {
    return [{ name: 'bridge', ok: false, error: 'config.bridge.url is not set — the bot reads the ring through the bridge' }];
  }
  const labelOf = (agent) => bridge.agents?.[agent] ?? agent.replace(/^tcp:\/\//, '');
  const probes = [];

  let read;
  try {
    read = await readBridge(bridge, deps);
  } catch (error) {
    probes.push({ name: 'bridge', ok: false, error: String(error.message ?? error).slice(0, 300) });
    // Without the bridge, every agent is unknown — say so per agent, so a
    // renderer that lists agents shows the gap where the agent would be.
    for (const agent of Object.keys(bridge.agents ?? {})) {
      probes.push({ name: `host:${labelOf(agent)}`, ok: false, error: 'bridge unreachable' });
      probes.push({ name: `stages:${labelOf(agent)}`, ok: false, error: 'bridge unreachable' });
    }
    pushGauges(probes, bridge, deps);
    return probes;
  }

  const { health, healthStatus, runtime, runtimeError } = read;
  const catalog = readCatalog(bridge.catalog, deps);
  const generationOf = (nodeId) => catalog?.models?.find((m) => m.nodes.includes(nodeId))?.load_generation ?? null;

  probes.push({
    name: 'bridge', ok: true, value: {
      url: read.base,
      status: healthStatus,
      serving: healthStatus === 200 && (health.serving_models ?? 0) > 0,
      serving_models: health.serving_models ?? 0,
      inspect_error: health.inspect_error ?? null,
      dials_stopped: health.dials_stopped ?? [],
      awaiting_probe: health.awaiting_probe ?? [],
      uptime_ms: health.uptime_ms ?? null,
      runtime: runtime ? 'read' : `not read: ${runtimeError}`,
      catalog: catalog ? (catalog.error ? `not read: ${catalog.error}` : catalog.models.map((m) => `${m.id}@${m.load_generation}`).join(', ')) : 'not configured',
    },
  });

  // The agents the report should list: the ones configured, plus any the
  // bridge knows that the config does not name (a new machine must not vanish
  // from the report because nobody added a label).
  const known = new Set(Object.keys(bridge.agents ?? {}));
  for (const row of runtime?.dial_ledger ?? []) known.add(row.agent);
  for (const machine of runtime?.machines ?? []) known.add(machine.agent);

  for (const agent of known) {
    const label = labelOf(agent);
    if (!runtime) {
      probes.push({ name: `host:${label}`, ok: false, error: runtimeError });
      probes.push({ name: `stages:${label}`, ok: false, error: runtimeError });
      continue;
    }
    const dial = runtime.dial_ledger?.find((row) => row.agent === agent) ?? null;
    const machine = runtime.machines?.find((row) => row.agent === agent) ?? null;
    const errors = [];
    if (dial?.stopped) errors.push(`dials stopped: ${dial.stopped}`);
    if (dial && !dial.connected && dial.next_probe_in_s) errors.push(`not connected; next probe in ${dial.next_probe_in_s}s after ${dial.failed_probes} failed`);
    if (health.inspect_error) errors.push(`inspect: ${String(health.inspect_error).slice(0, 120)}`);
    if (machine?.transport?.failures) errors.push(`${machine.transport.failures} preserved transport failure(s)`);

    probes.push({
      name: `host:${label}`, ok: true, value: {
        label,
        // "running" now means: the bridge holds a live connection to it. An
        // agent process that is up but not accepting (the leak's end state)
        // reads as not running here — which is the operationally true answer.
        running: Boolean(dial?.connected),
        advertised: agent,
        uptime: null,
        listening: Boolean(dial?.connected),
        recentErrors: errors,
        gpuRows: machine?.gpus?.length ?? 0,
        dial,
        transport: machine?.transport ?? null,
      },
    });
    probes.push({
      name: `stages:${label}`, ok: true, value: {
        label,
        nodes: (machine?.nodes ?? []).map((node) => ({
          node: node.node_id, state: node.state,
          generation: null,                          // the bridge does not relay it
          catalogGeneration: generationOf(node.node_id),
          adapter: node.adapter_kind,
        })),
        gpus: machine?.gpus?.length ?? 0,
        vramBytes: machine?.gpus?.[0]?.memory_total_bytes ?? null,
      },
    });
  }

  pushGauges(probes, bridge, deps);
  return probes;
}

function pushGauges(probes, bridge, deps) {
  for (const [label, file] of Object.entries(bridge.gauges ?? {})) {
    try { probes.push({ name: `gauge:${label}`, ok: true, value: { label, ...readGauge(file, deps) } }); }
    catch (error) { probes.push({ name: `gauge:${label}`, ok: false, error: String(error.message ?? error).slice(0, 300) }); }
  }
}
