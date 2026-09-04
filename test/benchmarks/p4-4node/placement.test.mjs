// What a cut may be, and what it may not.
//
// `cutForLanes` had no tests when it was written and returned `[[0,35],[35,35]]`
// for `cutForLanes(35, 2, [35, 35])` - an empty stage, from the function whose
// purpose is to refuse what a constraint cannot give. These fix the contract.

import assert from "node:assert/strict";
import test from "node:test";

import {
  GEMMA4_LAYERS,
  GEMMA4_SHARED_KV,
  ORNITH35B_LAYERS,
  cutForLanes,
  placeOnLanes,
} from "./scenarios.mjs";
import { buildConfig } from "./spec.mjs";

test("a cut covers every layer once, in order, with no empty stage", () => {
  for (const [layers, lanes, forbidden] of [
    [GEMMA4_LAYERS, 1, GEMMA4_SHARED_KV],
    [GEMMA4_LAYERS, 2, GEMMA4_SHARED_KV],
    [GEMMA4_LAYERS, 4, GEMMA4_SHARED_KV],
    [ORNITH35B_LAYERS, 2, undefined],
    [ORNITH35B_LAYERS, 4, undefined],
    [ORNITH35B_LAYERS, 8, undefined],
  ]) {
    const cut = cutForLanes(layers, lanes, forbidden);
    assert.equal(cut.length, lanes, `${lanes} lanes should give ${lanes} stages`);
    assert.equal(cut[0][0], 0);
    assert.equal(cut.at(-1)[1], layers);
    for (const [begin, end] of cut) assert.ok(end > begin, `stage [${begin},${end}) is empty`);
    for (let index = 1; index < cut.length; index += 1) {
      assert.equal(cut[index][0], cut[index - 1][1], "stages must be contiguous");
    }
  }
});

test("no boundary falls inside a shared region", () => {
  const [from, to] = GEMMA4_SHARED_KV;
  for (const lanes of [1, 2, 3, 4]) {
    for (const [begin] of cutForLanes(GEMMA4_LAYERS, lanes, GEMMA4_SHARED_KV)) {
      assert.ok(begin <= from || begin >= to, `boundary ${begin} splits the shared region`);
    }
  }
});

test("the derived cuts are the ones that were measured by hand", () => {
  assert.deepEqual(cutForLanes(GEMMA4_LAYERS, 2, GEMMA4_SHARED_KV), [[0, 13], [13, 35]]);
  assert.deepEqual(cutForLanes(ORNITH35B_LAYERS, 2), [[0, 20], [20, 40]]);
  assert.deepEqual(cutForLanes(ORNITH35B_LAYERS, 4), [[0, 10], [10, 20], [20, 30], [30, 40]]);
});

test("a constraint with no room refuses rather than emitting an empty stage", () => {
  // Thirteen layers before the shared region cannot become fourteen stages.
  assert.throws(() => cutForLanes(GEMMA4_LAYERS, 15, GEMMA4_SHARED_KV), /cannot cut/);
  assert.throws(() => cutForLanes(4, 5), /between one lane and one lane per layer/);
});

test("a malformed shared region is refused", () => {
  for (const forbidden of [
    [35, 35], // empty - this one returned [[0,35],[35,35]]
    [20, 10], // inverted
    [-1, 35], // before the model
    [10, 99], // past the end
    [1.5, 35], // not layer indices
  ]) {
    assert.throws(
      () => cutForLanes(35, 2, forbidden),
      /shared region|runs to the last layer/,
      `forbidden ${JSON.stringify(forbidden)} should be refused`,
    );
  }
});

test("placeOnLanes gives one stage per named device", () => {
  const placement = placeOnLanes(GEMMA4_LAYERS, GEMMA4_SHARED_KV, ["0", "1"]);
  assert.deepEqual(placement.devices, ["0", "1"]);
  assert.equal(placement.cuts.length, 2);
  const wider = placeOnLanes(ORNITH35B_LAYERS, undefined, ["0", "1", "2", "3"]);
  assert.equal(wider.cuts.length, 4);
  assert.equal(new Set(wider.devices).size, 4);
});

test("two stages on one device is refused unless a scenario asks for it", () => {
  const oversubscribed = {
    name: "oversubscribed",
    cuts: [[0, 10], [10, 20], [20, 30], [30, 40]],
    devices: ["0", "0", "1", "1"],
    model: "model.gguf",
    binary: "server.exe",
    ingress: "tcp://127.0.0.1:42003",
    endpointBase: 42_011,
    nBatch: 512,
    nUbatch: 512,
    context: 512,
    parallel: 4,
    maxTokens: 8,
    requestCount: 1,
    waves: [{ after_ms: 0, count: 1 }],
    preInferenceHoldMs: 0,
    timeoutMs: 1000,
  };
  assert.throws(() => buildConfig(oversubscribed), /4 stages on 2 devices/);
  assert.doesNotThrow(() =>
    buildConfig({ ...oversubscribed, allowOversubscribedDevices: true }));
});
