import assert from "node:assert/strict";
import test from "node:test";
import { captureWorkingTree } from "./evidence.mjs";

// These run against this repository, which is the only tree available. What
// they check is the shape of the answer and its agreement with git, not a
// fabricated fixture that could agree with the code and not with reality.

test("a clean tree has no identity beyond its commit", () => {
  const { id, clean } = captureWorkingTree(process.cwd());
  if (!clean) {
    // The suite is being run against a dirty checkout; then the other half of
    // the contract is what matters.
    assert.equal(typeof id, "string");
    assert.equal(id.length, 64);
    return;
  }
  assert.equal(id, null);
});

test("the identity is stable across calls", () => {
  const first = captureWorkingTree(process.cwd());
  const second = captureWorkingTree(process.cwd());
  assert.deepEqual(first, second);
});

test("clean and identified are mutually exclusive", () => {
  const { id, clean } = captureWorkingTree(process.cwd());
  assert.equal(clean, id === null);
});
