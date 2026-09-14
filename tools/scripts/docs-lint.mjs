#!/usr/bin/env node
// Documentation gate. Errors on:
//   1. mixed EOL within one file;
//   2. reintroduction of a phrase a review retired (README included);
//   3. a claim owned by one document appearing in another (R1, mechanical);
//   4. a docs/*.md page the README does not actually link;
//   5. a code anchor of the form `path.ext` @ hash with no ::symbol (R2).
//   6. a present document-map omitting an in-scope page or linking a missing file.
// Scope: every project-owned Markdown file, recursively. Vendored llama.cpp
// (upstream/), build output (target/), and VCS internals are excluded.
// This is a literal-string canary: it cannot catch a semantic restatement in
// different words — that remains review's job.
// Usage: node tools/scripts/docs-lint.mjs [ROOT] [--all]
//   default: git-tracked Markdown only; --all: every Markdown on disk.
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const args = process.argv.slice(2);
const sweepAll = args.includes("--all");
const root = path.resolve(args.find((a) => a !== "--all") ?? ".");
const SKIP_DIRS = new Set([".git", ".cache", "node_modules", "target", "tmp_dummy", "upstream"]);

// Retirement is permanent; never delete entries. Each names the review that
// retired the phrase so a hit explains itself.
const RETIRED = [
  { phrase: "하나라도 실패 → Abort", reason: "2026-08-31 review: commit 단계에서 성립하지 않음 — 2PC 수렴 규칙 표를 참조하라" },
  { phrase: "원장 부재의 증거", reason: "2026-08-31 review: D4 원인은 미규명 — 원장은 검출 수단이지 원인 단정이 아니다" },
  { phrase: "복원 → 요청 프롬프트와 LCP 대조", reason: "2026-08-31 7차: 순서 역전 — LCP는 상태 import 전에 tokens.bin으로 판정한다(복원 판정 사다리)" },
];

// R1 mechanical check: a claim lives in exactly one file; any other file must
// link instead of restating. Needles are distinctive fragments of the claim.
const OWNED_CLAIMS = [
  { needle: "P-1 → P0 → P1a", owner: "docs/adapter-restructure-plan.md", what: "단계 순서" },
  { needle: "Persist는 roll-forward", owner: "docs/kv-state-store-convention.md", what: "2PC 수렴 방향" },
  { needle: "committed ≥1 + prepared 잔여", owner: "docs/kv-state-store-convention.md", what: "2PC 수렴 표" },
  { needle: "canonical binary encoding", owner: "docs/kv-state-store-convention.md", what: "모델 정체성 인코딩" },
];

// R2 anchor form: `path::symbol` @ short-commit. A backtick span naming a
// source file that is followed by "@ <hex>" must carry a ::symbol.
const ANCHOR = /`([^`\n]+\.(?:rs|cpp|hpp|h|c|mjs)[^`\n]*)`\s*@\s*[0-9a-f]{7,40}\b/g;

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

function trackedMarkdown(dir) {
  const result = spawnSync("git", ["-C", dir, "ls-files", "--", "*.md"], { encoding: "utf8" });
  if (result.status !== 0) return null;
  return result.stdout.split(/\r?\n/).filter(Boolean).map((f) => path.join(dir, f));
}
// Official mode checks git-tracked Markdown so an unrelated untracked draft
// cannot fail the gate or force its way into a scoped commit. --all sweeps
// the filesystem; outside a git tree we fall back to the sweep.
const files = sweepAll ? walk(root, []) : (trackedMarkdown(root) ?? walk(root, []));
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
    if (text.includes(phrase)) fail(`${rel}: retired phrase "${phrase}" — ${reason}`);
  }
  for (const { needle, owner, what } of OWNED_CLAIMS) {
    if (rel !== owner && text.includes(needle)) {
      fail(`${rel}: restates ${what} owned by ${owner} — link instead`);
    }
  }
  for (const match of text.matchAll(ANCHOR)) {
    if (!match[1].includes("::")) {
      fail(`${rel}: anchor \`${match[1]}\` @ … lacks ::symbol (R2)`);
    }
  }
  if (readme !== null && /^docs\/[^/]+\.md$/.test(rel)
      && !readme.includes(`](docs/${path.basename(rel)})`)) {
    fail(`${rel}: README has no actual link ](docs/${path.basename(rel)})`);
  }
}

// Optional for small/older fixtures, required by the repository's own docs.
// This validates navigation only, never a page's semantic role or correctness.
const mapPath = path.join(root, "docs", "document-map.md");
if (fs.existsSync(mapPath)) {
  const catalog = fs.readFileSync(mapPath, "utf8");
  const linked = new Set();
  for (const match of catalog.matchAll(/\]\(([^)\r\n]+)\)/g)) {
    const href = match[1].replace(/^<|>$/g, "");
    if (/^(?:[a-z][a-z0-9+.-]*:|#)/i.test(href)) continue;
    const target = path.resolve(path.dirname(mapPath), href.split("#")[0]);
    if (!fs.existsSync(target)) {
      fail(`docs/document-map.md: missing target ${href}`);
    } else {
      linked.add(target);
    }
  }
  for (const file of files) {
    if (!linked.has(path.resolve(file))) {
      fail(`docs/document-map.md: unlisted document ${path.relative(root, file).replaceAll("\\", "/")}`);
    }
  }
}

if (errors) { console.error(`docs-lint: ${errors} error(s) across ${files.length} files`); process.exit(1); }
console.log(`docs-lint: ${files.length} files clean`);
