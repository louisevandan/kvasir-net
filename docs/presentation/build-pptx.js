// P4 구조와 설계 — Google Slides 이식용 pptx 생성기
// HTML 장표(1280x720 px)를 13.333in x 7.5in 에 1:1 이식한다. 1px = 1/96in.
const P = require('pptxgenjs');

const pres = new P();
pres.defineLayout({ name: 'P4', width: 13.3333, height: 7.5 });
pres.layout = 'P4';
pres.author = 'P4';
pres.title = 'P4 구조와 설계';

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
   01 — 표지
   ════════════════════════════════════════════════════════ */
{
  const s = pres.addSlide();
  s.background = { color: DARK };
  T(s, 'DISTRIBUTED INFERENCE TRANSPORT   ·   2026-09-11', 60, 56, 900, 16,
    { size: 8.5, color: '5FB5C9', mono: true, cs: 1.8, bold: true });
  s.addShape('rect', { x: I(60), y: I(132), w: I(74), h: I(3), fill: { color: '5FB5C9' }, line: { type: 'none' } });
  T(s, '여러 컴퓨터에 흩어진 모델로\n하나의 응답을 만드는 통신 계층', 60, 158, 1000, 130,
    { size: 40, color: 'F2F5F8', bold: true, ls: 1.14 });
  T(s, 'P4는 모델을 실행하지 않고 처리량을 소유하지 않는다. 일을 backend까지 운반하고 답을 돌려주며,\n느려진 지점을 논쟁이 아니라 귀속으로 지목할 수 있게 만든다.',
    60, 308, 860, 60, { size: 12, color: 'AEBAC6', ls: 1.5 });

  const mx = [60, 358, 656, 954], mw = 268;
  const metrics = [
    ['108,815', 'RUST 줄 · 12 CRATE'],
    ['14,597', 'C++ STAGE RUNTIME 줄'],
    ['9', 'LLAMA.CPP 호환 PIN'],
    ['1384 / 0 / 7', 'WORKSPACE 통과/실패/무시'],
  ];
  metrics.forEach(([v, l], i) => {
    box(s, mx[i], 430, mw, 96, { fill: '1D242C', line: null });
    T(s, v, mx[i] + 18, 452, mw - 36, 32, { size: 19, color: 'F2F5F8', mono: true, bold: true });
    T(s, l, mx[i] + 18, 492, mw - 36, 16, { size: 7.5, color: '8B97A3', mono: true, cs: 1 });
  });
  rule(s, 60, 590, 1160, '2B333B');
  T(s, '기준 HEAD 429e057de (2026-09-11) · 코드 수치는 wc -l 실측(시험 포함), 게이트 수치는 docs/distributed-batching-roadmap.md §0 인용\n성능 수치는 모두 선별(screening)이며 승인이 아니다',
    60, 604, 1160, 44, { size: 8, color: '8B97A3', ls: 1.4 });
}

/* ════════════════════════════════════════════════════════
   02 — 무엇인가 · 무엇이 아닌가
   ════════════════════════════════════════════════════════ */
{
  const s = slide('전제', { eyebrow: '무엇인가 · 무엇이 아닌가', h: '이 계층이 내는 주장은 “검증 가능하다”는 점이 핵심이다' }, 2,
    '범용 메시징이 아니다. 워크로드를 상세히 알고 그 모양에 맞춰 깎았다. 그 밖의 것은 의도적으로 없다.');
  const y0 = s.bodyTop;

  h3(s, '이것이다', 54, y0, 380, FLOW);
  bullets(s, [
    '레이어 구간으로 여러 노드에 나뉜 모델',
    '노드들을 source-routing 하는 prefill chain',
    '한 토큰이 한 바퀴를 요구하는 decode ring',
    '선언된 상한 아래의 cohort 배치',
  ], 54, y0 + 20, 380, 110, { size: 10 });

  h3(s, '이것이 아니다', 54, y0 + 148, 380, BLK);
  bullets(s, [
    '범용 메시징 시스템 — 워크로드에 없는 기능은 없다',
    '처리량의 소유자 — 모델을 실행하지 않는다',
    'TLS·인증·인가 — 없다. 신뢰망 안에서만 쓴다',
    '내구 상태 — 재시작한 agent에는 노드가 없다',
  ], 54, y0 + 168, 380, 110, { size: 10 });

  note(s, 470, y0, 756, 78,
    '주장: 실행 중인 시스템의 문제는 이 계층의 것이 아니다. — 이 주장이 시험 가능하다는 것이 설계의 요점이다. ' +
    '모든 단계를 산술만 하는 mock backend(장치도, 런타임도, 아래에 탓할 것도 없음)로 실행하고, agent는 자기 lane 깊이를 node 깊이와 나란히 보고한다.', 'f');

  // 귀속 다이어그램
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
  T(s, 'hop = backend 안의 시간', 846, dy + 63, 360, 16, { size: 9, color: INK, align: 'center' });
  T(s, 'running: 지금 어댑터 안의 시퀀스 수\n= 지켜지고 있는 상한, 관측 가능', 846, dy + 100, 360, 40, { size: 9, color: MUT, ls: 1.35 });
  line(s, 800, dy - 8, 800, dy + 184, { color: BND, dash: true, arrow: false, lw: 1.2 });
  T(s, 'adapter 경계', 700, dy + 190, 200, 14, { size: 7.5, color: BND, mono: true, align: 'center' });

  foot(s, '출처 docs/overview.md, docs/constraints.md, layers/agent/src/agent(NodeStatus) · running은 bool이던 시절 1과 100에서 똑같이 참이었다');
}

/* ════════════════════════════════════════════════════════
   03 — 토폴로지
   ════════════════════════════════════════════════════════ */
{
  const s = slide('토폴로지', { eyebrow: '프로세스 타입은 하나다', h: 'controller는 없다. node는 agent 안에 있다' }, 3,
    'agent가 다른 agent에 닿는 경로는, agent가 외부에 답하는 경로와 같은 경로다. 특별한 진입 프로세스를 따로 두지 않는다.');
  const y0 = s.bodyTop;

  box(s, 54, y0 + 48, 104, 52, { fill: BNDW, line: BND, lw: 1.2 });
  T(s, 'OUTER', 54, y0 + 58, 104, 16, { size: 10, color: BND, mono: true, align: 'center', bold: true });
  T(s, '배치·토폴로지 권위', 54, y0 + 78, 104, 14, { size: 7.5, color: MUT, align: 'center' });
  line(s, 162, y0 + 74, 206, y0 + 74, { color: FLOW, lw: 1.4 });

  box(s, 208, y0 + 42, 120, 64, { fill: FLOWW, line: FLOW, lw: 1.2 });
  T(s, 'entry agent', 208, y0 + 56, 120, 16, { size: 10, color: INK, mono: true, align: 'center' });
  T(s, '= 그 agent의 진입점', 208, y0 + 76, 120, 14, { size: 7.5, color: MUT, align: 'center' });

  const rows = [y0 + 0, y0 + 72, y0 + 144];
  rows.forEach((ry) => line(s, 330, y0 + 74, 368, ry + 24, { color: FLOW, lw: 1.4 }));
  T(s, '같은 경로', 300, y0 + 182, 100, 12, { size: 7.5, color: FLOW, mono: true, align: 'center' });

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
  T(s, 'agent 안의 유일한 내부 엔티티가 node다 — 노드 프로세스는 없다', 490, y0 + 208, 386, 14,
    { size: 8, color: MUT, align: 'center' });

  line(s, 906, y0 - 6, 906, y0 + 196, { color: RULE, arrow: false, lw: 0.75 });
  T(s, '일반화하는 축은 하나다 — 경계를 누가 소유하는가', 928, y0, 298, 30, { size: 9, color: FLOW, mono: true, bold: true, ls: 1.3 });
  rule(s, 928, y0 + 40, 298);
  T(s, 'llama.cpp pipeline', 928, y0 + 50, 200, 14, { size: 9, color: INK, mono: true });
  T(s, 'chain n', 1026, y0 + 50, 200, 14, { size: 9, color: FLOW, mono: true, align: 'right' });
  T(s, 'P4가 조각 사이 경계를 소유한다', 928, y0 + 68, 298, 14, { size: 8, color: MUT });
  rule(s, 928, y0 + 90, 298);
  T(s, 'vLLM · SGLang', 928, y0 + 100, 200, 14, { size: 9, color: INK, mono: true });
  T(s, 'chain 1', 1026, y0 + 100, 200, 14, { size: 9, color: FLOW, mono: true, align: 'right' });
  T(s, 'backend가 스스로 모델을 펼친다', 928, y0 + 118, 298, 14, { size: 8, color: MUT });
  rule(s, 928, y0 + 140, 298);
  T(s, '추가 비용 = 이름 하나 + Adapter 구현 하나', 928, y0 + 152, 298, 28, { size: 8, color: MUT, ls: 1.3 });

  const cy = y0 + 236;
  const defs = [
    ['OUTER', '요청하는 쪽. 배치·load 계획·chain과 모든 업무 식별자를 소유한다. agent는 기계 사실을 보고할 뿐 그것을 배치로 바꾸지 않는다.'],
    ['AGENT', '메인 큐 하나와 worker 풀을 가진 소켓 프로그램. 유일한 내부 엔티티는 node다.'],
    ['NODE · ADAPTER', 'node는 load가 어댑터를 물릴 때까지 id에 불과하다. 자기 큐와 자기 긴 작업을 쥔다. adapter는 hop을 받고 event를 보고한다.'],
  ];
  defs.forEach(([k, v], i) => {
    const x = 54 + i * 396;
    h3(s, k, x, cy, 370);
    T(s, v, x, cy + 20, 370, 76, { size: 9.5, ls: 1.35 });
  });

  foot(s, '출처 README.md, docs/overview.md, docs/event-protocol-v2.md · 실행: p4-agent 0.0.0.0:52001 tcp://THIS_HOST:52001');
}

