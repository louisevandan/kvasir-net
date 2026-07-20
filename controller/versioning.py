#!/usr/bin/env python3
"""Runtime and backend identity checks for linkcpp units."""
import os
import subprocess
from functools import lru_cache
from typing import Any, Dict, Iterable, List, Optional

from controller.release import RELEASE_VERSION

UNIT_VERSION = RELEASE_VERSION
RUNTIME_PACK_VERSION = os.environ.get("LINKCPP_RUNTIME_PACK_VERSION", RELEASE_VERSION).strip() or RELEASE_VERSION
RPC_ABI = os.environ.get("LINKCPP_RPC_ABI", "llama.cpp-rpc").strip() or "llama.cpp-rpc"


def _default_backend_kind() -> str:
    configured = os.environ.get("LINKCPP_LLAMA_CPP_BACKEND", "cuda").strip().lower() or "cuda"
    if configured != "auto":
        return configured
    try:
        from controller import host_resources
        return str(host_resources.accelerator().get("backend_kind") or "cpu").lower()
    except Exception:
        return "cpu"


DEFAULT_BACKEND_KIND = _default_backend_kind()


def _repo_root() -> str:
    return os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))


@lru_cache(maxsize=1)
def _llama_cpp_version() -> str:
    configured = os.environ.get("LINKCPP_LLAMA_CPP_VERSION", "").strip()
    if configured:
        return configured
    llama_dir = os.path.join(_repo_root(), "external", "llama.cpp")
    try:
        out = subprocess.check_output(
            ["git", "-C", llama_dir, "rev-parse", "--short=12", "HEAD"],
            text=True,
            stderr=subprocess.DEVNULL,
        ).strip()
        if out:
            return out
    except Exception:
        pass
    # Runtime images contain only the binaries, not the llama.cpp checkout.
    # Dockerfiles persist the build revision beside the controller package so
    # a native Metal agent can still pass the protocol compatibility check.
    try:
        with open(os.path.join(_repo_root(), "llama-cpp-version"), encoding="utf-8") as f:
            version = f.read().strip()
        if version:
            return version
    except OSError:
        pass
    return "unknown"


def runtime_identity() -> Dict[str, str]:
    return {
        "unit_version": UNIT_VERSION,
        "runtime_pack_version": RUNTIME_PACK_VERSION,
        "llama_cpp_version": _llama_cpp_version(),
        "rpc_abi": RPC_ABI,
        # Legacy field kept so older callers can still render a backend label.
        "llama_cpp_backend": _default_backend_kind(),
    }


def backend_identity() -> Dict[str, str]:
    return {
        "backend_kind": _default_backend_kind(),
        "backend_runtime_version": os.environ.get("LINKCPP_BACKEND_RUNTIME_VERSION", "").strip(),
        "backend_driver_version": os.environ.get("LINKCPP_BACKEND_DRIVER_VERSION", "").strip(),
        "backend_device": os.environ.get("LINKCPP_BACKEND_DEVICE", "").strip(),
        "backend_pack_version": os.environ.get("LINKCPP_BACKEND_PACK_VERSION", "").strip(),
    }


def runtime_label(identity: Optional[Dict[str, Any]] = None) -> str:
    ident = identity or runtime_identity()
    return (
        f"unit {ident.get('unit_version', 'unknown')} / "
        f"runtime pack {ident.get('runtime_pack_version', ident.get('unit_version', 'unknown'))} / "
        f"llama.cpp {ident.get('llama_cpp_version', 'unknown')} / "
        f"rpc {ident.get('rpc_abi', 'unknown')}"
    )


def backend_label(identity: Optional[Dict[str, Any]] = None) -> str:
    ident = identity or backend_identity()
    kind = ident.get("backend_kind") or ident.get("llama_cpp_backend") or "unknown"
    runtime = ident.get("backend_runtime_version") or ident.get("runtime_version") or ""
    driver = ident.get("backend_driver_version") or ident.get("driver_version") or ""
    parts = [str(kind)]
    if runtime:
        parts.append(f"runtime {runtime}")
    if driver:
        parts.append(f"driver {driver}")
    return " / ".join(parts)


