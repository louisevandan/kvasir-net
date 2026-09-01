import assert from "node:assert/strict";
import test from "node:test";
import { beginFence, endFence, fencedRecords } from "./fence.mjs";

const run = "20260901T000000Z-abc";
const other = "20260901T111111Z-def";
const wrap = (runId, ...records) => [beginFence(runId), ...records, endFence(runId)];

test("only what lies between this run's fences is returned", () => {
  const text = [
    "P4_SESSION_KEY_ADMITTED request=req key=stale",
    ...wrap(other, "P4_SESSION_KEY_ADMITTED request=req key=someone-else"),
    ...wrap(run, "P4_SESSION_KEY_ADMITTED request=req key=mine"),
  ].join("\n");
  const result = fencedRecords(text, run);
  assert.equal(result.ok, true);
  assert.deepEqual(result.records, ["P4_SESSION_KEY_ADMITTED request=req key=mine"]);
});

test("a run that wrote nothing yields nothing, not the file", () => {
  // The failure offsets could not catch: an adapter that emitted no record at
  // all, in a file full of earlier runs that used the same request ids.
  const text = [
    "P4_SESSION_KEY_ADMITTED request=req key=stale",
    ...wrap(run),
  ].join("\r\n");
  const result = fencedRecords(text, run);
  assert.equal(result.ok, true);
  assert.deepEqual(result.records, []);
});

test("a missing fence is a failure, not an empty result", () => {
  const result = fencedRecords("P4_SESSION_KEY_ADMITTED request=req key=stale", run);
  assert.equal(result.ok, false);
  assert.match(result.reason, /no fence/);
});

test("an unclosed fence is a failure", () => {
  const text = [beginFence(run), "P4_SESSION_KEY_ADMITTED request=req key=mine"].join("\n");
  const result = fencedRecords(text, run);
  assert.equal(result.ok, false);
  assert.match(result.reason, /never closed/);
});

test("a concurrent run writing into the same file is a failure", () => {
  const text = [beginFence(run), "noise", beginFence(other), "more", endFence(other)].join("\n");
  const result = fencedRecords(text, run);
  assert.equal(result.ok, false);
  assert.match(result.reason, /another run/);
});

test("a fence closed by someone else is a failure", () => {
  const text = [beginFence(run), "noise", endFence(other)].join("\n");
  const result = fencedRecords(text, run);
  assert.equal(result.ok, false);
  assert.match(result.reason, /closed by/);
});

test("a repeated begin for this run is a failure", () => {
  const text = [beginFence(run), beginFence(run), endFence(run)].join("\n");
  const result = fencedRecords(text, run);
  assert.equal(result.ok, false);
  assert.match(result.reason, /two fences/);
});
