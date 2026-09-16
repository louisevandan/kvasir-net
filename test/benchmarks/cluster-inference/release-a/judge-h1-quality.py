#!/usr/bin/env python3
"""Judge a complete Release A H1 quality artifact against its sealed oracles."""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import re
from collections import defaultdict
from pathlib import Path


CLASSES = ("short", "medium", "long")

SOURCE_RECORD = re.compile(
    r"\[(R[0-9]{5})\] Station ([0-9]+); revision ([0-9]+)\. "
    r"Measured RMS current: ([0-9]+) A\. Isolated conductor resistance: "
    r"([0-9]+) milliohms\. Operating duration: ([0-9]+) hours\. "
    r"Inlet pressure: ([0-9]+) kPa\. Pressure alarm threshold: ([0-9]+) kPa; "
    r"equality is not an exceedance\. (?:No temperature measurement is recorded\.|"
    r"Temperature was not measured\.)"
)
POWER_TASK = (
    'Compute power in milliwatts as current_A squared times resistance_milliohms, '
    'and energy in milliwatt-hours as power_mW times duration_hours. '
    'Return JSON with keys "rows" and "temperature_measured". Each row must contain '
    '"id", "revision", "power_mW", "energy_mWh", and boolean "pressure_alarm". '
    '"temperature_measured" must state whether those records contain a measured temperature. '
    'Use integer arithmetic; do not infer a temperature or a pressure/heat causal relation.'
)


