#!/usr/bin/env python3
"""Judge a complete Release A H1 quality artifact against its sealed oracles."""
from __future__ import annotations

import argparse
import hashlib
import json
import math
from collections import defaultdict
from pathlib import Path


CLASSES = ("short", "medium", "long")


def prompt_digest(value: str) -> str:
    return hashlib.sha256(value.encode()).hexdigest()


def nearest_rank(values: list[int], percentile: float) -> int | None:
    if not values:
        return None
    ordered = sorted(values)
    return ordered[math.ceil(len(ordered) * percentile) - 1]


def nonnegative_integer(value: object) -> bool:
    return isinstance(value, int) and not isinstance(value, bool) and value >= 0


def timing_evidence(request: dict, timeout_ms: int | None) -> tuple[list[str], dict | None]:
    failures: list[str] = []
    arrival = request.get("arrival_ms")
    first = request.get("first_output_ms")
    completed = request.get("completed_ms")
    receipts = request.get("output_received_ms")
    outcomes = request.get("outcomes") or []
    if not all(nonnegative_integer(value) for value in (arrival, first, completed)):
        return ["timing_missing"], None
    if not isinstance(receipts, list) or not receipts or not all(nonnegative_integer(value) for value in receipts):
        return ["output_timing_missing"], None
    if len(receipts) != len(outcomes):
        failures.append("output_timing_count")
    if not (arrival <= first <= completed):
        failures.append("timing_order")
    if receipts != sorted(receipts) or receipts[0] != first or receipts[-1] != completed:
        failures.append("output_timing_order")
    e2e = completed - arrival
    if not nonnegative_integer(timeout_ms):
        failures.append("deadline_contract_missing")
    elif e2e > timeout_ms:
        failures.append("deadline_exceeded")
    timing = {
        "e2e_ms": e2e,
        "ttft_ms": first - arrival,
        "itl_ms": [right - left for left, right in zip(receipts, receipts[1:])],
    }
    return failures, timing


def evaluate(artifact: dict, seal: dict) -> dict:
    cases = seal.get("cases") or []
    expected = {row.get("prompt_sha256"): row for row in cases}
    slo = seal.get("slo") or {}
    ttft_limits = slo.get("ttft_ms_by_class") or {}
    itl_limit = slo.get("itl_p95_ms")
    rows, seen = [], set()
    ttft_by_class: dict[str, list[int]] = defaultdict(list)
    itl_by_class: dict[str, list[int]] = defaultdict(list)
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
        if not isinstance(deadline, int) or isinstance(deadline, bool) or deadline <= 0:
            failures.append("deadline_missing")
        timing_failures, timing = timing_evidence(
            request, case.get("request_timeout_ms") if case is not None else None
        )
        failures.extend(timing_failures)
        request_class = case.get("class") if case is not None else None
        if timing is not None and request_class in CLASSES:
            ttft_by_class[request_class].append(timing["ttft_ms"])
            itl_by_class[request_class].extend(timing["itl_ms"])
        rows.append({"request_id": request.get("request_id"), "case_id": case and case["id"],
                     "class": request_class, "timing": timing,
                     "passed": not failures, "failures": failures})

    count, submissions = seal.get("requests"), artifact.get("submissions") or {}
    global_failures = []
    if seal.get("schema") != 3 or slo.get("percentile") != "nearest_rank":
        global_failures.append("slo_contract_missing")
    if set(ttft_limits) != set(CLASSES) or any(
        not isinstance(value, int) or isinstance(value, bool) or value <= 0
        for value in ttft_limits.values()
    ) or not isinstance(itl_limit, int) or isinstance(itl_limit, bool) or itl_limit <= 0:
        global_failures.append("slo_limits_invalid")
    class_metrics = {}
    for request_class in CLASSES:
        ttft = ttft_by_class[request_class]
        itl = itl_by_class[request_class]
        ttft_p95 = nearest_rank(ttft, 0.95)
        itl_p95 = nearest_rank(itl, 0.95)
        class_metrics[request_class] = {
            "requests": len(ttft), "ttft_samples": len(ttft), "ttft_p95_ms": ttft_p95,
            "ttft_limit_ms": ttft_limits.get(request_class), "itl_samples": len(itl),
            "itl_p95_ms": itl_p95, "itl_limit_ms": itl_limit,
        }
        if ttft_p95 is None:
            global_failures.append(f"ttft_samples_missing:{request_class}")
        elif isinstance(ttft_limits.get(request_class), int) and ttft_p95 > ttft_limits[request_class]:
            global_failures.append(f"ttft_p95_exceeded:{request_class}")
        if itl_p95 is None:
            global_failures.append(f"itl_samples_missing:{request_class}")
        elif isinstance(itl_limit, int) and itl_p95 > itl_limit:
            global_failures.append(f"itl_p95_exceeded:{request_class}")
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
    return {"schema": 3, "passed": passed, "requests": len(rows),
            "passed_requests": sum(row["passed"] for row in rows),
            "global_failures": global_failures, "class_metrics": class_metrics, "rows": rows}


