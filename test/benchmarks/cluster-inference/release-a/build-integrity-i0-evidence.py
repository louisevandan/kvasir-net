#!/usr/bin/env python3
"""Bind raw remote telemetry and lifecycle snapshots to one I0 artifact."""
from __future__ import annotations

import argparse
import copy
import hashlib
import json
import math
from pathlib import Path


HOSTS = ("spark", "mac20", "mac21")


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def finite(value: object) -> bool:
    return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value)


def bind_samples(raw: dict, started: int, elapsed: int, score: dict) -> dict:
    interval = score["gpu_sample_interval_ms"]
    tolerance = score["gpu_sample_clock_tolerance_ms"]
    if raw.get("sample_interval_ms") != interval:
        raise ValueError("raw GPU sample interval differs")
    rows = raw.get("hosts") or []
    if [row.get("host") for row in rows] != list(HOSTS):
        raise ValueError("raw GPU host order differs")
    expected = elapsed // interval + 1
    output = []
    for host in rows:
        samples = host.get("samples") or []
        times = [row.get("captured_unix_ms") for row in samples]
        if any(not isinstance(value, int) for value in times) or times != sorted(set(times)):
            raise ValueError("raw GPU sample clocks are missing, duplicate, or unordered")
        remaining = list(samples)
        selected = []
        for tick in range(expected):
            target = started + tick * interval
            candidates = [(abs(row["captured_unix_ms"] - target), index, row)
                          for index, row in enumerate(remaining)
                          if abs(row["captured_unix_ms"] - target) <= tolerance]
            if not candidates:
                continue
            _, index, row = min(candidates, key=lambda item: (item[0], item[2]["captured_unix_ms"]))
            remaining.pop(index)
            sample = copy.deepcopy(row)
            sample["elapsed_ms"] = tick * interval
            for name, lower, upper in (("utilization_percent", 0, 100),
                                       ("memory_used_bytes", 1, None)):
                value = sample.get(name)
                if not finite(value) or value < lower or (upper is not None and value > upper):
                    raise ValueError(f"raw GPU {name} differs")
            selected.append(sample)
        coverage = min(1.0, len(selected) / expected)
        if coverage < score["gpu_minimum_coverage"]:
            raise ValueError("raw GPU sample coverage is below the contract")
        output.append({"host": host["host"], "samples": selected,
                       "unavailable_reasons": copy.deepcopy(host.get("unavailable_reasons") or {})})
    return {"sample_interval_ms": interval, "hosts": output}


def bind_snapshots(raw: dict, started: int, elapsed: int, score: dict) -> list[dict]:
    snapshots = raw.get("resource_snapshots") or []
    names = [row.get("name") for row in snapshots]
    if names != score["resource_snapshots"]:
        raise ValueError("raw resource snapshot order differs")
    fields = set(score["resource_snapshot_host_fields"])
    end = started + elapsed
    prior = None
    output = []
    for snapshot in snapshots:
        name = snapshot["name"]
        hosts = snapshot.get("hosts") or []
        if [row.get("host") for row in hosts] != list(HOSTS):
            raise ValueError(f"{name} host order differs")
        if any(set(row) != fields for row in hosts):
            raise ValueError(f"{name} host fields differ")
        times = [row["captured_unix_ms"] for row in hosts]
        if any(not isinstance(value, int) or value <= 0 for value in times):
            raise ValueError(f"{name} capture clock differs")
        if max(times) - min(times) > score["resource_snapshot_skew_ms"]:
            raise ValueError(f"{name} capture skew exceeds the contract")
        if prior is not None and min(times) < prior:
            raise ValueError("resource snapshot order differs")
        prior = max(times)
        expected = score["resource_snapshot_states"][name]
        for row in hosts:
            if {key: row[key] for key in expected} != expected:
                raise ValueError(f"{name} resource state differs")
        if name == "before_load" and max(times) > started:
            raise ValueError("before-load snapshot occurred after inference start")
        if name == "peak" and (max(times) < started or min(times) > end):
            raise ValueError("peak snapshot is outside the inference window")
        if name in ("after_drain", "after_unload") and min(times) < end:
            raise ValueError(f"{name} snapshot occurred before inference drain")
        output.append(copy.deepcopy(snapshot))
    return output


def build(artifact_bytes: bytes, artifact: dict, seal: dict, raw: dict, spec: dict) -> dict:
    if raw.get("schema") != "p4.release-a.integrity-i0-raw.v1":
        raise ValueError("raw I0 evidence schema differs")
    if raw.get("artifact_sha256") != digest(artifact_bytes):
        raise ValueError("raw evidence is not bound to the runtime artifact")
    if raw.get("config_sha256") != seal.get("config_sha256"):
        raise ValueError("raw evidence is not bound to the execution config")
    started = artifact.get("started_unix_ms")
    elapsed = artifact.get("elapsed_ms")
    if not isinstance(started, int) or started <= 0 or not isinstance(elapsed, int) or elapsed <= 0:
        raise ValueError("runtime artifact lacks an absolute inference window")
    required = spec["required_distributed_evidence"]
    distributed = raw.get("distributed") or {}
    expected = {key: value for key, value in required.items()
                if key != "cross_host_transfer_bytes_positive"}
    if any(distributed.get(key) != value for key, value in expected.items()):
        raise ValueError("raw distributed evidence differs")
    if not isinstance(distributed.get("cross_host_transfer_bytes"), int) \
            or distributed["cross_host_transfer_bytes"] <= 0:
        raise ValueError("raw cross-host transfer evidence is absent")
    cleanup = raw.get("cleanup") or {}
    contract = spec["cleanup"]
    for key, value in contract.items():
        if key == "transport_failure_policy":
            continue
        if cleanup.get(key) != value:
            raise ValueError(f"raw cleanup differs: {key}")
    failures = cleanup.get("transport_failures")
    reconciled = cleanup.get("transport_failures_reconciled")
    if not isinstance(failures, int) or failures < 0 or (failures > 0 and reconciled is not True):
        raise ValueError("transport failures are not preserved and reconciled")
    return {
        "schema": "p4.release-a.integrity-i0-external.v1",
        "artifact_sha256": digest(artifact_bytes),
        "config_sha256": seal["config_sha256"],
        "run_window": {"started_unix_ms": started, "elapsed_ms": elapsed},
        "gpu": bind_samples(raw["gpu"], started, elapsed, spec["required_scorecard"]),
        "resource_snapshots": bind_snapshots(raw, started, elapsed, spec["required_scorecard"]),
        "distributed": copy.deepcopy(distributed),
        "cleanup": copy.deepcopy(cleanup),
    }


