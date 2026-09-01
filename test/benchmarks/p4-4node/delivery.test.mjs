import assert from "node:assert/strict";
import test from "node:test";
import { checkDelivery, discards } from "./delivery.mjs";

const target = 'OuterEndpoint { channel: "p4-4node-mixed" }';

test("a clean log passes", () => {
  const result = checkDelivery("P4_EVENT_CONNECTION_STOPPED error=eof\nnothing else\n");
  assert.deepEqual(result, { passed: true, counted: 0, uncounted: 0, endpoints: [] });
});

test("the running total is taken, not the line count", () => {
  // The agent reports a cumulative figure, so summing lines would triple-count.
  const log = [1, 2, 3].map((n) => `P4_EVENT_OUTER_MISSING discarded=${n} target=${target}`).join("\n");
  const result = checkDelivery(log);
  assert.equal(result.counted, 3);
  assert.equal(result.passed, false);
});

test("two endpoints are summed", () => {
  const log = [
    `P4_EVENT_OUTER_MISSING discarded=2 target=${target}`,
    'P4_EVENT_OUTER_MISSING discarded=5 target=OuterEndpoint { channel: "other" }',
  ].join("\r\n");
  assert.equal(checkDelivery(log).counted, 7);
  assert.equal(discards(log).totals.size, 2);
});

test("an agent too old to count still fails the run", () => {
  const log = `P4_EVENT_OUTER_MISSING target=${target}\nP4_EVENT_OUTER_MISSING target=${target}`;
  const result = checkDelivery(log);
  assert.equal(result.passed, false);
  assert.equal(result.uncounted, 2);
  assert.equal(result.counted, 0);
});

test("the discard the 2026-09-01 mixed run produced is caught", () => {
  const log = `P4_EVENT_WRITE_FAILED error=os error 10053\nP4_EVENT_OUTER_MISSING discarded=55 target=${target}`;
  assert.equal(checkDelivery(log).counted, 55);
});
