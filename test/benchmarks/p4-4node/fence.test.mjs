import assert from "node:assert/strict";
import test from "node:test";
import { fencedRecords } from "./fence.mjs";

test("a run that wrote nothing yields nothing", () => {
  // The failure a single offset could not catch: the reader must not fall
  // back to the whole file when this run appended none of it.
  const result = fencedRecords("", 4096, 4096);
  assert.equal(result.ok, true);
  assert.deepEqual(result.records, []);
});

test("the run's own records are returned", () => {
  const text = "P4_SESSION_KEY_ADMITTED request=req key=mine\n";
  const result = fencedRecords(text, 100, 144);
  assert.equal(result.ok, true);
  assert.deepEqual(result.records, ["P4_SESSION_KEY_ADMITTED request=req key=mine"]);
});

test("a file that shrank is a failure, not an empty result", () => {
  const result = fencedRecords("anything", 4096, 10);
  assert.equal(result.ok, false);
  assert.match(result.reason, /shrank/);
});

test("missing boundaries are a failure", () => {
  assert.equal(fencedRecords("x", null, 10).ok, false);
  assert.equal(fencedRecords("x", 0, undefined).ok, false);
});

test("blank lines are not records", () => {
  const result = fencedRecords("a\n\n\r\nb\n", 0, 8);
  assert.deepEqual(result.records, ["a", "b"]);
});
