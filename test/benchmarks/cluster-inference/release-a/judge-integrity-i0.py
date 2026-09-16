#!/usr/bin/env python3
"""Build and judge I0 scorecards from raw runtime and host evidence."""
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
H1_JUDGE = DIRECTORY / "judge-h1-quality.py"
INTEGRITY_JUDGE = DIRECTORY / "judge-integrity.py"
INTEGRITY_VALIDATOR = DIRECTORY / "validate-integrity-test-spec.py"


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def load_module(name: str, path: Path):
    module_spec = importlib.util.spec_from_file_location(name, path)
    if module_spec is None or module_spec.loader is None:
        raise RuntimeError(f"cannot load {path.name}")
    module = importlib.util.module_from_spec(module_spec)
    module_spec.loader.exec_module(module)
    return module


def finite(value: object) -> bool:
    return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value)


def percentile(values: list[float], fraction: float) -> float:
    if not values:
        raise ValueError("percentile source is empty")
    ordered = sorted(values)
    return ordered[math.ceil(len(ordered) * fraction) - 1]


def generated_tokens(request: dict) -> int:
    outcomes = request.get("outcomes") or []
    if not outcomes:
        raise ValueError("request outcomes are empty")
    terminal = outcomes[-1]
    remove_empty_eos = terminal.get("stop") == "eos" and terminal.get("text") == ""
    return len(outcomes) - int(remove_empty_eos)


def request_execution_ids(artifact: dict) -> dict[str, set[int]]:
    result: dict[str, set[int]] = defaultdict(set)
    for observation in artifact.get("batch_observations") or []:
        for physical in observation.get("physical_batches") or []:
            execution = physical.get("execution_id")
            if not isinstance(execution, int):
                raise ValueError("physical execution identity is missing")
            for owner in physical.get("owned_requests") or []:
                request_id = owner.get("request_id")
                if isinstance(request_id, str):
                    result[request_id].add(execution)
    return result


def intervals_peak(intervals: list[tuple[int, int]]) -> int:
    edges = []
    for start, end in intervals:
        if end < start:
            raise ValueError("stage interval order differs")
        edges.extend(((start, 1), (end, -1)))
    active = peak = 0
    for _, delta in sorted(edges, key=lambda edge: (edge[0], edge[1])):
        active += delta
        peak = max(peak, active)
    return peak


def overlap_ms(stage: int, by_stage: dict[int, list[tuple[int, int]]]) -> int:
    target = by_stage.get(stage, [])
    other = [interval for key, intervals in by_stage.items() if key != stage for interval in intervals]
    boundaries = sorted({point for interval in target + other for point in interval})
    total = 0
    for left, right in zip(boundaries, boundaries[1:]):
        if right <= left:
            continue
        target_open = any(start < right and end > left for start, end in target)
        other_open = any(start < right and end > left for start, end in other)
        if target_open and other_open:
            total += right - left
    return total


