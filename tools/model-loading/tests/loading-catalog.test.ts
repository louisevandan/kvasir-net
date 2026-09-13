import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { readLoadingCatalog } from "../src/model-loading-catalog.ts";

function writeShard(file: string, architecture: string | null, names: string[]) {
  const u32 = (n: number) => { const b = Buffer.alloc(4); b.writeUInt32LE(n); return b; };
  const u64 = (n: number) => { const b = Buffer.alloc(8); b.writeBigUInt64LE(BigInt(n)); return b; };
  const str = (s: string) => Buffer.concat([u64(Buffer.byteLength(s)), Buffer.from(s)]);
  const metadata = architecture ? [str("general.architecture"), u32(8), str(architecture)] : [];
  const table = names.flatMap((name, i) => [str(name), u32(1), u64(2), u32(0), u64(i * 32)]);
  const header = Buffer.concat([Buffer.from("GGUF"), u32(3), u64(names.length), u64(architecture ? 1 : 0), ...metadata, ...table]);
  const dataStart = Math.ceil(header.length / 32) * 32;
  const data = names.length ? (names.length - 1) * 32 + 8 : 0;
  const bytes = Buffer.alloc(dataStart + data); header.copy(bytes); fs.writeFileSync(file, bytes);
  return { dataStart, size: bytes.length };
}

test("real GGUF parser retains each shard and allows architecture metadata only in first shard", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "p4-loading-catalog-"));
  try {
    const a = writeShard(path.join(dir, "split-00001-of-00002.gguf"), "fixture", []);
    // Metadata-only shards need no tensor-data alignment padding at EOF.
    a.size--; a.dataStart--;
    fs.truncateSync(path.join(dir, "split-00001-of-00002.gguf"), a.size);
    const b = writeShard(path.join(dir, "split-00002-of-00002.gguf"), null, ["token_embd.weight", "blk.0.weight", "blk.1.weight"]);
    writeShard(path.join(dir, "mmproj-F32.gguf"), "projector", ["blk.0.weight"]);
    const [row] = readLoadingCatalog(dir);
    assert.equal(row.status, "storage_profile");
    assert.deepEqual(row.model!.layers.map((l) => l.weightBytes), [32, 8]);
    assert.equal(row.model!.fixedBytesPerStage, a.dataStart + b.dataStart + 32);
    assert.equal(row.fileBytes, a.size + b.size);
    const inventoryFile = path.join(dir, "inventory.json");
    const inventoryCli = path.resolve(import.meta.dirname, "../../../test/benchmarks/model-catalog/inventory.mjs");
    const inventoryRun = spawnSync(process.execPath, [inventoryCli, "--root", dir, "--out", inventoryFile], { encoding: "utf8", timeout: 60000 });
    assert.equal(inventoryRun.status, 0, inventoryRun.stderr + inventoryRun.stdout);
    const inventory = JSON.parse(fs.readFileSync(inventoryFile, "utf8"));
    assert.equal(inventory.models.length, 1);
    assert.equal(inventory.models[0].header_error, undefined);
    assert.equal(inventory.auxiliary.length, 1);
    writeShard(path.join(dir, "split-00002-of-00002.gguf"), "conflict", ["blk.0.weight"]);
    assert.equal(readLoadingCatalog(dir)[0].status, "profile_unavailable");
  } finally { fs.rmSync(dir, { recursive: true, force: true }); }
});

test("evaluation CLI reads a real catalog, freezes reference data, scores policy and rejects tampering", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "p4-loading-cli-"));
  try {
    const models = path.join(dir, "models"), output = path.join(dir, "study"); fs.mkdirSync(models);
    writeShard(path.join(models, "tiny.gguf"), "fixture", ["blk.0.weight", "blk.1.weight"]);
    const cli = path.resolve(import.meta.dirname, "../validation/evaluate-loading.ts");
    const run = (...args: string[]) => spawnSync(process.execPath, [cli, ...args, "--out", output], { encoding: "utf8", timeout: 60000 });
    for (const args of [["prepare", "--model-root", models], ["reference"], ["compare"]]) {
      const result = run(...args); assert.equal(result.status, 0, result.stderr + result.stdout);
    }
    const summary = JSON.parse(fs.readFileSync(path.join(output, "candidate-summary.json"), "utf8"));
    assert.equal(summary.cases, 169);
    assert.equal(summary.agreements, 169);
    assert.deepEqual(Object.keys(summary.policySources), [
      "tools/model-loading/src/model-loading-planner.ts", "tools/model-loading/src/placement-policy.ts", "tools/model-loading/src/model-loading-policy.ts",
    ]);
    const explicit = run("compare", "--label", "explicit", "--policy-root", path.resolve(import.meta.dirname, "../../.."), "--policy-dir", "tools/model-loading/src");
    assert.equal(explicit.status, 0, explicit.stderr + explicit.stdout);
    assert.equal(JSON.parse(fs.readFileSync(path.join(output, "explicit-summary.json"), "utf8")).agreements, 169);
    fs.appendFileSync(path.join(output, "reference-data.jsonl"), "{}\n");
    const refused = run("compare", "--label", "tampered");
    assert.notEqual(refused.status, 0); assert.match(refused.stderr, /reference seal mismatch/);
    assert.equal(fs.existsSync(path.join(output, "tampered-summary.json")), false);
  } finally { fs.rmSync(dir, { recursive: true, force: true }); }
});
