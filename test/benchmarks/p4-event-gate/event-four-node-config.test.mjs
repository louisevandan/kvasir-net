import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { buildEventFourNodeConfig } from "../../../apps/p4/tools/scripts/e2e/event-four-node-config.mjs";

async function fixture() {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), "p4-event-config-"));
  const promptsFile = path.join(directory, "prompts.json");
  const responsesFile = path.join(directory, "responses.json");
  const placementFile = path.join(directory, "placement.json");
  const optionsFile = path.join(directory, "options.json");
  await fs.writeFile(promptsFile, JSON.stringify(["one", "two"]));
  await fs.writeFile(responsesFile, JSON.stringify([
    { required_substrings: ["one"] },
    { required_substrings: ["two"] },
  ]));
  await fs.writeFile(placementFile, JSON.stringify({
    plan: {
      feasible: true,
      n_ctx: 12_000,
      n_parallel: 10,
      placement: [
        { stage_index: 0, node: 0, layers: [0, 3], n_layers: 3, ot: "" },
        { stage_index: 1, node: 1, layers: [3, 22], n_layers: 19, ot: "blk\\.3\\.expert=CPU" },
        { stage_index: 2, node: 2, layers: [22, 41], n_layers: 19, ot: "" },
        { stage_index: 3, node: 3, layers: [41, 60], n_layers: 19, ot: "" },
      ],
    },
  }));
  await fs.writeFile(optionsFile, JSON.stringify({ temperature: 0, seed: 7 }));
  const spec = {
    ingress_agent: "tcp://127.0.0.1:52003",
    channel: "gate",
    session_id: "session",
    request_id: "request",
    request_count: 2,
    parallel: 10,
    context_size: 1200,
    n_batch: 512,
    n_ubatch: 512,
    max_tokens: 500,
    prompts_file: promptsFile,
    responses_file: responsesFile,
    placement_file: placementFile,
    options_file: optionsFile,
    waves: [{ after_ms: 0, count: 1 }, { after_ms: 1000, count: 1 }],
    stages: [0, 1, 2, 3].map((index) => ({
      agent: `tcp://127.0.0.1:${index < 2 ? 52003 : 53001}`,
      node: `node-${index}`,
      binary: `C:\\runtime-${index}\\p4_staged_server.exe`,
      endpoint: `127.0.0.1:${52103 + index}`,
      model: `S:\\models\\model-${index}.gguf`,
      cuda_visible_devices: `${index % 2}`,
    })),
  };
  return { directory, placementFile, spec };
}

test("builds the exact four-node Event and llama capacity contract", async (context) => {
  const { directory, spec } = await fixture();
  context.after(() => fs.rm(directory, { recursive: true, force: true }));
  const config = await buildEventFourNodeConfig(spec);

  assert.equal(config.nodes.length, 4);
  assert.equal(new Set(config.nodes.map((node) => node.agent)).size, 2);
  assert.deepEqual(config.nodes.map((node) => node.agent), [
    "tcp://127.0.0.1:52003",
    "tcp://127.0.0.1:52003",
    "tcp://127.0.0.1:53001",
    "tcp://127.0.0.1:53001",
  ]);
  assert.equal(config.nodes[0].total_context_size, 12_000);
  assert.equal(config.nodes[0].sequence_capacity, 10);
  assert.equal(config.nodes[0].plan.includes("--n-gpu-layers 60"), true);
  assert.equal(config.nodes[1].plan.includes("--n-gpu-layers 57"), true);
  assert.equal(config.nodes[1].plan.includes("blk\\.3\\.expert=CPU"), true);
  assert.equal(config.nodes[1].plan.includes("blk\\.(0|1|2|22|23"), true);
  assert.equal(config.nodes[3].plan.includes("--layer-begin 41 --layer-end 60"), true);
  assert.equal(config.nodes.every((node) => node.plan.includes("--memory-topology discrete")), true);
  assert.equal(config.nodes.every((node) => node.plan.includes("--kv-unified")), true);
  assert.equal(config.nodes.every((node) => !node.plan.includes("--no-kv-unified")), true);
  assert.equal(config.nodes.every((node) => node.plan.includes("--spec-type none")), true);
  assert.equal(config.nodes[0].plan.includes("--flash-attn off"), true);
  assert.equal(config.nodes[0].plan.includes("--cache-type-v f16"), true);
  assert.equal(config.options, '{"temperature":0,"seed":7}');
  assert.deepEqual(config.prompts, ["one", "two"]);
  assert.equal(config.acceptance.responses.length, 2);
  assert.deepEqual(config.acceptance.allowed_stop_reasons, ["eos", "length"]);
});