/* ════════════════════════════════════════════════════════
   04 — 두 개의 wire
   ════════════════════════════════════════════════════════ */
{
  const s = slide('wire', { eyebrow: '헤더만 읽고 전체 길이를 안다', h: '봉투는 모든 홉이 읽고, 본문은 목적지만 읽는다' }, 4,
    '이 분리가 설계다. 중계는 한 번도 디코드하지 않은 바이트를 복사해 전달하므로, 전달 비용이 메시지 종류와 무관하고 새 메시지 종류가 중계기를 무겁게 만들 수 없다.');
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

  strip(y0, 'P4B1 · v8 — hop 프레임        layers/protocol/src/frame/mod.rs', [
    { t: 'P4B1', w: 86, k: 'f', off: 0 },
    { t: '8', w: 26, k: 'f', off: 4 },
    { t: 'zero×3', w: 64, small: true, off: 5 },
    { t: 'env_len', w: 86, k: 'b', off: 8 },
    { t: 'body_len', w: 86, k: 'b', off: 12 },
    { t: 'envelope — 모든 홉이 읽는다', w: 270, ko: true, off: 16 },
    { t: 'body — 목적지만 읽는다', w: 554, ko: true },
  ], [{ t: '≤ 256 KiB', x: 348 }, { t: '≤ 2 GiB', x: 618 }]);

  rule(s, 54, y0 + 108, 1172);

  strip(y0 + 126, 'P4E3 — event 프레임 (현재 실행 경로)        layers/protocol/src/event/wire.rs', [
    { t: 'P4E3', w: 86, k: 'f', off: 0 },
    { t: 'env_len', w: 86, k: 'b', off: 4 },
    { t: 'payload_len', w: 86, k: 'b', off: 8 },
    { t: 'envelope — 12 필드, 자기서술', w: 300, ko: true, off: 12 },
    { t: 'payload — 불투명 바이트', w: 614, ko: true },
  ], [{ t: '≤ 256 KiB', x: 258 }, { t: '≤ 2 GiB', x: 558 }]);

  const ny = y0 + 248;
  note(s, 54, ny, 576, 124,
    '본문 상한을 2 GiB로 올린 이유는 주석에 남아 있다. staged prefill hop은 시퀀스·토큰마다 hidden-state cut 하나를 F32로 나른다. ' +
    '2,048폭 모델에 5,000토큰 프롬프트면 시퀀스 하나가 39 MiB, 10폭 prefill 창은 391 MiB다. ' +
    '이전 128 MiB 상한은 그 창을 3개에서 끊었고 — 배포는 건강한 채로 60요청 중 53개가 HOP envelope too large 로 실패했다.');
  note(s, 650, ny, 576, 124,
    '문서·코드 차이(실측): docs/api.md는 “P4B1 v6 · 본문 최대 1 MiB”로 적혀 있으나 코드는 v8 · 2 GiB다. ' +
    'v7→v8 bump 이유도 코드 주석이 소유한다: SessionClose가 단방향 broadcast에서 ack 계약으로 바뀌었고, ' +
    '절반만 구형인 fleet은 첫 조기 종료까지 정상으로 보이다가 구형 쪽 stage를 조용히 누출시킨다. ' +
    '버전 bump가 그것을 본문 한 바이트 전에 양방향 거부로 바꾼다.', 'x');

  foot(s, '실측 frame/mod.rs:11–35(MAGIC·VERSION·상한), event/wire.rs:4–6 · 문서 인용 docs/api.md — 차이는 문서 갱신 대상이며, 이 장표는 코드 값을 따른다');
}

/* ════════════════════════════════════════════════════════
   05 — envelope
   ════════════════════════════════════════════════════════ */
{
  const s = slide('wire', { eyebrow: 'event envelope', h: 'P4는 prompt도 token도 KV도 모른다' }, 5,
    '모든 이벤트는 payload를 디코드하지 않고 라우팅할 수 있도록 자기서술적이다. 그 바이트를 해석할 자격은 content-type과 adapter_kind가 지정한 구상 어댑터에만 있다.');
  const y0 = s.bodyTop;

  h3(s, 'ENVELOPE — 12 필드      VERSION = 3', 54, y0, 640, FLOW);
  const rows = [
    ['protocol_version', '3이 아니면 읽지 않고 거부'],
    ['event_id', '불변 이벤트 정체성'],
    ['correlation_id', '수명/요청 정체성'],
    ['causation_id?', '이 이벤트를 유발한 이벤트'],
    ['source · target', '논리 생산자 · 최종 소비자 (Endpoint 값)'],
    ['return_route?', '안정적인 출력·telemetry 목적지'],
    ['class', 'control · data · output · telemetry'],
    ['sequence', 'correlation + source 안에서 단조 증가'],
    ['deadline_unix_ms?', '수용 펜스. 중단 약속이 아니다. 0은 거부'],
    ['adapter_kind?', '구상 어댑터 선택. payload는 여전히 불투명'],
    ['payload_content_type', '그 바이트의 해석 규약'],
  ].map(([a, b]) => [
    { text: a, options: { fontFace: M, fontSize: 8.5, color: INK } },
    { text: b, options: { fontSize: 9.5 } },
  ]);
  table(s, rows, 54, y0 + 22, 640, [200, 440], { size: 9.5, rowH: 30 });

  h3(s, 'ENDPOINT는 레지스트리 키가 아니라 값이다', 730, y0, 496);
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
    'source는 회신 주소를 겸할 수 없다. 꼬리 노드는 앞 노드로부터 데이터를 받지만, 출력은 추론을 시작한 OUTER의 것이다. ' +
    '안정적인 channel_id가 그 스트림을 가리키고, connection_generation이 재연결 후 옛 소켓으로 새 스트림이 흘러가는 것을 막는다.', 'f');
  note(s, 730, y0 + 210, 496, 80,
    '중간 agent는 source·target·return_route를 다시 쓰지 않는다. 어댑터가 작업을 끝내면 자신을 source로, ' +
    '정확한 다음 endpoint를 target으로, 유발 이벤트를 causation_id로 하는 새 이벤트를 만든다.');
  T(s, '정의하지 않는 필드: prompt · token · max-token · Prefill · Decode · KV · layer · tensor · batch · backend.\n' +
    '순서와 중복 억제는 per-source sequence + event 정체성이 제공하며, transport route를 해시해 논리 순서를 추론하는 것은 유효하지 않다.',
    730, y0 + 304, 496, 60, { size: 9.5, ls: 1.35 });

  foot(s, '실측 layers/protocol/src/event/mod.rs:94–177(Envelope·EventClass·validate) · 계약 docs/event-protocol-v2.md');
}

