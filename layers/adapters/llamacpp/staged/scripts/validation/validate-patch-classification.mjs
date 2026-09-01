#!/usr/bin/env node
// Enforces the compat queue's tri-split.
//
//   node validate-patch-classification.mjs --manifest <path>
//
// The split is what keeps an upstream pin bump affordable: an `upstream_fix`
// can be dropped on its own the moment upstream absorbs it, and a
// `model_feature` can be skipped for a build that does not want that model,
// but only if every patch declares which it is and stays inside the files that
// class is allowed to touch. Left undeclared, the queue collapses back into
// one blob where a single conflict blocks everything.
//
// Classes:
//   stage_hook    P4's own mechanism reaching into llama core. Only these may
//                 touch llama internals, and only under src/ and include/.
//   upstream_fix  A defect fix that is not about staging at all. Confined to
//                 the ggml layer, which is why these survived a 69-commit
//                 move untouched.
//   model_feature A model or speculative-decoding port. Never a mechanism.
import fs from "node:fs";
import path from "node:path";

const CLASSES = {
  stage_hook: {
    allow: [/^src\//, /^include\/llama\.h$/],
    deny: [/^ggml\//],
    why: "P4 mechanism inside llama core; must not reach below the llama layer",
  },
  upstream_fix: {
    allow: [/^ggml\//, /^src\//, /^tests\//],
    deny: [],
    why: "an upstream defect fix, droppable on its own once upstream absorbs it",
  },
  model_feature: {
    allow: [/^src\//, /^include\//, /^common\//, /^tests\//, /^tools\//],
    deny: [],
    why: "a model or speculative port, never a staging mechanism",
  },
};

function argument(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : null;
}

const manifestPath = argument("--manifest");
if (!manifestPath) {
  console.error("usage: validate-patch-classification.mjs --manifest <path>");
  process.exit(2);
}

const directory = path.dirname(path.resolve(manifestPath));
const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
const failures = [];
const counts = {};

for (const entry of manifest.patches) {
  const declared = entry.layer;
  if (!declared) {
    failures.push(`${entry.file}: no layer declared`);
    continue;
  }
  const rules = CLASSES[declared];
  if (!rules) {
    failures.push(`${entry.file}: unknown layer ${declared}`);
    continue;
  }
  counts[declared] = (counts[declared] ?? 0) + 1;

  const body = fs.readFileSync(path.join(directory, entry.file), "utf8");
  const touched = [...body.matchAll(/^diff --git a\/(\S+) b\/\S+/gm)].map((m) => m[1]);
  if (touched.length === 0) failures.push(`${entry.file}: touches no file`);
  for (const file of touched) {
    if (rules.deny.some((pattern) => pattern.test(file))) {
      failures.push(`${entry.file} (${declared}): ${file} is out of scope - ${rules.why}`);
    } else if (!rules.allow.some((pattern) => pattern.test(file))) {
      failures.push(`${entry.file} (${declared}): ${file} is not an allowed path for this class`);
    }
  }
}

if (failures.length > 0) {
  for (const failure of failures) console.error(`ERROR ${failure}`);
  console.error(`patch classification: ${failures.length} problem(s)`);
  process.exit(1);
}
console.log(JSON.stringify({ valid: true, total: manifest.patches.length, by_layer: counts }));
