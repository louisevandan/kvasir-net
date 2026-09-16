#!/usr/bin/env python3
"""Deterministically sample one I0 host and prove task-owned process state."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import plistlib
import re
import signal
import subprocess
import sys
import time
from pathlib import Path


def run(argv: list[str], timeout: int = 30) -> str:
    completed = subprocess.run(argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               timeout=timeout, check=False)
    if completed.returncode:
        raise RuntimeError(f"command failed ({completed.returncode}): {argv!r}: "
                           f"{completed.stderr.decode(errors='replace').strip()}")
    return completed.stdout.decode(errors="strict")


def sha256(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def processes(text: str) -> dict[int, tuple[int, str]]:
    result = {}
    for line in text.splitlines():
        fields = line.strip().split(maxsplit=2)
        if len(fields) == 3 and fields[0].isdigit() and fields[1].isdigit():
            result[int(fields[0])] = (int(fields[1]), fields[2])
    return result


def task_processes(config: dict) -> tuple[int | None, list[int], dict[int, tuple[int, str]]]:
    table = processes(run(["ps", "-axo", "pid=,ppid=,command="]))
    pid_path = Path(config["agent_pid_file"])
    candidate = int(pid_path.read_text().strip()) if pid_path.is_file() else None
    agent_pid = candidate if candidate in table else None
    if agent_pid is not None:
        expected = " ".join(config["agent_command"])
        if table.get(agent_pid, (None, None))[1] != expected:
            raise RuntimeError("task agent PID no longer owns the sealed command")
        binary = Path(config["agent_command"][0])
        if sha256(binary) != config["agent_sha256"]:
            raise RuntimeError("task agent binary hash differs")
    native = []
    if agent_pid is not None:
        for pid, (parent, command) in table.items():
            if parent == agent_pid and command.split(maxsplit=1)[0] == config["native_binary"]:
                native.append(pid)
    for pid in native:
        if sha256(Path(config["native_binary"])) != config["native_sha256"]:
            raise RuntimeError("task native binary hash differs")
    return agent_pid, native, table


def listener_pids() -> set[int]:
    if sys.platform == "darwin":
        text = run(["lsof", "-nP", "-iTCP", "-sTCP:LISTEN"])
        return {int(fields[1]) for line in text.splitlines()[1:]
                if len((fields := line.split())) > 1 and fields[1].isdigit()}
    text = run(["ss", "-Hltnp"])
    return {int(value) for value in re.findall(r"pid=(\d+)", text)}


def state(config: dict, expected: str) -> dict:
    agent, native, _ = task_processes(config)
    listeners = listener_pids()
    task_pids = ({agent} if agent is not None else set()) | set(native)
    listener_count = len(task_pids & listeners)
    expected_values = {
        "unloaded": (True, 0, 1, False),
        "loaded": (True, 1, 2, True),
        "clean": (False, 0, 0, False),
    }[expected]
    actual = (agent is not None, len(native), listener_count, bool(native))
    if actual != expected_values:
        raise RuntimeError(f"task resource state differs: expected={expected_values} actual={actual}")
    return {
        "host": config["host"], "captured_unix_ms": time.time_ns() // 1_000_000,
        "task_native_children": len(native), "task_listeners": listener_count,
        "model_resident": bool(native), "task_agent_present": agent is not None,
        "native_pids": native,
    }


def cuda_sample(config: dict) -> dict:
    line = run(["nvidia-smi", "--query-gpu=index,utilization.gpu,memory.used,power.draw,temperature.gpu",
                "--format=csv,noheader,nounits"]).splitlines()
    if len(line) != 1:
        raise RuntimeError("I0 CUDA host must expose exactly one accelerator")
    fields = [value.strip() for value in line[0].split(",")]
    if len(fields) != 5 or fields[0] != str(config["gpu_index"]):
        raise RuntimeError("CUDA telemetry device identity differs")
    _, native, table = task_processes(config)
    memory = fields[2]
    memory_source = "nvidia_smi_memory_used"
    if memory == "[N/A]":
        if len(native) != 1:
            raise RuntimeError("unified-memory telemetry requires one task native process")
        status = Path(f"/proc/{native[0]}/status").read_text()
        match = re.search(r"^VmRSS:\s+(\d+)\s+kB$", status, re.MULTILINE)
        if not match:
            raise RuntimeError("task native RSS is unavailable")
        memory_bytes = int(match.group(1)) * 1024
        memory_source = "task_native_rss_unified_memory_proxy"
    else:
        memory_bytes = int(float(memory) * 1024 * 1024)
    return {
        "captured_unix_ms": time.time_ns() // 1_000_000,
        "utilization_percent": float(fields[1]), "memory_used_bytes": memory_bytes,
        "power_w": None if fields[3] == "[N/A]" else float(fields[3]),
        "temperature_c": None if fields[4] == "[N/A]" else float(fields[4]),
        "memory_source": memory_source,
    }


def metal_sample(config: dict) -> dict:
    raw = subprocess.run(["ioreg", "-r", "-c", "AGXAccelerator", "-k",
                          "PerformanceStatistics", "-a"], stdout=subprocess.PIPE,
                         stderr=subprocess.PIPE, timeout=30, check=False)
    if raw.returncode:
        raise RuntimeError(raw.stderr.decode(errors="replace"))
    values = [row["PerformanceStatistics"] for row in plistlib.loads(raw.stdout)
              if "PerformanceStatistics" in row]
    if len(values) != 1:
        raise RuntimeError("Metal PerformanceStatistics identity differs")
    stats = values[0]
    utilization = stats.get("Device Utilization %")
    memory = stats.get("Alloc system memory")
    if not isinstance(utilization, (int, float)) or not isinstance(memory, int) or memory <= 0:
        raise RuntimeError("Metal utilization or allocated memory is unavailable")
    return {
        "captured_unix_ms": time.time_ns() // 1_000_000,
        "utilization_percent": float(utilization), "memory_used_bytes": memory,
        "power_w": None, "temperature_c": None,
        "memory_source": "ioreg_PerformanceStatistics_Alloc_system_memory",
    }


def sample_loop(config: dict, stop_file: Path) -> None:
    if stop_file.exists():
        raise RuntimeError("sampler stop file already exists")
    stopping = False

    def stop(_signum, _frame):
        nonlocal stopping
        stopping = True

    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    next_tick = time.monotonic()
    sampler = cuda_sample if config["telemetry"] == "cuda" else metal_sample
    while not stopping and not stop_file.exists():
        print(json.dumps(sampler(config), separators=(",", ":")), flush=True)
        next_tick += 1.0
        time.sleep(max(0.0, next_tick - time.monotonic()))


def self_test() -> None:
    table = processes("12 1 /a one\n13 12 /b two")
    assert table == {12: (1, "/a one"), 13: (12, "/b two")}
    xml = plistlib.dumps([{"PerformanceStatistics": {
        "Device Utilization %": 75, "Alloc system memory": 4096}}])
    values = plistlib.loads(xml)[0]["PerformanceStatistics"]
    assert values["Device Utilization %"] == 75 and values["Alloc system memory"] == 4096
    assert set(("unloaded", "loaded", "clean")) == {"unloaded", "loaded", "clean"}
    print(json.dumps({"passed": True, "tests": 3}, separators=(",", ":")))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", type=Path)
    parser.add_argument("--state", choices=("unloaded", "loaded", "clean"))
    parser.add_argument("--sample", action="store_true")
    parser.add_argument("--stop-file", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test(); return
    if args.config is None:
        parser.error("--config is required")
    config = json.loads(args.config.read_text())
    if args.sample:
        if args.stop_file is None:
            parser.error("--sample requires --stop-file")
        sample_loop(config, args.stop_file)
    elif args.state:
        print(json.dumps(state(config, args.state), separators=(",", ":")))
    else:
        parser.error("choose --state or --sample")


if __name__ == "__main__":
    main()
