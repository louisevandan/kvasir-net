// P4 Architecture and Design — pptx generator for porting to Google Slides
// Ports the HTML deck (1280x720 px) 1:1 onto 13.333in x 7.5in. 1px = 1/96in.
const P = require('pptxgenjs');

const pres = new P();
pres.defineLayout({ name: 'P4', width: 13.3333, height: 7.5 });
pres.layout = 'P4';
pres.author = 'P4';
pres.title = 'P4 Architecture and Design';

const I = (p) => p / 96;

// ── palette ──────────────────────────────────────────────
const INK = '151A20', INK2 = '39424D', MUT = '68737F';
const RULE = 'D2D8DC', RULE2 = 'BFC7CD';
const SURF = 'FBFCFC', SURF2 = 'F1F4F5', SURF3 = 'E4EAEC';
const FLOW = '16697A', FLOWW = 'E0EDF0';
const BND = 'B05E1D', BNDW = 'F7EADB';
const BLK = '9E2B3A', BLKW = 'F6E3E5';
const DARK = '141920', DARKW = 'E9EDF1';

const F = 'Noto Sans KR';
const M = 'Roboto Mono';

// ── primitives ───────────────────────────────────────────
function box(s, x, y, w, h, o = {}) {
  s.addShape(o.round ? 'roundRect' : 'rect', {
    x: I(x), y: I(y), w: I(w), h: I(h),
    fill: o.fill ? { color: o.fill } : { type: 'none' },
    line: o.line === null ? { type: 'none' }
      : { color: o.line || RULE2, width: o.lw || 1, dashType: o.dash ? 'dash' : 'solid' },
    ...(o.round ? { rectRadius: 0.02 } : {}),
  });
}
function T(s, str, x, y, w, h, o = {}) {
  s.addText(str, {
    x: I(x), y: I(y), w: I(w), h: I(h),
    fontFace: o.mono ? M : F,
    fontSize: o.size || 10.5,
    color: o.color || INK2,
    bold: !!o.bold,
    align: o.align || 'left',
    valign: o.valign || 'top',
    charSpacing: o.cs,
    lineSpacingMultiple: o.ls || 1.15,
    margin: 0,
    isTextBox: true,
    wrap: o.wrap !== false,
  });
}
function line(s, x1, y1, x2, y2, o = {}) {
  const x = Math.min(x1, x2), y = Math.min(y1, y2);
  const w = Math.abs(x2 - x1), h = Math.abs(y2 - y1);
  s.addShape('line', {
    x: I(x), y: I(y), w: I(w), h: I(h),
    flipH: x2 < x1, flipV: y2 < y1,
    line: {
      color: o.color || MUT, width: o.lw || 1,
      dashType: o.dash ? 'dash' : 'solid',
      ...(o.arrow === false ? {} : { endArrowType: 'triangle' }),
    },
  });
}
function rule(s, x, y, w, color = RULE) {
  s.addShape('line', { x: I(x), y: I(y), w: I(w), h: 0, line: { color, width: 0.75 } });
}
function bullets(s, items, x, y, w, h, o = {}) {
  const runs = items.map((t, i) => ({
    text: t,
    options: { bullet: { code: '2013' }, breakLine: i < items.length - 1, paraSpaceAfter: o.gap || 5 },
  }));
  s.addText(runs, {
    x: I(x), y: I(y), w: I(w), h: I(h),
    fontFace: F, fontSize: o.size || 10.5, color: o.color || INK2,
    lineSpacingMultiple: 1.2, margin: 0, isTextBox: true, valign: 'top',
  });
}
function table(s, rows, x, y, w, colW, o = {}) {
  s.addTable(rows, {
    x: I(x), y: I(y), w: I(w),
    colW: colW.map(I),
    fontFace: F, fontSize: o.size || 9.5, color: INK2,
    valign: 'top',
    margin: [4, 6, 5, 0],
    border: [{ type: 'none' }, { type: 'none' }, { pt: 0.5, color: RULE }, { type: 'none' }],
    autoPage: false,
    ...(o.rowH ? { rowH: I(o.rowH) } : {}),
  });
}
function note(s, x, y, w, h, str, kind = 'b') {
  const c = kind === 'x' ? BLK : kind === 'f' ? FLOW : BND;
  const bg = kind === 'x' ? BLKW : kind === 'f' ? FLOWW : BNDW;
  box(s, x, y, w, h, { fill: bg, line: null });
  s.addShape('rect', { x: I(x), y: I(y), w: I(2), h: I(h), fill: { color: c }, line: { type: 'none' } });
  T(s, str, x + 14, y + 10, w - 28, h - 18, { size: 9.5, color: INK2, ls: 1.25 });
}
function h3(s, str, x, y, w, color = MUT) {
  T(s, str, x, y, w, 14, { size: 8, color, mono: true, cs: 1.2, bold: true });
}

// ── slide chrome ─────────────────────────────────────────
function slide(sect, title, num, lede) {
  const s = pres.addSlide();
  s.background = { color: SURF };
  s.addText(
    [
      { text: sect, options: { color: FLOW } },
      { text: '   ·   ' + title.eyebrow, options: { color: MUT } },
    ],
    { x: I(54), y: I(36), w: I(900), h: I(14), fontFace: M, fontSize: 8, bold: true, cs: 1.4, margin: 0, isTextBox: true, valign: 'middle' }
  );
  T(s, `${String(num).padStart(2, '0')} / 16`, 1026, 36, 200, 14,
    { size: 8, color: MUT, mono: true, align: 'right', cs: 1, valign: 'middle' });
  T(s, title.h, 54, 56, 1172, title.two ? 80 : 44, { size: 25, color: INK, bold: true, ls: 1.15 });
  let top = title.two ? 142 : 106;
  if (lede) {
    T(s, lede, 54, top, 1130, 42, { size: 10.5, color: INK2, ls: 1.3 });
    top += 54;
  }
  s.bodyTop = top;
  return s;
}
function foot(s, str) {
  rule(s, 54, 650, 1172);
  T(s, str, 54, 658, 1172, 32, { size: 7.5, color: MUT, mono: false, ls: 1.3 });
}

/* ════════════════════════════════════════════════════════
   01 — Cover
   ════════════════════════════════════════════════════════ */
{
  const s = pres.addSlide();
  s.background = { color: DARK };
  T(s, 'DISTRIBUTED INFERENCE TRANSPORT   ·   2026-09-11', 60, 56, 900, 16,
    { size: 8.5, color: '5FB5C9', mono: true, cs: 1.8, bold: true });
  s.addShape('rect', { x: I(60), y: I(132), w: I(74), h: I(3), fill: { color: '5FB5C9' }, line: { type: 'none' } });
  T(s, 'A communication layer: one answer\nfrom a model split across machines', 60, 158, 1000, 130,
    { size: 40, color: 'F2F5F8', bold: true, ls: 1.14 });
  T(s, 'P4 does not run the model or own throughput. It carries work to the backend and returns the answer,\nand lets you name where things slowed down by attribution, not argument.',
    60, 308, 860, 60, { size: 12, color: 'AEBAC6', ls: 1.5 });

  const mx = [60, 358, 656, 954], mw = 268;
  const metrics = [
    ['108,815', 'RUST LINES · 12 CRATES'],
    ['14,597', 'C++ STAGE RUNTIME LINES'],
    ['9', 'LLAMA.CPP COMPAT PINS'],
    ['1384 / 0 / 7', 'WORKSPACE PASS/FAIL/IGNORED'],
  ];
  metrics.forEach(([v, l], i) => {
    box(s, mx[i], 430, mw, 96, { fill: '1D242C', line: null });
    T(s, v, mx[i] + 18, 452, mw - 36, 32, { size: 19, color: 'F2F5F8', mono: true, bold: true });
    T(s, l, mx[i] + 18, 492, mw - 36, 16, { size: 7.5, color: '8B97A3', mono: true, cs: 1 });
  });
  rule(s, 60, 590, 1160, '2B333B');
  T(s, 'Baseline HEAD 429e057de (2026-09-11) · code figures measured with wc -l (tests included); gate figures quoted from docs/distributed-batching-roadmap.md §0\nAll performance figures are screening results, not approval',
    60, 604, 1160, 44, { size: 8, color: '8B97A3', ls: 1.4 });
}

