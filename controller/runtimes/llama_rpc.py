"""Stock llama.cpp master/worker RPC runtime contract.

This module must not import any linkcpp ring adapter or stage protocol code.
"""

from __future__ import annotations

from typing import Any, Iterable, Mapping

from controller.runtimes.base import LLAMA_RPC


def data_plane_contract(topology: Iterable[Mapping[str, Any]]) -> dict[str, Any]:
    nodes = list(topology)
    return {
        "mode": LLAMA_RPC,
        "protocol": "llama.cpp-rpc",
        "topology": "master_star",
        "weight_source": "master_stream_or_rpc_cache",
        "inference_payload": "rpc_graph_and_tensors",
        "links": [
            {"master": "llama-server", "node_id": item.get("node_id"),
             "endpoint": item.get("rpc_endpoint")}
            for item in nodes
        ],
        "available": True,
    }


def catalog_entry() -> dict[str, Any]:
    return {
        "id": LLAMA_RPC,
        "label": "llama.cpp RPC",
        "stability": "stable",
        "available": True,
        "description": "Stock llama-server master with ggml-rpc-server workers.",
    }
