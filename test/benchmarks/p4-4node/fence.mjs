// Cuts one run's records out of a file the agent appends to across runs.
//
// Offsets were the first attempt and one offset is not enough: reading
// "from byte N" returns the whole file when the run wrote nothing, which is
// exactly the failure being investigated. Writing a fence into the file was
// the second attempt and it broke the invariant it depended on - the record
// file is declared to have one writer, and the harness became a second.
//
// Two offsets, taken by the harness into its own record, settle it. The
// run's records are the bytes between them: a run that wrote nothing has
// begin == end and yields nothing, and neither endpoint is a write into the
// agent's file.

/// The slice of a record file that belongs to one run.
///
/// Returns `{ ok, records, reason }`. `records` is only meaningful when
/// `ok`; a file that shrank below the opening offset was replaced under us,
/// which no offset can describe.
export function fencedRecords(text, begin, end) {
  if (!Number.isInteger(begin) || !Number.isInteger(end) || begin < 0) {
    return { ok: false, records: [], reason: "run boundaries were not recorded" };
  }
  if (end < begin) {
    return {
      ok: false,
      records: [],
      reason: `record file shrank from ${begin} to ${end} during the run`,
    };
  }
  const lines = text.split(/\r?\n/).filter((line) => line.trim() !== "");
  return { ok: true, records: lines, reason: "" };
}
