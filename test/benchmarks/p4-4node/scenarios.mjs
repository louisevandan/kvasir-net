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
// The background sentences.
//
// There have to be at least as many as the longest prompt uses. A first
// version had sixteen and cycled them, so the 56-line prompt repeated each
// sentence three and a half times - and the model noticed, spending its whole
// budget reasoning in English about "a list of 56 numbered statements, many
// duplicated (1-16, 17-32, 33-48, 49-56)" and never answering in Korean. Two
// of sixty-four runs failed the judge for that, and it was the fixture.
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
  "오버로드 시그니처는 구현 시그니처와 별개로 검사된다.",
  "타입 가드는 런타임 검사와 타입 좁히기를 잇는 다리다.",
  "assertion 함수는 통과하면 그 이후를 좁힌 상태로 만든다.",
  "선언 병합은 인터페이스를 여러 곳에서 확장하게 하지만 추적을 어렵게 한다.",
  "열거형은 런타임 객체를 남기지만 const 열거형은 호출 지점에 인라인된다.",
  "데코레이터는 메타데이터를 붙일 뿐 타입을 바꾸지 않는다.",
  "타입 단언은 검사를 우회하므로 증거가 있을 때만 쓴다.",
  "제네릭 제약은 추론 범위를 좁혀 오류 메시지를 읽을 만하게 만든다.",
  "인덱스 시그니처는 편의를 주지만 키의 존재를 보장하지 않는다.",
  "튜플은 길이와 위치가 의미를 갖는 배열 계약이다.",
  "satisfies는 값의 타입을 넓히지 않으면서 계약 적합성만 검사한다.",
  "컴파일 대상과 라이브러리 선언이 함께 사용 가능한 API를 정한다.",
  "strictNullChecks는 null 처리 누락을 컴파일 시점 오류로 바꾼다.",
  "점진적 타이핑은 기존 코드를 한 번에 바꾸지 않아도 되게 한다.",
  "선언 파일은 런타임 코드 없이 계약만 배포하는 수단이다.",
  "타입 수준 재귀는 강력하지만 컴파일 시간을 지수적으로 늘릴 수 있다.",
  "키 재매핑은 매핑된 타입에서 키 이름 자체를 바꾼다.",
  "infer는 조건부 타입 안에서 부분 구조를 이름 붙여 꺼낸다.",
  "가변 인자 튜플은 함수 시그니처를 타입 수준에서 조립하게 한다.",
  "this 타입은 메서드 체인이 하위 타입을 잃지 않게 한다.",
  "추상 클래스는 구현 없는 멤버로 계약만 상속시킨다.",
  "믹스인은 클래스를 반환하는 함수로 다중 상속을 흉내 낸다.",
  "모듈 해석 전략은 같은 코드가 다른 파일을 가리키게 만들 수 있다.",
  "경로 별칭은 편의를 주지만 런타임 해석기와 어긋날 수 있다.",
  "타입 전용 import는 방출된 코드에서 사라져 순환 의존을 끊는다.",
  "구조적 호환성은 초과 속성 검사 때문에 리터럴에서만 엄격해진다.",
  "부분 타입은 갱신 API를 표현하지만 필수 필드 누락을 숨긴다.",
  "레코드 타입은 키 집합과 값 타입을 한꺼번에 규정한다.",
  "제외 타입은 유니온에서 특정 구성원을 걷어낸다.",
  "반환 타입 추출은 함수의 계약을 다른 곳에서 재사용하게 한다.",
  "브랜드 타입은 구조가 같아도 다른 것으로 다루게 만드는 표식이다.",
  "불투명 타입은 내부 표현을 감추고 생성 경로를 강제한다.",
  "옵셔널 체이닝은 존재 검사와 접근을 한 표현으로 합친다.",
  "널 병합은 falsy와 nullish를 구분해 기본값을 고른다.",
  "단언 시그니처는 검증 함수의 효과를 타입 시스템에 알린다.",
  "제네릭 기본값은 호출자가 생략해도 합리적인 타입을 정한다.",
  "재귀적 부분 타입은 깊은 구조에 선택성을 퍼뜨린다.",
  "판별자 없는 유니온은 좁히기를 어렵게 하므로 태그를 붙이는 편이 낫다.",
  "컴파일러 옵션은 같은 소스에 다른 계약을 부여할 수 있다.",
  "타입 검사와 방출은 분리되어 있어 오류가 있어도 코드가 나올 수 있다.",
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
/// The same mix with the thinking block already closed, for a model that
/// would otherwise open one.
export function mixedPrefillPromptNoThinking(index) {
  return `${mixedPrefillPrompt(index)}<think></think>${String.fromCharCode(10)}${String.fromCharCode(10)}`;
}

