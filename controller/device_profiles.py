"""Device-specific accelerator profiles used by host discovery and node setup.

Keep hardware memory semantics here so platform probing can remain independent
from CUDA, GB10 unified-memory, Metal, and future accelerator policies.
"""
from typing import Any, Dict


def mib_to_gib(value: str) -> float:
    """Convert an nvidia-smi MiB value, accepting unavailable reports."""
    try:
        return round(float(value) / 1024, 2)
    except (TypeError, ValueError):
        return 0.0


def is_gb10(name: str) -> bool:
    return "gb10" in (name or "").lower()


def cuda_vram_gib(name: str, memory_total_mib: str, system_memory_gib: float,
                  configured_vram_gib: float = 0.0) -> float:
    """Return the CUDA allocation budget for a device.

    A conventional CUDA device reports dedicated VRAM through ``nvidia-smi``.
    GB10 instead exposes unified system memory to CUDA and reports ``[N/A]``;
    its CUDA budget is therefore the configured limit or system-memory total.
    """
    dedicated_vram = mib_to_gib(memory_total_mib)
    if dedicated_vram or not is_gb10(name):
        return dedicated_vram
    return float(configured_vram_gib or 0.0) or float(system_memory_gib or 0.0)


def cuda_device(uuid: str, name: str, memory_total_mib: str, memory_used_mib: str,
                system_memory_gib: float, configured_vram_gib: float = 0.0) -> Dict[str, Any]:
    return {
        "uuid": uuid,
        "name": name,
        "vram_gib": cuda_vram_gib(name, memory_total_mib, system_memory_gib, configured_vram_gib),
        "used_gib": mib_to_gib(memory_used_mib),
        "backend_kind": "cuda",
    }


def metal_device(uuid: str, name: str, system_memory_gib: float,
                 configured_vram_gib: float = 0.0) -> Dict[str, Any]:
    """Build a Metal profile whose GPU budget is the unified-memory budget."""
    return {
        "uuid": uuid or "metal0",
        "name": name or "Apple Metal",
        "vram_gib": float(configured_vram_gib or 0.0) or float(system_memory_gib or 0.0),
        "used_gib": 0.0,
        "backend_kind": "metal",
    }
