// Reads the agent's own account of events it could not deliver to the OUTER.
//
// A run whose relay discarded Output events did not do the work it reports:
// the tokens were generated, paid for in GPU time, and then dropped between
// the adapter and the caller. The drive notices only when the loss happens to
// break position contiguity, which is a side effect rather than a check - a
// discarded Telemetry event, or a discard in the tail of a request, leaves no
// trace in the artifact at all. So the verdict reads the agent's count.

const DISCARDED = /^P4_EVENT_OUTER_MISSING discarded=(\d+) target=(.*)$/;
// Runs made before the counter existed logged the discard without a total.
const UNCOUNTED = /^P4_EVENT_OUTER_MISSING target=/;

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
export function checkDelivery(agentLog) {
  const { totals, uncounted } = discards(agentLog);
  const counted = [...totals.values()].reduce((sum, value) => sum + value, 0);
  return {
    passed: counted === 0 && uncounted === 0,
    counted,
    uncounted,
    endpoints: [...totals.entries()].map(([target, count]) => ({ target, count })).slice(0, 5),
  };
}