export function mixedPrefillPrompt(index) {
  return backgroundPrompt(PREFILL_MIX[index % PREFILL_MIX.length]);
}

// ChatML, as the Qwen3.5 family (qwen35 / qwen35moe / qwen4exp, and the
// Ornith-1.0-35B fine-tune, whose GGUF template is ChatML with <|im_end|> as
// EOS) formats a turn. Read from the GGUF `tokenizer.chat_template` on
// 2026-09-07; BOS is not added by these tokenizers, so nothing is written here.
export function chatmlTurn(question) {
  return `<|im_start|>user\n${question}<|im_end|>\n<|im_start|>assistant\n`;
}

function backgroundBody(lines) {
  if (lines === 0) return ACCEPTANCE_QUESTION;
  const body = [];
  for (let index = 0; index < lines; index += 1) {
    body.push(`${index + 1}. ${BACKGROUND[index % BACKGROUND.length]}`);
  }
  return `${body.join("\n")}\n\n위 배경을 참고하여, ${ACCEPTANCE_QUESTION}`;
}

/// The same heavy-tailed prefill mix as `mixedPrefillPrompt`, in ChatML.
export function mixedPrefillPromptChatml(index) {
  return chatmlTurn(backgroundBody(PREFILL_MIX[index % PREFILL_MIX.length]));
}

export const STOPS_CHATML = ["<|im_end|>", "<|im_start|>"];

/// ChatML with the thinking block already closed, as this family's own
/// template writes it.
///
/// Read from Ornith-1.0-35B's GGUF `tokenizer.chat_template` on 2026-09-09:
/// after `<|im_start|>assistant\n` the template emits `<think>\n` by default
/// and `<think>\n\n</think>\n\n` when `enable_thinking` is false. P4 owns no
/// template, so OUTER writes the second form itself. The exact newlines
/// matter - this is the string the model was tuned to see, not an
/// approximation of it.
export function chatmlTurnClosedThinking(question) {
  return `${chatmlTurn(question)}<think>\n\n</think>\n\n`;
}

/// The acceptance question in that form, for scenarios whose prompt does not
/// vary by request.
export function acceptancePromptChatmlNoThinking() {
  return chatmlTurnClosedThinking(ACCEPTANCE_QUESTION);
}

