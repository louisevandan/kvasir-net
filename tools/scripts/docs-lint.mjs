#!/usr/bin/env node
// Documentation gate. Errors on:
//   1. mixed EOL within one file;
//   2. reintroduction of a phrase a review retired (README included);
//   3. a claim owned by one document appearing in another (R1, mechanical);
//   4. a docs/*.md page missing from the README table.
// Scope: every project-owned Markdown file, recursively. Vendored llama.cpp
// (upstream/), build output (target/), and VCS internals are excluded.
// Usage: node tools/scripts/docs-lint.mjs [ROOT]   (ROOT defaults to cwd)
import fs from "node:fs";
import path from "node:path";

const root = path.resolve(process.argv[2] ?? ".");
const SKIP_DIRS = new Set([".git", "node_modules", "target", "tmp_dummy", "upstream"]);

// Retirement is permanent; never delete entries. Each names the review that
// retired the phrase so a hit explains itself.
const RETIRED = [
  { phrase: "\ud558\ub098\ub77c\ub3c4 \uc2e4\ud328 \u2192 Abort", reason: "2026-08-31 review: commit \ub2e8\uacc4\uc5d0\uc11c \uc131\ub9bd\ud558\uc9c0 \uc54a\uc74c \u2014 2PC \uc218\ub834 \uaddc\uce59 \ud45c\ub97c \ucc38\uc870\ud558\ub77c" },
  { phrase: "\uc6d0\uc7a5 \ubd80\uc7ac\uc758 \uc99d\uac70", reason: "2026-08-31 review: D4 \uc6d0\uc778\uc740 \ubbf8\uaddc\uba85 \u2014 \uc6d0\uc7a5\uc740 \uac80\ucd9c \uc218\ub2e8\uc774\uc9c0 \uc6d0\uc778 \ub2e8\uc815\uc774 \uc544\ub2c8\ub2e4" },
];

// R1 mechanical check: a claim lives in exactly one file; any other file must
// link instead of restating. Needles are distinctive fragments of the claim.
const OWNED_CLAIMS = [
  { needle: "P-1 \u2192 P0 \u2192 P1a", owner: "docs/adapter-restructure-plan.md", what: "\ub2e8\uacc4 \uc21c\uc11c" },
  { needle: "Persist\ub294 roll-forward", owner: "docs/kv-state-store-convention.md", what: "2PC \uc218\ub834 \ubc29\ud5a5" },
  { needle: "committed \u22651 + prepared \uc794\uc5ec", owner: "docs/kv-state-store-convention.md", what: "2PC \uc218\ub834 \ud45c" },
];

function walk(dir, out) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.isDirectory()) {
      if (!SKIP_DIRS.has(entry.name)) walk(path.join(dir, entry.name), out);
    } else if (entry.name.endsWith(".md")) {
      out.push(path.join(dir, entry.name));
    }
  }
  return out;
}

const files = walk(root, []);
const readmePath = path.join(root, "README.md");
const readme = fs.existsSync(readmePath) ? fs.readFileSync(readmePath, "utf8") : null;
let errors = 0;
const fail = (message) => { console.error(`ERROR ${message}`); errors += 1; };

for (const file of files) {
  const rel = path.relative(root, file).replaceAll("\\", "/");
  const text = fs.readFileSync(file, "utf8");
  const crlf = (text.match(/\r\n/g) ?? []).length;
  const bare = (text.match(/(?<!\r)\n/g) ?? []).length;
  if (crlf > 0 && bare > 0) fail(`${rel}: mixed EOL (${crlf} CRLF, ${bare} LF)`);
  for (const { phrase, reason } of RETIRED) {
    if (text.includes(phrase)) fail(`${rel}: retired phrase "${phrase}" \u2014 ${reason}`);
  }
  for (const { needle, owner, what } of OWNED_CLAIMS) {
    if (rel !== owner && text.includes(needle)) {
      fail(`${rel}: restates ${what} owned by ${owner} \u2014 link instead`);
    }
  }
  if (readme !== null && /^docs\/[^/]+\.md$/.test(rel) && !readme.includes(path.basename(rel))) {
    fail(`${rel}: not indexed in README`);
  }
}

if (errors) { console.error(`docs-lint: ${errors} error(s) across ${files.length} files`); process.exit(1); }
console.log(`docs-lint: ${files.length} files clean`);