/* ════════════════════════════════════════════════════════
   02 — What it is · what it is not
   ════════════════════════════════════════════════════════ */
{
  const s = slide('Premise', { eyebrow: 'What it is · what it is not', h: 'The key to this layer\'s claim is that it is “verifiable”' }, 2,
    'It is not general-purpose messaging. It knows its workload in detail and is cut to that shape. Everything else is deliberately absent.');
  const y0 = s.bodyTop;

  h3(s, 'WHAT IT IS', 54, y0, 380, FLOW);
  bullets(s, [
    'A model split across several nodes by layer range',
    'A prefill chain source-routed through the nodes',
    'A decode ring where one token takes one lap',
    'Cohort batching under a declared ceiling',
  ], 54, y0 + 20, 380, 110, { size: 10 });

  h3(s, 'WHAT IT IS NOT', 54, y0 + 148, 380, BLK);
  bullets(s, [
    'Generic messaging — nothing the workload does not use',
    'The owner of throughput — it does not run the model',
    'TLS, authn, authz — none. Trusted networks only',
    'Durable state — a restarted agent has no nodes',
  ], 54, y0 + 168, 380, 110, { size: 10 });

  note(s, 470, y0, 756, 78,
    'Claim: problems in the running system do not belong to this layer. — That this claim is testable is the point of the design. ' +
    'Every stage runs against a mock backend that only does arithmetic (no device, no runtime, nothing underneath to blame), and the agent reports its lane depths side by side with node depth.', 'f');

  // attribution diagram
  const dy = y0 + 100;
  box(s, 470, dy, 300, 176, { fill: SURF2 });
  T(s, 'agent', 486, dy + 14, 200, 14, { size: 9, color: FLOW, mono: true, bold: true });
  T(s, 'lane depth', 486, dy + 32, 200, 12, { size: 7.5, color: MUT, mono: true });
  const lanes = [['control', 118], ['response', 62], ['decode', 30], ['prefill', 12]];
  lanes.forEach(([n, v], i) => {
    const yy = dy + 50 + i * 24;
    box(s, 486, yy, 160, 14, { fill: SURF3, line: null });
    box(s, 486, yy, v, 14, { fill: FLOW, line: null });
    T(s, n, 654, yy - 1, 110, 14, { size: 7.5, color: MUT, mono: true });
  });
  line(s, 782, dy + 88, 826, dy + 88, { color: MUT });
  T(s, 'hop', 782, dy + 68, 46, 12, { size: 7.5, color: MUT, mono: true, align: 'center' });
  box(s, 830, dy, 396, 176, { fill: SURF2 });
  T(s, 'node', 846, dy + 14, 200, 14, { size: 9, color: FLOW, mono: true, bold: true });
  T(s, 'depth · running · waiting', 846, dy + 32, 300, 12, { size: 7.5, color: MUT, mono: true });
  box(s, 846, dy + 52, 360, 36, { fill: FLOWW, line: FLOW, lw: 1.2 });
  T(s, 'hop = time inside the backend', 846, dy + 63, 360, 16, { size: 9, color: INK, align: 'center' });
  T(s, 'running: sequences in the adapter right now\n= the ceiling being held, observable', 846, dy + 100, 360, 40, { size: 9, color: MUT, ls: 1.35 });
  line(s, 800, dy - 8, 800, dy + 184, { color: BND, dash: true, arrow: false, lw: 1.2 });
  T(s, 'adapter boundary', 700, dy + 190, 200, 14, { size: 7.5, color: BND, mono: true, align: 'center' });

  foot(s, 'Sources docs/overview.md, docs/constraints.md, layers/agent/src/agent(NodeStatus) · when running was a bool, it was equally true at 1 and at 100');
}

/* ════════════════════════════════════════════════════════
   03 — Topology
   ════════════════════════════════════════════════════════ */
{
  const s = slide('Topology', { eyebrow: 'There is one process type', h: 'There is no controller. The node lives inside the agent' }, 3,
    'The path an agent takes to reach another agent is the same path it uses to answer the outside world. There is no separate, special entry process.');
  const y0 = s.bodyTop;

  box(s, 54, y0 + 48, 104, 52, { fill: BNDW, line: BND, lw: 1.2 });
  T(s, 'OUTER', 54, y0 + 58, 104, 16, { size: 10, color: BND, mono: true, align: 'center', bold: true });
  T(s, 'layout authority', 54, y0 + 78, 104, 14, { size: 7.5, color: MUT, align: 'center' });
  line(s, 162, y0 + 74, 206, y0 + 74, { color: FLOW, lw: 1.4 });

  box(s, 208, y0 + 42, 120, 64, { fill: FLOWW, line: FLOW, lw: 1.2 });
  T(s, 'entry agent', 208, y0 + 56, 120, 16, { size: 10, color: INK, mono: true, align: 'center' });
  T(s, '= agent entry point', 208, y0 + 76, 120, 14, { size: 7.5, color: MUT, align: 'center' });

  const rows = [y0 + 0, y0 + 72, y0 + 144];
  rows.forEach((ry) => line(s, 330, y0 + 74, 368, ry + 24, { color: FLOW, lw: 1.4 }));
  T(s, 'same path', 300, y0 + 182, 100, 12, { size: 7.5, color: FLOW, mono: true, align: 'center' });

  const cols = [
    { x: 370, w: 96, label: 'agent', fill: SURF2, line: RULE2 },
    { x: 494, w: 96, label: 'node', fill: SURF2, line: RULE2 },
    { x: 618, w: 110, label: 'adapter', fill: SURF2, line: RULE2 },
    { x: 756, w: 112, label: 'backend', fill: BNDW, line: BND },
  ];
  rows.forEach((ry) => {
    cols.forEach((c, ci) => {
      box(s, c.x, ry, c.w, 48, { fill: c.fill, line: c.line });
      T(s, c.label, c.x, ry + 16, c.w, 16, { size: 9, color: ci === 3 ? BND : INK, mono: true, align: 'center' });
      if (ci > 0) line(s, cols[ci - 1].x + cols[ci - 1].w + 2, ry + 24, c.x - 2, ry + 24, { color: MUT });
    });
  });
  box(s, 490, y0 - 12, 386, 216, { line: RULE2, dash: true });
  T(s, 'The node is the only entity inside an agent — no node process', 490, y0 + 208, 386, 14,
    { size: 8, color: MUT, align: 'center' });

  line(s, 906, y0 - 6, 906, y0 + 196, { color: RULE, arrow: false, lw: 0.75 });
  T(s, 'There is one axis of generalization — who owns the boundary', 928, y0, 298, 30, { size: 9, color: FLOW, mono: true, bold: true, ls: 1.3 });
  rule(s, 928, y0 + 40, 298);
  T(s, 'llama.cpp pipeline', 928, y0 + 50, 200, 14, { size: 9, color: INK, mono: true });
  T(s, 'chain n', 1026, y0 + 50, 200, 14, { size: 9, color: FLOW, mono: true, align: 'right' });
  T(s, 'P4 owns the boundaries between pieces', 928, y0 + 68, 298, 14, { size: 8, color: MUT });
  rule(s, 928, y0 + 90, 298);
  T(s, 'vLLM · SGLang', 928, y0 + 100, 200, 14, { size: 9, color: INK, mono: true });
  T(s, 'chain 1', 1026, y0 + 100, 200, 14, { size: 9, color: FLOW, mono: true, align: 'right' });
  T(s, 'the backend lays out the model itself', 928, y0 + 118, 298, 14, { size: 8, color: MUT });
  rule(s, 928, y0 + 140, 298);
  T(s, 'added cost = one name + one Adapter implementation', 928, y0 + 152, 298, 28, { size: 8, color: MUT, ls: 1.3 });

  const cy = y0 + 236;
  const defs = [
    ['OUTER', 'The requesting side. It owns placement, load plans, chains and every business identifier. The agent only reports machine facts and never turns them into placement.'],
    ['AGENT', 'A socket program with one main queue and a worker pool. Its only internal entity is the node.'],
    ['NODE · ADAPTER', 'A node is just an id until a load attaches an adapter. It holds its own queue and its own long work. The adapter receives hops and reports events.'],
  ];
  defs.forEach(([k, v], i) => {
    const x = 54 + i * 396;
    h3(s, k, x, cy, 370);
    T(s, v, x, cy + 20, 370, 76, { size: 9.5, ls: 1.35 });
  });

  foot(s, 'Sources README.md, docs/overview.md, docs/event-protocol-v2.md · run: p4-agent 0.0.0.0:52001 tcp://THIS_HOST:52001');
}

