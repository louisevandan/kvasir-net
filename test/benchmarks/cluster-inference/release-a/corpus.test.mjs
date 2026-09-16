import { test } from 'node:test';
import assert from 'node:assert/strict';
import { corpusCase, judge, wrap } from './corpus.mjs';

test('Qwen3.5 no-thinking prompt matches the GGUF chat-template suffix exactly', () => {
  const prompt = wrap('probe');
  assert.equal(prompt.endsWith('<|im_start|>assistant\n<think>\n\n</think>\n\n'), true);
  assert.equal(prompt.includes('<think></think>'), false);
});

test('controlled corpus derives answers from beginning, middle and end source facts', () => {
  const c = corpusCase('long', 0, 8);
  assert.deepEqual(c.expected, { rows: [
    { id: 'R00002', revision: 2, power_mW: 6144, energy_mWh: 36864, pressure_alarm: false },
    { id: 'R00005', revision: 5, power_mW: 86247, energy_mWh: 172494, pressure_alarm: true },
    { id: 'R00007', revision: 7, power_mW: 231489, energy_mWh: 2777868, pressure_alarm: true },
  ], temperature_measured: false });
  assert.equal(judge(c, JSON.stringify(c.expected), 'eos').passed, true);
  const bad = structuredClone(c.expected); bad.rows[1].energy_mWh++;
  assert.equal(judge(c, JSON.stringify(bad), 'eos').passed, false);
  assert.equal(judge(c, JSON.stringify(c.expected), 'length').passed, false);
  assert.equal(judge(c, 'I found R00002, R00005, R00007.', 'eos').passed, false);
});

test('code correction includes the equality counterexample and rejects unmodified code', () => {
  const c = corpusCase('short', 3, 8);
  assert.equal(c.expected.checks.at(-1).alarm, false);
  assert.equal(judge(c, JSON.stringify(c.expected), 'eos').passed, true);
  const bad = { ...c.expected, replacement: 'return pressure >= threshold;' };
  assert.equal(judge(c, JSON.stringify(bad), 'eos').passed, false);
});

test('wave cases differ in source data and cannot reuse another case answer', () => {
  const a = corpusCase('medium', 0, 32), b = corpusCase('medium', 1, 32);
  assert.notEqual(a.prompt, b.prompt);
  assert.equal(judge(b, JSON.stringify(a.expected), 'eos').passed, false);
});
