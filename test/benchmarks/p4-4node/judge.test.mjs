import assert from "node:assert/strict";
import test from "node:test";
import { judgeAnswer, judgeArtifact } from "./judge.mjs";

// A real excerpt from the 2026-08-30 four-node run, kept as the positive
// fixture so a future change to the judge cannot silently start rejecting
// output that was accepted as meaningful.
const GOOD = `타입스크립트(TypeScript)는 자바스크립트에 정적 타입 기능을 추가한 언어입니다.
자바스크립트는 동적 타입 언어이기 때문에 개발 과정에서 예상치 못한 타입 오류가
발생하기 쉽습니다. 타입스크립트는 이러한 문제를 컴파일 시점에 잡아내어 버그를 줄이고
코드의 안정성을 높여줍니다. 변수나 함수가 어떤 종류의 데이터를 다룰지 명확하게
선언되므로 코드를 읽는 사람이 이해하기 쉬워지고, 자동 완성이 강력해져 개발 속도가
빨라집니다. 인터페이스와 제네릭을 사용하면 객체의 구조와 재사용 가능한 함수를
타입 수준에서 표현할 수 있으며, 타입 추론 덕분에 모든 곳에 타입을 적지 않아도
됩니다. 프로젝트 규모가 커질수록 이러한 정적 검사의 이점이 커집니다.`;

test("accepts the recorded four-node answer", () => {
  const verdict = judgeAnswer(GOOD, "length");
  assert.equal(verdict.meaningful, true, JSON.stringify(verdict.failures));
});

test("rejects an empty answer", () => {
  const verdict = judgeAnswer("", "length");
  assert.equal(verdict.meaningful, false);
  assert.match(verdict.failures.join(" "), /too short/u);
});

test("rejects English output for a Korean prompt", () => {
  const english = "TypeScript is a typed superset of JavaScript that compiles to plain JavaScript. ".repeat(6);
  const verdict = judgeAnswer(english, "length");
  assert.equal(verdict.meaningful, false);
  assert.match(verdict.failures.join(" "), /not Korean enough/u);
});

test("rejects degenerate repetition", () => {
  const looped = "타입스크립트는 타입을 지정합니다. ".repeat(40);
  const verdict = judgeAnswer(looped, "length");
  assert.equal(verdict.meaningful, false);
  assert.match(verdict.failures.join(" "), /degenerate repetition/u);
});

test("rejects on-length Korean prose that is off topic", () => {
  const offTopic = "오늘 날씨가 매우 맑아서 공원에 산책을 다녀왔습니다. 나무들이 푸르고 바람이 시원했으며 사람들이 즐겁게 웃고 있었습니다. 강가에서는 아이들이 뛰어놀았고 어른들은 벤치에 앉아 조용히 책을 읽었습니다. 저녁 무렵에는 노을이 하늘을 붉게 물들였고 거리에는 가로등이 하나둘 켜지기 시작했습니다. 집으로 돌아오는 길에 근처 시장에 들러 저녁거리를 조금 샀습니다.";
  const verdict = judgeAnswer(offTopic, "length");
  assert.equal(verdict.meaningful, false);
  assert.match(verdict.failures.join(" "), /off topic/u);
});

test("rejects a split multi-byte token", () => {
  const verdict = judgeAnswer(`${GOOD}�`, "length");
  assert.equal(verdict.meaningful, false);
  assert.match(verdict.failures.join(" "), /U\+FFFD/u);
});

test("rejects a request with no terminal outcome", () => {
  const verdict = judgeAnswer(GOOD, null);
  assert.equal(verdict.meaningful, false);
  assert.match(verdict.failures.join(" "), /stop reason/u);
});

test("an artifact passes only when every request is meaningful", () => {
  const request = (response) => ({
    request_id: "r",
    response,
    outcomes: [{ stop: "length" }],
  });
  assert.equal(judgeArtifact({ requests: [request(GOOD), request(GOOD)] }).passed, true);
  assert.equal(judgeArtifact({ requests: [request(GOOD), request("")] }).passed, false);
  assert.equal(judgeArtifact({ requests: [] }).passed, false);
});
