#!/usr/bin/env python3
"""Fail closed on a standalone model's exact I0 prompt/oracle capability."""

from __future__ import annotations

import argparse
import copy
import json
import struct
import tempfile
from pathlib import Path


CASES = ("case-00", "case-04", "case-06")


def evaluate(corpus: Path, responses: Path) -> dict:
    rows = []
    for case in CASES:
        failures = []
        try:
            prompt = (corpus / f"{case}.prompt.txt").read_text(encoding="utf-8")
            oracle = json.loads((corpus / f"{case}.oracle.json").read_text(encoding="utf-8"))
            token_bytes = (corpus / f"{case}.tokens.bin").read_bytes()
            if len(token_bytes) % struct.calcsize("<i") or not token_bytes:
                failures.append("invalid_token_file")
            response = json.loads((responses / f"{case}.response.json").read_text(encoding="utf-8"))
        except (OSError, ValueError, TypeError) as error:
            rows.append({"case": case, "passed": False, "failures": [f"evidence_missing_or_invalid:{type(error).__name__}"]})
            continue
        if not isinstance(response, dict):
            rows.append({"case": case, "passed": False, "failures": ["invalid_response_shape"]})
            continue
        if response.get("prompt") != prompt:
            failures.append("prompt_mismatch")
        if type(response.get("tokens_evaluated")) is not int or response["tokens_evaluated"] != len(token_bytes) // 4:
            failures.append("token_count_mismatch")
        if response.get("stop") is not True or response.get("stop_type") != "eos" or response.get("truncated") is not False:
            failures.append("non_eos")
        predicted = response.get("tokens_predicted")
        if type(predicted) is not int or not 0 < predicted <= 2048:
            failures.append("output_token_count")
        try:
            actual = json.loads(response["content"])
        except (KeyError, TypeError, ValueError):
            failures.append("invalid_json")
        else:
            if actual != oracle:
                failures.append("oracle_mismatch")
        rows.append({"case": case, "passed": not failures, "failures": failures})
    return {"passed": all(row["passed"] for row in rows), "cases": rows}


def self_test() -> None:
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        corpus, responses = root / "corpus", root / "responses"
        corpus.mkdir()
        responses.mkdir()
        baseline = {"prompt": "example", "tokens_evaluated": 2, "stop": True,
                    "stop_type": "eos", "truncated": False, "tokens_predicted": 1,
                    "content": '{"answer": 4}'}
        for case in CASES:
            (corpus / f"{case}.prompt.txt").write_text("example", encoding="utf-8")
            (corpus / f"{case}.oracle.json").write_text('{"answer": 4}', encoding="utf-8")
            (corpus / f"{case}.tokens.bin").write_bytes(struct.pack("<ii", 1, 2))
            (responses / f"{case}.response.json").write_text(json.dumps(baseline), encoding="utf-8")
        assert evaluate(corpus, responses)["passed"]
        changes = ({"prompt": "other"}, {"tokens_evaluated": 1}, {"stop_type": "length"},
                   {"content": "```json\n{}\n```"}, {"content": '{"answer": 5}'})
        for change in changes:
            altered = copy.deepcopy(baseline)
            altered.update(change)
            file = responses / "case-04.response.json"
            file.write_text(json.dumps(altered), encoding="utf-8")
            assert not evaluate(corpus, responses)["passed"], change
        file.write_text("[]", encoding="utf-8")
        assert evaluate(corpus, responses)["cases"][1]["failures"] == ["invalid_response_shape"]
        file.unlink()
        assert evaluate(corpus, responses)["cases"][1]["failures"] == ["evidence_missing_or_invalid:FileNotFoundError"]
        print(json.dumps({"passed": True, "mutations": len(changes) + 2}, separators=(",", ":")))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--corpus", type=Path)
    parser.add_argument("--responses", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return
    if args.corpus is None or args.responses is None:
        parser.error("--corpus and --responses are required")
    result = evaluate(args.corpus, args.responses)
    print(json.dumps(result, separators=(",", ":")))
    if not result["passed"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
