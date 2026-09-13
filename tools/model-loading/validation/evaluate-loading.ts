import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
import { pathToFileURL } from "node:url";
import { readLoadingCatalog } from "../src/model-loading-catalog.ts";
import { loadingScenarios, STUDY_WORKLOADS, scenarioInput } from "./loading-scenarios.ts";
import { referencePlan, comparePlan } from "./loading-reference.ts";
import { analystJudgments } from "./loading-judgments.ts";

const hash = (data: unknown) => crypto.createHash("sha256").update(JSON.stringify(data)).digest("hex");
const fileHash = (file: string) => crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
const option = (name: string, fallback?: string) => {
  const index = process.argv.indexOf(name);
  if (index !== -1 && process.argv[index + 1]) return process.argv[index + 1];
  if (fallback !== undefined) return fallback;
  throw new Error(`missing ${name}`);
};
const mode = process.argv[2];
const out = path.resolve(option("--out"));
const inputFile = path.join(out, "study.json");
const referenceFile = path.join(out, "reference-data.jsonl");
const save = (file: string, value: unknown) => fs.writeFileSync(file, JSON.stringify(value, null, 2) + "\n", { flag: "wx" });
const sourceRoot = path.resolve(import.meta.dirname, "../../..");
const sourceHashes = (root: string, names: string[]) => Object.fromEntries(names.map((name) => [name, fileHash(path.join(root, name))]));
const referenceSources = ["tools/model-loading/validation/loading-reference.ts", "tools/model-loading/validation/loading-judgments.ts", "tools/model-loading/validation/loading-scenarios.ts"];

