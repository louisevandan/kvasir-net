#!/usr/bin/env python3
"""Judge all 64 I1 service requests and aggregate one distributed scorecard."""
from __future__ import annotations

import argparse
import copy
import hashlib
import importlib.util
import json
import math
from collections import defaultdict
from pathlib import Path


DIRECTORY = Path(__file__).resolve().parent
CLASSES = ("short", "medium", "long")


def load(name: str, filename: str):
    spec = importlib.util.spec_from_file_location(name, DIRECTORY / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def percentile(values: list[float], fraction: float) -> float:
    if not values:
        raise ValueError("I1 scorecard percentile has no observations")
    ordered = sorted(values)
    return ordered[math.ceil(len(ordered) * fraction) - 1]


def scheduler_metrics(artifact: dict, phase: str, ubatch: int) -> dict:
    rows, observations, mixed = [], [], 0
    for observation in artifact.get("batch_observations") or []:
        selected = False
        for physical in observation.get("physical_batches") or []:
            count = physical.get(f"{phase}_rows", 0)
            if count:
                if not isinstance(count, int) or count < 0:
                    raise ValueError("I1 scheduler row count differs")
                rows.append(count)
                selected = True
                mixed += int(bool(physical.get("prefill_rows") and physical.get("decode_rows")))
        if selected:
            observations.append(observation)
    if not rows:
        raise ValueError(f"I1 {phase} scheduler rows are absent")
    snapshots = [row.get("scheduling") or {} for row in observations]
    eligible_key = "eligible_prefill" if phase == "prefill" else "eligible_decode"
    return {
        "physical_batches": len(rows), "rows_mean": sum(rows) / len(rows),
        "rows_p50": percentile(rows, .50), "rows_p95": percentile(rows, .95),
        "rows_max": max(rows),
        "full_ubatch_fraction": sum(value >= ubatch for value in rows) / len(rows),
        "mixed_batches": mixed,
        "runnable": sum(row.get("ready_sequences", 0) for row in observations),
        "eligible": sum(row.get(eligible_key, 0) for row in snapshots),
        "blocked": sum(row.get("blocked_outstanding", 0) for row in snapshots),
        "pending": sum(row.get("pending_admission", 0) for row in snapshots),
        "blocked_reasons": {
            "blocked_outstanding": sum(row.get("blocked_outstanding", 0) for row in snapshots),
            "no_ready_input": sum(row.get("no_ready_input", 0) for row in snapshots),
            "idle_gated": sum(row.get("idle_gated", 0) for row in observations),
        },
        "flight_peak": max((row.get("open_batches_before_issue", 0) for row in snapshots), default=0),
        "open_peak": max((row.get("open_batches_before_issue", 0) for row in snapshots), default=0),
        "idle_ms": sum(row.get("idle_ms", 0) for row in observations),
    }


def sweep_stage_intervals(by_stage: dict[int, list[tuple[int, int]]]) -> tuple[dict[int, int], dict[int, int]]:
    events = []
    for stage in range(3):
        for start, end in by_stage.get(stage, []):
            if end < start:
                raise ValueError("I1 stage interval order differs")
            if end > start:
                events.extend(((start, stage, 1), (end, stage, -1)))
    events.sort()
    active = {stage: 0 for stage in range(3)}
    peak = {stage: 0 for stage in range(3)}
    overlap = {stage: 0 for stage in range(3)}
    prior = None
    index = 0
    while index < len(events):
        moment = events[index][0]
        if prior is not None and moment > prior:
            for stage in range(3):
                if active[stage] and any(active[other] for other in range(3) if other != stage):
                    overlap[stage] += moment - prior
        while index < len(events) and events[index][0] == moment:
            _, stage, delta = events[index]
            active[stage] += delta
            if active[stage] < 0:
                raise ValueError("I1 stage active count became negative")
            index += 1
        for stage in range(3):
            peak[stage] = max(peak[stage], active[stage])
        prior = moment
    if any(active.values()):
        raise ValueError("I1 stage intervals did not close")
    return peak, overlap


def stage_metrics(artifact: dict, execution_ids: set[int]) -> tuple[list[dict], dict[int, int]]:
    selected = defaultdict(list)
    by_stage = defaultdict(list)
    queue_by_execution = defaultdict(int)
    for span in artifact.get("stage_spans") or []:
        ids = set(span.get("execution_ids") or [])
        if not ids or not ids <= execution_ids:
            raise ValueError("I1 stage span execution identity differs")
        stage = span.get("node")
        if stage not in (0, 1, 2):
            raise ValueError("I1 stage identity differs")
        times = [span.get(key) for key in
                 ("ingress_unix_ms", "start_unix_ms", "end_unix_ms", "forward_unix_ms")]
        if not all(isinstance(value, int) for value in times) or times != sorted(times):
            raise ValueError("I1 stage clock order differs")
        selected[stage].append(span)
        by_stage[stage].append((times[1], times[2]))
        for execution in ids:
            queue_by_execution[execution] += times[1] - times[0]
    if set(selected) != {0, 1, 2}:
        raise ValueError("I1 stage coverage differs")
    peak, overlap = sweep_stage_intervals(by_stage)
    result = []
    for stage in range(3):
        spans = selected[stage]
        compute = [row["end_unix_ms"] - row["start_unix_ms"] for row in spans]
        result.append({"stage": stage, "metrics": {
            "spans": len(spans),
            "executions": len({value for row in spans for value in row["execution_ids"]}),
            "rows": sum(row.get("rows", 0) for row in spans),
            "queue_ms": sum(row["start_unix_ms"] - row["ingress_unix_ms"] for row in spans),
            "compute_ms": sum(compute),
            "publish_ms": sum(row["forward_unix_ms"] - row["end_unix_ms"] for row in spans),
            "compute_p50_ms": percentile(compute, .50),
            "compute_p95_ms": percentile(compute, .95),
            "open_peak": peak[stage], "overlap_ms": overlap[stage],
        }})
    return result, queue_by_execution


def build_bundle(artifact: dict, artifact_bytes: bytes, seal: dict, external: dict,
                 spec: dict) -> dict:
    i0 = load("i1_i0_judge_helpers", "judge-integrity-i0.py")
    h1 = load("i1_h1_judge", "judge-h1-quality.py")
    validator = load("i1_integrity_validator", "validate-integrity-test-spec.py")
    validator.validate(spec)
    if external.get("schema") != "p4.release-a.integrity-i1-external.v1":
        raise ValueError("I1 external evidence schema differs")
    i0.validate_external({**external, "schema": "p4.release-a.integrity-i0-external.v1"},
                         artifact, artifact_bytes, seal, artifact.get("elapsed_ms"), spec)
    if artifact.get("request_count") != 64 or len(artifact.get("requests") or []) != 64:
        raise ValueError("I1 request coverage differs")
    if len(artifact.get("stage_builds") or []) != 3:
        raise ValueError("I1 stage build coverage differs")
    judgment = h1.evaluate(artifact, seal)
    if not judgment["passed"]:
        failed = [(row["case_id"], row["failures"]) for row in judgment["rows"] if row["failures"]]
        raise ValueError(f"I1 service oracle/timing gate failed: {judgment['global_failures']}; {failed}")
    executions = i0.request_execution_ids(artifact)
    all_ids = set().union(*executions.values()) if executions else set()
    if not all_ids:
        raise ValueError("I1 scheduler execution identities are absent")
    stages, queue_by_execution = stage_metrics(artifact, all_ids)
    case_by_prompt = {case["prompt_sha256"]: case for case in seal["cases"]}
    judged = {row["case_id"]: row for row in judgment["rows"]}
    request_fields = []
    useful_tokens = total_tokens = 0
    for request in artifact["requests"]:
        case = case_by_prompt.get(digest(request.get("prompt", "").encode()))
        if case is None or not judged[case["id"]]["passed"]:
            raise ValueError("I1 request source identity differs")
        if request["request_id"] not in executions:
            raise ValueError("I1 request has no scheduled execution")
        generated = i0.generated_tokens(request)
        total_tokens += generated
        processed = request.get("response_processor") is not None
        if processed != (request.get("model_response") is not None):
            raise ValueError("I1 service/model output provenance differs")
        if not processed:
            useful_tokens += generated
        if not (request["eligible_ms"] <= request["send_started_ms"] <=
                request["send_completed_ms"] <= request["first_output_ms"] <=
                request["completed_ms"] <= request["release_ms"]):
            raise ValueError("I1 request clock order differs")
        timing = judged[case["id"]]["timing"]
        request_fields.append({
            "eligible_ms": request["eligible_ms"], "send_started_ms": request["send_started_ms"],
            "send_completed_ms": request["send_completed_ms"], "arrival_ms": request["arrival_ms"],
            "first_output_ms": request["first_output_ms"],
            "output_received_ms": request["output_received_ms"], "terminal_ms": request["completed_ms"],
            "release_ms": request["release_ms"], "class": case["class"],
            "deadline_ms": case["request_timeout_ms"], "input_tokens": case["input_tokens"],
            "generated_tokens": generated, "stop_reason": request["outcomes"][-1]["stop"],
            "oracle_passed": True,
            "queue_ms": sum(queue_by_execution[value] for value in executions[request["request_id"]]),
            "prefill_rows": request["prefill_rows"], "prefill_ms": request["prefill_elapsed_ms"],
            "decode_rows": request["decode_rows"], "decode_ms": request["generation_elapsed_ms"],
        })
        if timing["e2e_ms"] > case["request_timeout_ms"]:
            raise ValueError("I1 request deadline differs")
    first = min(row["eligible_ms"] for row in request_fields)
    last = max(row["terminal_ms"] for row in request_fields)
    released = max(row["release_ms"] for row in request_fields)
    prefill_ms = sum(row["prefill_ms"] for row in request_fields)
    decode_ms = sum(row["decode_ms"] for row in request_fields)
    if not first < last <= released or prefill_ms <= 0 or decode_ms <= 0:
        raise ValueError("I1 throughput denominator differs")
    run = {
        "useful_generation_tps": useful_tokens * 1000 / (last - first),
        "total_generation_tps": total_tokens * 1000 / (released - first),
        "prefill_rows_per_second": sum(row["prefill_rows"] for row in request_fields) * 1000 / prefill_ms,
        "decode_rows_per_second": sum(row["decode_rows"] for row in request_fields) * 1000 / decode_ms,
        "ttft": {name: {"p95_ms": judgment["class_metrics"][name]["ttft_p95_ms"]} for name in CLASSES},
        "itl": {name: {"p95_ms": judgment["class_metrics"][name]["itl_p95_ms"]} for name in CLASSES},
        "deadline_violations": 0,
        "send_slip_violations": sum(row["send_started_ms"] - row["eligible_ms"]
                                    > spec["slo"]["send_slip_ms"] for row in request_fields),
        "first_scheduled_send_ms": first, "last_terminal_ms": last,
    }
    counts = {key: 0 for key in ("rejected", "failed", "canceled", "uncertain", "unsubmitted",
                                 "incomplete", "unreleased")}
    counts.update(configured=64, classified=64, delivered=64, completed=64, released=64,
                  oracle_passed=64, eos=64, accepted=64)
    arm = {
        "id": "I1-Q64", "passed": True, "counts": counts,
        "error": None, "evidence_missing": None, "cleanup_error": None,
        "requests": request_fields,
        "scorecard": {
            "requests_with_all_fields": 64,
            "request_field_names": list(request_fields[0]), "run": run,
            "scheduler_phases": [{"phase": phase, "metrics": scheduler_metrics(
                artifact, phase, spec["model"]["n_ubatch"])} for phase in ("prefill", "decode")],
            "stages": stages,
            "hosts": i0.host_metrics(external, artifact["started_unix_ms"],
                                     artifact["elapsed_ms"], spec),
            "resource_snapshots": [row["name"] for row in external["resource_snapshots"]],
        },
        "distributed": copy.deepcopy(external["distributed"]),
        "cleanup": copy.deepcopy(external["cleanup"]),
    }
    integrity = load("i1_integrity_judge", "judge-integrity.py")
    failures = integrity.arm_failures(arm, next(row for row in spec["arms"] if row["id"] == "I1-Q64"), spec)
    if failures:
        raise ValueError(f"I1 integrity arm failed: {failures}")
    return {"schema": 1, "spec_id": spec["spec_id"],
            "runtime_commit": spec["source"]["runtime_commit"],
            "integrity_baseline": False, "performance_improvement_claimed": False,
            "execution_group": {"id": "I1", "same_load": True, "load_count": 1, "unload_count": 1},
            "arms": [arm]}


def self_test() -> None:
    peak, overlap = sweep_stage_intervals({
        0: [(0, 10)], 1: [(5, 15)], 2: [(8, 12)]})
    assert peak == {0: 1, 1: 1, 2: 1}
    assert overlap == {0: 5, 1: 7, 2: 4}
    peak, overlap = sweep_stage_intervals({0: [(0, 5), (5, 10)], 1: [(10, 12)], 2: []})
    assert peak[0] == 1 and all(value == 0 for value in overlap.values())
    try:
        sweep_stage_intervals({0: [(3, 2)]})
    except ValueError:
        pass
    else:
        raise AssertionError("reversed I1 stage interval was accepted")
    print(json.dumps({"passed": True, "tests": 3}, separators=(",", ":")))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--spec", type=Path)
    parser.add_argument("--artifact", type=Path)
    parser.add_argument("--seal", type=Path)
    parser.add_argument("--external", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test(); return
    if any(value is None for value in (args.spec, args.artifact, args.seal, args.external, args.output)):
        parser.error("spec, artifact, seal, external and output are required")
    artifact_bytes = args.artifact.read_bytes()
    result = build_bundle(json.loads(artifact_bytes), artifact_bytes,
                          json.loads(args.seal.read_text()), json.loads(args.external.read_text()),
                          json.loads(args.spec.read_text()))
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({"passed": True, "arms": [row["id"] for row in result["arms"]]}, separators=(",", ":")))


if __name__ == "__main__":
    main()
