'use strict';
/**
 * Tool calling for a model whose template the bridge renders itself.
 *
 * The rendering is checked against a golden prompt. That golden was compared
 * byte for byte with the GGUF's own `tokenizer.chat_template` rendered by
 * Jinja2 (Step-3.7-Flash-Q4_K_XL.gguf on GB10 #1), with only the generation
 * prompt differing: the template always opens `<think>\n`, and this fixture
 * asks for thinking off, which the bridge closes in the prompt.
 *
 * The parsing is checked whole and as a stream, including a stream cut at
 * every character, so an opener split across tokens cannot hide.
 */
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const http = require('node:http');
const path = require('node:path');

const toolcalls = require('../toolcalls.js');
const { promptFrom, createServer, engineOptions } = require('../server.js');

const FIXTURE = require('./fixtures/newtype-agent-turn.json');
const GOLDEN = fs.readFileSync(path.join(__dirname, 'fixtures', 'newtype-agent-turn.prompt.txt'), 'utf8');
const TOOLS = FIXTURE.tools;

const CALL = '<tool_call>\n<function=run_command>\n<parameter=command>\nls -la\n</parameter>\n'
  + '<parameter=timeout_s>\n30\n</parameter>\n</function>\n</tool_call>';

/* ---- rendering ---------------------------------------------------------- */

test('the Newtype fixture renders to the golden prompt', () => {
  assert.equal(promptFrom(FIXTURE, 'chatml', true, false, 'step'), GOLDEN);
});

test('a conversation without tools renders exactly as plain chatml did', () => {
  const body = { messages: [
    { role: 'system', content: 'Be brief.' },
    { role: 'user', content: 'hi' },
    { role: 'assistant', content: 'hello' },
    { role: 'user', content: 'again' },
  ] };
  for (const thinking of [true, false]) {
    assert.equal(promptFrom(body, 'chatml', true, thinking, 'step'), promptFrom(body, 'chatml', true, thinking, 'none'));
  }
});

test('tools open a system turn when the caller sent none', () => {
  const prompt = promptFrom({ messages: [{ role: 'user', content: 'list files' }], tools: TOOLS }, 'chatml', true, true, 'step');
  assert.ok(prompt.startsWith('<|im_start|>system\n# Tools\n\nYou have access to the following functions in JSONSchema format:\n\n<tools>\n{"type": "function"'));
  assert.ok(prompt.includes('</IMPORTANT><|im_end|>\n<|im_start|>user\nlist files<|im_end|>\n<|im_start|>assistant\n<think>\n'));
  assert.equal((prompt.match(/<\|im_start\|>system/g) ?? []).length, 1);
});

test('the caller system prompt goes first in the tool turn, once', () => {
  const prompt = promptFrom(FIXTURE, 'chatml', true, false, 'step');
  assert.ok(prompt.startsWith(`<|im_start|>system\n${FIXTURE.messages[0].content}\n\n# Tools\n`));
  assert.equal(prompt.split(FIXTURE.messages[0].content).length, 2);
});

test('assistant calls render in the model syntax and tool results group as the template does', () => {
  assert.ok(GOLDEN.includes('<|im_start|>assistant\n<think>\n\n</think>\n<tool_call>\n<function=run_command>\n'
    + '<parameter=command>\ncd p4bridge && node --test test/\n</parameter>\n<parameter=timeout_s>\n120\n</parameter>\n'
    + '</function>\n</tool_call><|im_end|>\n'));
  // Two results in a row share one tool_response turn, with no separator.
  assert.ok(GOLDEN.includes('<|im_start|>tool_response\n<tool_response>delivered</tool_response><tool_response># p4 bridge'));
  assert.equal((GOLDEN.match(/<\|im_start\|>tool_response/g) ?? []).length, 2);
  // Python's str() for a boolean, and tojson for a list: the template's own.
  assert.ok(GOLDEN.includes('<parameter=urgent>\nFalse\n</parameter>'));
  assert.ok(GOLDEN.includes('<parameter=lines>\n[1, 20]\n</parameter>'));
  assert.ok(!GOLDEN.includes('tool_call_id'));
});

