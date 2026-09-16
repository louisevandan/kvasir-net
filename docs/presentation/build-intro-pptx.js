// P4 — architecture explainer deck for general developers (pptx for porting to Google Slides)
// Ports the 1280x720 px design space 1:1 onto 13.333in x 7.5in. 1px = 1/96in.
const P = require('pptxgenjs');

const pres = new P();
pres.defineLayout({ name: 'P4', width: 13.3333, height: 7.5 });
pres.layout = 'P4';
pres.author = 'P4';
pres.title = 'P4 Architecture Explained';

const I = (p) => p / 96;

const INK = '151A20', INK2 = '39424D', MUT = '68737F';
const RULE = 'D2D8DC', RULE2 = 'BFC7CD';
const SURF = 'FBFCFC', SURF2 = 'F1F4F5', SURF3 = 'E4EAEC';
const FLOW = '16697A', FLOWW = 'E0EDF0';
const BND = 'B05E1D', BNDW = 'F7EADB';
const BLK = '9E2B3A', BLKW = 'F6E3E5';
const DARK = '141920';

const F = 'Noto Sans KR';
const M = 'Roboto Mono';

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
    options: { bullet: { code: '2013' }, breakLine: i < items.length - 1, paraSpaceAfter: o.gap || 6 },
  }));
  s.addText(runs, {
    x: I(x), y: I(y), w: I(w), h: I(h),
    fontFace: F, fontSize: o.size || 10.5, color: o.color || INK2,
    lineSpacingMultiple: 1.25, margin: 0, isTextBox: true, valign: 'top',
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
  T(s, str, x + 14, y + 11, w - 28, h - 20, { size: 9.5, color: INK2, ls: 1.3 });
}
function h3(s, str, x, y, w, color = MUT) {
  T(s, str, x, y, w, 14, { size: 8, color, mono: true, cs: 1.2, bold: true });
}
function slide(sect, eyebrow, title, num, lede, two) {
  const s = pres.addSlide();
  s.background = { color: SURF };
  s.addText(
    [{ text: sect, options: { color: FLOW } }, { text: '   ·   ' + eyebrow, options: { color: MUT } }],
    { x: I(54), y: I(36), w: I(900), h: I(14), fontFace: M, fontSize: 8, bold: true, cs: 1.4, margin: 0, isTextBox: true, valign: 'middle' }
  );
  T(s, `${String(num).padStart(2, '0')} / 15`, 1026, 36, 200, 14,
    { size: 8, color: MUT, mono: true, align: 'right', cs: 1, valign: 'middle' });
  T(s, title, 54, 56, 1172, two ? 80 : 44, { size: 25, color: INK, bold: true, ls: 1.15 });
  let top = two ? 142 : 106;
  if (lede) { T(s, lede, 54, top, 1140, 42, { size: 10.5, color: INK2, ls: 1.35 }); top += 54; }
  s.bodyTop = top;
  return s;
}
function foot(s, str) {
  rule(s, 54, 650, 1172);
  T(s, str, 54, 658, 1172, 32, { size: 7.5, color: MUT, ls: 1.3 });
}

/* ═══ 01 Cover ═══════════════════════════════════════════ */
{
  const s = pres.addSlide();
  s.background = { color: DARK };
  T(s, 'P4  —  DISTRIBUTED INFERENCE RUNTIME LAYER', 60, 56, 900, 16,
    { size: 8.5, color: '5FB5C9', mono: true, cs: 1.8, bold: true });
  s.addShape('rect', { x: I(60), y: I(130), w: I(74), h: I(3), fill: { color: '5FB5C9' }, line: { type: 'none' } });
  T(s, 'Run a big model on many computers\nas if they were one', 60, 156, 1000, 126,
    { size: 40, color: 'F2F5F8', bold: true, ls: 1.16 });
  T(s, 'A model too big for one GPU is split by layer and loaded across several machines;\nonly intermediate values cross during inference. P4 is the layer for that link — not an inference engine.',
    60, 300, 900, 60, { size: 12, color: 'AEBAC6', ls: 1.55 });

  const mx = [60, 358, 656, 954], mw = 268;
  [['1', 'process type — agent'],
   ['18', 'ggml backends under llama.cpp'],
   ['114,154', 'Rust lines · 12 crates'],
   ['10', 'pinned llama.cpp commits']].forEach(([v, l], i) => {
    box(s, mx[i], 424, mw, 96, { fill: '1D242C', line: null });
    T(s, v, mx[i] + 18, 446, mw - 36, 32, { size: 19, color: 'F2F5F8', mono: true, bold: true });
    T(s, l, mx[i] + 18, 486, mw - 36, 16, { size: 7.5, color: '8B97A3', mono: true, cs: 0.6 });
  });
  rule(s, 60, 586, 1160, '2B333B');
  T(s, 'Baseline HEAD b3a0d51ef (2026-09-12) · code and backend figures measured from the repository · this deck explains structure and makes no performance claims',
    60, 600, 1160, 40, { size: 8, color: '8B97A3', ls: 1.4 });
}

/* ═══ 02 Why it is needed ════════════════════════════════════ */
{
  const s = slide('Background', 'Why a layer like this is needed', 'When a model outgrows one device, the split decides performance', 2,
    'Instead of shrinking weights (quantization) or pushing them to slower memory (offloading), you can load the model across several devices — and then “how do you cut it?” becomes the question.');
  const y0 = s.bodyTop;

  // tensor parallel
  box(s, 54, y0, 556, 208, { fill: SURF2, line: null });
  T(s, 'Tensor parallelism — split one layer', 76, y0 + 18, 400, 18, { size: 12, color: INK, bold: true });
  const tpx = [96, 340];
  tpx.forEach((x) => { box(s, x, y0 + 54, 180, 44, { fill: SURF, line: RULE2 }); });
  T(s, 'GPU A — half of layer L', 96, y0 + 69, 180, 16, { size: 8.5, color: INK, align: 'center' });
  T(s, 'GPU B — half of layer L', 340, y0 + 69, 180, 16, { size: 8.5, color: INK, align: 'center' });
  for (let i = 0; i < 4; i++) {
    line(s, 280, y0 + 62 + i * 10, 336, y0 + 62 + i * 10, { color: BLK, lw: 1, arrow: false });
  }
  T(s, 'all-reduce on every layer', 96, y0 + 112, 424, 16, { size: 9, color: BLK, align: 'center' });
  T(s, 'Each cut syncs full activations, so inter-device bandwidth is the limit.\nIt assumes ultra-fast links such as NVLink inside one machine.',
    76, y0 + 140, 512, 50, { size: 9.5, color: MUT, ls: 1.4 });

  // pipeline parallel
  box(s, 640, y0, 586, 208, { fill: FLOWW, line: FLOW, lw: 1.2 });
  T(s, 'Pipeline parallelism — cut by layer range', 662, y0 + 18, 460, 18, { size: 12, color: INK, bold: true });
  box(s, 682, y0 + 54, 200, 44, { fill: SURF, line: FLOW });
  T(s, 'machine 1 — layers 0‥39', 682, y0 + 69, 200, 16, { size: 8.5, color: INK, align: 'center' });
  box(s, 962, y0 + 54, 200, 44, { fill: SURF, line: FLOW });
  T(s, 'machine 2 — layers 40‥79', 962, y0 + 69, 200, 16, { size: 8.5, color: INK, align: 'center' });
  line(s, 886, y0 + 76, 958, y0 + 76, { color: FLOW, lw: 1.6 });
  T(s, '1 boundary', 886, y0 + 52, 72, 14, { size: 8, color: FLOW, mono: true, align: 'center' });
  T(s, 'Only activations at the cut cross; weights and KV cache stay on each machine.\nThat is why ordinary Ethernet can tie several machines together.',
    662, y0 + 140, 542, 50, { size: 9.5, color: INK2, ls: 1.4 });

  const ny = y0 + 232;
  h3(s, 'WHAT P4 OWNS / WHAT IT DOES NOT', 54, ny, 600, FLOW);
  bullets(s, [
    'Owns — which node on which machine takes which range, the order a request walks that chain, and what crosses when',
    'Does not own — matrix math, kernels, quantization, sampling (backend work: llama.cpp etc.)',
    'So the backend swaps without touching P4, and placement changes without touching the backend',
  ], 54, ny + 22, 600, 110, { size: 10 });

  note(s, 680, ny, 546, 132,
    'Pipeline parallelism is not free. A request passes through the stages in turn, so latency adds up per stage, ' +
    'and earlier stages sit idle. That makes batching — running many requests overlapped — the core problem of this layer, ' +
    'covered later.', 'f');

  foot(s, 'The names and properties of these splitting schemes are standard distributed-inference terms. What P4 implements in this deck is pipeline parallelism (layer-range splitting).');
}

/* ═══ 03 ① Layer structure ═════════════════════════════════ */
{
  const s = slide('① Layer structure', 'What sits on what', 'Upper layers never know lower layers by name', 3,
    'Each layer sees only what the layer right below it promised. Because backend names never leak upward, adding a backend changes not a single line of envelope, queue, node or routing code.');
  const y0 = s.bodyTop;

  const L = [
    ['OUTER', 'the requesting side — decides what is loaded where and who receives what', 'request · placement plan · session', 'b'],
    ['P4 protocol', 'envelopes and frames — knows only address, order and boundaries', 'contents are opaque bytes', 'f'],
    ['P4 agent', 'processes · queues · workers · node lifetime · delivery and backpressure', 'backend-neutral', 'f'],
    ['Adapter contract', 'what a node requires of a backend — submit · complete · cancel', 'no backend names', 'f'],
    ['Concrete adapters', 'llamacpp-staged · llamacpp · vllm · sglang · mock', 'speak their backend\'s language', ''],
    ['backend', 'llama.cpp → ggml → CUDA · ROCm · Metal · CPU …', 'actual compute', 'q'],
  ];
  const X = 54, W = 760;
  let yy = y0;
  L.forEach(([a, b, c, k]) => {
    const h = 54;
    box(s, X, yy, W, h, {
      fill: k === 'b' ? BNDW : k === 'f' ? FLOWW : k === 'q' ? SURF3 : SURF2,
      line: k === 'b' ? BND : k === 'f' ? FLOW : RULE2, lw: k === 'b' || k === 'f' ? 1.2 : 1,
    });
    T(s, a, X + 18, yy + 10, 200, 18, { size: 11.5, color: k === 'b' ? BND : INK, bold: true });
    T(s, b, X + 18, yy + 32, 560, 16, { size: 9, color: INK2 });
    T(s, c, X + W - 250, yy + 10, 232, 16, { size: 8, color: MUT, mono: true, align: 'right' });
    yy += h + 8;
  });
  line(s, 36, y0 - 4, 36, yy - 12, { color: FLOW, lw: 1.5 });
  T(s, '↓ dependency direction', 36, yy - 4, 200, 14, { size: 8, color: FLOW, mono: true });

  h3(s, 'THREE PROPERTIES OF THIS STRUCTURE', 846, y0, 380, FLOW);
  bullets(s, [
    'Reading the envelope is enough to forward — relays never open the contents, so new message kinds do not make relaying heavier',
    'Registering a backend takes one file — entrypoints/agent/src/adapters/mod.rs: one name, one factory, one implementation',
    'mock is always built in — a whole fleet runs without GPUs to verify batching and ordering',
  ], 846, y0 + 22, 380, 170, { size: 9.5 });

  note(s, 846, y0 + 208, 380, 142,
    'The only surface an agent exposes is P4 over a socket. Whether an adapter talks to its backend over HTTP, ' +
    'over a pipe, or through function calls in the same process is invisible above it. ' +
    'That is the real reason backends can be swapped.');

  foot(s, 'Layer names map 1:1 to repository paths — layers/protocol, layers/agent, layers/adapters/adapter, layers/adapters/*, upstream llama.cpp');
}

/* ═══ 04 ② Agent topology ═══════════════════════════ */
{
  const s = slide('② Topology', 'Agents and nodes', 'One process per machine, several nodes inside it', 4,
    'There is no controller process. The path an agent uses to reach another agent is the same path it uses to answer the outside world.');
  const y0 = s.bodyTop;

  box(s, 54, y0 + 96, 150, 70, { fill: BNDW, line: BND, lw: 1.2 });
  T(s, 'OUTER', 54, y0 + 116, 150, 18, { size: 12, color: BND, bold: true, align: 'center' });
  T(s, 'decides placement/sessions', 54, y0 + 138, 150, 14, { size: 8, color: MUT, align: 'center' });

  const hosts = [
    ['machine A', ['node 0', 'node 1'], 'CUDA0 / CUDA1'],
    ['machine B', ['node 2', 'node 3'], 'ROCm0 / ROCm1'],
    ['machine C', ['node 4'], 'MTL0'],
  ];
  const hx = 268, hw = 300;
  hosts.forEach(([name, nodes, dev], i) => {
    const x = hx + i * (hw + 22);
    box(s, x, y0, hw, 272, { line: RULE2, dash: true });
    T(s, name, x + 14, y0 + 10, 160, 16, { size: 9, color: MUT, mono: true });
    T(s, dev, x + hw - 174, y0 + 10, 160, 16, { size: 8, color: MUT, mono: true, align: 'right' });
    box(s, x + 14, y0 + 34, hw - 28, 220, { fill: FLOWW, line: FLOW, lw: 1.2 });
    T(s, 'agent  (1 process)', x + 28, y0 + 46, 200, 16, { size: 9.5, color: INK, bold: true });
    T(s, 'socket · queue · worker pool', x + 28, y0 + 66, 200, 14, { size: 8, color: MUT });
    nodes.forEach((n, j) => {
      const ny = y0 + 90 + j * 76;
      box(s, x + 28, ny, hw - 56, 62, { fill: SURF, line: RULE2 });
      T(s, n, x + 42, ny + 10, 120, 16, { size: 9.5, color: INK, mono: true, bold: true });
      T(s, 'own queue + 1 adapter', x + 42, ny + 30, 200, 14, { size: 8, color: MUT });
      T(s, 'one layer range', x + 42, ny + 44, 200, 14, { size: 8, color: FLOW });
    });
    line(s, x + hw / 2, y0 + 290, x + hw / 2, y0 + 276, { color: FLOW, lw: 1.4 });
  });
  box(s, 268, y0 + 292, 944, 26, { fill: FLOWW, line: FLOW, lw: 1.2 });
  line(s, 129, y0 + 168, 129, y0 + 305, { color: BND, lw: 1.2, arrow: false });
  line(s, 129, y0 + 305, 264, y0 + 305, { color: BND, lw: 1.2 });
  T(s, 'agent ↔ agent uses the same P4 frames (TCP)', 268, y0 + 299, 944, 16, { size: 8.5, color: FLOW, align: 'center' });

  const dy = y0 + 336;
  const defs = [
    ['AGENT', 'One process per machine. It takes frames off the socket into a queue, and workers judge from the address alone whether a frame is theirs. If not, they pass it on whole.'],
    ['NODE', 'A logical execution unit inside an agent. It starts as just an id and becomes real when LOAD attaches an adapter. It has its own queue and holds its long work alone.'],
    ['OUTER', 'The requesting side outside. It decides which node takes which range and which nodes form a session. The agent only reports facts.'],
  ];
  defs.forEach(([k, v], i) => {
    const x = 54 + i * 396;
    h3(s, k, x, dy, 370);
    T(s, v, x, dy + 20, 370, 76, { size: 9.5, ls: 1.4 });
  });

  foot(s, 'Configurations run so far — 1 host with 8 stages, 2 hosts with 16 stages, 5 hosts with 6 stages. Node count is set by model size, KV capacity and legal cut points, not by the number of cards.');
}

/* ═══ 05 ② Model loading ═══════════════════════════════════ */
{
  const s = slide('② Topology', 'Many nodes into one pipeline', 'Load and pipeline have separate lifetimes', 5,
    'First each node is loaded on its own, then the order is installed. At load time, nodes know nothing of each other.');
  const y0 = s.bodyTop;

  // phase 1
  T(s, 'Phase 1  LOAD — an independent command per node', 54, y0, 560, 18, { size: 12, color: INK, bold: true });
  T(s, 'Each node loads only its layer range from the GGUF — no neighbors, order or session.',
    54, y0 + 24, 560, 32, { size: 9.5, color: MUT, ls: 1.4 });
  const ly = y0 + 66;
  [['node 0', '[0, 20)'], ['node 2', '[20, 40)'], ['node 4', '[40, 60)']].forEach(([n, r], i) => {
    const x = 54 + i * 190;
    box(s, x, ly, 172, 74, { fill: SURF2 });
    T(s, n, x + 14, ly + 12, 144, 16, { size: 9.5, color: INK, mono: true, bold: true });
    T(s, 'stage_begin·end', x + 14, ly + 32, 144, 14, { size: 7.5, color: MUT, mono: true });
    T(s, r, x + 14, ly + 48, 144, 16, { size: 10, color: FLOW, mono: true });
  });

  rule(s, 54, y0 + 168, 560);

  // phase 2
  T(s, 'Phase 2  SESSION — install the order', 54, y0 + 186, 560, 18, { size: 12, color: INK, bold: true });
  T(s, 'Sends all participants and each node\'s own index at once — only then is it a chain.',
    54, y0 + 210, 560, 32, { size: 9.5, color: MUT, ls: 1.4 });
  T(s, 'stages: [ {agent, node, generation}, … ]   +   stage_index',
    54, y0 + 248, 560, 18, { size: 9, color: FLOW, mono: true });

  const sy = y0 + 278;
  [['stage 0', 'head', 'f'], ['stage 1', 'middle', ''], ['stage 2', 'terminal', '']].forEach(([n, r, k], i) => {
    const x = 54 + i * 190;
    box(s, x, sy, 172, 58, { fill: k === 'f' ? FLOWW : SURF2, line: k === 'f' ? FLOW : RULE2, lw: k ? 1.2 : 1 });
    T(s, n, x + 14, sy + 10, 144, 16, { size: 9.5, color: INK, mono: true, bold: true });
    T(s, r, x + 14, sy + 30, 144, 16, { size: 9, color: k === 'f' ? FLOW : MUT });
    if (i > 0) line(s, x - 16, sy + 29, x - 2, sy + 29, { color: FLOW, lw: 1.4 });
  });

  h3(s, 'WHY THIS SPLIT PAYS OFF', 660, y0, 566, FLOW);
  bullets(s, [
    'Several sessions can be built on the same load. Reordering or reopening needs no reload',
    'Unloading a node (UNLOAD) invalidates every session bound to that load generation — the generation number enforces it',
    'The head owns settlement and output approval; the terminal produces tokens. They are different nodes',
    'first/previous/next/terminal relations between stages derive only from the installed order. Nodes do not decide them',
  ], 660, y0 + 22, 566, 170, { size: 10 });

  note(s, 660, y0 + 216, 566, 120,
    'Where a node sits in the chain is a property of the session, not of the node. ' +
    'The same node can take different positions in different sessions, so changing placement is a configuration change, not a reload.', 'f');

  foot(s, 'Field names are exactly as on the wire — stage_begin/stage_end (load), stages[]/stage_index (session), load_generation (generation)');
}

/* ═══ 06 ③ Nodes and concrete adapters ══════════════════════════ */
{
  const s = slide('③ Adapters', 'How a node picks a backend', 'A node knows one interface; everything below it is swappable', 6,
    'Attaching a backend costs one name, one factory and one implementation. The envelope, queue, worker and node code above it is identical for every backend.');
  const y0 = s.bodyTop;

  box(s, 500, y0, 280, 56, { fill: SURF2 });
  T(s, 'node', 500, y0 + 18, 280, 20, { size: 12, color: INK, mono: true, bold: true, align: 'center' });
  line(s, 640, y0 + 58, 640, y0 + 84, { color: FLOW, lw: 1.5 });
  box(s, 440, y0 + 86, 400, 46, { fill: FLOWW, line: FLOW, lw: 1.3 });
  T(s, 'Adapter contract — submit · complete · cancel', 440, y0 + 100, 400, 18, { size: 11, color: INK, bold: true, align: 'center' });
  T(s, 'No backend names appear here' + ' — the names below pick the implementation at LOAD', 340, y0 + 138, 600, 14, { size: 8, color: MUT, align: 'center' });

  const ad = [
    ['mock', 'arithmetic only, no device', 'f'],
    ['mock-instant', 'instant replies; order tests', 'f'],
    ['llamacpp', 'llama-server (HTTP)', ''],
    ['vllm', 'vLLM server', ''],
    ['sglang', 'SGLang server', ''],
    ['llamacpp-staged', 'layer-split execution', 'b'],
  ];
  const aw = 178, agap = 20;
  ad.forEach(([n, d, k], i) => {
    const x = 54 + i * (aw + agap);
    line(s, 640, y0 + 158, x + aw / 2, y0 + 186, { color: MUT, lw: 1 });
    box(s, x, y0 + 188, aw, 68, {
      fill: k === 'b' ? BNDW : k === 'f' ? FLOWW : SURF2,
      line: k === 'b' ? BND : k === 'f' ? FLOW : RULE2, lw: k ? 1.2 : 1,
    });
    T(s, n, x + 14, y0 + 200, aw - 28, 16, { size: 9, color: k === 'b' ? BND : INK, mono: true, bold: true });
    T(s, d, x + 14, y0 + 222, aw - 28, 28, { size: 8.5, color: MUT, ls: 1.3 });
  });

  const dy = y0 + 282;
  h3(s, 'TWO SHAPES OF llama.cpp', 54, dy, 560, FLOW);
  bullets(s, [
    'llamacpp — one process holds the whole model and answers over HTTP. One node; chain length 1',
    'llamacpp-staged — cuts the model into layer ranges across nodes. Chain length = node count',
    'vLLM and SGLang attach at the same spot. Backends that lay out the model themselves join with chain length 1',
  ], 54, dy + 22, 560, 110, { size: 10 });

  note(s, 660, dy, 566, 132,
    'It matters in practice that mock is always in the build. With no GPU and no model files, a whole fleet can be started ' +
    'and routing, ordering, batching and cancellation run end to end. Compute is replaced by arithmetic, so results are deterministic, ' +
    'and if two runs differ, the difference is in P4.', 'f');

  foot(s, 'Registration point entrypoints/agent/src/adapters/mod.rs — llamacpp-staged is exposed only on hosts with a prepared executable (without one, the capability is absent; nothing runs in its place)');
}

/* ═══ 07 ④ Platform coverage through llama.cpp ═════════════════════ */
{
  const s = slide('④ Backends', 'Below the llama adapter', 'Platform support is work llama.cpp already did, not P4', 7,
    'The adapter deals with one pinned llama.cpp. Which device does the computing below it is decided by ggml\'s backend registry.');
  const y0 = s.bodyTop;

  const stack = [
    ['llamacpp-staged adapter', 'P4 side — submission · ledger · settlement', 'f'],
    ['native stage runtime', 'layer-range execution and the stage protocol', ''],
    ['llama.cpp', 'model · graph · memory semantics', 'q'],
    ['ggml backend registry', 'picks the execution device', 'q'],
  ];
  let yy = y0;
  stack.forEach(([a, b, k]) => {
    box(s, 54, yy, 430, 44, {
      fill: k === 'f' ? FLOWW : k === 'q' ? SURF3 : SURF2,
      line: k === 'f' ? FLOW : RULE2, lw: k === 'f' ? 1.2 : 1,
    });
    T(s, a, 70, yy + 8, 400, 16, { size: 9.5, color: INK, bold: true });
    T(s, b, 70, yy + 26, 400, 14, { size: 8, color: MUT });
    yy += 52;
  });

  T(s, 'The 18 ggml backends in the pinned upstream (451b89bae)', 528, y0, 698, 16,
    { size: 9, color: FLOW, mono: true, bold: true });
  const bks = ['CUDA', 'HIP / ROCm', 'Metal', 'Vulkan', 'SYCL', 'OpenCL',
    'CPU', 'BLAS', 'CANN', 'MUSA', 'WebGPU', 'Hexagon',
    'OpenVINO', 'zDNN', 'ZenDNN', 'virtGPU', 'ET', 'RPC'];
  const run = ['CUDA', 'HIP / ROCm', 'Metal', 'CPU'];
  bks.forEach((b, i) => {
    const cx = 528 + (i % 6) * 118, cy = y0 + 26 + Math.floor(i / 6) * 46;
    const hot = run.includes(b);
    box(s, cx, cy, 108, 36, { fill: hot ? BNDW : SURF2, line: hot ? BND : RULE2, lw: hot ? 1.2 : 1 });
    T(s, b, cx, cy + 11, 108, 14, { size: 8, color: hot ? BND : INK2, mono: true, align: 'center' });
  });
  T(s, 'Orange = backends this repository has run on real hardware. The rest is llama.cpp\'s list and does not mean P4 has verified them.',
    528, y0 + 168, 698, 28, { size: 8.5, color: MUT, ls: 1.35 });

  const ny = y0 + 212;
  note(s, 54, ny, 586, 136,
    'llama.cpp moves fast, so the adapter is pinned to one commit, and the patches that commit needs ' +
    'are collected in a single compat/<commit>/ directory. The official checkout is left alone; the prepare script builds a separate worktree, ' +
    'checks hashes and then applies the patches. 10 commits have been pinned so far.');
  note(s, 660, ny, 566, 136,
    'Thanks to this structure, “support a new model” or “support a new device” is mostly not P4\'s job. ' +
    'If llama.cpp supports it, the work becomes a pin bump, and its impact ends inside the compat directory. ' +
    'P4\'s envelope, queue, node and batching code stays as it is.', 'f');

  foot(s, 'Measured — ls upstream/ggml/src/ggml-* gives 18, ls staged/compat gives 10 pins, latest pin 451b89bae (27 patches) @HEAD f57543c8d');
}

/* ═══ 08 ⑤ What distributed layer loading looks like ════════════════ */
{
  const s = slide('⑤ Execution', 'What a load looks like', 'Each node holds the weights and KV for its own range', 8,
    'An 80-layer model split across four nodes. Each node knows only its own range and holds only that range\'s memory.');
  const y0 = s.bodyTop;

  const nodes = [
    ['node 0', '[0, 20)', 'CUDA0', '173.5 MB'],
    ['node 1', '[20, 40)', 'CUDA1', '63.3 MB'],
    ['node 2', '[40, 60)', 'ROCm0', '157.7 MB'],
    ['node 3', '[60, 80)', 'MTL0', '126.1 MB'],
  ];
  const nw = 276, ngap = 22;
  nodes.forEach(([n, r, dev, kv], i) => {
    const x = 54 + i * (nw + ngap);
    box(s, x, y0, nw, 210, { fill: SURF2 });
    T(s, n, x + 16, y0 + 12, 120, 16, { size: 10, color: INK, mono: true, bold: true });
    T(s, dev, x + nw - 136, y0 + 12, 120, 16, { size: 8.5, color: BND, mono: true, align: 'right' });
    T(s, 'layers ' + r, x + 16, y0 + 34, 200, 16, { size: 10, color: FLOW, mono: true });
    rule(s, x + 16, y0 + 58, nw - 32);
    box(s, x + 16, y0 + 70, nw - 32, 38, { fill: SURF, line: RULE2 });
    T(s, 'weights: own range only', x + 28, y0 + 81, 200, 16, { size: 8.5, color: INK2 });
    box(s, x + 16, y0 + 114, nw - 32, 38, { fill: SURF, line: RULE2 });
    T(s, 'KV cache: own range only', x + 28, y0 + 125, 150, 16, { size: 8.5, color: INK2 });
    T(s, kv, x + nw - 116, y0 + 125, 100, 16, { size: 8.5, color: INK, mono: true, align: 'right' });
    box(s, x + 16, y0 + 158, nw - 32, 38, { fill: SURF, line: RULE2 });
    T(s, 'compute buffer', x + 28, y0 + 169, 200, 16, { size: 8.5, color: INK2 });
    if (i > 0) line(s, x - 18, y0 + 105, x - 2, y0 + 105, { color: FLOW, lw: 1.4 });
  });
  T(s, 'KV cost differs per node even at the same n_ctx — because layer composition differs per range. The most expensive node sets the pipeline\'s limit.',
    54, y0 + 222, 1172, 16, { size: 9, color: MUT });

  const dy = y0 + 254;
  h3(s, 'CHECK THAT PLAN MATCHES REALITY', 54, dy, 560, FLOW);
  T(s, '--expect-layer-device  begin:end:name', 54, dy + 22, 560, 18, { size: 10, color: FLOW, mono: true });
  bullets(s, [
    'Checks that the declared ranges cover the whole cut with no gaps or overlaps',
    'Queries device placement twice, before load (PLAN) and after (LOAD), and compares. Matching total memory alone does not pass',
    'Ranges meant to stay on CPU are declared explicitly too — nothing silently falls back to CPU',
  ], 54, dy + 48, 560, 96, { size: 9.5 });

  note(s, 660, dy, 566, 144,
    '“Node = one GPU” is not a rule. One device can host several stages, one stage can use several devices, ' +
    'and some ranges can stay on CPU. Node count is set by model size, KV capacity and where that model can legally be cut, ' +
    'not by the number of cards.');

  foot(s, 'KV figures were measured per node at the same n_ctx and vary with model, cut and backend. Device names are the real names ggml reports.');
}

/* ═══ 09 ⑤ What crosses the network ══════════════════════════ */
{
  const s = slide('⑤ Execution', 'What moves during inference', 'Only intermediate values cross the network', 9,
    'Neither weights nor the KV cache ever leave a node. What crosses in one step is the tensor bundle at the cut boundary — the in-flight batch.');
  const y0 = s.bodyTop;

  const sx = [140, 520, 900], sw = 240;
  sx.forEach((x, i) => {
    box(s, x, y0 + 74, sw, 122, { fill: SURF2 });
    T(s, ['stage 0 (head)', 'stage 1', 'stage 2 (terminal)'][i], x + 16, y0 + 86, sw - 32, 16,
      { size: 9.5, color: INK, mono: true, bold: true });
    rule(s, x + 16, y0 + 110, sw - 32);
    T(s, 'weights', x + 16, y0 + 120, 100, 14, { size: 8.5, color: MUT });
    T(s, 'does not move', x + sw - 136, y0 + 120, 120, 14, { size: 8.5, color: INK2, align: 'right' });
    T(s, 'KV cache', x + 16, y0 + 142, 100, 14, { size: 8.5, color: MUT });
    T(s, 'does not move', x + sw - 136, y0 + 142, 120, 14, { size: 8.5, color: INK2, align: 'right' });
    T(s, 'compute buffer', x + 16, y0 + 164, 120, 14, { size: 8.5, color: MUT });
    T(s, 'does not move', x + sw - 136, y0 + 164, 120, 14, { size: 8.5, color: INK2, align: 'right' });
    if (i > 0) {
      line(s, sx[i - 1] + sw + 6, y0 + 56, x - 6, y0 + 56, { color: FLOW, lw: 1.8 });
      T(s, 'boundary tensor set', sx[i - 1] + sw, y0 + 32, 146, 14, { size: 8.5, color: FLOW, mono: true, align: 'center' });
    }
  });
  T(s, 'What crosses — the in-flight batch', 140, y0 + 6, 400, 16, { size: 10, color: FLOW, bold: true });
  line(s, 1146, y0 + 135, 1186, y0 + 135, { color: BND, lw: 1.4, arrow: false });
  line(s, 1186, y0 + 135, 1186, y0 + 214, { color: BND, lw: 1.4, arrow: false });
  line(s, 1186, y0 + 214, 200, y0 + 214, { color: BND, lw: 1.4, dash: true, arrow: false });
  line(s, 200, y0 + 214, 200, y0 + 198, { color: BND, lw: 1.4 });
  T(s, 'Tokens made by the terminal go back to the head and leave only after approval', 300, y0 + 220, 800, 16,
    { size: 8.5, color: BND, align: 'center' });

  const dy = y0 + 252;
  h3(s, 'THE SIZE GAP SHAPES THE DESIGN', 54, dy, 560, FLOW);
  const rows = [
    ['Weights', 'tens–hundreds of GB', 'loaded once, never moved after'],
    ['KV cache', 'tens–hundreds of MB', 'per node/request; never moves, so a request is bound to the nodes with its KV'],
    ['Per-step send', 'a few boundary tensors', 'gemma-4: 31·27·23 tensors, 81 per step. Qwen family: 1'],
  ];
  rows.forEach(([a, b, c], i) => {
    const yy = dy + 24 + i * 44;
    T(s, a, 54, yy, 110, 16, { size: 9.5, color: INK, bold: true });
    T(s, b, 170, yy, 190, 16, { size: 9.5, color: FLOW, mono: true });
    T(s, c, 370, yy, 244, 32, { size: 8.5, color: MUT, ls: 1.3 });
    if (i < 2) rule(s, 54, yy + 34, 560);
  });

  note(s, 660, dy, 566, 88,
    'So the bandwidth needed between nodes is far smaller than for tensor parallelism. Only the tensors at the cut cross, and only at the boundary, ' +
    'so ordinary networks can tie several machines together.', 'f');
  note(s, 660, dy + 100, 566, 108,
    'There is a price. A request passes the stages in turn, so latency grows with stage count, and running one request at a time ' +
    'leaves earlier stages idle. Filling that idle time by overlapping many requests is what this layer keeps refining.');

  foot(s, 'The unit of transfer is the tensor bundle (capsule) at a stage boundary. Tensor count is a constant set by model structure, and tensors pointing at the same value are not carried twice.');
}

/* ═══ 10 ⑥ Batching — what goes in ═══════════════════════ */
{
  const s = slide('⑥ Batching', 'What goes into one batch', 'Filling idle time depends entirely on batch selection', 10,
    'With one request at a time, earlier pipeline stages sit idle. At every issue opportunity the adapter picks which requests and how many rows go into a batch, and that choice is a pure computation that commits no resources.');
  const y0 = s.bodyTop;

  T(s, 'The order that fills one logical batch', 54, y0, 640, 18, { size: 11.5, color: INK, bold: true });
  const bx = 54, bw = 640;
  box(s, bx, y0 + 28, bw, 40, { fill: SURF2 });
  const cells = [['D', FLOW], ['D', FLOW], ['D', FLOW], ['P', BND], ['P', BND], ['P', BND], ['P', BND], ['P', BND]];
  cells.forEach(([t, c], i) => {
    const w = (bw - 16) / cells.length;
    box(s, bx + 8 + i * w, y0 + 36, w - 4, 24, { fill: c === FLOW ? FLOWW : BNDW, line: c, lw: 1 });
    T(s, t, bx + 8 + i * w, y0 + 42, w - 4, 14, { size: 8.5, color: c, mono: true, align: 'center' });
  });
  T(s, 'D = decode, 1 row each, first', 54, y0 + 74, 300, 14, { size: 8.5, color: FLOW });
  T(s, 'P = prefill water-fills the remaining rows in rotation', 340, y0 + 74, 354, 14, { size: 8.5, color: BND });
  bullets(s, [
    'Attention models fill the logical batch to llama_n_batch; llama.cpp splits at n_ubatch',
    'recurrent/hybrid need one width per sequence, so each call makes exactly one physical UBATCH',
    'Verify·Replay is one indivisible transaction — it must fit in one physical UBATCH',
  ], 54, y0 + 96, 640, 84, { size: 9.5 });

  h3(s, 'WIDTH IS SET BY THE POPULATION, NOT “FREE SLOTS NOW”', 740, y0, 486, FLOW);
  box(s, 740, y0 + 22, 486, 84, { fill: FLOWW, line: FLOW, lw: 1.2 });
  T(s, 'ready + in flight + waiting  ÷  window', 756, y0 + 34, 454, 16, { size: 9.5, color: INK, mono: true, bold: true });
  T(s, 'One request briefly returning does not widen the group. A fully sent but unsettled request ' +
    'stays in its cohort and widens no new group.', 756, y0 + 56, 454, 44, { size: 8.5, color: INK2, ls: 1.35 });

  h3(s, 'WHILE GENERATION IS LIVE, PREFILL GETS A QUANTUM', 740, y0 + 118, 486, BND);
  box(s, 740, y0 + 140, 486, 60, { fill: BNDW, line: BND, lw: 1.2 });
  T(s, 'decode in progress → prefill rows = mixed_prefill_rows', 756, y0 + 150, 454, 16, { size: 9, color: INK, mono: true });
  T(s, 'Pure prefill gets the full token budget. Tests pin 128 rows while generating, 512 for pure prefill. ' +
    'A non-preemptive work unit, not preemption.', 756, y0 + 170, 454, 30, { size: 8.5, color: INK2, ls: 1.35 });

  const ny = y0 + 208;
  box(s, 54, ny, 640, 108, { fill: SURF2 });
  T(s, 'PREFILL_PATIENCE = 8', 72, ny + 14, 400, 16, { size: 10, color: INK, mono: true, bold: true });
  T(s, 'If a prompt still waits after 8 straight decode batches, the next batch is the prompt\'s turn. A cap, not a quota.',
    72, ny + 36, 604, 30, { size: 8.5, color: INK2, ls: 1.35 });
  T(s, 'The code comment verbatim — “8 is not a measured value. It is the smallest thing that makes the bound exist; nothing has judged it against throughput on real hardware.”',
    72, ny + 70, 604, 30, { size: 8.5, color: MUT, ls: 1.35 });
  note(s, 740, ny, 486, 108,
    'A prepared choice spends no fairness until it is accepted. Rejected or cancelled candidates use no turn, ' +
    'and foreign or stale plans are refused by name. The selection layer is pure — neither KV nor execution rights are committed here.', 'f');

  foot(s, 'Measured v2/scheduler.rs(Phase·Demand·PREFILL_PATIENCE:94·371·627·PreparedPlan), v2/scheduler/pipeline.rs:70-113(population-based cohort width and generation-first quantum)');
}

/* ═══ 11 ⑥ Batching — when to send ═════════════════════ */
{
  const s = slide('⑥ Batching', 'When to send', 'Measure stage service time, then decide to admit or defer', 11,
    'Sending a chosen batch at once queues it up at the tail; always waiting loses depth. So real elapsed times are collected to predict the cost of the next piece of work.');
  const y0 = s.bodyTop;

  const flow = [
    ['Observe', 'samples per-stage Frame\nround-trip times', FLOW],
    ['Predict', 'profile-based per-stage\ntime for the candidate', ''],
    ['Project', 'also adds uncommitted\nin-flight work, FIFO\nacross all stages', ''],
    ['Verdict', 'within budget → admit, over → defer', BND],
  ];
  flow.forEach(([a, b, c], i) => {
    const x = 54 + i * 172;
    box(s, x, y0, 156, 92, { fill: c === FLOW ? FLOWW : c === BND ? BNDW : SURF2, line: c || RULE2, lw: c ? 1.2 : 1 });
    T(s, a, x + 12, y0 + 12, 132, 16, { size: 10, color: c || INK, bold: true });
    T(s, b, x + 12, y0 + 34, 132, 48, { size: 8, color: MUT, ls: 1.4 });
    if (i > 0) line(s, x - 14, y0 + 46, x - 2, y0 + 46, { color: FLOW, lw: 1.4 });
  });

  T(s, 'There are seven verdicts', 54, y0 + 106, 700, 18, { size: 11.5, color: INK, bold: true });
  const verdicts = [
    ['PurePrefill', 'no generation running — full width'],
    ['DecodeOnly', 'no prefill rows'],
    ['Cold', 'a batch I did not issue is open — no profile'],
    ['CalibrationWait', 'not enough samples yet'],
    ['Admit', 'fits within the budget'],
    ['DeferPrefill', 'over budget — no prefill this time'],
    ['ProgressProbe', 'issues one quantum even when the target cannot be met, to avoid starvation'],
  ];
  verdicts.forEach(([a, b], i) => {
    const yy = y0 + 132 + i * 26;
    T(s, a, 54, yy, 150, 14, { size: 8.5, color: i === 5 ? BLK : FLOW, mono: true });
    T(s, b, 212, yy, 482, 14, { size: 8.5, color: INK2 });
  });

  h3(s, 'THE SWITCH AND ITS LIMITS', 740, y0 + 106, 486, FLOW);
  box(s, 740, y0 + 128, 486, 56, { fill: SURF2 });
  T(s, 'P4_STAGED_PREFILL_SERVICE_MS', 756, y0 + 138, 454, 14, { size: 8.5, color: INK, mono: true });
  T(s, 'Given in ms, it becomes a µs budget. Unset, the policy itself is off.', 756, y0 + 158, 454, 20, { size: 8.5, color: MUT });
  note(s, 740, y0 + 194, 486, 118,
    'The code pins this down itself — this is a prediction policy, not execution, KV authority, transfer credit or a response-time guarantee. ' +
    'The projection leaves out unmeasured transfer and return delays and promises nothing about the inter-token gap a client sees. ' +
    'The delay for gathering decode-only work is capped at 2 ms.', 'x');

  note(s, 54, y0 + 320, 686, 96,
    'Separately, when enough batches are in flight, the head holds the plan briefly. The comment records why, in numbers — ' +
    'batches arriving while the tail was busy waited p50 128 ms behind the previous one, 63% of all. A batch costs about 55 ms fixed from tail to first layer. ' +
    'With room, even a thin batch goes at once — an earlier try that waited for width regardless of room lost 26%.');

  foot(s, 'Measured v2/scheduler/service.rs:1-60·324-412(samples, prediction, seven verdicts), v2/node/worker/service.rs:8-25(budget knob), v2/node/worker/drive.rs:55-80(deferred issue) · all these knobs default to off, and the figures above ground a hypothesis, not a promotion result');
}

/* ═══ 12 ⑦ KV cache persistence ═════════════════════════════ */
{
  const s = slide('⑦ Persistence', 'KV cache to files', 'Each node saves and restores the KV for its own range', 12,
    'Each stage holds the KV for its own layer range, so save and restore happen per node. It uses llama.cpp\'s public state API as is.');
  const y0 = s.bodyTop;

  const steps = [
    ['Persist', 'llama_state_seq_get_size_ext\nllama_state_seq_get_data_ext', FLOW],
    ['<kv-root>/<key>.lkv', 'manifest + state bytes\nwritten with a checksum', BND],
    ['Cell reclaim', 'llama_memory_seq_rm\nfrees KV cells after saving', ''],
  ];
  steps.forEach(([a, b, c], i) => {
    const x = 54 + i * 236;
    box(s, x, y0, 216, 88, { fill: c === FLOW ? FLOWW : c === BND ? BNDW : SURF2, line: c || RULE2, lw: c ? 1.2 : 1 });
    T(s, a, x + 14, y0 + 12, 188, 16, { size: 9.5, color: c || INK, mono: true, bold: true });
    T(s, b, x + 14, y0 + 34, 188, 44, { size: 8, color: MUT, mono: true, ls: 1.4 });
    if (i > 0) line(s, x - 18, y0 + 44, x - 2, y0 + 44, { color: FLOW, lw: 1.4 });
  });
  line(s, 162, y0 + 100, 162, y0 + 124, { color: BND, lw: 1.4, arrow: false });
  line(s, 162, y0 + 124, 640, y0 + 124, { color: BND, lw: 1.4, arrow: false });
  line(s, 640, y0 + 124, 640, y0 + 100, { color: BND, lw: 1.4 });
  T(s, 'Restore — reads the file back via llama_state_seq_set_data_ext; no next decode until that upload finishes',
    54, y0 + 132, 700, 16, { size: 8.5, color: BND });

  h3(s, 'THE MANIFEST DECIDES WHAT MAY BE RESTORED', 740, y0, 486, FLOW);
  const man = [
    ['build_identity', 'which build produced the state'],
    ['runtime_identity', 'which runtime configuration'],
    ['context_identity', 'n_ctx · n_seq · kv_unified …'],
    ['kv_format', 'K=type ; V=type ; flags'],
    ['token_position', 'the position the state reaches'],
    ['checksum · bytes', 'whether the content is intact'],
  ].map(([a, b]) => [
    { text: a, options: { fontFace: M, fontSize: 8.5, color: INK } },
    { text: b, options: { fontSize: 9 } },
  ]);
  table(s, man, 740, y0 + 22, 486, [148, 338], { size: 9 });

  const ny = y0 + 180;
  bullets(s, [
    'On restore, a mismatch in any of the six is rejected — state from another build, context or KV type is never revived',
    'Without --kv-root the feature itself is reported as off. There is no silent memory-only path',
    'A runtime whose memory was dirtied by a failed decode refuses save, restore and delete. llama.cpp does not say which sequence broke, so possibly torn state is never written to a file',
    'A single sequence state over 128 MiB is refused',
  ], 54, ny, 700, 132, { size: 9.5 });

  note(s, 776, ny, 450, 132,
    'This is storage for continuing conversations, not a fault-recovery mechanism. Since KV cells are freed after saving, it is used to move ' +
    'mid-conversation state of long chats off the device and reclaim room. Whether it can be read back is decided by the six items on the left.');

  foot(s, 'Measured server/src/runtime/llama_stage_runtime_kv.cpp:113-175(save·restore), state_store.cpp:153-360(manifest check·checksum), main.cpp:284-293(capability off without --kv-root)');
}

/* ═══ 13 ⑧ MTP and speculative decoding ═══════════════════════════ */
{
  const s = slide('⑧ Speculative', 'MTP and other methods', 'Not auto-enabled when supported — rejected when not supported', 13,
    'On the staged path, proposal, verification and rollback are state that crosses node boundaries. So it is deliberately built the other way round: new upstream methods do not switch on automatically.');
  const y0 = s.bodyTop;

  note(s, 54, y0, 700, 92,
    '“If llama.cpp supports it, it is supported automatically” does not hold on this path. Supported methods are listed in one place, and a value not on that list ' +
    'fails LOAD. The comment explains why — if call sites scanned the upstream enum, a new enumerator would silently be misclassified as supported.', 'x');

  T(s, 'Implemented', 54, y0 + 112, 340, 18, { size: 11.5, color: INK, bold: true });
  box(s, 54, y0 + 138, 340, 54, { fill: FLOWW, line: FLOW, lw: 1.2 });
  T(s, 'COMMON_SPECULATIVE_TYPE_DRAFT_MTP', 68, y0 + 150, 312, 16, { size: 8, color: FLOW, mono: true, bold: true });
  T(s, 'one method: the model proposes its next tokens', 68, y0 + 170, 312, 14, { size: 8.5, color: MUT });

  T(s, 'Rejected', 414, y0 + 112, 340, 18, { size: 11.5, color: INK, bold: true });
  box(s, 414, y0 + 138, 340, 26, { fill: BLKW, line: BLK, lw: 1 });
  T(s, 'if a separate draft model is required', 428, y0 + 143, 312, 14, { size: 8.5, color: INK2 });
  box(s, 414, y0 + 168, 340, 26, { fill: BLKW, line: BLK, lw: 1 });
  T(s, 'if it is any other speculative method', 428, y0 + 173, 312, 14, { size: 8.5, color: INK2 });
  T(s, 'LOAD fails with CAPABILITY_UNAVAILABLE and returns the two reasons as distinct names', 414, y0 + 204, 340, 32,
    { size: 8.5, color: BLK, ls: 1.35 });

  h3(s, 'WHAT EXISTS SPECIFICALLY FOR MTP', 776, y0 + 112, 450, FLOW);
  bullets(s, [
    'Verify·Replay are first-class scheduler phases and indivisible atomic transactions',
    'The tail stage proposes and tracks proposal state per sequence',
    'The memory plan also measures draft-context usage — before load',
    'compat patches list the MTP tail stage and speculative sequence lifetime as separate items',
  ], 776, y0 + 134, 450, 118, { size: 9.5 });

  note(s, 54, y0 + 252, 1172, 78,
    'The truly automatic part lies elsewhere — the device backend. The stage runtime calls ggml_backend_load_all() and that is all; ' +
    'it has no CUDA·Vulkan·HIP·Metal·OpenCL branches at all. Model architecture, quantization, samplers and grammars are likewise used from llama.cpp as is. ' +
    'This is where what simply follows upstream splits from what crosses a boundary and needs explicit implementation.', 'f');

  foot(s, 'Measured compat/p4_llama_compat.cpp:232-248(supported list), runtime/llama_stage_runtime.cpp:47-58(LOAD rejection), :64-66(backend delegation), v2/scheduler.rs(Verify·Replay atomicity)');
}

/* ═══ 14 ⑨ Caches beyond KV ═══════════════════════════════ */
{
  const s = slide('⑨ Memory', 'Models that hold more than KV', 'Only “declared” extra-cache implementations can be split', 14,
    'llama.cpp memory is not just one plain KV. Sliding windows, recurrent state, sparse attention and hybrids each hold a different store.');
  const y0 = s.bodyTop;

  const mem = [
    ['llama-kv-cache', 1], ['llama-kv-cache-iswa', 1], ['llama-memory-recurrent', 1], ['llama-memory-hybrid', 1],
    ['llama-kv-cache-dsa', 0], ['llama-kv-cache-dsa-iswa', 0], ['llama-kv-cache-dsv4', 0], ['llama-kv-cache-msa', 0],
    ['llama-memory-hybrid-idx', 0], ['llama-memory-hybrid-iswa', 0],
  ];
  T(s, 'The 10 memory implementations in the pinned upstream', 54, y0, 700, 16, { size: 9, color: FLOW, mono: true, bold: true });
  mem.forEach(([n, ok], i) => {
    const x = 54 + (i % 4) * 178, yy = y0 + 24 + Math.floor(i / 4) * 44;
    box(s, x, yy, 166, 34, { fill: ok ? FLOWW : SURF2, line: ok ? FLOW : RULE2, lw: ok ? 1.2 : 1 });
    T(s, n, x + 10, yy + 10, 146, 14, { size: 7.5, color: ok ? FLOW : MUT, mono: true });
  });
  T(s, 'Only the 4 blue ones declare stage residency; the rest are rejected as partial stages rather than silently wrong.',
    54, y0 + 160, 700, 16, { size: 8.5, color: MUT });

  h3(s, 'THE GATE HAS TWO LAYERS', 776, y0, 450, FLOW);
  box(s, 776, y0 + 22, 450, 64, { fill: SURF2 });
  T(s, 'linkcpp_stage_residency_supported', 790, y0 + 32, 422, 14, { size: 8, color: INK, mono: true });
  T(s, '= false  (the default is refusal)', 790, y0 + 50, 422, 14, { size: 8.5, color: BLK, mono: true, bold: true });
  T(s, 'splitting requires the implementation to override it to true', 790, y0 + 68, 422, 14, { size: 8, color: MUT });
  bullets(s, [
    'Once in the factory as a compile-time constant — a partial stage without the declaration never gets memory at all',
    'Again at context creation via a virtual call — it throws “does not declare stage-local residency support”',
    'iSWA has no store of its own and delegates to base and swa caches; their constructors refuse boundaries that split reused KV regions',
  ], 776, y0 + 98, 450, 110, { size: 9 });

  const ny = y0 + 224;
  T(s, 'Computed per device before load', 54, ny, 700, 18, { size: 11.5, color: INK, bold: true });
  const cols = [['model', 'weights'], ['context', 'KV and other state'], ['compute', 'execution buffers']];
  cols.forEach(([a, b], i) => {
    const x = 54 + i * 236;
    box(s, x, ny + 26, 216, 54, { fill: SURF2 });
    T(s, a, x + 14, ny + 36, 188, 16, { size: 9.5, color: FLOW, mono: true, bold: true });
    T(s, b, x + 14, ny + 56, 188, 14, { size: 8.5, color: MUT });
  });
  T(s, 'The sum is checked against free device memory; if it does not fit, LOAD fails — before allocating.',
    54, ny + 90, 700, 20, { size: 8.5, color: MUT });

  note(s, 776, ny, 450, 116,
    'So “extra-cache models are handled too” is only half true. Extra stores are planned and stage-resident only for the 4 declared kinds; ' +
    'sparse-attention families like DSA·DSV4·MSA have no declaration yet, so splitting them is rejected. Loaded whole on one node, they run as upstream does.', 'x');

  foot(s, 'Measured: 10 memory implementations in upstream/src, compat/0016·0017·0022 patches (4 declarations and default false), runtime/stage_memory_plan.hpp:56-98 (model·context·compute per device)');
}

/* ═══ 15 Summary ═══════════════════════════════════════════ */
{
  const s = slide('Summary', 'From a developer\'s point of view', 'What this structure actually gives you', 15);
  const y0 = s.bodyTop;

  const cards = [
    ['①', 'Run big models by adding nodes', 'One GPU or one machine no longer caps model size. Cutting by layer range means adding a node adds only one more boundary of communication.', FLOW],
    ['②', 'Swap backends', 'A node knows only the adapter contract. Whether llama.cpp, vLLM or an in-house engine, registering one name and one implementation changes not a line of the layers above.', FLOW],
    ['③', 'Borrow platform support', 'Device support for CUDA, ROCm, Metal, Vulkan, CPU and more is already in llama.cpp. P4 rides on top, and upstream churn ends inside the pin directory.', BND],
    ['④', 'Point at the slow spot', 'Agent queue depth and the number running inside a node are reported separately. When things slow down, observed values show whether P4 or the backend is holding the work.', BND],
  ];
  cards.forEach(([n, t, d, c], i) => {
    const x = 54 + (i % 2) * 596, y = y0 + Math.floor(i / 2) * 168;
    box(s, x, y, 576, 148, { fill: SURF2, line: null });
    T(s, n, x + 24, y + 20, 40, 20, { size: 13, color: c, mono: true, bold: true });
    T(s, t, x + 24, y + 48, 528, 22, { size: 13, color: INK, bold: true });
    T(s, d, x + 24, y + 78, 528, 60, { size: 9.5, color: INK2, ls: 1.45 });
  });

  note(s, 54, y0 + 340, 1172, 74,
    'Boundaries, stated plainly — there is no TLS, authentication or authorization. The design trusts addresses to identify themselves, so use it only inside a trusted network. ' +
    'There is no durable state either. When an agent restarts its nodes are gone, and OUTER holds the record of what should exist.', 'x');

  foot(s, 'This deck explains structure. Performance figures such as throughput and latency are owned by separate, condition-bound measurement records; the numbers here are measured examples used to explain the structure.');
}

const out = process.argv[2] || 'p4-intro.pptx';
pres.writeFile({ fileName: out }).then(() => console.log('wrote', out));
