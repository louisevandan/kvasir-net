/**
 * The week ahead: meetings from a calendar feed, plan milestones from the repo.
 *
 * The monitoring job runs unattended from launchd, so it cannot use an
 * interactive Google connector. A calendar's secret iCal address is a plain
 * HTTPS URL that returns the same events, which is the only shape that works
 * here — put it in config.json under `calendar.icsUrl`.
 *
 * The parser is deliberately small: enough VEVENT and enough RRULE to place a
 * weekly stand-up and a one-off meeting on the right day. Anything it cannot
 * expand is left out rather than guessed at, and the renderer says when the
 * feed is missing instead of drawing an empty week as if it were a free one.
 */

const DAY_MS = 86_400_000;

/* ---- iCalendar ----------------------------------------------------------- */

/** RFC 5545 folds long lines with a leading space; unfold before parsing. */
const unfold = (text) => text.replace(/\r\n[ \t]/g, '').replace(/\n[ \t]/g, '');

function parseDate(value, params = '') {
  if (!value) return null;
  const dateOnly = /^\d{8}$/.test(value);
  if (dateOnly) {
    const y = +value.slice(0, 4), m = +value.slice(4, 6), d = +value.slice(6, 8);
    return { at: Date.UTC(y, m - 1, d), allDay: true };
  }
  const match = value.match(/^(\d{4})(\d{2})(\d{2})T(\d{2})(\d{2})(\d{2})(Z)?$/);
  if (!match) return null;
  const [, y, mo, d, h, mi, s, zulu] = match;
  const at = Date.UTC(+y, +mo - 1, +d, +h, +mi, +s);
  // A floating or TZID time is treated as the calendar's own zone; the renderer
  // is given that zone and formats accordingly.
  return { at, allDay: false, floating: !zulu, tzid: (params.match(/TZID=([^;:]+)/) ?? [])[1] ?? null };
}

/** A calendar note with the invitation boilerplate taken out. */
function tidyNote(value) {
  return value
    .replace(/\\,/g, ',')
    .replace(/\\n/g, ' ')
    .replace(/<[^>]+>/g, ' ')                                  // some clients send HTML
    .replace(/-::~:~[^\n]*/g, ' ')                             // Google's own separator
    .replace(/Join with Google Meet:?\s*\S+/gi, ' ')
    .replace(/Learn more about Meet at:?\s*\S+/gi, ' ')
    .replace(/Or dial:?[^.]*/gi, ' ')
    .replace(/More phone numbers:?\s*\S+/gi, ' ')
    .replace(/\s+/g, ' ')
    .trim()
    .slice(0, 240);
}

export function parseIcs(text) {
  const events = [];
  let current = null;
  for (const raw of unfold(text).split(/\r?\n/)) {
    if (raw === 'BEGIN:VEVENT') { current = {}; continue; }
    if (raw === 'END:VEVENT') { if (current?.start && current.summary) events.push(current); current = null; continue; }
    if (!current) continue;
    const colon = raw.indexOf(':');
    if (colon < 0) continue;
    const left = raw.slice(0, colon);
    const value = raw.slice(colon + 1);
    const name = left.split(';')[0].toUpperCase();
    const params = left.slice(name.length);
    if (name === 'SUMMARY') current.summary = value.replace(/\\,/g, ',').replace(/\\n/g, ' ').trim();
    else if (name === 'LOCATION') current.location = value.replace(/\\,/g, ',').trim();
    // Descriptions carry the detail a title leaves out — an agenda, who is
    // attending, a link that matters. They also carry a great deal that is not
    // detail: Google stitches a video-call advert onto every invitation, and
    // repeating that back to the team is noise wearing the shape of an answer.
    else if (name === 'DESCRIPTION') current.description = tidyNote(value);
    else if (name === 'DTSTART') current.start = parseDate(value, params);
    else if (name === 'DTEND') current.end = parseDate(value, params);
    else if (name === 'RRULE') current.rrule = value;
    else if (name === 'STATUS') current.status = value;
    else if (name === 'UID') current.uid = value;
  }
  return events.filter((event) => event.status !== 'CANCELLED');
}

