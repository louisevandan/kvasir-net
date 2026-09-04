// Decides whether a generated answer to the fixed acceptance prompt
// ("타입스크립트에 대해 한국어로 설명하라") is meaningful.
//
// The bar exists because transport success proves nothing about correctness:
// a run can deliver 40/40 responses that are empty, looped, or in the wrong
// language. Every check below is a property that a broken pipeline actually
// produces - wrong-language output when the cut-set is mis-sliced, degenerate
// repetition when KV is stale, and replacement characters when a multi-byte
// token is split across a boundary.

const HANGUL = /[가-힣]/gu;
const REPLACEMENT = /�/u;

// Domain vocabulary. Distinct hits are counted, so a loop repeating one word
// cannot satisfy this.
export const DOMAIN_TERMS = [
  "타입", "자바스크립트", "javascript", "typescript", "컴파일",
  "인터페이스", "제네릭", "정적", "변수", "함수", "객체", "코드",
  "오류", "에러", "선언", "추론",
];

export const DEFAULT_CRITERIA = {
  minimumChars: 200,
  minimumHangulRatio: 0.3,
  minimumDistinctTerms: 4,
  maximumRepeatRatio: 0.35,
  // A stop string is a finish, not a fault. The drive-side acceptance was
  // widened for scenarios that supply them; this list is the judge's own and
  // was missed, so a run that ended exactly where OUTER asked it to still
  // failed here.
  allowedStops: ["eos", "length", "stop"],
};

// Fraction of 12-character shingles that are duplicates. Degenerate loops
// score near 1; ordinary prose stays low even when a term recurs.
export function repeatRatio(text, window = 12) {
  const compact = text.replace(/\s+/gu, "");
  if (compact.length <= window) return 0;
  const seen = new Set();
  let duplicates = 0;
  let total = 0;
  for (let index = 0; index + window <= compact.length; index += 1) {
    const shingle = compact.slice(index, index + window);
    total += 1;
    if (seen.has(shingle)) duplicates += 1;
    else seen.add(shingle);
  }
  return total === 0 ? 0 : duplicates / total;
}

export function hangulRatio(text) {
  const letters = text.replace(/[\s\p{P}\p{S}\d]/gu, "");
  if (letters.length === 0) return 0;
  return (text.match(HANGUL) ?? []).length / letters.length;
}

export function distinctTerms(text) {
  const lowered = text.toLowerCase();
  return DOMAIN_TERMS.filter((term) => lowered.includes(term));
}

/// Judges one answer. `stop` is the terminal stop reason, or null when the
/// request never produced one.
export function judgeAnswer(text, stop, criteria = DEFAULT_CRITERIA) {
  const failures = [];
  const chars = [...(text ?? "")].length;
  const ratio = hangulRatio(text ?? "");
  const terms = distinctTerms(text ?? "");
  const repeat = repeatRatio(text ?? "");

  if (chars < criteria.minimumChars) {
    failures.push(`too short: ${chars} chars < ${criteria.minimumChars}`);
  }
  if (ratio < criteria.minimumHangulRatio) {
    failures.push(`not Korean enough: hangul ratio ${ratio.toFixed(2)} < ${criteria.minimumHangulRatio}`);
  }
  if (terms.length < criteria.minimumDistinctTerms) {
    failures.push(`off topic: ${terms.length} distinct domain terms < ${criteria.minimumDistinctTerms}`);
  }
  if (repeat > criteria.maximumRepeatRatio) {
    failures.push(`degenerate repetition: ${repeat.toFixed(2)} > ${criteria.maximumRepeatRatio}`);
  }
  if (REPLACEMENT.test(text ?? "")) {
    failures.push("contains U+FFFD - a multi-byte token was split");
  }
  if (!criteria.allowedStops.includes(stop ?? "")) {
    failures.push(`stop reason ${JSON.stringify(stop)} is not allowed`);
  }
  return {
    meaningful: failures.length === 0,
    chars,
    hangulRatio: Number(ratio.toFixed(3)),
    distinctTerms: terms.length,
    repeatRatio: Number(repeat.toFixed(3)),
    stop,
    failures,
  };
}

/// Judges every request in a drive artifact.
export function judgeArtifact(artifact, criteria = DEFAULT_CRITERIA) {
  const results = (artifact.requests ?? []).map((request) => ({
    request_id: request.request_id,
    ...judgeAnswer(request.response, request.outcomes?.at(-1)?.stop ?? null, criteria),
  }));
  const meaningful = results.filter((r) => r.meaningful).length;
  return {
    passed: results.length > 0 && meaningful === results.length,
    total: results.length,
    meaningful,
    results,
  };
}
