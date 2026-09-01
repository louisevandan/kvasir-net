#!/usr/bin/env node
// Keeps the stage server's reach into llama.cpp inside one file.
//
//   node validate-private-headers.mjs [--root <server src dir>]
//
// llama.cpp publishes `include/llama.h` and `include/llama-cpp.h`. Everything
// else - `src/llama-ext.h` above all, which announces itself as a staging
// surface where breaking changes are allowed - is private, and a pin that
// moves one of those signatures breaks every file that named it.
//
// U0 (3) bounds that blast radius to one translation unit: the compat unit
// includes the private header, hands back P4's own types, and the rest of the
// server never learns the private names exist. This gate is what stops the
// second such include from being added without a decision.
import fs from "node:fs";
import path from "node:path";

// Anything under llama.cpp's src/ is private. The two public headers live in
// include/, and ggml's public surface is its own `ggml*.h` set.
const PRIVATE = /^(llama-(?!cpp\.h$)[a-z0-9-]+\.h|ggml-impl\.h|ggml-backend-impl\.h|ggml-common\.h)$/;

/// The single file allowed to cross, relative to the scanned root.
export const PERMITTED = path.join("compat", "p4_llama_compat.cpp");

export function sources(root) {
  const found = [];
  const walk = (directory) => {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const full = path.join(directory, entry.name);
      if (entry.isDirectory()) walk(full);
      else if (/\.(cpp|hpp|h|inc)$/.test(entry.name)) found.push(full);
    }
  };
  walk(root);
  return found.sort();
}

/// Every private-header include in one file, as `{ header, line }`.
export function privateIncludes(text) {
  const found = [];
  text.split(/\r?\n/).forEach((line, index) => {
    const match = /^\s*#\s*include\s*[<"]([^">]+)[">]/.exec(line);
    if (!match) return;
    const header = path.basename(match[1]);
    if (PRIVATE.test(header)) found.push({ header, line: index + 1 });
  });
  return found;
}

/// Violations are private includes outside the one permitted file. A missing
/// include *inside* it is not a violation: the compat unit is allowed to stop
/// needing one.
export function violations(root, files = sources(root)) {
  const found = [];
  for (const file of files) {
    const relative = path.relative(root, file);
    if (relative === PERMITTED) continue;
    for (const { header, line } of privateIncludes(fs.readFileSync(file, "utf8"))) {
      found.push({ file: relative, header, line });
    }
  }
  return found;
}

function main() {
  const index = process.argv.indexOf("--root");
  const root = index >= 0 && process.argv[index + 1]
    ? process.argv[index + 1]
    : path.join(import.meta.dirname, "..", "..", "server", "src");
  const found = violations(root);
  if (found.length > 0) {
    for (const { file, header, line } of found) {
      process.stderr.write(`${file}:${line}: private llama header \`${header}\`;`
        + ` route it through ${PERMITTED}\n`);
    }
    process.stderr.write(`private-headers: ${found.length} violation(s)\n`);
    process.exitCode = 1;
    return;
  }
  process.stdout.write(`private-headers: ${sources(root).length} files clean,`
    + ` crossings confined to ${PERMITTED}\n`);
}

import { pathToFileURL } from "node:url";

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main();