def scheduler_metrics(artifact: dict, request_id: str, phase: str, ubatch: int) -> dict:
    rows, observations, mixed = [], [], 0
    for observation in artifact.get("batch_observations") or []:
        selected = []
        for physical in observation.get("physical_batches") or []:
            owned = [owner for owner in physical.get("owned_requests") or []
                     if owner.get("request_id") == request_id]
            phase_rows = sum(owner.get(f"{phase}_rows", 0) for owner in owned)
            if phase_rows:
                selected.append(phase_rows)
                if physical.get("prefill_rows", 0) and physical.get("decode_rows", 0):
                    mixed += 1
        if selected:
            rows.extend(selected)
            observations.append(observation)
    if not rows:
        raise ValueError(f"{request_id} has no {phase} scheduler rows")
    snapshots = [row.get("scheduling") or {} for row in observations]
    eligible_key = "eligible_prefill" if phase == "prefill" else "eligible_decode"
    ready = [row.get("ready_sequences", 0) for row in observations]
    blocked_reasons = {
        "blocked_outstanding": sum(value.get("blocked_outstanding", 0) for value in snapshots),
        "no_ready_input": sum(value.get("no_ready_input", 0) for value in snapshots),
        "idle_gated": sum(value.get("idle_gated", 0) for value in observations),
    }
    return {
        "physical_batches": len(rows), "rows_mean": sum(rows) / len(rows),
        "rows_p50": percentile(rows, .50), "rows_p95": percentile(rows, .95),
        "rows_max": max(rows),
        "full_ubatch_fraction": sum(value >= ubatch for value in rows) / len(rows),
        "mixed_batches": mixed, "runnable": sum(ready),
        "eligible": sum(value.get(eligible_key, 0) for value in snapshots),
        "blocked": sum(value.get("blocked_outstanding", 0) for value in snapshots),
        "pending": sum(value.get("pending_admission", 0) for value in snapshots),
        "blocked_reasons": blocked_reasons,
        "flight_peak": max((value.get("open_batches_before_issue", 0) for value in snapshots), default=0),
        "open_peak": max((value.get("open_batches_before_issue", 0) for value in snapshots), default=0),
        "idle_ms": sum(value.get("idle_ms", 0) for value in observations),
    }


def stage_metrics(artifact: dict, executions: set[int]) -> list[dict]:
    by_stage: dict[int, list[tuple[int, int]]] = defaultdict(list)
    selected: dict[int, list[dict]] = defaultdict(list)
    for span in artifact.get("stage_spans") or []:
        if executions.intersection(span.get("execution_ids") or []):
            stage = span.get("node")
            if stage not in (0, 1, 2):
                raise ValueError("stage identity differs")
            times = [span.get(key) for key in
                     ("ingress_unix_ms", "start_unix_ms", "end_unix_ms", "forward_unix_ms")]
            if not all(isinstance(value, int) for value in times) or times != sorted(times):
                raise ValueError("stage local clock order differs")
            selected[stage].append(span)
            by_stage[stage].append((times[1], times[2]))
    if set(selected) != {0, 1, 2}:
        raise ValueError("all three stages did not report request execution")
    rows = []
    for stage in range(3):
        spans = selected[stage]
        compute = [span["end_unix_ms"] - span["start_unix_ms"] for span in spans]
        execution_count = len({value for span in spans for value in span["execution_ids"] if value in executions})
        metrics = {
            "spans": len(spans), "executions": execution_count,
            "rows": sum(span.get("rows", 0) for span in spans),
            "queue_ms": sum(span["start_unix_ms"] - span["ingress_unix_ms"] for span in spans),
            "compute_ms": sum(compute),
            "publish_ms": sum(span["forward_unix_ms"] - span["end_unix_ms"] for span in spans),
            "compute_p50_ms": percentile(compute, .50), "compute_p95_ms": percentile(compute, .95),
            "open_peak": intervals_peak(by_stage[stage]), "overlap_ms": overlap_ms(stage, by_stage),
        }
        rows.append({"stage": stage, "metrics": metrics})
    return rows


