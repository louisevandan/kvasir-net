import assert from "node:assert/strict";
import test from "node:test";
import { fencedRecords } from "./fence.mjs";

const whole = (text) => ({ text, endsOnRecord: true });
const cut = (text) => ({ text, endsOnRecord: false });

test("a run that wrote nothing yields nothing", () => {
  const result = fencedRecords(whole(""), 4096, 4096);
  assert.equal(result.ok, true);
  assert.deepEqual(result.records, []);
});

test("the run's own records are returned", () => {
  const result = fencedRecords(whole("P4_SESSION_KEY_ADMITTED request=req key=mine"), 100, 145);
  assert.equal(result.ok, true);
  assert.deepEqual(result.records, ["P4_SESSION_KEY_ADMITTED request=req key=mine"]);
});

test("a slice cut inside a record is refused, not trimmed", () => {
  // The far side saw that the last byte was not a newline: the closing length
  // landed while the agent was still writing that record.
  const result = fencedRecords(cut("P4_SESSION_KEY_ADMITTED request=req key=mi"), 100, 142);
  assert.equal(result.ok, false);
  assert.match(result.reason, /inside a record/);
});

test("a file that shrank is a failure", () => {
  assert.equal(fencedRecords(whole("anything"), 4096, 10).ok, false);
  assert.match(fencedRecords(whole("anything"), 4096, 10).reason, /shrank/);
});

test("missing boundaries are a failure", () => {
  assert.equal(fencedRecords(whole("x"), null, 10).ok, false);
  assert.equal(fencedRecords(whole("x"), 0, undefined).ok, false);
});

test("blank lines are not records", () => {
  const result = fencedRecords(whole("a\n\n\r\nb"), 0, 8);
  assert.deepEqual(result.records, ["a", "b"]);
});

test("a slice that begins inside a record is refused", () => {
  // The opening length landed mid-write, so the first line here would be the
  // tail of a record another run owns.
  const result = fencedRecords(
    { text: "key=mine\nP4_SESSION_KEY_ADMITTED request=req key=next\n", beginsOnRecord: false, endsOnRecord: true },
    100,
    160,
  );
  assert.equal(result.ok, false);
  assert.match(result.reason, /began inside a record/);
});

test("a slice whose boundaries are both clean is accepted", () => {
  const result = fencedRecords(
    { text: "P4_SESSION_KEY_ADMITTED request=req key=mine\n", beginsOnRecord: true, endsOnRecord: true },
    100,
    145,
  );
  assert.equal(result.ok, true);
  assert.equal(result.records.length, 1);
});