def verified_source_response(request: dict, *, check_timing: bool = True) -> list[str]:
    """Independently check OUTER's result from prompt and preserved model text."""
    if request.get("response_processor") != "engineering_power_v1":
        return ["source_processor_missing"]
    if request.get("service_error") is not None:
        return ["source_processing_failed"]
    raw = request.get("model_response")
    outcomes = request.get("outcomes")
    if (not isinstance(raw, str) or not isinstance(outcomes, list)
            or any(not isinstance(outcome, dict) or not isinstance(outcome.get("text"), str)
                   for outcome in outcomes)
            or raw != "".join(outcome["text"] for outcome in outcomes)):
        return ["model_response_evidence_mismatch"]
    prompt = request.get("prompt", "")
    if prompt.count("<|im_start|>user\n") != 1:
        return ["source_user_boundary"]
    user = prompt.split("<|im_start|>user\n", 1)[1].split("<|im_end|>", 1)[0]
    if "<|im_end|>" not in prompt.split("<|im_start|>user\n", 1)[1]:
        return ["source_user_boundary"]
    source, boundary, task = user.rpartition("\n\n")
    if not boundary:
        return ["source_task_boundary"]
    header, *lines = source.splitlines()
    count = re.fullmatch(r"Case [0-9]+: ([0-9]+) archived records\.", header)
    if count is None or not 0 < int(count[1]) <= 10000 or len(lines) != int(count[1]):
        return ["source_record_count"]
    records = {}
    for line in lines:
        match = SOURCE_RECORD.fullmatch(line)
        if match is None or match[1] in records:
            return ["source_record_invalid"]
        numbers = tuple(int(match[index]) for index in range(2, 9))
        if any(value > 2**64 - 1 for value in numbers):
            return ["source_quantity_overflow"]
        records[match[1]] = numbers
    prefix = "Connect the source facts for "
    marker = ", in that order. "
    if not task.startswith(prefix) or marker not in task or not task.endswith(POWER_TASK):
        return ["source_operation_contract"]
    ids, suffix = task[len(prefix):].split(marker, 1)
    if suffix != POWER_TASK:
        return ["source_operation_contract"]
    selected = ids.split(", ")
    if (not 0 < len(selected) <= 256 or len(selected) != len(set(selected))
            or any(identifier not in records for identifier in selected)):
        return ["source_selection"]
    raw_json = raw.strip()
    if raw_json.startswith("```json\n"):
        if not raw_json.endswith("\n```"):
            return ["model_json_fence"]
        raw_json = raw_json[len("```json\n"):-len("\n```")]
    try:
        model = json.loads(raw_json)
    except (TypeError, ValueError):
        return ["model_json_invalid"]
    if (not isinstance(model, dict) or set(model) != {"rows", "temperature_measured"}
            or model["temperature_measured"] is not False
            or not isinstance(model["rows"], list)
            or len(model["rows"]) != len(selected)):
        return ["model_schema_or_temperature"]
    calculated = []
    for identifier, row in zip(selected, model["rows"]):
        if (not isinstance(row, dict)
                or set(row) != {"id", "revision", "power_mW", "energy_mWh", "pressure_alarm"}
                or row.get("id") != identifier
                or not isinstance(row.get("revision"), int)
                or isinstance(row.get("revision"), bool)
                or row["revision"] != records[identifier][1]
                or any(not isinstance(row.get(field), int) or isinstance(row.get(field), bool)
                       or row[field] < 0 or row[field] > 2**64 - 1
                       for field in ("power_mW", "energy_mWh"))
                or not isinstance(row.get("pressure_alarm"), bool)):
            return ["model_source_identity_or_schema"]
        _, revision, current, resistance, duration, pressure, threshold = records[identifier]
        power = current * current * resistance
        energy = power * duration
        if power > 2**64 - 1 or energy > 2**64 - 1:
            return ["source_arithmetic_overflow"]
        calculated.append({"id": identifier, "revision": revision, "power_mW": power,
                           "energy_mWh": energy, "pressure_alarm": pressure > threshold})
    try:
        final = json.loads(request.get("response", ""))
    except (TypeError, ValueError):
        return ["service_json_invalid"]
    if final != {"rows": calculated, "temperature_measured": False}:
        return ["service_calculation_mismatch"]
    if check_timing:
        completed, service, released = (request.get(key) for key in
                                        ("completed_ms", "service_completed_ms", "release_ms"))
        if (not all(nonnegative_integer(value) for value in (completed, service, released))
                or not completed <= service <= released):
            return ["service_completion_order"]
    return []


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
    service = request.get("service_completed_ms")
    if request.get("response_processor") is not None:
        if not nonnegative_integer(service) or service < completed:
            failures.append("service_timing_missing")
            service = completed
        first_visible = service
    else:
        first_visible = first
    e2e = (service if request.get("response_processor") is not None else completed) - arrival
    if not nonnegative_integer(timeout_ms):
        failures.append("deadline_contract_missing")
    elif e2e > timeout_ms:
        failures.append("deadline_exceeded")
    timing = {
        "e2e_ms": e2e,
        "ttft_ms": first_visible - arrival,
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
        if case is not None and case.get("class") in ("medium", "long"):
            failures.extend(verified_source_response(request))
        elif request.get("response_processor") is not None or request.get("model_response") is not None:
            failures.append("unexpected_source_processor")
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
    source_row = ("[R00002] Station 7; revision 6. Measured RMS current: 28 A. "
                  "Isolated conductor resistance: 52 milliohms. Operating duration: 10 hours. "
                  "Inlet pressure: 121 kPa. Pressure alarm threshold: 120 kPa; "
                  "equality is not an exceedance. Temperature was not measured.")
    for name, case_number in (("medium", 5), ("long", 7)):
        prompts[name] = (f"<|im_start|>user\nCase {case_number}: 1 archived records.\n"
                         f"{source_row}\n\nConnect the source facts for R00002, in that order. "
                         f"{POWER_TASK}<|im_end|>")
    grounded_expected = {"rows": [{"id": "R00002", "revision": 6, "power_mW": 40768,
                                   "energy_mWh": 407680, "pressure_alarm": True}],
                         "temperature_measured": False}
    model_raw = json.dumps({"rows": [{"id": "R00002", "revision": 6, "power_mW": 1,
                                      "energy_mWh": 2, "pressure_alarm": False}],
                            "temperature_measured": False})
    seal = {
        "schema": 3, "requests": 3,
        "slo": {"percentile": "nearest_rank",
                "ttft_ms_by_class": {"short": 10, "medium": 50, "long": 60},
                "itl_p95_ms": 20},
        "cases": [{"id": f"c-{name}", "class": name,
                   "prompt_sha256": prompt_digest(prompt),
                   "expected": {"class": name} if name == "short" else grounded_expected,
                   "request_timeout_ms": 100} for name, prompt in prompts.items()],
    }
    requests = []
    for index, (name, prompt) in enumerate(prompts.items()):
        arrival = index * 100
        first = arrival + (index + 1) * 10
        completed = first + 20
        grounded = name != "short"
        requests.append({"request_id": f"r-{name}", "prompt": prompt,
                         "response": json.dumps(grounded_expected if grounded else {"class": name}),
                         "outcomes": [{"stop": None, "text": ""},
                                      {"stop": "eos", "text": model_raw if grounded else ""}],
                         "response_processor": "engineering_power_v1" if grounded else None,
                         "model_response": model_raw if grounded else None,
                         "service_error": None,
                         "service_completed_ms": completed + 5 if grounded else None,
                         "release_ms": completed + 10,
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
    for field, value in (("response", "{}"), ("model_response", "{}"),
                         ("service_completed_ms", 0), ("response_processor", None)):
        changed = json.loads(json.dumps([artifact, seal]))
        changed[0]["requests"][1][field] = value
        assert not evaluate(*changed)["passed"], field
    changed = json.loads(json.dumps([artifact, seal]))
    request = changed[0]["requests"][1]
    request["model_response"] = request["model_response"].replace('"revision": 6', '"revision": 7')
    request["outcomes"][-1]["text"] = request["model_response"]
    assert "model_source_identity_or_schema" in evaluate(*changed)["rows"][1]["failures"]
    print(json.dumps({"passed": True, "tests": 15}, separators=(",", ":")))


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
