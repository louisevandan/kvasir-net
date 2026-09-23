/**
 * What time it is for each of us, and when an event lands where each of us is.
 *
 * The team is spread across Seoul, Abu Dhabi and Michigan — thirteen hours end
 * to end — and the cost of that shows up as people missing things by a day
 * rather than by an hour. Somebody reads "Monday 23:59" and does not ask whose
 * Monday.
 *
 * None of this is asked of the model. A language model does not know what time
 * it is, and it converts timezones the way it does arithmetic: fluently and
 * sometimes wrong. Every clock and every conversion here is computed and handed
 * over as a fact, so the model's only job is to choose which of them answers the
 * question.
 *
 * Zones are IANA names, never fixed offsets. Michigan is UTC-4 today and UTC-5
 * in November, and a hardcoded offset would be quietly wrong for half the year —
 * which is the exact failure this module exists to prevent.
 */

/** Weekday, date and time as they read on the wall in `zone`. */
function wall(at, zone) {
  return new Intl.DateTimeFormat('en-GB', {
    timeZone: zone, weekday: 'short', day: 'numeric', month: 'short',
    hour: '2-digit', minute: '2-digit', hour12: false,
  }).format(at);
}

/** The zone's current offset, e.g. "UTC+9" — computed, because it moves. */
function offset(at, zone) {
  const part = new Intl.DateTimeFormat('en-GB', { timeZone: zone, timeZoneName: 'shortOffset' })
    .formatToParts(at).find((p) => p.type === 'timeZoneName');
  return part?.value ?? zone;
}

/** Just the date, for things that are a date and not a moment. */
function day(at, zone) {
  return new Intl.DateTimeFormat('en-GB', {
    timeZone: zone, weekday: 'short', day: 'numeric', month: 'short',
  }).format(at);
}

/** Every person's wall clock, right now. */
export function clocksNow(people, at = new Date()) {
  const lines = [`Right now it is ${at.toISOString().slice(0, 16).replace('T', ' ')} UTC.`];
  for (const person of people) {
    lines.push(`  ${person.who} — ${person.where}: ${wall(at, person.zone)} (${offset(at, person.zone)})`);
  }
  return lines.join('\n');
}

/** One moment, on everyone's wall, on one line. */
export function everywhere(people, at) {
  return people.map((p) => `${p.where} ${wall(at, p.zone)}`).join(' · ');
}

/**
 * How far off is a moment, in plain words.
 *
 * "In 2 days" is what someone actually wants from a deadline; the wall clocks
 * answer "when", this answers "how long have I got".
 */
export function away(at, from = Date.now()) {
  const ms = at - from;
  const hours = Math.round(Math.abs(ms) / 3_600_000);
  const text = hours < 48 ? `${hours}h` : `${Math.round(hours / 24)}d`;
  return ms < 0 ? `${text} ago` : `in ${text}`;
}

/**
 * Deadlines with a real moment behind them, rendered for everyone.
 *
 * A bare date is not converted. "2026-09-20" is a date, and turning it into
 * somebody's 03:00 would be inventing a fact — so those are listed as dates and
 * said to be dates.
 */
export function deadlineLines(people, deadlines = [], bareDates = []) {
  const lines = [];
  for (const d of deadlines) {
    const at = Date.parse(d.at);
    if (Number.isNaN(at)) continue;
    lines.push(`  ${d.label} — closes ${d.as_stated ?? ''} (${away(at)})`.replace('  —  (', '  — ('));
    lines.push(`      ${everywhere(people, at)}`);
  }
  for (const b of bareDates) {
    lines.push(`  ${b.label} — ${b.date}. A date, not a time: no closing hour was published, so it is the same day everywhere.`);
  }
  return lines;
}

/**
 * Calendar entries, each on every wall, each labelled with how far off it is.
 *
 * The relative marker is what answers "what's next?". Without it the reader has
 * to compare wall clocks against wall clocks to find the first one that has not
 * happened yet — which is the arithmetic this module exists to remove.
 */
export function eventLines(people, events = [], from = Date.now(), english = new Map()) {
  return events.map((e) => {
    // The group is bilingual, so an entry written in Korean is given in English
    // with the original alongside: the English is what everyone can read, the
    // original is what someone will actually find in their own calendar.
    const en = english.get(e.summary);
    const title = en && en !== e.summary ? `${en} (${e.summary})` : (en ?? e.summary);
    // Written as a sentence, not as a bracketed tag. Anything in the facts may
    // be quoted back into a reply verbatim, so internal notation ends up in
    // front of the team — "[in 3h]" did, once.
    const when = e.allDay
      ? `${away(e.at, from)}, all day on ${day(e.at, people[0].zone)} (a whole day, the same date everywhere)`
      : `${away(e.at, from)}: ${everywhere(people, e.at)}`;
    // A note only earns its place if it says something the title did not.
    const raw = e.description ? (english.get(e.description) ?? e.description) : '';
    const note = raw && raw.length > 8 ? ` — ${raw.slice(0, 140)}` : '';
    return `  ${title} — ${when}${note}`;
  });
}
