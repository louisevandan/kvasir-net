import assert from "node:assert/strict";
import test from "node:test";
import { OFFICIAL_REPOSITORY, nextManifest, parseArguments, summarize } from "./manifest.mjs";

const TARGET = "7ba604f1cb61aaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BASE = "3e3a7a416d6588597523c792fc5874a543d59ed9";
const HASH = "a".repeat(64);

const previous = {
  schema_version: 1,
  upstream_repository: OFFICIAL_REPOSITORY,
  upstream_commit: BASE,
  upstream_commit_date: "2026-08-05T11:03:23+02:00",
  upstream_subject: "old subject",
  nearest_release_tag: "b10242",
  previous_linker_pin: "4308a4f035791f58ae111f56c39dac598bf476be",
  patch_set_sha256: "b".repeat(64),
  patched_tree: "c".repeat(40),
  patches: [{ file: "0001-ggml-backend.patch", sha256: "d".repeat(64) }],
};

const identity = {
  date: "2026-08-09T00:42:50+02:00",
  subject: "server: report the isolate working directory",
  tag: "b10331",
};

function bumped(overrides = {}) {
  return nextManifest(previous, {
    target: TARGET,
    identity,
    patches: [{ file: "0001-ggml-backend.patch", sha256: HASH }],
    patchSetSha256: HASH,
    patchedTree: "e".repeat(40),
    observedAt: "2026-08-12T02:00:00.000Z",
    ...overrides,
  });
}

test("a ref is required", () => {
  assert.throws(() => parseArguments([]), /usage/u);
});

test("flags are parsed independently of the ref", () => {
  assert.deepEqual(parseArguments(["master"]), { ref: "master", apply: false, from: null });
  assert.deepEqual(parseArguments(["master", "--apply"]), {
    ref: "master",
    apply: true,
    from: null,
  });
  assert.deepEqual(parseArguments(["--apply", "b10331", "--from", "3e3a7a416"]), {
    ref: "b10331",
    apply: true,
    from: "3e3a7a416",
  });
});

test("--from requires a value", () => {
  assert.throws(() => parseArguments(["master", "--from"]), /--from requires/u);
  assert.throws(() => parseArguments(["master", "--from", "--apply"]), /--from requires/u);
});

test("a bump records the pin it was derived from", () => {
  const manifest = bumped();
  assert.equal(manifest.upstream_commit, TARGET);
  assert.equal(manifest.previous_linker_pin, BASE);
  assert.equal(manifest.previous_official_base, BASE);
  assert.equal(manifest.nearest_release_tag, "b10331");
  assert.equal(manifest.upstream_subject, identity.subject);
});

test("re-recording one official target preserves its original lineage", () => {
  const manifest = bumped({ target: BASE });
  assert.equal(manifest.upstream_commit, BASE);
  assert.equal(manifest.previous_linker_pin, previous.previous_linker_pin);
  assert.equal(manifest.previous_official_base, previous.previous_official_base);
});

test("a bump never rewrites the official repository or the schema", () => {
  const manifest = bumped();
  assert.equal(manifest.upstream_repository, OFFICIAL_REPOSITORY);
  assert.equal(manifest.schema_version, previous.schema_version);
});

test("a bump refuses an abbreviated commit", () => {
  assert.throws(() => bumped({ target: "7ba604f1cb61" }), /full sha/u);
});

test("a bump refuses an empty or malformed patch set", () => {
  assert.throws(() => bumped({ patches: [] }), /at least one patch/u);
  assert.throws(
    () => bumped({ patches: [{ file: "0001.patch", sha256: "short" }] }),
    /invalid sha256/u,
  );
});

test("a series is adoptable only when every patch applies", () => {
  assert.deepEqual(
    summarize([
      { file: "0001.patch", ok: true },
      { file: "0002.patch", ok: true },
    ]),
    { total: 2, conflicts: [], adoptable: true },
  );
  assert.deepEqual(
    summarize([
      { file: "0001.patch", ok: true },
      { file: "0009.patch", ok: false },
    ]),
    { total: 2, conflicts: ["0009.patch"], adoptable: false },
  );
});
