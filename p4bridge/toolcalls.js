'use strict';
/**
 * Tool calling, in the syntax the model was trained on.
 *
 * p4 applies no chat template, so the bridge renders one (see promptFrom in
 * server.js). Tools add three things to that job: declaring them in the system
 * turn, writing earlier calls and their results back into the history, and
 * reading the model's own calls out of its reply as OpenAI `tool_calls`.
 * None of it can be guessed per model, so the catalog names it: `tool_format`.
 *
 * `step` is the format in Step-3.7-Flash's GGUF `tokenizer.chat_template`
 * (the same XML shape Qwen3-Coder uses):
 *
 *   system turn   # Tools … <tools>\n{json}\n…</tools> … reminder <|im_end|>
 *   a call        <tool_call>\n<function=NAME>\n<parameter=KEY>\nVALUE\n</parameter>\n</function>\n</tool_call>
 *   results       <|im_start|>tool_response\n<tool_response>R1</tool_response><tool_response>R2</tool_response><|im_end|>
 *
 * The rendering below follows that template line by line; the comments name
 * the template branch each piece comes from. `none` leaves tools out entirely,
 * which is what the bridge did before this file existed.
 */
const crypto = require('node:crypto');

const FORMATS = new Set(['step', 'none']);

/** The catalog's tool format for a model, defaulting by what the model is. */
function toolFormatFor(entry) {
  if (entry.tool_format !== undefined) {
    const named = String(entry.tool_format);
    if (!FORMATS.has(named)) {
      throw new Error(`catalog model ${entry.id} names an unknown tool_format ${named} (known: ${[...FORMATS].join(', ')})`);
    }
    return named;
  }
  // The Step 3.x template is the only one read so far. Another chatml model
  // (Qwen2.5's hermes JSON calls, say) would be misled by it, so the default is
  // keyed on the model, not on the turn format.
  return /^step-3/i.test(String(entry.id)) ? 'step' : 'none';
}

/* ---- rendering ---------------------------------------------------------- */

/**
 * JSON the way the template's `tojson` writes it: Python's json.dumps with its
 * default separators, `", "` and `": "`. The tool block is part of the prompt
 * the model was trained on, so the spacing is kept rather than JSON.stringify's.
 */
function pyJson(value) {
  if (value === null || value === undefined) return 'null';
  if (Array.isArray(value)) return `[${value.map(pyJson).join(', ')}]`;
  if (typeof value === 'object') {
    return `{${Object.entries(value)
      .filter(([, v]) => v !== undefined)
      .map(([k, v]) => `${JSON.stringify(k)}: ${pyJson(v)}`).join(', ')}}`;
  }
  return JSON.stringify(value);
}

/** The template's render_message_content macro. */
function contentOf(message) {
  const content = message?.content;
  if (content === null || content === undefined) return '';
  if (typeof content === 'string') return content;
  if (Array.isArray(content)) {
    let out = '';
    let separate = false;
    for (const item of content) {
      if (item?.type === 'text') {
        if (separate) out += ' ';
        out += item.value ?? item.text ?? '';
        separate = true;
      } else if (item?.type === 'image' || item?.type === 'image_url') {
        out += '<im_patch>';
        separate = false;
      }
    }
    return out;
  }
  if (typeof content === 'object') return String(content.value ?? content.text ?? '');
  return String(content);
}

/** `args_value | string` in Jinja is Python's str(): True, False, None. */
function pyStr(value) {
  if (value === true) return 'True';
  if (value === false) return 'False';
  if (value === null) return 'None';
  return String(value);
}

/** One assistant call in the template's syntax. */
function renderCall(call) {
  const fn = call?.function ?? call ?? {};
  let out = `<tool_call>\n<function=${fn.name ?? ''}>\n`;
  let args = fn.arguments;
  if (typeof args === 'string') {
    // The template runs `fromjson` here and would fail outright on bad JSON.
    // A caller replaying a model's own malformed call should not lose the turn,
    // so the parameters are left out instead.
    try { args = args.trim() ? JSON.parse(args) : {}; } catch { args = {}; }
  }
  if (args && typeof args === 'object' && !Array.isArray(args)) {
    for (const [key, value] of Object.entries(args)) {
      const text = value !== null && typeof value === 'object' ? pyJson(value) : pyStr(value);
      out += `<parameter=${key}>\n${text}\n</parameter>\n`;
    }
  }
  return `${out}</function>\n</tool_call>`;
}

/** The function list a request offers the model, after tool_choice. */
function activeTools(body) {
  if (!Array.isArray(body?.tools) || body.tools.length === 0) return [];
  if (body.tool_choice === 'none') return [];
  return body.tools.filter((tool) => tool && (tool.function?.name || tool.name));
}

