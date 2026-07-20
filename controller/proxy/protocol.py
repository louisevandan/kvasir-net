"""Proxy-only control-plane request models.

These models deliberately do not live in ``controller.protocol`` so changes
to the experimental adapter cannot conflict with stable RPC/unit protocols.
"""

from __future__ import annotations

from typing import Any, Dict, List, Optional

from pydantic import BaseModel


class StageStartRequest(BaseModel):
    model: str
    layers: List[int]
    role: str
    listen_port: int
    next_endpoint: str
    op_id: Optional[str] = None
    gpu_layers: int = -1
    ctx: int = 4096
    parallel: int = 1
    batch: int = 0
    ubatch: int = 0
    cache_type_k: str = "f16"
    cache_type_v: str = "f16"
    kv_offload: bool = True
    cont_batching: bool = True
    coordinator: bool = False
    # A one-node controller runs a node-local llama-server coordinator without
    # creating a degenerate stage socket back to itself.  The node still owns
    # all model weights; the hub remains control-plane only.
    single_node: bool = False
    server_port: Optional[int] = None
    # P2-3 slot affinity: when >0, the coordinator's llama-server picks the slot
    # whose cached prompt shares the longest prefix with an incoming request, so
    # requests with a common system prompt reuse the KV prefix (free prefill,
    # smaller prefill dispatch). 0 = default round-robin slot selection.
    slot_prompt_similarity: float = 0.0
    # NAT traversal: a stage behind NAT dials both neighbours outbound, so it
    # dials its predecessor (dial_prev_endpoint) instead of accepting it; the
    # predecessor of a NAT'd stage accepts that edge (accept_next) instead of
    # dialing. Empty/false = the original wiring (dial next, accept prev).
    dial_prev_endpoint: str = ""
    accept_next: bool = False
    stage_manifest: Optional[Dict[str, Any]] = None
    # Comma-joined llama.cpp --override-tensor rules (e.g. "blk\.0\.ffn_..exps=CPU,...")
    # from the planner's per-stage placement, so a backbone stage keeps attention on
    # the GPU and streams its MoE expert FFNs from CPU RAM — the M0 path that lets a
    # large MoE fit a stage whose experts exceed its VRAM.
    ot: str = ""
    ring_protocol: Optional[str] = None
    ring_adapter_abi: Optional[int] = None
    ring_build_id: Optional[str] = None


class StageInferRequest(BaseModel):
    prompt: str = ""
    messages: Optional[List[Dict[str, str]]] = None
    max_tokens: int = 512
    request_id: Optional[int] = None


class RuntimePackInstallRequest(BaseModel):
    url: str
    sha256: str
    expected_build_id: Optional[str] = None


class UnloadRequest(BaseModel):
    op_id: Optional[str] = None
    reason: str = "requested"


def model_to_dict(model: BaseModel) -> Dict[str, Any]:
    if hasattr(model, "model_dump"):
        return model.model_dump()
    return model.dict()