test('tool_choice none leaves the tools out but keeps the history readable', () => {
  const prompt = promptFrom({ ...FIXTURE, tool_choice: 'none' }, 'chatml', true, false, 'step');
  assert.ok(!prompt.includes('# Tools'));
  assert.ok(!prompt.includes('<tools>'));
  assert.ok(prompt.startsWith(`<|im_start|>system\n${FIXTURE.messages[0].content}<|im_end|>\n`));
  assert.ok(prompt.includes('<function=run_command>'));
  assert.ok(prompt.includes('<|im_start|>tool_response\n'));
});

test('a forced function is asked for in words', () => {
  const forced = { messages: [{ role: 'user', content: 'go' }], tools: TOOLS, tool_choice: { type: 'function', function: { name: 'read_file' } } };
  assert.ok(promptFrom(forced, 'chatml', true, true, 'step').includes('</IMPORTANT>\n\nYou MUST call the function `read_file` in this reply.<|im_end|>'));
  const required = { ...forced, tool_choice: 'required' };
  assert.ok(promptFrom(required, 'chatml', true, true, 'step').includes('You MUST call at least one of the functions above'));
  const auto = { ...forced, tool_choice: 'auto' };
  assert.ok(promptFrom(auto, 'chatml', true, true, 'step').includes('</IMPORTANT><|im_end|>'));
});

test('a model without a tool format ignores tools, as before', () => {
  const body = { messages: [{ role: 'user', content: 'hi' }], tools: TOOLS };
  assert.equal(promptFrom(body, 'chatml', false, false, 'none'), '<|im_start|>user\nhi<|im_end|>\n<|im_start|>assistant\n');
});

test('the catalog picks the Step format for step-3.7-flash and refuses an unknown one', () => {
  assert.equal(toolcalls.toolFormatFor({ id: 'step-3.7-flash' }), 'step');
  assert.equal(toolcalls.toolFormatFor({ id: 'qwen2.5-7b' }), 'none');
  assert.equal(toolcalls.toolFormatFor({ id: 'qwen2.5-7b', tool_format: 'step' }), 'step');
  assert.equal(toolcalls.toolFormatFor({ id: 'step-3.7-flash', tool_format: 'none' }), 'none');
  assert.throws(() => toolcalls.toolFormatFor({ id: 'x', tool_format: 'hermes' }), /unknown tool_format/);
});

/* ---- parsing ------------------------------------------------------------ */

test('a reply with a call parses into OpenAI tool_calls', () => {
  const parsed = toolcalls.parseToolCalls(`I will look.\n\n${CALL}`, TOOLS);
  assert.equal(parsed.content, 'I will look.');
  assert.equal(parsed.toolCalls.length, 1);
  const [call] = parsed.toolCalls;
  assert.match(call.id, /^call_[0-9a-f]{24}$/);
  assert.equal(call.type, 'function');
  assert.equal(call.function.name, 'run_command');
  assert.equal(typeof call.function.arguments, 'string');
  assert.deepEqual(JSON.parse(call.function.arguments), { command: 'ls -la', timeout_s: 30 });
});

test('a reply that is only a call has null content', () => {
  const parsed = toolcalls.parseToolCalls(CALL, TOOLS);
  assert.equal(parsed.content, null);
  assert.equal(parsed.toolCalls.length, 1);
});

test('several calls in one reply keep their order and get distinct ids', () => {
  const second = '<tool_call>\n<function=send_message>\n<parameter=to>\ncoordinator\n</parameter>\n'
    + '<parameter=text>\nline one\nline two\n</parameter>\n<parameter=urgent>\ntrue\n</parameter>\n</function>\n</tool_call>';
  const parsed = toolcalls.parseToolCalls(`${CALL}\n${second}`, TOOLS);
  assert.equal(parsed.content, null);
  assert.deepEqual(parsed.toolCalls.map((c) => c.function.name), ['run_command', 'send_message']);
  assert.notEqual(parsed.toolCalls[0].id, parsed.toolCalls[1].id);
  assert.deepEqual(JSON.parse(parsed.toolCalls[1].function.arguments), { to: 'coordinator', text: 'line one\nline two', urgent: true });
});