/**
 * A forced choice has no switch in the template, so it is said in words. The
 * engine samples freely either way; this is best effort, not a guarantee.
 */
function choiceLine(choice) {
  if (choice === 'required') {
    return '\n\nYou MUST call at least one of the functions above in this reply.';
  }
  const name = choice?.function?.name ?? (choice?.type === 'function' ? choice.name : null);
  if (name) return `\n\nYou MUST call the function \`${name}\` in this reply.`;
  return '';
}

function toolsBlock(tools, choice) {
  const lines = tools.map((tool) => `\n${pyJson(tool)}`).join('');
  return '# Tools\n\nYou have access to the following functions in JSONSchema format:\n\n<tools>'
    + lines
    + '\n</tools>\n\nIf you choose to call a function ONLY reply in the following format with NO suffix:\n\n'
    + '<tool_call>\n<function=example_function_name>\n<parameter=example_parameter_1>\nvalue_1\n</parameter>\n'
    + '<parameter=example_parameter_2>\nThis is the value for the second parameter\nthat can span\nmultiple lines\n'
    + '</parameter>\n</function>\n</tool_call>\n\n<IMPORTANT>\nReminder:\n'
    + '- Function calls MUST follow the specified format: an inner <function=...>\n...\n</function> block must be '
    + 'nested within <tool_call>\n...\n</tool_call> XML tags\n- Required parameters MUST be specified\n</IMPORTANT>'
    + choiceLine(choice);
}

const isToolResult = (text) => text.startsWith('<tool_response>') && text.endsWith('</tool_response>');

/**
 * The Step template, rendered. `generation` is what opens the reply: the
 * template ends `<|im_start|>assistant\n<think>\n`, and the bridge closes the
 * block there instead when thinking is off (see promptFrom).
 */
function renderStep(body, generation) {
  const messages = (Array.isArray(body.messages) ? body.messages : []).map((message) => {
    // OpenAI's newer names for the same two roles.
    if (message?.role === 'developer') return { ...message, role: 'system' };
    if (message?.role === 'function') return { ...message, role: 'tool' };
    return message ?? {};
  });
  const tools = activeTools(body);
  let out = '';

  // {%- if tools %} … the first system message is folded into the tool turn.
  if (tools.length) {
    out += '<|im_start|>system\n';
    if (messages[0]?.role === 'system') out += `${contentOf(messages[0])}\n\n`;
    out += `${toolsBlock(tools, body.tool_choice)}<|im_end|>\n`;
  } else if (messages[0]?.role === 'system') {
    out += `<|im_start|>system\n${contentOf(messages[0])}<|im_end|>\n`;
  }

  // ns.last_query_index: the last user turn that is not a wrapped tool result.
  let lastQuery = messages.length - 1;
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index];
    if (message.role === 'user' && !isToolResult(contentOf(message))) { lastQuery = index; break; }
  }

  messages.forEach((message, index) => {
    let content = contentOf(message);
    const role = message.role ?? 'user';
    if (role === 'user' || (role === 'system' && index > 0)) {
      const name = role === 'system' && message.name === 'observation' ? 'observation' : role;
      out += `<|im_start|>${name}\n${content}<|im_end|>\n`;
    } else if (role === 'assistant') {
      let reasoning = '';
      if (typeof message.reasoning_content === 'string') {
        reasoning = message.reasoning_content;
      } else if (content.includes('</think>')) {
        const [before] = content.split('</think>');
        reasoning = before.replace(/\n+$/, '').split('<think>').pop().replace(/^\n+/, '');
        content = content.split('</think>').pop().replace(/^\n+/, '');
      }
      out += index > lastQuery
        ? `<|im_start|>assistant\n<think>\n${reasoning}\n</think>\n${content}`
        : `<|im_start|>assistant\n${content}`;
      for (const call of Array.isArray(message.tool_calls) ? message.tool_calls : []) out += renderCall(call);
      out += '<|im_end|>\n';
    } else if (role === 'tool') {
      if (index === 0 || messages[index - 1].role !== 'tool') out += '<|im_start|>tool_response\n';
      out += `<tool_response>${content}</tool_response>`;
      if (index === messages.length - 1 || messages[index + 1].role !== 'tool') out += '<|im_end|>\n';
    }
    // Any other role is dropped, as the template drops it.
  });

  return `${out}<|im_start|>assistant\n${generation}`;
}

/* ---- parsing ------------------------------------------------------------ */

const OPENERS = ['<tool_call>', '<function='];
const CALL_BLOCK = /<tool_call>\s*([\s\S]*?)\s*<\/tool_call>/y;

