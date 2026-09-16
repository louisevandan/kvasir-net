#!/usr/bin/env python3
"""Exercise the complete I1 evidence and judgment path with 64 fixed requests."""
from __future__ import annotations

import copy
import hashlib
import importlib.util
import json
from pathlib import Path


DIRECTORY = Path(__file__).resolve().parent
SPEC = DIRECTORY / "integrity-test-spec-qwen122b-i0-v2.json"
SOURCE = (
    "[R00002] Station 7; revision 6. Measured RMS current: 28 A. "
    "Isolated conductor resistance: 52 milliohms. Operating duration: 10 hours. "
    "Inlet pressure: 121 kPa. Pressure alarm threshold: 120 kPa; "
    "equality is not an exceedance. Temperature was not measured."
)
TASK = (
    'Connect the source facts for R00002, in that order. '
    'Compute power in milliwatts as current_A squared times resistance_milliohms, '
    'and energy in milliwatt-hours as power_mW times duration_hours. '
    'Return JSON with keys "rows" and "temperature_measured". Each row must contain '
    '"id", "revision", "power_mW", "energy_mWh", and boolean "pressure_alarm". '
    '"temperature_measured" must state whether those records contain a measured temperature. '
    'Use integer arithmetic; do not infer a temperature or a pressure/heat causal relation.'
)


def load(name: str, filename: str):
    module_spec = importlib.util.spec_from_file_location(name, DIRECTORY / filename)
    module = importlib.util.module_from_spec(module_spec)
    module_spec.loader.exec_module(module)
    return module


def encode(value: dict) -> bytes:
    return (json.dumps(value, separators=(",", ":")) + "\n").encode()