def host_metrics(external: dict, started_unix_ms: int, elapsed_ms: int, spec: dict) -> list[dict]:
    gpu = external.get("gpu") or {}
    interval = spec["required_scorecard"]["gpu_sample_interval_ms"]
    if gpu.get("sample_interval_ms") != interval:
        raise ValueError("GPU sample interval differs")
    hosts = gpu.get("hosts") or []
    if len(hosts) != 3 or len({row.get("host") for row in hosts}) != 3:
        raise ValueError("GPU host coverage differs")
    expected = elapsed_ms // interval + 1
    tolerance = spec["required_scorecard"]["gpu_sample_clock_tolerance_ms"]
    result = []
    for host in hosts:
        samples = [row for row in host.get("samples") or []
                   if isinstance(row.get("elapsed_ms"), int) and 0 <= row["elapsed_ms"] <= elapsed_ms]
        if [row["elapsed_ms"] for row in samples] != sorted({row["elapsed_ms"] for row in samples}):
            raise ValueError("GPU sample times are duplicate or unordered")
        if any(not isinstance(row.get("captured_unix_ms"), int)
               or abs(row["captured_unix_ms"] - (started_unix_ms + row["elapsed_ms"])) > tolerance
               for row in samples):
            raise ValueError("GPU samples are not anchored to the runtime window")
        if not samples:
            raise ValueError("GPU samples are absent")
        coverage = min(1.0, len(samples) / expected)
        if coverage < spec["required_scorecard"]["gpu_minimum_coverage"]:
            raise ValueError("GPU sample coverage is below the contract")
        util = [row.get("utilization_percent") for row in samples]
        memory = [row.get("memory_used_bytes") for row in samples]
        if any(not finite(value) or value < 0 or value > 100 for value in util):
            raise ValueError("GPU utilization sample differs")
        if any(not finite(value) or value <= 0 for value in memory):
            raise ValueError("GPU memory sample differs")
        unavailable = host.get("unavailable_reasons") or {}
        optional = {}
        for source, target in (("power_w", "power"), ("temperature_c", "temperature")):
            values = [row.get(source) for row in samples if row.get(source) is not None]
            if values and len(values) != len(samples):
                raise ValueError(f"partial GPU {target} samples")
            if not values and target not in unavailable:
                raise ValueError(f"missing GPU {target} reason")
            optional[target] = sum(values) / len(values) if values else None
        result.append({"host": host["host"], "metrics": {
            "samples": len(samples), "coverage": coverage,
            "util_mean": sum(util) / len(util), "util_p50": percentile(util, .50),
            "util_p90": percentile(util, .90),
            "zero_fraction": sum(value == 0 for value in util) / len(util),
            "memory_peak": max(memory), **optional, "unavailable_reasons": unavailable,
        }})
    return result


def validate_external(external: dict, artifact: dict, artifact_bytes: bytes,
                      seal: dict, elapsed_ms: int, spec: dict) -> None:
    if external.get("schema") != "p4.release-a.integrity-i0-external.v1":
        raise ValueError("I0 external evidence schema differs")
    if external.get("artifact_sha256") != digest(artifact_bytes):
        raise ValueError("external evidence is not bound to the runtime artifact")
    if external.get("config_sha256") != seal.get("config_sha256"):
        raise ValueError("external evidence is not bound to the execution config")
    started = artifact.get("started_unix_ms")
    if not isinstance(started, int) or started <= 0:
        raise ValueError("runtime artifact lacks an absolute inference window")
    if external.get("run_window") != {"started_unix_ms": started, "elapsed_ms": elapsed_ms}:
        raise ValueError("external evidence run window differs")
    snapshots = external.get("resource_snapshots") or []
    names = [row.get("name") for row in snapshots]
    if names != spec["required_scorecard"]["resource_snapshots"]:
        raise ValueError("resource snapshot sequence differs")
    for snapshot in snapshots:
        hosts = snapshot.get("hosts") or []
        if len(hosts) != 3 or len({row.get("host") for row in hosts}) != 3:
            raise ValueError("resource snapshot host coverage differs")
        expected = spec["required_scorecard"]["resource_snapshot_states"][snapshot["name"]]
        if any({key: row.get(key) for key in expected} != expected for row in hosts):
            raise ValueError("resource snapshot state differs")
    host_metrics(external, started, elapsed_ms, spec)


