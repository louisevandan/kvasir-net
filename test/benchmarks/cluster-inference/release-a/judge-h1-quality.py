#!/usr/bin/env python3
"""Judge a complete Release A H1 quality artifact against its sealed oracles."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path


def prompt_digest(value: str) -> str:
    return hashlib.sha256(value.encode()).hexdigest()


def evaluate(artifact: dict, seal: dict) -> dict:
    expected = {row["prompt_sha256"]: row for row in seal["cases"]}
    rows, seen = [], set()
    for request in artifact.get("requests", []):
        digest = prompt_digest(request.get("prompt", ""))
        case, failures = expected.get(digest), []
        if case is None:
            failures.append("unknown_prompt")
        elif digest in seen:
            failures.append("duplicate_prompt")
        else:
            seen.add(digest)
        try:
            value = json.loads(request.get("response", ""))
        except Exception:
            value = None
            failures.append("invalid_json")
        if case is not None and value != case["expected"]:
            failures.append("oracle_mismatch")
        outcomes = request.get("outcomes") or []
        if not outcomes or outcomes[-1].get("stop") != "eos":
            failures.append("non_eos")
        if request.get("submission") != "delivered":
            failures.append("not_delivered")
        if request.get("released") is not True:
            failures.append("not_released")
        deadline = (request.get("submission_authority") or {}).get("deadline")
        if not isinstance(deadline, int) or deadline <= 0:
            failures.append("deadline_missing")
        rows.append({"request_id": request.get("request_id"), "case_id": case and case["id"],
                     "passed": not failures, "failures": failures})

    count, submissions = seal["requests"], artifact.get("submissions") or {}
    global_failures = []
    if artifact.get("passed") is not True:
        global_failures.append("artifact_acceptance")
    if len(rows) != count or len(seen) != count:
        global_failures.append("request_coverage")
    if any(artifact.get(key) != count for key in ("request_count", "completed_count", "released_count")):
        global_failures.append("count_mismatch")
    if submissions.get("configured") != count or submissions.get("delivered") != count:
        global_failures.append("submission_count")
    if any(submissions.get(key) not in (0, None) for key in ("uncertain", "unsubmitted", "incomplete", "unreleased")):
        global_failures.append("submission_remainder")
    if any(artifact.get(key) is not None for key in ("error", "evidence_missing", "cleanup_error")):
        global_failures.append("runtime_error")
    passed = not global_failures and all(row["passed"] for row in rows)
    return {"schema": 2, "passed": passed, "requests": len(rows),
            "passed_requests": sum(row["passed"] for row in rows),
            "global_failures": global_failures, "rows": rows}


def fixture() -> tuple[dict, dict]:
    prompt = "p"
    seal = {"requests": 1, "cases": [{"id": "c", "prompt_sha256": prompt_digest(prompt), "expected": {"x": 1}}]}
    request = {"request_id": "r", "prompt": prompt, "response": '{"x":1}',
               "outcomes": [{"stop": "eos"}], "submission": "delivered", "released": True,
               "submission_authority": {"deadline": 1}}
    artifact = {"passed": True, "requests": [request], "request_count": 1, "completed_count": 1,
                "released_count": 1, "submissions": {"configured": 1, "delivered": 1, "uncertain": 0,
                "unsubmitted": 0, "incomplete": 0, "unreleased": 0}, "error": None,
                "evidence_missing": None, "cleanup_error": None}
    return artifact, seal


def self_test() -> None:
    artifact, seal = fixture()
    assert evaluate(artifact, seal)["passed"]
    for mutate in (
        lambda value: value["requests"][0].update(response='{"x":2}'),
        lambda value: value.update(passed=False),
        lambda value: value["requests"][0]["submission_authority"].update(deadline=None),
    ):
        changed = json.loads(json.dumps(artifact))
        mutate(changed)
        assert not evaluate(changed, seal)["passed"]
    print(json.dumps({"passed": True, "tests": 4}, separators=(",", ":")))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact", type=Path)
    parser.add_argument("--seal", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return
    if not args.artifact or not args.seal or not args.output:
        parser.error("artifact, seal and output are required")
    result = evaluate(json.loads(args.artifact.read_text(encoding="utf-8")),
                      json.loads(args.seal.read_text(encoding="utf-8")))
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({key: result[key] for key in ("passed", "requests", "passed_requests", "global_failures")},
                     separators=(",", ":")))
    if not result["passed"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
