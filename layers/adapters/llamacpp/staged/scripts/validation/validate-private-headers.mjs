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
//
// `common/` is a third thing: shipped, but a convenience library for
// llama.cpp's own tools rather than an API, and it moves freely between
// versions. It is not confinable today - the stage runtime's public header
// takes `common_params` by value and holds `common_prompt_checkpoint` and
// `common_speculative_ptr` as members - so it is measured instead: the files
// that depend on it are listed, and the gate fails when a new one appears.
// That is the difference between a debt and a leak.
const PRIVATE = /^(llama-(?!cpp\.h$)[a-z0-9-]+\.h|ggml-impl\.h|ggml-backend-impl\.h|ggml-common\.h)$/;

/// llama.cpp's convenience library. Unstable, but load-bearing here.
const UNSTABLE = /^(common|sampling|speculative|arg|log|chat)\.h$/;

/// The files that depend on `common/` as of 2026-09-01. This list is debt,
/// not permission: U0 (3b) is to move these behind a P4-owned facade, and
/// until then the gate's job is to stop the list from growing.
export const UNSTABLE_DEBT = [
  "main.cpp",
  "runtime/llama_stage_runtime.hpp",
  "runtime/request_options.cpp",
  "runtime/request_options.hpp",
  "runtime/request_options_grammar.hpp",
  "runtime/request_stops.cpp",
  "runtime/stage_memory_plan.cpp",
  "runtime/stage_memory_plan.hpp",
  "server/plan.cpp",
  "server/plan.hpp",
];

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
  return includesMatching(text, PRIVATE);
}

/// Includes of llama.cpp's convenience library in one file.
export function unstableIncludes(text) {
  return includesMatching(text, UNSTABLE);
}

function includesMatching(text, pattern) {
  const found = [];
  text.split(/\r?\n/).forEach((line, index) => {
    const match = /^\s*#\s*include\s*[<"]([^">]+)[">]/.exec(line);
    if (!match) return;
    const header = path.basename(match[1]);
    if (pattern.test(header)) found.push({ header, line: index + 1 });
  });
  return found;
}

/// Files depending on `common/` that the debt list does not already name,
/// and names in the list that no longer depend on it. Both are failures: the
/// first is the list growing, the second is a stale record of a debt paid.
export function unstableDrift(root, files = sources(root), debt = UNSTABLE_DEBT) {
  const seen = new Set();
  for (const file of files) {
    if (unstableIncludes(fs.readFileSync(file, "utf8")).length > 0) {
      seen.add(path.relative(root, file).split(path.sep).join("/"));
    }
  }
  const known = new Set(debt);
  return {
    added: [...seen].filter((file) => !known.has(file)).sort(),
    paid: [...known].filter((file) => !seen.has(file)).sort(),
    total: seen.size,
  };
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
  const drift = unstableDrift(root);
  if (drift.added.length > 0 || drift.paid.length > 0) {
    for (const file of drift.added) {
      process.stderr.write(`${file}: new dependency on llama.cpp's common/;`
        + " route it through a P4-owned facade (U0 3b)\n");
    }
    for (const file of drift.paid) {
      process.stderr.write(`${file}: no longer depends on common/;`
        + " remove it from UNSTABLE_DEBT\n");
    }
    process.exitCode = 1;
    return;
  }
  process.stdout.write(`private-headers: ${sources(root).length} files clean,`
    + ` crossings confined to ${PERMITTED};`
    + ` ${drift.total} file(s) still on llama.cpp common/ (U0 3b debt)\n`);
}

import { pathToFileURL } from "node:url";

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main();
