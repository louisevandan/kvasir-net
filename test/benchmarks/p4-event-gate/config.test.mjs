import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { loadConfig } from "./config.mjs";

test("expands prompt and response oracle files relative to the config", async (context) => {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), "p4-event-gate-config-"));
  context.after(() => fs.rm(directory, { recursive: true, force: true }));
  await fs.writeFile(path.join(directory, "prompts.json"), '["prompt"]');
  await fs.writeFile(path.join(directory, "responses.json"), '[{"required_substrings":["marker"]}]');
  const configPath = path.join(directory, "config.json");
  await fs.writeFile(configPath, JSON.stringify({
    prompts_file: "prompts.json",
    acceptance: { responses_file: "responses.json" },
  }));

  const config = await loadConfig(configPath);
  assert.deepEqual(config.prompts, ["prompt"]);
  assert.deepEqual(config.acceptance.responses, [{ required_substrings: ["marker"] }]);
  assert.equal("prompts_file" in config, false);
  assert.equal("responses_file" in config.acceptance, false);
});

test("rejects inline and file-backed response oracles together", async (context) => {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), "p4-event-gate-config-"));
  context.after(() => fs.rm(directory, { recursive: true, force: true }));
  const configPath = path.join(directory, "config.json");
  await fs.writeFile(configPath, JSON.stringify({
    acceptance: { responses: [], responses_file: "responses.json" },
  }));

  await assert.rejects(loadConfig(configPath), /either responses or responses_file/u);
});

test("rejects non-object response oracle entries", async (context) => {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), "p4-event-gate-config-"));
  context.after(() => fs.rm(directory, { recursive: true, force: true }));
  await fs.writeFile(path.join(directory, "responses.json"), '["not-an-object"]');
  const configPath = path.join(directory, "config.json");
  await fs.writeFile(configPath, JSON.stringify({
    acceptance: { responses_file: "responses.json" },
  }));

  await assert.rejects(loadConfig(configPath), /JSON responses array/u);
});