test("passes the Outer-selected stock MTP mode through every adapter plan", async (context) => {
  const { directory, spec } = await fixture();
  context.after(() => fs.rm(directory, { recursive: true, force: true }));
  spec.speculative_type = "draft-mtp";
  const config = await buildEventFourNodeConfig(spec);
  assert.equal(config.nodes.every((node) => node.plan.includes("--spec-type draft-mtp")), true);
});

test("rejects speculative modes not implemented by the staged adapter", async (context) => {
  const { directory, spec } = await fixture();
  context.after(() => fs.rm(directory, { recursive: true, force: true }));
  spec.speculative_type = "draft-simple";
  await assert.rejects(buildEventFourNodeConfig(spec), /none or draft-mtp/);
});

test("pairs a quantized V cache only with explicit flash attention", async (context) => {
  const { directory, spec } = await fixture();
  context.after(() => fs.rm(directory, { recursive: true, force: true }));
  spec.flash_attention = true;
  const config = await buildEventFourNodeConfig(spec);
  assert.equal(config.nodes[0].plan.includes("--flash-attn on"), true);
  assert.equal(config.nodes[0].plan.includes("--cache-type-v q8_0"), true);
});

test("rejects the stale bare-address agent launch contract", async (context) => {
  const { directory, spec } = await fixture();
  context.after(() => fs.rm(directory, { recursive: true, force: true }));
  spec.stages[2].agent = "127.0.0.1:53001";
  await assert.rejects(buildEventFourNodeConfig(spec), /tcp:\/\/HOST:PORT/);
});

test("rejects a placement gap before it reaches llama.cpp", async (context) => {
  const { directory, placementFile, spec } = await fixture();
  context.after(() => fs.rm(directory, { recursive: true, force: true }));
  const value = JSON.parse(await fs.readFile(placementFile, "utf8"));
  value.plan.placement[2].layers = [23, 41];
  value.plan.placement[2].n_layers = 18;
  await fs.writeFile(placementFile, JSON.stringify(value));
  await assert.rejects(buildEventFourNodeConfig(spec), /contiguous from zero/);
});

test("rejects a placement planned for a different context or parallel width", async (context) => {
  const { directory, placementFile, spec } = await fixture();
  context.after(() => fs.rm(directory, { recursive: true, force: true }));
  const value = JSON.parse(await fs.readFile(placementFile, "utf8"));
  value.plan.n_parallel = 1;
  await fs.writeFile(placementFile, JSON.stringify(value));
  await assert.rejects(buildEventFourNodeConfig(spec), /placement capacity mismatch/);
});

test("preserves the planner-owned physical node order", async (context) => {
  const { directory, placementFile, spec } = await fixture();
  context.after(() => fs.rm(directory, { recursive: true, force: true }));
  const value = JSON.parse(await fs.readFile(placementFile, "utf8"));
  value.plan.placement.forEach((entry, index) => {
    entry.node = [2, 3, 1, 0][index];
  });
  value.plan.stage_node_indexes = [2, 3, 1, 0];
  await fs.writeFile(placementFile, JSON.stringify(value));
  const config = await buildEventFourNodeConfig(spec);
  assert.deepEqual(config.nodes.map((node) => node.node), ["node-2", "node-3", "node-1", "node-0"]);
  assert.equal(config.nodes[0].plan.includes("--layer-begin 0 --layer-end 3"), true);
  assert.equal(config.nodes[3].plan.includes("--layer-begin 41 --layer-end 60"), true);
});

test("rejects duplicate physical node ownership", async (context) => {
  const { directory, placementFile, spec } = await fixture();
  context.after(() => fs.rm(directory, { recursive: true, force: true }));
  const value = JSON.parse(await fs.readFile(placementFile, "utf8"));
  value.plan.placement[3].node = 2;
  await fs.writeFile(placementFile, JSON.stringify(value));
  await assert.rejects(buildEventFourNodeConfig(spec), /node indexes must be unique/);
});
