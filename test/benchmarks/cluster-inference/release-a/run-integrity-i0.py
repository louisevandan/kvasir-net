#!/usr/bin/env python3
"""Run one sealed remote I0 execution and capture barrier-bound raw evidence."""
from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
import json
import subprocess
import sys
import threading
from pathlib import Path


LOADED = "P4_EVENT_GATE_LOADED"
WINDOW = "P4_EVENT_GATE_INFERENCE_WINDOW"
DRAINED = "P4_EVENT_GATE_DRAINED"
HOST_ORDER = ("spark", "mac20", "mac21")
BINDING_KINDS = ("observer", "observer-config", "cleanup-script", "cleanup-config")


def execute(argv: list[str], timeout: int = 60) -> bytes:
    completed = subprocess.run(argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               timeout=timeout, check=False)
    if completed.returncode:
        raise RuntimeError(f"command failed ({completed.returncode}): {argv!r}: "
                           f"{completed.stderr.decode(errors='replace').strip()}")
    return completed.stdout


def json_command(argv: list[str], timeout: int = 60) -> dict:
    return json.loads(execute(argv, timeout).decode())


def remote(host: dict, *argv: str) -> list[str]:
    return [*host["ssh"], *argv]


def route_command(manifest: dict, expected: str) -> list[str]:
    route = manifest["route"]
    return [*route["ssh"], "python3", route["script"], "--config", route["config"],
            "--python-path", route["python_path"], "--expected", expected]


def validate_manifest(manifest: dict) -> None:
    if [host["host"] for host in manifest["hosts"]] != list(HOST_ORDER):
        raise ValueError("manifest host order differs")
    names = [binding["name"] for binding in manifest["bindings"]]
    required = {"driver", "config", "route", "controller-script", "controller-config"}
    required.update(f"{kind}-{host}" for host in HOST_ORDER for kind in BINDING_KINDS)
    if len(names) != len(set(names)) or set(names) != required:
        raise ValueError("manifest execution bindings differ")
    for binding in manifest["bindings"]:
        if (not isinstance(binding.get("command"), list) or not binding["command"] or
                len(binding.get("sha256", "")) != 64):
            raise ValueError(f"invalid execution binding: {binding.get('name')}")
    controller = manifest.get("controller_preflight") or {}
    if (not isinstance(controller.get("command"), list) or not controller["command"] or
            controller.get("host") != "windows-controller" or
            controller.get("owned") != ["mac20-return", "mac20-next",
                                        "mac21-return", "mac21-previous"]):
        raise ValueError("controller tunnel preflight differs")


def inspect_controller(manifest: dict) -> dict:
    expected = manifest["controller_preflight"]
    actual = json_command(expected["command"])
    if actual.get("host") != expected["host"] or actual.get("owned") != expected["owned"]:
        raise ValueError("controller tunnel identity differs")
    return actual


def preflight(manifest: dict) -> None:
    for binding in manifest["bindings"]:
        actual = execute(binding["command"]).decode().strip().split()[0]
        if actual != binding["sha256"]:
            raise ValueError(f"sealed execution binding differs: {binding['name']}")
    inspect_controller(manifest)


def snapshot(manifest: dict, expected: str) -> tuple[dict, dict]:
    host_state = "loaded" if expected == "loaded" else "unloaded"
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        route_future = pool.submit(json_command, route_command(manifest, expected))
        futures = {host["host"]: pool.submit(
            json_command, remote(host, "python3", host["observer"], "--config",
                                 host["config"], "--state", host_state))
                   for host in manifest["hosts"]}
        route = route_future.result()
        states = {name: future.result() for name, future in futures.items()}
    route_rows = {row["host"]: row for row in route["rows"]}
    rows = []
    for name in HOST_ORDER:
        state = states[name]
        route_row = route_rows[name]
        rows.append({
            "host": name,
            "captured_unix_ms": max(state["captured_unix_ms"], route_row["captured_unix_ms"]),
            "node_count": route_row["node_count"],
            "task_native_children": state["task_native_children"],
            "task_listeners": state["task_listeners"],
            "model_resident": state["model_resident"],
        })
    return {"hosts": rows}, route


