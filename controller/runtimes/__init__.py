"""Lazy runtime driver registry.

Selecting llama RPC never imports the ring proxy implementation and vice versa.
"""

from importlib import import_module
from typing import Any, Iterable, Mapping

from controller.runtimes.base import (
    DEFAULT_RUNTIME_MODE,
    LLAMA_RPC,
    RING_PROXY,
    RUNTIME_MODES,
    normalize_runtime_mode,
)


def _driver(mode: object):
    selected = normalize_runtime_mode(mode)
    module = "controller.runtimes.llama_rpc" if selected == LLAMA_RPC else "controller.proxy.runtime"
    return import_module(module)


async def serve(mode: object, controller, request, plan, active):
    driver = _driver(mode)
    if not hasattr(driver, "serve"):
        raise RuntimeError(f"runtime {normalize_runtime_mode(mode)} cannot serve models")
    return await driver.serve(controller, request, plan, active)


async def serve_background(mode: object, controller, request, plan, active):
    """Run a non-RPC driver without leaking task lifecycle into the hub."""
    try:
        return await serve(mode, controller, request, plan, active)
    except Exception as exc:
        controller.update(phase="error", detail=str(exc))
        return None
    finally:
        controller.pop("load_cancel", None)
        controller.pop("load_task", None)
        controller.pop("pending_load", None)


async def unload(mode: object, controller, reason: str):
    driver = _driver(mode)
    if not hasattr(driver, "unload"):
        raise RuntimeError(f"runtime {normalize_runtime_mode(mode)} cannot unload models")
    return await driver.unload(controller, reason)


async def chat_completion(mode: object, controller, body, request_id=None):
    driver = _driver(mode)
    if not hasattr(driver, "chat_completion"):
        raise RuntimeError(f"runtime {normalize_runtime_mode(mode)} has no chat adapter")
    return await driver.chat_completion(controller, body, request_id=request_id)


async def stream_chat_completion(mode: object, controller, body, request_id=None):
    driver = _driver(mode)
    if not hasattr(driver, "stream_chat_completion"):
        raise RuntimeError(f"runtime {normalize_runtime_mode(mode)} has no streaming chat adapter")
    async for chunk in driver.stream_chat_completion(controller, body, request_id=request_id):
        yield chunk


def data_plane_contract(mode: object, topology: Iterable[Mapping[str, Any]]) -> dict[str, Any]:
    return _driver(mode).data_plane_contract(topology)


def enrich_topology_item(mode: object, node: Mapping[str, Any], item: Mapping[str, Any]) -> dict[str, Any]:
    driver = _driver(mode)
    if hasattr(driver, "enrich_topology_item"):
        return driver.enrich_topology_item(node, item)
    return dict(item)


def runtime_mode_catalog() -> list[dict[str, Any]]:
    return [_driver(mode).catalog_entry() for mode in RUNTIME_MODES]


__all__ = [
    "DEFAULT_RUNTIME_MODE", "LLAMA_RPC", "RING_PROXY", "RUNTIME_MODES",
    "chat_completion", "stream_chat_completion", "data_plane_contract", "enrich_topology_item", "normalize_runtime_mode",
    "runtime_mode_catalog", "serve", "serve_background", "unload",
]
