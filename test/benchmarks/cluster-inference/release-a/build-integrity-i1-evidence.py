#!/usr/bin/env python3
"""Bind I1's long GPU trace and lifecycle snapshots without quadratic scans."""
from __future__ import annotations

import argparse
import bisect
import copy
import hashlib
import importlib.util
import json
from pathlib import Path


DIRECTORY = Path(__file__).resolve().parent
HOSTS = ("spark", "mac20", "mac21")


def load_i0_builder():
    spec = importlib.util.spec_from_file_location("i0_evidence_builder",
                                                  DIRECTORY / "build-integrity-i0-evidence.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def bind_samples(raw: dict, started: int, elapsed: int, score: dict) -> dict:
    interval = score["gpu_sample_interval_ms"]
    tolerance = score["gpu_sample_clock_tolerance_ms"]
    if raw.get("sample_interval_ms") != interval:
        raise ValueError("I1 GPU sample interval differs")
    hosts = raw.get("hosts") or []
    if [host.get("host") for host in hosts] != list(HOSTS):
        raise ValueError("I1 GPU host order differs")
    expected = elapsed // interval + 1
    output = []
    finite = load_i0_builder().finite
    for host in hosts:
        samples = host.get("samples") or []
        times = [row.get("captured_unix_ms") for row in samples]
        if any(not isinstance(value, int) for value in times) or times != sorted(set(times)):
            raise ValueError("I1 GPU sample clocks differ")
        used = set()
        selected = []
        for tick in range(expected):
            target = started + tick * interval
            left = bisect.bisect_left(times, target - tolerance)
            right = bisect.bisect_right(times, target + tolerance)
            candidates = [index for index in range(left, right) if index not in used]
            if not candidates:
                continue
            index = min(candidates, key=lambda item: (abs(times[item] - target), times[item]))
            used.add(index)
            sample = copy.deepcopy(samples[index])
            sample["elapsed_ms"] = tick * interval
            for name, lower, upper in (("utilization_percent", 0, 100),
                                       ("memory_used_bytes", 1, None)):
                value = sample.get(name)
                if not finite(value) or value < lower or (upper is not None and value > upper):
                    raise ValueError(f"I1 raw GPU {name} differs")
            selected.append(sample)
        if len(selected) / expected < score["gpu_minimum_coverage"]:
            raise ValueError("I1 GPU sample coverage is below the contract")
        output.append({"host": host["host"], "samples": selected,
                       "unavailable_reasons": copy.deepcopy(host.get("unavailable_reasons") or {})})
    return {"sample_interval_ms": interval, "hosts": output}


def build(artifact_bytes: bytes, artifact: dict, seal: dict, raw: dict, spec: dict) -> dict:
    if raw.get("schema") != "p4.release-a.integrity-i1-raw.v1":
        raise ValueError("I1 raw evidence schema differs")
    if raw.get("artifact_sha256") != digest(artifact_bytes):
        raise ValueError("I1 raw evidence is not bound to the runtime artifact")
    if raw.get("config_sha256") != seal.get("config_sha256"):
        raise ValueError("I1 raw config binding differs")
    if (artifact.get("request_count") != 64 or len(artifact.get("requests") or []) != 64
            or artifact.get("completed_count") != 64 or artifact.get("released_count") != 64
            or artifact.get("passed") is not True):
        raise ValueError("I1 runtime request coverage differs")
    started, elapsed = artifact.get("started_unix_ms"), artifact.get("elapsed_ms")
    if not isinstance(started, int) or started <= 0 or not isinstance(elapsed, int) or elapsed <= 0:
        raise ValueError("I1 runtime inference window is absent")
    distributed = raw.get("distributed") or {}
    required = spec["required_distributed_evidence"]
    if any(distributed.get(key) != value for key, value in required.items()
           if key != "cross_host_transfer_bytes_positive"):
        raise ValueError("I1 distributed topology differs")
    if not isinstance(distributed.get("cross_host_transfer_bytes"), int) \
            or distributed["cross_host_transfer_bytes"] <= 0:
        raise ValueError("I1 cross-host transfer is absent")
    cleanup = raw.get("cleanup") or {}
    for key, value in spec["cleanup"].items():
        if cleanup.get(key) != value:
            raise ValueError(f"I1 cleanup differs: {key}")
    failures = cleanup.get("transport_failures")
    if (not isinstance(failures, int) or failures < 0
            or (failures > 0 and cleanup.get("transport_failures_reconciled") is not True)):
        raise ValueError("I1 transport failures are not reconciled")
    score = spec["required_scorecard"]
    i0 = load_i0_builder()
    return {
        "schema": "p4.release-a.integrity-i1-external.v1",
        "artifact_sha256": digest(artifact_bytes), "config_sha256": seal["config_sha256"],
        "run_window": {"started_unix_ms": started, "elapsed_ms": elapsed},
        "gpu": bind_samples(raw["gpu"], started, elapsed, score),
        "resource_snapshots": i0.bind_snapshots(raw, started, elapsed, score),
        "distributed": copy.deepcopy(distributed), "cleanup": copy.deepcopy(cleanup),
    }


def self_test(spec: dict) -> None:
    i0 = load_i0_builder()
    artifact, _, seal, raw = i0.fixture(spec)
    artifact.update(request_count=64, completed_count=64, released_count=64,
                    passed=True, requests=[{} for _ in range(64)])
    artifact_bytes = (json.dumps(artifact, separators=(",", ":")) + "\n").encode()
    raw.update(schema="p4.release-a.integrity-i1-raw.v1",
               artifact_sha256=digest(artifact_bytes))
    result = build(artifact_bytes, artifact, seal, raw, spec)
    assert result["schema"] == "p4.release-a.integrity-i1-external.v1"
    assert [len(host["samples"]) for host in result["gpu"]["hosts"]] == [3, 3, 3]
    for mutate in (
            lambda a, r: r.update(artifact_sha256="0" * 64),
            lambda a, r: r["gpu"]["hosts"][0]["samples"].pop(),
            lambda a, r: a.update(request_count=63),
            lambda a, r: r.update(config_sha256="0" * 64),
    ):
        changed_artifact, changed_raw = copy.deepcopy(artifact), copy.deepcopy(raw)
        mutate(changed_artifact, changed_raw)
        changed_bytes = (json.dumps(changed_artifact, separators=(",", ":")) + "\n").encode()
        if changed_raw["artifact_sha256"] == digest(artifact_bytes):
            changed_raw["artifact_sha256"] = digest(changed_bytes)
        try:
            build(changed_bytes, changed_artifact, seal, changed_raw, spec)
        except (ValueError, KeyError, TypeError):
            pass
        else:
            raise AssertionError("weakened I1 raw evidence was accepted")
    print(json.dumps({"passed": True, "tests": 5}, separators=(",", ":")))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--spec", type=Path, required=True)
    parser.add_argument("--artifact", type=Path)
    parser.add_argument("--seal", type=Path)
    parser.add_argument("--raw", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    spec = json.loads(args.spec.read_text())
    if args.self_test:
        self_test(spec); return
    if any(value is None for value in (args.artifact, args.seal, args.raw, args.output)):
        parser.error("artifact, seal, raw and output are required")
    artifact_bytes = args.artifact.read_bytes()
    result = build(artifact_bytes, json.loads(artifact_bytes),
                   json.loads(args.seal.read_text()), json.loads(args.raw.read_text()), spec)
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({"passed": True, "output": str(args.output)}, separators=(",", ":")))


if __name__ == "__main__":
    main()
