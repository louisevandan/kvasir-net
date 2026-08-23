import fs from "node:fs";
import path from "node:path";

const architectureToken = /\bLLM_ARCH_[A-Z0-9_]+\b/u;
const modelImplementationCast = /\b(?:static|dynamic)_cast\s*<[^>]*\bllama_model_[a-z0-9_]+\b/iu;
const privateHeader = /#include\s*[<"](?:models\/|llama-(?:context|graph|hparams|impl|memory|model)(?:-[^>"/]*)?\.(?:h|hpp))[>"]/iu;

function addedLines(patch) {
  return patch.split(/\r?\n/u)
    .filter((line) => line.startsWith("+") && !line.startsWith("+++"))
    .map((line) => line.slice(1));
}

export function validateCompatibilityPatch(file, patch) {
  if (/^diff --git a\/src\/models\//mu.test(patch)) {
    throw new Error(`${file} changes a model implementation; update official llama.cpp instead`);
  }
  for (const line of addedLines(patch)) {
    if (architectureToken.test(line) || modelImplementationCast.test(line)) {
      throw new Error(`${file} adds model or architecture knowledge: ${line.trim()}`);
    }
  }
}

export function validateRuntimeSource(file, source) {
  for (const line of source.split(/\r?\n/u)) {
    if (architectureToken.test(line) || modelImplementationCast.test(line)) {
      throw new Error(`${file} contains model or architecture knowledge: ${line.trim()}`);
    }
    if (privateHeader.test(line)) {
      throw new Error(`${file} includes a private llama.cpp header: ${line.trim()}`);
    }
  }
}

function sourceFiles(root) {
  if (!fs.existsSync(root)) return [];
  const files = [];
  for (const entry of fs.readdirSync(root, { withFileTypes: true })) {
    const absolute = path.join(root, entry.name);
    if (entry.isDirectory()) files.push(...sourceFiles(absolute));
    if (entry.isFile() && /\.(?:c|cc|cpp|cxx|h|hh|hpp)$/iu.test(entry.name)) files.push(absolute);
  }
  return files;
}

export function validateRuntimeTree(root) {
  for (const file of sourceFiles(root)) {
    validateRuntimeSource(path.relative(root, file), fs.readFileSync(file, "utf8"));
  }
}
