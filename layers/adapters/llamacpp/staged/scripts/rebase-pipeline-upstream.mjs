#!/usr/bin/env node
// Rebases the compat queue onto a new upstream pin, one patch at a time.
//
// Usage:
//   P4_REBASE_WT=<worktree> P4_VERIFY_WT=<worktree> P4_REBASE_SRC=<compat dir> \n//   P4_OBSERVED_AT=<iso8601> node rebase-pipeline-upstream.mjs <target-sha>
//
// Both worktrees must be fresh detached checkouts of the target commit.
//
// Each patch is applied on top of the ones before it and re-recorded as the
// incremental diff it produced, so every patch keeps owning exactly the change
// it owned before - the queue's semantic split (stage_hook / upstream_fix /
// model_feature) survives instead of collapsing into one blob.
//
// Phase 1 rebases, committing after each patch so a failed escalation can be
// undone without losing the patches already placed. Phase 2 replays the
// recorded set on a pristine tree with plain `git apply`, which both verifies
// the result and produces the aggregate identity the way prepare computes it.
//
// Escalation per patch: git apply -> git apply -3 -> GNU patch with fuzz ->
// a named manual fix. Anything still rejected stops the run.
import crypto from "node:crypto";
import { execSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const wt = process.env.P4_REBASE_WT;
const verifyWt = process.env.P4_VERIFY_WT;
const src = process.env.P4_REBASE_SRC;
const target = process.argv[2];
const dest = path.join(path.dirname(src), target.slice(0, 9));

const sha256 = (data) => crypto.createHash("sha256").update(data).digest("hex");
const gitIn = (dir) => (args, opts = {}) =>
  execSync(`git -C ${JSON.stringify(dir)} ${args}`, { maxBuffer: 512 * 1024 * 1024, ...opts });
const git = gitIn(wt);
const sweep = (dir) => {
  const listed = execSync(`git -C ${JSON.stringify(dir)} ls-files --others --exclude-standard`,
    { encoding: "utf8" }).split(/\r?\n/);
  for (const n of listed.filter((f) => f.endsWith(".rej") || f.endsWith(".orig"))) {
    fs.rmSync(path.join(dir, n), { force: true });
  }
};
// A failed three-way apply leaves unmerged index entries that `checkout`
// refuses to overwrite, so undo is a hard reset to the last recorded patch.
const restore = () => {
  spawnSync("git", ["-C", wt, "reset", "-q", "--hard"], { encoding: "utf8" });
  sweep(wt);
};
const tryGit = (args) => {
  const r = spawnSync("git", ["-C", wt, ...args], { encoding: "utf8" });
  if (r.status !== 0) restore();
  return r.status === 0;
};

// Hunks upstream drift moved beyond what fuzz can place.
const MANUAL = {
  "0009-model-loader.patch": () => {
    // Upstream restructured create_tensor around the stage-ownership block.
    // Re-apply the patch hunk by hunk with maximum fuzz and accept partial
    // placement only if nothing is left rejected afterwards.
    const r = spawnSync("patch", ["-p1", "--fuzz=3", "-l", "--forward",
      "--no-backup-if-mismatch", "-i", path.join(src, "0009-model-loader.patch")],
      { cwd: wt, encoding: "utf8" });
    const left = execSync(`git -C ${JSON.stringify(wt)} ls-files --others --exclude-standard`,
      { encoding: "utf8" }).split(/\r?\n/).filter((n) => n.endsWith(".rej"));
    if (left.length > 0) throw new Error(`0009 still rejects: ${left.join(" ")}`);
    return r.status === 0;
  },
  "0016-stage-memory-residency.patch": () => {
    const file = path.join(wt, "src/llama-model.cpp");
    const before = fs.readFileSync(file, "utf8");
    // Every memory family must be constructed through the residency gate.
    // Upstream keeps adding construction sites (and whole families); a site
    // that bypasses the gate silently re-enables partial stages for a family
    // whose state layout was never audited.
    // Match any memory family, including ones upstream adds after this fix was
    // written - naming them explicitly is exactly how a new family keeps its
    // bypass unnoticed.
    const after = before.replace(
      /res = new (llama_(?:kv_cache|memory)[A-Za-z0-9_]*)\(/g,
      (_m, cls) => `res = llama_linkcpp_new_memory<${cls}>(`,
    );
    if (after === before) return false;
    fs.writeFileSync(file, after);
    return true;
  },
  "0017-recurrent-stage-memory-residency.patch": () => {
    const CRLF = "\r\n";
    let changed = false;
    const header = path.join(wt, "src/llama-memory-recurrent.h");
    const h = fs.readFileSync(header, "utf8");
    if (!h.includes("resident_l")) {
      const anchor = "    std::vector<ggml_tensor *> s_l;";
      if (!h.includes(anchor)) throw new Error("recurrent header anchor missing");
      fs.writeFileSync(header, h.replace(anchor, `${anchor}${CRLF}    std::vector<bool> resident_l;`));
      changed = true;
    }
    const impl = path.join(wt, "src/llama-memory-recurrent.cpp");
    const c = fs.readFileSync(impl, "utf8");
    if (!c.includes("ctx_for_meta")) {
      const anchor = ["        return it->second.get();", "    };", "",
        "    r_l.resize(n_layer);", "    s_l.resize(n_layer);"].join(CRLF);
      if (!c.includes(anchor)) throw new Error("recurrent ctor anchor missing");
      const body = ["        return it->second.get();", "    };", "",
        "    auto ctx_for_meta = [&]() -> ggml_context * {",
        "        if (!ctx_meta) {",
        "            ggml_init_params params = {",
        "                /*.mem_size   =*/ size_t(2u*n_layer*ggml_tensor_overhead()),",
        "                /*.mem_buffer =*/ NULL,",
        "                /*.no_alloc   =*/ true,",
        "            };",
        "            ctx_meta.reset(ggml_init(params));",
        "        }",
        "        return ctx_meta.get();",
        "    };", "",
        "    r_l.resize(n_layer);", "    s_l.resize(n_layer);",
        "    resident_l.resize(n_layer, false);"].join(CRLF);
      fs.writeFileSync(impl, c.replace(anchor, body));
      changed = true;
    }
    return changed;
  },
};

// Applied after the owning patch however it landed, so a fuzzed placement
// cannot leave a newer upstream construction site outside the gate.
const POST = {
  "0016-stage-memory-residency.patch": MANUAL["0016-stage-memory-residency.patch"],
};

fs.mkdirSync(dest, { recursive: true });
const manifest = JSON.parse(fs.readFileSync(path.join(src, "manifest.json"), "utf8"));
const report = [];

// ── phase 1: rebase ───────────────────────────────────────────────────────
for (const entry of manifest.patches) {
  const patch = path.join(src, entry.file);
  let how;
  if (tryGit(["apply", patch])) how = "clean";
  else if (tryGit(["apply", "-3", patch])) how = "3way";
  else {
    const fuzz = spawnSync("patch", ["-p1", "--fuzz=3", "--forward",
      "--no-backup-if-mismatch", "-i", patch], { cwd: wt, encoding: "utf8" });
    const rejects = execSync(`git -C ${JSON.stringify(wt)} ls-files --others --exclude-standard`,
      { encoding: "utf8" }).split(/\r?\n/).filter((n) => n.endsWith(".rej"));
    if (fuzz.status === 0 && rejects.length === 0) how = "fuzz";
    else {
      const fix = MANUAL[entry.file];
      if (!fix) throw new Error(`${entry.file}: rejected with no manual fix registered`);
      restore();
      if (!fix()) throw new Error(`${entry.file}: manual fix changed nothing`);
      how = "manual";
    }
  }
  sweep(wt);
  // A patch can land by fuzz and still leave upstream's newer construction
  // sites ungated, so the gate is re-asserted after every patch that owns it
  // rather than only when the patch failed outright.
  if (POST[entry.file]) POST[entry.file]();
  // GNU patch and a three-way apply can leave part of the change staged, so
  // the increment is measured against the last commit rather than the index.
  git("add -A");
  const produced = git("diff --cached --binary --full-index HEAD");
  if (produced.length === 0) throw new Error(`${entry.file}: produced no change`);
  fs.writeFileSync(path.join(dest, entry.file),
    Buffer.from(produced.toString("utf8").replace(/\r\n/g, "\n"), "utf8"));
  git(`-c user.name=rebase -c user.email=rebase@local commit -q -m ${JSON.stringify(entry.file)}`);
  report.push({ file: entry.file, how });
}

// ── phase 2: verify the recorded set on a pristine tree ───────────────────
const vgit = gitIn(verifyWt);
for (const entry of manifest.patches) {
  const r = spawnSync("git", ["-C", verifyWt, "apply", path.join(dest, entry.file)],
    { encoding: "utf8" });
  if (r.status !== 0) throw new Error(`recorded ${entry.file} does not replay: ${r.stderr}`);
}
const aggregate = vgit("diff --binary --full-index");
vgit("add -A");
const patchedTree = vgit("write-tree", { encoding: "utf8" }).trim();
const api = fs.readFileSync(path.join(verifyWt, "include", "llama.h"), "utf8");
if (!api.includes("llama_linkcpp_runtime_configure")) {
  throw new Error("replayed source does not expose the pipeline compatibility ABI");
}
// Fail-closed only holds if every memory family is constructed through the
// residency gate. Upstream adds families and construction sites between pins,
// and a bypassed site re-enables partial stages for a family whose state
// layout was never audited - so this is checked, not assumed.
const modelSource = fs.readFileSync(path.join(verifyWt, "src", "llama-model.cpp"), "utf8");
const bypass = [...modelSource.matchAll(/res = new (llama_(?:kv_cache|memory)[A-Za-z0-9_]*)\(/g)]
  .map((m) => m[1]);
if (bypass.length > 0) {
  throw new Error(`memory constructions bypass the residency gate: ${[...new Set(bypass)].join(", ")}`);
}

const next = {
  ...manifest,
  upstream_commit: target,
  upstream_commit_date: git(`log -1 --format=%cI ${target}`, { encoding: "utf8" }).trim(),
  upstream_subject: git(`log -1 --format=%s ${target}`, { encoding: "utf8" }).trim(),
  observed_at: process.env.P4_OBSERVED_AT,
  nearest_release_tag: (() => {
    try { return git(`describe --tags --abbrev=0 ${target}`, { encoding: "utf8" }).trim(); }
    catch { return manifest.nearest_release_tag; }
  })(),
  previous_linker_pin: manifest.upstream_commit,
  previous_official_base: manifest.upstream_commit,
  patch_set_sha256: sha256(aggregate),
  patched_tree: patchedTree,
  patches: manifest.patches.map((entry) => ({
    ...entry,
    sha256: sha256(fs.readFileSync(path.join(dest, entry.file))),
  })),
};
fs.writeFileSync(path.join(dest, "manifest.json"), `${JSON.stringify(next, null, 2)}\n`);

const counts = report.reduce((acc, r) => ({ ...acc, [r.how]: (acc[r.how] ?? 0) + 1 }), {});
process.stdout.write(`${JSON.stringify(counts)}\n`);
for (const r of report.filter((r) => r.how !== "clean")) {
  process.stdout.write(`  ${r.how.padEnd(6)} ${r.file}\n`);
}
process.stdout.write(`replay verified: ${manifest.patches.length}/${manifest.patches.length} clean on pristine ${target.slice(0, 9)}\n`);
process.stdout.write(`patch_set_sha256 ${next.patch_set_sha256}\npatched_tree ${next.patched_tree}\n`);