test('values follow the schema: a string stays a string, others parse', () => {
  const reply = '<tool_call>\n<function=read_file>\n<parameter=path>\n123\n</parameter>\n'
    + '<parameter=lines>\n[3, 9]\n</parameter>\n</function>\n</tool_call>';
  const [call] = toolcalls.parseToolCalls(reply, TOOLS).toolCalls;
  assert.deepEqual(JSON.parse(call.function.arguments), { path: '123', lines: [3, 9] });
  // Python spellings, as the template itself writes them back.
  assert.equal(toolcalls.coerce('False', ['boolean']), false);
  assert.equal(toolcalls.coerce('None', null), null);
  // No schema: JSON when it parses, text when it does not.
  assert.deepEqual(toolcalls.coerce('{"a": 1}', null), { a: 1 });
  assert.equal(toolcalls.coerce('hello world', null), 'hello world');
  // A schema the value does not fit: the value is kept, not dropped.
  assert.equal(toolcalls.coerce('soon', ['integer']), 'soon');
});

test('a missing closing </tool_call> at the very end is tolerated', () => {
  const parsed = toolcalls.parseToolCalls(CALL.replace(/<\/tool_call>$/, ''), TOOLS);
  assert.equal(parsed.toolCalls.length, 1);
  assert.equal(parsed.content, null);
});

test('malformed calls come back as text and never throw', () => {
  const broken = [
    '<tool_call>\n<function=run_command>\n<parameter=command>\nls\n</parameter>\n',   // never closed
    '<tool_call>\n<function=>\n</function>\n</tool_call>',                           // no name
    '<tool_call>\n{"name": "run_command", "arguments": {}}\n</tool_call>',           // hermes JSON, not this format
    '<tool_call>\n<function=run_command>\nstray text\n</function>\n</tool_call>',    // text where parameters go
    'use <tool_call> tags like this',
    '<function=run_command',
    '',
  ];
  for (const text of broken) {
    const parsed = toolcalls.parseToolCalls(text, TOOLS);
    assert.deepEqual(parsed.toolCalls, [], text);
    assert.equal(parsed.content, text.trim() || null, text);
  }
});

test('a bare <function=…> block without its wrapper still counts', () => {
  const parsed = toolcalls.parseToolCalls('<function=run_command>\n<parameter=command>\npwd\n</parameter>\n</function>', TOOLS);
  assert.equal(parsed.toolCalls.length, 1);
  assert.deepEqual(JSON.parse(parsed.toolCalls[0].function.arguments), { command: 'pwd' });
});

/* ---- streaming ---------------------------------------------------------- */

function runStream(chunks, options) {
  const shaper = new toolcalls.StreamShaper(options);
  const deltas = [];
  for (const chunk of chunks) deltas.push(...shaper.push(chunk));
  deltas.push(...shaper.end());
  return {
    deltas,
    content: deltas.filter((d) => d.content !== undefined).map((d) => d.content).join(''),
    reasoning: deltas.filter((d) => d.reasoning_content !== undefined).map((d) => d.reasoning_content).join(''),
    calls: deltas.flatMap((d) => d.tool_calls ?? []),
    shaper,
  };
}

test('a stream passes text through and emits the call when it closes, opener split across chunks', () => {
  const chunks = ['Let me check.', '\n<to', 'ol_c', 'all>\n<function=run_', 'command>\n<parameter=command>\nls -la\n',
    '</parameter>\n<parameter=timeout_s>\n30\n</parameter>\n</function>\n</tool', '_call>'];
  const shaper = new toolcalls.StreamShaper({ tools: TOOLS, parseCalls: true });
  // Text before the opener is not held hostage by the call that follows it.
  assert.deepEqual(shaper.push(chunks[0]), [{ content: 'Let me check.' }]);
  assert.deepEqual(shaper.push(chunks[1]), [{ content: '\n' }]);
  const rest = runStream(chunks, { tools: TOOLS, parseCalls: true });
  assert.equal(rest.content, 'Let me check.\n');
  assert.equal(rest.calls.length, 1);
  const [call] = rest.calls;
  assert.equal(call.index, 0);
  assert.match(call.id, /^call_/);
  assert.equal(call.type, 'function');
  assert.equal(call.function.name, 'run_command');
  assert.deepEqual(JSON.parse(call.function.arguments), { command: 'ls -la', timeout_s: 30 });
  assert.ok(!rest.content.includes('<'));
});