/* ════════════════════════════════════════════════════════
   06 — 판정과 레인
   ════════════════════════════════════════════════════════ */
{
  const s = slide('agent 코어', { eyebrow: '판정과 레인', h: '프레임은 정확히 두 번 판정된다. 그리고 우선순위는 유계다' }, 6);
  const y0 = s.bodyTop;

  // 판정
  box(s, 174, y0, 280, 40, { fill: FLOWW, line: FLOW, lw: 1.2 });
  T(s, 'target이 내 주소인가?', 174, y0 + 12, 280, 18, { size: 10, color: INK, mono: false, align: 'center' });
  line(s, 254, y0 + 42, 174, y0 + 74, { color: MUT });
  T(s, '아니오', 176, y0 + 46, 60, 12, { size: 7.5, color: MUT, mono: true });
  line(s, 374, y0 + 42, 454, y0 + 74, { color: MUT });
  T(s, '예', 412, y0 + 46, 40, 12, { size: 7.5, color: MUT, mono: true });
  box(s, 54, y0 + 76, 216, 52, { fill: SURF2 });
  T(s, 'Forward(target)', 54, y0 + 86, 216, 16, { size: 9, color: INK, mono: true, align: 'center' });
  T(s, '통째로 전달. peer와 OUTER는 같은 경우', 54, y0 + 104, 216, 14, { size: 7.5, color: MUT, align: 'center' });
  box(s, 354, y0 + 76, 216, 40, { fill: SURF2 });
  T(s, 'recipient는?', 354, y0 + 88, 216, 16, { size: 9, color: INK, mono: true, align: 'center' });
  line(s, 414, y0 + 118, 384, y0 + 142, { color: MUT });
  line(s, 510, y0 + 118, 524, y0 + 142, { color: MUT });
  box(s, 306, y0 + 144, 130, 48, { fill: SURF2 });
  T(s, 'Agent', 306, y0 + 154, 130, 16, { size: 9, color: INK, mono: true, align: 'center' });
  T(s, '노드 생성·삭제, 조회', 306, y0 + 172, 130, 14, { size: 7.5, color: MUT, align: 'center' });
  box(s, 446, y0 + 144, 128, 48, { fill: BNDW, line: BND, lw: 1.2 });
  T(s, 'Node(id)', 446, y0 + 154, 128, 16, { size: 9, color: BND, mono: true, align: 'center' });
  T(s, '그 노드의 큐로', 446, y0 + 172, 128, 14, { size: 7.5, color: MUT, align: 'center' });
  rule(s, 54, y0 + 212, 520);
  T(s, '판정은 lane이나 chain에 의존하지 않는다 — 의존하는 순간 전달이 자기 트래픽을 이해해야 한다.',
    54, y0 + 224, 520, 32, { size: 9.5, color: INK2, align: 'center', ls: 1.35 });
  T(s, '순수 함수: envelope과 자기 주소가 들어가고 판정이 나온다. 세 번째 변종이 생긴다면 agent가 가져선 안 될 상태를 얻은 것이다.',
    54, y0 + 268, 520, 32, { size: 8.5, color: MUT, ls: 1.35 });

  // 레인
  const lx = 654;
  T(s, '선호 순서', lx, y0, 200, 12, { size: 7.5, color: MUT, mono: true });
  const L = [['control', true], ['response', false], ['decode', false], ['prefill', false]];
  L.forEach(([n, hot], i) => {
    const yy = y0 + 16 + i * 26;
    box(s, lx, yy, 150, 22, { fill: hot ? FLOWW : SURF2, line: hot ? FLOW : RULE2, lw: hot ? 1.2 : 1 });
    T(s, n, lx + 8, yy + 4, 140, 14, { size: 9, color: INK, mono: true });
  });
  line(s, lx + 154, y0 + 64, lx + 196, y0 + 64, { color: FLOW, lw: 1.4 });
  box(s, lx + 198, y0 + 42, 106, 48, { fill: FLOWW, line: FLOW, lw: 1.2 });
  T(s, 'dispatcher', lx + 198, y0 + 54, 106, 16, { size: 9, color: INK, mono: true, align: 'center' });
  T(s, '한 개의 task', lx + 198, y0 + 72, 106, 14, { size: 7.5, color: MUT, align: 'center' });
  T(s, 'take 1 … 16', lx + 330, y0 + 22, 200, 12, { size: 7.5, color: MUT, mono: true });
  for (let i = 0; i < 16; i++) {
    box(s, lx + 330 + i * 12, y0 + 34, 9, 18, { fill: i === 15 ? BND : SURF3, line: null });
  }
  T(s, '16번째 = 공정', lx + 330, y0 + 58, 190, 12, { size: 7.5, color: BND, mono: true, align: 'right' });
  T(s, '네 레인이 동등하게 경쟁한다', lx + 330, y0 + 76, 200, 14, { size: 8, color: MUT });
  rule(s, lx, y0 + 122, 572);
  T(s, '엄격한 우선순위는 선호가 아니라 거부권이다. 한 번도 dispatch되지 않은 lap은 끝나지 않는 요청이다.',
    lx, y0 + 134, 572, 32, { size: 9.5, color: INK2, ls: 1.35 });
  note(s, lx, y0 + 178, 572, 122,
    '한 route는 한 worker. route 안의 순서가 유일한 순서 보장이고, 그 보장은 등록 순서에서 나온다. ' +
    '한 route의 연속 프레임을 서로 다른 worker에 넘기면 그것이 버려지고, 호출자에게는 P4가 스트림을 재정렬한 것으로 보인다. ' +
    'route를 해시해 route별 직렬성을 지키면서 다른 route는 동시에 달린다.');

  foot(s, '출처 layers/agent/src/worker/judge, layers/agent/src/queue/main, docs/architecture.md, docs/constraints.md · 전달도 자기 task가 아니라 dispatcher에서 일어난다');
}