/// Cuts a model across the execution lanes the hardware actually has.
///
/// Measured 2026-09-04, interleaved, every arm passing its judge: one stage a
/// card beats two stages a card by 42% on gemma-4-E2B (624.98 against 438.88
/// total rows/s) and 33% on a 35B (175.00 against 131.96). Two cards give two
/// independent lanes; a third and fourth stage add a full set of per-batch
/// fixed cost - frame decode, `llama_decode`, cut-set copy, process hop - to
/// lanes that cannot overlap, 78 to 149 ms a lap here. Two processes on one
/// card do not pipeline, they contend.
///
/// So a scenario names its devices and its model, and the split follows. It
/// used to name the split too, and every scenario wrote four because the
/// harness is called four-node - a name that comes from where gemma-4-E2B
/// allows a boundary, not from a measurement.
///
/// **This produces a legal cut, not the best one.** Layers are dealt evenly
/// where the constraint allows, and even layers are not even cost: the first
/// stage carries the embedding and `layers/agent/tests/pipelining.rs` fixes
/// 16/24 over 20/20 for exactly that reason, while the 35B's own two-stage
/// runs measured 81.4 ms against 90.2 ms. Choosing the cut belongs to a
/// later step that reads measured stage time, memory and transfer cost; this
/// only says where a boundary may go and hands back a reasonable seed.
///
/// `forbidden` is the region no boundary may fall inside, as `[begin, end)`.
/// A model that shares KV across a span of layers reads them as one storage
/// region: gemma-4-E2B shares over 13..34, so `[13, 35)` is forbidden and the
/// only two-way cut is `[0,13)` and `[13,35)`. Layers are dealt as evenly as
/// the constraint allows, and a boundary that would land inside the region is
/// pushed to its start.
export function cutForLanes(totalLayers, lanes, forbidden) {
  if (!Number.isInteger(totalLayers) || totalLayers < 1) {
    throw new Error('cutForLanes needs a positive layer count');
  }
  if (!Number.isInteger(lanes) || lanes < 1 || lanes > totalLayers) {
    throw new Error('cutForLanes needs between one lane and one lane per layer');
  }
  // Without a shared region every boundary is legal, so deal the layers out
  // as evenly as they divide.
  const even = (from, to, parts) => {
    const span = to - from;
    if (parts > span) {
      throw new Error(`cannot cut ${span} layers into ${parts} stages`);
    }
    const edges = [from];
    for (let part = 1; part < parts; part += 1) {
      edges.push(from + Math.round((part * span) / parts));
    }
    edges.push(to);
    return edges.slice(0, -1).map((begin, index) => [begin, edges[index + 1]]);
  };
  if (!forbidden) return even(0, totalLayers, lanes);
  const [from, to] = forbidden;
  // A region that is empty, inverted or off the end is not a constraint, it
  // is a mistake - and an unchecked one produced `[[0,35],[35,35]]` from
  // `cutForLanes(35, 2, [35, 35])`: an empty last stage, from a function whose
  // whole point is to refuse what the constraint cannot give.
  if (![from, to].every((edge) => Number.isInteger(edge))
    || from < 0 || from >= to || to > totalLayers) {
    throw new Error(
      `a shared region must be integer [begin, end) inside 0..${totalLayers}, not` +
        ` [${from}, ${to})`,
    );
  }
  // A shared region is one storage region: it is a whole stage, not a place
  // to put a boundary. A first version dealt the layers evenly and then
  // pushed offending boundaries back to the region's start, where the second
  // one collided with the first; the region has to be reserved before the
  // rest is divided, not repaired afterwards.
  if (to !== totalLayers) {
    throw new Error(
      'cutForLanes only handles a shared region that runs to the last layer',
    );
  }
  if (lanes === 1) return [[0, totalLayers]];
  return [...even(0, from, lanes - 1), [from, totalLayers]];
}

/// The devices this harness may use, in the order a stage is dealt one.
///
/// One stage a device: that is what an independent execution lane is, and
/// measuring past it cost a third to a half of the throughput.
/// **A device ordinal is a stand-in for a lane, not a definition of one.**
/// Two entries differ here only as strings, so this cannot tell `CUDA:0` on
/// two hosts apart, or CUDA 0 from CPU 0, or MIG instances, Metal devices,
/// NUMA domains, a stage spread over several GPUs, or tensors that fell back
/// to host buffers. The adapter says as much: `backend_inventory` enumerates
/// registries and explicitly is not placement, and the `execution_layout_id`
/// that would carry real placement is unimplemented. This is a benchmark
/// harness on one host with two identical cards, and it is only sound there.
export const LANES = ["0", "1"];

/// A scenario's placement, derived rather than written.
///
/// Give it the model's layer count and its shared-KV region, and it returns
/// the `cuts` and `devices` a scenario used to spell out by hand. Every
/// scenario wrote four stages because the harness is called four-node;
/// nothing checked that four was a good number until it was measured, and it
/// was not.
export function placeOnLanes(totalLayers, forbidden, lanes = LANES) {
  return {
    cuts: cutForLanes(totalLayers, lanes.length, forbidden),
    devices: [...lanes],
  };
}
/// gemma-4-E2B: 35 layers, KV shared over 13..34.
export const GEMMA4_LAYERS = 35;
export const GEMMA4_SHARED_KV = [13, 35];

/// The 35B: 40 layers, no shared region to avoid.
export const ORNITH35B_LAYERS = 40;
export const GEMMA4_CUTS = [[0, 5], [5, 9], [9, 13], [13, 35]];

// Two stages per card. Locally that is a 3090 and a 4080; on the remote host
// it is two 3090s. Either way the split is four stages over two devices.
export const GEMMA4_DEVICES = ["0", "0", "1", "1"];

