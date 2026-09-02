// Decides whether a run's slice of the record file is usable as evidence.
//
// The bytes themselves are cut on the far side, by reading exactly
// `[begin, end)` - the two lengths the harness took around the run. Reading
// to the end of the file instead would fold in whatever the agent wrote
// after the closing length was taken, which would make `end` a name for a
// boundary rather than a boundary.
//
// What is left for this file is the part a byte range cannot express: a
// range that begins or ends inside a record. Half a record is not evidence,
// and accepting it would hide exactly the interleaving the single-writer
// record file exists to prevent.

/// Checks the slice and splits it into records.
///
/// Returns `{ ok, records, reason }`. `records` is only meaningful when
/// `ok`.
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
  if (text !== "" && !text.endsWith("\n")) {
    // The closing length landed inside a record the agent was still writing.
    return {
      ok: false,
      records: [],
      reason: "run boundary fell inside a record; the agent had not finished writing",
    };
  }
  const lines = text.split(/\r?\n/).filter((line) => line.trim() !== "");
  return { ok: true, records: lines, reason: "" };
}
