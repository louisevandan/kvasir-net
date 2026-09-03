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

// Prefill sizes a real service actually sees.
//
// Every scenario above asks the same 19-token question, so a request
// contributes 19 prefill rows once and one decode row per step thereafter.
// That is a batching problem with one shape. The scheduler's actual job is
// harder: a 1,600-token prompt and a one-row decode want the same UBATCH, and
// water-filling across those is where a strategy either exists or does not.
// These sentences vary rather than repeat, because a degenerate prefix
// invites a degenerate answer and the judge would then fail a run for a
// reason that has nothing to do with batching.
const BACKGROUND = [
  "타입 시스템은 값의 집합과 그 위에서 허용되는 연산을 함께 규정한다.",
  "구조적 타이핑은 이름이 아니라 형태가 같으면 대입을 허용한다.",
  "제네릭은 타입을 값처럼 매개변수로 받아 재사용 가능한 계약을 만든다.",
  "유니온 타입은 여러 가능성을 한 자리에 담고 좁히기로 분해된다.",
  "교차 타입은 여러 계약을 동시에 만족하는 값을 표현한다.",
  "리터럴 타입은 단일 값을 타입으로 승격시켜 상태를 표현하게 한다.",
  "판별 유니온은 공통 태그로 분기를 컴파일러가 검증하게 만든다.",
  "조건부 타입은 타입 수준의 분기이며 분배 법칙을 따른다.",
  "매핑된 타입은 기존 타입의 키를 순회하며 새 타입을 만든다.",
  "템플릿 리터럴 타입은 문자열 조합을 타입 수준에서 계산한다.",
  "타입 추론은 선언을 줄이지만 공개 API에서는 명시가 계약을 고정한다.",
  "any는 검사를 끄고 unknown은 검사를 미루므로 둘은 같지 않다.",
  "never는 값이 없는 타입이며 도달 불가능을 표현한다.",
  "readonly는 재할당을 막을 뿐 깊은 불변을 보장하지 않는다.",
  "선택적 속성과 undefined 유니온은 미묘하게 다른 계약이다.",
  "함수 매개변수는 반공변이고 반환값은 공변이다.",
];

/// A prompt whose background section is `lines` sentences long.
///
/// A first attempt sized these by characters and the load was refused: this
/// tokenizer spends far more tokens on Korean than the 2.8 characters each
/// that the 19-token acceptance prompt suggests, because that prompt is
/// mostly single-token turn markers. The run's own `prefill_rows` is what
/// the report quotes; these numbers only have to produce a spread that fits.
function backgroundPrompt(lines) {
  if (lines === 0) return ACCEPTANCE_PROMPT;
  const body = [];
  for (let index = 0; index < lines; index += 1) {
    body.push(`${index + 1}. ${BACKGROUND[index % BACKGROUND.length]}`);
  }
  return gemma4Turn(`${body.join("\n")}\n\n위 배경을 참고하여, ${ACCEPTANCE_QUESTION}`);
}

// Heavy-tailed, like traffic rather than like a benchmark: half the requests
// are the bare question, and the tail carries most of the prefill rows.
const PREFILL_MIX = [0, 0, 0, 0, 0, 0, 0, 0, 8, 8, 8, 8, 24, 24, 56, 56];

/// The prompt for request `index` under a mixed-prefill scenario. Deterministic
/// in the index, so two runs of the same scenario send the same work.
export function mixedPrefillPrompt(index) {
  return backgroundPrompt(PREFILL_MIX[index % PREFILL_MIX.length]);
}

export const GEMMA4_CUTS = [[0, 5], [5, 9], [9, 13], [13, 35]];

// Two stages per card. Locally that is a 3090 and a 4080; on the remote host
// it is two 3090s. Either way the split is four stages over two devices.
export const GEMMA4_DEVICES = ["0", "0", "1", "1"];

// The model path is the same on either host: S: is a mapped drive that an
// SSH logon cannot see but a process owned by the logged-on user can, which
// is why the remote agent runs as an interactive scheduled task.
export const MODEL = "S:\\models\\unsloth\\gemma-4-E2B-it-GGUF\\gemma-4-E2B-it-Q8_0.gguf";
export const BINARY = "F:\\dev\\p4\\target\\p4-staged-cuda\\p4_staged_server.exe";
export const REMOTE_ROOT = "C:\\Users\\42mob\\p4-remote";
export const REMOTE_BINARY = `${REMOTE_ROOT}\\staged\\p4_staged_server.exe`;

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

  // What the batching strategy is actually for: prompts of four very
  // different sizes arriving continuously while earlier requests decode, at a
  // concurrency high enough that the UBATCH is worth filling. A step here has
  // to water-fill a 1,600-row prefill demand and a hundred one-row decodes
  // into the same 512-wide physical batch, and the report's mixed-batch count
  // and fill say whether it did.
  //
  // Per-sequence context is 2560 because the longest prompt measured 2,048
  // short of its budget and the load was refused; 96 sequences puts total
  // context at 245,760, which at the measured 54 MiB per 1,024 over a
  // 2,840 MiB base leaves headroom on a 24 GiB card.
  prefill_mix: {
    ...base,
    description: "96 sequences, mixed prefill sizes, 12 arrivals every second",
    parallel: 96,
    context: 2560,
    maxTokens: 200,
    promptFor: mixedPrefillPrompt,
    waves: Array.from({ length: 16 }, (_, index) => ({
      after_ms: index * 1_000,
      count: 12,
    })),
  },

  // Concurrency pressure. `service` and `mixed` were built to prove the path,
  // not to saturate it: their steady-state active set is about 16 to 40, so a
  // 512-wide UBATCH could never be more than a few percent full whatever the
  // scheduler did. These raise the active set by an order of magnitude and
  // make arrivals large and frequent enough that prefill is essentially
  // always co-resident with decode.
  //
  // Per-sequence context is cut to 512 because the acceptance prompt is 19
  // tokens and the budget is 200: total context is parallel x context, so
  // holding 2048 per sequence would reserve four times the KV these requests
  // can ever use and cap concurrency on memory instead of on scheduling.
  pressure: {
    ...base,
    description: "256 sequences, 32 arrivals every second for 16 s",
    parallel: 256,
    context: 512,
    maxTokens: 200,
    waves: Array.from({ length: 16 }, (_, index) => ({
      after_ms: index * 1_000,
      count: 32,
    })),
  },

  // Half the sequences of `pressure` so the same run can say whether width is
  // limited by the active set or by something that does not scale with it.
  pressure_128: {
    ...base,
    description: "128 sequences, 16 arrivals every second for 16 s",
    parallel: 128,
    context: 512,
    maxTokens: 200,
    waves: Array.from({ length: 16 }, (_, index) => ({
      after_ms: index * 1_000,
      count: 16,
    })),
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
  // than directly. The agent advertises the tunnel entrance rather than its own
  // LAN address: event endpoints are matched by value, so an agent calling
  // itself 192.168.0.29 rejects traffic the drive addressed to 127.0.0.1. The
  // stage servers it spawns are all local to the remote host either way.
  remote: {
    ingress: "tcp://127.0.0.1:42003",
    binary: REMOTE_BINARY,
    tunnel: {
      host: "42mob@192.168.0.29",
      localPort: 42003,
      remotePort: 42003,
      root: REMOTE_ROOT,
    },
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
