/**
 * Asking our own model.
 *
 * The endpoint is Qwen3.5-27B served by vLLM on GB10 #1. It is not exposed
 * outside the office, so a call from the fleet opens an SSH tunnel for its own
 * duration and closes it again — the same shape the collector uses to reach an
 * agent on loopback.
 *
 * Two things here are not preference but hard-won:
 *
 * `enable_thinking: false` — Qwen3.x otherwise spends the whole token budget on
 * a hidden reasoning pass and returns an empty message, which reads as a model
 * refusing rather than a model never reaching the answer.
 *
 * A schema, not a request for one. Asking for JSON produced valid prose and a
 * truncated array; a cut-off reply is indistinguishable from a model that
 * cannot follow instructions, and both throw the run away. Constrain the shape
 * and the failure mode disappears.
 */
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import net from 'node:net';
// No './net.mjs' here on purpose: this only ever fetches 127.0.0.1 through a
// tunnel it opened itself, so there is no DNS lookup and no address family to
// get wrong. Every module that reaches a real hostname imports it.

const run = promisify(execFile);

/** A port the kernel just told us is free. */
function freePort() {
  return new Promise((resolve, reject) => {
    const probe = net.createServer();
    probe.once('error', reject);
    probe.listen(0, '127.0.0.1', () => {
      const { port } = probe.address();
      probe.close(() => resolve(port));
    });
  });
}

/**
 * Open the tunnel this endpoint needs, and hand back the URL that reaches it.
 *
 * The local port is chosen per call, not fixed. Two jobs overlapped on one
 * port — the daily assessment and a question in the group — and the second
 * ssh could not bind, so its `fetch` went to a port whose tunnel belonged to
 * someone else and died with it. The symptom was `answering failed: fetch
 * failed`, which says nothing about a port collision.
 */
async function withTunnel(fn) {
  const configured = process.env.KVASIR_LLM_URL;
  const tunnel = process.env.KVASIR_LLM_TUNNEL;        // user@host:remote-port
  if (!tunnel) return fn(configured);
  const [target, remotePort] = tunnel.split(':');
  const localPort = await freePort();
  const socket = `/tmp/kvasir-llm-${process.pid}-${localPort}.sock`;
  await run('ssh', ['-o', 'BatchMode=yes', '-o', 'StrictHostKeyChecking=accept-new',
    '-o', 'ExitOnForwardFailure=yes',
    '-i', `${process.env.HOME}/.ssh/id_ed25519_kvasir_watch`,
    '-f', '-N', '-M', '-S', socket, '-L', `${localPort}:127.0.0.1:${remotePort}`, target],
    { timeout: 30_000 });
  const url = new URL(configured);
  url.port = String(localPort);
  url.hostname = '127.0.0.1';
  try { return await fn(url.toString().replace(/\/$/, '')); }
  finally { await run('ssh', ['-S', socket, '-O', 'exit', target]).catch(() => {}); }
}

/**
 * One call, one answer, shaped by `schema`.
 *
 * @param {string} prompt
 * @param {object} schema JSON Schema the reply must satisfy
 * @param {{maxTokens?: number, temperature?: number, name?: string}} [options]
 */
/**
 * Pull one JSON object out of a reply that may wrap it in prose or a fence.
 * The ring's bridge forwards only the prompt and max_tokens to the engine —
 * `response_format` never reaches it (p4bridge/server.js chatCompletions) —
 * so the shape has to be asked for in the prompt and recovered from the text.
 */
export function extractJson(text) {
  if (typeof text !== 'string') return null;
  const unfenced = text.replace(/```(?:json)?\s*([\s\S]*?)```/g, '$1');
  const start = unfenced.indexOf('{');
  const end = unfenced.lastIndexOf('}');
  if (start < 0 || end <= start) return null;
  try { return JSON.parse(unfenced.slice(start, end + 1)); } catch { return null; }
}

/** The keys the schema says must be there, checked one level deep. */
function fitsSchema(value, schema) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false;
  return (schema?.required ?? []).every((key) => key in value);
}

const shapeInstruction = (schema) => [
  'Answer with exactly one JSON object and nothing else: no prose before or after, no code fence.',
  'It must match this JSON Schema:',
  JSON.stringify(schema),
].join('\n');

export async function ask(prompt, schema, options = {}) {
  if (!process.env.KVASIR_LLM_URL) throw new Error('KVASIR_LLM_URL is not set');
  return withTunnel(async (url) => {
    let lastText = '';
    for (let attempt = 0; attempt < 2; attempt++) {
      const reminder = attempt === 0 ? '' : `\n\nYour previous reply was not a JSON object matching the schema. ${shapeInstruction(schema)}`;
      const text = await complete(url, `${prompt}\n\n${shapeInstruction(schema)}${reminder}`, schema, options);
      lastText = text;
      const value = extractJson(text);
      if (fitsSchema(value, schema)) return value;
    }
    throw new Error(`the model's reply was not the shape asked for: ${lastText.slice(0, 200)}`);
  });
}

async function complete(url, prompt, schema, options) {
  {
    const response = await fetch(`${url}/chat/completions`, {
      method: 'POST',
      headers: {
        'content-type': 'application/json',
        ...(process.env.KVASIR_LLM_KEY ? { authorization: `Bearer ${process.env.KVASIR_LLM_KEY}` } : {}),
      },
      body: JSON.stringify({
        model: process.env.KVASIR_LLM_MODEL,
        messages: [{ role: 'user', content: prompt }],
        max_tokens: options.maxTokens ?? 2400,
        temperature: options.temperature ?? 0.2,
        chat_template_kwargs: { enable_thinking: false },
        response_format: {
          type: 'json_schema',
          json_schema: { name: options.name ?? 'reply', strict: true, schema },
        },
      }),
      signal: AbortSignal.timeout(300_000),
    });
    const body = await response.json();
    const text = body?.choices?.[0]?.message?.content;
    if (!text) throw new Error(`the model returned no content (${body?.error?.message ?? response.status})`);
    return text;
  }
}

export const available = () => Boolean(process.env.KVASIR_LLM_URL);
