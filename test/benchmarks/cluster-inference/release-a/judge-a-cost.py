#!/usr/bin/env python3
"""Judge the sealed one-request A-COST short feasibility artifact."""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
from pathlib import Path


DIRECTORY = Path(__file__).resolve().parent
H1_JUDGE = DIRECTORY / "judge-h1-quality.py"


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def load_h1_judge():
    module_spec = importlib.util.spec_from_file_location("release_a_h1_judge", H1_JUDGE)
    if module_spec is None or module_spec.loader is None:
        raise RuntimeError("cannot load the sealed H1 judge")
    module = importlib.util.module_from_spec(module_spec)
    module_spec.loader.exec_module(module)
    return module


def positive_integer(value: object) -> bool:
    return isinstance(value, int) and not isinstance(value, bool) and value > 0


def evaluate(artifact: dict, seal: dict) -> dict:
    h1 = load_h1_judge()
    global_failures: list[str] = []
    if seal.get("schema") != 1 or seal.get("mode") != "a_cost_short_feasibility":
        global_failures.append("seal_identity")
    if seal.get("requests") != 1 or seal.get("scope") != "short_feasibility_only":
        global_failures.append("scope_contract")
    if seal.get("h1_promotion_allowed") is not False:
        global_failures.append("h1_promotion_forbidden")
    if seal.get("judge_sha256") != digest(Path(__file__).read_bytes()):
        global_failures.append("judge_binding")
    if seal.get("h1_judge_sha256") != digest(H1_JUDGE.read_bytes()):
        global_failures.append("h1_judge_binding")

    slo = seal.get("slo") or {}
    if slo.get("percentile") != "nearest_rank" or not all(
        positive_integer(slo.get(key))
        for key in ("ttft_limit_ms", "itl_p95_limit_ms", "e2e_limit_ms")
    ):
        global_failures.append("slo_contract")
    case = seal.get("case") or {}
    if (
        case.get("id") != "case-00"
        or case.get("class") != "short"
        or case.get("request_timeout_ms") != slo.get("e2e_limit_ms")
    ):
        global_failures.append("case_contract")

    requests = artifact.get("requests") or []
    failures: list[str] = []
    timing = None
    if len(requests) != 1:
        global_failures.append("request_coverage")
    else:
        request = requests[0]
        if h1.prompt_digest(request.get("prompt", "")) != case.get("prompt_sha256"):
            failures.append("unknown_prompt")
        try:
            value = json.loads(request.get("response", ""))
        except Exception:
            value = None
            failures.append("invalid_json")
        if value != case.get("expected"):
            failures.append("oracle_mismatch")
        outcomes = request.get("outcomes") or []
        if not outcomes or outcomes[-1].get("stop") != "eos":
            failures.append("non_eos")
        if request.get("submission") != "delivered":
            failures.append("not_delivered")
        if request.get("released") is not True:
            failures.append("not_released")
        deadline = (request.get("submission_authority") or {}).get("deadline")
        if not positive_integer(deadline):
            failures.append("deadline_missing")
        timing_failures, timing = h1.timing_evidence(request, slo.get("e2e_limit_ms"))
        failures.extend(timing_failures)
        if timing is not None:
            if timing["ttft_ms"] > slo.get("ttft_limit_ms", -1):
                failures.append("ttft_exceeded")
            itl_p95 = h1.nearest_rank(timing["itl_ms"], 0.95)
            if itl_p95 is None:
                failures.append("itl_samples_missing")
            elif itl_p95 > slo.get("itl_p95_limit_ms", -1):
                failures.append("itl_p95_exceeded")

    count = 1
    submissions = artifact.get("submissions") or {}
    if artifact.get("passed") is not True:
        global_failures.append("artifact_acceptance")
    if any(artifact.get(key) != count for key in ("request_count", "completed_count", "released_count")):
        global_failures.append("count_mismatch")
    if submissions.get("configured") != count or submissions.get("delivered") != count:
        global_failures.append("submission_count")
    if any(submissions.get(key) not in (0, None) for key in ("uncertain", "unsubmitted", "incomplete", "unreleased")):
        global_failures.append("submission_remainder")
    if any(artifact.get(key) is not None for key in ("error", "evidence_missing", "cleanup_error")):
        global_failures.append("runtime_error")
    passed = not global_failures and not failures
    return {
        "schema": 1,
        "passed": passed,
        "scope": "short_feasibility_only",
        "h1_accepted": False,
        "global_failures": global_failures,
        "request": {"case_id": case.get("id"), "timing": timing,
                    "passed": not failures, "failures": failures},
    }


def fixture() -> tuple[dict, dict]:
    h1 = load_h1_judge()
    prompt = "short-probe"
    case = {
        "id": "case-00", "class": "short", "prompt_sha256": h1.prompt_digest(prompt),
        "expected": {"answer": 4}, "request_timeout_ms": 100,
    }
    seal = {
        "schema": 1, "mode": "a_cost_short_feasibility", "requests": 1,
        "scope": "short_feasibility_only", "h1_promotion_allowed": False,
        "judge_sha256": digest(Path(__file__).read_bytes()),
        "h1_judge_sha256": digest(H1_JUDGE.read_bytes()),
        "case": case,
        "slo": {"percentile": "nearest_rank", "ttft_limit_ms": 20,
                "itl_p95_limit_ms": 20, "e2e_limit_ms": 100},
    }
    request = {
        "request_id": "cost-0", "prompt": prompt, "response": json.dumps({"answer": 4}),
        "outcomes": [{"stop": None}, {"stop": "eos"}],
        "submission": "delivered", "released": True,
        "submission_authority": {"deadline": 1},
        "arrival_ms": 0, "first_output_ms": 20, "completed_ms": 40,
        "output_received_ms": [20, 40],
    }
    artifact = {
        "passed": True, "requests": [request], "request_count": 1,
        "completed_count": 1, "released_count": 1,
        "submissions": {"configured": 1, "delivered": 1, "uncertain": 0,
                        "unsubmitted": 0, "incomplete": 0, "unreleased": 0},
        "error": None, "evidence_missing": None, "cleanup_error": None,
    }
    return artifact, seal


def self_test() -> None:
    artifact, seal = fixture()
    assert evaluate(artifact, seal)["passed"]
    mutations = (
        lambda value: value[0]["requests"][0].update(response='{"answer":5}'),
        lambda value: value[0].update(passed=False),
        lambda value: value[0]["requests"][0]["submission_authority"].update(deadline=None),
        lambda value: (value[0]["requests"][0].update(completed_ms=101),
                       value[0]["requests"][0].update(output_received_ms=[20, 101])),
        lambda value: (value[0]["requests"][0].update(first_output_ms=21),
                       value[0]["requests"][0].update(output_received_ms=[21, 40])),
        lambda value: (value[0]["requests"][0].update(completed_ms=41),
                       value[0]["requests"][0].update(output_received_ms=[20, 41])),
        lambda value: (value[0]["requests"][0].update(outcomes=[{"stop": "eos"}]),
                       value[0]["requests"][0].update(completed_ms=20),
                       value[0]["requests"][0].update(output_received_ms=[20])),
        lambda value: value[1].update(h1_promotion_allowed=True),
        lambda value: value[1].update(h1_judge_sha256="0" * 64),
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
    result = evaluate(
        json.loads(args.artifact.read_text(encoding="utf-8")),
        json.loads(args.seal.read_text(encoding="utf-8")),
    )
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"passed": result["passed"], "scope": result["scope"],
                      "h1_accepted": result["h1_accepted"],
                      "global_failures": result["global_failures"]}, separators=(",", ":")))
    if not result["passed"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
