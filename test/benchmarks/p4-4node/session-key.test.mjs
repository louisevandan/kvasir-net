import assert from "node:assert/strict";
import test from "node:test";
import { admittedKeys, checkSessionKeys, expectedKeys } from "./session-key.mjs";

const config = { session_key_template: "sk1:p4-4node/smoke-{{request_id}}" };
const ids = ["req-001", "req-002"];
const goodLog = [
  "some unrelated line",
  "P4_SESSION_KEY_ADMITTED request=req-001 key=sk1:p4-4node/smoke-req-001",
  "P4_SESSION_KEY_ADMITTED request=req-002 key=sk1:p4-4node/smoke-req-002",
].join("\r\n");

test("substitution matches what the drive mints", () => {
  const keys = expectedKeys({ session_key_template: "sk1:t/{{request_index}}-{{request_id}}" }, ids);
  assert.equal(keys.get("req-002"), "sk1:t/2-req-002");
});

test("a matching trace passes", () => {
  const result = checkSessionKeys(config, ids, goodLog);
  assert.equal(result.passed, true);
  assert.equal(result.checked, 2);
});

test("a missing trace is not a pass", () => {
  // The failure this guards: an adapter that drops the field logs nothing,
  // and an empty log must not read as agreement.
  const result = checkSessionKeys(config, ids, "");
  assert.equal(result.passed, false);
  assert.equal(result.mismatches.length, 2);
});

test("a key that changed in transit is caught", () => {
  const log = goodLog.replace("smoke-req-002", "smoke-req-999");
  const result = checkSessionKeys(config, ids, log);
  assert.equal(result.passed, false);
  assert.deepEqual(result.mismatches, [{
    request_id: "req-002",
    expected: "sk1:p4-4node/smoke-req-002",
    admitted: "sk1:p4-4node/smoke-req-999",
  }]);
});

test("an OUTER that mints no key is not checked", () => {
  const result = checkSessionKeys({ session_key_template: "" }, ids, "");
  assert.equal(result.applicable, false);
  assert.equal(result.passed, true);
});

test("a repeated admission under the same key collapses", () => {
  const log = `${goodLog}\r\nP4_SESSION_KEY_ADMITTED request=req-001 key=sk1:p4-4node/smoke-req-001`;
  assert.equal(admittedKeys(log).size, 2);
  assert.equal(checkSessionKeys(config, ids, log).passed, true);
});