def normalize_runtime(value: Optional[Dict[str, Any]]) -> Optional[Dict[str, str]]:
    if not value:
        return None
    return {
        "unit_version": str(value.get("unit_version", "") or ""),
        "runtime_pack_version": str(
            value.get("runtime_pack_version", value.get("unit_version", "")) or ""
        ),
        "llama_cpp_version": str(value.get("llama_cpp_version", "") or ""),
        "rpc_abi": str(value.get("rpc_abi", "") or ""),
        "llama_cpp_backend": str(value.get("llama_cpp_backend", "") or "").lower(),
    }


def normalize_backend(value: Optional[Dict[str, Any]]) -> Optional[Dict[str, str]]:
    if not value:
        return None
    return {
        "backend_kind": str(
            value.get("backend_kind", value.get("llama_cpp_backend", "")) or ""
        ).lower(),
        "backend_runtime_version": str(
            value.get("backend_runtime_version", value.get("runtime_version", "")) or ""
        ),
        "backend_driver_version": str(
            value.get("backend_driver_version", value.get("driver_version", "")) or ""
        ),
        "backend_device": str(value.get("backend_device", value.get("device", "")) or ""),
        "backend_pack_version": str(value.get("backend_pack_version", "") or ""),
    }


def backend_from_runtime(runtime: Optional[Dict[str, Any]]) -> Optional[Dict[str, str]]:
    if not runtime:
        return None
    kind = str(runtime.get("llama_cpp_backend", "") or "").lower()
    if not kind:
        return None
    return {"backend_kind": kind}


def compatibility_report(
    actual: Optional[Dict[str, Any]],
    expected: Optional[Dict[str, Any]] = None,
) -> Dict[str, Any]:
    exp = normalize_runtime(expected or runtime_identity())
    act = normalize_runtime(actual)
    # A unit version identifies the controller package that happens to be
    # deployed on a host; it is not a wire-protocol version.  Requiring every
    # remote unit to be rebuilt for each hub patch made a normal `git pull`
    # turn into a cluster-wide maintenance operation.  Keep the actual RPC
    # ABI as the hard compatibility gate and report package/pack drift as a
    # visible, non-blocking warning instead.
    hard_fields = ["rpc_abi"]
    advisory_fields = ["unit_version", "runtime_pack_version", "llama_cpp_version"]
    mismatches = []
    warnings = []
    if not act:
        mismatches.append({"field": "runtime", "expected": exp, "actual": None})
    else:
        for field in hard_fields:
            if act.get(field) != exp.get(field):
                mismatches.append({
                    "field": field,
                    "expected": exp.get(field),
                    "actual": act.get(field),
                })
        for field in advisory_fields:
            # Slim Docker runtime images intentionally omit the source Git
            # directory, so their llama.cpp revision can be unavailable.
            # The RPC ABI remains the hard contract; do not turn this
            # optional provenance value into a bind gate when one side
            # explicitly reports it as unknown.
            if field == "llama_cpp_version" and "unknown" in {
                act.get(field), exp.get(field)
            }:
                continue
            if act.get(field) != exp.get(field):
                warnings.append({
                    "field": field,
                    "expected": exp.get(field),
                    "actual": act.get(field),
                })
    return {
        "compatible": not mismatches,
        "expected": exp,
        "actual": act,
        "mismatches": mismatches,
        "warnings": warnings,
        "scope": "protocol",
    }


def backend_report(actual: Optional[Dict[str, Any]]) -> Dict[str, Any]:
    backend = normalize_backend(actual)
    warnings = []
    if not backend:
        warnings.append({"field": "backend", "message": "backend runtime not reported"})
    elif not backend.get("backend_kind"):
        warnings.append({"field": "backend_kind", "message": "backend kind not reported"})
    return {
        "compatible": True,
        "actual": backend,
        "warnings": warnings,
        "scope": "backend",
    }


def incompatible_nodes(nodes: Iterable[Dict[str, Any]]) -> List[Dict[str, Any]]:
    out = []
    for node in nodes:
        report = compatibility_report(node.get("runtime"))
        if not report["compatible"]:
            out.append({
                "id": node.get("id"),
                "name": node.get("name"),
                "kind": node.get("kind", "local"),
                "runtime": report,
            })
    return out
