import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const builder = fs.readFileSync(path.join(scriptDir, "build-stage-server.mjs"), "utf8");
const cmake = fs.readFileSync(path.join(scriptDir, "..", "server", "CMakeLists.txt"), "utf8");

test("official staged builder builds every registered CTest executable", () => {
  const registered = [...cmake.matchAll(/add_test\(NAME\s+(p4_staged_[a-z_]+_test)\b/gu)]
    .map((match) => match[1]);
  assert.ok(registered.length > 0, "CMake must register staged tests");
  for (const target of registered) {
    assert.match(builder, new RegExp(`"${target}"`, "u"),
      `${target} is registered for CTest but absent from the official build targets`);
  }
});