def build_bundle(artifact: dict, artifact_bytes: bytes, seal: dict, external: dict, spec: dict) -> dict:
    load_module("release_a_integrity_validator", INTEGRITY_VALIDATOR).validate(spec)
    if seal.get("mode") != "release_a_integrity_i0" or seal.get("same_load") is not True:
        raise ValueError("I0 seal does not prove one loaded execution group")
    if seal.get("load_count") != 1 or seal.get("unload_count") != 1:
        raise ValueError("I0 load/unload count differs")
    if seal.get("config_sha256") != external.get("config_sha256"):
        raise ValueError("I0 config binding differs")
    if artifact.get("request_count") != 3 or len(artifact.get("requests") or []) != 3:
        raise ValueError("I0 runtime request coverage differs")
    if len(artifact.get("stage_builds") or []) != 3:
        raise ValueError("I0 stage identity coverage differs")
    elapsed_ms = artifact.get("elapsed_ms")
    if not isinstance(elapsed_ms, int) or elapsed_ms <= 0:
        raise ValueError("I0 elapsed time is invalid")
    validate_external(external, artifact, artifact_bytes, seal, elapsed_ms, spec)

    h1 = load_module("release_a_h1_judge", H1_JUDGE).evaluate(artifact, seal)
    if not h1["passed"]:
        global_failures = ",".join(h1["global_failures"]) or "none"
        request_failures = ";".join(
            f"{row.get('case_id') or row.get('request_id')}:{','.join(row['failures'])}"
            for row in h1["rows"] if row["failures"]
        ) or "none"
        raise ValueError("I0 raw oracle/timing gate failed: "
                         f"global={global_failures}; requests={request_failures}")
    request_rows = artifact["requests"]
    h1_by_case = {row["case_id"]: row for row in h1["rows"]}
    executions = request_execution_ids(artifact)
    gpu = host_metrics(external, artifact["started_unix_ms"], elapsed_ms, spec)
    if any(request.get("release_ms") is None for request in request_rows):
        raise ValueError("I0 release timestamps are missing")
    for prior, following in zip(request_rows, request_rows[1:]):
        if prior["release_ms"] > following.get("eligible_ms", -1):
            raise ValueError("I0 next request became eligible before prior RELEASE")

    arm_by_case = {case["id"]: arm for case, arm in zip(seal["cases"], seal["arms"])}
    case_by_digest = {case["prompt_sha256"]: case for case in seal["cases"]}
    results = []
    for request in request_rows:
        prompt_sha = digest(request.get("prompt", "").encode())
        case = case_by_digest.get(prompt_sha)
        if case is None:
            raise ValueError("I0 request prompt is not in the seal")
        arm_id = arm_by_case[case["id"]]
        timing = h1_by_case[case["id"]]["timing"]
        if request.get("send_completed_ms") is None:
            raise ValueError("I0 successful send completion time is missing")
        if not (request.get("eligible_ms", -1) <= request.get("send_started_ms", -1)
                <= request["send_completed_ms"] <= request["first_output_ms"]
                <= request["completed_ms"] <= request["release_ms"]):
            raise ValueError("I0 request clock order differs")
        if request["send_started_ms"] - request["eligible_ms"] > spec["slo"]["send_slip_ms"]:
            raise ValueError("I0 send slip exceeds the SLO")
        generated = generated_tokens(request)
        useful_ms = request["completed_ms"] - request["arrival_ms"]
        release_ms = request["release_ms"] - request["arrival_ms"]
        if useful_ms <= 0 or release_ms <= 0:
            raise ValueError("I0 throughput denominator is not positive")
        scheduler = [
            {"phase": phase, "metrics": scheduler_metrics(
                artifact, request["request_id"], phase, spec["model"]["n_ubatch"]
            )} for phase in ("prefill", "decode")
        ]
        stages = stage_metrics(artifact, executions.get(request["request_id"], set()))
        request_fields = {
            "eligible_ms": request["eligible_ms"], "send_started_ms": request["send_started_ms"],
            "send_completed_ms": request["send_completed_ms"], "arrival_ms": request["arrival_ms"],
            "first_output_ms": request["first_output_ms"],
            "output_received_ms": request["output_received_ms"], "terminal_ms": request["completed_ms"],
            "release_ms": request["release_ms"], "class": case["class"],
            "deadline_ms": case["request_timeout_ms"], "input_tokens": case["input_tokens"],
            "generated_tokens": generated, "stop_reason": request["outcomes"][-1]["stop"],
            "oracle_passed": h1_by_case[case["id"]]["passed"],
            "queue_ms": sum(row["metrics"]["queue_ms"] for row in stages),
            "prefill_rows": request["prefill_rows"], "prefill_ms": request["prefill_elapsed_ms"],
            "decode_rows": request["decode_rows"], "decode_ms": request["generation_elapsed_ms"],
        }
        run = {
            "useful_generation_tps": generated * 1000 / useful_ms,
            "total_generation_tps": generated * 1000 / release_ms,
            "prefill_rows_per_second": request["prefill_rows"] * 1000 / request["prefill_elapsed_ms"],
            "decode_rows_per_second": request["decode_rows"] * 1000 / request["generation_elapsed_ms"],
            "ttft": {case["class"]: {"p95_ms": timing["ttft_ms"]}},
            "itl": {case["class"]: {"p95_ms": percentile(timing["itl_ms"], .95)}},
            "deadline_violations": int(timing["e2e_ms"] > case["request_timeout_ms"]),
            "send_slip_violations": int(request["send_started_ms"] - request["eligible_ms"]
                                        > spec["slo"]["send_slip_ms"]),
            "first_scheduled_send_ms": request["eligible_ms"],
            "last_terminal_ms": request["completed_ms"],
        }
        counts = {key: 0 for key in ("rejected", "failed", "canceled", "uncertain", "unsubmitted",
                                             "incomplete", "unreleased")}
        counts.update(configured=1, classified=1, delivered=1, completed=1, released=1,
                      oracle_passed=1, eos=1, accepted=1)
        results.append({
            "id": arm_id, "passed": True, "counts": counts, "error": None,
            "evidence_missing": None, "cleanup_error": None,
            "requests": [request_fields],
            "scorecard": {
                "requests_with_all_fields": 1,
                "request_field_names": list(request_fields), "run": run,
                "scheduler_phases": scheduler, "stages": stages, "hosts": copy.deepcopy(gpu),
                "resource_snapshots": [row["name"] for row in external["resource_snapshots"]],
            },
            "distributed": copy.deepcopy(external["distributed"]),
            "cleanup": copy.deepcopy(external["cleanup"]),
        })
    return {
        "schema": 1, "spec_id": spec["spec_id"],
        "runtime_commit": spec["source"]["runtime_commit"],
        "integrity_baseline": False, "performance_improvement_claimed": False,
        "execution_group": {"id": "I0", "same_load": True, "load_count": 1, "unload_count": 1},
        "arms": results,
    }