/** Expand an event into the occurrences that fall inside [from, to). */
function occurrences(event, from, to) {
  const out = [];
  const length = event.end && event.start ? Math.max(0, event.end.at - event.start.at) : 0;
  const push = (at) => { if (at >= from && at < to) out.push({ ...event, at, length }); };

  if (!event.rrule) { push(event.start.at); return out; }

  const rule = Object.fromEntries(event.rrule.split(';').map((part) => part.split('=')));
  const freq = rule.FREQ;
  const interval = Number(rule.INTERVAL ?? 1) || 1;
  const until = rule.UNTIL ? (parseDate(rule.UNTIL)?.at ?? Infinity) : Infinity;
  const count = rule.COUNT ? Number(rule.COUNT) : Infinity;

  // Walk forward from the first occurrence. The window is one week, so a few
  // thousand steps is the worst case even for a daily rule from years back.
  const step = freq === 'DAILY' ? DAY_MS * interval : freq === 'WEEKLY' ? 7 * DAY_MS * interval : null;
  if (step) {
    let at = event.start.at;
    let seen = 0;
    while (at < to && at <= until && seen < count && seen < 4000) {
      if (at >= from) push(at);
      at += step;
      seen += 1;
    }
    return out;
  }
  if (freq === 'MONTHLY' || freq === 'YEARLY') {
    const first = new Date(event.start.at);
    for (let i = 0; i < 400; i += 1) {
      const at = freq === 'MONTHLY'
        ? Date.UTC(first.getUTCFullYear(), first.getUTCMonth() + i * interval, first.getUTCDate(), first.getUTCHours(), first.getUTCMinutes())
        : Date.UTC(first.getUTCFullYear() + i * interval, first.getUTCMonth(), first.getUTCDate(), first.getUTCHours(), first.getUTCMinutes());
      if (at >= to || at > until) break;
      if (at >= from) push(at);
    }
  }
  return out;
}

/* ---- the week ------------------------------------------------------------ */

/** Monday 00:00 of the week containing `now`, in the given zone offset (minutes). */
export function weekStart(now, offsetMinutes) {
  const local = new Date(now.getTime() + offsetMinutes * 60_000);
  const weekday = (local.getUTCDay() + 6) % 7;        // Monday = 0
  const midnight = Date.UTC(local.getUTCFullYear(), local.getUTCMonth(), local.getUTCDate()) - weekday * DAY_MS;
  return midnight - offsetMinutes * 60_000;
}

export async function fetchEvents(icsUrl, from, to, { timeoutMs = 15_000 } = {}) {
  const response = await fetch(icsUrl, { signal: AbortSignal.timeout(timeoutMs) });
  if (!response.ok) throw new Error(`calendar feed returned ${response.status}`);
  const events = parseIcs(await response.text());
  return events.flatMap((event) => occurrences(event, from, to)).sort((a, b) => a.at - b.at);
}

/**
 * Render the week as a row of seven days.
 *
 * Meetings and plan milestones share the grid on purpose: the residency plan is
 * only real if it sits next to the meetings that will eat the same hours.
 */
export function renderWeek({ start, offsetMinutes, events, milestones, labels, today }) {
  const fmtDay = (at) => new Date(at + offsetMinutes * 60_000).toISOString().slice(0, 10);
  const dayNum = (at) => new Date(at + offsetMinutes * 60_000).getUTCDate();
  const time = (at) => new Date(at + offsetMinutes * 60_000).toISOString().slice(11, 16);
  const esc = (value) => String(value ?? '').replace(/[&<>]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;' }[c]));

  const cells = [];
  for (let i = 0; i < 7; i += 1) {
    const dayStart = start + i * DAY_MS;
    const iso = fmtDay(dayStart);
    const dayEvents = events.filter((event) => fmtDay(event.at) === iso);
    const dayPlan = (milestones ?? []).filter((milestone) => milestone.date === iso);
    const isToday = iso === today;
    cells.push(
      `<td class="${isToday ? 'cal-today' : ''}">` +
      `<div class="cal-date">${dayNum(dayStart)}</div>` +
      dayPlan.map((milestone) =>
        `<div class="cal-plan">${esc(milestone.label)}</div>`).join('') +
      dayEvents.map((event) =>
        `<div class="cal-event"><span class="cal-time">${event.allDay ? labels.allDay : time(event.at)}</span> ${esc(event.summary)}</div>`).join('') +
      '</td>',
    );
  }

  return (
    '<div class="table-wrap"><table class="cal">' +
    `<thead><tr>${labels.days.map((day) => `<th>${day}</th>`).join('')}</tr></thead>` +
    `<tbody><tr>${cells.join('')}</tr></tbody>` +
    '</table></div>'
  );
}