/* ════════════════════════════════════════════════════════
   04 — The two wires
   ════════════════════════════════════════════════════════ */
{
  const s = slide('wire', { eyebrow: 'The header alone gives the full length', h: 'Every hop reads the envelope, only its destination the body' }, 4,
    'This split is the design. A relay forwards by copying bytes it never decoded, so forwarding cost does not depend on the message kind, and a new message kind cannot make relays heavier.');
  const y0 = s.bodyTop;

  function strip(yy, tag, segs, caps) {
    T(s, tag, 54, yy, 700, 14, { size: 9, color: FLOW, mono: true, bold: true });
    let x = 54;
    segs.forEach((sg) => {
      box(s, x, yy + 20, sg.w, 42, { fill: sg.k === 'f' ? FLOWW : sg.k === 'b' ? BNDW : SURF2, line: sg.k === 'f' ? FLOW : sg.k === 'b' ? BND : RULE2, lw: sg.k ? 1.2 : 1 });
      T(s, sg.t, x, yy + 34, sg.w, 16, { size: sg.small ? 7.5 : 9, color: sg.small ? MUT : INK, mono: !sg.ko, align: 'center' });
      x += sg.w;
    });
    let ox = 54;
    segs.forEach((sg, i) => {
      if (sg.off !== undefined) T(s, String(sg.off), ox - 12, yy + 66, 24, 12, { size: 7.5, color: MUT, mono: true, align: 'center' });
      ox += sg.w;
    });
    caps.forEach((c) => T(s, c.t, c.x, yy + 82, 200, 12, { size: 7.5, color: MUT, mono: true }));
  }

  strip(y0, 'P4B1 · v8 — hop frame        layers/protocol/src/frame/mod.rs', [
    { t: 'P4B1', w: 86, k: 'f', off: 0 },
    { t: '8', w: 26, k: 'f', off: 4 },
    { t: 'zero×3', w: 64, small: true, off: 5 },
    { t: 'env_len', w: 86, k: 'b', off: 8 },
    { t: 'body_len', w: 86, k: 'b', off: 12 },
    { t: 'envelope — read by every hop', w: 270, ko: true, off: 16 },
    { t: 'body — read only by the destination', w: 554, ko: true },
  ], [{ t: '≤ 256 KiB', x: 348 }, { t: '≤ 2 GiB', x: 618 }]);

  rule(s, 54, y0 + 108, 1172);

  strip(y0 + 126, 'P4E3 — event frame (current execution path)        layers/protocol/src/event/wire.rs', [
    { t: 'P4E3', w: 86, k: 'f', off: 0 },
    { t: 'env_len', w: 86, k: 'b', off: 4 },
    { t: 'payload_len', w: 86, k: 'b', off: 8 },
    { t: 'envelope — 12 fields, self-describing', w: 300, ko: true, off: 12 },
    { t: 'payload — opaque bytes', w: 614, ko: true },
  ], [{ t: '≤ 256 KiB', x: 258 }, { t: '≤ 2 GiB', x: 558 }]);

  const ny = y0 + 248;
  note(s, 54, ny, 576, 124,
    'Why the body ceiling was raised to 2 GiB is kept in a code comment. A staged prefill hop carries one hidden-state cut per sequence and token, as F32. ' +
    'A 2,048-wide model with a 5,000-token prompt makes one sequence 39 MiB and a 10-wide prefill window 391 MiB. ' +
    'The old 128 MiB ceiling cut that window at 3 — and with the deployment healthy, 53 of 60 requests failed with HOP envelope too large.');
  note(s, 650, ny, 576, 124,
    'Doc/code gap (measured): docs/api.md says “P4B1 v6 · body max 1 MiB”, but the code is v8 · 2 GiB. ' +
    'The code comment also owns the reason for the v7→v8 bump: SessionClose changed from a one-way broadcast to an ack contract, ' +
    'and a half-old fleet looks healthy until the first early close, then silently leaks stages on the old side. ' +
    'The version bump turns that into a two-way rejection before a single body byte.', 'x');

  foot(s, 'Measured frame/mod.rs:11–35(MAGIC·VERSION·ceilings), event/wire.rs:4–6 · doc quoted: docs/api.md — the gap calls for a doc update; this slide follows the code values');
}

/* ════════════════════════════════════════════════════════
   05 — envelope
   ════════════════════════════════════════════════════════ */
{
  const s = slide('wire', { eyebrow: 'event envelope', h: 'P4 knows nothing of prompts, tokens or KV' }, 5,
    'Every event is self-describing so it can be routed without decoding the payload. Only the concrete adapter named by content-type and adapter_kind may interpret those bytes.');
  const y0 = s.bodyTop;

  h3(s, 'ENVELOPE — 12 FIELDS      VERSION = 3', 54, y0, 640, FLOW);
  const rows = [
    ['protocol_version', 'rejected unread unless it is 3'],
    ['event_id', 'immutable event identity'],
    ['correlation_id', 'lifetime/request identity'],
    ['causation_id?', 'the event that caused this one'],
    ['source · target', 'logical producer · final consumer (Endpoint values)'],
    ['return_route?', 'stable output/telemetry destination'],
    ['class', 'control · data · output · telemetry'],
    ['sequence', 'monotonic within correlation + source'],
    ['deadline_unix_ms?', 'acceptance fence. Not a promise to abort. 0 is rejected'],
    ['adapter_kind?', 'selects the concrete adapter. payload stays opaque'],
    ['payload_content_type', 'how to interpret those bytes'],
  ].map(([a, b]) => [
    { text: a, options: { fontFace: M, fontSize: 8.5, color: INK } },
    { text: b, options: { fontSize: 9.5 } },
  ]);
  table(s, rows, 54, y0 + 22, 640, [200, 440], { size: 9.5, rowH: 30 });

  h3(s, 'AN ENDPOINT IS A VALUE, NOT A REGISTRY KEY', 730, y0, 496);
  const eps = [
    ['Agent', '{ address }'],
    ['Node', '{ agent_address, node_id }'],
    ['Outer', '{ ingress_agent_address, channel_id, connection_generation }'],
  ];
  eps.forEach(([k, v], i) => {
    T(s, k, 730, y0 + 24 + i * 22, 70, 14, { size: 9, color: FLOW, mono: true });
    T(s, v, 806, y0 + 24 + i * 22, 420, 14, { size: i === 2 ? 8 : 9, color: INK2, mono: true });
  });

  note(s, 730, y0 + 100, 496, 96,
    'source cannot double as the reply address. The tail node gets data from the node before it, but the output belongs to the OUTER that started inference. ' +
    'A stable channel_id names that stream; connection_generation stops a new stream from flowing into an old socket after reconnect.', 'f');
  note(s, 730, y0 + 210, 496, 80,
    'Intermediate agents never rewrite source, target or return_route. A finished adapter creates ' +
    'a new event with itself as source, the exact next endpoint as target and the causing event as causation_id.');
  T(s, 'Fields it does not define: prompt · token · max-token · Prefill · Decode · KV · layer · tensor · batch · backend.\n' +
    'Order and dedup come from per-source sequence + event identity; hashing the transport route to infer logical order is invalid.',
    730, y0 + 304, 496, 60, { size: 9.5, ls: 1.35 });

  foot(s, 'Measured layers/protocol/src/event/mod.rs:94–177(Envelope·EventClass·validate) · contract docs/event-protocol-v2.md');
}

/* ════════════════════════════════════════════════════════
   06 — Verdicts and lanes
   ════════════════════════════════════════════════════════ */
{
  const s = slide('agent core', { eyebrow: 'Verdicts and lanes', h: 'A frame is judged exactly twice, and priority is bounded' }, 6);
  const y0 = s.bodyTop;

  // verdict
  box(s, 174, y0, 280, 40, { fill: FLOWW, line: FLOW, lw: 1.2 });
  T(s, 'Is target my address?', 174, y0 + 12, 280, 18, { size: 10, color: INK, mono: false, align: 'center' });
  line(s, 254, y0 + 42, 174, y0 + 74, { color: MUT });
  T(s, 'no', 176, y0 + 46, 60, 12, { size: 7.5, color: MUT, mono: true });
  line(s, 374, y0 + 42, 454, y0 + 74, { color: MUT });
  T(s, 'yes', 412, y0 + 46, 40, 12, { size: 7.5, color: MUT, mono: true });
  box(s, 54, y0 + 76, 216, 52, { fill: SURF2 });
  T(s, 'Forward(target)', 54, y0 + 86, 216, 16, { size: 9, color: INK, mono: true, align: 'center' });
  T(s, 'Forwarded whole. Peer and OUTER alike', 54, y0 + 104, 216, 14, { size: 7.5, color: MUT, align: 'center' });
  box(s, 354, y0 + 76, 216, 40, { fill: SURF2 });
  T(s, 'recipient?', 354, y0 + 88, 216, 16, { size: 9, color: INK, mono: true, align: 'center' });
  line(s, 414, y0 + 118, 384, y0 + 142, { color: MUT });
  line(s, 510, y0 + 118, 524, y0 + 142, { color: MUT });
  box(s, 306, y0 + 144, 130, 48, { fill: SURF2 });
  T(s, 'Agent', 306, y0 + 154, 130, 16, { size: 9, color: INK, mono: true, align: 'center' });
  T(s, 'create/delete, query', 306, y0 + 172, 130, 14, { size: 7.5, color: MUT, align: 'center' });
  box(s, 446, y0 + 144, 128, 48, { fill: BNDW, line: BND, lw: 1.2 });
  T(s, 'Node(id)', 446, y0 + 154, 128, 16, { size: 9, color: BND, mono: true, align: 'center' });
  T(s, 'to that node\'s queue', 446, y0 + 172, 128, 14, { size: 7.5, color: MUT, align: 'center' });
  rule(s, 54, y0 + 212, 520);
  T(s, 'The judge does not depend on lanes or chains — the moment it does, forwarding has to understand its own traffic.',
    54, y0 + 224, 520, 32, { size: 9.5, color: INK2, align: 'center', ls: 1.35 });
  T(s, 'A pure function: an envelope and its own address go in, a verdict comes out. If a third variant appears, the agent has acquired state it must not have.',
    54, y0 + 268, 520, 32, { size: 8.5, color: MUT, ls: 1.35 });

  // lanes
  const lx = 654;
  T(s, 'preference order', lx, y0, 200, 12, { size: 7.5, color: MUT, mono: true });
  const L = [['control', true], ['response', false], ['decode', false], ['prefill', false]];
  L.forEach(([n, hot], i) => {
    const yy = y0 + 16 + i * 26;
    box(s, lx, yy, 150, 22, { fill: hot ? FLOWW : SURF2, line: hot ? FLOW : RULE2, lw: hot ? 1.2 : 1 });
    T(s, n, lx + 8, yy + 4, 140, 14, { size: 9, color: INK, mono: true });
  });
  line(s, lx + 154, y0 + 64, lx + 196, y0 + 64, { color: FLOW, lw: 1.4 });
  box(s, lx + 198, y0 + 42, 106, 48, { fill: FLOWW, line: FLOW, lw: 1.2 });
  T(s, 'dispatcher', lx + 198, y0 + 54, 106, 16, { size: 9, color: INK, mono: true, align: 'center' });
  T(s, 'a single task', lx + 198, y0 + 72, 106, 14, { size: 7.5, color: MUT, align: 'center' });
  T(s, 'take 1 … 16', lx + 330, y0 + 22, 200, 12, { size: 7.5, color: MUT, mono: true });
  for (let i = 0; i < 16; i++) {
    box(s, lx + 330 + i * 12, y0 + 34, 9, 18, { fill: i === 15 ? BND : SURF3, line: null });
  }
  T(s, 'every 16th = fair', lx + 330, y0 + 58, 190, 12, { size: 7.5, color: BND, mono: true, align: 'right' });
  T(s, 'all four lanes compete equally', lx + 330, y0 + 76, 200, 14, { size: 8, color: MUT });
  rule(s, lx, y0 + 122, 572);
  T(s, 'Strict priority is not a preference but a veto. A lap that is never dispatched is a request that never finishes.',
    lx, y0 + 134, 572, 32, { size: 9.5, color: INK2, ls: 1.35 });
  note(s, lx, y0 + 178, 572, 122,
    'One route, one worker. Order within a route is the only ordering guarantee, and it comes from registration order. ' +
    'Handing consecutive frames of one route to different workers throws that away, and to the caller it looks as if P4 reordered the stream. ' +
    'Hashing the route keeps each route serial while different routes run concurrently.');

  foot(s, 'Sources layers/agent/src/worker/judge, layers/agent/src/queue/main, docs/architecture.md, docs/constraints.md · forwarding also happens in the dispatcher, not in a task of its own');
}