def fixture(spec: dict) -> tuple[dict, dict, dict]:
    requests, cases, observations, spans = [], [], [], []
    model = {"rows": [{"id": "R00002", "revision": 6, "power_mW": 1,
                       "energy_mWh": 2, "pressure_alarm": False}],
             "temperature_measured": False}
    expected = {"rows": [{"id": "R00002", "revision": 6, "power_mW": 40768,
                          "energy_mWh": 407680, "pressure_alarm": True}],
                "temperature_measured": False}
    for index in range(64):
        class_name = "short" if index < 32 else "medium" if index < 48 else "long"
        case_id = f"case-{index:02d}"
        request_id = f"i1-{index:02d}"
        prompt = (f"short-{index}" if class_name == "short" else
                  f"<|im_start|>user\nCase {index + 1}: 1 archived records.\n{SOURCE}\n\n{TASK}<|im_end|>")
        answer = {"answer": index} if class_name == "short" else expected
        raw = json.dumps(answer if class_name == "short" else model, separators=(",", ":"))
        first, last = raw[:len(raw) // 2], raw[len(raw) // 2:]
        arrival = index * 10
        processor = None if class_name == "short" else "engineering_power_v1"
        requests.append({
            "request_id": request_id, "prompt": prompt, "response": json.dumps(answer, separators=(",", ":")),
            "model_response": raw if processor else None, "response_processor": processor,
            "service_error": None, "service_completed_ms": arrival + 3 if processor else None,
            "outcomes": [{"text": first, "stop": "continue"}, {"text": last, "stop": "eos"}],
            "submission": "delivered", "submission_authority": {"deadline": 1_000_000},
            "released": True, "eligible_ms": arrival, "send_started_ms": arrival,
            "send_completed_ms": arrival, "arrival_ms": arrival, "first_output_ms": arrival + 1,
            "output_received_ms": [arrival + 1, arrival + 2], "completed_ms": arrival + 2,
            "release_ms": arrival + 4, "prefill_rows": 64, "prefill_elapsed_ms": 1,
            "decode_rows": 1, "generation_elapsed_ms": 1,
        })
        cases.append({"id": case_id, "class": class_name,
                      "prompt_sha256": hashlib.sha256(prompt.encode()).hexdigest(),
                      "expected": answer, "input_tokens": 64,
                      "request_timeout_ms": spec["slo"]["request_deadline_ms"][class_name]})
        for ordinal, phase, rows in ((0, "prefill", 64), (1, "decode", 1)):
            execution = index * 2 + ordinal + 1
            physical = {"execution_id": execution, "prefill_rows": rows if phase == "prefill" else 0,
                        "decode_rows": rows if phase == "decode" else 0,
                        "owned_requests": [{"request_id": request_id}]}
            observations.append({"physical_batches": [physical], "ready_sequences": 1,
                                 "idle_ms": 0, "idle_gated": 0,
                                 "scheduling": {"eligible_prefill": int(phase == "prefill"),
                                                "eligible_decode": int(phase == "decode"),
                                                "blocked_outstanding": 0, "pending_admission": 0,
                                                "no_ready_input": 0, "open_batches_before_issue": 0}})
            for stage in range(3):
                start = 1_000_000 + arrival + ordinal * 4 + stage
                spans.append({"node": stage, "execution_ids": [execution], "rows": rows,
                              "ingress_unix_ms": start, "start_unix_ms": start,
                              "end_unix_ms": start + 1, "forward_unix_ms": start + 1})
    artifact = {
        "passed": True, "stage_builds": [{}, {}, {}], "request_count": 64,
        "completed_count": 64, "released_count": 64, "requests": requests,
        "batch_observations": observations, "stage_spans": spans,
        "started_unix_ms": 1_000_000, "elapsed_ms": 650,
        "error": None, "evidence_missing": None, "cleanup_error": None,
        "submissions": {"configured": 64, "delivered": 64, "uncertain": 0,
                        "unsubmitted": 0, "incomplete": 0, "unreleased": 0},
    }
    seal = {"schema": 3, "requests": 64, "config_sha256": "c" * 64,
            "slo": {"percentile": "nearest_rank",
                    "ttft_ms_by_class": spec["slo"]["ttft_p95_ms"],
                    "itl_p95_ms": spec["slo"]["itl_p95_ms"]},
            "cases": cases}
    states = spec["required_scorecard"]["resource_snapshot_states"]
    snapshot_times = (999_900, 1_000_100, 1_000_660, 1_000_670)
    snapshots = [{"name": name, "hosts": [
        {"host": host, "captured_unix_ms": time, **states[name]}
        for host in ("spark", "mac20", "mac21")]}
        for name, time in zip(spec["required_scorecard"]["resource_snapshots"], snapshot_times)]
    raw = {
        "schema": "p4.release-a.integrity-i1-raw.v1", "artifact_sha256": hashlib.sha256(encode(artifact)).hexdigest(),
        "config_sha256": seal["config_sha256"],
        "gpu": {"sample_interval_ms": 1000, "hosts": [
            {"host": host, "samples": [{"captured_unix_ms": 1_000_000,
                                         "utilization_percent": 50, "memory_used_bytes": 1024,
                                         "power_w": None, "temperature_c": None}],
             "unavailable_reasons": {"power": "unsupported", "temperature": "unsupported"}}
            for host in ("spark", "mac20", "mac21")]},
        "resource_snapshots": snapshots,
        "distributed": {"physical_hosts": 3, "stages": 3, "all_hosts_own_model_shard": True,
                        "all_hosts_own_kv": True, "all_stages_compute": True,
                        "cross_host_transfer_bytes": 1, "undeclared_hosts": 0},
        "cleanup": {**spec["cleanup"], "transport_failures": 0,
                    "transport_failures_reconciled": True},
    }
    return artifact, seal, raw


def main() -> None:
    spec = json.loads(SPEC.read_text())
    artifact, seal, raw = fixture(spec)
    builder = load("i1_full_builder", "build-integrity-i1-evidence.py")
    judge = load("i1_full_judge", "judge-integrity-i1.py")
    external = builder.build(encode(artifact), artifact, seal, raw, spec)
    bundle = judge.build_bundle(artifact, encode(artifact), seal, external, spec)
    arm = bundle["arms"][0]
    assert arm["id"] == "I1-Q64" and arm["counts"]["released"] == 64
    assert len(arm["requests"]) == 64 and arm["scorecard"]["run"]["send_slip_violations"] == 0
    useful = arm["scorecard"]["run"]["useful_generation_tps"]
    duration = arm["scorecard"]["run"]["last_terminal_ms"] - arm["scorecard"]["run"]["first_scheduled_send_ms"]
    assert round(useful * duration / 1000) == 64  # 32 short requests, two model tokens each.
    for mutation in (
            lambda a, r: a["requests"].pop(),
            lambda a, r: a["requests"][32].update(response="{}"),
            lambda a, r: a["requests"][33].update(response_processor=None),
            lambda a, r: r["gpu"]["hosts"][0]["samples"].clear(),
    ):
        changed_artifact, changed_raw = copy.deepcopy(artifact), copy.deepcopy(raw)
        mutation(changed_artifact, changed_raw)
        changed_bytes = encode(changed_artifact)
        changed_raw["artifact_sha256"] = hashlib.sha256(changed_bytes).hexdigest()
        try:
            changed_external = builder.build(changed_bytes, changed_artifact, seal, changed_raw, spec)
            judge.build_bundle(changed_artifact, changed_bytes, seal, changed_external, spec)
        except (ValueError, KeyError, TypeError):
            pass
        else:
            raise AssertionError("weakened I1 evidence was accepted")
    print(json.dumps({"passed": True, "tests": 5, "requests": 64}, separators=(",", ":")))


if __name__ == "__main__":
    main()
