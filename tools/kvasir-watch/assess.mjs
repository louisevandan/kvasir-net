#!/usr/bin/env node
/**
 * The judgement at the end of the review.
 *
 * The rest of the page is facts, and facts can be rendered. What a reader wants
 * after them is an opinion: given what the machines said this morning, is the
 * project where it needs to be, and what should happen next. That is written
 * each day by a Claude run over the same collected JSON the page is built from.
 *
 * Two rules keep it honest. The model is given only the collected facts and a
 * fixed brief, so it cannot cite a number nobody measured. And it returns
 * structured text, never HTML — the markup here is ours, so a bad answer can
 * make the assessment wrong but cannot break the document or inject anything
 * into it.
 *
 * If the run fails, the previous assessment is left in place and the failure is
 * logged. A stale opinion clearly dated is better than an empty section.
 *
 * The model is ours. When KVASIR_LLM_URL is set this calls that endpoint —
 * Qwen3.5-27B on our own GB10, reached through a tunnel this opens and closes
 * — rather than shelling out to a vendor CLI. Summarising collected facts needs
 * no browser and no filesystem, which is exactly the shape of work our own
 * serving can take, so it takes it.
 *
 * Usage: node assess.mjs <collect.json>
 */
import { execFile } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { promisify } from 'node:util';
import path from 'node:path';
import { ask, available as ourModelAvailable } from './llm.mjs';

const run = promisify(execFile);
const HERE = path.dirname(new URL(import.meta.url).pathname);
const config = JSON.parse(readFileSync(
  process.env.KVASIR_WATCH_CONFIG ?? path.join(HERE, 'config.json'), 'utf8',
));
const collectPath = process.argv[2];
const data = JSON.parse(readFileSync(collectPath, 'utf8'));

const OPEN = '<!-- LIVE:ASSESSMENT -->';
const CLOSE = '<!-- /LIVE:ASSESSMENT -->';
const esc = (value) => String(value ?? '').replace(/[&<>]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;' }[c]));

const BRIEF = `You are reviewing the Kvasir project for its own team, the way an engineer who has read the code would: plainly, without pitch language.

Below is the JSON the monitoring job collected this morning. It is the ONLY evidence you may use. Do not introduce numbers, model names, dates or claims that are not in it — no recalled figures, no plausible-sounding detail. Where the JSON records a probe that failed, treat that as a fact about the project, not as an absence to skip.

Standing context you may rely on:
- Three tracks are in flight: hardening the Ring, porting the settlement gateway to p4, and turning the desktop client into a real node.
- The settlement gateway's public endpoint is offline deliberately, pending that port. It is not an incident.
- The YZi Labs EASY Residency S5 application is the near deadline.
- p4 is the current engine; linkcpp is the previous one.

Write, in both English and Korean:
1. "verdict": two or three sentences. Where the project actually stands this morning. Lead with what changed or what is at risk, not with a summary of the obvious.
2. "steps": three to five recommended actions, each one sentence, ordered by what should happen first. Each must be something a person can do this week, and must follow from the evidence.

Return ONLY a JSON object, no prose around it, no code fence:
{"en":{"verdict":"...","steps":["...","..."]},"ko":{"verdict":"...","steps":["...","..."]}}

The Korean must carry the same judgement as the English, not a looser version of it.`;

/** Pull the JSON object out of whatever the model wrapped it in. */
function extract(text) {
  const fenced = text.match(/```(?:json)?\s*([\s\S]*?)```/);
  const body = fenced ? fenced[1] : text;
  const start = body.indexOf('{');
  const end = body.lastIndexOf('}');
  if (start < 0 || end <= start) throw new Error('no JSON object in the reply');
  return JSON.parse(body.slice(start, end + 1));
}

function block(lang, verdict, steps, stamp) {
  const heading = lang === 'ko' ? '평가와 권고 계획' : 'Assessment and recommended plan';
  const sub = lang === 'ko'
    ? `${esc(stamp)} 기준으로 그날의 사실만 보고 쓴 판단입니다.`
    : `Written from this morning's facts alone — ${esc(stamp)}.`;
  const next = lang === 'ko' ? '권고' : 'Recommended';
  return [
    OPEN,
    '  <section id="assessment">',
    `    <h2>${heading}</h2>`,
    `    <p class="sub">${sub}</p>`,
    `    <p>${esc(verdict)}</p>`,
    `    <h3>${next}</h3>`,
    '    <ol>',
    ...steps.map((step) => `      <li>${esc(step)}</li>`),
    '    </ol>',
    '  </section>',
    CLOSE,
  ].join('\n');
}

async function main() {
  const prompt = `${BRIEF}\n\n--- collected facts (${data.generatedAt}) ---\n${JSON.stringify(data)}`;
  const LANG_SHAPE = {
    type: 'object', additionalProperties: false, required: ['verdict', 'steps'],
    properties: {
      verdict: { type: 'string' },
      steps: { type: 'array', minItems: 3, maxItems: 5, items: { type: 'string' } },
    },
  };
  const answer = ourModelAvailable()
    ? await ask(prompt, {
        type: 'object', additionalProperties: false, required: ['en', 'ko'],
        properties: { en: LANG_SHAPE, ko: LANG_SHAPE },
      }, { name: 'assessment' })
    : extract((await run('claude', ['-p', prompt], { timeout: 240_000, maxBuffer: 8e6, env: { ...process.env } })).stdout);
  for (const lang of ['en', 'ko']) {
    const part = answer[lang];
    if (!part?.verdict || !Array.isArray(part.steps) || !part.steps.length) {
      throw new Error(`the reply had no usable ${lang} assessment`);
    }
  }

  const stamp = data.generatedAt.replace('T', ' ').slice(0, 16) + ' UTC';
  for (const [lang, entry] of Object.entries(config.readiness ?? {})) {
    const file = path.resolve(HERE, entry.file);
    const html = readFileSync(file, 'utf8');
    const start = html.indexOf(OPEN);
    const end = html.indexOf(CLOSE);
    if (start < 0 || end < 0) { console.error(`${entry.file}: no assessment block`); continue; }
    const part = answer[lang] ?? answer.en;
    writeFileSync(file, html.slice(0, start) + block(lang, part.verdict, part.steps, stamp) + html.slice(end + CLOSE.length));
    console.log(`${entry.file}: assessment updated`);
  }
}

main().catch((error) => {
  console.error(`assessment not updated: ${error.message}`);
  process.exit(1);            // the runner keeps the previous one
});
