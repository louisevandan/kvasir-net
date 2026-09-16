#!/usr/bin/env python3
"""Judge the complete Release A integrity-first evidence bundle."""
from __future__ import annotations

import argparse
import copy
import json
import math
from pathlib import Path

from importlib.util import module_from_spec, spec_from_file_location


DIRECTORY = Path(__file__).resolve().parent
VALIDATOR = DIRECTORY / "validate-integrity-test-spec.py"


def load_validator():
    module_spec = spec_from_file_location("integrity_spec_validator", VALIDATOR)
    if module_spec is None or module_spec.loader is None:
        raise RuntimeError("cannot load integrity spec validator")
    module = module_from_spec(module_spec)
    module_spec.loader.exec_module(module)
    return module


def finite_number(value: object) -> bool:
    return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value)


def expected_requests(arm: dict) -> int:
    if isinstance(arm.get("requests"), int):
        return arm["requests"]
    selection = arm.get("selection")
    if isinstance(selection, list):
        return len(selection)
    raise ValueError(f"arm request count missing: {arm.get('id')}")


def scorecard_failures(result: dict, arm: dict, spec: dict) -> list[str]:
    failures: list[str] = []
    score = result.get("scorecard") or {}
    required = spec["required_scorecard"]
    configured = expected_requests(arm)
    if score.get("requests_with_all_fields") != configured:
        failures.append("request_scorecard_incomplete")
    if set(score.get("request_field_names") or []) != set(required["request_fields"]):
        failures.append("request_scorecard_fields")
    run = score.get("run") or {}
    if set(run) != set(required["run_fields"]):
        failures.append("run_scorecard_fields")
    elif any(not finite_number(run[key]) for key in (
        "useful_generation_tps", "total_generation_tps", "prefill_rows_per_second",
        "decode_rows_per_second", "first_scheduled_send_ms", "last_terminal_ms",
        "deadline_violations", "send_slip_violations",
    )) or not isinstance(run.get("ttft"), dict) or not isinstance(run.get("itl"), dict):
        failures.append("run_scorecard_values")
    elif run["deadline_violations"] != 0 or run["send_slip_violations"] != 0:
        failures.append("request_timing_violation")

    expected_classes = {
        "I0-S": {"short"}, "I0-M": {"medium"}, "I0-L": {"long"},
    }.get(arm["id"], {"short", "medium", "long"} if
          (arm["kind"] == "normal" or arm.get("requires_oracle_eos_release") is True) else set())
    if expected_classes:
        if set(run.get("ttft") or {}) != expected_classes or set(run.get("itl") or {}) != expected_classes:
            failures.append("latency_class_coverage")
        else:
            for request_class in expected_classes:
                ttft = run["ttft"][request_class].get("p95_ms")
                itl = run["itl"][request_class].get("p95_ms")
                if not finite_number(ttft) or ttft > spec["slo"]["ttft_p95_ms"][request_class]:
                    failures.append(f"ttft_exceeded:{request_class}")
                if not finite_number(itl) or itl > spec["slo"]["itl_p95_ms"]:
                    failures.append(f"itl_exceeded:{request_class}")

    stage_rows = score.get("stage_phases") or []
    seen = {(row.get("stage"), row.get("phase")) for row in stage_rows}
    required_pairs = {(stage, phase) for stage in range(3) for phase in ("prefill", "decode")}
    if not required_pairs.issubset(seen):
        failures.append("stage_phase_coverage")
    for row in stage_rows:
        values = row.get("metrics") or {}
        if set(values) != set(required["stage_phase_fields"]):
            failures.append("stage_phase_fields")
            break
        numeric = set(required["stage_phase_fields"]) - {"blocked_reasons"}
        if any(not finite_number(values[key]) for key in numeric) or not isinstance(values["blocked_reasons"], dict):
            failures.append("stage_phase_values")
            break

    hosts = score.get("hosts") or []
    if len(hosts) != 3 or len({row.get("host") for row in hosts}) != 3:
        failures.append("host_coverage")
    for row in hosts:
        values = row.get("metrics") or {}
        if set(values) != set(required["host_fields"]):
            failures.append("host_fields")
            break
        if not isinstance(values.get("samples"), int) or values["samples"] <= 0:
            failures.append("gpu_samples_missing")
        coverage = values.get("coverage")
        if not finite_number(coverage) or coverage < required["gpu_minimum_coverage"]:
            failures.append("gpu_coverage")
        for key in ("util_mean", "util_p50", "util_p90", "zero_fraction"):
            value = values.get(key)
            if not finite_number(value) or value < 0 or value > 100:
                failures.append("gpu_util_value")
                break
        if not finite_number(values.get("memory_peak")) or values["memory_peak"] <= 0:
            failures.append("gpu_memory_value")
        for optional in ("power", "temperature"):
            if values.get(optional) is None and optional not in (values.get("unavailable_reasons") or {}):
                failures.append("telemetry_unavailable_reason")
    if score.get("resource_snapshots") != required["resource_snapshots"]:
        failures.append("resource_snapshot_coverage")
    return sorted(set(failures))