/* ════════════════════════════════════════════════════════
   07 — The node's long work
   ════════════════════════════════════════════════════════ */
{
  const s = slide('agent core', { eyebrow: 'The node\'s long work', h: 'A node advances on exactly two events — there is no timer' }, 7,
    'A node\'s speed is the backend\'s speed. A third trigger would be a guess about that speed.');
  const y0 = s.bodyTop;

  box(s, 54, y0 + 40, 150, 46, { fill: SURF2 });
  T(s, 'worker', 54, y0 + 50, 150, 16, { size: 9, color: INK, mono: true, align: 'center' });
  T(s, 'hands off and is done', 54, y0 + 68, 150, 14, { size: 7.5, color: MUT, align: 'center' });
  line(s, 208, y0 + 63, 268, y0 + 63, { color: FLOW, lw: 1.4 });
  T(s, '① arrival', 208, y0 + 44, 62, 12, { size: 7.5, color: FLOW, mono: true, align: 'center' });

  box(s, 270, y0 + 20, 230, 150, { fill: FLOWW, line: FLOW, lw: 1.2 });
  T(s, 'node run loop', 270, y0 + 32, 230, 18, { size: 10.5, color: INK, mono: true, align: 'center', bold: true });
  rule(s, 284, y0 + 56, 202);
  const kv = [['queue', 'depth · waiting'], ['in-flight map', 'keyed by sequence'], ['ceiling', 'declared by load'], ['lifecycle', 'Never·At(gen)·Refused']];
  kv.forEach(([a, b], i) => {
    T(s, a, 284, y0 + 66 + i * 22, 110, 14, { size: 8.5, color: INK, mono: true });
    T(s, b, 344, y0 + 66 + i * 22, 142, 14, { size: 7.5, color: MUT, mono: true, align: 'right' });
  });
  T(s, 'one hop at a time — by construction', 270, y0 + 152, 230, 14, { size: 8, color: MUT, align: 'center' });

  line(s, 502, y0 + 76, 614, y0 + 76, { color: FLOW, lw: 1.4 });
  T(s, 'hop(window)', 502, y0 + 58, 112, 12, { size: 7.5, color: MUT, mono: true, align: 'center' });
  box(s, 616, y0 + 50, 150, 52, { fill: SURF2 });
  T(s, 'adapter', 616, y0 + 60, 150, 16, { size: 9, color: INK, mono: true, align: 'center' });
  T(s, 'on a blocking thread', 616, y0 + 78, 150, 14, { size: 7.5, color: MUT, align: 'center' });
  line(s, 770, y0 + 76, 832, y0 + 76, { color: MUT });
  box(s, 834, y0 + 50, 150, 52, { fill: BNDW, line: BND, lw: 1.2 });
  T(s, 'backend', 834, y0 + 60, 150, 16, { size: 9, color: BND, mono: true, align: 'center' });
  T(s, 'time on the device', 834, y0 + 78, 150, 14, { size: 7.5, color: MUT, align: 'center' });

  line(s, 909, y0 + 104, 909, y0 + 140, { color: FLOW, dash: true, arrow: false, lw: 1.3 });
  line(s, 909, y0 + 140, 390, y0 + 140, { color: FLOW, dash: true, arrow: false, lw: 1.3 });
  line(s, 390, y0 + 140, 390, y0 + 172, { color: FLOW, dash: true, lw: 1.3 });
  T(s, '② hop ends — only after seeing this does the next one start', 440, y0 + 118, 440, 14, { size: 8, color: FLOW, mono: false, align: 'center' });

  box(s, 1010, y0 + 40, 216, 62, { line: BLK, lw: 1.2, fill: BLKW });
  T(s, 'No timer', 1010, y0 + 52, 216, 16, { size: 9.5, color: BLK, mono: false, align: 'center', bold: true });
  T(s, 'a node\'s speed is the backend\'s speed', 1010, y0 + 72, 216, 14, { size: 7.5, color: MUT, align: 'center' });

  rule(s, 54, y0 + 196, 1172);
  T(s, 'backpressure flows back up the same chain', 54, y0 + 206, 500, 14, { size: 9, color: BND, mono: true, bold: true });
  const chain = ['peer queue full', 'dispatcher stalls', 'lane fills', 'node outbox stalls', 'slows on the next hop', 'producer'];
  let cx = 54;
  chain.forEach((c, i) => {
    const w = [110, 130, 100, 140, 170, 70][i];
    T(s, c, cx, y0 + 228, w, 14, { size: 8.5, color: INK2, mono: false });
    if (i < chain.length - 1) line(s, cx + w + 4, y0 + 234, cx + w + 44, y0 + 234, { color: BND, lw: 1.2 });
    cx += w + 50;
  });
  T(s, 'the chain ends at whatever produces work', 900, y0 + 206, 326, 14, { size: 8, color: MUT, align: 'right' });

  const dy = y0 + 262;
  const trio = [
    ['CANCELLATION AND DEADLINES', 'Decided at this boundary. There is no way to abort a hop, and no need for one — not starting the next one is the whole mechanism.'],
    ['A HOP CARRIES A WINDOW', 'Batching is the node\'s decision, capped by the value the load declared and never derived. The window is composed close to the work.'],
    ['NOTHING RETURNS A VALUE', 'A handler is a procedure whose only output is putting frames on a queue. A response path tied to the call stack dies with that frame, so continuations are registered.'],
  ];
  trio.forEach(([k, v], i) => {
    const x = 54 + i * 396;
    h3(s, k, x, dy, 370);
    T(s, v, x, dy + 20, 370, 72, { size: 9.5, ls: 1.35 });
  });

  foot(s, 'Sources layers/agent/src/node/runner(+events·handle·bound), node/window(compose), docs/architecture.md · the run loop ends when its work channel closes — otherwise replaced or deleted nodes stayed resident with their adapter, queue and in-flight map');
}

