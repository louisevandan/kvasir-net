import fs from "node:fs";
import path from "node:path";

const architectureToken = /\bLLM_ARCH_[A-Z0-9_]+\b/u;
const modelImplementationCast = /\b(?:static|dynamic)_cast\s*<[^>]*\bllama_model_[a-z0-9_]+\b/iu;
const privateHeader = /#include\s*[<"](?:models\/|llama-(?:context|graph|hparams|impl|memory|model)(?:-[^>"/]*)?\.(?:h|hpp))[>"]/iu;

function escaped(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
}

export function officialModelIdentifiers(modelsRoot) {
  if (!fs.existsSync(modelsRoot)) return [];
  return fs.readdirSync(modelsRoot, { withFileTypes: true })
    .filter((entry) => entry.isFile() && /\.cpp$/iu.test(entry.name))
    .map((entry) => path.basename(entry.name, path.extname(entry.name)).toLowerCase())
    // Very short names are ordinary vocabulary and are not a safe textual
    // boundary. Architecture enums and implementation casts remain covered
    // independently above.
    .filter((name) => name.length >= 5 && name !== "llama")
    .sort((left, right) => right.length - left.length);
}

function addedLines(patch) {
  return patch.split(/\r?\n/u)
    .filter((line) => line.startsWith("+") && !line.startsWith("+++"))
    .map((line) => line.slice(1));
}

function isOfficialUpstreamPort(provenance) {
  return provenance?.repository === "https://github.com/ggml-org/llama.cpp.git"
    && /^https:\/\/github\.com\/ggml-org\/llama\.cpp\/pull\/[1-9][0-9]*$/u.test(provenance.pull_request)
    && /^[0-9a-f]{40}$/u.test(provenance.commit);
}

export function validateCompatibilityPatch(file, patch, provenance = null) {
  if (/^diff --git a\/src\/models\//mu.test(patch) && !isOfficialUpstreamPort(provenance)) {
    throw new Error(`${file} changes a model implementation; update official llama.cpp instead`);
  }
  for (const line of addedLines(patch)) {
    if (architectureToken.test(line) || modelImplementationCast.test(line)) {
      throw new Error(`${file} adds model or architecture knowledge: ${line.trim()}`);
    }
  }
}

export function validateRuntimeSource(file, source, modelIdentifiers = []) {
  for (const line of source.split(/\r?\n/u)) {
    if (architectureToken.test(line) || modelImplementationCast.test(line)) {
      throw new Error(`${file} contains model or architecture knowledge: ${line.trim()}`);
    }
    if (privateHeader.test(line)) {
      throw new Error(`${file} includes a private llama.cpp header: ${line.trim()}`);
    }
    for (const identifier of modelIdentifiers) {
      const modelName = new RegExp(`\\b${escaped(identifier)}\\b`, "iu");
      if (modelName.test(line)) {
        throw new Error(
          `${file} names an official model implementation instead of a generic tensor contract: ${line.trim()}`,
        );
      }
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

export function validateRuntimeTree(root, modelIdentifiers = []) {
  for (const file of sourceFiles(root)) {
    validateRuntimeSource(
      path.relative(root, file),
      fs.readFileSync(file, "utf8"),
      modelIdentifiers,
    );
  }
}
