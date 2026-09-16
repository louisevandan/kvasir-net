#!/usr/bin/env python3
"""Fail-closed validator for the Release A integrity-first test contract."""
from __future__ import annotations

import argparse
import copy
import json
import re
from pathlib import Path


DIRECTORY = Path(__file__).resolve().parent
CANONICAL = DIRECTORY / "integrity-test-spec-qwen122b-i0-v1.json"
HEX64 = re.compile(r"^[0-9a-f]{64}$")
HEX40 = re.compile(r"^[0-9a-f]{40}$")

ARM_IDS = (
    "I0-S", "I0-M", "I0-L", "I1-Q64", "I2-COLD8", "I2-WAVE64",
    "I2-RECOVERY24", "I3-OVERLOAD80", "I3-CANCEL", "I3-SLOW",
    "I3-DISCONNECT", "I3-RESTART", "I3-LATE", "I4-SOAK",
)
REQUEST_FIELDS = {
    "eligible_ms", "send_started_ms", "send_completed_ms", "arrival_ms", "first_output_ms",
    "output_received_ms", "terminal_ms", "release_ms", "class", "deadline_ms", "input_tokens",
    "generated_tokens", "stop_reason", "oracle_passed", "queue_ms",
    "prefill_rows", "prefill_ms", "decode_rows", "decode_ms",
}
RUN_FIELDS = {
    "useful_generation_tps", "total_generation_tps", "prefill_rows_per_second",
    "decode_rows_per_second", "ttft", "itl", "first_scheduled_send_ms",
    "last_terminal_ms", "deadline_violations", "send_slip_violations",
}
SCHEDULER_PHASE_FIELDS = {
    "physical_batches", "rows_mean", "rows_p50", "rows_p95", "rows_max",
    "full_ubatch_fraction", "mixed_batches", "runnable", "eligible", "blocked",
    "pending", "blocked_reasons", "flight_peak", "open_peak", "idle_ms",
}
STAGE_FIELDS = {
    "spans", "executions", "rows", "queue_ms", "compute_ms", "publish_ms",
    "compute_p50_ms", "compute_p95_ms", "open_peak", "overlap_ms",
}
HOST_FIELDS = {
    "samples", "coverage", "util_mean", "util_p50", "util_p90", "zero_fraction",
    "memory_peak", "power", "temperature", "unavailable_reasons",
}
SNAPSHOT_HOST_FIELDS = {
    "host", "captured_unix_ms", "node_count", "task_native_children",
    "task_listeners", "model_resident",
}
SNAPSHOT_STATES = {
    "before_load": {"node_count": 0, "task_native_children": 0,
                    "task_listeners": 1, "model_resident": False},
    "peak": {"node_count": 1, "task_native_children": 1,
             "task_listeners": 2, "model_resident": True},
    "after_drain": {"node_count": 1, "task_native_children": 1,
                    "task_listeners": 2, "model_resident": True},
    "after_unload": {"node_count": 0, "task_native_children": 0,
                     "task_listeners": 1, "model_resident": False},
}


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def arms_by_id(spec: dict) -> dict[str, dict]:
    arms = spec.get("arms")
    require(isinstance(arms, list), "arms must be a list")
    ids = [arm.get("id") for arm in arms]
    require(tuple(ids) == ARM_IDS, "arm order or identity differs")
    require(len(set(ids)) == len(ids), "arm ids are not unique")
    return {arm["id"]: arm for arm in arms}


