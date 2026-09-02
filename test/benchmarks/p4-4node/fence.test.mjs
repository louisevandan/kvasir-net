import assert from "node:assert/strict";
import test from "node:test";
import { fencedRecords } from "./fence.mjs";

test("a run that wrote nothing yields nothing", () => {
  const result = fencedRecords("", 4096, 4096);
  assert.equal(result.ok, true);
  assert.deepEqual(result.records, []);
});

test("the run's own records are returned", () => {
  const result = fencedRecords("P4_SESSION_KEY_ADMITTED request=req key=mine\n", 100, 145);
  assert.equal(result.ok, true);
  assert.deepEqual(result.records, ["P4_SESSION_KEY_ADMITTED request=req key=mine"]);
});

test("a slice cut inside a record is refused, not trimmed", () => {
  // The closing length landed while the agent was mid-write. Half a record is
  // not evidence, and trimming it would hide the interleaving silently.
  const result = fencedRecords("P4_SESSION_KEY_ADMITTED request=req key=mi", 100, 142);
  assert.equal(result.ok, false);
  assert.match(result.reason, /inside a record/);
});

test("a file that shrank is a failure", () => {
  const result = fencedRecords("anything\n", 4096, 10);
  assert.equal(result.ok, false);
  assert.match(result.reason, /shrank/);
});

test("missing boundaries are a failure", () => {
  assert.equal(fencedRecords("x\n", null, 10).ok, false);
  assert.equal(fencedRecords("x\n", 0, undefined).ok, false);
});

test("blank lines are not records", () => {
  const result = fencedRecords("a\n\n\r\nb\n", 0, 8);
  assert.deepEqual(result.records, ["a", "b"]);
});