test('a stream cut at every character agrees with the whole-reply parse', () => {
  const second = '<tool_call>\n<function=read_file>\n<parameter=path>\nREADME.md\n</parameter>\n</function>\n</tool_call>';
  const reply = `Checking two things.\n${CALL}\n${second}`;
  const whole = toolcalls.parseToolCalls(reply, TOOLS);
  const streamed = runStream([...reply], { tools: TOOLS, parseCalls: true });
  assert.equal(streamed.content.trim(), whole.content);
  assert.deepEqual(streamed.calls.map((c) => c.index), [0, 1]);
  assert.deepEqual(streamed.calls.map((c) => [c.function.name, c.function.arguments]),
    whole.toolCalls.map((c) => [c.function.name, c.function.arguments]));
  assert.equal(streamed.shaper.calls.length, 2);
});

test('a malformed call in a stream is released as text at the end', () => {
  const text = 'see <tool_call>\n<function=run_command>\n<parameter=command>\nls';
  const out = runStream([...text], { tools: TOOLS, parseCalls: true });
  assert.equal(out.content, text);
  assert.deepEqual(out.calls, []);
  const closed = 'x <tool_call>{"name": "a"}</tool_call> y';
  assert.equal(runStream([...closed], { tools: TOOLS, parseCalls: true }).content, closed);
});

test('without tools a stream is passed through untouched', () => {
  const out = runStream(['<tool', '_call>', 'hi'], { tools: [], parseCalls: false });
  assert.equal(out.content, '<tool_call>hi');
});

test('a stream with thinking open sends the thought as reasoning_content', () => {
  const out = runStream(['The user wants', ' a listing.</th', 'ink>\n\n', CALL], { tools: TOOLS, parseCalls: true, thinkingOpen: true });
  assert.equal(out.reasoning, 'The user wants a listing.');
  assert.equal(out.content, '');
  assert.equal(out.calls.length, 1);
  const plain = runStream(['hm', '</think>', '\n\nAnswer.'], { thinkingOpen: true });
  assert.equal(plain.reasoning, 'hm');
  assert.equal(plain.content, 'Answer.');
});

/* ---- finish reason and engine options ---------------------------------- */

test('the engine stop reasons map onto OpenAI', () => {
  assert.equal(toolcalls.finishReason('eos'), 'stop');
  assert.equal(toolcalls.finishReason('stop'), 'stop');
  assert.equal(toolcalls.finishReason('length'), 'length');
  assert.equal(toolcalls.finishReason(undefined), 'stop');
});

test('temperature and stop reach the engine options; nothing else changes them', () => {
  assert.equal(engineOptions('', {}), '');
  assert.equal(engineOptions('', { temperature: 0.2, stop: 'END' }), '{"temperature":0.2,"stop":["END"]}');
  assert.equal(engineOptions('{"top_k":20,"temperature":1}', { temperature: 0 }), '{"top_k":20,"temperature":0}');
  assert.equal(engineOptions('', { stop: ['a', '', 3] }), '{"stop":["a"]}');
  assert.equal(engineOptions('', { temperature: -1 }), '');
  assert.equal(engineOptions('not json', { temperature: 0 }), 'not json');
});

/* ---- over HTTP ---------------------------------------------------------- */

