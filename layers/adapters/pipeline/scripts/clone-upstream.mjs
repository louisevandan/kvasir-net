#!/usr/bin/env node

// Puts llama.cpp where the pipeline adapter expects it, by cloning it from
// upstream rather than carrying it in our history.
//
// It used to be a submodule, which meant our repository pinned a commit of
// somebody else's project and every clone of ours dragged 165MB of it. The
// folder is unchanged — `upstream/` beside `compat/`, where the prepare script
// already looks — but the repository in it is llama.cpp's, ignored by ours.
//
// What that buys is the thing that matters when upstream moves: taking a newer
// llama.cpp is a fetch and a checkout here, not a commit to us. The HTTP
// adapter (`layers/adapters/llamacpp`) needs nothing from this folder at all;
// only the staged pipeline adapter, which patches llama.cpp internals, does.
//
//   node clone-upstream.mjs            # clone if missing, otherwise report
//   node clone-upstream.mjs <ref>      # and check that ref out
//   node clone-upstream.mjs --update   # fetch first, then check the ref out

import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const REMOTE = "https://github.com/ggml-org/llama.cpp.git";
const adapterRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);
const upstreamDir = path.join(adapterRoot, "upstream");

function git(args, cwd = upstreamDir) {
  const result = spawnSync("git", args, { cwd, encoding: "utf8" });
  if (result.status !== 0) {
    throw new Error(`git ${args.join(" ")} failed: ${result.stderr.trim()}`);
  }
  return result.stdout.trim();
}

function isClone() {
  return fs.existsSync(path.join(upstreamDir, ".git"));
}

function main() {
  const args = process.argv.slice(2);
  const update = args.includes("--update");
  const ref = args.find((argument) => !argument.startsWith("--"));

  if (!isClone()) {
    console.log(`cloning ${REMOTE}`);
    // Full history: the patch series in compat/ is written against particular
    // commits, and rebasing it onto a newer one needs their ancestry.
    const result = spawnSync("git", ["clone", REMOTE, upstreamDir], {
      stdio: "inherit",
    });
    if (result.status !== 0) {
      throw new Error("clone failed");
    }
  } else if (update) {
    console.log("fetching");
    git(["fetch", "--tags", "origin"]);
  }

  if (ref) {
    // Refused rather than discarded: a dirty tree here is somebody's
    // in-progress rebase of the patch series, which is expensive to redo.
    const dirty = git(["status", "--porcelain"]);
    if (dirty) {
      throw new Error(
        `upstream/ has uncommitted changes; not checking out ${ref}:\n${dirty}`,
      );
    }
    git(["checkout", "--detach", ref]);
  }

  console.log(`P4_UPSTREAM ${git(["rev-parse", "--short", "HEAD"])} ${upstreamDir}`);
  console.log(git(["log", "-1", "--format=%s"]));
}

try {
  main();
} catch (error) {
  console.error(String(error.message ?? error));
  process.exit(1);
}