const callId = () => `call_${crypto.randomBytes(12).toString('hex')}`;

function schemaTypes(tools, name, key) {
  const tool = tools.find((t) => (t.function?.name ?? t.name) === name);
  const prop = (tool?.function?.parameters ?? tool?.parameters)?.properties?.[key];
  if (!prop) return null;
  if (Array.isArray(prop.type)) return prop.type;
  if (typeof prop.type === 'string') return [prop.type];
  if (prop.enum) return ['string'];
  return null;
}

const typeOf = (value) => {
  if (value === null) return 'null';
  if (Array.isArray(value)) return 'array';
  if (typeof value === 'number') return Number.isInteger(value) ? 'integer' : 'number';
  return typeof value;     // string, boolean, object
};

/**
 * A parameter value is text in this syntax; JSON wants a type. The schema
 * decides when there is one. Without it: JSON when the text parses, a string
 * when it does not.
 */
function coerce(raw, types) {
  const pythonic = { True: true, False: false, None: null };
  let parsed;
  let parses = false;
  try { parsed = JSON.parse(raw); parses = true; } catch {
    if (raw in pythonic) { parsed = pythonic[raw]; parses = true; }
  }
  if (!types) return parses ? parsed : raw;
  if (parses) {
    // A string parameter keeps its text verbatim, quotes and all: `"x"` written
    // into a string slot is more likely meant literally than as JSON.
    const kind = typeOf(parsed);
    if (kind !== 'string' && (types.includes(kind) || (kind === 'integer' && types.includes('number')))) return parsed;
  }
  if (types.includes('string')) return raw;
  return parses ? parsed : raw;
}

/** The inside of one call: `<function=NAME> … </function>`. Null when malformed. */
function parseFunction(inner, tools) {
  const head = /^\s*<function=([^>\n]+)>/.exec(inner);
  if (!head) return null;
  const name = head[1].trim();
  if (!name) return null;
  const close = inner.lastIndexOf('</function>');
  const body = inner.slice(head[0].length, close === -1 ? inner.length : close);
  if (close === -1) return null;
  const args = {};
  const param = /<parameter=([^>\n]+)>\n?([\s\S]*?)\n?<\/parameter>/g;
  let match;
  let consumed = 0;
  while ((match = param.exec(body))) {
    if (body.slice(consumed, match.index).trim()) return null;   // stray text between parameters
    const key = match[1].trim();
    args[key] = coerce(match[2], schemaTypes(tools, name, key));
    consumed = param.lastIndex;
  }
  if (body.slice(consumed).trim()) return null;
  return { id: callId(), type: 'function', function: { name, arguments: JSON.stringify(args) } };
}

/**
 * Read one call starting at `text[at]` (which is an opener). Returns
 * `{ call, end }`, or null when the text there is not a well-formed call.
 * A final `</tool_call>` is optional at the very end of the reply: the model
 * stops on its end-of-turn token, and a missing closing tag there is a
 * sampling slip, not a different meaning.
 */
function readCall(text, at, tools, final) {
  if (text.startsWith('<tool_call>', at)) {
    CALL_BLOCK.lastIndex = at;
    const block = CALL_BLOCK.exec(text);
    if (block) {
      const call = parseFunction(block[1], tools);
      return call ? { call, end: CALL_BLOCK.lastIndex } : null;
    }
    if (!final) return null;
    const call = parseFunction(text.slice(at + '<tool_call>'.length), tools);
    return call ? { call, end: text.length } : null;
  }
  // A bare <function=…> with no wrapper: accepted, as vLLM's parser for this
  // family does, because the model drops the wrapper now and then.
  const close = text.indexOf('</function>', at);
  if (close === -1) return null;
  const end = close + '</function>'.length;
  const call = parseFunction(text.slice(at, end), tools);
  return call ? { call, end } : null;
}

function nextOpener(text, from) {
  let best = -1;
  for (const opener of OPENERS) {
    const at = text.indexOf(opener, from);
    if (at !== -1 && (best === -1 || at < best)) best = at;
  }
  return best;
}

/**
 * Split a whole reply into text and calls. Anything that does not parse
 * stays in `content`, so a malformed call reaches the caller as text instead
 * of vanishing or failing the request.
 */
function parseToolCalls(text, tools = []) {
  const calls = [];
  let content = '';
  let cursor = 0;
  for (;;) {
    const at = nextOpener(text, cursor);
    if (at === -1) { content += text.slice(cursor); break; }
    const read = readCall(text, at, tools, true);
    if (!read) {
      // Not a call after all: keep the opener as text and look past it.
      content += text.slice(cursor, at + 1);
      cursor = at + 1;
      continue;
    }
    content += text.slice(cursor, at);
    calls.push(read.call);
    cursor = read.end;
  }
  const trimmed = content.trim();
  return { content: trimmed ? trimmed : null, toolCalls: calls };
}