/* ════════════════════════════════════════════════════════
   08 — Layer isolation stack
   ════════════════════════════════════════════════════════ */
{
  const s = slide('Isolation contract', { eyebrow: 'Per-layer responsibility and dependency direction', h: 'Keep llama.cpp churn out of transport core, ledger, batch policy' }, 8,
    'llama.cpp is not a simple device wrapper. It is an abstraction layer that owns model, graph and memory execution, and the CUDA, CPU, Metal, HIP and Vulkan implementations in ggml/backend below it change separately.');
  const y0 = s.bodyTop;

  const L = [
    ['OUTER', 'product goals · resources/topology · requests/SLO · snapshot triggers', 'direct engine KV edits ✗ · wire ACK as gen. done ✗', 'b'],
    ['layers/protocol', 'event envelope · address/identity/routing/type boundary', 'llama seq · ggml type · CUDA device ✗', 'f'],
    ['layers/agent', 'transport · broker · ordering/delivery · node lifetime · backpressure', 'choosing prefill/decode ✗ · KV cell unit cost ✗', 'f'],
    ['adapters/adapter', 'offer/take · completion contract · event ownership · Full/Closed', 'an engine\'s private types / bypass casts ✗', 'f'],
    ['llamacpp/staged', 'L0 queue → L1 ledger ↔ L2 acceptance → L3 policy → L4 proof → L5 issue', 'extending the shared P4 interpreter per model ✗', ''],
    ['native stage shell · engine facade', 'load/inspect/execute/settle/state/quiesce + opaque handle', 'llama_context internals · sampler ownership ✗', ''],
    ['engine bridge · private-common compat', 'current pin\'s public llama.h/ggml + isolated private stage hooks', 'treating public API as fixed ABI ✗', ''],
    ['llama.cpp model · graph · memory · backend scheduler abstraction layer', '', 'owned upstream', 'q'],
    ['ggml / backend — CUDA · CPU · Metal · HIP · Vulkan concrete implementations', '', 'changes separately', 'q'],
  ];
  const X = 86, W = 1054;
  let yy = y0;
  L.forEach(([a, b, c, k], i) => {
    const h = k === 'q' ? 26 : 34;
    box(s, X, yy, W, h, {
      fill: k === 'b' ? BNDW : k === 'f' ? FLOWW : k === 'q' ? null : SURF2,
      line: k === 'b' ? BND : k === 'f' ? FLOW : RULE2,
      lw: k === 'b' || k === 'f' ? 1.2 : 1,
      dash: k === 'q',
    });
    T(s, a, X + 12, yy + (k === 'q' ? 6 : 9), b ? 330 : 820, 18, { size: 8.5, color: k === 'b' ? BND : INK, mono: true });
    if (b) T(s, b, X + 350, yy + 9, 430, 18, { size: 8.5, color: INK2 });
    T(s, c, X + 12, yy + (k === 'q' ? 6 : 9), W - 24, 18, { size: 7.5, color: MUT, align: 'right' });
    yy += h + (k === 'q' ? 4 : 12);
  });
  line(s, 66, y0 - 4, 66, yy - 10, { color: FLOW, lw: 1.4 });
  T(s, '↓  left arrow: semantic dependency direction', 86, yy + 6, 500, 16, { size: 8, color: FLOW });
  T(s, '↑  right arrow: results/telemetry — via explicit contracts, creating no reverse ownership', 600, yy + 6, 540, 16, { size: 8, color: BND, align: 'right' });
  line(s, 1156, yy - 10, 1156, y0 - 4, { color: BND, dash: true, lw: 1.3 });


  foot(s, 'Source docs/layer-isolation-contract.md §2·§4 · the current event boundary is adapter/src/node_adapter/mod.rs::NodeAdapter and agent/src/event_node/mod.rs::EventNode; do not mix it with the older service boundary of Adapter::start(Work)');
}

/* ════════════════════════════════════════════════════════
   09 — Three pass conditions
   ════════════════════════════════════════════════════════ */
{
  const s = slide('Isolation contract', { eyebrow: 'There are three pass conditions', h: '“include 0 · compiles · clean on this pin” is not full isolation' }, 9,
    'A refactor that passes only one of the three is not reported as full layer isolation.');
  const y0 = s.bodyTop;

  const C = [
    {
      n: '01', k: 'Dependency isolation', c: FLOW,
      p: 'Upper code cannot know lower implementations by name. The build enforces that only allowed surfaces are depended on.',
      b: ['Means: protocol·agent·adapter-contract crates have no normal dependency on a concrete backend. Registration happens in the entrypoint',
        'pure scheduler/ledger tests build without a llama checkout, GPU or network'],
      note: 'Measured gap: layers/protocol has no [dependencies] section at all (0 deps). But adapters/adapter now depends on p4-protocol·serde·serde_json — “does not even depend on protocol” in docs/implementation.md was true at that time.',
      nk: 'x',
    },
    {
      n: '02', k: 'Authority isolation', c: BND,
      p: 'Transport and policy cannot commit the execution ledger or KV at will. Layers are not split by file names alone.',
      b: ['Basis: who commits state, who only runs effects',
        'L3 output is a candidate. A rejected issue never silently uses execution quota or credit',
        'L1 commits request delta, in-flight ledger, settlement receipt and output intent in one commit',
        'L5 executes effects and returns results. If unclear, it leaves them Uncertain/fenced'],
      note: 'No token/native effect goes out ahead of settlement approval. Success at the tail engine is not output authority that bypasses ledger approval.',
      nk: 'b',
    },
    {
      n: '03', k: 'Semantic isolation', c: BLK,
      p: 'Even an upstream change that compiles cannot be promoted if it breaks an existing contract.',
      b: ['Means: conformance catches semantic changes, and product LOAD enforces that verification result and the execution identity',
        'Advertising a capability that cannot be honored fails LOAD/conformance',
        'An unknown value is never read as “default CUDA” or “supported”'],
      note: 'A clean replay on the pin is not proof of semantic compatibility. If an opaque class exposes impl() and a consumer can include an internal header, the boundary is not finished — confirm by compiling a real consumer.',
      nk: 'x',
    },
  ];
  C.forEach((c, i) => {
    const x = 54 + i * 394;
    box(s, x, y0, 384, 404, { fill: SURF2, line: null });
    T(s, c.n, x + 22, y0 + 20, 60, 16, { size: 8.5, color: c.c, mono: true, bold: true });
    T(s, c.k, x + 22, y0 + 42, 340, 22, { size: 13, color: c.c, bold: true });
    T(s, c.p, x + 22, y0 + 72, 340, 50, { size: 9.5, color: INK2, ls: 1.35 });
    bullets(s, c.b, x + 22, y0 + 132, 340, 130, { size: 9 });
    note(s, x + 22, y0 + 272, 340, 116, c.note, c.nk);
  });

  foot(s, 'Source docs/layer-isolation-contract.md §1·§3·§4 · measured layers/protocol/Cargo.toml, layers/adapters/adapter/Cargo.toml @429e057de');
}

/* ════════════════════════════════════════════════════════
   10 — L0–L5
   ════════════════════════════════════════════════════════ */
{
  const s = slide('Adapter', { eyebrow: 'Batching layers L0–L5', h: 'Only one layer commits state' }, 10,
    'OUTER decides the model, topology, SLO, request arrivals and snapshot triggers; the adapter composes eligible rows and batches. No llama-specific batching/KV rules go into the P4 core.');
  const y0 = s.bodyTop;

  const Ls = [
    ['L0 queue', 'arrival/hold. No policy', 'drop as success ✗', ''],
    ['L1 ledger', 'truth: ID·residency·cells', 'atomic commit · sole write authority', 'b'],
    ['L2 accept/occupy', 'reserve·lease·quota·refusal', 'inventing TTL/victim ✗', ''],
    ['L3 compose policy', 'pure, deterministic choice', 'I/O·native call·mutation ✗', 'f'],
    ['L4 proof', 'shape·membership·position', 'byte budget check', ''],
    ['L5 issue/transport', 'stage call·capsule delivery', 'confusing ACK/terminal ✗', ''],
  ];
  Ls.forEach(([a, b, c, k], i) => {
    const x = 54 + i * 198;
    const tall = k === 'b';
    box(s, x, tall ? y0 : y0 + 12, 168, tall ? 82 : 58, {
      fill: k === 'b' ? BNDW : k === 'f' ? FLOWW : SURF2,
      line: k === 'b' ? BND : k === 'f' ? FLOW : RULE2, lw: k ? 1.2 : 1,
    });
    const ty = tall ? y0 + 10 : y0 + 22;
    T(s, a, x + 12, ty, 144, 16, { size: 9, color: k === 'b' ? BND : INK, mono: true, bold: k === 'b' });
    T(s, b, x + 12, ty + 20, 144, 14, { size: 7.5, color: MUT });
    T(s, c, x + 12, ty + 36, 144, 26, { size: 7.5, color: k === 'b' ? BND : MUT, ls: 1.25 });
    if (i > 0) line(s, x - 28, y0 + 41, x - 4, y0 + 41, { color: FLOW, lw: 1.4 });
  });
  T(s, 'candidate allocation', 592, y0 - 14, 168, 12, { size: 7.5, color: MUT, mono: true, align: 'center' });
  T(s, 'validated issue', 790, y0 - 14, 168, 12, { size: 7.5, color: MUT, mono: true, align: 'center' });
  line(s, 1080, y0 + 74, 1080, y0 + 108, { color: BND, dash: true, arrow: false, lw: 1.3 });
  line(s, 1080, y0 + 108, 336, y0 + 108, { color: BND, dash: true, arrow: false, lw: 1.3 });
  line(s, 336, y0 + 108, 336, y0 + 86, { color: BND, dash: true, lw: 1.3 });
  T(s, 'execution result · Full: keep intent · execution unknown: Uncertain — re-running because a timeout is assumed not run ✗',
    360, y0 + 112, 700, 14, { size: 8, color: BND, align: 'center' });

  rule(s, 54, y0 + 140, 1172);
  T(s, 'A returned transport credit means “the peer took it”, not that compute/KV finished — stop-point evidence is the per-stage SequenceQuiesced attest.',
    54, y0 + 152, 1172, 18, { size: 10, color: INK2 });

  const by = y0 + 190;
  h3(s, 'INVARIANTS A STRATEGY MUST HOLD (EXCERPT)', 54, by, 560);
  bullets(s, [
    'Residency precondition: before a row runs on a stage, that stage\'s prefix KV and auxiliary state must be valid — this does not mean draining the whole pipeline',
    'No submission without proof: never assume KV survived a failed execution',
    'Snapshot consistency fence: Persist·Fork only at a stop point with zero in-flight rows and every stage settled — a snapshot exported mid-batch is a wrong answer with an ambiguous position',
  ], 54, by + 22, 560, 130, { size: 9.5 });

  h3(s, 'COST DIFFERS PER NODE (PAST OBSERVATIONS)', 666, by, 560);
  const obs = [
    ['per-node KV', '173.5 / 63.3 / 157.7 / 126.1 MB at the same n_ctx — the bottleneck is the most expensive node'],
    ['compute buffer', '1,412MB@ubatch512 ↔ 386MB@128 — batch width is the dominant VRAM knob'],
    ['cut-set width', 'gemma-4: 31/27/23 tensors, 81 transfers per step. Qwen family: 1 — a per-model constant'],
  ].map(([a, b]) => [
    { text: a, options: { fontFace: M, fontSize: 8.5, color: INK } },
    { text: b, options: { fontSize: 9 } },
  ]);
  table(s, obs, 666, by + 22, 560, [130, 430], { size: 9 });

  foot(s, 'Sources docs/adapter-batching-layers.md(layers, invariants, past observations), docs/layer-isolation-contract.md §3 · observations hold only for the conditions at the time and do not establish the current bottleneck');
}

