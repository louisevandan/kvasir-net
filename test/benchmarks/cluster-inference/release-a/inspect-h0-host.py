#!/usr/bin/env python3
"""Read-only Release A H0 host inventory.

The script is transferred as a file and invoked with an argv-only SSH command.
It never starts an agent, native worker, build, or model load.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import shutil
import socket
import subprocess
import sys
import time
from pathlib import Path


SOURCE_COMMIT = "c6a28b58269f9ff06c21c8cf9aee73a7ec0b21fa"
HOSTS = {
    "spark": {
        "peers": ["192.168.0.20", "192.168.0.21"],
        "source": "/home/m42/p4-release-a-h0-c6a28b582",
        "binaries": {
            "agent": "/home/m42/p4-release-a-h0-c6a28b582-target/release/p4-agent",
            "event_drive": "/home/m42/p4-release-a-h0-c6a28b582-target/release/p4-event-drive",
            "native": "/home/m42/p4-release-a-bytes-b0-a39eef08/build/p4_staged_server",
        },
    },
    "mac20": {
        "peers": ["192.168.0.26", "192.168.0.21"],
        "source": "/Users/mobimac/p4-release-a-h0-c6a28b582",
        "binaries": {
            "agent": "/Users/mobimac/p4-release-a-h0-c6a28b582-target/release/p4-agent",
            "native": "/Users/mobimac/p4-b5-native-build-48437eaeb/p4_staged_server",
        },
    },
    "mac21": {
        "peers": ["192.168.0.26", "192.168.0.20"],
        "source": "/Users/mobimac/p4-release-a-h0-c6a28b582",
        "binaries": {
            "agent": "/Users/mobimac/p4-release-a-h0-c6a28b582-target/release/p4-agent",
            "native": "/Users/mobimac/p4-b5-native-build-48437eaeb/p4_staged_server",
        },
    },
}


def run(argv: list[str], *, required: bool = False) -> dict[str, object]:
    executable = shutil.which(argv[0])
    if executable is None:
        result = {"argv": argv, "status": "unavailable", "reason": "executable_not_found"}
        if required:
            raise RuntimeError(json.dumps(result, sort_keys=True))
        return result
    completed = subprocess.run(
        [executable, *argv[1:]], text=True, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, timeout=30, check=False,
    )
    result = {
        "argv": argv,
        "exit_code": completed.returncode,
        "stdout": completed.stdout.strip(),
        "stderr": completed.stderr.strip(),
    }
    if required and completed.returncode != 0:
        raise RuntimeError(json.dumps(result, sort_keys=True))
    return result


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def file_binding(path_text: str) -> dict[str, object]:
    path = Path(path_text)
    if not path.is_file():
        raise RuntimeError(f"missing sealed binary: {path}")
    stat = path.stat()
    return {"path": str(path), "bytes": stat.st_size, "sha256": sha256(path)}


def physical_memory_bytes() -> int:
    if sys.platform == "darwin":
        return int(run(["sysctl", "-n", "hw.memsize"], required=True)["stdout"])
    pages = os.sysconf("SC_PHYS_PAGES")
    size = os.sysconf("SC_PAGE_SIZE")
    return int(pages * size)


def cpu_inventory() -> dict[str, object]:
    if sys.platform == "darwin":
        return run(["sysctl", "-n", "machdep.cpu.brand_string"], required=True)
    return run(["lscpu", "-J"], required=True)


def dynamic_port_range() -> dict[str, int]:
    if sys.platform == "darwin":
        first = int(run(["sysctl", "-n", "net.inet.ip.portrange.hifirst"], required=True)["stdout"])
        last = int(run(["sysctl", "-n", "net.inet.ip.portrange.hilast"], required=True)["stdout"])
    else:
        fields = Path("/proc/sys/net/ipv4/ip_local_port_range").read_text().split()
        if len(fields) != 2:
            raise RuntimeError("unexpected Linux dynamic port range")
        first, last = map(int, fields)
    if not 0 < first <= last <= 65535:
        raise RuntimeError("invalid dynamic port range")
    return {"first": first, "last": last}


def route(peer: str) -> dict[str, object]:
    if sys.platform == "darwin":
        raw = run(["route", "-n", "get", peer], required=True)
        match = re.search(r"^\s*interface:\s+(\S+)", str(raw["stdout"]), re.MULTILINE)
        if not match:
            raise RuntimeError(f"route interface missing for {peer}")
        interface = match.group(1)
        link = run(["ifconfig", interface], required=True)
    else:
        raw = run(["ip", "route", "get", peer], required=True)
        match = re.search(r"\bdev\s+(\S+)", str(raw["stdout"]), re.MULTILINE)
        if not match:
            raise RuntimeError(f"route interface missing for {peer}")
        interface = match.group(1)
        link = run(["ethtool", interface])
    ping = run(["ping", "-c", "5", peer], required=True)
    return {"peer": peer, "interface": interface, "route": raw, "link": link, "ping": ping}


def platform_accelerator() -> dict[str, object]:
    if sys.platform == "darwin":
        metal = run([
            "xcrun", "swift", "-e",
            "import Metal; for d in MTLCopyAllDevices() { print(\"\\(d.name)|\\(d.registryID)\") }",
        ])
        return {
            "backend": "Metal",
            "inventory": run(["system_profiler", "SPDisplaysDataType", "-json"], required=True),
            "device_registry": metal,
            "power_cap": {"status": "unavailable", "reason": "macOS_Metal_has_no_configurable_device_power_cap"},
        }
    query = run([
        "nvidia-smi",
        "--query-gpu=index,uuid,name,driver_version,memory.total,power.limit",
        "--format=csv,noheader,nounits",
    ], required=True)
    rows = []
    for line in str(query["stdout"]).splitlines():
        fields = [field.strip() for field in line.split(",")]
        if len(fields) != 6:
            raise RuntimeError(f"unexpected nvidia-smi field count: {line}")
        rows.append(dict(zip(
            ("index", "uuid", "name", "driver_version", "memory_total_mib", "power_limit_watts"),
            fields,
        )))
    if not rows:
        raise RuntimeError("nvidia-smi returned no accelerators")
    power_values = [row["power_limit_watts"] for row in rows]
    power_cap = (
        {"status": "unavailable", "reason": "nvidia_smi_reports_NA_for_power_limit"}
        if any(value == "[N/A]" for value in power_values)
        else {"status": "measured", "watts": [float(value) for value in power_values]}
    )
    return {
        "backend": "CUDA",
        "inventory": query,
        "devices": rows,
        "compute_processes": run([
            "nvidia-smi", "--query-compute-apps=pid,gpu_uuid,used_memory",
            "--format=csv,noheader,nounits",
        ]),
        "power_cap": power_cap,
    }


def dependencies(native_path: str) -> dict[str, object]:
    if sys.platform == "darwin":
        root = Path(native_path).parent
        files = [file_binding(str(path)) for path in sorted(root.rglob("*.dylib")) if path.is_file()]
        if not files:
            raise RuntimeError(f"no deployed dylib found below {root}")
        return {
            "binding": "macOS_dyld_shared_cache_and_file_hashes",
            "otool": run(["otool", "-L", native_path], required=True),
            "os_build": run(["sw_vers", "-buildVersion"], required=True),
            "files": files,
        }
    ldd = run(["ldd", native_path], required=True)
    paths = sorted(set(re.findall(r"(?:=>\s+)?(/\S+)", str(ldd["stdout"]))))
    files = []
    for raw_path in paths:
        clean = raw_path.rstrip("():")
        path = Path(clean)
        if path.is_file():
            files.append(file_binding(str(path)))
    return {"binding": "file_hashes", "ldd": ldd, "files": files}


def telemetry_capability() -> dict[str, object]:
    if sys.platform == "darwin":
        return {
            "primary": run([
                "sudo", "-n", "powermetrics", "-n", "1", "-i", "100",
                "--samplers", "gpu_power",
            ]),
            "fallback": run([
                "ioreg", "-r", "-c", "AGXAccelerator", "-k", "PerformanceStatistics", "-a",
            ]),
        }
    return {"primary": run([
        "nvidia-smi",
        "--query-gpu=timestamp,index,utilization.gpu,utilization.memory,power.draw,temperature.gpu,clocks.current.sm,memory.used",
        "--format=csv,noheader,nounits",
    ], required=True)}


def task_safety() -> dict[str, object]:
    processes = run(["ps", "-axo", "pid=,ppid=,command="], required=True)
    processes["stdout"] = "\n".join(
        line for line in str(processes["stdout"]).splitlines()
        if any(name in line for name in ("p4-agent", "p4_staged_server", "hf_worker"))
    )
    listeners = run(["lsof", "-nP", "-iTCP:52005", "-sTCP:LISTEN"])
    if sys.platform != "darwin":
        listeners = run(["ss", "-ltnp"], required=True)
        listeners["stdout"] = "\n".join(
            line for line in str(listeners["stdout"]).splitlines() if "52005" in line
        )
    return {"processes": processes, "listeners": listeners}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--role", choices=sorted(HOSTS), required=True)
    args = parser.parse_args()
    config = HOSTS[args.role]
    script = Path(__file__).resolve()
    source = Path(config["source"])
    git_head = run(["git", "-C", str(source), "rev-parse", "HEAD"], required=True)
    if git_head["stdout"] != SOURCE_COMMIT:
        raise RuntimeError(f"source commit mismatch: {git_head['stdout']}")
    git_status = run(["git", "-C", str(source), "status", "--short"], required=True)
    if git_status["stdout"]:
        raise RuntimeError("source tree is dirty")
    inventory = {
        "schema": "p4.release-a.h0-host-inspect.v1",
        "role": args.role,
        "captured_unix_ms": time.time_ns() // 1_000_000,
        "script": file_binding(str(script)),
        "source_commit": SOURCE_COMMIT,
        "source_path": str(source),
        "hostname": socket.gethostname(),
        "platform": platform.platform(),
        "machine": platform.machine(),
        "logical_cpu_count": os.cpu_count(),
        "cpu_inventory": cpu_inventory(),
        "physical_memory_bytes": physical_memory_bytes(),
        "dynamic_port_range": dynamic_port_range(),
        "disk": shutil.disk_usage(source)._asdict(),
        "git_head": git_head,
        "git_status": git_status,
        "binaries": {name: file_binding(path) for name, path in config["binaries"].items()},
        "accelerator": platform_accelerator(),
        "links": [route(peer) for peer in config["peers"]],
        "native_dependencies": dependencies(config["binaries"]["native"]),
        "telemetry_capability": telemetry_capability(),
        "task_safety": task_safety(),
    }
    print(json.dumps(inventory, sort_keys=True, separators=(",", ":")))


if __name__ == "__main__":
    main()
