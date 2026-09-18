#!/usr/bin/env node
/**
 * Keep the S5 readiness review current.
 *
 * The review is judgement — what is proven, what is missing, what an investor
 * will ask — and judgement should not be regenerated every morning by a script.
 * But the part of it that goes stale overnight is not judgement at all: whether
 * the agents are up, which stages they hold, whether the gateway answers, what
 * landed in the repository. So only that part is rewritten, inside a marked
 * block, and the reviewed text around it is left exactly as written.
 *
 * Both languages are kept in step: a Korean reader and an English reader must
 * not see different facts.
 *
 * Usage: node readiness.mjs <collect.json>
 */
import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import path from 'node:path';
import { fetchEvents, weekStart, renderWeek } from './calendar.mjs';
import { pipeline } from './seed.mjs';
import { toEnglish, hasHangul } from './translate.mjs';

const HERE = path.dirname(new URL(import.meta.url).pathname);
const config = JSON.parse(readFileSync(
  process.env.KVASIR_WATCH_CONFIG ?? path.join(HERE, 'config.json'), 'utf8',
));
const data = JSON.parse(readFileSync(process.argv[2], 'utf8'));

const OPEN = '<!-- LIVE:STATUS -->';
const CLOSE = '<!-- /LIVE:STATUS -->';
const CAL_OPEN = '<!-- LIVE:CALENDAR -->';
const CAL_CLOSE = '<!-- /LIVE:CALENDAR -->';
const PIPE_OPEN = '<!-- LIVE:PIPELINE -->';
const PIPE_CLOSE = '<!-- /LIVE:PIPELINE -->';