/** A bridge with one serving model and a pipeline that replays canned tokens. */
function fakeBridge(tokens, stop = 'eos') {
  const model = {
    id: 'step-3.7-flash', name: 'Step', maxTokens: 512, options: '', promptFormat: 'chatml',
    reasoning: true, toolFormat: 'step', stages: [],
  };
  const seen = [];
  return {
    seen,
    catalog: { models: [model] },
    serving: new Map([[model.id, { serving: true, stages: [] }]]),
    recordContribution() {},
    async pipelineFor() {
      return {
        async generate({ prompt, options, onToken }) {
          seen.push({ prompt, options });
          for (const text of tokens) onToken?.({ text });
          return { requestId: 'r', text: tokens.join(''), finishReason: stop, completionTokens: tokens.length, promptTokens: 7, stageRows: {} };
        },
      };
    },
  };
}

async function post(bridge, body) {
  const server = createServer(bridge);
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  try {
    const response = await fetch(`http://127.0.0.1:${server.address().port}/c/step-3.7-flash/v1/chat/completions`, {
      method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body),
    });
    return { status: response.status, text: await response.text() };
  } finally {
    server.close();
  }
}

const request = (extra) => ({
  messages: [{ role: 'user', content: 'list the files' }],
  tools: TOOLS,
  chat_template_kwargs: { enable_thinking: false },
  ...extra,
});

test('HTTP: a non-stream reply carries tool_calls and finish_reason tool_calls', async () => {
  const bridge = fakeBridge(['Sure.\n', CALL]);
  const { status, text } = await post(bridge, request({ temperature: 0.3, stop: ['<|im_end|>'] }));
  assert.equal(status, 200);
  const reply = JSON.parse(text);
  const { message, finish_reason: finish } = reply.choices[0];
  assert.equal(finish, 'tool_calls');
  assert.equal(message.content, 'Sure.');
  assert.equal(message.tool_calls[0].function.name, 'run_command');
  assert.ok(bridge.seen[0].prompt.includes('<tools>'));
  assert.equal(bridge.seen[0].options, '{"temperature":0.3,"stop":["<|im_end|>"]}');
});

test('HTTP: a plain reply maps eos to stop and has no tool_calls', async () => {
  const { text } = await post(fakeBridge(['Hello.'], 'eos'), request());
  const reply = JSON.parse(text);
  assert.equal(reply.choices[0].finish_reason, 'stop');
  assert.equal(reply.choices[0].message.content, 'Hello.');
  assert.equal(reply.choices[0].message.tool_calls, undefined);
  const long = JSON.parse((await post(fakeBridge(['Hel'], 'length'), request())).text);
  assert.equal(long.choices[0].finish_reason, 'length');
});

test('HTTP: tool_choice none returns call-shaped text as text', async () => {
  const bridge = fakeBridge([CALL]);
  const reply = JSON.parse((await post(bridge, request({ tool_choice: 'none' }))).text);
  assert.equal(reply.choices[0].finish_reason, 'stop');
  assert.equal(reply.choices[0].message.content, CALL);
  assert.ok(!bridge.seen[0].prompt.includes('<tools>'));
});

test('HTTP: a stream sends delta.tool_calls and finishes with tool_calls', async () => {
  const tokens = ['On it.', '<tool', '_call>\n<function=run_command>\n', '<parameter=command>\nls -la\n</parameter>\n', '</function>\n</tool_call>'];
  const { status, text } = await post(fakeBridge(tokens), request({ stream: true }));
  assert.equal(status, 200);
  const frames = text.split('\n\n').filter((line) => line.startsWith('data: ') && !line.includes('[DONE]'))
    .map((line) => JSON.parse(line.slice(6)));
  const deltas = frames.flatMap((frame) => frame.choices.map((choice) => choice.delta));
  const content = deltas.map((delta) => delta.content ?? '').join('');
  assert.equal(content, 'On it.');
  const calls = deltas.flatMap((delta) => delta.tool_calls ?? []);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].index, 0);
  assert.equal(calls[0].function.name, 'run_command');
  assert.deepEqual(JSON.parse(calls[0].function.arguments), { command: 'ls -la' });
  const finishes = frames.flatMap((frame) => frame.choices.map((choice) => choice.finish_reason)).filter(Boolean);
  assert.deepEqual(finishes, ['tool_calls']);
  assert.ok(frames.at(-1).usage);
});
