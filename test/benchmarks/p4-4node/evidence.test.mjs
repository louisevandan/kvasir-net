import assert from "node:assert/strict";
import test from "node:test";
import { agreesWithExpected } from "./evidence.mjs";

const pinned = {
  upstream_commit: "0eadefebd3f8f92a86d634a0e5b8fffc9dc792c0",
  patch_set_sha256: "3cfc636181e4ee1033249b8f7ca4d50156174138bf07c2797a476f4c42ab8c47",
};
const observed = (over = {}) => ({
  upstream_commit: pinned.upstream_commit,
  patch_set: pinned.patch_set_sha256,
  ...over,
});

test("a pipeline built from the pinned tree agrees", () => {
  assert.equal(agreesWithExpected(pinned, observed()).ok, true);
});

test("stages that agree with each other but not with the pin are refused", () => {
  // The case stage-to-stage agreement cannot see: four stages of the same
  // stale build agree perfectly. Every run between 2026-09-01 and 2026-09-02
  // recorded a pin its stages were not built from.
  const result = agreesWithExpected(pinned, observed({
    upstream_commit: "557614e0296ff4a5b6f649737a65ae2076eea2fd",
    patch_set: "00e66c6b27602232d7413ffc78ed69d5c427400e27b5a9e57b9c35107b0615dc",
  }));
  assert.equal(result.ok, false);
  assert.match(result.reason, /the checkout pins/);
});

test("the same upstream with a different queue is refused", () => {
  const result = agreesWithExpected(pinned, observed({ patch_set: "00e66c6b27602232" }));
  assert.equal(result.ok, false);
});

test("a stage server too old to report is refused", () => {
  const result = agreesWithExpected(pinned, observed({ upstream_commit: "unknown", patch_set: "unknown" }));
  assert.equal(result.ok, false);
});

test("no expectation means nothing to disagree with", () => {
  assert.equal(agreesWithExpected(null, observed()).ok, true);
});