// The model path is the same on either host: S: is a mapped drive that an
// SSH logon cannot see but a process owned by the logged-on user can, which
// is why the remote agent runs as an interactive scheduled task.
export const MODEL = "S:\\models\\unsloth\\gemma-4-E2B-it-GGUF\\gemma-4-E2B-it-Q8_0.gguf";
// A model large enough to say whether the small one distorted the picture.
//
// gemma-4-E2B put 0.29 ms a row into sampling against 0.11 ms a row into
// its 22 layers, which is what made the sampler worth parallelising. That
// ratio is a property of a 2B model with a 249k vocabulary: the layers are
// cheap and the vocabulary is not. A 35B model over 40 layers should invert
// it, and if it does the sampler work matters far less than the small-model
// runs suggested.
//
// qwen35moe, 40 layers, 23.2 GiB at Q5_K_S. Two 3090s hold it at two stages
// a card, about 11.6 GiB of weights per card, so the context budget is much
// tighter than the 2B scenarios'. Flash attention is on and the V cache is
// quantised, as the 2026-08-20 four-card reference ran it.
export const MODEL_35B =
  "S:\\models\\unsloth\\Ornith-1.0-35B-GGUF\\Ornith-1.0-35B-UD-Q5_K_S.gguf";
export const CUTS_35B = [[0, 10], [10, 20], [20, 30], [30, 40]];

/// Where this model ends a turn.
///
/// It is prompted with the gemma turn markers, which it follows closely
/// enough to answer, but it closes with `</turn|>` and `<|end|>` and then
/// opens a fresh `<|turn>model` and starts over. Three of sixty-four runs
/// failed the judge for degenerate repetition that was exactly that second
/// turn. Stop strings are OUTER's to supply - P4 carries them inside the
/// request options and never reads them.
export const STOPS_35B = ["</turn|>", "<turn|>", "<|end|>", "<|turn>user"];

/// No thinking, please.
///
/// This model opens every answer with a `<think>` block and spent most of a
/// 200-token budget inside it; at 600 it got out, but the block is still most
/// of what the run measures and none of it is the answer. A budget of zero
/// makes llama.cpp's reasoning-budget sampler force the closing tag at once,
/// which is what `enable_thinking: false` does in a chat template - except
/// that P4 owns no template, so the tags arrive from OUTER with the request.
/// The adapter already carried these - flat keys, advertised in HELLO as
/// reasoning_budget_tokens / _start_tag / _end_tags / _message. A nested
/// `reasoning` object was written before checking, and the parser refused it:
/// the option list is an allowlist, which is why the mistake surfaced as a
/// rejected request rather than a silently ignored setting.
/// Ends the thinking block in the prompt, before the model can open one.
///
/// The budget sampler alone is not enough: with a budget of zero it forces
/// `</think>` the moment a block opens, the model opens another, and four of
/// sixty-four answers became `<think></think>` repeated to the token limit.
/// What `enable_thinking: false` actually does in a chat template is put the
/// closed block in the prompt so no block is ever opened, and that is this
/// layer's job - P4 carries the prompt verbatim.
export function gemma4TurnClosedThinking(question) {
  return `${gemma4Turn(question)}<think></think>${String.fromCharCode(10)}${String.fromCharCode(10)}`;
}

export const NO_THINKING_35B = {
  reasoning_budget_tokens: 0,
  reasoning_budget_start_tag: "<think>",
  reasoning_budget_end_tags: ["</think>"],
};

// 2026-09-07 local resource ladder (RTX 3090 24 GiB + RTX 4080 16 GiB, 256 GiB
// host RAM). GGUF headers read that day: gemma-4-31B is dense, 60 layers,
// 19.7 GiB; Qwen3.5-122B-A10B is qwen35moe, 49 blocks of which one is a
// NextN (MTP) layer (`nextn_predict_layers=1`), so the trunk llama.cpp cuts
// is 48 layers - a cut ending at 49 trips `apply_linkcpp_stage`'s
// `end <= n_layer` assert. 256 experts (8 used), 82.2 GiB of which 75.9 GiB
// are routed experts. Both live on the S: share,
// which the stage processes read through mmap (from the central PC that
// share measured about 64 MB/s, so a load is minutes, not seconds).
export const MODEL_31B =
  "S:\\models\\unsloth\\gemma-4-31B-it-GGUF\\gemma-4-31B-it-Q5_K_S.gguf";
