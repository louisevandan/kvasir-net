"""Compatibility facade for the separated runtime driver registry."""

from controller.runtimes import (  # noqa: F401
    DEFAULT_RUNTIME_MODE,
    LLAMA_RPC,
    RING_PROXY,
    RUNTIME_MODES,
    data_plane_contract,
    enrich_topology_item,
    normalize_runtime_mode,
    runtime_mode_catalog,
    serve,
    serve_background,
    unload,
    chat_completion,
)

__all__ = [
    "DEFAULT_RUNTIME_MODE", "LLAMA_RPC", "RING_PROXY", "RUNTIME_MODES",
    "chat_completion", "data_plane_contract", "enrich_topology_item", "normalize_runtime_mode",
    "runtime_mode_catalog", "serve", "serve_background", "unload",
]
