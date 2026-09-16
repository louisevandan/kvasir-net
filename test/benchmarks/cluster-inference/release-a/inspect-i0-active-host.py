#!/usr/bin/env python3
"""Fail-closed active task-agent preflight for Release A I0."""
from __future__ import annotations

import argparse
import hashlib
import json
import platform
import re
import subprocess
import time
from pathlib import Path


HOSTS = {
    "spark": {
        "protected_pid": 1287453, "task_run": "/home/m42/p4-i0-20260916/agent",
        "binary": "/home/m42/p4-h0-v4-19f2b1afa/source-git/target/release/p4-agent",
        "sha256": "86cac7b9e26ac4297c7d46f6ffcb2d098f9fa0fa6969f393db2f74276e8bb233",
        "advertised": "tcp://localhost:22151", "established_peer": "192.168.0.6",
        "established_count": 2,
    },
    "mac20": {
        "protected_pid": 69656, "task_run": "/Users/mobimac/p4-i0-20260916/agent",
        "binary": "/Users/mobimac/p4-h0-v4-19f2b1afa/source-git/target/release/p4-agent",
        "sha256": "5c2a3301a3b821d6ba990a7947fbbbd5182d8d1fdd64b2833183ba037cad0cdf",
        "advertised": "tcp://localhost:22152", "established_peer": "192.168.0.26",
        "established_count": 1,
    },
    "mac21": {
        "protected_pid": 85992, "task_run": "/Users/mobimac/p4-i0-20260916/agent",
        "binary": "/Users/mobimac/p4-h0-v4-19f2b1afa/source-git/target/release/p4-agent",
        "sha256": "5c2a3301a3b821d6ba990a7947fbbbd5182d8d1fdd64b2833183ba037cad0cdf",
        "advertised": "tcp://localhost:22153", "established_peer": "192.168.0.26",
        "established_count": 1,
    },
}


def run(argv: list[str]) -> dict[str, object]:
    completed = subprocess.run(argv, text=True, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, timeout=30, check=False)
    return {"argv": argv, "exit_code": completed.returncode,
            "stdout": completed.stdout.strip(), "stderr": completed.stderr.strip()}


def sha256(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def process_rows(text: str) -> dict[int, str]:
    rows = {}
    for line in text.splitlines():
        fields = line.strip().split(maxsplit=1)
        if len(fields) == 2 and fields[0].isdigit():
            rows[int(fields[0])] = fields[1]
    return rows


def linux_states(text: str) -> list[tuple[str, str]]:
    rows = []
    for line in text.splitlines():
        fields = line.split()
        if len(fields) >= 5:
            rows.append((fields[0].upper().replace("-", "_"), fields[4]))
    return rows


def mac_states(text: str, pid: int) -> list[tuple[str, str]]:
    rows = []
    for line in text.splitlines()[1:]:
        fields = line.split()
        if len(fields) < 10 or not fields[1].isdigit() or int(fields[1]) != pid:
            continue
        match = re.search(r"TCP\s+(\S+)\s+\(([^)]+)\)$", line)
        if match:
            endpoint = match.group(1)
            peer = endpoint.split("->", 1)[1] if "->" in endpoint else "*"
            rows.append((match.group(2).upper().replace("-", "_"), peer))
    return rows


def self_test() -> None:
    assert process_rows("12 /x/a one\n13 /x/b two") == {12: "/x/a one", 13: "/x/b two"}
    linux = "LISTEN 0 128 0.0.0.0:22150 0.0.0.0:*\nESTAB 0 0 1:22150 2:3"
    assert linux_states(linux) == [("LISTEN", "0.0.0.0:*"), ("ESTAB", "2:3")]
    mac = "COMMAND PID USER FD TYPE DEV SIZE NODE NAME\np4-agent 7 u 9u IPv4 x 0t0 TCP *:22150 (LISTEN)\np4-agent 7 u 10u IPv4 x 0t0 TCP 1:22150->2:3 (ESTABLISHED)"
    assert mac_states(mac, 7) == [("LISTEN", "*"), ("ESTABLISHED", "2:3")]
    assert not any(state == "CLOSE_WAIT" for state, _ in linux_states(linux))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--role", choices=sorted(HOSTS))
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        print(json.dumps({"passed": True, "tests": 4}, separators=(",", ":")))
        return
    if args.role is None:
        parser.error("--role is required")
    config = HOSTS[args.role]
    pid_path = Path(config["task_run"]) / "agent.pid"
    if not pid_path.is_file() or not pid_path.read_text().strip().isdigit():
        raise RuntimeError("task agent PID evidence is absent")
    task_pid = int(pid_path.read_text().strip())
    processes = run(["ps", "-axo", "pid=,command="])
    if processes["exit_code"] != 0:
        raise RuntimeError("process inventory failed")
    by_pid = process_rows(str(processes["stdout"]))
    expected_command = f'{config["binary"]} 0.0.0.0:22150 {config["advertised"]}'
    if by_pid.get(task_pid) != expected_command:
        raise RuntimeError("task agent PID/command differs")
    if config["protected_pid"] not in by_pid or "p4-agent" not in by_pid[config["protected_pid"]]:
        raise RuntimeError("protected agent is absent")
    owned = [row for row in by_pid.values()
             if any(token in row for token in ("p4_staged_server", "hf_worker"))]
    if owned:
        raise RuntimeError("native or HF worker exists before LOAD")
    binary = Path(config["binary"])
    if sha256(binary) != config["sha256"]:
        raise RuntimeError("task agent binary hash differs")
    if platform.system() == "Darwin":
        sockets = run(["lsof", "-nP", "-a", "-p", str(task_pid), "-iTCP:22150"])
        states = mac_states(str(sockets["stdout"]), task_pid)
    else:
        sockets = run(["ss", "-Htan", "sport", "=", ":22150"])
        states = linux_states(str(sockets["stdout"]))
        owner = run(["ss", "-Hltnp", "sport", "=", ":22150"])
        if f"pid={task_pid}," not in str(owner["stdout"]):
            raise RuntimeError("task listener owner differs")
    normalized = [("ESTAB" if state == "ESTABLISHED" else state, peer) for state, peer in states]
    if sum(state == "LISTEN" for state, _ in normalized) != 1:
        raise RuntimeError("task listener count differs")
    if any(state == "CLOSE_WAIT" for state, _ in normalized):
        raise RuntimeError("task agent has CLOSE_WAIT sockets")
    established = [peer for state, peer in normalized if state == "ESTAB"]
    if len(established) != config["established_count"] \
            or any(config["established_peer"] not in peer for peer in established):
        raise RuntimeError("task agent established topology differs")
    result = {
        "schema": "p4.release-a.i0-active-host.v1", "role": args.role,
        "captured_unix_ms": time.time_ns() // 1_000_000, "passed": True,
        "task_agent": {"pid": task_pid, "command": expected_command,
                       "binary_sha256": config["sha256"], "listener_count": 1,
                       "established_count": len(established), "close_wait_count": 0},
        "protected_agent": {"pid": config["protected_pid"], "present": True},
        "node_count": 0, "native_count": 0, "hf_worker_count": 0,
    }
    print(json.dumps(result, sort_keys=True, separators=(",", ":")))


if __name__ == "__main__":
    main()
