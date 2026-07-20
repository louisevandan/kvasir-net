#!/usr/bin/env python3
"""Host and accelerator discovery shared by hub and managed node agents."""
import ctypes
import json
import os
import platform
import shutil
import socket
import subprocess
from typing import Any, Dict, List, Optional

from controller import device_profiles

_CPU_PREV: Optional[tuple[int, int]] = None


def _run(args: List[str], timeout: int = 8) -> str:
    return subprocess.check_output(args, text=True, stderr=subprocess.DEVNULL, timeout=timeout).strip()


def _gib(value: float) -> float:
    return round(value / (1024 ** 3), 2)


def host_platform() -> Dict[str, str]:
    return {
        "system": platform.system().lower() or "unknown",
        "release": platform.release(),
        "machine": platform.machine(),
        "hostname": socket.gethostname(),
    }


def _nvidia_gpus() -> List[Dict[str, Any]]:
    try:
        out = _run([
            "nvidia-smi",
            "--query-gpu=uuid,name,memory.total,memory.used",
            "--format=csv,noheader,nounits",
        ])
    except Exception:
        return []
    system_memory_gib = _memory_info()["total"]
    configured_vram_gib = float(os.environ.get("LINKCPP_VRAM_TOTAL", 0) or 0)
    gpus = []
    for row in out.splitlines():
        parts = [x.strip() for x in row.split(",")]
        if len(parts) >= 4:
            gpus.append(device_profiles.cuda_device(
                parts[0], parts[1], parts[2], parts[3], system_memory_gib, configured_vram_gib,
            ))
    return gpus


def _amd_gpus() -> List[Dict[str, Any]]:
    """AMD ROCm GPUs via rocm-smi JSON (total + used VRAM per card). Without this the
    ROCm path reported vram=0, starving the hub's load gate and the planner."""
    for tool in ("rocm-smi", "/opt/rocm/bin/rocm-smi"):
        if not (shutil.which(tool) or os.path.exists(tool)):
            continue
        try:
            data = json.loads(_run([tool, "--showmeminfo", "vram", "--json"]))
        except Exception:
            continue
        gpus = []
        for card, info in (data.items() if isinstance(data, dict) else []):
            total = info.get("VRAM Total Memory (B)")
            if total is None:
                continue
            try:
                total_b = int(total)
                used_b = int(info.get("VRAM Total Used Memory (B)") or 0)
            except (TypeError, ValueError):
                continue
            gpus.append({
                "uuid": card,  # rocm-smi has no stable per-card uuid here; card key is the id
                "name": os.environ.get("LINKCPP_BACKEND_DEVICE", "").strip() or "AMD ROCm",
                "vram_gib": _gib(total_b),
                "used_gib": _gib(used_b),
                "backend_kind": "rocm",
            })
        if gpus:
            return gpus
    return []


def _amd_primary() -> Optional[Dict[str, Any]]:
    """The single AMD card this node represents. Honors an explicit device pin
    (ROCR_/HIP_VISIBLE_DEVICES); otherwise, since the node advertises ONE device on a
    multi-GPU box and the RPC worker binds one card, reports the busiest card so the
    reported VRAM headroom reflects the card the model actually loads onto."""
    gpus = _amd_gpus()
    if not gpus:
        return None
    pin = (os.environ.get("ROCR_VISIBLE_DEVICES") or os.environ.get("HIP_VISIBLE_DEVICES") or "").split(",")[0].strip()
    if pin.isdigit() and 0 <= int(pin) < len(gpus):
        return gpus[int(pin)]
    return max(gpus, key=lambda g: g["used_gib"])


def rocm_process_vram_gib(pid) -> Optional[float]:
    """VRAM (GiB) a specific process uses on AMD GPUs, via rocm-smi's per-process
    view. This is the reliable way to report a node's real VRAM use: HIP and
    rocm-smi can enumerate devices in different orders, so mapping a
    HIP_VISIBLE_DEVICES pin to a rocm-smi card index (see _amd_primary) can point at
    the wrong card. Attributing by the worker's own PID avoids that entirely.
    Returns None if rocm-smi is unavailable; 0.0 if the pid holds no VRAM."""
    if not pid:
        return None
    for tool in ("rocm-smi", "/opt/rocm/bin/rocm-smi"):
        if not (shutil.which(tool) or os.path.exists(tool)):
            continue
        try:
            data = json.loads(_run([tool, "--showpids", "--json"]))
        except Exception:
            continue
        procs = data.get("system") if isinstance(data, dict) else None
        if not isinstance(procs, dict):
            continue
        entry = procs.get(f"PID{pid}")
        if entry is None:
            return 0.0
        # rocm-smi encodes the row as "<name>, <gpu_count>, <vram_bytes>, <sdma>, <cu>"
        parts = [p.strip() for p in str(entry).split(",")]
        try:
            return _gib(int(parts[2]))
        except (ValueError, IndexError):
            return None
    return None


def _darwin_metal_device() -> Optional[Dict[str, Any]]:
    if platform.system() != "Darwin":
        return None
    name = os.environ.get("LINKCPP_BACKEND_DEVICE", "").strip()
    if not name:
        try:
            data = json.loads(_run(["system_profiler", "SPDisplaysDataType", "-json"], timeout=15))
            displays = data.get("SPDisplaysDataType") or []
            for item in displays:
                candidate = item.get("sppci_model") or item.get("sppci_device_type")
                if candidate:
                    name = str(candidate)
                    break
        except Exception:
            pass
    return device_profiles.metal_device(
        os.environ.get("LINKCPP_ACCELERATOR_ID", "metal0"), name, _memory_info()["total"],
        float(os.environ.get("LINKCPP_VRAM_TOTAL", 0) or 0),
    )