/* ---- streaming ---------------------------------------------------------- */

/** Length of the longest suffix of `text` that begins one of `tags`. */
function partialTail(text, tags) {
  let keep = 0;
  for (const tag of tags) {
    for (let n = Math.min(tag.length - 1, text.length); n > keep; n -= 1) {
      if (text.endsWith(tag.slice(0, n))) { keep = n; break; }
    }
  }
  return keep;
}

/**
 * Shapes a token stream into OpenAI deltas: reasoning while the think block is
 * open, then content, then calls. Text is passed through as it comes until a
 * call opener appears — held back only as long as it could still be the start
 * of one (an opener split across tokens) — and each call is emitted whole once
 * its closing tag has arrived.
 *
 * push() and end() return a list of deltas to send, in order.
 */
class StreamShaper {
  constructor({ tools = [], parseCalls = false, thinkingOpen = false } = {}) {
    this.tools = tools;
    this.parseCalls = parseCalls;
    this.inThink = thinkingOpen;
    this.afterThink = false;      // strip the whitespace the template puts after </think>
    this.pending = '';            // text not yet classified
    this.capturing = false;       // inside something that began with an opener
    this.calls = [];
  }

  push(text) {
    this.pending += text;
    return this.drain(false);
  }

  end() {
    return this.drain(true);
  }

  drain(final) {
    const out = [];
    const content = (text) => {
      if (this.afterThink) {
        text = text.replace(/^\s+/, '');
        if (!text) return;
        this.afterThink = false;
      }
      if (!text) return;
      // Whitespace between calls is formatting, not something to show.
      if (this.calls.length && !text.trim()) return;
      const last = out[out.length - 1];
      if (last && last.content !== undefined) last.content += text; else out.push({ content: text });
    };

    if (this.inThink) {
      const close = this.pending.indexOf('</think>');
      if (close === -1) {
        const keep = final ? 0 : partialTail(this.pending, ['</think>']);
        const thought = this.pending.slice(0, this.pending.length - keep);
        if (thought) out.push({ reasoning_content: thought });
        this.pending = this.pending.slice(this.pending.length - keep);
        return out;
      }
      const thought = this.pending.slice(0, close);
      if (thought) out.push({ reasoning_content: thought });
      this.pending = this.pending.slice(close + '</think>'.length);
      this.inThink = false;
      this.afterThink = true;
    }

    if (!this.parseCalls) {
      content(this.pending);
      this.pending = '';
      return out;
    }

    for (;;) {
      if (!this.capturing) {
        const at = nextOpener(this.pending, 0);
        if (at === -1) {
          const keep = final ? 0 : partialTail(this.pending, OPENERS);
          content(this.pending.slice(0, this.pending.length - keep));
          this.pending = this.pending.slice(this.pending.length - keep);
          return out;
        }
        content(this.pending.slice(0, at));
        this.pending = this.pending.slice(at);
        this.capturing = true;
      }
      // pending starts with an opener.
      const read = readCall(this.pending, 0, this.tools, final);
      if (read) {
        const index = this.calls.length;
        this.calls.push(read.call);
        out.push({ tool_calls: [{ index, ...read.call }] });
        this.pending = this.pending.slice(read.end);
        this.capturing = false;
        continue;
      }
      if (!final && !this.hopeless()) return out;
      // Not a call after all: release the opener's first character as text
      // and keep scanning what follows.
      content(this.pending.slice(0, 1));
      this.pending = this.pending.slice(1);
      this.capturing = false;
      if (!this.pending) return out;
    }
  }

  /** True when buffered text can no longer turn into a call. */
  hopeless() {
    const p = this.pending;
    if (p.startsWith('<tool_call>')) {
      // A complete block that failed to parse is malformed for good.
      return p.includes('</tool_call>');
    }
    if (p.startsWith('<function=')) {
      const head = /^<function=([^>\n]*)(>|\n)/.exec(p);
      if (head && (head[2] === '\n' || !head[1].trim())) return true;
      return p.includes('</function>');
    }
    return true;
  }
}

/** The engine's stop reasons in OpenAI's vocabulary. */
function finishReason(engine) {
  if (engine === 'length') return 'length';
  return 'stop';      // eos, stop (a stop string matched), and anything unexpected
}

module.exports = {
  FORMATS,
  toolFormatFor,
  activeTools,
  renderStep,
  renderCall,
  pyJson,
  contentOf,
  parseToolCalls,
  StreamShaper,
  finishReason,
  coerce,
};