def cleanup_failures(result: dict, spec: dict) -> list[str]:
    actual = result.get("cleanup") or {}
    expected = spec["cleanup"]
    failures = []
    for key in ("node_unload_once_per_node", "unload_status", "unload_resource_state", "nodes",
                "task_native_children", "task_listeners", "gpu_compute_processes"):
        if actual.get(key) != expected[key]:
            failures.append(f"cleanup:{key}")
    if actual.get("transport_failure_policy") != expected["transport_failure_policy"]:
        failures.append("cleanup:transport_failure_policy")
    if actual.get("transport_failures", 0) != 0 and actual.get("transport_failures_reconciled") is not True:
        failures.append("cleanup:unreconciled_transport_failure")
    return failures


def distributed_failures(result: dict, spec: dict) -> list[str]:
    actual = result.get("distributed") or {}
    expected = spec["required_distributed_evidence"]
    failures = []
    for key, value in expected.items():
        if key == "cross_host_transfer_bytes_positive":
            if value is True and (not finite_number(actual.get("cross_host_transfer_bytes"))
                                  or actual["cross_host_transfer_bytes"] <= 0):
                failures.append("distributed:cross_host_transfer")
        elif actual.get(key) != value:
            failures.append(f"distributed:{key}")
    return failures


def arm_failures(result: dict, arm: dict, spec: dict) -> list[str]:
    failures = []
    arm_id = arm["id"]
    configured = expected_requests(arm)
    counts = result.get("counts") or {}
    if counts.get("configured") != configured or counts.get("classified") != configured:
        failures.append("request_classification")
    if result.get("error") is not None:
        failures.append("runtime_error")
    if result.get("evidence_missing") is not None:
        failures.append("evidence_missing")
    if result.get("cleanup_error") is not None:
        failures.append("cleanup_error")
    if result.get("passed") is not True:
        failures.append("arm_not_passed")

    if arm["kind"] == "normal" or arm.get("requires_oracle_eos_release") is True:
        if any(counts.get(key) != configured for key in ("delivered", "completed", "released",
                                                          "oracle_passed", "eos")):
            failures.append("normal_count")
        if any(counts.get(key, 0) != 0 for key in ("rejected", "failed", "canceled", "uncertain",
                                                   "unsubmitted", "incomplete", "unreleased")):
            failures.append("normal_remainder")
    if arm_id == "I2-COLD8" or arm.get("requires_overlap") is True:
        if result.get("overlap_proved") is not True:
            failures.append("overlap_missing")
    if arm.get("requires_backlog_convergence") is True and result.get("backlog_converged") is not True:
        failures.append("backlog_not_converged")
    if arm.get("requires_zero_between_blocks") is True:
        zeros = result.get("zero_between_blocks")
        if zeros != [True] * (arm["blocks"] - 1):
            failures.append("between_block_residue")

    if arm_id == "I3-OVERLOAD80":
        accepted = counts.get("accepted")
        rejected = counts.get("rejected")
        if not isinstance(accepted, int) or accepted > arm["accepted_max"]:
            failures.append("overload_over_admit")
        if not isinstance(rejected, int) or rejected < arm["rejected_min"]:
            failures.append("overload_rejection_shortfall")
        if isinstance(accepted, int) and any(counts.get(key) != accepted for key in
                                             ("completed", "released", "oracle_passed", "eos")):
            failures.append("overload_accepted_not_complete")
        if result.get("rejection_no_effect") is not True:
            failures.append("rejection_side_effect")
    if arm_id == "I3-CANCEL":
        if counts.get("canceled") != arm["targets"] or counts.get("completed") != configured - arm["targets"]:
            failures.append("cancel_outcome")
        if counts.get("released") != configured or result.get("no_post_linearization_output") is not True:
            failures.append("cancel_linearization")
    if arm_id == "I3-DISCONNECT":
        target = result.get("target_terminal")
        if target not in arm["allowed_target_terminals"] or result.get("failure_ledger_preserved") is not True:
            failures.append("disconnect_ownership")
    if arm.get("requires_generation_fence") is True and result.get("generation_fence_proved") is not True:
        failures.append("generation_fence")
    if arm.get("requires_rejection_no_effect") is True and result.get("rejection_no_effect") is not True:
        failures.append("rejection_side_effect")

    failures.extend(scorecard_failures(result, arm, spec))
    failures.extend(distributed_failures(result, spec))
    failures.extend(cleanup_failures(result, spec))
    return sorted(set(failures))


