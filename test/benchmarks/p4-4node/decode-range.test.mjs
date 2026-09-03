import assert from "node:assert/strict";
import crypto from "node:crypto";
import test from "node:test";
import { decodeRange } from "./remote.mjs";

// The range is only exact if it arrives exact. These drive the checks that
// say so - the harness's other tests build results by hand and never touch
// this path.

function answer({ head = 1, tail = 1, bytes = null, sha256 = null, text = "one\ntwo\n" } = {}) {
  const buffer = Buffer.from(text, "utf8");
  return [
    `P4_RANGE_HEAD=${head}`,
    `P4_RANGE_TAIL=${tail}`,
    `P4_RANGE_BYTES=${bytes ?? buffer.length}`,
    `P4_RANGE_SHA256=${sha256 ?? crypto.createHash("sha256").update(buffer).digest("hex")}`,
    `P4_RANGE_BASE64=${buffer.toString("base64")}`,
  ].join("\n");
}

test("an intact range decodes", () => {
  const range = decodeRange(0, answer(), 0, 8);
  assert.equal(range.text, "one\ntwo\n");
  assert.equal(range.beginsOnRecord, true);
  assert.equal(range.endsOnRecord, true);
});

test("a length that disagrees is refused", () => {
  assert.throws(() => decodeRange(0, answer({ bytes: 99 })), /lost bytes in transit/);
});

test("a digest that disagrees is refused", () => {
  const wrong = "0".repeat(64);
  assert.throws(() => decodeRange(0, answer({ sha256: wrong })), /digest does not match/);
});

test("a failed remote command is refused even with output", () => {
  assert.throws(() => decodeRange(1, answer()), /unreadable/);
});

test("a missing payload is refused", () => {
  assert.throws(() => decodeRange(0, "P4_RANGE_ERROR record shrank"), /unreadable/);
});

test("boundary flags are read, not assumed", () => {
  const cut = decodeRange(0, answer({ head: 0, tail: 0 }), 0, 8);
  assert.equal(cut.beginsOnRecord, false);
  assert.equal(cut.endsOnRecord, false);
});

test("an empty range is still checked", () => {
  const range = decodeRange(0, answer({ text: "" }), 100, 100);
  assert.equal(range.text, "");
  assert.equal(range.endsOnRecord, true);
});