/* ════════════════════════════════════════════════════════
   07 — 노드의 긴 작업
   ════════════════════════════════════════════════════════ */
{
  const s = slide('agent 코어', { eyebrow: '노드의 긴 작업', h: '노드는 두 개의 사건으로만 전진한다 — 타이머는 없다' }, 7,
    '노드의 속도는 backend의 속도다. 세 번째 트리거는 그 속도에 대한 추측이 된다.');
  const y0 = s.bodyTop;

  box(s, 54, y0 + 40, 150, 46, { fill: SURF2 });
  T(s, 'worker', 54, y0 + 50, 150, 16, { size: 9, color: INK, mono: true, align: 'center' });
  T(s, '옮기고 끝난다', 54, y0 + 68, 150, 14, { size: 7.5, color: MUT, align: 'center' });
  line(s, 208, y0 + 63, 268, y0 + 63, { color: FLOW, lw: 1.4 });
  T(s, '① 일 도착', 208, y0 + 44, 62, 12, { size: 7.5, color: FLOW, mono: true, align: 'center' });

  box(s, 270, y0 + 20, 230, 150, { fill: FLOWW, line: FLOW, lw: 1.2 });
  T(s, 'node run loop', 270, y0 + 32, 230, 18, { size: 10.5, color: INK, mono: true, align: 'center', bold: true });
  rule(s, 284, y0 + 56, 202);
  const kv = [['queue', 'depth · waiting'], ['in-flight map', 'keyed by sequence'], ['ceiling', 'load가 선언한 값'], ['lifecycle', 'Never·At(gen)·Refused']];
  kv.forEach(([a, b], i) => {
    T(s, a, 284, y0 + 66 + i * 22, 110, 14, { size: 8.5, color: INK, mono: true });
    T(s, b, 344, y0 + 66 + i * 22, 142, 14, { size: 7.5, color: MUT, mono: true, align: 'right' });
  });
  T(s, '한 번에 hop 하나 — 구조적으로', 270, y0 + 152, 230, 14, { size: 8, color: MUT, align: 'center' });

  line(s, 502, y0 + 76, 614, y0 + 76, { color: FLOW, lw: 1.4 });
  T(s, 'hop(window)', 502, y0 + 58, 112, 12, { size: 7.5, color: MUT, mono: true, align: 'center' });
  box(s, 616, y0 + 50, 150, 52, { fill: SURF2 });
  T(s, 'adapter', 616, y0 + 60, 150, 16, { size: 9, color: INK, mono: true, align: 'center' });
  T(s, 'blocking thread에서', 616, y0 + 78, 150, 14, { size: 7.5, color: MUT, align: 'center' });
  line(s, 770, y0 + 76, 832, y0 + 76, { color: MUT });
  box(s, 834, y0 + 50, 150, 52, { fill: BNDW, line: BND, lw: 1.2 });
  T(s, 'backend', 834, y0 + 60, 150, 16, { size: 9, color: BND, mono: true, align: 'center' });
  T(s, '장치 위의 시간', 834, y0 + 78, 150, 14, { size: 7.5, color: MUT, align: 'center' });

  line(s, 909, y0 + 104, 909, y0 + 140, { color: FLOW, dash: true, arrow: false, lw: 1.3 });
  line(s, 909, y0 + 140, 390, y0 + 140, { color: FLOW, dash: true, arrow: false, lw: 1.3 });
  line(s, 390, y0 + 140, 390, y0 + 172, { color: FLOW, dash: true, lw: 1.3 });
  T(s, '② hop 종료 — 이것을 보고서야 다음을 시작한다', 440, y0 + 118, 440, 14, { size: 8, color: FLOW, mono: false, align: 'center' });

  box(s, 1010, y0 + 40, 216, 62, { line: BLK, lw: 1.2, fill: BLKW });
  T(s, '타이머 없음', 1010, y0 + 52, 216, 16, { size: 9.5, color: BLK, mono: false, align: 'center', bold: true });
  T(s, '노드의 속도는 backend의 속도다', 1010, y0 + 72, 216, 14, { size: 7.5, color: MUT, align: 'center' });

  rule(s, 54, y0 + 196, 1172);
  T(s, 'backpressure는 같은 사슬을 거꾸로 흐른다', 54, y0 + 206, 500, 14, { size: 9, color: BND, mono: true, bold: true });
  const chain = ['peer 큐 full', 'dispatcher 정지', 'lane이 찬다', 'node outbox 정지', '다음 hop에서 느려진다', '생산자'];
  let cx = 54;
  chain.forEach((c, i) => {
    const w = [110, 130, 100, 140, 170, 70][i];
    T(s, c, cx, y0 + 228, w, 14, { size: 8.5, color: INK2, mono: false });
    if (i < chain.length - 1) line(s, cx + w + 4, y0 + 234, cx + w + 44, y0 + 234, { color: BND, lw: 1.2 });
    cx += w + 50;
  });
  T(s, '사슬은 일을 만들어내는 것에서 끝난다', 900, y0 + 206, 326, 14, { size: 8, color: MUT, align: 'right' });

  const dy = y0 + 262;
  const trio = [
    ['취소와 데드라인', '이 경계에서 결정된다. hop을 중단하는 방법은 없고 필요도 없다 — 다음 것을 시작하지 않는 것이 메커니즘 전부다.'],
    ['hop은 창을 나른다', '배치는 노드의 결정이며 load가 선언한 값에 갇히고 결코 파생되지 않는다. 창은 작업에서 멀지 않은 곳에서 구성한다.'],
    ['아무것도 값을 반환하지 않는다', '핸들러는 큐에 프레임을 놓는 것이 유일한 출력인 절차다. 호출 스택에 묶인 응답 경로는 그 프레임과 함께 죽으므로 continuation을 등록한다.'],
  ];
  trio.forEach(([k, v], i) => {
    const x = 54 + i * 396;
    h3(s, k, x, dy, 370);
    T(s, v, x, dy + 20, 370, 72, { size: 9.5, ls: 1.35 });
  });

  foot(s, '출처 layers/agent/src/node/runner(+events·handle·bound), node/window(compose), docs/architecture.md · run loop는 자기 work 채널이 닫힐 때 끝난다 — 그렇지 않으면 교체·삭제된 노드가 어댑터·큐·in-flight 맵과 함께 상주했다');
}

