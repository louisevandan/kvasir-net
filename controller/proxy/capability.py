"""Runtime capability checks for the versioned adjacent-ring adapter."""

from __future__ import annotations

import json
import os
import subprocess
from functools import lru_cache
from typing import Any, Mapping


RING_PROTOCOL = "linkcpp-stage-v1"
# Must track LINKCPP_RING_ADAPTER_ABI in apps/linkcpp-ring-protocol.h.
# ABI 5 adds the NAT-traversal connection role preamble.
# ABI 6 adds the rank-local KV offload startup setting.
RING_ADAPTER_ABI = 6


def binary_info(path: str, *, env: Mapping[str, str] | None = None) -> dict[str, Any]:
    if not path or not os.path.isfile(path):
        return {"available": False, "path": path, "error": "binary is missing"}
    try:
        output = subprocess.check_output(
            [path, "--runtime-info"], text=True, stderr=subprocess.STDOUT, timeout=10,
            env=dict(env) if env is not None else None,
        ).strip()
        value = json.loads(output)
        if not isinstance(value, dict):
            raise ValueError("runtime info is not an object")
        return {"available": True, "path": path, **value}
    except Exception as exc:
        return {"available": False, "path": path, "error": str(exc)}


def compatibility(
    info: Mapping[str, Any] | None, *, expected_build_id: str | None = None,
) -> dict[str, Any]:
    actual = dict(info or {})
    mismatches = []
    if actual.get("protocol") != RING_PROTOCOL:
        mismatches.append({
            "field": "protocol", "expected": RING_PROTOCOL, "actual": actual.get("protocol"),
        })
    if int(actual.get("adapter_abi") or 0) != RING_ADAPTER_ABI:
        mismatches.append({
            "field": "adapter_abi", "expected": RING_ADAPTER_ABI,
            "actual": actual.get("adapter_abi"),
        })
    for field in ("state_snapshot", "chunked_state"):
        if actual.get(field) is not True:
            mismatches.append({"field": field, "expected": True, "actual": actual.get(field)})
    if expected_build_id and actual.get("build_id") != expected_build_id:
        mismatches.append({
            "field": "build_id", "expected": expected_build_id,
            "actual": actual.get("build_id"),
        })
    expected = {
        "protocol": RING_PROTOCOL,
        "adapter_abi": RING_ADAPTER_ABI,
        "state_snapshot": True,
        "chunked_state": True,
    }
    if expected_build_id:
        expected["build_id"] = expected_build_id
    return {
        "compatible": not mismatches,
        "expected": expected,
        "actual": actual or None,
        "mismatches": mismatches,
        "scope": "ring_adapter",
    }


def runtime_pair_info(
    stage_path: str, server_path: str, *, env: Mapping[str, str] | None = None,
) -> dict[str, Any]:
    stage = binary_info(stage_path, env=env)
    server = binary_info(server_path, env=env)
    stage_check = compatibility(stage if stage.get("available") else None)
    server_check = compatibility(server if server.get("available") else None)
    build_id = stage.get("build_id") if stage.get("available") else None
    same_build = bool(build_id and build_id != "unknown" and server.get("build_id") == build_id)
    available = bool(stage.get("available") and server.get("available")
                     and stage_check["compatible"] and server_check["compatible"]
                     and same_build)
    result = {
        "protocol": RING_PROTOCOL,
        "adapter_abi": RING_ADAPTER_ABI,
        "build_id": build_id,
        "available": available,
        "stage": stage,
        "server": server,
        "stage_compatibility": stage_check,
        "server_compatibility": server_check,
    }
    if not available:
        if stage.get("available") and server.get("available") and not same_build:
            result["blocker"] = "ring stage/server build IDs do not match"
        else:
            result["blocker"] = "matching linkcpp ring stage/server binaries are not installed"
    return result


@lru_cache(maxsize=1)
def local_runtime_info() -> dict[str, Any]:
    stage_path = os.environ.get("LINKCPP_STAGE_BIN", "/app/bin/linkcpp-node")
    server_path = os.environ.get("LINKCPP_SERVER_BIN", "/app/bin/linkcpp-server")
    return runtime_pair_info(stage_path, server_path)


def catalog_capability(entry: Mapping[str, Any] | None) -> dict[str, Any] | None:
    """Extract a ring mode capability returned by another unit's /api/runtime."""
    for mode in (entry or {}).get("runtime_modes", []) or []:
        if mode.get("id") == "ring_proxy":
            return dict(mode)
    return None