def fixture(spec: dict) -> tuple[dict, bytes, dict, dict]:
    prompts = ["p-short", "p-medium", "p-long"]
    classes = ["short", "medium", "long"]
    starts = [0, 100, 200]
    requests = []
    observations, spans = [], []
    cases = []
    for index, (prompt, request_class, start) in enumerate(zip(prompts, classes, starts)):
        request_id = f"integrity-i0-{index + 1:03}"
        cases.append({"id": ("case-00", "case-04", "case-06")[index],
                      "class": request_class, "prompt_sha256": digest(prompt.encode()),
                      "expected": {"class": request_class}, "input_tokens": (10, 20, 30)[index],
                      "request_timeout_ms": spec["slo"]["request_deadline_ms"][request_class]})
        requests.append({
            "request_id": request_id, "prompt": prompt,
            "response": json.dumps({"class": request_class}),
            "submission": "delivered", "released": True,
            "submission_authority": {"deadline": 1},
            "eligible_ms": start, "send_started_ms": start + 1, "send_completed_ms": start + 2,
            "arrival_ms": start + 1, "first_output_ms": start + 20,
            "completed_ms": start + 60, "release_ms": start + 70,
            "output_received_ms": [start + 20, start + 60],
            "prefill_rows": 64, "decode_rows": 2, "verify_rows": 0, "replay_rows": 0,
            "prefill_elapsed_ms": 19, "generation_elapsed_ms": 40,
            "logical_prefill_tps": 1.0, "logical_generation_tps": 1.0,
            "outcomes": [{"text": "x", "stop": None}, {"text": "", "stop": "eos"}],
        })
        physical = {"execution_id": index + 1, "rows": 66, "prefill_rows": 64,
                    "decode_rows": 2, "verify_rows": 0, "replay_rows": 0,
                    "owned_requests": [{"request_id": request_id, "prefill_rows": 64,
                                        "decode_rows": 2, "verify_rows": 0, "replay_rows": 0}]}
        scheduling = {"eligible_prefill": 1, "eligible_decode": 1, "blocked_outstanding": 0,
                      "pending_admission": 0, "no_ready_input": 0, "open_batches_before_issue": 0}
        observations.append({"physical_batches": [physical], "scheduling": scheduling,
                             "ready_sequences": 1, "idle_gated": 0, "idle_ms": 1})
        for stage in range(3):
            base = 1_000 + index * 100 + stage * 10
            spans.append({"node": stage, "execution_ids": [index + 1], "rows": 66,
                          "ingress_unix_ms": base, "start_unix_ms": base + 1,
                          "end_unix_ms": base + 6, "forward_unix_ms": base + 7})
    artifact = {
        "passed": True, "request_count": 3, "completed_count": 3, "released_count": 3,
        "requests": requests, "submissions": {"configured": 3, "delivered": 3,
        "uncertain": 0, "unsubmitted": 0, "incomplete": 0, "unreleased": 0},
        "stage_builds": [{"node": index} for index in range(3)],
        "batch_observations": observations, "stage_spans": spans,
        "started_unix_ms": 1_000_000, "elapsed_ms": 1000,
        "telemetry_complete_elapsed_ms": 1000,
        "error": None, "evidence_missing": None, "cleanup_error": None,
    }
    artifact_bytes = (json.dumps(artifact, separators=(",", ":")) + "\n").encode()
    seal = {
        "schema": 3, "mode": "release_a_integrity_i0", "same_load": True,
        "load_count": 1, "unload_count": 1, "requests": 3,
        "arms": ["I0-S", "I0-M", "I0-L"], "cases": cases,
        "slo": {"percentile": "nearest_rank", "ttft_ms_by_class": spec["slo"]["ttft_p95_ms"],
                "itl_p95_ms": spec["slo"]["itl_p95_ms"]}, "config_sha256": "c" * 64,
    }
    samples = [{"elapsed_ms": value, "captured_unix_ms": 1_000_000 + value,
                "utilization_percent": 50,
                "memory_used_bytes": 1024, "power_w": None, "temperature_c": None}
               for value in (0, 1000)]
    states = spec["required_scorecard"]["resource_snapshot_states"]
    external = {
        "schema": "p4.release-a.integrity-i0-external.v1",
        "artifact_sha256": digest(artifact_bytes), "config_sha256": seal["config_sha256"],
        "run_window": {"started_unix_ms": 1_000_000, "elapsed_ms": 1000},
        "gpu": {"sample_interval_ms": 1000, "hosts": [
            {"host": f"h{index}", "samples": copy.deepcopy(samples),
             "unavailable_reasons": {"power": "unsupported", "temperature": "unsupported"}}
            for index in range(3)]},
        "resource_snapshots": [{"name": name, "hosts": [
            {"host": f"h{i}", "captured_unix_ms": 1_000_000, **states[name]}
            for i in range(3)]} for name in spec["required_scorecard"]["resource_snapshots"]],
        "distributed": {"physical_hosts": 3, "stages": 3,
                        "all_hosts_own_model_shard": True, "all_hosts_own_kv": True,
                        "all_stages_compute": True, "cross_host_transfer_bytes": 1,
                        "undeclared_hosts": 0},
        "cleanup": {**spec["cleanup"], "transport_failures": 0,
                    "transport_failures_reconciled": True},
    }
    return artifact, artifact_bytes, seal, external


