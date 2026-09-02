// Decides whether a run's slice of the record file is usable as evidence.
//
// The bytes are cut on the far side, by reading exactly `[begin, end)` - the
// two lengths the harness took around the run. Reading to the end of the
// file instead would fold in whatever the agent wrote after the closing
// length was taken, which would make `end` a name for a boundary rather
// than a boundary.
//
// Whether the slice ends on a record is also decided over there. The
// transport trims trailing whitespace, so a final newline cannot survive the
// journey to be checked here - a check on this side would fail every run.

/// Checks the slice and splits it into records.
///
/// Returns `{ ok, records, reason }`. `records` is only meaningful when
/// `ok`. A slice that begins or ends inside a record is refused rather than
/// trimmed: half a record is not evidence, and accepting it would hide the
/// interleaving the single-writer record file exists to prevent.
export function fencedRecords(slice, begin, end) {
  const { text, endsOnRecord } = slice;
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
  if (!endsOnRecord) {
    return {
      ok: false,
      records: [],
      reason: "run boundary fell inside a record; the agent had not finished writing",
    };
  }
  const records = text.split(/\r?\n/).filter((line) => line.trim() !== "");
  return { ok: true, records, reason: "" };
}
