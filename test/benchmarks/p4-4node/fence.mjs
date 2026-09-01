// Cuts one run's records out of a file the agent appends to across runs.
//
// Offsets were the first attempt and they are not fail-closed: reading "from
// byte N" still returns the whole file when the run wrote nothing, which is
// exactly the failure being investigated. A scenario also reuses its request
// ids, so a previous run's records satisfy this run's checks - the harness
// then certifies an adapter that emitted nothing at all.
//
// A fence removes the arithmetic. The harness appends a BEGIN carrying this
// run's id before the drive starts and an END after it, and only what lies
// between them counts. Absence of either is a failure, not an empty result,
// and a second BEGIN arriving first says another run is writing to the same
// file - which makes both runs' evidence worthless rather than merely thin.

const BEGIN = /^P4_RUN_FENCE_BEGIN run_id=(\S+)$/;
const END = /^P4_RUN_FENCE_END run_id=(\S+)$/;

export function beginFence(runId) {
  return `P4_RUN_FENCE_BEGIN run_id=${runId}`;
}

export function endFence(runId) {
  return `P4_RUN_FENCE_END run_id=${runId}`;
}

/// The records this run wrote, or an explanation of why they cannot be told
/// apart from anyone else's.
///
/// Returns `{ ok, records, reason }`. `records` is only meaningful when `ok`.
export function fencedRecords(text, runId) {
  const lines = text.split(/\r?\n/);
  let started = -1;
  for (let index = 0; index < lines.length; index += 1) {
    const begin = BEGIN.exec(lines[index].trim());
    if (begin && begin[1] === runId) {
      if (started >= 0) {
        return { ok: false, records: [], reason: `run ${runId} opened two fences` };
      }
      started = index;
      continue;
    }
    if (started < 0) continue;
    if (begin) {
      return {
        ok: false,
        records: [],
        reason: `another run (${begin[1]}) wrote to this record file during ${runId}`,
      };
    }
    const end = END.exec(lines[index].trim());
    if (end) {
      if (end[1] !== runId) {
        return { ok: false, records: [], reason: `fence closed by ${end[1]}, not ${runId}` };
      }
      return { ok: true, records: lines.slice(started + 1, index), reason: "" };
    }
  }
  if (started < 0) return { ok: false, records: [], reason: `no fence for run ${runId}` };
  return { ok: false, records: [], reason: `fence for run ${runId} was never closed` };
}
