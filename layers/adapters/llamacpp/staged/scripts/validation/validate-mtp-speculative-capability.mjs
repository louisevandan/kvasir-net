#!/usr/bin/env node

// Validates the distinction between common-parser acceptance and staged
// execution. It invokes only --validate-plan, so no model is loaded.
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

function arg(name, fallback) {
  const index = process.argv.indexOf(name);
  return index >= 0 && process.argv[index + 1] ? process.argv[index + 1] : fallback;
}

const executable = path.resolve(arg("--executable", ".cache/staged-server-llama/Release/p4_staged_server.exe"));
if (!fs.existsSync(executable)) throw new Error(`stage server executable not found: ${executable}`);
async function run(validateOnly) {
  const plan = Buffer.from(
    `${validateOnly ? "--validate-plan " : ""}--model ${JSON.stringify(arg("--model", "not-loaded.gguf"))} --spec-type draft-mtp`,
    "utf8",
  );
  const prefix = Buffer.alloc(4);
  prefix.writeUInt32LE(plan.length);
  // The server defaults to 127.0.0.1; keeping this smoke to --port avoids
  // coupling the parser-only check to socket binding configuration.
  const child = spawn(executable, ["--port", "1"], {
    stdio: ["pipe", "ignore", "pipe"], windowsHide: true,
    env: {
      ...process.env,
      PATH: `${path.dirname(executable)}${path.delimiter}${process.env.PATH ?? ""}`,
    },
  });
  let stderr = "";
  child.stderr.on("data", chunk => { stderr += chunk.toString(); });
  child.stdin.end(Buffer.concat([prefix, plan]));
  const result = await new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("exit", (code, signal) => resolve({ code, signal }));
  });
  const report = stderr.split(/\r?\n/u).find(line => line.startsWith("CAPABILITY_REPORT "));
  return { result, report, stderr };
}

const parserOnly = await run(true);
const executionUnavailable = await run(false);
const validReport = value => value.report &&
  value.report.includes("mtp_parser=1") && value.report.includes("mtp_execution=0") &&
  value.report.includes("speculative_parser=1") && value.report.includes("speculative_execution=0");
if (parserOnly.result.code !== 0 || !validReport(parserOnly) ||
    executionUnavailable.result.code !== 6 || !validReport(executionUnavailable) ||
    !executionUnavailable.stderr.includes("CAPABILITY_UNAVAILABLE")) {
  throw new Error(`capability validation failed: ${JSON.stringify({ parserOnly, executionUnavailable })}`);
}
console.log(JSON.stringify({
  status: "passed",
  parser_only: parserOnly.report,
  execution_unavailable: executionUnavailable.report,
}, null, 2));