/* ════════════════════════════════════════════════════════
   08 — 계층 격리 스택
   ════════════════════════════════════════════════════════ */
{
  const s = slide('격리 계약', { eyebrow: '층별 책임과 의존 방향', h: '잦은 llama.cpp 변경이 전달 코어·비행 원장·배치 정책으로 번지지 않게 한다' }, 8,
    'llama.cpp는 단순 장치 wrapper가 아니다. 모델·graph·memory 실행을 소유하는 추상층이고, 그 아래 ggml/backend의 CUDA·CPU·Metal·HIP·Vulkan 구현은 따로 변한다.');
  const y0 = s.bodyTop;

  const L = [
    ['OUTER', '제품 목표 · 자원/토폴로지 · 요청/SLO · 스냅샷 트리거', 'engine KV 직접 변경 ✗ · wire ACK를 생성 완료로 ✗', 'b'],
    ['layers/protocol', 'event envelope · 주소/식별/라우팅/타입 경계', 'llama seq · ggml type · CUDA device ✗', 'f'],
    ['layers/agent', 'transport · broker · 순서/전달 · 노드 수명 · backpressure', 'prefill/decode 선택 ✗ · KV cell 단가 ✗', 'f'],
    ['adapters/adapter', 'offer/take · completion 계약 · 이벤트 소유권 · Full/Closed', '특정 engine의 private 타입·우회 캐스팅 ✗', 'f'],
    ['llamacpp/staged', 'L0 큐 → L1 원장 ↔ L2 수용 → L3 정책 → L4 증명 → L5 발행', 'P4 공용 해석기를 모델별로 확장 ✗', ''],
    ['native stage shell · engine facade', 'load/inspect/execute/settle/state/quiesce + opaque handle', 'llama_context 내부 · sampler ownership ✗', ''],
    ['engine bridge · private-common compat', '현재 pin의 공개 llama.h/ggml + 격리된 private stage hook', '공개 API도 불변 ABI로 가정 ✗', ''],
    ['llama.cpp 모델 · graph · memory · backend 스케줄러 추상층', '', 'upstream 소유', 'q'],
    ['ggml / backend — CUDA · CPU · Metal · HIP · Vulkan 구상 구현', '', '따로 변한다', 'q'],
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
  T(s, '↓  왼쪽 화살표: 의미 의존 방향', 86, yy + 6, 500, 16, { size: 8, color: FLOW });
  T(s, '↑  오른쪽 화살표: 결과·telemetry — 명시 계약을 통과하되 역방향 소유권을 만들지 않는다', 600, yy + 6, 540, 16, { size: 8, color: BND, align: 'right' });
  line(s, 1156, yy - 10, 1156, y0 - 4, { color: BND, dash: true, lw: 1.3 });


  foot(s, '출처 docs/layer-isolation-contract.md §2·§4 · 현재 event 경계는 adapter/src/node_adapter/mod.rs::NodeAdapter 와 agent/src/event_node/mod.rs::EventNode 이며 Adapter::start(Work) 의 이전 service 경계와 섞지 않는다');
}

/* ════════════════════════════════════════════════════════
   09 — 세 가지 합격 조건
   ════════════════════════════════════════════════════════ */
{
  const s = slide('격리 계약', { eyebrow: '합격 조건은 세 개다', h: '“include 0 · 컴파일 성공 · 이번 pin clean”은 격리 완료가 아니다' }, 9,
    '셋 중 하나만 통과한 리팩터를 전체 계층 격리 완료로 보고하지 않는다.');
  const y0 = s.bodyTop;

  const C = [
    {
      n: '01', k: '의존 격리', c: FLOW,
      p: '상위 코드가 하위 구현을 이름으로 알지 못하게 한다. 허용된 표면만 의존하도록 빌드가 강제한다.',
      b: ['수단: protocol·agent·adapter-contract crate는 구상 backend에 normal dependency를 갖지 않는다. 등록은 entrypoint에서 한다',
        'pure scheduler/ledger 시험은 llama checkout·GPU·네트워크 없이 빌드된다'],
      note: '실측 차이: layers/protocol은 [dependencies] 절 자체가 없다(의존 0). 그러나 adapters/adapter는 현재 p4-protocol·serde·serde_json에 의존한다 — docs/implementation.md의 “protocol조차 의존하지 않는다”는 그 시점 기준이다.',
      nk: 'x',
    },
    {
      n: '02', k: '권한 격리', c: BND,
      p: '전송·정책이 실행 원장이나 KV를 임의로 확정하지 못하게 한다. 층을 파일 이름으로만 나누는 것이 아니다.',
      b: ['기준: 누가 상태를 확정하고 누가 효과만 실행하는가',
        'L3의 결과는 후보다. 거부된 issue가 실행량·credit을 몰래 소비하지 않는다',
        'L1이 요청 delta·비행 원장·정산 receipt·출력 의도를 한 commit으로 확정한다',
        'L5는 효과를 실행하고 결과를 되돌린다. 불명확하면 Uncertain/fenced로 남긴다'],
      note: '정산 승인을 우회해 token/native 효과를 먼저 내보내지 않는다. 꼬리 engine의 성공은 원장 승인을 우회할 출력 권한이 아니다.',
      nk: 'b',
    },
    {
      n: '03', k: '의미 격리', c: BLK,
      p: '컴파일 가능한 upstream 변화도 기존 계약을 깨면 승격하지 못하게 한다.',
      b: ['수단: conformance가 의미 변경을 잡고, 제품 LOAD가 그 검증 결과와 실행 identity를 강제한다',
        '이행할 수 없는 capability를 광고하면 LOAD/conformance가 실패한다',
        'unknown 값을 “기본 CUDA”나 “지원”으로 자동 해석하지 않는다'],
      note: 'pin clean replay는 의미 호환 증명이 아니다. 불투명 클래스의 impl()이 공개되고 consumer가 internal header를 include할 수 있으면 완성된 경계가 아니다 — 실제 consumer 컴파일로 확인한다.',
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

  foot(s, '출처 docs/layer-isolation-contract.md §1·§3·§4 · 실측 layers/protocol/Cargo.toml, layers/adapters/adapter/Cargo.toml @429e057de');
}

/* ════════════════════════════════════════════════════════
   10 — L0–L5
   ════════════════════════════════════════════════════════ */
{
  const s = slide('어댑터', { eyebrow: '배치 레이어 L0–L5', h: '상태를 확정하는 층은 하나다' }, 10,
    'OUTER는 모델·토폴로지·SLO·요청 도착·스냅샷 트리거를 정하고, 어댑터가 적격 행과 배치를 구성한다. P4 코어에 llama 전용 배치/KV 규칙을 넣지 않는다.');
  const y0 = s.bodyTop;

  const Ls = [
    ['L0 큐', '도착·보류. 정책 없음', 'drop을 성공으로 ✗', ''],
    ['L1 원장', 'ID·상주·셀 회계의 단일 진실', 'atomic commit · 유일한 쓰기 권한', 'b'],
    ['L2 수용·점유', '예약·lease·quota·명시 거절', 'TTL/victim 발명 ✗', ''],
    ['L3 구성 정책', '순수·결정론적 선택', 'I/O·native call·mutation ✗', 'f'],
    ['L4 증명', 'shape·membership·position', 'byte budget 확인', ''],
    ['L5 발행·운반', 'stage 호출·캡슐 전달', 'ACK/terminal 혼동 ✗', ''],
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
  T(s, '후보 allocation', 592, y0 - 14, 168, 12, { size: 7.5, color: MUT, mono: true, align: 'center' });
  T(s, 'validated issue', 790, y0 - 14, 168, 12, { size: 7.5, color: MUT, mono: true, align: 'center' });
  line(s, 1080, y0 + 74, 1080, y0 + 108, { color: BND, dash: true, arrow: false, lw: 1.3 });
  line(s, 1080, y0 + 108, 336, y0 + 108, { color: BND, dash: true, arrow: false, lw: 1.3 });
  line(s, 336, y0 + 108, 336, y0 + 86, { color: BND, dash: true, lw: 1.3 });
  T(s, '실행 결과 · Full이면 의도 보관 · 실행 여부 불명이면 Uncertain — timeout을 미실행으로 가정해 재실행 ✗',
    360, y0 + 112, 700, 14, { size: 8, color: BND, align: 'center' });

  rule(s, 54, y0 + 140, 1172);
  T(s, '전송 credit 반환은 “peer가 인수했다”이지 compute/KV 완료 증거가 아니다 — 정지점의 증거는 stage별 SequenceQuiesced attest다.',
    54, y0 + 152, 1172, 18, { size: 10, color: INK2 });

  const by = y0 + 190;
  h3(s, '전략이 지켜야 할 불변식 (발췌)', 54, by, 560);
  bullets(s, [
    '상주 전제: 행이 각 스테이지에서 실행되기 전 그 스테이지의 prefix KV·보조 상태가 유효해야 한다 — 전 pipeline을 비우라는 뜻은 아니다',
    '증명 없는 제출 금지: 실행 실패 후 KV가 보존됐다고 가정하지 않는다',
    '스냅샷 정합 펜스: Persist·Fork는 in-flight 행이 전무하고 전 스테이지가 정산된 정지점에서만 — 배치 도중 export한 스냅샷은 position이 모호한 오답이다',
  ], 54, by + 22, 560, 130, { size: 9.5 });

  h3(s, '비용은 노드마다 다르다 (과거 관측)', 666, by, 560);
  const obs = [
    ['노드별 KV', '동일 n_ctx에 173.5 / 63.3 / 157.7 / 126.1 MB — 병목은 가장 비싼 노드'],
    ['compute buffer', '1,412MB@ubatch512 ↔ 386MB@128 — 배치 폭이 VRAM 지배 knob'],
    ['cut-set 폭', 'gemma-4: 31/27/23 텐서, 스텝당 81 전송. Qwen 계열은 1 — 모델별 상수'],
  ].map(([a, b]) => [
    { text: a, options: { fontFace: M, fontSize: 8.5, color: INK } },
    { text: b, options: { fontSize: 9 } },
  ]);
  table(s, obs, 666, by + 22, 560, [130, 430], { size: 9 });

  foot(s, '출처 docs/adapter-batching-layers.md(레이어·불변식·과거 관측), docs/layer-isolation-contract.md §3 · 관측값은 당시 조건 한정이며 현재 병목의 확정이 아니다');
}

/* ════════════════════════════════════════════════════════
   11 — staged 실행과 출력 권위
   ════════════════════════════════════════════════════════ */
{
  const s = slide('staged 실행', { eyebrow: '스테이지 사슬과 출력 권위', h: 'native 계산 위치와 출력 승인 권위는 다른 것이다' }, 11,
    '꼬리 노드가 결과를 계산해도, OUTER 출력은 정산을 소유한 head가 원장을 검증·commit한 뒤에만 발행된다.');
  const y0 = s.bodyTop;

  box(s, 200, y0, 200, 38, { fill: BNDW, line: BND, lw: 1.2 });
  T(s, 'OUTER', 200, y0 + 10, 200, 18, { size: 10, color: BND, mono: true, align: 'center', bold: true });
  line(s, 262, y0 + 92, 262, y0 + 42, { color: BND, lw: 1.3 });
  T(s, 'OUTPUT v4', 120, y0 + 56, 130, 12, { size: 7.5, color: BND, mono: true, align: 'right' });
  line(s, 340, y0 + 40, 340, y0 + 88, { color: MUT, dash: true });
  T(s, 'SESSION v4', 350, y0 + 56, 120, 12, { size: 7.5, color: MUT, mono: true });

  const st = [
    [y0 + 94, 'stage 0 — head', '정산·검증·commit·RELEASE 생성', 'f'],
    [y0 + 172, 'stage 1', 'next ≠ terminal (3-stage에서)', ''],
    [y0 + 242, 'stage 2 — terminal', 'native 결과를 head에 반환', ''],
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
  T(s, 'first/previous/next/terminal은 설치한 순서에서만 파생한다', 54, y0 + 312, 560, 14, { size: 8, color: MUT });
  T(s, '검사는 원장·KV·슬롯·출력을 바꾸기 전에 수행한다. head의 next와 terminal은 3-stage에서 서로 다르다.',
    54, y0 + 334, 560, 30, { size: 8.5, color: MUT, ls: 1.35 });

  h3(s, 'SESSION v4가 설치하는 것', 634, y0, 592, FLOW);
  bullets(s, [
    '필수: load_generation, session_id, 전체 순서 stages:[{agent,node,generation}, …], 수신 노드의 stage_index',
    'role/first/next 선언은 제거했다. 최상위 unknown 필드와 구 v3 명령은 거부한다',
    '노드 주소·nonzero generation·중복 없는 정체성·유효한 index를 검사하고, index의 전체 endpoint가 실제 worker와 envelope target과 같아야 설치한다',
    '같은 agent/node의 다른 generation을 동시에 다른 stage로 선언할 수 없다',
  ], 634, y0 + 22, 592, 140, { size: 9.5 });

  note(s, 634, y0 + 176, 592, 80,
    'OUTPUT의 envelope source는 configured first endpoint다. OUTER는 agent 주소·node ID·node generation을 포함한 endpoint 전체와 ' +
    'load/session/request, target/return route·correlation·position을 대조한다. 꼬리도 configured node라는 이유로 동시 허용하지 않는다.');
  note(s, 634, y0 + 270, 592, 96,
    '한계를 분명히 한다. 수신 노드별 검증만으로는 fleet 전체가 같은 선언을 받았다는 합의, 사용자/네트워크 인증, 프로세스 재시작 후 freshness를 증명하지 않는다. ' +
    '현재 stage 경로는 head/tail이 분리되는 2개 이상만 지원한다 — 단일 stage 구현이 없다는 제한이며 장치당 노드 수를 줄이라는 배치 정책이 아니다.', 'x');

  foot(s, '출처 docs/adapter-batching-layers.md(SESSION/OUTPUT v4 절) · 구현 staged/adapter/src/v2/(commands·worker·completion·issue_witness), tools/event-drive/src/run/mod.rs::session_events');
}

/* ════════════════════════════════════════════════════════
   12 — upstream 흡수
   ════════════════════════════════════════════════════════ */
{
  const s = slide('upstream', { eyebrow: 'llama.cpp 변경 흡수', h: '공식 체크아웃은 손대지 않는다. 패치는 pin별 디렉터리 하나가 소유한다' }, 12,
    '패치 하나라도 깨끗하게 적용되지 않으면 CMake 전에 멈춘다. 빌드를 통과시키려고 공식 체크아웃을 편집하지 않는다.');
  const y0 = s.bodyTop;

  T(s, '현재 후보 pin — ef6876693', 54, y0, 500, 14, { size: 9, color: FLOW, mono: true, bold: true });
  T(s, '공식 PR #27742 head · upstream_status “open-candidate” · nearest release b10645 · 자체 수용 증거 없이 안정 pack을 대체하지 않는다',
    54, y0 + 18, 620, 30, { size: 8, color: MUT, ls: 1.35 });
  T(s, '보관 중인 pin 9개', 706, y0, 520, 14, { size: 9, color: INK, mono: true, bold: true });
  T(s, '0eadefebd · 1269cb1ff · 3e3a7a416 · 4308a4f03 · 434ddbbc0 · 557614e02 · d7a207411 · ef6876693 · fe2adf0e7',
    706, y0 + 18, 520, 30, { size: 8, color: MUT, ls: 1.35 });

  const dy = y0 + 62;
  box(s, 54, dy + 46, 196, 62, { line: RULE2, dash: true });
  T(s, 'upstream/', 68, dy + 56, 170, 14, { size: 9, color: INK, mono: true });
  T(s, '공식 클론 · 원본 유지\ngit ignore · 커밋 안 함', 68, dy + 74, 170, 30, { size: 7.5, color: MUT, ls: 1.3 });
  line(s, 254, dy + 58, 326, dy + 58, { color: MUT });
  T(s, '무패치', 254, dy + 40, 72, 12, { size: 7.5, color: MUT, mono: true, align: 'center' });
  box(s, 328, dy + 36, 186, 46, { fill: SURF2 });
  T(s, 'stock 빌드', 342, dy + 46, 160, 14, { size: 9, color: INK, mono: true });
  T(s, 'ggml-rpc-server · llama-server', 342, dy + 62, 160, 14, { size: 7.5, color: MUT });

  line(s, 152, dy + 112, 152, dy + 142, { color: FLOW, lw: 1.4 });
  box(s, 54, dy + 144, 196, 58, { fill: FLOWW, line: FLOW, lw: 1.2 });
  T(s, 'prepare-pipeline-\nupstream.mjs', 68, dy + 152, 170, 28, { size: 8.5, color: INK, mono: true, ls: 1.2 });
  T(s, '두 번 실행: 생성 → 재사용 검증', 68, dy + 182, 170, 14, { size: 7.5, color: MUT });

  box(s, 300, dy + 128, 216, 80, { fill: BNDW, line: BND, lw: 1.2 });
  T(s, 'compat/<pin>/', 314, dy + 138, 190, 14, { size: 9, color: BND, mono: true, bold: true });
  T(s, '0001…0021 순서 패치\nmanifest.json — 패치별 sha256,\npatch_set_sha256, patched_tree',
    314, dy + 156, 190, 46, { size: 7.5, color: MUT, ls: 1.3 });
  line(s, 296, dy + 158, 254, dy + 166, { color: BND, lw: 1.3 });
  T(s, '해시 검증', 244, dy + 138, 60, 12, { size: 7.5, color: BND, mono: true, align: 'center' });

  line(s, 520, dy + 168, 582, dy + 168, { color: FLOW, lw: 1.4 });
  box(s, 584, dy + 138, 210, 62, { fill: SURF2 });
  T(s, '.cache/ worktree', 598, dy + 148, 184, 14, { size: 9, color: INK, mono: true });
  T(s, '생성된 패치 소스\n커밋하지 않는다', 598, dy + 166, 184, 30, { size: 7.5, color: MUT, ls: 1.3 });
  line(s, 798, dy + 168, 860, dy + 168, { color: FLOW, lw: 1.4 });
  box(s, 862, dy + 138, 212, 62, { fill: FLOWW, line: FLOW, lw: 1.2 });
  T(s, 'pipeline 빌드', 876, dy + 148, 186, 14, { size: 9, color: INK, mono: true });
  T(s, 'abi_revision: pipeline-abi-v1\n필수 심볼 2개 확인', 876, dy + 166, 186, 30, { size: 7.5, color: MUT, ls: 1.3 });
  line(s, 1078, dy + 168, 1124, dy + 168, { color: FLOW, lw: 1.4 });
  box(s, 1080, dy + 36, 146, 72, { fill: BLKW, line: BLK, lw: 1.2 });
  T(s, 'fleet 승격', 1080, dy + 48, 146, 16, { size: 9.5, color: BLK, align: 'center', bold: true });
  T(s, 'CUDA·Metal·OpenCL\n실적재 후', 1080, dy + 70, 146, 32, { size: 7.5, color: MUT, align: 'center', ls: 1.3 });
  line(s, 1152, dy + 136, 1152, dy + 112, { color: BLK, lw: 1.3 });

  const ny = y0 + 310;
  note(s, 54, ny, 576, 104,
    '패치가 건드리는 곳이 곧 격리 지점이다. ggml-backend·ggml-rpc, public pipeline ABI, llama-context/graph/memory 헤더, model-loader, ' +
    'KV stage window, stage memory residency(+recurrent), MTP tail stage, speculative sequence lifecycle, stage noalloc memory breakdown — ' +
    '전부 compat 경계 안이며 위층으로 올라오지 않는다.', 'f');
  note(s, 650, ny, 576, 104,
    'upstream 적응은 허용 모듈 안에서 끝낸다. direct include만이 아니라 transitive include·링크·전방 선언·public signature·imported relink를 ' +
    '함께 검사한다. allowlist를 늘린 뒤 “침범 0”이라고 보고하지 않는다.');

  foot(s, '출처 staged/compat/ef6876693/{README.md, manifest.json}(실측: 패치 21개·필수 심볼 2개), docs/layer-isolation-contract.md §4 · pin 개수는 ls staged/compat 실측');
}

/* ════════════════════════════════════════════════════════
   13 — 코드 지형
   ════════════════════════════════════════════════════════ */
{
  const s = slide('코드 지형', { eyebrow: 'crate별 실측', h: '무게는 어댑터에 있고, 경계는 의존이 없는 곳에 있다' }, 13,
    'Rust 399파일 108,815줄(시험 포함) · staged C++ stage runtime 14,597줄. wc -l @429e057de.');
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
  T(s, '경계 crate — 여기서 컴파일 에러가 규약을 강제한다', AX + 16, y0 + 343, 420, 14, { size: 8, color: MUT });

  h3(s, '두 개의 경계 crate', 700, y0, 526);
  const deps = [
    ['p4-protocol', '없음 — [dependencies] 절 자체가 없다. 프로토콜은 backend를 배울 수 없다'],
    ['p4-adapter', 'p4-protocol · serde · serde_json'],
    ['p4-agent-core', 'p4-adapter · p4-protocol · tokio'],
    ['p4-mock', 'p4-adapter · tokio — 산술만으로 인터페이스 전체를 구현하는 두 번째 구현'],
  ].map(([a, b]) => [
    { text: a, options: { fontFace: M, fontSize: 8.5, color: INK } },
    { text: b, options: { fontSize: 9 } },
  ]);
  table(s, deps, 700, y0 + 22, 526, [126, 400], { size: 9 });

  note(s, 700, y0 + 150, 526, 96,
    '문서와의 차이를 그대로 둔다. docs/implementation.md는 “9 crate · 16,447줄”, “adapter 계약은 protocol조차 의존하지 않는다”로 적혀 있다. ' +
    '현재 workspace는 12 crate이고 adapter 계약은 protocol·serde에 의존한다. 그 표는 event 경로 이전 기준이며, 이 장표는 실측을 따른다.', 'x');
  note(s, 700, y0 + 260, 526, 96,
    'backend 추가 비용은 한 파일이다. entrypoints/agent/src/adapters/mod.rs — 이름 하나, factory 하나, Adapter 구현 하나. ' +
    'agent의 유일한 표면은 소켓 위의 P4다. 어댑터가 자기 backend와 HTTP로 말하든 파이프로 말하든 그 위에서는 보이지 않는다.', 'f');

  foot(s, "실측 명령 find layers entrypoints tools -name '*.rs' | xargs wc -l @429e057de · Cargo.toml 직접 확인 · C++은 staged/server/src 의 .cpp/.hpp/.inc 합계");
}

/* ════════════════════════════════════════════════════════
   14 — 불변식
   ════════════════════════════════════════════════════════ */
{
  const s = slide('불변식', { eyebrow: '없앴을 때 일어난 일', h: '대부분의 불변식은 한 번 깨졌기 때문에 여기 있다' }, 14);
  const y0 = s.bodyTop;

  const head = [
    { text: '불변식', options: { fontFace: M, fontSize: 8, color: MUT, bold: true, border: [{ type: 'none' }, { type: 'none' }, { pt: 1, color: RULE2 }, { type: 'none' }] } },
    { text: '없이 무엇이 잘못됐는가', options: { fontFace: M, fontSize: 8, color: MUT, bold: true, border: [{ type: 'none' }, { type: 'none' }, { pt: 1, color: RULE2 }, { type: 'none' }] } },
  ];
  const body = [
    ['소켓 리더는 큐에 넣기만 한다', '핸들러가 read loop 안에서 돌아, 모든 핸들러의 실행 시간이 거기 쌓였다'],
    ['노드의 큐가 긴 작업을 쥔다', '그렇지 않으면 GPU 시간이 agent 큐 깊이로 나타나고 아무것도 귀속할 수 없다'],
    ['한 route, 한 worker', '한 route의 프레임이 여러 worker에서 경합하고, 호출자에게는 P4가 스트림을 재정렬한 것으로 보인다'],
    ['노드의 select는 편향되지 않는다', '완료를 선호하자 도착이 완전히 굶었다 — 채널에 앉아, 어떤 큐에도 없이, 보이지 않게'],
    ['광고한 주소는 peer가 닿을 수 있는 주소다', '두 agent가 모두 자신을 127.0.0.1로 불러 prefill을 끝내고 한 토큰 뒤 멈췄다 — lap이 프레임을 쥔 쪽 기계로 해소됐다'],
    ['광고 힌트는 파싱하고 이어붙이지 않는다', '포트를 가진 힌트에 바인딩 포트를 덧붙여 HOST:52001:52001 이 됐다 — 파싱되고, 아무것도 resolve되지 않으며, 스스로 ready라고 보고한다'],
    ['노드 run loop는 handle이 drop되면 끝난다', '노드가 자기 event sender를 쥐므로 그 채널을 기다리는 루프는 자신을 기다린다. 교체·삭제된 모든 노드가 상주했다'],
    ['조용한 peer는 해제된다', 'peer 맵이 본 적 있는 모든 주소에 대해 append-only였다 — 고정 fleet에서는 유계, 호출자가 새 포트에서 돌아오는 순간 무계'],
    ['선언된 프레임 길이는 할당 전에 제한된다', '헤더는 16바이트이고 무엇이든 주장할 수 있다. 상한이 그 함정의 비용을 프로세스가 아니라 연결당 1.25MB로 만든다'],
    ['사슬의 끝만 토큰을 생산한다', '중간 stage가 함께 세면 n-stage 사슬이 lap마다 n개의 토큰을 낸다'],
    ['만료된 작업은 버리지 않고 답한다', '오지 않는 terminal을 기다리는 호출자 — 그것이 누출된 route의 모습이다'],
  ].map(([a, b]) => [
    { text: a, options: { fontSize: 9, color: INK, bold: true } },
    { text: b, options: { fontSize: 9 } },
  ]);
  table(s, [head, ...body], 54, y0, 1172, [330, 842], { size: 9, rowH: 38 });

  foot(s, '출처 docs/constraints.md · 전체 표는 그 문서가 소유한다. 여기 실린 것은 발췌이며, 각 행은 수정 당시의 실패 증거와 결속돼 있다');
}

/* ════════════════════════════════════════════════════════
   15 — 검증 게이트
   ════════════════════════════════════════════════════════ */
{
  const s = slide('검증', { eyebrow: '게이트와 2026-09-11 선별', h: '단위 시험·시뮬레이터·한 컴퓨터의 여러 프로세스는 최종 성과 증명이 아니다' }, 15);
  const y0 = s.bodyTop;

  T(s, '최종 성과는 다중 컴퓨터 실기 웨이브 증거로만 승인한다', 54, y0, 700, 14, { size: 9, color: FLOW, mono: true, bold: true });
  const G = [
    ['pure 단위·변이', 'GPU·네트워크 없이', ''],
    ['mock 분산', '산술 backend', ''],
    ['단일 호스트 다중 프로세스', '성과 증명 아님', ''],
    ['두 클러스터 실기 선별', 'MI250 · Hy3 동시', 'f'],
    ['100k 연속 웨이브', '긴 정상 응답·오프로딩', 'q'],
    ['H5 성능·서비스 승인', 'paired 8쌍 / holdout 4쌍', 'x'],
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
  T(s, '▼ 현재 위치', 642, gy - 20, 168, 14, { size: 8, color: FLOW, mono: true, align: 'center' });
  T(s, 'BLOCKED', 1034, gy + 56, 168, 14, { size: 8.5, color: BLK, mono: true, align: 'center', bold: true });
  T(s, '노드 수는 모델·KV 용량·합법적 컷·배포 제약이다. 카드당 한 노드 제한을 일반 규칙으로 만들지 않는다.',
    54, gy + 82, 900, 14, { size: 8, color: MUT });

  const ty = y0 + 158;
  h3(s, '2026-09-11 동시 선별 — 인용값', 54, ty, 640, FLOW);
  const rows = [
    ['Hy3 · 5호스트 / 6stage', 'decode cap 0→2에서 5.32 → 9.55 TPS, ITL p50 1.179 → 0.533 s, 양쪽 8/8 완료·해제·UNLOAD'],
    ['MI250 · 2호스트 / 16stage', 'native CPU threads4 + cap4/min4 후보가 34.05 TPS, ITL p50 0.319 s, 16/16 완료·해제·UNLOAD'],
    ['MI250 · 기본 CPU', 'cap4 실패, cap8 8.35 TPS (기준 28.54 / 29.73) — 회귀'],
    ['로컬 최종 게이트', 'workspace 1384 / 0 / 7 (58 summary, filtered 0) · 변이 실패 1/5/3/1 · docs-lint 92 clean'],
  ].map(([a, b]) => [
    { text: a, options: { fontFace: M, fontSize: 8.5, color: INK } },
    { text: b, options: { fontSize: 9 } },
  ]);
  table(s, rows, 54, ty + 22, 640, [186, 454], { size: 9, rowH: 42 });

  note(s, 730, ty, 496, 110,
    '이 수치가 아닌 것. 같은 CPU4 기준군은 13.59 TPS로 완주했지만 외부 Python GPU 작업이 겹쳐 배치 단독 개선율 비교에서 제외했다. ' +
    'GPU 비간섭 시간대 재검증 전 개선율 승격은 BLOCKED다. Hy3도 한 쌍의 선별이며 context 100k·출력 256의 짧은 부하다 — ' +
    '실제 100k prefill·정상 EOS·연속 웨이브 승인이 아니다.', 'x');
  note(s, 730, ty + 124, 496, 108,
    '합치지 않는 것. 서로 다른 upstream·모델·backend·워크로드의 TPS를 합산하지 않는다. 8-stage 301.63과 16-stage 7.64는 ' +
    'resident 256/16, 짧은 입력/100k 혼합 등 조건이 달라 노드 수 손실률·최적값으로 승인하지 않고 토폴로지별 기준선을 새로 고정한다. ' +
    'GPU 사용률·RPC 깊이·행 수만으로 성능이나 정상 응답을 승인하지 않는다.');

  foot(s, '인용 docs/distributed-batching-roadmap.md §0(2026-09-11) · 원자료·해시 staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md · 판정 계약 docs/distributed-batching-verification.md §v11-gates');
}

/* ════════════════════════════════════════════════════════
   16 — 실험 합성과 다음
   ════════════════════════════════════════════════════════ */
{
  const s = slide('다음', { eyebrow: '실험 합성과 V1.1 순서', h: '모델별 브랜치 대신, 입력의 조합으로 실험을 만든다' }, 16,
    '장기 개발·릴리즈 기준은 main 하나다. 모델 이름은 데이터를 고르는 것이고 Git 브랜치를 고르는 것이 아니다.');
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
  T(s, '합성기가 거부하는 것', 54, y0 + 224, 300, 14, { size: 9, color: BLK, mono: true, bold: true });
  T(s, '기존 출력 디렉터리 · wave 불일치 · runtime identity 누락 · 중복 native thread 설정 · 측정된 prompt+출력의 context 초과\n' +
    'tokenCounts가 없으면 tokenizer_verified=false — 실제 입력 주장으로 쓰지 않는다',
    54, y0 + 244, 574, 44, { size: 8.5, color: MUT, ls: 1.4 });
  T(s, '이 모듈은 입력을 준비한다. agent 기동·정리, 호스트 가용성, 실제 바이너리 해시 검증, 절대 시간 강제와 실행은 cluster lifecycle 실행기의 책임이다.',
    54, y0 + 300, 574, 32, { size: 8, color: MUT, ls: 1.35 });

  h3(s, 'V1.1 구현 순서 — 다음 첫 구현은 V1.1-0', 666, y0, 560, FLOW);
  const v = [
    ['V1.1-0', '계측 결속 — 유효 환경설정, 요청별 수용·eligible·blocked 사유, head 단조시계 issue→settle, stage queue/native/forward 분해'],
    ['V1.1-1', '예산과 종료 — pending prompt·KV·전송 payload·출력·완료 receipt를 각각 제한. 한계 초과는 부작용 전 거부'],
    ['V1.1-2', '배치 구성 — decode 묶음 상한과 prefill quantum을 독립 정책으로. decode outstanding ≤ 1 유지'],
    ['V1.1-3', '파이프라인 창 — prefill fragment 1→2→4→8을 단계별 검증. node 실행 credit = 1'],
    ['V1.1-4', '실기 승격 — 짧은 대조 → 후보 선택 → 100k 연속 웨이브·긴 정상 응답·장기 반복'],
  ].map(([a, b]) => [
    { text: a, options: { fontFace: M, fontSize: 8.5, color: FLOW, bold: true } },
    { text: b, options: { fontSize: 9 } },
  ]);
  table(s, v, 666, y0 + 22, 560, [72, 488], { size: 9, rowH: 44 });

  note(s, 666, y0 + 262, 560, 92,
    '완료가 아닌 것을 완료로 적지 않는다. TLS·인증·인가 없음(신뢰망 전용) · 내구 상태 없음(재시작한 agent에는 노드가 없고 OUTER가 기록을 쥔다) · ' +
    '토큰 종류 필드 없음(reasoning과 답 텍스트가 합쳐진다) · Windows ARM64 미실행. 새 정책은 기본 비활성이고 ordinary attention에만 적용한다.', 'x');

  foot(s, '출처 test/benchmarks/cluster-inference/README.md, docs/distributed-batching-roadmap.md §0.V1.1, docs/implementation.md(미구현 절) · 버전 bump/tag는 아직 하지 않는다');
}

const out = process.argv[2] || 'p4-architecture.pptx';
pres.writeFile({ fileName: out }).then(() => console.log('wrote', out));
