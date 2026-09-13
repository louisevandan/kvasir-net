import fs from "node:fs";
import path from "node:path";
import { planPlacement, type PlacementRequest } from "./placement-policy.ts";

function argument(name: string): string | null {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] ?? null : null;
}

const input = argument("--input");
const output = argument("--out");
if (!input || !output) throw new Error("usage: node plan-placement.ts --input <request.json> --out <plan.json>");
const request = JSON.parse(fs.readFileSync(input, "utf8")) as PlacementRequest;
const plan = planPlacement(request);
fs.mkdirSync(path.dirname(output), { recursive: true });
fs.writeFileSync(output, `${JSON.stringify(plan, null, 2)}\n`);
process.stdout.write(`${JSON.stringify({ output, maxTier: plan.maxTier, stages: plan.stages.length,
  predictedPipelinePeriodMs: plan.predictedPipelinePeriodMs })}\n`);
