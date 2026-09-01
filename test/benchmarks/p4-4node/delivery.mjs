// Reads the agent's own account of events that did not reach the OUTER.
//
// Two different losses, because they happen in two different places. An
// event can be discarded before it reaches a connection's send queue - no
// route, or a route whose writer has gone - and an event sitting in that
// queue can be abandoned when the socket write fails. Counting only the
// first would let a run whose socket died read as having delivered
// everything: the queue accepted those events, and accepted is not
// delivered.
//
// Neither count proves delivery. What they prove is the absence of the two
// losses the agent can see; an event the agent wrote to a socket that the
// far side never read is beyond what this file can say.

const DISCARDED = /^P4_EVENT_OUTER_MISSING discarded=(\d+) target=(.*)$/;
// Runs made before the counter existed logged the discard without a total.
const UNCOUNTED = /^P4_EVENT_OUTER_MISSING target=/;

// Written when a socket write fails, naming how many queued events went
// down with it.
const ABANDONED = /^P4_EVENT_WRITE_FAILED abandoned=(\d+)/;
/// The highest running total the agent reported per endpoint, plus how many
/// discard lines carried no count at all.
export function discards(agentLog) {
  const totals = new Map();
  let uncounted = 0;
  for (const line of agentLog.split(/\r?\n/)) {
    const trimmed = line.trim();
    const match = DISCARDED.exec(trimmed);
    if (match) {
      const [, total, target] = match;
      totals.set(target, Math.max(totals.get(target) ?? 0, Number(total)));
    } else if (UNCOUNTED.test(trimmed)) {
      uncounted += 1;
    }
  }
  return { totals, uncounted };
}

/// A run passes delivery only when nothing was discarded. `counted` is the
/// total across endpoints; `uncounted` covers an agent too old to count, which
/// is still a failure - just one whose size is only a line tally.
/// A run passes delivery only when neither loss occurred. `counted` is what
/// the relay discarded, `abandoned` what a failed socket write took with it,
/// and `uncounted` covers an agent too old to count - still a failure, just
/// one whose size is only a line tally.
export function checkDelivery(records) {
  const { totals, uncounted } = discards(records);
  const counted = [...totals.values()].reduce((sum, value) => sum + value, 0);
  let abandoned = 0;
  let writeFailures = 0;
  for (const line of records.split(/\r?\n/)) {
    const match = ABANDONED.exec(line.trim());
    if (match) {
      writeFailures += 1;
      abandoned += Number(match[1]);
    }
  }
  return {
    passed: counted === 0 && uncounted === 0 && writeFailures === 0,
    counted,
    uncounted,
    write_failures: writeFailures,
    abandoned,
    endpoints: [...totals.entries()].map(([target, count]) => ({ target, count })).slice(0, 5),
  };
}