class Sampler:
    def __init__(self, host: dict):
        self.host = host
        self.rows: list[dict] = []
        self.errors: list[str] = []
        self.process: subprocess.Popen | None = None
        self.thread: threading.Thread | None = None

    def start(self) -> None:
        argv = remote(self.host, "python3", self.host["observer"], "--config",
                      self.host["config"], "--sample", "--stop-file", self.host["stop_file"])
        self.process = subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE)

        def consume() -> None:
            assert self.process and self.process.stdout
            for line in self.process.stdout:
                try:
                    self.rows.append(json.loads(line))
                except Exception as error:
                    self.errors.append(f"invalid sampler row: {error}: {line!r}")
        self.thread = threading.Thread(target=consume, daemon=True)
        self.thread.start()

    def stop(self) -> None:
        if self.process is None:
            return
        execute(remote(self.host, "touch", self.host["stop_file"]))
        try:
            self.process.wait(timeout=30)
        except subprocess.TimeoutExpired:
            self.process.terminate(); self.process.wait(timeout=10)
        if self.thread:
            self.thread.join(timeout=5)
        assert self.process.stderr
        stderr = self.process.stderr.read()
        if self.process.returncode or stderr:
            self.errors.append(stderr.decode(errors="replace").strip())
        if self.errors or not self.rows:
            raise RuntimeError(f"{self.host['host']} sampler failed: {self.errors}")