def self_test(spec: dict) -> None:
    artifact, artifact_bytes, seal, external = fixture(spec)
    bundle = build_bundle(artifact, artifact_bytes, seal, external, spec)
    assert [row["id"] for row in bundle["arms"]] == ["I0-S", "I0-M", "I0-L"]
    integrity = load_module("release_a_integrity_judge", INTEGRITY_JUDGE)
    full = integrity.fixture(spec)
    replacements = {row["id"]: row for row in bundle["arms"]}
    full["arms"] = [replacements.get(row["id"], row) for row in full["arms"]]
    full["integrity_baseline"] = True
    assert integrity.evaluate(full, spec)["passed"]
    wrong = copy.deepcopy(artifact)
    wrong["requests"][0]["response"] = json.dumps({"class": "wrong"})
    wrong_bytes = (json.dumps(wrong, separators=(",", ":")) + "\n").encode()
    wrong_external = copy.deepcopy(external)
    wrong_external["artifact_sha256"] = digest(wrong_bytes)
    try:
        build_bundle(wrong, wrong_bytes, seal, wrong_external, spec)
    except ValueError as error:
        assert "case-00:oracle_mismatch" in str(error)
    else:
        raise AssertionError("request-level oracle failure was accepted")
    mutations = (
        lambda a, s, e: a["requests"][0].pop("release_ms"),
        lambda a, s, e: a["requests"][1].update(eligible_ms=50),
        lambda a, s, e: a["requests"][0].update(send_started_ms=1002),
        lambda a, s, e: a["stage_spans"].__setitem__(slice(None), [x for x in a["stage_spans"] if x["node"] != 2]),
        lambda a, s, e: e["gpu"]["hosts"][0]["samples"].pop(),
        lambda a, s, e: e["gpu"]["hosts"][0]["samples"][0].update(captured_unix_ms=999_000),
        lambda a, s, e: e["resource_snapshots"].pop(),
        lambda a, s, e: e["resource_snapshots"][1]["hosts"][0].update(model_resident=False),
        lambda a, s, e: e.update(artifact_sha256="0" * 64),
        lambda a, s, e: s.update(load_count=3),
    )
    for mutate in mutations:
        changed_artifact = copy.deepcopy(artifact)
        changed_seal = copy.deepcopy(seal)
        changed_external = copy.deepcopy(external)
        mutate(changed_artifact, changed_seal, changed_external)
        changed_bytes = (json.dumps(changed_artifact, separators=(",", ":")) + "\n").encode()
        if changed_external["artifact_sha256"] == digest(artifact_bytes):
            changed_external["artifact_sha256"] = digest(changed_bytes)
        try:
            build_bundle(changed_artifact, changed_bytes, changed_seal, changed_external, spec)
        except (ValueError, KeyError, TypeError):
            pass
        else:
            raise AssertionError("weakened I0 evidence was accepted")
    print(json.dumps({"passed": True, "tests": 12}, separators=(",", ":")))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--spec", type=Path, required=True)
    parser.add_argument("--artifact", type=Path)
    parser.add_argument("--seal", type=Path)
    parser.add_argument("--external", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    spec = json.loads(args.spec.read_text(encoding="utf-8"))
    if args.self_test:
        self_test(spec)
        return
    if any(value is None for value in (args.artifact, args.seal, args.external, args.output)):
        parser.error("artifact, seal, external and output are required")
    artifact_bytes = args.artifact.read_bytes()
    bundle = build_bundle(json.loads(artifact_bytes), artifact_bytes,
                          json.loads(args.seal.read_text(encoding="utf-8")),
                          json.loads(args.external.read_text(encoding="utf-8")), spec)
    args.output.write_text(json.dumps(bundle, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"passed": True, "arms": [row["id"] for row in bundle["arms"]]},
                     separators=(",", ":")))


if __name__ == "__main__":
    main()