def validate(spec: dict) -> dict:
    require(spec.get("schema") == 1, "schema differs")
    require(spec.get("spec_id") == "release-a-qwen122b-integrity-i0-v1", "spec id differs")
    require(spec.get("status") == "planned", "unexecuted contract must remain planned")
    require(spec.get("integrity_baseline") is False, "contract claims integrity before execution")
    require(spec.get("performance_improvement_claimed") is False,
            "integrity contract claims performance improvement")
    require(spec.get("determinism") == {
        "product_algorithm": "deterministic_state_machine",
        "randomized_recovery": False, "runtime_retry_discovery": False,
        "test_role": "proof_only", "stress_schedule": "fixed", "seed": 20260916,
        "replay_requires_identical_terminal_classification": True,
    }, "deterministic execution contract differs")

    source = spec.get("source") or {}
    require(HEX40.fullmatch(str(source.get("runtime_commit"))) is not None,
            "runtime commit is not a full hash")
    require(HEX64.fullmatch(str(source.get("base_h0_spec_sha256"))) is not None,
            "base H0 spec digest is invalid")
    require(HEX64.fullmatch(str(source.get("corpus_sha256"))) is not None,
            "corpus digest is invalid")

    model = spec.get("model") or {}
    require(model == {
        "id": "Qwen3.5-122B-A10B", "variant": "UD-Q5_K_S", "physical_hosts": 3,
        "cuts": [[0, 24], [24, 36], [36, 48]], "resident": 8,
        "context_per_sequence": 102400, "total_context": 819200,
        "n_batch": 128, "n_ubatch": 64, "max_output_tokens": 2048,
    }, "model or execution shape differs")

    slo = spec.get("slo") or {}
    require(slo == {
        "request_deadline_ms": {"short": 600000, "medium": 1200000, "long": 1800000},
        "ttft_p95_ms": {"short": 60000, "medium": 300000, "long": 900000},
        "itl_p95_ms": 250, "send_slip_ms": 1000, "clock_error_ms": 10,
        "normal_response_pass_rate": 1.0, "normal_terminal_pass_rate": 1.0,
    }, "SLO contract differs")

    arms = arms_by_id(spec)
    singles = (("I0-S", "case-00", 900000), ("I0-M", "case-04", 1500000),
               ("I0-L", "case-06", 2100000))
    for arm_id, case_id, timeout in singles:
        arm = arms[arm_id]
        require(arm.get("kind") == "normal" and arm.get("selection") == [case_id],
                f"{arm_id} selection differs")
        require(arm.get("waves") == [{"after_ms": 0, "count": 1}]
                and arm.get("max_in_flight") == 1 and arm.get("timeout_ms") == timeout,
                f"{arm_id} schedule differs")
        require(arm.get("same_load_group") == "I0"
                and arm.get("requires_oracle_eos_release") is True,
                f"{arm_id} acceptance differs")

    group = (spec.get("execution_groups") or {}).get("I0")
    require(group == {
        "arms": ["I0-S", "I0-M", "I0-L"], "same_load": True,
        "selection": ["case-00", "case-04", "case-06"],
        "classes": ["short", "medium", "long"], "requests": 3,
        "waves": [{"after_ms": 0, "count": 3}],
        "submission": "release_closed_loop", "max_in_flight": 1,
        "request_deadline_ms": [600000, 1200000, 1800000],
        "overall_grace_ms": 300000, "timeout_ms": 3900000,
        "load_count": 1, "unload_count": 1,
    }, "I0 same-load execution group differs")

    q64 = arms["I1-Q64"]
    deadlines = slo["request_deadline_ms"]
    expected_q64_timeout = 32 * deadlines["short"] + 16 * deadlines["medium"] \
        + 16 * deadlines["long"] + 300000
    require(q64.get("requests") == 64 and q64.get("max_in_flight") == 1
            and q64.get("submission") == "release_closed_loop"
            and q64.get("timeout_ms") == expected_q64_timeout and q64.get("same_load") is True,
            "I1-Q64 closed-loop contract differs")

    cold = arms["I2-COLD8"]
    require(cold.get("requests") == 8 and cold.get("waves") == [{"after_ms": 0, "count": 8}]
            and cold.get("open_loop") is True and cold.get("requires_overlap") is True,
            "I2-COLD8 differs")
    wave = arms["I2-WAVE64"]
    require(wave.get("requests") == 64 and sum(row.get("count", 0) for row in wave.get("waves", [])) == 64,
            "I2-WAVE64 count differs")
    require([row.get("after_ms") for row in wave["waves"]]
            == [0, 180000, 480000, 780000, 1080000, 1380000, 1680000, 1980000],
            "I2-WAVE64 arrival schedule differs")
    require(wave.get("open_loop") is True and wave.get("requires_overlap") is True
            and wave.get("requires_backlog_convergence") is True and wave.get("timeout_ms") == 3780000,
            "I2-WAVE64 convergence contract differs")
    recovery = arms["I2-RECOVERY24"]
    require(recovery.get("requests") == 24 and recovery.get("blocks") == 3
            and recovery.get("requests_per_block") == 8 and recovery.get("quiescence_ms") == 30000
            and recovery.get("same_load") is True and recovery.get("requires_zero_between_blocks") is True,
            "I2-RECOVERY24 differs")

    overload = arms["I3-OVERLOAD80"]
    require(overload.get("requests") == 80 and overload.get("accepted_max") == 72
            and overload.get("rejected_min") == 8 and overload.get("submit_window_ms") == 1000
            and overload.get("rejection_deadline_ms") == 5000
            and overload.get("request_id_pattern") == "integrity-overload-{index:03d}"
            and overload.get("requires_rejection_no_effect") is True
            and overload.get("requires_all_classified") is True,
            "I3-OVERLOAD80 differs")
    require(arms["I3-CANCEL"].get("targets") == 2
            and arms["I3-CANCEL"].get("selection") == "corpus_0_7"
            and arms["I3-CANCEL"].get("target_request_indexes") == [1, 6]
            and arms["I3-CANCEL"].get("trigger_output_ordinal") == 0
            and arms["I3-CANCEL"].get("requires_no_post_linearization_output") is True,
            "I3-CANCEL differs")
    require(arms["I3-SLOW"].get("edge_delay_ms") == 1000
            and arms["I3-SLOW"].get("selection") == "corpus_0_7"
            and arms["I3-SLOW"].get("affected_request_indexes") == list(range(8))
            and arms["I3-SLOW"].get("injection_boundary") == "outer_output_delivery"
            and arms["I3-SLOW"].get("requires_oracle_eos_release") is True,
            "I3-SLOW differs")
    require(arms["I3-DISCONNECT"].get("allowed_target_terminals")
            == ["failed", "canceled", "uncertain"]
            and arms["I3-DISCONNECT"].get("selection") == "corpus_0_7"
            and arms["I3-DISCONNECT"].get("target_request_indexes") == [2]
            and arms["I3-DISCONNECT"].get("trigger_output_ordinal") == 0
            and arms["I3-DISCONNECT"].get("requires_failure_ledger") is True,
            "I3-DISCONNECT differs")
    require(arms["I3-RESTART"].get("selection") == "corpus_0_7"
            and arms["I3-RESTART"].get("target_request_indexes") == [4]
            and arms["I3-RESTART"].get("restart_stage") == "mac20-1"
            and arms["I3-RESTART"].get("trigger_output_ordinal") == 0
            and arms["I3-RESTART"].get("generation_increment") == 1
            and arms["I3-RESTART"].get("requires_generation_fence") is True
            and arms["I3-RESTART"].get("requires_all_classified") is True,
            "I3-RESTART differs")
    require(arms["I3-LATE"].get("delay_ms") == 30000
            and arms["I3-LATE"].get("selection") == "corpus_0_7"
            and arms["I3-LATE"].get("target_request_indexes") == [5]
            and arms["I3-LATE"].get("return_stage") == "mac20-1"
            and arms["I3-LATE"].get("source_generation_offset") == -1
            and arms["I3-LATE"].get("requires_generation_fence") is True
            and arms["I3-LATE"].get("requires_rejection_no_effect") is True,
            "I3-LATE rejection differs")

    soak = arms["I4-SOAK"]
    require(soak.get("requests") == 256 and len(soak.get("waves", [])) == 32
            and sum(row.get("count", 0) for row in soak["waves"]) == 256,
            "I4-SOAK count differs")
    require([row.get("after_ms") for row in soak["waves"]] == [index * 180000 for index in range(32)],
            "I4-SOAK cadence differs")
    require(soak.get("minimum_duration_ms") == 5580000 and soak.get("timeout_ms") == 7380000
            and soak.get("same_load") is True and soak.get("requires_backlog_convergence") is True,
            "I4-SOAK duration or convergence differs")
    require(soak.get("request_id_pattern") == "integrity-soak-{index:03d}",
            "I4-SOAK request identity differs")

    score = spec.get("required_scorecard") or {}
    require(set(score.get("request_fields") or []) == REQUEST_FIELDS, "request scorecard differs")
    require(set(score.get("run_fields") or []) == RUN_FIELDS, "run scorecard differs")
    require(set(score.get("scheduler_phase_fields") or []) == SCHEDULER_PHASE_FIELDS,
            "scheduler phase scorecard differs")
    require(set(score.get("stage_fields") or []) == STAGE_FIELDS, "stage scorecard differs")
    require(set(score.get("host_fields") or []) == HOST_FIELDS, "host scorecard differs")
    require(score.get("resource_snapshots") == ["before_load", "peak", "after_drain", "after_unload"],
            "resource snapshots differ")
    require(score.get("gpu_sample_interval_ms") == 1000
            and score.get("gpu_minimum_coverage") == 0.95
            and score.get("gpu_sample_clock_tolerance_ms") == 750,
            "GPU sampling contract differs")
    require(score.get("resource_snapshot_skew_ms") == 5000,
            "resource snapshot skew differs")
    require(set(score.get("resource_snapshot_host_fields") or []) == SNAPSHOT_HOST_FIELDS,
            "resource snapshot fields differ")
    require(score.get("resource_snapshot_states") == SNAPSHOT_STATES,
            "resource snapshot states differ")

    distributed = spec.get("required_distributed_evidence") or {}
    require(distributed == {
        "physical_hosts": 3, "stages": 3, "all_hosts_own_model_shard": True,
        "all_hosts_own_kv": True, "all_stages_compute": True,
        "cross_host_transfer_bytes_positive": True, "undeclared_hosts": 0,
    }, "distributed evidence contract differs")

    cleanup = spec.get("cleanup") or {}
    require(cleanup == {
        "node_unload_once_per_node": True, "unload_status": "succeeded",
        "unload_resource_state": "absent", "nodes": 0, "task_native_children": 0,
        "task_listeners": 0, "gpu_compute_processes": 0,
        "transport_failure_policy": "zero_or_preserved_with_reconciliation",
    }, "cleanup contract differs")
    return {"passed": True, "arms": len(arms), "normal_arms": 8,
            "overload_arms": 1, "fault_arms": 5}