if (mode === "prepare") {
  fs.mkdirSync(out, { recursive: true });
  if (fs.existsSync(inputFile)) throw new Error("study already exists; choose a new output directory");
  const catalog = readLoadingCatalog(path.resolve(option("--model-root")));
  const study = { schema: "p4-loading-study-v1", createdAt: new Date().toISOString(),
    evidenceClass: "AI-authored objective and literal judgments; independent exact solver expands references. Not individual LLM calls per row; not measured performance or native load approval.",
    objective: ["minimum tier prefix", "minimum maximum stage service", "minimum total stage service", "minimum stage count"],
    limits: ["device order fixed by input", "one contiguous nonempty range per selected device", "all cuts assumed legal unless supplied",
      "GGUF storage extents include padding; non-block bytes repeat per stage", "KV/runtime and all timing profiles are synthetic study assumptions",
      "DDR is a whole-layer CPU stage, not GPU expert offload", "per-device hop constants are not a topology-dependent link model",
      "model/backend capability, shared GPU execution contention, native PLAN, response quality and TPS are unverified"],
    referenceSources: sourceHashes(sourceRoot, referenceSources),
    catalog, scenarios: loadingScenarios(), workloads: STUDY_WORKLOADS, judgments: analystJudgments() };
  save(inputFile, study);
  console.log(JSON.stringify({ models: catalog.length, unavailable: catalog.filter((m) => m.status !== "storage_profile"), scenarios: study.scenarios.length,
    combinations: catalog.length * study.scenarios.length * study.workloads.length, studySha256: fileHash(inputFile) }));
} else {
  const study = JSON.parse(fs.readFileSync(inputFile, "utf8"));
  if (hash(study.referenceSources) !== hash(sourceHashes(sourceRoot, referenceSources))) throw new Error("reference source changed since study freeze");
  const rows: Array<{ id: string; modelId: string; scenarioId: string; workloadId: string; input: any }> = [];
  for (const model of study.catalog) for (const scenario of study.scenarios) for (const workload of study.workloads) {
    rows.push({ id: `${model.modelId}|${scenario.id}|${workload.id}`, modelId: model.modelId, scenarioId: scenario.id, workloadId: workload.id,
      input: model.model ? scenarioInput(model.model, scenario, workload) : null });
  }
  for (const judgment of study.judgments) rows.push({ id: `analyst:${judgment.id}`, modelId: "analyst-literals", scenarioId: judgment.id, workloadId: "literal", input: judgment.input });
  if (mode === "reference") {
    const fd = fs.openSync(referenceFile, "wx");
    try {
      for (const [i, row] of rows.entries()) {
        const started = performance.now();
        const plan = row.input ? referencePlan(row.input) : null;
        fs.writeSync(fd, JSON.stringify({ id: row.id, inputSha256: hash(row.input), status: !row.input ? "profile_unavailable" : plan ? "feasible" : "infeasible",
          plan, elapsedMs: performance.now() - started }) + "\n");
        if (i % 159 === 0) console.log(`reference ${i + 1}/${rows.length}: ${row.modelId}`);
      }
    } finally { fs.closeSync(fd); }
    save(path.join(out, "reference-seal.json"), { studySha256: fileHash(inputFile), referenceSha256: fileHash(referenceFile), rows: rows.length,
      sources: study.referenceSources, authoring: study.evidenceClass });
  } else if (mode === "compare") {
    const seal = JSON.parse(fs.readFileSync(path.join(out, "reference-seal.json"), "utf8"));
    if (seal.studySha256 !== fileHash(inputFile) || seal.referenceSha256 !== fileHash(referenceFile)) throw new Error("reference seal mismatch");
    const refs = fs.readFileSync(referenceFile, "utf8").trim().split("\n").map((line) => JSON.parse(line));
    if (refs.length !== rows.length || seal.rows !== rows.length) throw new Error("reference coverage mismatch");
    const policyRoot = path.resolve(option("--policy-root", sourceRoot));
    const policyDirectory = option("--policy-dir", "tools/model-loading/src");
    const policySources = ["model-loading-planner.ts", "placement-policy.ts", "model-loading-policy.ts"].map((name) => path.join(policyDirectory, name).replaceAll("\\", "/"));
    const { planModelLoading } = await import(pathToFileURL(path.join(policyRoot, policySources[0])).href);
    const label = option("--label", "candidate");
    if (!/^[a-z0-9-]+$/.test(label)) throw new Error("invalid label");
    const outputFile = path.join(out, `${label}-policy-data.jsonl`);
    const fd = fs.openSync(outputFile, "wx");
    const categories: Record<string, number> = {};
    const groups: Record<string, { cases: number; agreements: number }> = {};
    const mismatches: unknown[] = [];
    let planTime = 0;
    try {
      for (let i = 0; i < rows.length; i++) {
        const row = rows[i], ref = refs[i];
        if (row.id !== ref.id || hash(row.input) !== ref.inputSha256) throw new Error(`reference input mismatch: ${row.id}`);
        let actual = null, error: string | null = null, result;
        const started = performance.now();
        if (row.input) try { actual = planModelLoading(row.input); } catch (caught) { error = String(caught); }
        const elapsedMs = performance.now() - started;
        planTime += elapsedMs;
        if (!row.input) result = { agrees: false, category: "profile_unavailable", periodRegret: null };
        else if (error && !/no (?:memory-)?tier prefix/.test(error)) result = { agrees: false, category: "policy_error", periodRegret: null };
        else result = comparePlan(row.input, ref.plan, actual);
        categories[result.category] = (categories[result.category] ?? 0) + 1;
        for (const key of [`model:${row.modelId}`, `scenario:${row.scenarioId}`, `workload:${row.workloadId}`]) {
          const group = groups[key] ??= { cases: 0, agreements: 0 }; group.cases++; if (result.agrees) group.agreements++;
        }
        if (!result.agrees) mismatches.push({ id: row.id, ...result, error });
        fs.writeSync(fd, JSON.stringify({ id: row.id, inputSha256: ref.inputSha256, ...result, error, elapsedMs, plan: actual }) + "\n");
        if (i % 159 === 0) console.log(`${label} ${i + 1}/${rows.length}: ${row.modelId}`);
      }
    } finally { fs.closeSync(fd); }
    const summary = { schema: "p4-loading-score-v1", label, cases: rows.length, categories,
      agreements: (categories.optimal ?? 0) + (categories.both_infeasible ?? 0),
      feasibleQualityDenominator: refs.filter((r) => r.status === "feasible").length,
      exactObjectiveMatches: categories.optimal ?? 0, planningTimeMs: planTime,
      studySha256: seal.studySha256, referenceSha256: seal.referenceSha256, policyDataSha256: fileHash(outputFile),
      policySources: sourceHashes(policyRoot, policySources),
      groups, mismatches };
    save(path.join(out, `${label}-summary.json`), summary);
    console.log(JSON.stringify({ label, cases: summary.cases, categories, agreements: summary.agreements, exactObjectiveMatches: summary.exactObjectiveMatches,
      feasibleQualityDenominator: summary.feasibleQualityDenominator, planningTimeMs: planTime }));
  } else throw new Error("use prepare, reference or compare");
}
