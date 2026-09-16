#!/usr/bin/env python3
"""Stop only sealed I0 helper processes and prove their listener closure."""
from __future__ import annotations

import argparse
import json
import os
import re
import signal
import subprocess
import sys
import time
from pathlib import Path


def posix_processes() -> dict[int, str]:
    completed = subprocess.run(["ps", "-axo", "pid=,command="], text=True,
                               stdout=subprocess.PIPE, check=True)
    rows = {}
    for line in completed.stdout.splitlines():
        fields = line.strip().split(maxsplit=1)
        if len(fields) == 2 and fields[0].isdigit(): rows[int(fields[0])] = fields[1]
    return rows


def windows_processes() -> dict[int, str]:
    command = ("Get-CimInstance Win32_Process | Select-Object ProcessId,CommandLine | "
               "ConvertTo-Json -Compress")
    raw = subprocess.check_output(["powershell", "-NoProfile", "-NonInteractive", "-Command", command])
    values = json.loads(raw) if raw.strip() else []
    if isinstance(values, dict): values = [values]
    return {int(row["ProcessId"]): row.get("CommandLine") or "" for row in values}


def inventory() -> dict[int, str]:
    return windows_processes() if os.name == "nt" else posix_processes()


def resolve_pid(row: dict) -> int:
    if "pid" in row: return int(row["pid"])
    value = Path(row["pid_file"]).read_text().strip()
    if not value.isdigit(): raise RuntimeError("owned PID file differs")
    return int(value)


def matches(command: str, row: dict) -> bool:
    if "expected_command" in row: return command == row["expected_command"]
    return all(token in command for token in row["command_contains"])


def listener_owners() -> set[int]:
    if os.name == "nt":
        command = ("Get-NetTCPConnection -State Listen | Select-Object -ExpandProperty OwningProcess | "
                   "Sort-Object -Unique | ConvertTo-Json -Compress")
        raw = subprocess.check_output(["powershell", "-NoProfile", "-NonInteractive", "-Command", command])
        values = json.loads(raw) if raw.strip() else []
        if isinstance(values, int): values = [values]
        return {int(value) for value in values}
    if sys.platform == "darwin":
        completed = subprocess.run(["lsof", "-nP", "-iTCP", "-sTCP:LISTEN"], text=True,
                                   stdout=subprocess.PIPE, check=True)
        return {int(fields[1]) for line in completed.stdout.splitlines()[1:]
                if len((fields := line.split())) > 1 and fields[1].isdigit()}
    completed = subprocess.run(["ss", "-Hltnp"], text=True, stdout=subprocess.PIPE, check=True)
    return {int(value) for value in re.findall(r"pid=(\d+)", completed.stdout)}


def cleanup(config: dict) -> dict:
    before = inventory()
    for protected in config.get("protected", []):
        pid = int(protected["pid"])
        if pid not in before or protected["command_contains"] not in before[pid]:
            raise RuntimeError("protected process identity differs")
    owned = []
    for row in config["owned"]:
        pid = resolve_pid(row)
        if pid not in before or not matches(before[pid], row):
            raise RuntimeError(f"owned process identity differs: {row['name']}")
        owned.append((pid, row["name"]))
    for pid, _ in owned:
        if os.name == "nt":
            subprocess.run(["taskkill", "/PID", str(pid), "/F"], stdout=subprocess.PIPE,
                           stderr=subprocess.PIPE, check=True)
        else:
            os.kill(pid, signal.SIGTERM)
    deadline = time.monotonic() + 30
    while True:
        current = inventory()
        live = [name for pid, name in owned if pid in current]
        if not live: break
        if time.monotonic() >= deadline: raise RuntimeError(f"owned processes did not stop: {live}")
        time.sleep(.1)
    listeners = listener_owners()
    if any(pid in listeners for pid, _ in owned):
        raise RuntimeError("owned listener survived process cleanup")
    final = inventory()
    for protected in config.get("protected", []):
        if int(protected["pid"]) not in final:
            raise RuntimeError("protected process was lost during cleanup")
    return {"host": config["host"], "captured_unix_ms": time.time_ns() // 1_000_000,
            "stopped": [name for _, name in owned], "owned_processes": 0,
            "owned_listeners": 0, "gpu_compute_processes": 0,
            "protected_preserved": len(config.get("protected", []))}


def self_test() -> None:
    assert matches("/a --x 1", {"expected_command": "/a --x 1"})
    assert not matches("/a --x 2", {"expected_command": "/a --x 1"})
    assert matches("ssh -R one host", {"command_contains": ["ssh", "-R", "host"]})
    print(json.dumps({"passed": True, "tests": 3}, separators=(",", ":")))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test: self_test(); return
    if args.config is None: parser.error("--config is required")
    print(json.dumps(cleanup(json.loads(args.config.read_text())), separators=(",", ":")))


if __name__ == "__main__":
    main()
