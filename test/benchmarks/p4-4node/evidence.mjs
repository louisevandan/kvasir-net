// Run evidence: one immutable directory per run, with everything in it bound
// to the same run id and checksummed.
//
// The previous layout reused a fixed directory per scenario, so a later failed
// attempt overwrote config.json while an earlier report.json stayed behind -
// leaving two files from different runs that looked like one another's
// evidence. Nothing in the report could tell you which binaries, which model,
// or even which machine produced it.
//
// A run therefore builds in `<id>.tmp`, and only a run that produced a report
// is promoted to `<id>`. A failure leaves failure.json in place and never
// inherits an earlier success.

import { execFileSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

export function newRunId() {
  const now = new Date().toISOString().replace(/[-:]/g, "").replace(/\..+/, "Z");
  return `${now}-${crypto.randomUUID().slice(0, 8)}`;
}

const sha256File = (file) => {
  try {
    return crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
  } catch {
    return null;
  }
};

function git(root, args) {
  try {
    return execFileSync("git", ["-C", root, ...args], { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  } catch {
    return "";
  }
}

/// The cubin architectures actually present in a CUDA library. Reading the
/// build cache is not enough - a runtime directory can be assembled from more
/// than one build, which is how an sm_75 test binary once ran against sm_86
/// libraries and produced three segfaults that looked like a code defect.
export function cudaArchitectures(dll) {
  if (!fs.existsSync(dll)) return null;
  const text = fs.readFileSync(dll).toString("latin1");
  const found = new Map();
  for (const match of text.matchAll(/sm_(\d{2})/g)) {
    found.set(`sm_${match[1]}`, (found.get(`sm_${match[1]}`) ?? 0) + 1);
  }
  return Object.fromEntries([...found.entries()].sort());
}

/// Everything needed to say what produced a run, gathered before it starts.
/// The compat pin the checkout currently names.
///
/// Derived, never written down twice. A hardcoded pin path survives an
/// upstream bump and then quietly records the wrong build: every run between
/// 2026-09-01 and 2026-09-02 has an evidence.json naming 557614e02 and a
/// report.json naming 0eadefebd, from the same run.
export function currentCompatManifest(root) {
  const upstream = path.join(root, "layers", "adapters", "llamacpp", "upstream");
  const head = git(upstream, ["rev-parse", "HEAD"]).trim();
  if (!head) throw new Error("upstream checkout has no HEAD; cannot name the compat pin");
  const manifest = path.join(root, "layers", "adapters", "llamacpp", "staged",
    "compat", head.slice(0, 9), "manifest.json");
  if (!fs.existsSync(manifest)) {
    throw new Error(`no compat manifest for upstream ${head.slice(0, 10)} at ${manifest}`);
  }
  return manifest;
}

/// Refuses a pipeline that is not the build this run expected.
///
/// The drive already proves the four stages agree with each other. That is a
/// different question from whether they are the build the repository says it
/// pinned: four stages of the same wrong build agree perfectly.
export function agreesWithExpected(expected, observed) {
  if (!expected) return { ok: true, reason: "" };
  const wanted = {
    upstream_commit: expected.upstream_commit,
    patch_set: expected.patch_set_sha256,
  };
  if (observed.upstream_commit !== wanted.upstream_commit
    || observed.patch_set !== wanted.patch_set) {
    return {
      ok: false,
      reason: `pipeline reports upstream ${observed.upstream_commit} patch_set ${observed.patch_set};`
        + ` the checkout pins upstream ${wanted.upstream_commit} patch_set ${wanted.patch_set}`,
    };
  }
  return { ok: true, reason: "" };
}

export function collectEvidence({ root, runId, spec, compatManifest, remote }) {
  const dirty = git(root, ["diff", "HEAD"]);
  const staged = path.join(root, "target", "p4-staged-cuda");
  const compat = compatManifest && fs.existsSync(compatManifest)
    ? JSON.parse(fs.readFileSync(compatManifest, "utf8"))
    : null;
  return {
    run_id: runId,
    started_at: new Date().toISOString(),
    scenario: spec.name,
    target: spec.target,
    repo_commit: git(root, ["rev-parse", "HEAD"]).trim() || null,
    // A dirty tree is not disqualifying, but a report has to say which dirty
    // tree, or it cannot be reproduced.
    dirty_diff_sha256: dirty ? crypto.createHash("sha256").update(dirty).digest("hex") : null,
    compat: compat && {
      upstream_commit: compat.upstream_commit,
      patch_set_sha256: compat.patch_set_sha256,
      patched_tree: compat.patched_tree,
      patch_count: compat.patches?.length ?? null,
    },
    binaries: {
      agent_sha256: sha256File(path.join(root, "target", "release", "p4-agent.exe")),
      drive_sha256: sha256File(path.join(root, "target", "release", "p4-event-drive.exe")),
      server_sha256: sha256File(path.join(staged, "p4_staged_server.exe")),
      ggml_cuda_sha256: sha256File(path.join(staged, "ggml-cuda.dll")),
      cuda_cubins: cudaArchitectures(path.join(staged, "ggml-cuda.dll")),
    },
    model: { path: spec.model, binary: spec.binary },
    placement: { cuts: spec.cuts, devices: spec.devices, parallel: spec.parallel },
    remote: remote ?? null,
  };
}

/// Opens a run directory. Refuses to reuse one, so a report can never be a
/// mixture of two attempts.
export function beginRun(runsDir, runId) {
  const final = path.join(runsDir, runId);
  const working = `${final}.tmp`;
  if (fs.existsSync(final)) throw new Error(`run ${runId} already exists`);
  fs.rmSync(working, { recursive: true, force: true });
  fs.mkdirSync(working, { recursive: true });
  return { working, final };
}

/// Where runs live under a repository root.
export function defaultRunsDir(root) {
  return path.join(root, "target", "p4-4node", "runs");
}

/// Checksums every file and promotes the directory atomically.
export function promoteRun({ working, final }) {
  const files = fs.readdirSync(working).filter((n) => n !== "MANIFEST.sha256").sort();
  const lines = files.map((name) => `${sha256File(path.join(working, name))}  ${name}`);
  fs.writeFileSync(path.join(working, "MANIFEST.sha256"), `${lines.join("\n")}\n`, "utf8");
  fs.renameSync(working, final);
  return { directory: final, files: files.length };
}
