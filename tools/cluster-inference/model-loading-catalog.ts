import fs from "node:fs";
import crypto from "node:crypto";
import { catalogGgufModels } from "./model-loading-policy.ts";
import { readGgufHeader } from "../../test/benchmarks/model-catalog/gguf-header.mjs";
import type { ModelLoadingDefinition } from "./model-loading-planner.ts";

/** Storage geometry only: GGUF extents are not native PLAN allocations or legal-cut evidence. */
export function readLoadingCatalog(root: string) {
  return catalogGgufModels(root).map((entry) => {
    const files = entry.files.map((file) => ({ path: file, size: fs.statSync(file).size, mtimeMs: fs.statSync(file).mtimeMs }));
    try {
      if (!entry.complete) throw new Error("artifact_incomplete");
      const weights: number[] = [];
      let fixed = 0;
      let architecture = "";
      const headerHashes: string[] = [];
      for (const file of files) {
        const header = readGgufHeader(file.path);
        const declaredArchitecture = header.kv["general.architecture"];
        if (declaredArchitecture !== undefined) {
          if (architecture && architecture !== declaredArchitecture) throw new Error("inconsistent shard architecture");
          architecture = declaredArchitecture;
        }
        const fd = fs.openSync(file.path, "r");
        const prefix = Buffer.alloc(header.dataStart);
        try {
          if (fs.readSync(fd, prefix, 0, prefix.length, 0) !== prefix.length) throw new Error("short header read");
        } finally { fs.closeSync(fd); }
        headerHashes.push(crypto.createHash("sha256").update(prefix).digest("hex"));
        fixed += header.dataStart;
        const tensors = [...header.tensors].sort((a, b) => a.dataOffset - b.dataOffset);
        if (tensors.length) fixed += tensors[0].dataOffset;
        for (let i = 0; i < tensors.length; i++) {
          const tensor = tensors[i];
          const end = i + 1 < tensors.length ? tensors[i + 1].dataOffset : file.size - header.dataStart;
          const bytes = end - tensor.dataOffset;
          if (!Number.isSafeInteger(bytes) || bytes < 0 || (tensor.bytes !== null && bytes < tensor.bytes)) throw new Error(`invalid tensor extent: ${tensor.name}`);
          const layer = /^blk\.(\d+)\./.exec(tensor.name);
          if (layer) weights[Number(layer[1])] = (weights[Number(layer[1])] ?? 0) + bytes;
          else fixed += bytes;
        }
        const after = fs.statSync(file.path);
        if (after.size !== file.size || after.mtimeMs !== file.mtimeMs) throw new Error("artifact changed during scan");
      }
      if (!architecture) throw new Error("missing model architecture");
      if (!weights.length || Array.from(weights).some((value) => value === undefined)) throw new Error("missing contiguous block tensor geometry");
      if (fixed + weights.reduce((a, b) => a + b, 0) !== entry.weightBytes) throw new Error("file extent accounting mismatch");
      const fingerprint = crypto.createHash("sha256").update(JSON.stringify({ files, headerHashes })).digest("hex");
      const model: ModelLoadingDefinition = { id: entry.modelId, fingerprint, architecture, fixedBytesPerStage: fixed,
        layers: weights.map((weightBytes, index) => ({ index, weightBytes, runtimeBytes: 0, kvBytesPerTokenPerSequence: 0 })) };
      return { modelId: entry.modelId, fileBytes: entry.weightBytes, files, headerHashes, status: "storage_profile" as const, model,
        evidence: "header hashes and stat identity; file extents incl. padding; non-block/header bytes repeated per stage; all stored blocks incl. auxiliary blocks; no payload hash/native PLAN/cut approval" };
    } catch (error) {
      return { modelId: entry.modelId, fileBytes: entry.weightBytes, files, status: "profile_unavailable" as const,
        error: error instanceof Error ? error.message : String(error) };
    }
  });
}
