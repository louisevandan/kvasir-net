import { test } from 'node:test';
import assert from 'node:assert/strict';
import { compactSubset } from './materialize-corpus.mjs';
import { corpusCase, judge } from './corpus.mjs';

test('exact token sizing uses each equivalent prose alternative at most once', () => {
  assert.deepEqual(compactSubset([1,1,1], 2), [0,1]);
  assert.equal(compactSubset([3,3], 1), null);
  assert.deepEqual(compactSubset([3,4,2], 6), [1,2]);
  const full = corpusCase('long', 0, 8), short = corpusCase('long', 0, 8, [0,1,2]);
  assert.deepEqual(full.source_facts, short.source_facts);
  assert.deepEqual(full.expected, short.expected);
  assert.equal(judge(short, JSON.stringify(full.expected), 'eos').passed, true);
  assert.notEqual(full.prompt, short.prompt);
  assert.throws(() => corpusCase('long', 0, 8, [0,0]));
});
