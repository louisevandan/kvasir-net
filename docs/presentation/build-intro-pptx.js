// P4 — 일반 개발자 대상 구조 설명 덱 (Google Slides 이식용 pptx)
// 1280x720 px 설계 공간을 13.333in x 7.5in 에 1:1 이식한다. 1px = 1/96in.
const P = require('pptxgenjs');

const pres = new P();
pres.defineLayout({ name: 'P4', width: 13.3333, height: 7.5 });
pres.layout = 'P4';
pres.author = 'P4';
pres.title = 'P4 구조 설명';

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
  T(s, `${String(num).padStart(2, '0')} / 14`, 1026, 36, 200, 14,
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

/* ═══ 01 표지 ═══════════════════════════════════════════ */
{
  const s = pres.addSlide();
  s.background = { color: DARK };
  T(s, 'P4  —  DISTRIBUTED INFERENCE RUNTIME LAYER', 60, 56, 900, 16,
    { size: 8.5, color: '5FB5C9', mono: true, cs: 1.8, bold: true });
  s.addShape('rect', { x: I(60), y: I(130), w: I(74), h: I(3), fill: { color: '5FB5C9' }, line: { type: 'none' } });
  T(s, '여러 대의 컴퓨터를 한 대처럼 써서\n큰 모델을 돌린다', 60, 156, 1000, 126,
    { size: 40, color: 'F2F5F8', bold: true, ls: 1.16 });
  T(s, 'GPU 한 장에 들어가지 않는 모델을 여러 머신에 레이어 단위로 나눠 싣고,\n추론 중에는 계산 중간값만 주고받는다. P4는 그 연결을 담당하는 계층이다 — 추론 엔진이 아니다.',
    60, 300, 900, 60, { size: 12, color: 'AEBAC6', ls: 1.55 });

  const mx = [60, 358, 656, 954], mw = 268;
  [['1', '프로세스 타입 — agent'],
   ['18', 'llama.cpp가 받치는 ggml backend'],
   ['114,154', 'Rust 줄 · 12 crate'],
   ['10', '고정된 llama.cpp pin']].forEach(([v, l], i) => {
    box(s, mx[i], 424, mw, 96, { fill: '1D242C', line: null });
    T(s, v, mx[i] + 18, 446, mw - 36, 32, { size: 19, color: 'F2F5F8', mono: true, bold: true });
    T(s, l, mx[i] + 18, 486, mw - 36, 16, { size: 7.5, color: '8B97A3', mono: true, cs: 0.6 });
  });
  rule(s, 60, 586, 1160, '2B333B');
  T(s, '기준 HEAD f57543c8d (2026-09-12) · 코드·backend 수치는 저장소 실측 · 이 덱은 구조 설명이며 성능 주장이 아니다',
    60, 600, 1160, 40, { size: 8, color: '8B97A3', ls: 1.4 });
}

/* ═══ 02 왜 필요한가 ════════════════════════════════════ */
{
  const s = slide('배경', '왜 이런 계층이 필요한가', '모델이 장치 한 개에 들어가지 않으면, 나누는 방법이 성능을 정한다', 2,
    '가중치를 줄이거나(양자화) 느린 메모리로 밀어내는(오프로딩) 대신, 여러 장치에 나눠 싣는 길을 택하면 곧바로 “어떻게 자를 것인가”가 문제가 된다.');
  const y0 = s.bodyTop;

  // 텐서 병렬
  box(s, 54, y0, 556, 208, { fill: SURF2, line: null });
  T(s, '텐서 병렬 — 레이어 하나를 쪼갠다', 76, y0 + 18, 400, 18, { size: 12, color: INK, bold: true });
  const tpx = [96, 340];
  tpx.forEach((x) => { box(s, x, y0 + 54, 180, 44, { fill: SURF, line: RULE2 }); });
  T(s, 'GPU A — 레이어 L의 절반', 96, y0 + 69, 180, 16, { size: 8.5, color: INK, align: 'center' });
  T(s, 'GPU B — 레이어 L의 절반', 340, y0 + 69, 180, 16, { size: 8.5, color: INK, align: 'center' });
  for (let i = 0; i < 4; i++) {
    line(s, 280, y0 + 62 + i * 10, 336, y0 + 62 + i * 10, { color: BLK, lw: 1, arrow: false });
  }
  T(s, '레이어마다 all-reduce', 96, y0 + 112, 424, 16, { size: 9, color: BLK, align: 'center' });
  T(s, '자르는 곳마다 전체 활성값을 맞춰야 하므로 장치 사이 대역폭이 곧 한계가 된다.\n한 대 안의 NVLink 같은 초고속 링크를 전제한다.',
    76, y0 + 140, 512, 50, { size: 9.5, color: MUT, ls: 1.4 });

  // 파이프라인 병렬
  box(s, 640, y0, 586, 208, { fill: FLOWW, line: FLOW, lw: 1.2 });
  T(s, '파이프라인 병렬 — 레이어 구간으로 자른다', 662, y0 + 18, 460, 18, { size: 12, color: INK, bold: true });
  box(s, 682, y0 + 54, 200, 44, { fill: SURF, line: FLOW });
  T(s, '머신 1 — 레이어 0‥39', 682, y0 + 69, 200, 16, { size: 8.5, color: INK, align: 'center' });
  box(s, 962, y0 + 54, 200, 44, { fill: SURF, line: FLOW });
  T(s, '머신 2 — 레이어 40‥79', 962, y0 + 69, 200, 16, { size: 8.5, color: INK, align: 'center' });
  line(s, 886, y0 + 76, 958, y0 + 76, { color: FLOW, lw: 1.6 });
  T(s, '경계 1곳', 886, y0 + 52, 72, 14, { size: 8, color: FLOW, mono: true, align: 'center' });
  T(s, '자른 경계에서만 계산 중간값이 건너간다. 가중치와 KV 캐시는 각 머신에 그대로 머문다.\n일반 이더넷으로도 여러 대를 묶을 수 있는 이유다.',
    662, y0 + 140, 542, 50, { size: 9.5, color: INK2, ls: 1.4 });

  const ny = y0 + 232;
  h3(s, 'P4가 맡는 것 / 맡지 않는 것', 54, ny, 600, FLOW);
  bullets(s, [
    '맡는다 — 어느 머신의 어느 노드가 어느 구간을 맡는지, 요청이 그 사슬을 어떤 순서로 지나는지, 무엇이 언제 건너가는지',
    '맡지 않는다 — 행렬 곱, 커널, 양자화, 샘플링. 그건 backend(llama.cpp 등)의 일이다',
    '그래서 P4를 바꾸지 않고 backend를 갈아끼울 수 있고, backend를 바꾸지 않고 배치를 바꿀 수 있다',
  ], 54, ny + 22, 600, 110, { size: 10 });

  note(s, 680, ny, 546, 132,
    '파이프라인 병렬은 공짜가 아니다. 한 요청이 스테이지를 차례로 지나므로 스테이지 수만큼 지연이 쌓이고, ' +
    '앞 스테이지가 노는 시간이 생긴다. 그래서 여러 요청을 겹쳐 흘리는 배치 구성이 이 계층의 핵심 과제가 된다 — ' +
    '뒤에서 다시 나온다.', 'f');

  foot(s, '자르는 방식의 이름과 성질은 일반적인 분산 추론 용어다. 이 덱에서 P4가 구현한 쪽은 파이프라인 병렬(레이어 구간 분할)이다.');
}

/* ═══ 03 ① 레이어 구조 ═════════════════════════════════ */
{
  const s = slide('① 레이어 구조', '무엇이 무엇 위에 있는가', '위층은 아래층의 이름을 모른다', 3,
    '각 층은 바로 아래 층이 약속한 것만 본다. backend 이름이 위로 새지 않기 때문에, 새 backend를 붙여도 봉투·큐·노드·라우팅은 한 줄도 바뀌지 않는다.');
  const y0 = s.bodyTop;

  const L = [
    ['OUTER', '요청하는 쪽 — 어디에 무엇을 싣고 누가 무엇을 받을지 정한다', '요청·배치 계획·세션', 'b'],
    ['P4 protocol', '봉투와 프레임 — 주소·순서·경계만 안다', '내용은 불투명 바이트', 'f'],
    ['P4 agent', '프로세스·큐·워커·노드 수명·전달과 역압', 'backend 중립', 'f'],
    ['Adapter 계약', '노드가 backend에 요구하는 것 — 제출·완료·취소', 'backend 이름 없음', 'f'],
    ['구상 어댑터', 'llamacpp-staged · llamacpp · vllm · sglang · mock', '자기 backend의 말을 한다', ''],
    ['backend', 'llama.cpp → ggml → CUDA · ROCm · Metal · CPU …', '실제 계산', 'q'],
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
  T(s, '↓ 의존 방향', 36, yy - 4, 200, 14, { size: 8, color: FLOW, mono: true });

  h3(s, '이 구조가 만드는 세 가지 성질', 846, y0, 380, FLOW);
  bullets(s, [
    '봉투만 읽으면 전달된다 — 중계 노드는 내용을 열지 않는다. 새 메시지 종류가 중계를 무겁게 만들지 않는다',
    'backend 등록은 한 파일 — entrypoints/agent/src/adapters/mod.rs. 이름 하나, 팩토리 하나, 구현 하나',
    'mock이 항상 들어 있다 — GPU 없이 전체 fleet을 띄워 배치와 순서를 검증할 수 있다',
  ], 846, y0 + 22, 380, 170, { size: 9.5 });

  note(s, 846, y0 + 208, 380, 142,
    'agent가 바깥에 내보이는 표면은 소켓 위의 P4 하나뿐이다. 어댑터가 자기 backend와 HTTP로 말하든, ' +
    '파이프로 말하든, 같은 프로세스 안에서 함수로 부르든 그 위에서는 보이지 않는다. ' +
    '이것이 backend를 갈아끼울 수 있게 하는 실제 이유다.');

  foot(s, '층 이름은 저장소 경로와 1:1 이다 — layers/protocol, layers/agent, layers/adapters/adapter, layers/adapters/*, upstream llama.cpp');
}

/* ═══ 04 ② 에이전트 토폴로지 ═══════════════════════════ */
{
  const s = slide('② 토폴로지', '에이전트와 노드', '머신마다 프로세스 하나, 그 안에 노드 여러 개', 4,
    'controller 프로세스는 없다. agent가 다른 agent에 닿는 길과, agent가 바깥에 답하는 길이 같은 길이다.');
  const y0 = s.bodyTop;

  box(s, 54, y0 + 96, 150, 70, { fill: BNDW, line: BND, lw: 1.2 });
  T(s, 'OUTER', 54, y0 + 116, 150, 18, { size: 12, color: BND, bold: true, align: 'center' });
  T(s, '배치·세션 결정', 54, y0 + 138, 150, 14, { size: 8, color: MUT, align: 'center' });

  const hosts = [
    ['머신 A', ['node 0', 'node 1'], 'CUDA0 / CUDA1'],
    ['머신 B', ['node 2', 'node 3'], 'ROCm0 / ROCm1'],
    ['머신 C', ['node 4'], 'MTL0'],
  ];
  const hx = 268, hw = 300;
  hosts.forEach(([name, nodes, dev], i) => {
    const x = hx + i * (hw + 22);
    box(s, x, y0, hw, 272, { line: RULE2, dash: true });
    T(s, name, x + 14, y0 + 10, 160, 16, { size: 9, color: MUT, mono: true });
    T(s, dev, x + hw - 174, y0 + 10, 160, 16, { size: 8, color: MUT, mono: true, align: 'right' });
    box(s, x + 14, y0 + 34, hw - 28, 220, { fill: FLOWW, line: FLOW, lw: 1.2 });
    T(s, 'agent  (프로세스 1개)', x + 28, y0 + 46, 200, 16, { size: 9.5, color: INK, bold: true });
    T(s, '소켓 · 큐 · 워커 풀', x + 28, y0 + 66, 200, 14, { size: 8, color: MUT });
    nodes.forEach((n, j) => {
      const ny = y0 + 90 + j * 76;
      box(s, x + 28, ny, hw - 56, 62, { fill: SURF, line: RULE2 });
      T(s, n, x + 42, ny + 10, 120, 16, { size: 9.5, color: INK, mono: true, bold: true });
      T(s, '자기 큐 + 어댑터 1개', x + 42, ny + 30, 200, 14, { size: 8, color: MUT });
      T(s, '레이어 구간 하나', x + 42, ny + 44, 200, 14, { size: 8, color: FLOW });
    });
    line(s, x + hw / 2, y0 + 290, x + hw / 2, y0 + 276, { color: FLOW, lw: 1.4 });
  });
  box(s, 268, y0 + 292, 944, 26, { fill: FLOWW, line: FLOW, lw: 1.2 });
  line(s, 129, y0 + 168, 129, y0 + 305, { color: BND, lw: 1.2, arrow: false });
  line(s, 129, y0 + 305, 264, y0 + 305, { color: BND, lw: 1.2 });
  T(s, 'agent ↔ agent 도 같은 P4 프레임 (TCP)', 268, y0 + 299, 944, 16, { size: 8.5, color: FLOW, align: 'center' });

  const dy = y0 + 336;
  const defs = [
    ['AGENT', '머신마다 하나 뜨는 프로세스. 소켓으로 프레임을 받아 큐에 넣고, 워커가 주소만 보고 자기 것인지 판단한다. 자기 것이 아니면 통째로 넘긴다.'],
    ['NODE', 'agent 안의 논리 실행 단위. id 하나로 시작해 LOAD가 어댑터를 물리면 실체가 된다. 자기 큐를 갖고 긴 작업을 혼자 쥔다.'],
    ['OUTER', '바깥에서 요청하는 쪽. 어느 노드가 어느 구간을 맡을지, 어떤 노드들이 한 세션을 이룰지 정한다. agent는 사실만 보고한다.'],
  ];
  defs.forEach(([k, v], i) => {
    const x = 54 + i * 396;
    h3(s, k, x, dy, 370);
    T(s, v, x, dy + 20, 370, 76, { size: 9.5, ls: 1.4 });
  });

  foot(s, '실제 구성 예 — 2호스트 16스테이지, 5호스트 6스테이지. 노드 수는 모델 크기·KV 용량·합법적인 자르기 지점이 정하지, 카드 수가 정하지 않는다.');
}

/* ═══ 05 ② 모델 로딩 ═══════════════════════════════════ */
{
  const s = slide('② 토폴로지', '여러 노드를 하나의 파이프라인으로', '적재와 파이프라인은 서로 다른 수명이다', 5,
    '먼저 노드마다 따로 싣고, 그 다음에 순서를 설치한다. 적재할 때 노드는 서로를 모른다.');
  const y0 = s.bodyTop;

  // 1단계
  T(s, '1단계  LOAD — 노드마다 독립 명령', 54, y0, 560, 18, { size: 12, color: INK, bold: true });
  T(s, '각 노드가 GGUF에서 자기 레이어 구간만 읽어 장치에 올린다. 이웃도, 순서도, 세션도 이 명령에 없다.',
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

  // 2단계
  T(s, '2단계  SESSION — 순서를 설치', 54, y0 + 186, 560, 18, { size: 12, color: INK, bold: true });
  T(s, '참여 노드 전체 목록과 각 노드의 자기 번호를 한 번에 알려 준다. 이때 비로소 사슬이 된다.',
    54, y0 + 210, 560, 32, { size: 9.5, color: MUT, ls: 1.4 });
  T(s, 'stages: [ {agent, node, generation}, … ]   +   stage_index',
    54, y0 + 248, 560, 18, { size: 9, color: FLOW, mono: true });

  const sy = y0 + 278;
  [['stage 0', 'head', 'f'], ['stage 1', '중간', ''], ['stage 2', 'terminal', '']].forEach(([n, r, k], i) => {
    const x = 54 + i * 190;
    box(s, x, sy, 172, 58, { fill: k === 'f' ? FLOWW : SURF2, line: k === 'f' ? FLOW : RULE2, lw: k ? 1.2 : 1 });
    T(s, n, x + 14, sy + 10, 144, 16, { size: 9.5, color: INK, mono: true, bold: true });
    T(s, r, x + 14, sy + 30, 144, 16, { size: 9, color: k === 'f' ? FLOW : MUT });
    if (i > 0) line(s, x - 16, sy + 29, x - 2, sy + 29, { color: FLOW, lw: 1.4 });
  });

  h3(s, '이 분리가 사는 이유', 660, y0, 566, FLOW);
  bullets(s, [
    '같은 적재 위에 세션을 여러 번 세울 수 있다. 순서를 바꾸거나 다시 여는 데 재적재가 필요 없다',
    '노드를 내리면(UNLOAD) 그 적재 세대에 묶인 세션이 모두 무효가 된다 — 세대 번호가 그것을 강제한다',
    'head는 정산과 출력 승인을 소유하고, terminal은 토큰을 만든다. 둘은 다른 노드다',
    '단계 사이의 first/previous/next/terminal 관계는 설치된 순서에서만 파생한다. 노드가 스스로 정하지 않는다',
  ], 660, y0 + 22, 566, 170, { size: 10 });

  note(s, 660, y0 + 216, 566, 120,
    '한 노드가 사슬의 어디에 있는지는 그 노드의 성질이 아니라 세션의 성질이다. ' +
    '같은 노드가 다른 세션에서 다른 자리에 설 수 있고, 그래서 배치를 바꾸는 일이 재적재가 아니라 설정 변경이 된다.', 'f');

  foot(s, '필드 이름은 실제 wire 그대로다 — stage_begin/stage_end(적재), stages[]/stage_index(세션), load_generation(세대)');
}

/* ═══ 06 ③ 노드와 구상 어댑터 ══════════════════════════ */
{
  const s = slide('③ 어댑터', '노드가 backend를 고르는 법', '노드는 인터페이스 하나만 알고, 그 아래는 갈아끼운다', 6,
    'backend를 붙이는 비용은 이름 하나, 팩토리 하나, 구현 하나다. 그 위의 봉투·큐·워커·노드는 어느 backend에서도 같은 코드다.');
  const y0 = s.bodyTop;

  box(s, 500, y0, 280, 56, { fill: SURF2 });
  T(s, 'node', 500, y0 + 18, 280, 20, { size: 12, color: INK, mono: true, bold: true, align: 'center' });
  line(s, 640, y0 + 58, 640, y0 + 84, { color: FLOW, lw: 1.5 });
  box(s, 440, y0 + 86, 400, 46, { fill: FLOWW, line: FLOW, lw: 1.3 });
  T(s, 'Adapter 계약 — 제출 · 완료 · 취소', 440, y0 + 100, 400, 18, { size: 11, color: INK, bold: true, align: 'center' });
  T(s, '여기에는 backend 이름이 하나도 없다' + ' — 아래 이름들이 LOAD에서 구현을 고른다', 340, y0 + 138, 600, 14, { size: 8, color: MUT, align: 'center' });

  const ad = [
    ['mock', '산술만. 장치 없이', 'f'],
    ['llamacpp', 'llama-server (HTTP)', ''],
    ['vllm', 'vLLM 서버', ''],
    ['sglang', 'SGLang 서버', ''],
    ['llamacpp-staged', '레이어 분할 실행', 'b'],
  ];
  const aw = 218, agap = 20;
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
  h3(s, '두 가지 모양의 llama.cpp', 54, dy, 560, FLOW);
  bullets(s, [
    'llamacpp — 한 프로세스가 모델 전체를 쥐고 HTTP로 답한다. 노드 하나로 끝나며 사슬 길이는 1이다',
    'llamacpp-staged — 모델을 레이어 구간으로 잘라 여러 노드에 싣는다. 사슬 길이는 노드 수만큼이다',
    'vLLM·SGLang도 같은 자리에 붙는다. backend가 스스로 모델을 펼치는 쪽은 사슬 길이 1로 참여한다',
  ], 54, dy + 22, 560, 110, { size: 10 });

  note(s, 660, dy, 566, 132,
    'mock이 항상 빌드에 들어 있다는 점이 실용적으로 중요하다. GPU도, 모델 파일도 없이 fleet 전체를 띄워 ' +
    '라우팅·순서·배치·취소를 끝까지 돌려 볼 수 있다. 계산은 산술로 대체되므로 결과가 결정론적이고, ' +
    '두 번 돌린 결과가 다르면 그 차이는 P4에 있다.', 'f');

  foot(s, '등록 지점 entrypoints/agent/src/adapters/mod.rs — llamacpp-staged 는 준비된 실행 파일이 있는 호스트에서만 노출된다(없으면 능력 없음이지 대체 실행이 아니다)');
}

/* ═══ 07 ④ llama.cpp의 플랫폼 수용 ═════════════════════ */
{
  const s = slide('④ 백엔드', 'llama 어댑터 아래', '플랫폼 대응은 P4가 아니라 llama.cpp가 이미 해 둔 일이다', 7,
    '어댑터는 고정된 llama.cpp 하나를 상대한다. 그 아래에서 어떤 장치로 계산되는지는 ggml의 backend 레지스트리가 정한다.');
  const y0 = s.bodyTop;

  const stack = [
    ['llamacpp-staged 어댑터', 'P4 쪽 — 제출·원장·정산', 'f'],
    ['native stage 런타임', '레이어 구간 실행과 스테이지 프로토콜', ''],
    ['llama.cpp', '모델·graph·memory 의미', 'q'],
    ['ggml backend 레지스트리', '실행 장치 선택', 'q'],
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

  T(s, '핀된 upstream(451b89bae)이 담고 있는 ggml backend 18개', 528, y0, 698, 16,
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
  T(s, '주황색 = 이 저장소가 실기로 돌린 backend. 나머지는 llama.cpp가 제공하는 목록이며 P4가 검증했다는 뜻이 아니다.',
    528, y0 + 168, 698, 28, { size: 8.5, color: MUT, ls: 1.35 });

  const ny = y0 + 212;
  note(s, 54, ny, 586, 136,
    'llama.cpp는 빠르게 움직인다. 그래서 어댑터는 어느 시점의 커밋 하나에 고정하고, 그 커밋에 필요한 패치를 ' +
    'compat/<커밋>/ 디렉터리 하나에 모아 둔다. 공식 체크아웃은 건드리지 않고, 준비 스크립트가 별도 작업 트리를 만들어 ' +
    '해시를 확인한 뒤 패치를 적용한다. 지금까지 고정한 커밋이 10개다.');
  note(s, 660, ny, 566, 136,
    '이 구조 덕분에 “새 모델 지원”이나 “새 장치 지원”이 대부분 P4의 일이 아니게 된다. ' +
    'llama.cpp가 지원하면 pin을 올리는 작업이 되고, 그 작업의 충격은 compat 디렉터리 안에서 끝난다. ' +
    'P4의 봉투·큐·노드·배치 코드는 그대로 있는다.', 'f');

  foot(s, '실측 — ls upstream/ggml/src/ggml-* 18개, ls staged/compat 10개 pin, 최신 pin 451b89bae(패치 27개) @HEAD f57543c8d');
}

/* ═══ 08 ⑤ 레이어 분산 로딩의 실제 모습 ════════════════ */
{
  const s = slide('⑤ 실행', '적재된 모습', '노드마다 자기 구간의 가중치와 KV를 스스로 쥔다', 8,
    '80레이어 모델을 네 노드에 나눈 예. 각 노드는 자기 구간만 알고, 자기 구간의 메모리만 갖는다.');
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
    T(s, '레이어 ' + r, x + 16, y0 + 34, 200, 16, { size: 10, color: FLOW, mono: true });
    rule(s, x + 16, y0 + 58, nw - 32);
    box(s, x + 16, y0 + 70, nw - 32, 38, { fill: SURF, line: RULE2 });
    T(s, '가중치 — 이 구간만', x + 28, y0 + 81, 200, 16, { size: 8.5, color: INK2 });
    box(s, x + 16, y0 + 114, nw - 32, 38, { fill: SURF, line: RULE2 });
    T(s, 'KV 캐시 — 이 구간만', x + 28, y0 + 125, 150, 16, { size: 8.5, color: INK2 });
    T(s, kv, x + nw - 116, y0 + 125, 100, 16, { size: 8.5, color: INK, mono: true, align: 'right' });
    box(s, x + 16, y0 + 158, nw - 32, 38, { fill: SURF, line: RULE2 });
    T(s, 'compute buffer', x + 28, y0 + 169, 200, 16, { size: 8.5, color: INK2 });
    if (i > 0) line(s, x - 18, y0 + 105, x - 2, y0 + 105, { color: FLOW, lw: 1.4 });
  });
  T(s, '같은 n_ctx인데도 노드마다 KV 비용이 다르다 — 레이어 구성이 구간마다 다르기 때문이다. 파이프라인의 한계는 가장 비싼 노드가 정한다.',
    54, y0 + 222, 1172, 16, { size: 9, color: MUT });

  const dy = y0 + 254;
  h3(s, '계획과 실제가 같은지 확인한다', 54, dy, 560, FLOW);
  T(s, '--expect-layer-device  begin:end:name', 54, dy + 22, 560, 18, { size: 10, color: FLOW, mono: true });
  bullets(s, [
    '선언한 구간이 빈틈도 겹침도 없이 전체 컷을 덮는지 검사한다',
    '적재 전(PLAN)과 적재 후(LOAD) 두 번 장치 배치를 질의해 대조한다. 메모리 총량이 맞는 것만으로는 통과하지 않는다',
    'CPU에 남길 구간도 명시적으로 선언한다 — 말없이 CPU로 흘러내리는 것을 막는다',
  ], 54, dy + 48, 560, 96, { size: 9.5 });

  note(s, 660, dy, 566, 144,
    '“노드 = GPU 한 장”이 규칙인 것은 아니다. 한 장치에 여러 스테이지를 둘 수도, 한 스테이지가 여러 장치를 쓸 수도, ' +
    '일부 구간을 CPU에 둘 수도 있다. 노드 수를 정하는 것은 모델 크기와 KV 용량, 그리고 그 모델에서 합법적으로 자를 수 있는 ' +
    '지점이지 카드 수가 아니다.');

  foot(s, 'KV 수치는 같은 n_ctx에서 노드별로 측정된 값이며 모델·컷·backend에 따라 달라진다. 장치 이름은 ggml이 보고하는 실제 이름이다.');
}

/* ═══ 09 ⑤ 네트워크를 넘는 것 ══════════════════════════ */
{
  const s = slide('⑤ 실행', '추론 중 오가는 것', '네트워크를 넘는 것은 계산 중간값뿐이다', 9,
    '가중치도 KV 캐시도 노드를 떠나지 않는다. 한 스텝에서 건너가는 것은 자른 경계의 텐서 묶음 — in-flight 배치다.');
  const y0 = s.bodyTop;

  const sx = [140, 520, 900], sw = 240;
  sx.forEach((x, i) => {
    box(s, x, y0 + 74, sw, 122, { fill: SURF2 });
    T(s, ['stage 0 (head)', 'stage 1', 'stage 2 (terminal)'][i], x + 16, y0 + 86, sw - 32, 16,
      { size: 9.5, color: INK, mono: true, bold: true });
    rule(s, x + 16, y0 + 110, sw - 32);
    T(s, '가중치', x + 16, y0 + 120, 100, 14, { size: 8.5, color: MUT });
    T(s, '움직이지 않음', x + sw - 136, y0 + 120, 120, 14, { size: 8.5, color: INK2, align: 'right' });
    T(s, 'KV 캐시', x + 16, y0 + 142, 100, 14, { size: 8.5, color: MUT });
    T(s, '움직이지 않음', x + sw - 136, y0 + 142, 120, 14, { size: 8.5, color: INK2, align: 'right' });
    T(s, 'compute buffer', x + 16, y0 + 164, 120, 14, { size: 8.5, color: MUT });
    T(s, '움직이지 않음', x + sw - 136, y0 + 164, 120, 14, { size: 8.5, color: INK2, align: 'right' });
    if (i > 0) {
      line(s, sx[i - 1] + sw + 6, y0 + 56, x - 6, y0 + 56, { color: FLOW, lw: 1.8 });
      T(s, '경계 텐서 묶음', sx[i - 1] + sw, y0 + 32, 146, 14, { size: 8.5, color: FLOW, mono: true, align: 'center' });
    }
  });
  T(s, '넘어가는 것 — in-flight 배치', 140, y0 + 6, 400, 16, { size: 10, color: FLOW, bold: true });
  line(s, 1146, y0 + 135, 1186, y0 + 135, { color: BND, lw: 1.4, arrow: false });
  line(s, 1186, y0 + 135, 1186, y0 + 214, { color: BND, lw: 1.4, arrow: false });
  line(s, 1186, y0 + 214, 200, y0 + 214, { color: BND, lw: 1.4, dash: true, arrow: false });
  line(s, 200, y0 + 214, 200, y0 + 198, { color: BND, lw: 1.4 });
  T(s, 'terminal이 만든 토큰은 head로 돌아가 승인된 뒤에야 바깥으로 나간다', 300, y0 + 220, 800, 16,
    { size: 8.5, color: BND, align: 'center' });

  const dy = y0 + 252;
  h3(s, '크기의 차이가 설계를 정한다', 54, dy, 560, FLOW);
  const rows = [
    ['가중치', '수십~수백 GB', '한 번 적재, 이후 이동 없음'],
    ['KV 캐시', '노드·요청마다 수십~수백 MB', '이동 없음 — 그래서 요청은 자기 KV가 있는 노드 집합에 묶인다'],
    ['스텝당 전송', '경계 텐서 몇 개', 'gemma-4에서 31·27·23개, 스텝당 81회. Qwen 계열은 1개'],
  ];
  rows.forEach(([a, b, c], i) => {
    const yy = dy + 24 + i * 44;
    T(s, a, 54, yy, 110, 16, { size: 9.5, color: INK, bold: true });
    T(s, b, 170, yy, 190, 16, { size: 9.5, color: FLOW, mono: true });
    T(s, c, 370, yy, 244, 32, { size: 8.5, color: MUT, ls: 1.3 });
    if (i < 2) rule(s, 54, yy + 34, 560);
  });

  note(s, 660, dy, 566, 88,
    '그래서 노드 사이에 요구되는 대역폭이 텐서 병렬보다 훨씬 작다. 경계에서만, 그것도 자른 지점의 텐서만 건너가기 때문에 ' +
    '일반적인 네트워크로 여러 대를 묶을 수 있다.', 'f');
  note(s, 660, dy + 100, 566, 108,
    '대신 값을 치른다. 한 요청이 스테이지를 차례로 지나므로 스테이지가 늘수록 지연이 쌓이고, 한 번에 한 요청만 흘리면 ' +
    '앞 스테이지가 논다. 여러 요청을 겹쳐 흘려 그 빈 시간을 메우는 것이 이 계층이 계속 다듬는 지점이다.');

  foot(s, '전송 단위는 스테이지 경계의 텐서 묶음(캡슐)이다. 텐서 개수는 모델 구조가 정하는 상수이며, 같은 값을 가리키는 텐서는 중복해 싣지 않는다.');
}

/* ═══ 10 ⑥ 인플라이트 배치 전략 ═══════════════════════ */
{
  const s = slide('⑥ 배치', '한 배치를 고르는 규칙', '빈 시간을 메우는 일은 전부 배치 선택에 달려 있다', 10,
    '파이프라인은 한 요청만 흘리면 앞 스테이지가 논다. 어댑터는 매 발행 기회마다 어떤 요청 몇 행을 한 배치에 넣을지 고르고, 그 선택은 자원을 확정하지 않는 순수 계산이다.');
  const y0 = s.bodyTop;

  T(s, '논리 배치 하나를 채우는 순서', 54, y0, 640, 18, { size: 11.5, color: INK, bold: true });
  const bx = 54, bw = 640;
  box(s, bx, y0 + 28, bw, 40, { fill: SURF2 });
  const cells = [['D', FLOW], ['D', FLOW], ['D', FLOW], ['P', BND], ['P', BND], ['P', BND], ['P', BND], ['P', BND]];
  cells.forEach(([t, c], i) => {
    const w = (bw - 16) / cells.length;
    box(s, bx + 8 + i * w, y0 + 36, w - 4, 24, { fill: c === FLOW ? FLOWW : BNDW, line: c, lw: 1 });
    T(s, t, bx + 8 + i * w, y0 + 42, w - 4, 14, { size: 8.5, color: c, mono: true, align: 'center' });
  });
  T(s, 'D = decode 1행씩 먼저', 54, y0 + 74, 300, 14, { size: 8.5, color: FLOW });
  T(s, 'P = 남은 행을 prefill이 회전 water-fill', 340, y0 + 74, 354, 14, { size: 8.5, color: BND });
  bullets(s, [
    '어텐션 모델은 논리 배치를 llama_n_batch까지 채우고, 쪼개는 일은 llama.cpp가 n_ubatch에서 한다',
    'recurrent·hybrid는 시퀀스마다 같은 폭을 요구하므로, 한 번의 호출이 정확히 물리 UBATCH 하나를 만든다',
    'Verify·Replay는 쪼갤 수 없는 한 트랜잭션이다 — 한 물리 UBATCH 안에 있어야 한다',
  ], 54, y0 + 96, 640, 84, { size: 9.5 });

  h3(s, '두 개의 바운드', 740, y0, 486, FLOW);
  box(s, 740, y0 + 22, 486, 66, { fill: FLOWW, line: FLOW, lw: 1.2 });
  T(s, 'PREFILL_PATIENCE = 8', 756, y0 + 34, 300, 16, { size: 10, color: INK, mono: true, bold: true });
  T(s, 'decode에게 연속 8배치를 준 뒤에도 프롬프트가 기다리고 있으면 다음 배치는 프롬프트 차례다. 몫이 아니라 상한이다.',
    756, y0 + 54, 454, 30, { size: 8.5, color: INK2, ls: 1.35 });
  T(s, '코드 주석 그대로 — “8은 측정값이 아니다. 바운드가 존재하게 만드는 최소한일 뿐이고, 실제 하드웨어의 처리량으로 판정된 적이 없다.”',
    740, y0 + 96, 486, 30, { size: 8.5, color: MUT, ls: 1.35 });
  box(s, 740, y0 + 134, 486, 50, { fill: SURF2 });
  T(s, '준비된 선택은 수락되기 전까지 공정성을 소비하지 않는다', 756, y0 + 146, 454, 16, { size: 9.5, color: INK, bold: true });
  T(s, '거부되거나 취소된 후보는 순번을 쓰지 않는다. 남의 계획과 낡은 계획은 이름을 붙여 거절한다.', 756, y0 + 166, 454, 30, { size: 8, color: MUT, ls: 1.3 });

  const ny = y0 + 200;
  T(s, '언제 내보내는가 — 지연 발행의 측정된 근거', 54, ny, 700, 18, { size: 11.5, color: INK, bold: true });
  note(s, 54, ny + 26, 700, 132,
    '꼬리 노드가 바쁜 동안 도착한 배치는 앞 배치를 p50 128 ms 기다렸고, 전체의 63%가 그랬다. 배치 하나는 꼬리에서 첫 레이어에 닿기까지 ' +
    '약 55 ms의 고정비를 쓴다. 꼬리에 배치가 줄을 서면 같은 고정비를 여러 번 내는 셈이므로, 이미 충분한 수가 비행 중이면 계획을 head에서 잠시 쥔다. ' +
    '같은 대기를 폭이 넓어지는 자리로 옮기는 것이다. 행 수 문턱이 아니다 — 파이프라인에 자리가 있으면 얇아도 즉시 보내 깊이를 지킨다. ' +
    '자리와 무관하게 폭을 기다렸던 이전 실험은 26%를 잃었다.');
  note(s, 776, ny + 26, 450, 132,
    '이 knob들은 전부 기본값이 0 또는 off다. DECODE_MEMBERS · PREFILL_ROWS · PREFILL_ROWS_PER_REQUEST · MAX_OPEN_BATCHES · ' +
    'MAX_ISSUE_ROWS · MIN_BATCH_ROWS · PREFILL_FRAGMENTS · PIPELINE_BATCHING. 켜지 않으면 기존 경로 그대로 돈다. ' +
    '위 수치는 그 가설의 근거이지 승격 결과가 아니다.', 'x');

  foot(s, '실측 v2/scheduler.rs(Phase·Demand·PREFILL_PATIENCE·PreparedPlan), v2/node/state.rs:520-575(knob 기본값), v2/node/worker/drive.rs:55-80(지연 발행과 그 근거 주석)');
}

/* ═══ 11 ⑦ KV 캐시 영속화 ═════════════════════════════ */
{
  const s = slide('⑦ 영속화', 'KV 캐시를 파일로', '노드마다 자기 구간의 KV를 내렸다가 되살린다', 11,
    '스테이지가 자기 레이어 구간의 KV를 갖고 있으므로 저장도 복원도 노드마다 따로 일어난다. llama.cpp의 공개 상태 API를 그대로 쓴다.');
  const y0 = s.bodyTop;

  const steps = [
    ['Persist', 'llama_state_seq_get_size_ext\nllama_state_seq_get_data_ext', FLOW],
    ['<kv-root>/<key>.lkv', 'manifest + 상태 바이트\n체크섬을 함께 기록', BND],
    ['셀 회수', 'llama_memory_seq_rm\n저장한 뒤 KV 셀을 비운다', ''],
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
  T(s, 'Restore — 파일을 읽어 llama_state_seq_set_data_ext 로 되돌리고, 그 업로드가 끝나기 전에는 다음 디코드를 허용하지 않는다',
    54, y0 + 132, 700, 16, { size: 8.5, color: BND });

  h3(s, '되살릴 자격을 정하는 manifest', 740, y0, 486, FLOW);
  const man = [
    ['build_identity', '어느 빌드가 만든 상태인가'],
    ['runtime_identity', '어느 런타임 구성인가'],
    ['context_identity', 'n_ctx · n_seq · kv_unified …'],
    ['kv_format', 'K=타입 ; V=타입 ; flags'],
    ['token_position', '어느 위치까지의 상태인가'],
    ['checksum · bytes', '내용이 그대로인가'],
  ].map(([a, b]) => [
    { text: a, options: { fontFace: M, fontSize: 8.5, color: INK } },
    { text: b, options: { fontSize: 9 } },
  ]);
  table(s, man, 740, y0 + 22, 486, [148, 338], { size: 9 });

  const ny = y0 + 180;
  bullets(s, [
    '복원할 때 여섯 중 하나라도 다르면 거부한다 — 다른 빌드·다른 컨텍스트·다른 KV 타입의 상태를 되살리지 않는다',
    '--kv-root 를 주지 않으면 기능 자체가 꺼진 것으로 보고된다. 조용히 메모리에만 남는 경로가 없다',
    '디코드가 실패해 메모리가 더러워진 런타임은 저장·복원·삭제를 모두 거부한다. 어느 시퀀스가 망가졌는지 llama.cpp가 알려 주지 않으므로, 찢어졌을 수 있는 상태를 파일로 만들지 않는다',
    '시퀀스 하나의 상태가 128 MiB를 넘으면 거절한다',
  ], 54, ny, 700, 132, { size: 9.5 });

  note(s, 776, ny, 450, 132,
    '이것은 대화를 이어 붙이기 위한 저장이지 장애 복구 장치가 아니다. 저장한 뒤 KV 셀을 비우므로 긴 대화의 중간 상태를 ' +
    '장치 밖으로 내보내 자리를 되찾는 용도로 쓴다. 다시 읽을 수 있는지는 왼쪽 여섯 항목이 정한다.');

  foot(s, '실측 server/src/runtime/llama_stage_runtime_kv.cpp:113-175(save·restore), state_store.cpp:153-360(manifest 대조·체크섬), main.cpp:284-293(--kv-root 없으면 capability off)');
}

/* ═══ 12 ⑧ MTP·스페큘러티브 ═══════════════════════════ */
{
  const s = slide('⑧ 스페큘러티브', 'MTP와 그 밖의 방법', '지원하면 자동이 아니라, 지원하지 않으면 거부한다', 12,
    '스테이지를 자른 경로에서 제안·검증·롤백은 노드 경계를 넘는 상태다. 그래서 상류가 새 방법을 추가해도 자동으로 켜지지 않도록 일부러 반대로 만들어져 있다.');
  const y0 = s.bodyTop;

  note(s, 54, y0, 700, 92,
    '“llama.cpp가 지원하면 자동으로 지원된다”는 이 경로에는 해당하지 않는다. 지원하는 방법의 목록을 한곳에 적어 두고, 그 목록에 없는 값이 오면 ' +
    'LOAD가 실패한다. 주석이 이유를 밝힌다 — 호출 지점에서 상류 enum을 훑는 방식이었다면 새 열거자가 조용히 지원되는 것으로 오분류된다.', 'x');

  T(s, '구현된 것', 54, y0 + 112, 340, 18, { size: 11.5, color: INK, bold: true });
  box(s, 54, y0 + 138, 340, 54, { fill: FLOWW, line: FLOW, lw: 1.2 });
  T(s, 'COMMON_SPECULATIVE_TYPE_DRAFT_MTP', 68, y0 + 150, 312, 16, { size: 8, color: FLOW, mono: true, bold: true });
  T(s, '모델이 스스로 다음 토큰을 제안하는 방식 하나', 68, y0 + 170, 312, 14, { size: 8.5, color: MUT });

  T(s, '거부되는 것', 414, y0 + 112, 340, 18, { size: 11.5, color: INK, bold: true });
  box(s, 414, y0 + 138, 340, 26, { fill: BLKW, line: BLK, lw: 1 });
  T(s, '별도 draft 모델을 요구하면', 428, y0 + 143, 312, 14, { size: 8.5, color: INK2 });
  box(s, 414, y0 + 168, 340, 26, { fill: BLKW, line: BLK, lw: 1 });
  T(s, '그 밖의 speculative 방법이면', 428, y0 + 173, 312, 14, { size: 8.5, color: INK2 });
  T(s, 'LOAD가 CAPABILITY_UNAVAILABLE 로 실패하고, 두 사유를 이름으로 구분해 돌려준다', 414, y0 + 204, 340, 32,
    { size: 8.5, color: BLK, ls: 1.35 });

  h3(s, 'MTP를 위해 따로 있는 것들', 776, y0 + 112, 450, FLOW);
  bullets(s, [
    '스케줄러의 1급 phase로 Verify·Replay가 있고, 둘은 쪼갤 수 없는 원자 트랜잭션이다',
    '꼬리 스테이지가 제안을 만들고 시퀀스마다 제안 상태를 따로 관리한다',
    '메모리 계획이 draft 컨텍스트의 사용량까지 함께 측정한다 — 적재하기 전에',
    'compat 패치에 MTP 꼬리 스테이지와 speculative 시퀀스 수명이 별도 항목으로 들어 있다',
  ], 776, y0 + 134, 450, 118, { size: 9.5 });

  note(s, 54, y0 + 252, 1172, 78,
    '진짜로 자동인 자리는 따로 있다 — 장치 backend다. 스테이지 런타임은 ggml_backend_load_all() 을 부르고 끝이며, ' +
    'CUDA·Vulkan·HIP·Metal·OpenCL 분기를 하나도 갖고 있지 않다. 모델 구조·양자화·샘플러·문법도 같은 뜻에서 llama.cpp의 것을 그대로 쓴다. ' +
    '따라오는 것과, 경계를 넘는 상태라서 명시 구현이 필요한 것이 나뉘는 지점이 여기다.', 'f');

  foot(s, '실측 compat/p4_llama_compat.cpp:232-248(지원 목록), runtime/llama_stage_runtime.cpp:47-58(LOAD 거부), :64-66(backend 위임), v2/scheduler.rs(Verify·Replay 원자성)');
}

/* ═══ 13 ⑨ KV 외의 캐시 ═══════════════════════════════ */
{
  const s = slide('⑨ 메모리', 'KV 말고 더 잡는 모델', '캐시를 더 잡는 구현은 “선언한 것만” 자를 수 있다', 13,
    'llama.cpp의 메모리는 평범한 KV 하나가 아니다. 슬라이딩 윈도우, 순환 상태, 희소 어텐션, 혼합형이 각각 다른 저장소를 잡는다.');
  const y0 = s.bodyTop;

  const mem = [
    ['llama-kv-cache', 1], ['llama-kv-cache-iswa', 1], ['llama-memory-recurrent', 1], ['llama-memory-hybrid', 1],
    ['llama-kv-cache-dsa', 0], ['llama-kv-cache-dsa-iswa', 0], ['llama-kv-cache-dsv4', 0], ['llama-kv-cache-msa', 0],
    ['llama-memory-hybrid-idx', 0], ['llama-memory-hybrid-iswa', 0],
  ];
  T(s, '핀된 upstream의 메모리 구현 10종', 54, y0, 700, 16, { size: 9, color: FLOW, mono: true, bold: true });
  mem.forEach(([n, ok], i) => {
    const x = 54 + (i % 4) * 178, yy = y0 + 24 + Math.floor(i / 4) * 44;
    box(s, x, yy, 166, 34, { fill: ok ? FLOWW : SURF2, line: ok ? FLOW : RULE2, lw: ok ? 1.2 : 1 });
    T(s, n, x + 10, yy + 10, 146, 14, { size: 7.5, color: ok ? FLOW : MUT, mono: true });
  });
  T(s, '파란색 4종만 스테이지 잔여를 선언했다. 나머지는 부분 stage로 만들려 하면 거부된다 — 조용히 틀리지 않는다.',
    54, y0 + 160, 700, 16, { size: 8.5, color: MUT });

  h3(s, '게이트는 두 겹이다', 776, y0, 450, FLOW);
  box(s, 776, y0 + 22, 450, 64, { fill: SURF2 });
  T(s, 'linkcpp_stage_residency_supported', 790, y0 + 32, 422, 14, { size: 8, color: INK, mono: true });
  T(s, '= false  (기본값이 거부)', 790, y0 + 50, 422, 14, { size: 8.5, color: BLK, mono: true, bold: true });
  T(s, '구현이 명시적으로 true 로 덮어써야 자를 수 있다', 790, y0 + 68, 422, 14, { size: 8, color: MUT });
  bullets(s, [
    '팩토리에서 컴파일 타임 상수로 한 번 — 부분 stage인데 선언이 없으면 메모리를 아예 만들지 않는다',
    '컨텍스트 생성에서 가상 호출로 다시 한 번 — “스테이지 잔여를 선언하지 않았다”로 던진다',
    'iSWA는 자기 저장소가 없고 base·swa 두 캐시에 위임한다. 재사용되는 KV 영역을 가르는 경계는 그 둘의 생성자가 거절한다',
  ], 776, y0 + 98, 450, 110, { size: 9 });

  const ny = y0 + 224;
  T(s, '적재 전에 장치별로 쪼개 계산한다', 54, ny, 700, 18, { size: 11.5, color: INK, bold: true });
  const cols = [['model', '가중치'], ['context', 'KV와 그 밖의 상태'], ['compute', '실행 버퍼']];
  cols.forEach(([a, b], i) => {
    const x = 54 + i * 236;
    box(s, x, ny + 26, 216, 54, { fill: SURF2 });
    T(s, a, x + 14, ny + 36, 188, 16, { size: 9.5, color: FLOW, mono: true, bold: true });
    T(s, b, x + 14, ny + 56, 188, 14, { size: 8.5, color: MUT });
  });
  T(s, '셋의 합을 장치의 여유와 대조하고, 맞지 않으면 LOAD가 실패한다 — 할당하기 전에.',
    54, ny + 90, 700, 20, { size: 8.5, color: MUT });

  note(s, 776, ny, 450, 116,
    '그래서 “캐시를 더 잡는 모델도 처리된다”는 반만 맞다. 추가 저장소가 계획에 잡히고 스테이지에 상주하는 것은 선언한 4종에 한정되며, ' +
    'DSA·DSV4·MSA 같은 희소 어텐션 계열은 아직 선언이 없어 자르면 거부된다. 한 노드에 통째로 싣는 경로에서는 upstream 그대로 동작한다.', 'x');

  foot(s, '실측 upstream/src 의 메모리 구현 10종, compat/0016·0017·0022 패치(선언 4곳과 기본값 false), runtime/stage_memory_plan.hpp:56-98(장치별 model·context·compute)');
}

/* ═══ 14 정리 ═══════════════════════════════════════════ */
{
  const s = slide('정리', '개발자 관점에서', '이 구조가 실제로 주는 것', 14);
  const y0 = s.bodyTop;

  const cards = [
    ['①', '큰 모델을 노드를 더해 돌린다', 'GPU 한 장, 머신 한 대의 한계가 모델 크기의 한계가 아니게 된다. 레이어 구간으로 자르므로 노드를 더해도 늘어나는 통신은 경계 하나뿐이다.', FLOW],
    ['②', 'backend를 갈아끼운다', '노드가 아는 것은 어댑터 계약 하나다. llama.cpp든 vLLM이든 자체 엔진이든, 이름 하나와 구현 하나를 등록하면 그 위층은 한 줄도 바뀌지 않는다.', FLOW],
    ['③', '플랫폼 대응을 빌려 쓴다', 'CUDA·ROCm·Metal·Vulkan·CPU 같은 장치 대응은 llama.cpp가 이미 하고 있다. P4는 그 위에 올라타고, 상류 변화의 충격은 pin 디렉터리 안에서 끝난다.', BND],
    ['④', '느린 곳을 지목할 수 있다', 'agent의 큐 깊이와 노드 안에서 실행 중인 수를 따로 보고한다. 느려졌을 때 P4가 쥐고 있는지 backend가 쥐고 있는지가 관측값으로 갈린다.', BND],
  ];
  cards.forEach(([n, t, d, c], i) => {
    const x = 54 + (i % 2) * 596, y = y0 + Math.floor(i / 2) * 168;
    box(s, x, y, 576, 148, { fill: SURF2, line: null });
    T(s, n, x + 24, y + 20, 40, 20, { size: 13, color: c, mono: true, bold: true });
    T(s, t, x + 24, y + 48, 528, 22, { size: 13, color: INK, bold: true });
    T(s, d, x + 24, y + 78, 528, 60, { size: 9.5, color: INK2, ls: 1.45 });
  });

  note(s, 54, y0 + 340, 1172, 74,
    '경계를 분명히 해 둔다 — TLS·인증·인가는 없다. 주소가 스스로를 밝히는 것을 믿는 구조이므로 신뢰할 수 있는 망 안에서만 쓴다. ' +
    '내구 상태도 없다. agent가 재시작하면 노드는 사라지고, 무엇이 있어야 하는지에 대한 기록은 OUTER가 쥐고 있다.', 'x');

  foot(s, '이 덱은 구조 설명이다. 처리량·지연 같은 성능 수치는 조건에 묶인 별도 측정 기록이 소유하며, 여기에 실린 숫자는 구조를 설명하기 위한 실측 예시다.');
}

const out = process.argv[2] || 'p4-intro.pptx';
pres.writeFile({ fileName: out }).then(() => console.log('wrote', out));