/* ════════════════════════════════════════════════════════
   11 — Staged execution and output authority
   ════════════════════════════════════════════════════════ */
{
  const s = slide('staged execution', { eyebrow: 'Stage chain and output authority', h: 'Native compute location and output approval authority differ' }, 11,
    'Even if the tail node computes the result, OUTER output is issued only after the head, which owns settlement, verifies and commits the ledger.');
  const y0 = s.bodyTop;

  box(s, 200, y0, 200, 38, { fill: BNDW, line: BND, lw: 1.2 });
  T(s, 'OUTER', 200, y0 + 10, 200, 18, { size: 10, color: BND, mono: true, align: 'center', bold: true });
  line(s, 262, y0 + 92, 262, y0 + 42, { color: BND, lw: 1.3 });
  T(s, 'OUTPUT v4', 120, y0 + 56, 130, 12, { size: 7.5, color: BND, mono: true, align: 'right' });
  line(s, 340, y0 + 40, 340, y0 + 88, { color: MUT, dash: true });
  T(s, 'SESSION v4', 350, y0 + 56, 120, 12, { size: 7.5, color: MUT, mono: true });

  const st = [
    [y0 + 94, 'stage 0 — head', 'settle, verify, commit, emit RELEASE', 'f'],
    [y0 + 172, 'stage 1', 'next ≠ terminal (in 3-stage)', ''],
    [y0 + 242, 'stage 2 — terminal', 'returns native result to head', ''],
  ];
  st.forEach(([yy, a, b, k]) => {
    box(s, 190, yy, 230, 52, { fill: k === 'f' ? FLOWW : SURF2, line: k === 'f' ? FLOW : RULE2, lw: k ? 1.2 : 1 });
    T(s, a, 204, yy + 12, 206, 16, { size: 9, color: INK, mono: true });
    T(s, b, 204, yy + 30, 206, 14, { size: 7.5, color: MUT });
  });
  line(s, 212, y0 + 148, 212, y0 + 170, { color: FLOW, lw: 1.4 });
  line(s, 212, y0 + 226, 212, y0 + 240, { color: FLOW, lw: 1.4 });
  T(s, 'PHYSICAL\nRELEASE\nSETTLE\nsource = previous', 54, y0 + 154, 128, 64, { size: 7.5, color: FLOW, mono: true, align: 'right', ls: 1.3 });
  line(s, 398, y0 + 240, 398, y0 + 226, { color: MUT });
  line(s, 398, y0 + 170, 398, y0 + 148, { color: MUT });
  T(s, 'TAIL_BATCH\nSETTLED\nRELEASED\nsource = terminal', 430, y0 + 154, 150, 64, { size: 7.5, color: MUT, mono: true, ls: 1.3 });
  T(s, 'first/previous/next/terminal derive only from the installed order', 54, y0 + 312, 560, 14, { size: 8, color: MUT });
  T(s, 'Checks run before the ledger, KV, slots or output change. In a 3-stage chain, the head\'s next and terminal differ.',
    54, y0 + 334, 560, 30, { size: 8.5, color: MUT, ls: 1.35 });

  h3(s, 'WHAT SESSION v4 INSTALLS', 634, y0, 592, FLOW);
  bullets(s, [
    'Required: load_generation, session_id, full order stages:[{agent,node,generation}, …], receiver\'s stage_index',
    'role/first/next removed; unknown top-level fields and old v3 commands rejected',
    'Checks node address, nonzero generation, unique identity, valid index; installs only if the index\'s full endpoint matches the real worker and envelope target',
    'Different generations of one agent/node cannot be different stages at once',
  ], 634, y0 + 22, 592, 140, { size: 9.5 });

  note(s, 634, y0 + 176, 592, 80,
    'OUTPUT\'s envelope source is the configured first endpoint. OUTER checks the whole endpoint (agent address, node ID, node generation) plus ' +
    'load/session/request, target/return route, correlation and position. The tail is not also accepted just for being a configured node.');
  note(s, 634, y0 + 270, 592, 96,
    'The limits are stated plainly. Per-receiver checks alone do not prove fleet-wide agreement on the same declaration, user/network authentication, or freshness after a process restart. ' +
    'The current stage path supports only 2 or more stages, with head and tail separate — a limit from having no single-stage implementation, not a placement policy to cut nodes per device.', 'x');

  foot(s, 'Sources docs/adapter-batching-layers.md(SESSION/OUTPUT v4 section) · implementation staged/adapter/src/v2/(commands·worker·completion·issue_witness), tools/event-drive/src/run/mod.rs::session_events');
}

/* ════════════════════════════════════════════════════════
   12 — Absorbing upstream
   ════════════════════════════════════════════════════════ */
{
  const s = slide('upstream', { eyebrow: 'Absorbing llama.cpp changes', h: 'Official checkout untouched. One directory per pin owns patches' }, 12,
    'If even one patch does not apply cleanly, the build stops before CMake. The official checkout is never edited to get a build through.');
  const y0 = s.bodyTop;

  T(s, 'current candidate pin — ef6876693', 54, y0, 500, 14, { size: 9, color: FLOW, mono: true, bold: true });
  T(s, 'official PR #27742 head · upstream_status “open-candidate” · nearest release b10645 · does not replace the stable pack without its own acceptance evidence',
    54, y0 + 18, 620, 30, { size: 8, color: MUT, ls: 1.35 });
  T(s, '9 pins kept', 706, y0, 520, 14, { size: 9, color: INK, mono: true, bold: true });
  T(s, '0eadefebd · 1269cb1ff · 3e3a7a416 · 4308a4f03 · 434ddbbc0 · 557614e02 · d7a207411 · ef6876693 · fe2adf0e7',
    706, y0 + 18, 520, 30, { size: 8, color: MUT, ls: 1.35 });

  const dy = y0 + 62;
  box(s, 54, dy + 46, 196, 62, { line: RULE2, dash: true });
  T(s, 'upstream/', 68, dy + 56, 170, 14, { size: 9, color: INK, mono: true });
  T(s, 'official clone · kept pristine\ngit ignore · not committed', 68, dy + 74, 170, 30, { size: 7.5, color: MUT, ls: 1.3 });
  line(s, 254, dy + 58, 326, dy + 58, { color: MUT });
  T(s, 'unpatched', 254, dy + 40, 72, 12, { size: 7.5, color: MUT, mono: true, align: 'center' });
  box(s, 328, dy + 36, 186, 46, { fill: SURF2 });
  T(s, 'stock build', 342, dy + 46, 160, 14, { size: 9, color: INK, mono: true });
  T(s, 'ggml-rpc-server · llama-server', 342, dy + 62, 160, 14, { size: 7.5, color: MUT });

  line(s, 152, dy + 112, 152, dy + 142, { color: FLOW, lw: 1.4 });
  box(s, 54, dy + 144, 196, 58, { fill: FLOWW, line: FLOW, lw: 1.2 });
  T(s, 'prepare-pipeline-\nupstream.mjs', 68, dy + 152, 170, 28, { size: 8.5, color: INK, mono: true, ls: 1.2 });
  T(s, 'run twice: create → verify reuse', 68, dy + 182, 170, 14, { size: 7.5, color: MUT });

  box(s, 300, dy + 128, 216, 80, { fill: BNDW, line: BND, lw: 1.2 });
  T(s, 'compat/<pin>/', 314, dy + 138, 190, 14, { size: 9, color: BND, mono: true, bold: true });
  T(s, 'ordered patches 0001…0021\nmanifest.json — per-patch sha256,\npatch_set_sha256, patched_tree',
    314, dy + 156, 190, 46, { size: 7.5, color: MUT, ls: 1.3 });
  line(s, 296, dy + 158, 254, dy + 166, { color: BND, lw: 1.3 });
  T(s, 'hash check', 244, dy + 138, 60, 12, { size: 7.5, color: BND, mono: true, align: 'center' });

  line(s, 520, dy + 168, 582, dy + 168, { color: FLOW, lw: 1.4 });
  box(s, 584, dy + 138, 210, 62, { fill: SURF2 });
  T(s, '.cache/ worktree', 598, dy + 148, 184, 14, { size: 9, color: INK, mono: true });
  T(s, 'generated patched source\nnever committed', 598, dy + 166, 184, 30, { size: 7.5, color: MUT, ls: 1.3 });
  line(s, 798, dy + 168, 860, dy + 168, { color: FLOW, lw: 1.4 });
  box(s, 862, dy + 138, 212, 62, { fill: FLOWW, line: FLOW, lw: 1.2 });
  T(s, 'pipeline build', 876, dy + 148, 186, 14, { size: 9, color: INK, mono: true });
  T(s, 'abi_revision: pipeline-abi-v1\nchecks 2 required symbols', 876, dy + 166, 186, 30, { size: 7.5, color: MUT, ls: 1.3 });
  line(s, 1078, dy + 168, 1124, dy + 168, { color: FLOW, lw: 1.4 });
  box(s, 1080, dy + 36, 146, 72, { fill: BLKW, line: BLK, lw: 1.2 });
  T(s, 'fleet promotion', 1080, dy + 48, 146, 16, { size: 9.5, color: BLK, align: 'center', bold: true });
  T(s, 'CUDA·Metal·OpenCL\nafter real load', 1080, dy + 70, 146, 32, { size: 7.5, color: MUT, align: 'center', ls: 1.3 });
  line(s, 1152, dy + 136, 1152, dy + 112, { color: BLK, lw: 1.3 });

  const ny = y0 + 310;
  note(s, 54, ny, 576, 104,
    'Where the patches touch is exactly where isolation happens. ggml-backend·ggml-rpc, public pipeline ABI, llama-context/graph/memory headers, model-loader, ' +
    'KV stage window, stage memory residency(+recurrent), MTP tail stage, speculative sequence lifecycle, stage noalloc memory breakdown — ' +
    'all inside the compat boundary, never surfacing to upper layers.', 'f');
  note(s, 650, ny, 576, 104,
    'Upstream adaptation ends inside the allowed modules. Not only direct includes but transitive includes, linking, forward declarations, public signatures and imported relinks ' +
    'are checked together. Never widen the allowlist and then report “0 violations”.');

  foot(s, 'Sources staged/compat/ef6876693/{README.md, manifest.json}(measured: 21 patches, 2 required symbols), docs/layer-isolation-contract.md §4 · pin count measured with ls staged/compat');
}

