// Checks that the conversation key OUTER minted is the key the adapter held.
//
// The key travels one way. Nothing in the reply carries it back, so a run that
// only watches the wire can conclude no more than "the adapter did not reject
// it" - which is equally true of an adapter that dropped the field on the
// floor. The adapter therefore traces each admission, and this reads that
// trace back against what the config said to mint.

const ADMITTED = /^P4_SESSION_KEY_ADMITTED request=(\S+) key=(\S+)$/;

/// Every key the config would mint for the artifact's requests, in the same
/// substitution the drive applies.
export function expectedKeys(config, requestIds) {
  if (!config.session_key_template) return new Map();
  return new Map(requestIds.map((requestId, index) => [
    requestId,
    config.session_key_template
      .replace("{{request_index}}", String(index + 1))
      .replace("{{request_id}}", requestId),
  ]));
}

/// What the adapter logged it admitted. A request admitted twice under the
/// same key appears once; under two different keys it would have been refused
/// at the adapter, so a disagreement here can only come from a lost field.
export function admittedKeys(agentLog) {
  const admitted = new Map();
  for (const line of agentLog.split(/\r?\n/)) {
    const match = ADMITTED.exec(line.trim());
    if (match) admitted.set(match[1], match[2]);
  }
  return admitted;
}

/// Compares the two, and demands they are the same set.
///
/// Presence of the expected keys is not enough. A record the run did not
/// ask for means the reader is looking at somebody else's records - a stale
/// file, a concurrent run - and a reader in that state cannot testify about
/// this run at all, including about the keys it did find. A 2026-09-01 smoke
/// run reported one key checked and forty-one observed and passed.
export function checkSessionKeys(config, requestIds, records) {
  const expected = expectedKeys(config, requestIds);
  if (expected.size === 0) {
    return { applicable: false, passed: true, checked: 0, observed: 0, mismatches: [], unexpected: [] };
  }
  const admitted = admittedKeys(records);
  const mismatches = [];
  for (const [requestId, key] of expected) {
    const seen = admitted.get(requestId);
    if (seen === undefined) mismatches.push({ request_id: requestId, expected: key, admitted: null });
    else if (seen !== key) mismatches.push({ request_id: requestId, expected: key, admitted: seen });
  }
  const unexpected = [...admitted.keys()].filter((requestId) => !expected.has(requestId));
  return {
    applicable: true,
    passed: mismatches.length === 0 && unexpected.length === 0 && admitted.size === expected.size,
    checked: expected.size,
    observed: admitted.size,
    mismatches: mismatches.slice(0, 5),
    unexpected: unexpected.slice(0, 5),
  };
}