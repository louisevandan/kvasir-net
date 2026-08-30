import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const lint = path.join(path.dirname(fileURLToPath(import.meta.url)), "docs-lint.mjs");

function fixture(files) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "docs-lint-"));
  for (const [name, content] of Object.entries(files)) {
    const full = path.join(dir, name);
    fs.mkdirSync(path.dirname(full), { recursive: true });
    fs.writeFileSync(full, content);
  }
  return dir;
}

function run(dir) {
  return spawnSync(process.execPath, [lint, dir], { encoding: "utf8" });
}

test("clean tree passes", () => {
  const dir = fixture({
    "README.md": "| a | [docs/a.md](docs/a.md) |\n",
    "docs/a.md": "# a\n",
  });
  const result = run(dir);
  assert.equal(result.status, 0, result.stderr);
});

test("mixed EOL in one file fails", () => {
  const dir = fixture({ "README.md": "line1\r\nline2\n" });
  const result = run(dir);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /mixed EOL/);
});

test("retired phrase fails anywhere, README included", () => {
  const dir = fixture({ "README.md": "\ud558\ub098\ub77c\ub3c4 \uc2e4\ud328 \u2192 Abort\n" });
  const result = run(dir);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /retired phrase/);
});

test("an owned claim restated outside its owner fails", () => {
  const dir = fixture({
    "README.md": "| a | [docs/other.md](docs/other.md) |\n",
    "docs/other.md": "\uc21c\uc11c\ub294 P-1 \u2192 P0 \u2192 P1a \ub2e4\n",
  });
  const result = run(dir);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /restates/);
});

test("a docs page missing from the README index fails", () => {
  const dir = fixture({ "README.md": "no index\n", "docs/lost.md": "# lost\n" });
  const result = run(dir);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /no actual link/);
});

test("vendored and build directories are skipped", () => {
  const dir = fixture({
    "README.md": "clean\n",
    "upstream/bad.md": "\ud558\ub098\ub77c\ub3c4 \uc2e4\ud328 \u2192 Abort mixed\r\n\n",
    "target/bad.md": "\uc6d0\uc7a5 \ubd80\uc7ac\uc758 \uc99d\uac70\n",
  });
  const result = run(dir);
  assert.equal(result.status, 0, result.stderr);
});

test("a source-file anchor without ::symbol fails (R2)", () => {
  const dir = fixture({
    "README.md": "| a | [docs/a.md](docs/a.md) |\n",
    "docs/a.md": "the behaviour lives in `server.cpp` @ df5b9ce7 today\n",
  });
  const result = run(dir);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /lacks ::symbol/);
});

test("README naming a docs page without an actual link fails", () => {
  const dir = fixture({
    "README.md": "mentions lost.md by name only\n",
    "docs/lost.md": "# lost\n",
  });
  const result = run(dir);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /no actual link/);
});