/* ════════════════════════════════════════════════════════
   13 — Code landscape
   ════════════════════════════════════════════════════════ */
{
  const s = slide('Code landscape', { eyebrow: 'Measured per crate', h: 'The weight is in the adapters; boundaries have no dependencies' }, 13,
    'Rust: 399 files, 108,815 lines (tests included) · staged C++ stage runtime: 14,597 lines. wc -l @429e057de.');
  const y0 = s.bodyTop;

  const bars = [
    ['llamacpp/staged', 46891, 'f'], ['agent', 17046, 'f'], ['service', 9744, ''],
    ['tools/event-drive', 7241, ''], ['adapters/adapter', 6463, 'b'], ['llamacpp/deployment', 5079, ''],
    ['tools/drive', 4408, ''], ['adapters/mock', 4209, ''], ['llamacpp/served', 3713, ''],
    ['protocol', 2311, 'b'], ['entrypoints/agent', 1300, ''], ['tools/link', 410, ''],
  ];
  const AX = 230, AW = 420, MAXV = 48000;
  [0, 10000, 20000, 30000, 40000].forEach((v) => {
    const x = AX + (v / MAXV) * AW;
    line(s, x, y0 + 4, x, y0 + 316, { color: RULE, arrow: false, lw: 0.75 });
    T(s, v === 0 ? '0' : v / 1000 + 'k', x - 20, y0 + 322, 40, 12, { size: 7.5, color: MUT, mono: true, align: 'center' });
  });
  bars.forEach(([n, v, k], i) => {
    const yy = y0 + 8 + i * 25;
    T(s, n, 54, yy, 168, 14, { size: 8.5, color: INK, mono: true, align: 'right' });
    const w = (v / MAXV) * AW;
    box(s, AX, yy, w, 14, { fill: k === 'b' ? BND : k === 'f' ? FLOW : SURF3, line: null });
    T(s, v.toLocaleString('en-US'), AX + w + 6, yy, 70, 14, { size: 7.5, color: MUT, mono: true });
  });
  box(s, AX, y0 + 344, 10, 10, { fill: BND, line: null });
  T(s, 'boundary crate — compile errors enforce the rules here', AX + 16, y0 + 343, 420, 14, { size: 8, color: MUT });

  h3(s, 'TWO BOUNDARY CRATES', 700, y0, 526);
  const deps = [
    ['p4-protocol', 'none — no [dependencies] section at all. The protocol cannot learn backends'],
    ['p4-adapter', 'p4-protocol · serde · serde_json'],
    ['p4-agent-core', 'p4-adapter · p4-protocol · tokio'],
    ['p4-mock', 'p4-adapter · tokio — a second implementation of the whole interface using arithmetic alone'],
  ].map(([a, b]) => [
    { text: a, options: { fontFace: M, fontSize: 8.5, color: INK } },
    { text: b, options: { fontSize: 9 } },
  ]);
  table(s, deps, 700, y0 + 22, 526, [126, 400], { size: 9 });

  note(s, 700, y0 + 150, 526, 96,
    'The gap with the docs is left as is. docs/implementation.md says “9 crates · 16,447 lines” and “the adapter contract does not even depend on protocol”. ' +
    'The current workspace has 12 crates, and the adapter contract depends on protocol and serde. That table predates the event path; this slide follows the measurements.', 'x');
  note(s, 700, y0 + 260, 526, 96,
    'Adding a backend costs one file. entrypoints/agent/src/adapters/mod.rs — one name, one factory, one Adapter implementation. ' +
    'The agent\'s only surface is P4 over a socket. Whether an adapter talks to its backend over HTTP or a pipe is invisible above it.', 'f');

  foot(s, "Measurement command find layers entrypoints tools -name '*.rs' | xargs wc -l @429e057de · Cargo.toml checked directly · C++ is the total of .cpp/.hpp/.inc in staged/server/src");
}

/* ════════════════════════════════════════════════════════
   14 — Invariants
   ════════════════════════════════════════════════════════ */
{
  const s = slide('Invariants', { eyebrow: 'What happened without them', h: 'Most invariants are here because they broke once' }, 14);
  const y0 = s.bodyTop;

  const head = [
    { text: 'INVARIANT', options: { fontFace: M, fontSize: 8, color: MUT, bold: true, border: [{ type: 'none' }, { type: 'none' }, { pt: 1, color: RULE2 }, { type: 'none' }] } },
    { text: 'WHAT WENT WRONG WITHOUT IT', options: { fontFace: M, fontSize: 8, color: MUT, bold: true, border: [{ type: 'none' }, { type: 'none' }, { pt: 1, color: RULE2 }, { type: 'none' }] } },
  ];
  const body = [
    ['The socket reader only enqueues', 'Handlers ran inside the read loop, so every handler\'s run time piled up there'],
    ['The node\'s queue holds the long work', 'Otherwise GPU time shows up as agent queue depth and nothing can be attributed'],
    ['One route, one worker', 'Frames of one route raced across several workers, and to the caller it looked as if P4 had reordered the stream'],
    ['The node\'s select is unbiased', 'Favoring completions starved arrivals completely — sitting in the channel, in no queue, invisible'],
    ['The advertised address is one peers can reach', 'Both agents called themselves 127.0.0.1, finished prefill, stopped after one token — the lap resolved to the machine holding the frame'],
    ['Advertise hints are parsed, not concatenated', 'Appending the bind port to a hint with a port gave HOST:52001:52001 — it parses, resolves nothing, and reports itself ready'],
    ['The node run loop ends when its handle drops', 'The node holds its own event sender, so a loop waiting on that channel waits on itself. Every replaced/deleted node stayed resident'],
    ['Quiet peers are released', 'The peer map was append-only for every address ever seen — bounded on a fixed fleet, unbounded once callers return on new ports'],
    ['Declared frame length is capped before allocation', 'The header is 16 bytes and can claim anything. The ceiling makes that trap cost 1.25MB per connection, not the process'],
    ['Only the end of the chain produces tokens', 'If middle stages also count, an n-stage chain emits n tokens per lap'],
    ['Expired work is answered, not dropped', 'A caller waiting for a terminal that never comes — that is what a leaked route looks like'],
  ].map(([a, b]) => [
    { text: a, options: { fontSize: 9, color: INK, bold: true } },
    { text: b, options: { fontSize: 9 } },
  ]);
  table(s, [head, ...body], 54, y0, 1172, [330, 842], { size: 9, rowH: 38 });

  foot(s, 'Source docs/constraints.md · that document owns the full table. This is an excerpt, and each row is bound to the failure evidence from when it was fixed');
}