def fixture(spec: dict) -> tuple[dict, bytes, dict, dict]:
    artifact = {"started_unix_ms": 1_000_000, "elapsed_ms": 2_000}
    artifact_bytes = (json.dumps(artifact, separators=(",", ":")) + "\n").encode()
    seal = {"config_sha256": "c" * 64}
    samples = [{"captured_unix_ms": 1_000_000 + tick, "utilization_percent": 50,
                "memory_used_bytes": 1024, "power_w": None, "temperature_c": None}
               for tick in (0, 1000, 2000)]
    states = spec["required_scorecard"]["resource_snapshot_states"]
    snapshot_times = (999_900, 1_001_000, 1_002_100, 1_002_200)
    raw = {
        "schema": "p4.release-a.integrity-i0-raw.v1",
        "artifact_sha256": digest(artifact_bytes), "config_sha256": seal["config_sha256"],
        "gpu": {"sample_interval_ms": 1000, "hosts": [
            {"host": host, "samples": copy.deepcopy(samples),
             "unavailable_reasons": {"power": "unsupported", "temperature": "unsupported"}}
            for host in HOSTS]},
        "resource_snapshots": [{"name": name, "hosts": [
            {"host": host, "captured_unix_ms": snapshot_times[index], **states[name]}
            for host in HOSTS]} for index, name in enumerate(spec["required_scorecard"]["resource_snapshots"])],
        "distributed": {"physical_hosts": 3, "stages": 3,
                        "all_hosts_own_model_shard": True, "all_hosts_own_kv": True,
                        "all_stages_compute": True, "cross_host_transfer_bytes": 1,
                        "undeclared_hosts": 0},
        "cleanup": {**spec["cleanup"], "transport_failures": 0,
                    "transport_failures_reconciled": True},
    }
    return artifact, artifact_bytes, seal, raw


def self_test(spec: dict) -> None:
    artifact, artifact_bytes, seal, raw = fixture(spec)
    assert build(artifact_bytes, artifact, seal, raw, spec)["gpu"]["hosts"][0]["samples"][0]["elapsed_ms"] == 0
    mutations = (
        lambda a, s, r: a.pop("started_unix_ms"),
        lambda a, s, r: r.update(artifact_sha256="0" * 64),
        lambda a, s, r: r["gpu"]["hosts"][0]["samples"].pop(1),
        lambda a, s, r: r["gpu"]["hosts"][0]["samples"][0].update(captured_unix_ms=990_000),
        lambda a, s, r: r["resource_snapshots"][1]["hosts"][0].update(node_count=0),
        lambda a, s, r: r["resource_snapshots"][3]["hosts"][2].update(captured_unix_ms=1_010_000),
        lambda a, s, r: r["distributed"].update(cross_host_transfer_bytes=0),
        lambda a, s, r: r["cleanup"].update(task_native_children=1),
        lambda a, s, r: r["cleanup"].update(transport_failures=1,
                                              transport_failures_reconciled=False),
    )
    for mutate in mutations:
        changed_artifact, changed_seal, changed_raw = copy.deepcopy(artifact), copy.deepcopy(seal), copy.deepcopy(raw)
        mutate(changed_artifact, changed_seal, changed_raw)
        changed_bytes = (json.dumps(changed_artifact, separators=(",", ":")) + "\n").encode()
        if changed_raw["artifact_sha256"] == digest(artifact_bytes):
            changed_raw["artifact_sha256"] = digest(changed_bytes)
        try:
            build(changed_bytes, changed_artifact, changed_seal, changed_raw, spec)
        except (KeyError, TypeError, ValueError):
            pass
        else:
            raise AssertionError("weakened raw I0 evidence was accepted")
    print(json.dumps({"passed": True, "tests": 1 + len(mutations)}, separators=(",", ":")))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--spec", type=Path, required=True)
    parser.add_argument("--artifact", type=Path)
    parser.add_argument("--seal", type=Path)
    parser.add_argument("--raw", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    spec = json.loads(args.spec.read_text(encoding="utf-8"))
    if args.self_test:
        self_test(spec)
        return
    if any(value is None for value in (args.artifact, args.seal, args.raw, args.output)):
        parser.error("artifact, seal, raw and output are required")
    artifact_bytes = args.artifact.read_bytes()
    result = build(artifact_bytes, json.loads(artifact_bytes),
                   json.loads(args.seal.read_text(encoding="utf-8")),
                   json.loads(args.raw.read_text(encoding="utf-8")), spec)
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"passed": True, "output": str(args.output)}, separators=(",", ":")))


if __name__ == "__main__":
    main()
