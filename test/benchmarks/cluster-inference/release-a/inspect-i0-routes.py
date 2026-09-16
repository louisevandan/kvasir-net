#!/usr/bin/env python3
"""Capture exact advertised-route INSPECT state for every I0 agent."""
from __future__ import annotations

import argparse
import importlib
import json
import sys
import uuid
from pathlib import Path


SNAPSHOT = "application/vnd.p4.agent.snapshot-v1+json"


def validate(snapshot: dict, expected_nodes: list[str]) -> dict:
    nodes = snapshot.get("nodes") or []
    actual = sorted(row.get("node_id") for row in nodes)
    if actual != sorted(expected_nodes):
        raise ValueError(f"INSPECT nodes differ: expected={expected_nodes} actual={actual}")
    if any(row.get("lifecycle_state") != "loaded" for row in nodes):
        raise ValueError("INSPECT node is not loaded")
    transport = snapshot.get("transport") or {}
    failures = (transport.get("failures") or {}).get("count")
    transfer = transport.get("transfer") or {}
    for key in ("hop_data_writes", "hop_data_bytes"):
        if not isinstance(transfer.get(key), int) or transfer[key] < 0:
            raise ValueError("INSPECT transfer counters are absent")
    if not isinstance(failures, int) or failures < 0:
        raise ValueError("INSPECT transport failure count is absent")
    return {
        "captured_unix_ms": snapshot.get("generated_at_unix_ms"),
        "node_count": len(nodes), "transport_failures": failures,
        "hop_data_writes": transfer["hop_data_writes"],
        "hop_data_bytes": transfer["hop_data_bytes"],
    }


def inspect(config: dict, python_path: Path, expected: str) -> dict:
    sys.path.insert(0, str(python_path))
    Client = importlib.import_module("p4hfadapter.integration.transport").Client
    ingress = config["ingress_agent"]
    address = ingress.removeprefix("tcp://")
    host, port = address.rsplit(":", 1)
    targets = []
    seen = set()
    for node in config["nodes"]:
        if node["agent"] not in seen:
            targets.append(node["agent"]); seen.add(node["agent"])
    if len(targets) != 3:
        raise ValueError("I0 requires three distinct advertised agents")
    client = Client(host, int(port), timeout=30)
    client.address = ingress
    client.outer = (2, ingress, uuid.uuid4().hex, config["connection_generation"])
    rows = []
    try:
        for name, target in zip(("spark", "mac20", "mac21"), targets):
            metadata, body = client.exchange(
                (0, target), "application/vnd.p4.agent.inspect-v1+json", b"{}")
            if metadata.get("content") != SNAPSHOT:
                raise ValueError("INSPECT reply content type differs")
            expected_nodes = [] if expected == "unloaded" else [
                node["node"] for node in config["nodes"] if node["agent"] == target]
            row = validate(json.loads(body), expected_nodes)
            row.update(host=name, address=target)
            rows.append(row)
    finally:
        try:
            client.finish()
        finally:
            client.close()
    return {"schema": "p4.release-a.i0-route-snapshot.v1", "expected": expected,
            "rows": rows}


def self_test() -> None:
    snapshot = {"generated_at_unix_ms": 1, "nodes": [], "transport": {
        "failures": {"count": 0}, "transfer": {"hop_data_writes": 0, "hop_data_bytes": 0}}}
    assert validate(snapshot, [])["node_count"] == 0
    loaded = json.loads(json.dumps(snapshot)); loaded["nodes"] = [
        {"node_id": "b", "lifecycle_state": "loaded"},
        {"node_id": "a", "lifecycle_state": "loaded"}]
    loaded["transport"]["transfer"] = {"hop_data_writes": 2, "hop_data_bytes": 100}
    assert validate(loaded, ["a", "b"])["hop_data_bytes"] == 100
    mutations = (
        lambda value: value["transport"].pop("transfer"),
        lambda value: value["transport"]["failures"].update(count=-1),
        lambda value: value.update(nodes=[{"node_id": "a", "lifecycle_state": "loading"}]),
    )
    for mutate in mutations:
        changed = json.loads(json.dumps(loaded)); mutate(changed)
        try:
            validate(changed, ["a", "b"])
        except ValueError:
            pass
        else:
            raise AssertionError("weakened route evidence was accepted")
    print(json.dumps({"passed": True, "tests": 5}, separators=(",", ":")))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", type=Path)
    parser.add_argument("--python-path", type=Path)
    parser.add_argument("--expected", choices=("loaded", "unloaded"))
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test(); return
    if None in (args.config, args.python_path, args.expected):
        parser.error("config, python-path and expected are required")
    print(json.dumps(inspect(json.loads(args.config.read_text()), args.python_path, args.expected),
                     separators=(",", ":")))


if __name__ == "__main__":
    main()