/* ════════════════════════════════════════════════════════
   15 — Verification gates
   ════════════════════════════════════════════════════════ */
{
  const s = slide('Verification', { eyebrow: 'Gates and the 2026-09-11 screening', h: 'Unit tests, simulators, one-host multi-process: not final proof' }, 15);
  const y0 = s.bodyTop;

  T(s, 'Final results are approved only on multi-computer real-hardware wave evidence', 54, y0, 700, 14, { size: 9, color: FLOW, mono: true, bold: true });
  const G = [
    ['pure unit/mutation', 'no GPU or network', ''],
    ['mock distributed', 'arithmetic backend', ''],
    ['one host, many processes', 'not proof of results', ''],
    ['2-cluster HW screening', 'MI250 · Hy3 concurrently', 'f'],
    ['100k continuous waves', 'long valid replies·offload', 'q'],
    ['H5 perf/svc approval', 'paired 8 / holdout 4 pairs', 'x'],
  ];
  const gy = y0 + 48;
  G.forEach(([a, b, k], i) => {
    const x = 54 + i * 196;
    box(s, x, gy, 168, 50, {
      fill: k === 'f' ? FLOWW : k === 'x' ? BLKW : k === 'q' ? null : SURF2,
      line: k === 'f' ? FLOW : k === 'x' ? BLK : RULE2, lw: k === 'f' || k === 'x' ? 1.2 : 1, dash: k === 'q',
    });
    T(s, a, x + 12, gy + 10, 146, 16, { size: 8.5, color: k === 'x' ? BLK : INK, mono: false, bold: k === 'f' || k === 'x' });
    T(s, b, x + 12, gy + 28, 146, 14, { size: 7.5, color: MUT });
    if (i > 0) line(s, x - 26, gy + 25, x - 4, gy + 25, { color: FLOW, lw: 1.4 });
  });
  T(s, '▼ current position', 642, gy - 20, 168, 14, { size: 8, color: FLOW, mono: true, align: 'center' });
  T(s, 'BLOCKED', 1034, gy + 56, 168, 14, { size: 8.5, color: BLK, mono: true, align: 'center', bold: true });
  T(s, 'Node count is set by the model, KV capacity, legal cuts and deployment constraints. A one-node-per-card limit is not made a general rule.',
    54, gy + 82, 900, 14, { size: 8, color: MUT });

  const ty = y0 + 158;
  h3(s, '2026-09-11 CONCURRENT SCREENING — QUOTED VALUES', 54, ty, 640, FLOW);
  const rows = [
    ['Hy3 · 5 hosts/6 stages', 'decode cap 0→2: 5.32 → 9.55 TPS, ITL p50 1.179 → 0.533 s, 8/8 completed, released, UNLOAD on both'],
    ['MI250 · 2 hosts/16 stages', 'native CPU threads4 + cap4/min4 candidate: 34.05 TPS, ITL p50 0.319 s, 16/16 completed, released, UNLOAD'],
    ['MI250 · default CPU', 'cap4 failed, cap8 8.35 TPS (baseline 28.54 / 29.73) — regression'],
    ['local final gate', 'workspace 1384 / 0 / 7 (58 summary, filtered 0) · mutation failures 1/5/3/1 · docs-lint 92 clean'],
  ].map(([a, b]) => [
    { text: a, options: { fontFace: M, fontSize: 8.5, color: INK } },
    { text: b, options: { fontSize: 9 } },
  ]);
  table(s, rows, 54, ty + 22, 640, [186, 454], { size: 9, rowH: 42 });

  note(s, 730, ty, 496, 110,
    'What these numbers are not. The CPU4 baseline finished at 13.59 TPS, but an outside Python GPU job overlapped, so it is out of the batching-only gain comparison. ' +
    'Promoting the gain is BLOCKED until rechecked free of GPU interference. Hy3 is one screened pair, a short load (context 100k, output 256) — ' +
    'not approval of real 100k prefill, valid EOS or continuous waves.', 'x');
  note(s, 730, ty + 124, 496, 108,
    'What is not combined. TPS from different upstreams, models, backends or workloads is never summed. 8-stage 301.63 and 16-stage 7.64 ' +
    'ran under different conditions (resident 256/16, short vs 100k input mix), so neither is approved as a node-count loss rate or optimum; baselines are pinned per topology. ' +
    'GPU use, RPC depth or row count alone never approves performance or valid output.');

  foot(s, 'Quoted docs/distributed-batching-roadmap.md §0(2026-09-11) · raw data and hashes staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md · verdict contract docs/distributed-batching-verification.md §v11-gates');
}

/* ════════════════════════════════════════════════════════
   16 — Experiment composition and next steps
   ════════════════════════════════════════════════════════ */
{
  const s = slide('Next', { eyebrow: 'Experiment composition and V1.1 order', h: 'Experiments come from input combinations, not per-model branches' }, 16,
    'There is one baseline for long-term development and releases: main. A model name selects data, not a Git branch.');
  const y0 = s.bodyTop;

  const inputs = [['model', 'chat template'], ['policy', 'scheduler env'], ['workload', 'arrival waves'], ['cluster', 'nodes · cuts · KV'], ['runtime', 'sources · sha256']];
  inputs.forEach(([a, b], i) => {
    const yy = y0 + 10 + i * 38;
    box(s, 54, yy, 180, 30, { fill: i === 4 ? BNDW : SURF2, line: i === 4 ? BND : RULE2, lw: i === 4 ? 1.2 : 1 });
    T(s, a, 66, yy + 8, 90, 14, { size: 9, color: i === 4 ? BND : INK, mono: true });
    T(s, b, 128, yy + 8, 96, 14, { size: 7, color: MUT, mono: true, align: 'right' });
    line(s, 238, yy + 15, 286, y0 + 106, { color: FLOW, lw: 1.2 });
  });
  box(s, 288, y0 + 84, 140, 54, { fill: FLOWW, line: FLOW, lw: 1.2 });
  T(s, 'compose.mjs', 288, y0 + 96, 140, 16, { size: 9, color: INK, mono: true, align: 'center' });
  T(s, '+ runId · generation', 288, y0 + 114, 140, 14, { size: 7.5, color: MUT, align: 'center' });
  line(s, 432, y0 + 111, 474, y0 + 111, { color: FLOW, lw: 1.4 });
  box(s, 476, y0 + 62, 152, 98, { fill: SURF2 });
  T(s, 'runs/<fresh-id>/', 490, y0 + 72, 130, 14, { size: 8.5, color: INK, mono: true });
  T(s, 'config · manifest\nartifact\ntelemetry\nresponses', 490, y0 + 92, 130, 60, { size: 7.5, color: MUT, mono: true, ls: 1.4 });

  rule(s, 54, y0 + 212, 574);
  T(s, 'What the composer rejects', 54, y0 + 224, 300, 14, { size: 9, color: BLK, mono: true, bold: true });
  T(s, 'existing output dir · wave mismatch · missing runtime identity · duplicate native thread settings · measured prompt+output over context\n' +
    'without tokenCounts, tokenizer_verified=false — not used to claim real input',
    54, y0 + 244, 574, 44, { size: 8.5, color: MUT, ls: 1.4 });
  T(s, 'This module prepares inputs. Agent startup and cleanup, host availability, verifying real binary hashes, enforcing absolute times and execution are the cluster lifecycle runner\'s job.',
    54, y0 + 300, 574, 32, { size: 8, color: MUT, ls: 1.35 });

  h3(s, 'V1.1 IMPLEMENTATION ORDER — NEXT UP: V1.1-0', 666, y0, 560, FLOW);
  const v = [
    ['V1.1-0', 'Bind instrumentation — effective config, per-request accept/eligible/blocked reason, head monotonic issue→settle, stage queue/native/forward split'],
    ['V1.1-1', 'Budgets and termination — cap pending prompt, KV, transfer payload, output and completion receipts separately; reject over-limit before side effects'],
    ['V1.1-2', 'Batch composition — decode group ceiling and prefill quantum as independent policies. Keep decode outstanding ≤ 1'],
    ['V1.1-3', 'Pipeline window — verify prefill fragments 1→2→4→8 step by step. node execution credit = 1'],
    ['V1.1-4', 'Real-hardware promotion — short comparison → candidate selection → 100k continuous waves, long valid responses, long repetition'],
  ].map(([a, b]) => [
    { text: a, options: { fontFace: M, fontSize: 8.5, color: FLOW, bold: true } },
    { text: b, options: { fontSize: 9 } },
  ]);
  table(s, v, 666, y0 + 22, 560, [72, 488], { size: 9, rowH: 44 });

  note(s, 666, y0 + 262, 560, 92,
    'Nothing unfinished is recorded as done. No TLS, authentication or authorization (trusted network only) · no durable state (a restarted agent has no nodes; OUTER holds the record) · ' +
    'no token-kind field (reasoning and answer text are merged) · Windows ARM64 not run. New policies are off by default and apply only to ordinary attention.', 'x');

  foot(s, 'Sources test/benchmarks/cluster-inference/README.md, docs/distributed-batching-roadmap.md §0.V1.1, docs/implementation.md(not-implemented section) · no version bump/tag yet');
}

const out = process.argv[2] || 'p4-architecture.pptx';
pres.writeFile({ fileName: out }).then(() => console.log('wrote', out));