def evaluate(bundle: dict, spec: dict) -> dict:
    validator = load_validator()
    validator.validate(spec)
    global_failures = []
    if bundle.get("schema") != 1 or bundle.get("spec_id") != spec["spec_id"]:
        global_failures.append("bundle_identity")
    if bundle.get("runtime_commit") != spec["source"]["runtime_commit"]:
        global_failures.append("runtime_identity")
    if bundle.get("performance_improvement_claimed") is not False:
        global_failures.append("premature_performance_claim")
    expected = {arm["id"]: arm for arm in spec["arms"]}
    results = bundle.get("arms") or []
    if len(results) != len(expected) or len({row.get("id") for row in results}) != len(results):
        global_failures.append("arm_coverage")
    rows = []
    for arm_id, arm in expected.items():
        matches = [row for row in results if row.get("id") == arm_id]
        failures = ["arm_missing"] if len(matches) != 1 else arm_failures(matches[0], arm, spec)
        rows.append({"id": arm_id, "passed": not failures, "failures": failures})
    passed = not global_failures and all(row["passed"] for row in rows)
    if bundle.get("integrity_baseline") is not passed:
        global_failures.append("integrity_verdict_mismatch")
        passed = False
    return {"schema": 1, "passed": passed, "integrity_baseline": passed,
            "performance_improvement_claimed": False, "global_failures": global_failures,
            "arms": rows}


def scorecard_fixture(configured: int, spec: dict) -> dict:
    stage_metrics = {key: 1 for key in spec["required_scorecard"]["stage_phase_fields"]}
    stage_metrics["blocked_reasons"] = {}
    host_metrics = {key: 1 for key in spec["required_scorecard"]["host_fields"]}
    host_metrics.update({"coverage": 1.0, "util_mean": 50.0, "util_p50": 50.0,
                         "util_p90": 70.0, "zero_fraction": 0.0,
                         "power": None, "temperature": None,
                         "unavailable_reasons": {"power": "unsupported", "temperature": "unsupported"}})
    run = {key: 1 for key in spec["required_scorecard"]["run_fields"]}
    run["deadline_violations"] = 0
    run["send_slip_violations"] = 0
    run["ttft"] = {name: {"p95_ms": 1} for name in ("short", "medium", "long")}
    run["itl"] = {name: {"p95_ms": 1} for name in ("short", "medium", "long")}
    return {
        "requests_with_all_fields": configured,
        "request_field_names": spec["required_scorecard"]["request_fields"], "run": run,
        "stage_phases": [{"stage": stage, "phase": phase, "metrics": copy.deepcopy(stage_metrics)}
                         for stage in range(3) for phase in ("prefill", "decode")],
        "hosts": [{"host": f"h{host}", "metrics": copy.deepcopy(host_metrics)} for host in range(3)],
        "resource_snapshots": spec["required_scorecard"]["resource_snapshots"],
    }


def fixture(spec: dict) -> dict:
    results = []
    for arm in spec["arms"]:
        configured = expected_requests(arm)
        counts = {"configured": configured, "classified": configured, "delivered": configured,
                  "completed": configured, "released": configured, "oracle_passed": configured,
                  "eos": configured, "accepted": configured, "rejected": 0, "failed": 0,
                  "canceled": 0, "uncertain": 0, "unsubmitted": 0, "incomplete": 0,
                  "unreleased": 0}
        row = {"id": arm["id"], "passed": True, "counts": counts, "error": None,
               "evidence_missing": None, "cleanup_error": None,
               "scorecard": scorecard_fixture(configured, spec),
               "distributed": {"physical_hosts": 3, "stages": 3,
                               "all_hosts_own_model_shard": True, "all_hosts_own_kv": True,
                               "all_stages_compute": True, "cross_host_transfer_bytes": 1,
                               "undeclared_hosts": 0},
               "cleanup": {"node_unload_once_per_node": True, "unload_status": "succeeded",
                           "unload_resource_state": "absent", "nodes": 0,
                           "task_native_children": 0, "task_listeners": 0,
                           "gpu_compute_processes": 0,
                           "transport_failure_policy": "zero_or_preserved_with_reconciliation",
                           "transport_failures": 0, "transport_failures_reconciled": True}}
        if arm["id"] == "I0-S":
            row["scorecard"]["run"]["ttft"] = {"short": {"p95_ms": 1}}
            row["scorecard"]["run"]["itl"] = {"short": {"p95_ms": 1}}
        elif arm["id"] == "I0-M":
            row["scorecard"]["run"]["ttft"] = {"medium": {"p95_ms": 1}}
            row["scorecard"]["run"]["itl"] = {"medium": {"p95_ms": 1}}
        elif arm["id"] == "I0-L":
            row["scorecard"]["run"]["ttft"] = {"long": {"p95_ms": 1}}
            row["scorecard"]["run"]["itl"] = {"long": {"p95_ms": 1}}
        if arm.get("requires_overlap"):
            row["overlap_proved"] = True
        if arm.get("requires_backlog_convergence"):
            row["backlog_converged"] = True
        if arm.get("requires_zero_between_blocks"):
            row["zero_between_blocks"] = [True] * (arm["blocks"] - 1)
        if arm["id"] == "I3-OVERLOAD80":
            counts.update(accepted=72, rejected=8, delivered=72, completed=72, released=72,
                          oracle_passed=72, eos=72)
            row["rejection_no_effect"] = True
        if arm["id"] == "I3-CANCEL":
            counts.update(completed=6, oracle_passed=6, eos=6, canceled=2, released=8)
            row["no_post_linearization_output"] = True
        if arm["id"] == "I3-DISCONNECT":
            counts.update(completed=7, oracle_passed=7, eos=7, failed=1, released=8)
            row.update(target_terminal="failed", failure_ledger_preserved=True)
        if arm["id"] == "I3-RESTART":
            counts.update(completed=7, oracle_passed=7, eos=7, failed=1, released=8)
            row["generation_fence_proved"] = True
        if arm["id"] == "I3-LATE":
            row.update(generation_fence_proved=True, rejection_no_effect=True)
        results.append(row)
    return {"schema": 1, "spec_id": spec["spec_id"],
            "runtime_commit": spec["source"]["runtime_commit"],
            "integrity_baseline": True, "performance_improvement_claimed": False, "arms": results}


