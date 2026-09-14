"""Measure Qwen stage loading, live hybrid state and service in fresh processes."""

import json
from pathlib import Path
import platform
import subprocess
import sys
import time

from p4hfadapter.models.qwen3_5_0_8b.configuration import Node, parse_plan
from p4hfadapter.models.qwen3_5_0_8b.identity import MODEL_ID, REVISION, TORCH_VERSION, TRANSFORMERS_VERSION
from p4hfadapter.models.qwen3_5_0_8b.planning import (base_plan, fingerprint, profile_checkpoint_sha,
    profile_contract, source_identity, validate_request)


def peak_rss():
    if sys.platform == "win32":
        import ctypes
        from ctypes import wintypes
        class Counters(ctypes.Structure):
            _fields_ = [("cb", wintypes.DWORD), ("PageFaultCount", wintypes.DWORD)] + [
                (name, ctypes.c_size_t) for name in ("PeakWorkingSetSize", "WorkingSetSize", "QuotaPeakPagedPoolUsage",
                "QuotaPagedPoolUsage", "QuotaPeakNonPagedPoolUsage", "QuotaNonPagedPoolUsage", "PagefileUsage", "PeakPagefileUsage")]
        counters = Counters()
        counters.cb = ctypes.sizeof(counters)
        kernel, psapi = ctypes.WinDLL("kernel32", use_last_error=True), ctypes.WinDLL("psapi", use_last_error=True)
        kernel.GetCurrentProcess.restype = wintypes.HANDLE
        psapi.GetProcessMemoryInfo.argtypes = (wintypes.HANDLE, ctypes.POINTER(Counters), wintypes.DWORD)
        if not psapi.GetProcessMemoryInfo(kernel.GetCurrentProcess(), ctypes.byref(counters), counters.cb):
            raise ctypes.WinError(ctypes.get_last_error())
        return counters.PeakWorkingSetSize
    import resource
    return resource.getrusage(resource.RUSAGE_SELF).ru_maxrss * (1 if sys.platform == "darwin" else 1024)


def measure_stage(root, directory, request, device_id, start, end):
    """Execute the real partial loader and StageSessions; keep all requested caches live."""
    _, devices, _ = validate_request(request)
    device = devices[device_id]
    if not device["enabled"] or start not in request["cuts"] or end not in request["cuts"] or start >= end:
        raise ValueError("invalid profiling device/range")
    import torch
    from p4hfadapter.models.qwen3_5_0_8b.evidence import verify_checkpoint
    from p4hfadapter.models.qwen3_5_0_8b.loading import load_stage, runtime_check
    from p4hfadapter.models.qwen3_5_0_8b.state import StageSessions, cache_bytes
    runtime_check()
    verify_checkpoint(root, directory)
    if device["device"].startswith("cuda"):
        torch.cuda.set_device(device["device"])
        torch.cuda.reset_peak_memory_stats(device["device"])
    node = Node("profile", device["host"], device["device"], start, end)
    plan = parse_plan(base_plan(request, [{"node_id": "profile", "host": "local", "device": "cpu", "layers": [0, 24]}]))
    stage, loading = load_stage(directory, node, request["dtype"])
    sessions = StageSessions(stage, plan)
    elapsed = 0.0
    try:
        for sequence in range(plan.max_requests):
            position, issue = 0, 0
            while position < plan.context:
                # End with a one-token cached decode; all preceding requests remain resident.
                chunk = min(request["prefill_chunk"], plan.context - 1 - position) if position < plan.context - 1 else 1
                shape = (1, chunk) if start == 0 else (1, chunk, 1024)
                tensor = torch.ones(shape, dtype=torch.int64 if start == 0 else torch.float32)
                if device["device"].startswith("cuda"):
                    torch.cuda.synchronize(device["device"])
                begin = time.perf_counter()
                result, _ = sessions.step(str(sequence), issue, position, tensor)
                if device["device"].startswith("cuda"):
                    torch.cuda.synchronize(device["device"])
                elapsed += time.perf_counter() - begin
                del result, tensor
                position += chunk
                issue += 1
        state_bytes = sum(cache_bytes(session.cache) for session in sessions.active.values())
        host_peak = peak_rss()
        device_peak = torch.cuda.max_memory_reserved(device["device"]) if device["device"].startswith("cuda") else host_peak
        sample = {"layers": [start, end], "weight_bytes": loading["weight_bytes"], "cache_bytes": state_bytes,
                  "host_peak_bytes": host_peak, "device_peak_bytes": device_peak, "service_ms": elapsed * 1000,
                  "tensor_names": loading["tensor_names"], "device_name": loading["device_name"],
                  "layer_types": stage.config.layer_types}
    finally:
        for name in list(sessions.active):
            sessions.release(name)
    sample["active_after_release"] = len(sessions.active)
    return sample


def create_profiles(root, directory, request, output, host, timeout=600):
    """Profile only the host explicitly assigned to this local machine; never contact peers."""
    _, devices, _ = validate_request(request)
    ids = sorted({slot["device_id"] for slot in request["slots"]
                  if devices[slot["device_id"]]["enabled"] and devices[slot["device_id"]]["host"] == host})
    if not ids:
        raise ValueError("no enabled slots on the requested profiling host")
    sources = source_identity()
    output.mkdir(parents=True, exist_ok=False)
    request_file = output / "request.json"
    request_file.write_text(json.dumps(request, indent=2) + "\n", encoding="utf-8")
    profiles = []
    for device_id in ids:
        profile = {"schema": "qwen3.5-0.8b-stage-profile-v1", "model_id": MODEL_ID, "model_revision": REVISION,
                   "contract": profile_contract(request), "source_sha256": sources,
                   "checkpoint_sha256": profile_checkpoint_sha(), "measurement_host": platform.node(),
                   "runtime": {"torch": TORCH_VERSION, "transformers": TRANSFORMERS_VERSION},
                   "binding": {key: devices[device_id][key] for key in ("id", "host", "device")}, "samples": [],
                   "scope": "synthetic tokens/hidden states, fresh process, full declared cache occupancy; not response quality/TPS"}
        for i, start in enumerate(request["cuts"][:-1]):
            for end in request["cuts"][i + 1:]:
                name = f"{device_id}-{start}-{end}"
                sample_file = output / (name + ".json")
                command = [sys.executable, "-B", str(root / "scripts/models/qwen3_5_0_8b/profiling/run.py"),
                           "--request", str(request_file.resolve()), "--model-dir", str(directory.resolve()),
                           "--device-id", device_id, "--start", str(start), "--end", str(end), "--output", str(sample_file.resolve())]
                run = subprocess.run(command, capture_output=True, timeout=timeout)
                (output / (name + ".log")).write_bytes(run.stdout + run.stderr)
                if run.returncode:
                    raise ValueError(f"stage profiling failed: {name}; see {output / (name + '.log')}")
                profile["samples"].append(json.loads(sample_file.read_text(encoding="utf-8")))
        profiles.append(profile)
    if source_identity() != sources:
        raise ValueError("source changed during profiling")
    (output / "profiles.json").write_text(json.dumps(profiles, indent=2) + "\n", encoding="utf-8")
    return {"profiles": str(output / "profiles.json"), "sha256": fingerprint(profiles), "devices": len(profiles)}