const esc = (value) => String(value ?? '').replace(/[&<>]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;' }[c]));
const byName = (probes, name) => probes.find((probe) => probe.name === name);
const named = (probes, prefix) => probes.filter((probe) => probe.name.startsWith(prefix));

/** The document's own vocabulary: pass / warn / fail pills inside .item blocks. */
const item = (name, pill, tone, body) =>
  `<div class="item"><div class="top"><span class="name">${esc(name)}</span>` +
  `<span class="pill ${tone}">${esc(pill)}</span></div><p>${body}</p></div>`;

const T = {
  en: {
    heading: 'Live status',
    sub: (stamp) => `Rewritten automatically from the machines and the repository — ${esc(stamp)}. Everything else on this page is the reviewed text.`,
    ring: 'Ring',
    gateway: 'Settlement gateway',
    client: 'Client app',
    serving: 'Serving',
    degraded: 'Degraded',
    unreachable: 'Unreachable',
    pausedOnPurpose: 'Paused on purpose',
    down: 'Down',
    up: 'Up',
    agentsUp: (n, total) => `${n} of ${total} agents up`,
    stagesLine: (rows) => rows.join(' · '),
    noStages: 'no stages held',
    gatewayOffline: (why) => `The public endpoint does not answer (${esc(why)}). This is deliberate: the gateway is stopped until it is ported to p4.`,
    gatewayUp: (status, ms) => `The public endpoint answered ${status} in ${ms} ms.`,
    siteUp: (status, ms, version) => `kvasir-ai.net answered ${status} in ${ms} ms. Desktop wallet ${esc(version)}.`,
    siteDown: (why) => `kvasir-ai.net did not answer (${esc(why)}).`,
    commits: (n, files) => `${n} commit${n === 1 ? '' : 's'}${files ? `, ${files} files` : ''} in the last ${data.sinceDays} day${data.sinceDays === 1 ? '' : 's'}.`,
    couldNotRead: (what) => `Could not read: ${esc(what)}.`,
    calHeading: 'This week',
    calSub: (range, zone) => `${esc(range)} · ${esc(zone)}. Meetings come from the team calendar; the marked days are the plan's own dates.`,
    calNoFeed: 'No calendar feed is configured, so only plan dates are shown. Add the calendar\'s secret iCal address to the monitoring config to include meetings.',
    calFeedFailed: (why) => `The calendar feed could not be read (${esc(why)}), so only plan dates are shown.`,
    calNext: 'Next after this week',
    calNothing: 'Nothing scheduled in this window.',
    allDay: 'all day',
    days: ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'],
    pipeHeading: 'Funding pipeline',
    pipeSub: (n, at) => `${n} programmes tracked, read from the pipeline at ${esc(at)}. Listed here: everything we are in the middle of, and everything that closes within 60 days. A status changes only when a person changes it.`,
    pipeCols: ['Programme', 'Where', 'Closes', 'Status', 'What it needs, where we stand'],
    pipeNone: 'Nothing is live and nothing closes in the next 60 days.',
    pipeUnread: (why) => `The pipeline could not be read (${esc(why)}).`,
    pipeStale: (why) => `Shown from the last copy that could be read — the refresh failed (${esc(why)}).`,
    pipeDays: (d) => (d < 0 ? `${-d}d ago` : d === 0 ? 'today' : `in ${d}d`),
    pipeNoDate: 'rolling',
    pipeKoOnly: '(note is in Korean — see the Korean edition)',
    pipeStatus: {},
  },
  ko: {
    heading: '실시간 상태',
    sub: (stamp) => `머신과 저장소에서 자동으로 다시 쓴 절입니다 — ${esc(stamp)}. 나머지 본문은 검토된 원문 그대로입니다.`,
    ring: '링',
    gateway: '정산 게이트웨이',
    client: '클라이언트 앱',
    serving: '서빙 중',
    degraded: '일부 이상',
    unreachable: '확인 불가',
    pausedOnPurpose: '의도적 정지',
    down: '응답 없음',
    up: '정상',
    agentsUp: (n, total) => `에이전트 ${total}대 중 ${n}대 가동`,
    stagesLine: (rows) => rows.join(' · '),
    noStages: '보유 스테이지 없음',
    gatewayOffline: (why) => `공개 엔드포인트가 응답하지 않습니다(${esc(why)}). p4 이식 전까지 의도적으로 내려 둔 상태입니다.`,
    gatewayUp: (status, ms) => `공개 엔드포인트가 ${status}로 ${ms} ms 만에 응답했습니다.`,
    siteUp: (status, ms, version) => `kvasir-ai.net이 ${status}로 ${ms} ms 만에 응답했습니다. 데스크탑 지갑 ${esc(version)}.`,
    siteDown: (why) => `kvasir-ai.net이 응답하지 않았습니다(${esc(why)}).`,
    commits: (n, files) => `최근 ${data.sinceDays}일간 커밋 ${n}건${files ? `, 파일 ${files}개` : ''}.`,
    couldNotRead: (what) => `확인 실패: ${esc(what)}.`,
    calHeading: '이번 주',
    calSub: (range, zone) => `${esc(range)} · ${esc(zone)}. 미팅은 팀 캘린더에서 가져오고, 표시된 날짜는 계획상의 일정입니다.`,
    calNoFeed: '캘린더 피드가 설정되지 않아 계획 일정만 표시합니다. 모니터링 설정에 캘린더의 비공개 iCal 주소를 넣으면 미팅도 함께 나옵니다.',
    calFeedFailed: (why) => `캘린더 피드를 읽지 못해(${esc(why)}) 계획 일정만 표시합니다.`,
    calNext: '이번 주 이후',
    calNothing: '이 기간에 잡힌 일정이 없습니다.',
    allDay: '종일',
    days: ['월', '화', '수', '목', '금', '토', '일'],
    pipeHeading: '자금 파이프라인',
    pipeSub: (n, at) => `추적 중인 프로그램 ${n}건, ${esc(at)} 기준으로 파이프라인에서 읽었습니다. 여기 실린 것은 진행 중인 건과 60일 안에 마감되는 건뿐입니다. 상태는 사람이 바꿀 때만 바뀝니다.`,
    pipeCols: ['프로그램', '지역', '마감', '상태', '요구 조건과 진행 상황'],
    pipeNone: '진행 중인 건이 없고 60일 안에 마감되는 것도 없습니다.',
    pipeUnread: (why) => `파이프라인을 읽지 못했습니다(${esc(why)}).`,
    pipeStale: (why) => `갱신에 실패해(${esc(why)}) 마지막으로 읽힌 사본을 표시합니다.`,
    pipeDays: (d) => (d < 0 ? `${-d}일 지남` : d === 0 ? '오늘' : `${d}일 남음`),
    pipeNoDate: '상시',
    pipeKoOnly: '',
    pipeStatus: { 'Applied': '지원 완료', 'Researching': '검토 중', 'Not started': '미착수', 'Rejected': '탈락', 'Accepted': '선정' },
  },
};

function ringItem(t) {
  const probes = data.tracks.ring ?? [];
  const hosts = named(probes, 'host:');
  const stages = named(probes, 'stages:');
  const up = hosts.filter((probe) => probe.ok && probe.value.running).length;
  const lines = stages.map((probe) => {
    if (!probe.ok) return `${esc(probe.name.slice(7))}: ${t.unreachable}`;
    const { label, nodes } = probe.value;
    return nodes.length
      ? `${esc(label)}: ${esc(nodes.map((node) => `${node.node} ${node.state}`).join(', '))}`
      : `${esc(label)}: ${t.noStages}`;
  });
  const allLoaded = stages.length > 0 && stages.every(
    (probe) => probe.ok && probe.value.nodes.length && probe.value.nodes.every((node) => node.state === 'loaded'),
  );
  const tone = allLoaded && up === hosts.length ? 'pass' : up > 0 ? 'warn' : 'fail';
  const pill = allLoaded && up === hosts.length ? t.serving : up > 0 ? t.degraded : t.unreachable;
  const commits = byName(probes, 'commits');
  const files = byName(probes, 'files');
  const body = [
    `${t.agentsUp(up, hosts.length)}. ${t.stagesLine(lines)}.`,
    commits?.ok ? t.commits(commits.value.length, files?.ok ? files.value : null) : t.couldNotRead('git'),
  ].join(' ');
  return item(t.ring, pill, tone, body);
}

function gatewayItem(t) {
  const probes = data.tracks.gateway ?? [];
  const probe = byName(probes, 'public');
  const commits = byName(probes, 'commits');
  const files = byName(probes, 'files');
  const answered = probe?.ok && probe.value.ok;
  const body = [
    answered ? t.gatewayUp(probe.value.status, probe.value.ms) : t.gatewayOffline(probe?.ok ? probe.value.status : (probe?.error ?? '—')),
    commits?.ok ? t.commits(commits.value.length, files?.ok ? files.value : null) : '',
  ].filter(Boolean).join(' ');
  return item(t.gateway, answered ? t.up : t.pausedOnPurpose, answered ? 'pass' : 'warn', body);
}

function clientItem(t) {
  const probes = data.tracks.client ?? [];
  const site = byName(probes, 'site');
  const version = byName(probes, 'version');
  const commits = byName(probes, 'commits');
  const files = byName(probes, 'files');
  const label = version?.ok && version.value ? `${version.value.name} ${version.value.version}` : '—';
  const body = [
    site?.ok && site.value.ok ? t.siteUp(site.value.status, site.value.ms, label) : t.siteDown(site?.error ?? String(site?.value?.status ?? '—')),
    commits?.ok ? t.commits(commits.value.length, files?.ok ? files.value : null) : '',
  ].filter(Boolean).join(' ');
  return item(t.client, site?.ok && site.value.ok ? t.up : t.down, site?.ok && site.value.ok ? 'pass' : 'fail', body);
}

function render(lang) {
  const t = T[lang];
  const stamp = data.generatedAt.replace('T', ' ').slice(0, 16) + ' UTC';
  return [
    OPEN,
    '  <section id="live">',
    `    <h2>${t.heading}</h2>`,
    `    <p class="sub">${t.sub(stamp)}</p>`,
    '    <div class="items">',
    `      ${ringItem(t)}`,
    `      ${gatewayItem(t)}`,
    `      ${clientItem(t)}`,
    '    </div>',
    '  </section>',
    CLOSE,
  ].join('\n');
}

const CAL_STYLE = `<style>
    .cal { width: 100%; table-layout: fixed; border-collapse: collapse; }
    .cal th { font-size: 11px; letter-spacing: .08em; text-transform: uppercase; opacity: .6; padding: 6px 4px; text-align: left; }
    .cal td { vertical-align: top; padding: 8px 6px; border-top: 1px solid rgba(127,127,127,.25); min-height: 84px; }
    .cal .cal-date { font-size: 12px; opacity: .55; margin-bottom: 6px; }
    .cal .cal-today { background: rgba(127,127,127,.10); }
    .cal .cal-today .cal-date { opacity: 1; font-weight: 600; }
    .cal .cal-event { font-size: 12px; line-height: 1.35; margin-bottom: 5px; }
    .cal .cal-time { opacity: .6; font-variant-numeric: tabular-nums; }
    .cal .cal-plan { font-size: 12px; line-height: 1.35; margin-bottom: 5px; font-weight: 600; }
  </style>`;

const DAY_MS = 86_400_000;

async function calendarBlock() {
  const settings = config.calendar ?? {};
  const offset = Number(settings.offsetMinutes ?? 0);
  const zone = settings.timeZone ?? 'UTC';
  const now = new Date(data.generatedAt);
  const start = weekStart(now, offset);
  const end = start + 7 * DAY_MS;

  const planFile = path.resolve(HERE, settings.plan ?? 'plan.json');
  const plan = existsSync(planFile) ? JSON.parse(readFileSync(planFile, 'utf8')) : { milestones: [] };
  const milestones = plan.milestones ?? [];

  // The feed address is a credential — anyone holding it can read the calendar
  // — so it lives in the environment, not in a config file that gets committed.
  const icsUrl = settings.icsUrl || process.env.KVASIR_CALENDAR_ICS || '';
  let events = [];
  let problem = null;
  if (icsUrl) {
    try { events = await fetchEvents(icsUrl, start, end); }
    catch (error) { problem = error.message; }
  }

  // The English edition carries no Hangul — that is the standing rule for
  // anything an investor reads, and a calendar entry written in Korean walked
  // straight past it until now. A title that cannot be translated is named
  // generically rather than published in Korean, and the runner says which one.
  const english = await toEnglish(events.map((event) => event.summary));
  const untranslated = events
    .filter((event) => hasHangul(event.summary) && !english.get(event.summary))
    .map((event) => event.summary);
  if (untranslated.length) {
    console.error(`calendar titles left in Korean, shown generically on the English page: ${untranslated.join(' | ')}`);
  }
  const eventsFor = (lang) => (lang !== 'en' ? events : events.map((event) => ({
    ...event,
    summary: english.get(event.summary) ?? (hasHangul(event.summary) ? 'Meeting' : event.summary),
  })));

  const iso = (at) => new Date(at + offset * 60_000).toISOString().slice(0, 10);
  const range = `${iso(start)} – ${iso(end - DAY_MS)}`;
  const today = iso(now.getTime());
  const upcoming = milestones
    .filter((milestone) => milestone.date >= iso(end))
    .sort((a, b) => a.date.localeCompare(b.date))
    .slice(0, 4);

  return (lang) => {
    const t = T[lang];
    const note = !icsUrl ? t.calNoFeed : problem ? t.calFeedFailed(problem) : null;
    const grid = renderWeek({
      start, offsetMinutes: offset, events: eventsFor(lang), milestones,
      labels: { days: t.days, allDay: t.allDay }, today,
    });
    const next = upcoming.length
      ? `<p class="sub">${t.calNext}: ${upcoming.map((m) => `${esc(m.date)} — ${esc(m.label)}`).join(' · ')}</p>`
      : (events.length || milestones.some((m) => m.date >= iso(start) && m.date < iso(end)) ? '' : `<p class="sub">${t.calNothing}</p>`);
    return [
      CAL_OPEN,
      '  <section id="calendar">',
      `    <h2>${t.calHeading}</h2>`,
      `    <p class="sub">${t.calSub(range, zone)}</p>`,
      `    ${CAL_STYLE}`,
      `    ${grid}`,
      note ? `    <p class="sub">${note}</p>` : '',
      next ? `    ${next}` : '',
      '  </section>',
      CAL_CLOSE,
    ].filter(Boolean).join('\n');
  };
}

const renderCalendar = await calendarBlock();

const HANGUL = /[\u1100-\u11FF\u3130-\u318F\uAC00-\uD7AF]/;

/** The few enum-ish values the pipeline stores in Korean. */
const ENUM_EN = {
  '공개 지원': 'Open application',
  '확인 불가': 'Not stated',
  '초청': 'By invitation',
  '이메일': 'Email',
};

/**
 * The English edition carries no Hangul — that is a standing rule for anything
 * an investor may read, and a pipeline note written in Korean would break it
 * silently. Known values are translated; anything else is withheld and named in
 * the runner's output, so the fix happens in the row rather than in the page.
 */
function english(text, missing, row, field) {
  const value = String(text ?? '').trim();
  if (!value) return '';
  if (ENUM_EN[value]) return ENUM_EN[value];
  if (HANGUL.test(value)) { missing.push(`${row.id}.${field}`); return null; }
  return value;
}

/**
 * The research behind a row is a paragraph; the table needs the part that
 * decides something. Cut at a sentence boundary so the cell ends on a thought
 * rather than mid-clause, and leave the owner's own note whole — that one is
 * the current fact, not background.
 */
function gist(text, limit = 220) {
  const value = String(text ?? '').trim();
  if (value.length <= limit) return value;
  const cut = value.slice(0, limit);
  const stop = Math.max(cut.lastIndexOf('. '), cut.lastIndexOf('? '), cut.lastIndexOf('! '));
  return (stop > limit * 0.4 ? cut.slice(0, stop + 1) : cut.replace(/\s+\S*$/, '') + '…');
}

/**
 * Where the money is coming from, and when each door shuts.
 *
 * The review is about one deadline, but that deadline sits in a list of them,
 * and a reader deciding what to spend the week on needs to see the others. Only
 * what is live and what closes inside two months: the rest of the table is
 * research, and research on a page about readiness reads as padding.
 */
async function pipelineBlock() {
  const entry = await pipeline().catch(() => null);

  return (lang) => {
    const t = T[lang];
    const head = (body) => [
      PIPE_OPEN,
      '  <section id="pipeline">',
      `    <h2>${t.pipeHeading}</h2>`,
      ...body,
      '  </section>',
      PIPE_CLOSE,
    ].join('\n');

    if (!entry) return head([`    <p class="sub">${t.pipeUnread('no cached copy')}</p>`]);

    const rows = entry.rows ?? [];
    const now = Date.now();
    const shown = rows
      .map((row) => {
        const at = Date.parse(row.deadline ?? '');
        const days = Number.isNaN(at) ? null : Math.round((at - now) / DAY_MS);
        return { row, days };
      })
      .filter(({ row, days }) => (row.status && row.status !== 'Not started') || (days !== null && days >= -7 && days <= 60))
      .sort((a, b) => (a.days ?? 9e9) - (b.days ?? 9e9));

    const stamp = entry.at.replace('T', ' ').slice(0, 16) + ' UTC';
    const sub = [`    <p class="sub">${t.pipeSub(rows.length, stamp)}</p>`];
    if (entry.staleBecause) sub.push(`    <p class="sub">${t.pipeStale(entry.staleBecause)}</p>`);
    if (!shown.length) return head([...sub, `    <p class="sub">${t.pipeNone}</p>`]);

    const missing = [];
    const cell = (value, row, field) => {
      if (lang !== 'en') return esc(value ?? '');
      const out = english(value, missing, row, field);
      return out === null ? `<em>${esc(t.pipeKoOnly)}</em>` : esc(out);
    };

    const body = shown.map(({ row, days }) => {
      const tone = days === null ? 'info' : days < 0 ? 'fail' : days <= 7 ? 'warn' : 'info';
      const when = days === null
        ? t.pipeNoDate
        : `${esc(row.deadline)} <span class="pill ${tone}">${esc(t.pipeDays(days))}</span>`;
      const status = t.pipeStatus[row.status] ?? row.status ?? '—';
      const standing = [
        cell(gist(row.note), row, 'note'),
        row.owner_note ? `<strong>${cell(row.owner_note, row, 'owner_note')}</strong>` : '',
      ].filter(Boolean).join(' ');
      return '      <tr>' +
        `<td>${cell(row.name, row, 'name')}</td>` +
        `<td>${cell(row.base, row, 'base')}</td>` +
        `<td>${when}</td>` +
        `<td>${esc(status)}</td>` +
        `<td>${standing}</td>` +
        '</tr>';
    });

    if (missing.length) console.error(`pipeline rows still holding Korean text, withheld from the English page: ${missing.join(', ')}`);

    return head([
      ...sub,
      '    <div class="table-wrap">',
      '      <table>',
      `        <thead><tr>${t.pipeCols.map((c) => `<th>${esc(c)}</th>`).join('')}</tr></thead>`,
      '        <tbody>',
      ...body,
      '        </tbody>',
      '      </table>',
      '    </div>',
    ]);
  };
}

const renderPipeline = await pipelineBlock();


let wrote = 0;
for (const [lang, entry] of Object.entries(config.readiness ?? {})) {
  const file = path.resolve(HERE, entry.file);
  const html = readFileSync(file, 'utf8');
  const start = html.indexOf(OPEN);
  const end = html.indexOf(CLOSE);
  if (start < 0 || end < 0) {
    console.error(`${entry.file}: no LIVE:STATUS block — leaving it alone`);
    continue;
  }
  let next = html.slice(0, start) + render(lang) + html.slice(end + CLOSE.length);

  // Each of these is its own block, so a document without one is left untouched.
  const calStart = next.indexOf(CAL_OPEN);
  const calEnd = next.indexOf(CAL_CLOSE);
  if (calStart >= 0 && calEnd >= 0) {
    next = next.slice(0, calStart) + renderCalendar(lang) + next.slice(calEnd + CAL_CLOSE.length);
  }

  const pipeStart = next.indexOf(PIPE_OPEN);
  const pipeEnd = next.indexOf(PIPE_CLOSE);
  if (pipeStart >= 0 && pipeEnd >= 0) {
    next = next.slice(0, pipeStart) + renderPipeline(lang) + next.slice(pipeEnd + PIPE_CLOSE.length);
  }

  writeFileSync(file, next);
  wrote += 1;
  console.log(`${entry.file} updated`);
}
if (!wrote) process.exit(1);