def self_test(spec: dict) -> None:
    baseline = fixture(spec)
    assert evaluate(baseline, spec)["passed"]
    mutations = (
        lambda value: value["arms"].pop(),
        lambda value: value["arms"][0].update(passed=False),
        lambda value: value["arms"][0]["counts"].update(oracle_passed=0),
        lambda value: value["arms"][0].update(error="boom"),
        lambda value: value["arms"][0]["scorecard"].update(requests_with_all_fields=0),
        lambda value: value["arms"][0]["scorecard"].update(request_field_names=[]),
        lambda value: value["arms"][0]["scorecard"]["run"].pop("useful_generation_tps"),
        lambda value: value["arms"][0]["scorecard"]["run"].update(deadline_violations=1),
        lambda value: value["arms"][0]["scorecard"]["run"]["ttft"]["short"].update(p95_ms=60001),
        lambda value: value["arms"][0]["scorecard"]["run"]["itl"]["short"].update(p95_ms=251),
        lambda value: value["arms"][0]["scorecard"]["stage_phases"].pop(),
        lambda value: value["arms"][0]["scorecard"]["hosts"][0]["metrics"].update(coverage=0.94),
        lambda value: value["arms"][0]["scorecard"].update(resource_snapshots=[]),
        lambda value: value["arms"][4].update(overlap_proved=False),
        lambda value: value["arms"][5].update(backlog_converged=False),
        lambda value: value["arms"][7]["counts"].update(rejected=7, classified=79),
        lambda value: value["arms"][7].update(rejection_no_effect=False),
        lambda value: value["arms"][8].update(no_post_linearization_output=False),
        lambda value: value["arms"][10].update(failure_ledger_preserved=False),
        lambda value: value["arms"][11].update(generation_fence_proved=False),
        lambda value: value["arms"][0]["distributed"].update(all_stages_compute=False),
        lambda value: value["arms"][0]["distributed"].update(cross_host_transfer_bytes=0),
        lambda value: value["arms"][0]["cleanup"].update(nodes=1),
        lambda value: value["arms"][0]["cleanup"].update(transport_failures=1,
                                                           transport_failures_reconciled=False),
    )
    for mutate in mutations:
        changed = copy.deepcopy(baseline)
        mutate(changed)
        changed["integrity_baseline"] = False
        assert not evaluate(changed, spec)["passed"]
    print(json.dumps({"passed": True, "tests": 1 + len(mutations)}, separators=(",", ":")))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--spec", type=Path, required=True)
    parser.add_argument("--bundle", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    spec = json.loads(args.spec.read_text(encoding="utf-8"))
    if args.self_test:
        self_test(spec)
        return
    if args.bundle is None or args.output is None:
        parser.error("--bundle and --output are required")
    result = evaluate(json.loads(args.bundle.read_text(encoding="utf-8")), spec)
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"passed": result["passed"], "integrity_baseline": result["integrity_baseline"],
                      "global_failures": result["global_failures"]}, separators=(",", ":")))
    if not result["passed"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
