// Scenario definitions for the four-node acceptance harness.
//
// Every scenario answers the same fixed prompt, because the acceptance bar is
// a meaningful Korean explanation of TypeScript and nothing else. What varies
// is the batching pressure the pipeline is put under while producing it.

export const ACCEPTANCE_QUESTION = "타입스크립트에 대해 한국어로 설명하라";

// The staged server tokenizes the prompt text verbatim (add_special and
// parse_special on) and never applies a chat template - formatting the turn
// structure is OUTER work, which is the correct layer. gemma-4 wraps each
// message as "<|turn>ROLE" + newline + content + "<turn|>" + newline, and the
// generation prompt is an opening model turn. BOS is added by the tokenizer,
// so it is not written here.
export function gemma4Turn(question) {
  return `<|turn>user\n${question}<turn|>\n<|turn>model\n`;
}

export const ACCEPTANCE_PROMPT = gemma4Turn(ACCEPTANCE_QUESTION);

// gemma-4-E2B shares KV: shared_kv_layers=20 over 35 layers leaves
// n_layer_kv_from_start=15, so layers 15..34 reuse the KV of layer 13 (SWA)
// or 14 (full). Layers 13..34 are one storage region and no stage boundary
// may fall inside it, which fixes a four-node split at 5/4/4/22.
export const GEMMA4_CUTS = [[0, 5], [5, 9], [9, 13], [13, 35]];

// Two stages per card. Locally that is a 3090 and a 4080; on the remote host
// it is two 3090s. Either way the split is four stages over two devices.
export const GEMMA4_DEVICES = ["0", "0", "1", "1"];

// The model path is the same on either host: S: is a mapped drive that an
// SSH logon cannot see but a process owned by the logged-on user can, which
// is why the remote agent runs as an interactive scheduled task.
export const MODEL = "S:\\models\\unsloth\\gemma-4-E2B-it-GGUF\\gemma-4-E2B-it-Q8_0.gguf";
export const BINARY = "F:\\dev\\p4\\target\\p4-staged-cuda\\p4_staged_server.exe";
export const REMOTE_BINARY = "C:\\Users\\42mob\\p4-remote\\staged\\p4_staged_server.exe";

const base = {
  cuts: GEMMA4_CUTS,
  devices: GEMMA4_DEVICES,
  model: MODEL,
  binary: BINARY,
  // Below the Windows dynamic port range (49152+), so an outbound
  // ephemeral connection cannot take the port from under the agent, and
  // outside every netsh excluded range on this machine.
  ingress: "tcp://127.0.0.1:42003",
  endpointBase: 42_011,
  nBatch: 512,
  nUbatch: 512,
  context: 2048,
  preInferenceHoldMs: 5_000,
  timeoutMs: 1_800_000,
};

/// `waves` is [{ after_ms, count }]; the harness derives request_count.
export const SCENARIOS = {
  // The smallest thing that proves the whole path: load four stages, answer
  // once, and mean it. Every later scenario is this plus pressure.
  smoke: {
    ...base,
    description: "single request, four stages, meaningful answer",
    parallel: 4,
    maxTokens: 400,
    waves: [{ after_ms: 0, count: 1 }],
  },

  // Continuous arrivals against a decode-dominated steady state. Measures the
  // amortization of the fixed per-step cost that the 2026-08-30 runs found.
  service: {
    ...base,
    description: "40 requests, 20 at once then 10 every 30 s, parallel 40",
    parallel: 40,
    maxTokens: 1000,
    waves: [
      { after_ms: 0, count: 20 },
      { after_ms: 30_000, count: 10 },
      { after_ms: 60_000, count: 10 },
    ],
  },

  // Arrivals spread across the whole run so a prompt is always waiting while
  // earlier requests decode. Prefill load is one-shot per request, so only a
  // continuous wave exercises the mixed-batch path fairly.
  mixed: {
    ...base,
    description: "continuous arrivals, prefill and decode co-resident",
    parallel: 24,
    maxTokens: 300,
    waves: Array.from({ length: 20 }, (_, index) => ({
      after_ms: index * 5_000,
      count: 2,
    })),
  },
};

/// Where the four stages run. `local` uses this machine; `remote` uses the
/// two-3090 host, whose agent must already be started by remote-agent.mjs.
export const TARGETS = {
  local: { ingress: "tcp://127.0.0.1:42003", binary: BINARY },
  // The remote host answers SSH and nothing else - ICMP and the agent port are
  // both blocked - so the drive reaches it through a local SSH forward rather
  // than directly. The agent still advertises its own LAN address because the
  // stage servers it spawns talk to each other on the remote side.
  remote: {
    ingress: "tcp://127.0.0.1:42003",
    binary: REMOTE_BINARY,
    tunnel: { host: "42mob@192.168.0.29", localPort: 42003, remotePort: 42003 },
  },
};

export function scenario(name, target = "local") {
  const value = SCENARIOS[name];
  if (!value) {
    throw new Error(`unknown scenario ${name}; known: ${Object.keys(SCENARIOS).join(", ")}`);
  }
  const placement = TARGETS[target];
  if (!placement) {
    throw new Error(`unknown target ${target}; known: ${Object.keys(TARGETS).join(", ")}`);
  }
  const requestCount = value.waves.reduce((total, wave) => total + wave.count, 0);
  return { name, target, requestCount, ...value, ...placement };
}
