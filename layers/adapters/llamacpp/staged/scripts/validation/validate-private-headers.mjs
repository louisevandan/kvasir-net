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
// `p4_llama_compat_internal.hpp` counts as a `common/` dependency because
// that is exactly what it is: the header a file includes to say it still
// needs the struct. Counting only direct `common.h` includes would let the
// debt look paid the moment it was routed through one more file.
const UNSTABLE = /^(common|sampling|speculative|arg|log|chat)\.h$|^p4_llama_compat_internal.hpp$/;

/// The files that depend on llama.cpp's `common/`, split by what each costs.
///
/// **Both lists end at zero.** U0 (3) says the server keeps public `llama.h`
/// and P4's own ABI, with internal access inside the compat implementation;
/// `common/` is upstream's convenience library, it changes freely, and every
/// file naming it is a file an upstream bump can break. The remaining
/// implementation files are a blast radius, not a resting place - the count
/// is in the lists below rather than in this sentence, so it cannot go
/// stale the way it did.
///
/// They are split because they are paid in that order, not because the second
/// is permitted. A header's dependency reaches every translation unit that
/// includes it, so headers come first and are the nearer gate.
///
/// Neither list is enforced by the compiler yet: CMake still puts llama.cpp's
/// `common` on the runtime target's PUBLIC include path, so this script is
/// all that stands there. Making that path private to the facade is part of
/// finishing (3b), and only then does the rule become true rather than
/// merely checked.
/// The facade: the files allowed to know llama.cpp's convenience library.
/// Not debt - this is where the debt is being moved to.
export const FACADE = [
  "compat/p4_llama_compat.cpp",
  "compat/p4_llama_compat_internal.hpp",
];

/// Empty since 2026-09-02, and the gate keeps it that way: a header naming
/// llama.cpp's convenience library is now a failure, not an entry.
export const UNSTABLE_HEADER_DEBT = [
];

/// Implementation files that still name it directly.
export const UNSTABLE_SOURCE_DEBT = [
  "runtime/llama_stage_mtp_ownership_test.cpp",
  "runtime/llama_stage_runtime_compile_test.cpp",
  "runtime/request_options.cpp",
  "runtime/request_options_grammar.cpp",
  "runtime/request_options_test.cpp",
];

export const UNSTABLE_DEBT = [...UNSTABLE_HEADER_DEBT, ...UNSTABLE_SOURCE_DEBT];

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
  // The facade is where the dependency is supposed to live, so it is neither
  // debt nor drift.
  for (const file of FACADE) seen.delete(file);
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
  const headerDebt = drift.added.filter((file) => file.endsWith(".hpp"));
  if (headerDebt.length > 0) {
    for (const file of headerDebt) {
      process.stderr.write(`${file}: a header may no longer depend on llama.cpp's`
        + " common/ - that dependency reaches every translation unit including it\n");
    }
    process.exitCode = 1;
    return;
  }
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
    + ` common/ debt ${UNSTABLE_HEADER_DEBT.length} header(s)`
    + ` + ${UNSTABLE_SOURCE_DEBT.length} source(s) (U0 3b)\n`);
}

import { pathToFileURL } from "node:url";

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main();