def fixture() -> tuple[dict, dict]:
    prompts = {name: f"p-{name}" for name in CLASSES}
    seal = {
        "schema": 3, "requests": 3,
        "slo": {"percentile": "nearest_rank",
                "ttft_ms_by_class": {"short": 10, "medium": 20, "long": 30},
                "itl_p95_ms": 20},
        "cases": [{"id": f"c-{name}", "class": name,
                   "prompt_sha256": prompt_digest(prompt), "expected": {"class": name},
                   "request_timeout_ms": 100} for name, prompt in prompts.items()],
    }
    requests = []
    for index, (name, prompt) in enumerate(prompts.items()):
        arrival = index * 100
        first = arrival + (index + 1) * 10
        completed = first + 20
        requests.append({"request_id": f"r-{name}", "prompt": prompt,
                         "response": json.dumps({"class": name}),
                         "outcomes": [{"stop": None}, {"stop": "eos"}],
                         "submission": "delivered", "released": True,
                         "submission_authority": {"deadline": 1},
                         "arrival_ms": arrival, "first_output_ms": first,
                         "completed_ms": completed, "output_received_ms": [first, completed]})
    artifact = {"passed": True, "requests": requests, "request_count": 3, "completed_count": 3,
                "released_count": 3, "submissions": {"configured": 3, "delivered": 3, "uncertain": 0,
                "unsubmitted": 0, "incomplete": 0, "unreleased": 0}, "error": None,
                "evidence_missing": None, "cleanup_error": None}
    return artifact, seal


def self_test() -> None:
    artifact, seal = fixture()
    assert nearest_rank(list(range(1, 21)), 0.95) == 19
    assert evaluate(artifact, seal)["passed"]
    mutations = (
        lambda value: value[0]["requests"][0].update(response='{"class":"wrong"}'),
        lambda value: value[0].update(passed=False),
        lambda value: value[0]["requests"][0]["submission_authority"].update(deadline=None),
        lambda value: (value[0]["requests"][0].update(completed_ms=101),
                       value[0]["requests"][0].update(output_received_ms=[10, 101])),
        lambda value: (value[0]["requests"][0].update(first_output_ms=11, completed_ms=31),
                       value[0]["requests"][0].update(output_received_ms=[11, 31])),
        lambda value: (value[0]["requests"][0].update(completed_ms=31),
                       value[0]["requests"][0].update(output_received_ms=[10, 31])),
        lambda value: value[0]["requests"][0].update(output_received_ms=[10]),
        lambda value: value[1].pop("slo"),
    )
    for mutate in mutations:
        changed = json.loads(json.dumps([artifact, seal]))
        mutate(changed)
        assert not evaluate(*changed)["passed"]
    print(json.dumps({"passed": True, "tests": 10}, separators=(",", ":")))


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
