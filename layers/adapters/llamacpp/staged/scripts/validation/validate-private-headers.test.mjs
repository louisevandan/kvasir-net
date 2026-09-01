import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { PERMITTED, privateIncludes, sources, violations } from "./validate-private-headers.mjs";

function tree(files) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "p4-private-headers-"));
  for (const [relative, text] of Object.entries(files)) {
    const full = path.join(root, relative);
    fs.mkdirSync(path.dirname(full), { recursive: true });
    fs.writeFileSync(full, text, "utf8");
  }
  return root;
}

test("the public headers are not private", () => {
  const found = privateIncludes('#include "llama.h"\n#include "llama-cpp.h"\n#include "ggml-backend.h"\n');
  assert.deepEqual(found, []);
});

test("the staging header is private", () => {
  const found = privateIncludes('#include "llama-ext.h"\n');
  assert.deepEqual(found, [{ header: "llama-ext.h", line: 1 }]);
});

test("a private include in the compat unit is permitted", () => {
  const root = tree({ [PERMITTED]: '#include "llama-ext.h"\n' });
  assert.deepEqual(violations(root), []);
});

test("the same include one file over is a violation", () => {
  // The failure this exists to stop: the second crossing, added quietly,
  // which turns one file's pin risk into the whole server's.
  const root = tree({
    [PERMITTED]: '#include "llama-ext.h"\n',
    "runtime/stage.cpp": '#include "llama.h"\n#include "llama-ext.h"\n',
  });
  assert.deepEqual(violations(root), [
    { file: path.join("runtime", "stage.cpp"), header: "llama-ext.h", line: 2 },
  ]);
});

test("angled and pathed forms are caught too", () => {
  const root = tree({ "runtime/stage.cpp": '#include <llama-model.h>\n#include "../src/ggml-impl.h"\n' });
  assert.deepEqual(violations(root).map((v) => v.header), ["llama-model.h", "ggml-impl.h"]);
});

test("a compat unit that stops needing the include is still clean", () => {
  const root = tree({ [PERMITTED]: '#include "llama.h"\n' });
  assert.deepEqual(violations(root), []);
});

test("headers are scanned, not only sources", () => {
  const root = tree({ "runtime/stage.hpp": '#include "llama-context.h"\n' });
  assert.equal(violations(root).length, 1);
  assert.equal(sources(root).length, 1);
});