def accelerator() -> Dict[str, Any]:
    requested = os.environ.get("LINKCPP_ACCELERATOR_ID") or os.environ.get("CUDA_VISIBLE_DEVICES", "")
    for gpu in _nvidia_gpus():
        if not requested or requested in (gpu["uuid"], gpu["name"], "all"):
            return gpu
    amd = _amd_primary()
    if amd:
        return amd
    metal = _darwin_metal_device()
    if metal:
        return metal
    backend = os.environ.get("LINKCPP_LLAMA_CPP_BACKEND", "").strip().lower() or "cpu"
    return {
        "uuid": os.environ.get("LINKCPP_ACCELERATOR_ID", ""),
        "name": os.environ.get("LINKCPP_BACKEND_DEVICE", "").strip() or backend.upper(),
        "vram_gib": float(os.environ.get("LINKCPP_VRAM_TOTAL", 0) or 0),
        "used_gib": 0.0,
        "backend_kind": backend,
    }


def local_gpus() -> List[Dict[str, Any]]:
    gpus = _nvidia_gpus()
    if gpus:
        return [{
            "uuid": g["uuid"],
            "name": g["name"],
            "vram_total_gib": round(g["vram_gib"], 1),
            "backend_kind": g["backend_kind"],
        } for g in gpus]
    amd = _amd_primary()
    if amd:
        # The node advertises one device slot; report the representative card.
        return [{
            "uuid": amd["uuid"],
            "name": amd["name"],
            "vram_total_gib": round(amd["vram_gib"], 1),
            "backend_kind": "rocm",
        }]
    metal = _darwin_metal_device()
    if metal:
        return [{
            "uuid": metal["uuid"],
            "name": metal["name"],
            "vram_total_gib": round(metal["vram_gib"], 1),
            "backend_kind": "metal",
        }]
    return []


def _memory_info() -> Dict[str, float]:
    vals: Dict[str, int] = {}
    try:
        with open("/proc/meminfo", encoding="utf-8") as f:
            for line in f:
                key, _, rest = line.partition(":")
                vals[key] = int(rest.split()[0])
        total = vals.get("MemTotal", 0) * 1024
        avail = vals.get("MemAvailable", 0) * 1024
        return {"total": _gib(total), "used": _gib(max(total - avail, 0))}
    except Exception:
        pass
    if platform.system() == "Darwin":
        try:
            total = int(_run(["sysctl", "-n", "hw.memsize"]))
            return {"total": _gib(total), "used": 0.0}
        except Exception:
            pass
    try:
        class MEMORYSTATUSEX(ctypes.Structure):
            _fields_ = [
                ("dwLength", ctypes.c_ulong),
                ("dwMemoryLoad", ctypes.c_ulong),
                ("ullTotalPhys", ctypes.c_ulonglong),
                ("ullAvailPhys", ctypes.c_ulonglong),
                ("ullTotalPageFile", ctypes.c_ulonglong),
                ("ullAvailPageFile", ctypes.c_ulonglong),
                ("ullTotalVirtual", ctypes.c_ulonglong),
                ("ullAvailVirtual", ctypes.c_ulonglong),
                ("ullAvailExtendedVirtual", ctypes.c_ulonglong),
            ]
        status = MEMORYSTATUSEX()
        status.dwLength = ctypes.sizeof(status)
        if ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(status)):
            total = status.ullTotalPhys
            avail = status.ullAvailPhys
            return {"total": _gib(total), "used": _gib(max(total - avail, 0))}
    except Exception:
        pass
    return {"total": 0.0, "used": 0.0}


def memory_info() -> Dict[str, float]:
    return _memory_info()


def cpu_percent() -> float:
    global _CPU_PREV
    try:
        with open("/proc/stat", encoding="utf-8") as f:
            parts = [int(x) for x in f.readline().split()[1:]]
        idle = parts[3] + (parts[4] if len(parts) > 4 else 0)
        total = sum(parts)
        if _CPU_PREV is None:
            _CPU_PREV = (idle, total)
            return 0.0
        prev_idle, prev_total = _CPU_PREV
        _CPU_PREV = (idle, total)
        total_delta = max(total - prev_total, 1)
        idle_delta = max(idle - prev_idle, 0)
        return round((1.0 - idle_delta / total_delta) * 100.0, 1)
    except Exception:
        return 0.0


def resource_snapshot(model_dir: str) -> Dict[str, float]:
    gpu = accelerator()
    mem = _memory_info()
    try:
        disk_free = shutil.disk_usage(model_dir).free / 1024 ** 3
    except Exception:
        disk_free = 0.0
    vram_budget = float(os.environ.get("LINKCPP_VRAM_BUDGET", 0)) or float(gpu["vram_gib"])
    ram_budget = float(os.environ.get("LINKCPP_RAM_BUDGET", 0)) or float(mem["total"])
    cores_total = os.cpu_count() or 0
    cores_budget = int(os.environ.get("LINKCPP_CORES", 0)) or cores_total
    return {
        "vram_total_gib": float(gpu["vram_gib"]),
        "vram_used_gib": float(gpu["used_gib"]),
        "vram_budget_gib": vram_budget,
        "ram_total_gib": mem["total"],
        "ram_used_gib": mem["used"],
        "ram_budget_gib": ram_budget,
        "cores_total": cores_total,
        "cores_budget": cores_budget,
        "cpu_used_percent": cpu_percent(),
        "disk_free_gib": round(disk_free, 2),
    }