def self_test() -> None:
    canonical = json.loads(CANONICAL.read_text(encoding="utf-8"))
    validate(canonical)
    mutations = (
        lambda value: value["source"].update(runtime_commit="0" * 39),
        lambda value: value["source"].pop("base_h0_spec_sha256"),
        lambda value: value.update(integrity_baseline=True),
        lambda value: value.update(performance_improvement_claimed=True),
        lambda value: value["determinism"].update(runtime_retry_discovery=True),
        lambda value: value["model"].update(resident=9),
        lambda value: value["slo"]["ttft_p95_ms"].update(short=60001),
        lambda value: value["arms"].pop(),
        lambda value: value["arms"][0].update(selection=["case-01"]),
        lambda value: value["execution_groups"]["I0"].update(load_count=3),
        lambda value: value["execution_groups"]["I0"].update(max_in_flight=3),
        lambda value: value["arms"][3].update(max_in_flight=2),
        lambda value: value["arms"][5]["waves"][1].update(after_ms=180001),
        lambda value: value["arms"][7].update(rejected_min=7),
        lambda value: value["arms"][10].update(allowed_target_terminals=["failed"]),
        lambda value: value["arms"][13].update(minimum_duration_ms=5579999),
        lambda value: value["required_scorecard"]["request_fields"].pop(),
        lambda value: value["required_scorecard"]["stage_fields"].pop(),
        lambda value: value["required_scorecard"].update(gpu_minimum_coverage=0.94),
        lambda value: value["required_scorecard"]["resource_snapshot_states"]["peak"].update(node_count=0),
        lambda value: value["required_distributed_evidence"].update(all_stages_compute=False),
        lambda value: value["cleanup"].update(nodes=1),
    )
    for mutate in mutations:
        changed = copy.deepcopy(canonical)
        mutate(changed)
        try:
            validate(changed)
        except ValueError:
            pass
        else:
            raise AssertionError("integrity contract weakening was accepted")
    print(json.dumps({"passed": True, "tests": 1 + len(mutations)}, separators=(",", ":")))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--spec", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return
    if args.spec is None:
        parser.error("--spec is required")
    result = validate(json.loads(args.spec.read_text(encoding="utf-8")))
    print(json.dumps(result, separators=(",", ":")))


if __name__ == "__main__":
    main()