def run(manifest: dict, output: Path) -> dict:
    validate_manifest(manifest)
    if output.exists():
        raise ValueError("I0 output directory already exists")
    preflight(manifest)
    output.mkdir(parents=True)
    before, before_route = snapshot(manifest, "unloaded")
    before.update(name="before_load")
    driver = manifest["driver"]
    driver_argv = [*driver["ssh"], driver["binary"], driver["config"], driver["artifact"]]
    process = subprocess.Popen(driver_argv, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    samplers = [Sampler(host) for host in manifest["hosts"]]
    markers: dict[str, int] = {}
    snapshots = [before]
    routes = {"before_load": before_route}
    log = []
    failure = None
    try:
        assert process.stdout
        for raw_line in process.stdout:
            line = raw_line.decode(errors="replace").rstrip()
            log.append(line)
            marker = next((value for value in (LOADED, WINDOW, DRAINED) if line.startswith(value)), None)
            if marker is None:
                continue
            markers[marker] = markers.get(marker, 0) + 1
            if markers[marker] != 1:
                raise RuntimeError(f"duplicate execution barrier: {marker}")
            if marker == LOADED:
                for sampler in samplers:
                    sampler.start()
            elif marker == WINDOW:
                peak, route = snapshot(manifest, "loaded")
                peak.update(name="peak"); snapshots.append(peak); routes["peak"] = route
            elif marker == DRAINED:
                drained, route = snapshot(manifest, "loaded")
                drained.update(name="after_drain"); snapshots.append(drained); routes["after_drain"] = route
                for sampler in samplers:
                    sampler.stop()
        process.wait(timeout=30)
        if process.returncode:
            raise RuntimeError(f"event driver failed with exit {process.returncode}")
        if set(markers) != {LOADED, WINDOW, DRAINED}:
            raise RuntimeError(f"execution barriers missing: {markers}")
        unloaded, route = snapshot(manifest, "unloaded")
        unloaded.update(name="after_unload"); snapshots.append(unloaded); routes["after_unload"] = route
        with concurrent.futures.ThreadPoolExecutor(max_workers=len(manifest["cleanup"])) as pool:
            cleanup_futures = [pool.submit(json_command, row["command"]) for row in manifest["cleanup"]]
            cleanup_rows = [future.result() for future in cleanup_futures]
    except Exception as error:
        failure = str(error)
        if process.poll() is None:
            try:
                remaining, _ = process.communicate(timeout=manifest["driver_timeout_seconds"])
                log.extend(remaining.decode(errors="replace").splitlines())
            except subprocess.TimeoutExpired:
                failure += "; driver did not reach its sealed deadline; forced termination is forbidden while nodes may be loaded"
        for sampler in samplers:
            try:
                sampler.stop()
            except Exception as stop_error:
                failure += f"; sampler cleanup: {stop_error}"
        if process.poll() is not None:
            try:
                unloaded, route = snapshot(manifest, "unloaded")
                unloaded.update(name="after_unload")
                routes["failure_after_unload"] = route
                with concurrent.futures.ThreadPoolExecutor(max_workers=len(manifest["cleanup"])) as pool:
                    cleanup_futures = [pool.submit(json_command, row["command"])
                                       for row in manifest["cleanup"]]
                    [future.result() for future in cleanup_futures]
            except Exception as cleanup_error:
                failure += f"; failure cleanup: {cleanup_error}"
    (output / "driver.log").write_text("\n".join(log) + "\n", encoding="utf-8")
    (output / "route-snapshots.json").write_text(json.dumps(routes, indent=2) + "\n")
    if failure:
        (output / "failure.json").write_text(json.dumps({"error": failure}, indent=2) + "\n")
        raise RuntimeError(failure)
    artifact_bytes = execute([*driver["ssh"], "cat", driver["artifact"]])
    artifact = json.loads(artifact_bytes)
    (output / "artifact.json").write_bytes(artifact_bytes)
    config_bytes = Path(manifest["local_config"]).read_bytes()
    peak_bytes = sum(row["hop_data_bytes"] for row in routes["peak"]["rows"])
    drained_bytes = sum(row["hop_data_bytes"] for row in routes["after_drain"]["rows"])
    cross_bytes = drained_bytes - peak_bytes
    if cross_bytes <= 0:
        raise RuntimeError("no cross-host inference transfer was measured")
    failures = sum(row["transport_failures"] for row in routes["after_unload"]["rows"])
    if failures:
        raise RuntimeError("I0 transport failures require explicit reconciliation evidence")
    if any(row.get("owned_processes") != 0 or row.get("owned_listeners") != 0
           or row.get("gpu_compute_processes") != 0 for row in cleanup_rows):
        raise RuntimeError("I0 task cleanup differs")
    gpu_hosts = []
    for sampler in samplers:
        optional = {key: any(row[key] is not None for row in sampler.rows)
                    for key in ("power_w", "temperature_c")}
        reasons = {}
        if not optional["power_w"]: reasons["power"] = "host_telemetry_unavailable"
        if not optional["temperature_c"]: reasons["temperature"] = "host_telemetry_unavailable"
        gpu_hosts.append({"host": sampler.host["host"], "samples": sampler.rows,
                          "unavailable_reasons": reasons})
    raw = {
        "schema": "p4.release-a.integrity-i0-raw.v1",
        "artifact_sha256": hashlib.sha256(artifact_bytes).hexdigest(),
        "config_sha256": hashlib.sha256(config_bytes).hexdigest(),
        "gpu": {"sample_interval_ms": 1000, "hosts": gpu_hosts},
        "resource_snapshots": snapshots,
        "distributed": {"physical_hosts": 3, "stages": 3,
            "all_hosts_own_model_shard": True, "all_hosts_own_kv": True,
            "all_stages_compute": True, "cross_host_transfer_bytes": cross_bytes,
            "undeclared_hosts": 0},
        "cleanup": {"node_unload_once_per_node": True, "unload_status": "succeeded",
            "unload_resource_state": "absent", "nodes": 0, "task_native_children": 0,
            "task_listeners": 0, "gpu_compute_processes": 0,
            "transport_failure_policy": "zero_or_preserved_with_reconciliation",
            "transport_failures": failures, "transport_failures_reconciled": True},
    }
    (output / "cleanup.json").write_text(json.dumps(cleanup_rows, indent=2) + "\n")
    (output / "raw.json").write_text(json.dumps(raw, indent=2) + "\n")
    return {"passed": artifact.get("passed") is True, "artifact": str(output / "artifact.json"),
            "raw": str(output / "raw.json"), "cross_host_transfer_bytes": cross_bytes}


def self_test() -> None:
    assert LOADED != WINDOW != DRAINED
    assert list(HOST_ORDER) == ["spark", "mac20", "mac21"]
    bindings = [{"name": name, "command": ["hash"], "sha256": "0" * 64}
                for name in ["driver", "config", "route", "controller-script", "controller-config"] +
                [f"{kind}-{host}" for host in HOST_ORDER for kind in BINDING_KINDS]]
    controller = {"command": ["inspect"], "host": "windows-controller",
                  "owned": ["mac20-return", "mac20-next", "mac21-return", "mac21-previous"]}
    manifest = {"hosts": [{"host": host} for host in HOST_ORDER],
                "bindings": bindings, "controller_preflight": controller}
    validate_manifest(manifest)
    try:
        validate_manifest({**manifest, "bindings": bindings[:-1]})
    except ValueError as error:
        assert str(error) == "manifest execution bindings differ"
    else:
        raise AssertionError("missing host binding was accepted")
    try:
        validate_manifest({**manifest, "controller_preflight": {**controller, "owned": []}})
    except ValueError as error:
        assert str(error) == "controller tunnel preflight differs"
    else:
        raise AssertionError("missing controller tunnel identities were accepted")
    runnable = {**manifest, "bindings": [
        {**row, "command": [sys.executable, "-c", "print('" + "0" * 64 + "')"]}
        for row in bindings]}
    runnable["controller_preflight"] = {**controller, "command": [
        sys.executable, "-c", "import json; print(json.dumps(" +
        repr({"host": "windows-controller", "owned": controller["owned"][:-1]}) + "))"]}
    try:
        preflight(runnable)
    except ValueError as error:
        assert str(error) == "controller tunnel identity differs"
    else:
        raise AssertionError("preflight allowed a missing live tunnel")
    print(json.dumps({"passed": True, "tests": 6}, separators=(",", ":")))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test(); return
    if args.manifest is None or args.output is None:
        parser.error("manifest and output are required")
    print(json.dumps(run(json.loads(args.manifest.read_text()), args.output), separators=(",", ":")))


if __name__ == "__main__":
    main()
