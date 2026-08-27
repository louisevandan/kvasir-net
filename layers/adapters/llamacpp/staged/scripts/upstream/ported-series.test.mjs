import assert from "node:assert/strict";
import test from "node:test";
import { parseRecordArguments, validatePortedSeries } from "./ported-series.mjs";

const patches = [
  { file: "0001-first.patch", sha256: "a".repeat(64) },
  { file: "0002-second.patch", sha256: "b".repeat(64) },
];

const commits = [
  { sha: "1".repeat(40), parent: "0".repeat(40), subject: "compat: 0001-first" },
  { sha: "2".repeat(40), parent: "1".repeat(40), subject: "compat: 0002-second" },
];

test("record arguments require an explicit candidate and official target", () => {
  assert.deepEqual(
    parseRecordArguments(["--candidate", "work", "--target", "master", "--from", "abcdef123"]),
    { candidate: "work", target: "master", from: "abcdef123", replace: false },
  );
  assert.equal(
    parseRecordArguments(["--candidate", "work", "--target", "master", "--replace"]).replace,
    true,
  );
  assert.throws(() => parseRecordArguments(["--target", "master"]), /usage/u);
  assert.throws(
    () => parseRecordArguments(["--candidate", "work", "--target"]),
    /--target requires/u,
  );
  assert.throws(
    () => parseRecordArguments([
      "--candidate", "work", "--target", "master", "--from", "../escape",
    ]),
    /nine-character lowercase commit prefix/u,
  );
});

test("a port preserves one named linear commit per compatibility patch", () => {
  assert.deepEqual(
    validatePortedSeries(patches, commits).map(({ patch, commit }) => ({
      file: patch.file,
      commit,
    })),
    [
      { file: "0001-first.patch", commit: "1".repeat(40) },
      { file: "0002-second.patch", commit: "2".repeat(40) },
    ],
  );
});

test("a port rejects count, name, and linear-history drift", () => {
  assert.throws(() => validatePortedSeries(patches, commits.slice(0, 1)), /expected 2/u);
  assert.throws(
    () => validatePortedSeries(patches, [{ ...commits[0], subject: "fix it" }, commits[1]]),
    /must be named/u,
  );
  assert.throws(
    () => validatePortedSeries(patches, [commits[0], { ...commits[1], parent: "f".repeat(40) }]),
    /not linear/u,
  );
});