export const GEMMA4_31B_LAYERS = 60;
export const MODEL_122B =
  "S:\\models\\unsloth\\Qwen3.5-122B-A10B-MTP-GGUF\\Qwen3.5-122B-A10B-UD-Q5_K_S-00001-of-00003.gguf";
export const QWEN35_122B_LAYERS = 48;

/// Every routed-expert weight of every owned layer stays on CPU and is
/// computed there (what llama.cpp's --cpu-moe does), written as an
/// override pattern so it composes with the unowned-layer pattern. Routers,
/// attention, norms, embeddings and the KV cache stay on the GPU.
export const EXPERTS_TO_CPU = ["blk\\..*\\.ffn_(up|down|gate)_exps.*=CPU"];

/// ChatML with the thinking block already closed, the Qwen3 convention.
export function mixedPrefillPromptChatmlNoThinking(index) {
  return `${mixedPrefillPromptChatml(index)}<think>\n\n</think>\n\n`;
}

export const BINARY = "F:\\dev\\p4\\target\\p4-staged-cuda\\p4_staged_server.exe";
export const REMOTE_ROOT = "C:\\Users\\42mob\\p4-remote";
export const REMOTE_BINARY = `${REMOTE_ROOT}\\staged\\p4_staged_server.exe`;

const base = {
  cuts: GEMMA4_CUTS,
  devices: GEMMA4_DEVICES,
  // Placement is deliberately absent here. Putting the oversubscription
  // exception in `base` made every scenario inherit it, including the
  // two-stage ones that do not oversubscribe at all - so the function
  // defaulted to refusing while the catalogue defaulted to allowing. Each
  // four-stage arm now says so for itself.
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
    // Two stages a card: the slower arm, kept because it is what the
    // lane-derived scenarios are compared against and what every earlier
    // run in the evidence used.
    allowOversubscribedDevices: true,
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
    // Two stages a card: the slower arm, kept because it is what the
    // lane-derived scenarios are compared against and what every earlier
    // run in the evidence used.
    allowOversubscribedDevices: true,
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

  // The same load with the tail alone on a card.
  //
  // Throughput across ten runs correlates with one thing at r=0.92: the share
  // of the run during which two or more stages are computing at once. Not
  // batch width, which is mildly negative. The stage cost fits 54.7 ms per
  // batch plus 3.04 ms per layer, so the 22-layer tail costs 121 ms against
  // 63 for a 4-layer stage - and under the default placement that tail shares
  // GPU1 with stage 2, so the two contend exactly when they should overlap.
  // The 5/4/4/22 cut cannot move (gemma-4-E2B shares KV over layers 13..34),
  // but the placement can.
  prefill_mix_tail_alone: {
    ...base,
    // Two stages a card: the slower arm, kept because it is what the
    // lane-derived scenarios are compared against and what every earlier
    // run in the evidence used.
    allowOversubscribedDevices: true,
    description: "96 sequences, mixed prefill, three light stages on one card and the tail on the other",
    devices: ["0", "0", "0", "1"],
    parallel: 96,
    context: 2560,
    maxTokens: 200,
    promptFor: mixedPrefillPrompt,
    waves: Array.from({ length: 16 }, (_, index) => ({
      after_ms: index * 1_000,
      count: 12,
    })),
  },

  // The 35B smoke: does it load at this split and answer once.
  smoke_35b: {
    ...base,
    description: "35B over four stages on two cards, one request",
    model: MODEL_35B,
    cuts: CUTS_35B,
    allowOversubscribedDevices: true,
    stops: STOPS_35B,

    flashAttn: "on",
    cacheTypeK: "q8_0",
    cacheTypeV: "q8_0",
    parallel: 4,
    context: 2560,
    maxTokens: 200,
    waves: [{ after_ms: 0, count: 1 }],
  },

  // `prefill_mix` on the 35B. Concurrency is 32 rather than 96 because the
  // weights take about 11.6 GiB of each card and the KV of 40 layers is far
  // heavier than the 2B model's 35 with its shared region.
  prefill_mix_35b: {
    ...base,
    description: "35B, 32 sequences, mixed prefill sizes, 8 arrivals every second",
    model: MODEL_35B,
    cuts: CUTS_35B,
    allowOversubscribedDevices: true,
    stops: STOPS_35B,

    flashAttn: "on",
    cacheTypeK: "q8_0",
    cacheTypeV: "q8_0",
    parallel: 32,
    context: 2560,
    // 600 rather than the 200 the 2B scenarios use. This model opens its
    // answer with a <think> block, and at 200 tokens a quarter of the runs
    // were still inside it when the budget ran out - the judge then failed
    // them for naming too few domain terms, which was the budget and not the
    // pipeline. Structural completion was 64/64 in that run.
    maxTokens: 600,
    promptFor: mixedPrefillPromptNoThinking,
    waves: Array.from({ length: 8 }, (_, index) => ({
      after_ms: index * 1_000,
      count: 8,
    })),
  },

  // The 2B load at one stage a card, to see whether the partition rule is
  // about the model or about the hardware.
  //
  // gemma-4-E2B shares KV across layers 13..34, so no boundary may fall inside
  // that region - which allows exactly one two-way cut, [0,13) and [13,35).
  // The four-node split exists because of that constraint, not because four
  // stages were measured to be better.
  prefill_mix_2stage: {
    ...base,
    description: "96 sequences, mixed prefill, one stage a card",
    ...placeOnLanes(GEMMA4_LAYERS, GEMMA4_SHARED_KV),
    parallel: 96,
    context: 2560,
    maxTokens: 200,
    promptFor: mixedPrefillPrompt,
    waves: Array.from({ length: 16 }, (_, index) => ({
      after_ms: index * 1_000,
      count: 12,
    })),
  },

  // The same 35B load with one stage a card instead of two.
  //
  // Four stages over two GPUs gives two independent execution lanes and four
  // sets of per-batch fixed cost - a frame decode, a llama_decode, a cut-set
  // copy and a process hop, each paid four times a lap where the hardware can
  // only overlap two. The stage-span metrics cannot settle this: they cover
  // whole stage RPCs, so four of them read as open at once on two devices.
  // Running the same work at the depth the cards actually provide does settle
  // it. Same model, cut, context, arrivals and prompts; only the partition
  // changes.
  prefill_mix_35b_2stage: {
    ...base,
    description: "35B, one stage a card, 32 sequences, mixed prefill",
    model: MODEL_35B,
    ...placeOnLanes(ORNITH35B_LAYERS),
    stops: STOPS_35B,
    flashAttn: "on",
    cacheTypeK: "q8_0",
    cacheTypeV: "q8_0",
    parallel: 32,
    context: 2560,
    maxTokens: 600,
    promptFor: mixedPrefillPromptNoThinking,
    waves: Array.from({ length: 8 }, (_, index) => ({
      after_ms: index * 1_000,
      count: 8,
    })),
  },

  // ---- 2026-09-07 local resource ladder (3090 + 4080) ----
  //
  // VRAM-only, a dense 31B split unevenly so the 16 GiB card holds 24 of the
  // 60 layers. Same mixed prefill and gemma-4 template as the 2B arms.
  vram_31b_2stage: {
    ...base,
    description: "gemma-4-31B dense, VRAM-only, two stages 36/24 layers, 16 sequences, mixed prefill",
    model: MODEL_31B,
    cuts: [[0, 36], [36, 60]],
    devices: ["0", "1"],
    flashAttn: "on",
    cacheTypeK: "q8_0",
    cacheTypeV: "q8_0",
    parallel: 16,
    context: 2048,
    maxTokens: 400,
    promptFor: mixedPrefillPrompt,
    waves: Array.from({ length: 8 }, (_, index) => ({
      after_ms: index * 1_000,
      count: 4,
    })),
  },

  // The same 35B MoE as prefill_mix_35b_2stage, but with its routed experts
  // kept and computed on CPU (RAM offload), mmap on, and the ChatML template
  // its GGUF declares (prefill_mix_35b_2stage sends gemma-4 turns to it).
  offload_35b_moe_2stage: {
    ...base,
    description: "35B MoE, experts on CPU, one stage a card, 32 sequences, mixed prefill (ChatML)",
    model: MODEL_35B,
    ...placeOnLanes(ORNITH35B_LAYERS),
    // mmap stays off: with the GGUF on the S: SMB share, mmapped experts
    // fault in page by page during the first prefill and the run stalled
    // (2026-09-07: one stage execution in 30 minutes). --no-mmap reads the
    // owned experts sequentially at load into host RAM instead.
    mmap: false,
    overrideTensors: EXPERTS_TO_CPU,
    // Load from the share plus CPU expert compute do not fit the 30-minute
    // default.
    timeoutMs: 3_600_000,
    stops: STOPS_CHATML,
    flashAttn: "on",
    cacheTypeK: "q8_0",
    cacheTypeV: "q8_0",
    parallel: 32,
    context: 2560,
    maxTokens: 600,
    promptFor: mixedPrefillPromptChatmlNoThinking,
    waves: Array.from({ length: 8 }, (_, index) => ({
      after_ms: index * 1_000,
      count: 8,
    })),
  },

  // RAM offload proper: a 122B-A10B MoE whose 76 GiB of experts cannot fit
  // the cards, cut across both. A one-stage baseline of the same offload
  // layout is not expressible here: p4-event-drive refuses a run with fewer
  // than two nodes (`run/config.rs::validate`).
  offload_122b_moe_2stage: {
    ...base,
    description: "Qwen3.5-122B-A10B MoE, experts on CPU, one stage a card, 16 sequences, mixed prefill (ChatML)",
    model: MODEL_122B,
    ...placeOnLanes(QWEN35_122B_LAYERS),
    // mmap stays off: with the GGUF on the S: SMB share, mmapped experts
    // fault in page by page during the first prefill and the run stalled
    // (2026-09-07: one stage execution in 30 minutes). --no-mmap reads the
    // owned experts sequentially at load into host RAM instead.
    mmap: false,
    overrideTensors: EXPERTS_TO_CPU,
    // Load from the share plus CPU expert compute do not fit the 30-minute
    // default.
    timeoutMs: 3_600_000,
    stops: STOPS_CHATML,
    flashAttn: "on",
    cacheTypeK: "q8_0",
    cacheTypeV: "q8_0",
    parallel: 16,
    context: 2048,
    maxTokens: 300,
    promptFor: mixedPrefillPromptChatmlNoThinking,
    waves: Array.from({ length: 8 }, (_, index) => ({
      after_ms: index * 1_000,
      count: 4,
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
    // Two stages a card: the slower arm, kept because it is what the
    // lane-derived scenarios are compared against and what every earlier
    // run in the evidence used.
    allowOversubscribedDevices: true,
    description: "256 sequences, 32 arrivals every second for 16 s",
    parallel: 256,
    context: 512,
    maxTokens: 200,
    waves: Array.from({ length: 16 }, (_, index) => ({
      after_ms: index * 1_000,
      count: 32,
    })),
  },

  // `pressure` at the depth the cards actually provide.
  //
  // The four-stage arm above measured 15.95% UBATCH fill and 21.4/31.9% GPU
  // over its stage-execution window, and a first-node stage time of 78.4,
  // 57.9, 57.6 and 149.9 ms for stages owning 5, 4, 4 and 22 layers. Fitting
  // a line through the two four-layer stages and the twenty-two-layer one
  // puts about 37 ms of that on per-stage fixed cost - a frame decode, a
  // llama_decode, a cut-set copy and a process hop - which four stages pay
  // four times a lap on two cards that can only overlap two. This arm pays it
  // twice. Same model, resident set, arrivals, context and prompts; only the
  // partition changes, so the difference is attributable.
  pressure_2stage: {
    ...base,
    description: "256 sequences, one stage a card, 32 arrivals every second for 16 s",
    ...placeOnLanes(GEMMA4_LAYERS, GEMMA4_SHARED_KV),
    parallel: 256,
    context: 512,
    maxTokens: 200,
    waves: Array.from({ length: 16 }, (_, index) => ({
      after_ms: index * 1_000,
      count: 32,
    })),
  },

  // The same pressure on a model that is not 2B, in the template its own
  // GGUF declares, with thinking closed before it can open.
  //
  // **The resident set here is bounded by recurrent state, not by context.**
  // This 35B is a hybrid: of its 40 layers only ten hold an attention KV
  // cache, and the rest hold a recurrent state that is per sequence and does
  // not shrink with the context length. Measured on 2026-09-09 at 256
  // sequences, one stage of twenty layers reported
  // `llama_memory_recurrent: size = 8040.00 MiB (256 cells, 40 layers,
  // 256 seqs)` against `llama_kv_cache: size = 680.00 MiB (131072 cells)` -
  // 31.4 MiB a sequence of recurrent state against 5.3 MiB of KV. The stage
  // refused the load: plan 19.72 GiB required against 15.43 GiB free. Ninety
  // six sequences is what fits with the weights beside it, so cutting the
  // per-sequence context would not have bought a single extra sequence.
  pressure_35b: {
    ...base,
    description: "35B, one stage a card, 96 sequences, 32 arrivals every second for 16 s",
    model: MODEL_35B,
    ...placeOnLanes(ORNITH35B_LAYERS),
    stops: STOPS_CHATML,
    flashAttn: "on",
    cacheTypeK: "q8_0",
    cacheTypeV: "q8_0",
    parallel: 96,
    context: 512,
    maxTokens: 200,
    promptFor: acceptancePromptChatmlNoThinking,
    waves: Array.from({ length: 16 }, (_, index) => ({
      after_ms: index * 1_000,
      count: 32,
    })),
  },

  // The same 35B pressure over four stages on the same two cards.
  //
  // This is not only the partition experiment. The staged server refuses a
  // load whose plan does not fit the memory free at that moment, and the plan
  // counts a stage's whole share of the weights - so a two-stage split of a
  // 23 GiB model leaves little room for recurrent state and caps the resident
  // set near a hundred. Four stages halve each stage's share, so the same two
  // cards admit the resident set the 2B arms run at. Both variables move at
  // once here; `pressure_35b_4stage_96` holds the resident set at
  // `pressure_35b`'s so the partition can be read alone.
  pressure_35b_4stage: {
    ...base,
    allowOversubscribedDevices: true,
    description: "35B, two stages a card, 256 sequences, 32 arrivals every second for 16 s",
    model: MODEL_35B,
    cuts: CUTS_35B,
    devices: ["0", "0", "1", "1"],
    stops: STOPS_CHATML,
    flashAttn: "on",
    cacheTypeK: "q8_0",
    cacheTypeV: "q8_0",
    parallel: 256,
    context: 512,
    maxTokens: 200,
    promptFor: acceptancePromptChatmlNoThinking,
    waves: Array.from({ length: 16 }, (_, index) => ({
      after_ms: index * 1_000,
      count: 32,
    })),
  },

  // The partition on its own: four stages at `pressure_35b`'s resident set.
  pressure_35b_4stage_96: {
    ...base,
    allowOversubscribedDevices: true,
    description: "35B, two stages a card, 96 sequences, 32 arrivals every second for 16 s",
    model: MODEL_35B,
    cuts: CUTS_35B,
    devices: ["0", "0", "1", "1"],
    stops: STOPS_CHATML,
    flashAttn: "on",
    cacheTypeK: "q8_0",
    cacheTypeV: "q8_0",
    parallel: 96,
    context: 512,
    maxTokens: 200,
    promptFor: acceptancePromptChatmlNoThinking,
    waves: Array.from({ length: 16 }, (_, index) => ({
      after_ms: index * 1_000,
      count: 32,
    })),
  },

  // Half the sequences of `pressure` so the same run can say whether width is
  // limited by the active set or by something that does not scale with it.
  pressure_128: {
    ...base,
    // Two stages a card: the slower arm, kept because it is what the
    // lane-derived scenarios are compared against and what every earlier
    // run in the evidence used.
    allowOversubscribedDevices: true,
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
    // Two stages a card: the slower arm, kept because it is what the
    // lane-derived scenarios are compared against and what every earlier
    // run in the evidence used.
    allowOversubscribedDevices: true,
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
    // Two stages a card: the slower arm, kept because it is what the
    // lane-derived scenarios are compared against and what every earlier
    // run in the evidence used.
    allowOversubscribedDevices: true,
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
