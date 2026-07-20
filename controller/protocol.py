#!/usr/bin/env python3
"""Shared control-plane protocol models for managed linkcpp nodes."""
import time
from typing import Any, Dict, List, Optional

from pydantic import BaseModel, Field
from controller.release import RELEASE_VERSION


class ResourceSnapshot(BaseModel):
    vram_total_gib: float = 0.0
    vram_used_gib: float = 0.0
    vram_budget_gib: float = 0.0
    ram_total_gib: float = 0.0
    ram_used_gib: float = 0.0
    ram_budget_gib: float = 0.0
    cores_total: int = 0
    cores_budget: int = 0
    cpu_used_percent: float = 0.0
    disk_free_gib: float = 0.0


class RuntimeIdentity(BaseModel):
    unit_version: str = RELEASE_VERSION
    runtime_pack_version: str = RELEASE_VERSION
    llama_cpp_version: str = "unknown"
    rpc_abi: str = "unknown"
    llama_cpp_backend: str = "unknown"


class BackendRuntime(BaseModel):
    backend_kind: str = "unknown"
    backend_runtime_version: str = ""
    backend_driver_version: str = ""
    backend_device: str = ""
    backend_pack_version: str = ""


class ModelSource(BaseModel):
    kind: str = "direct_url"
    url: Optional[str] = None
    repo_id: Optional[str] = None
    filename: Optional[str] = None
    revision: str = "main"
    hf_token: Optional[str] = None
    sha256: Optional[str] = None
    size_bytes: Optional[int] = None

    def redacted(self) -> Dict[str, Any]:
        data = model_to_dict(self)
        if data.get("hf_token"):
            data["hf_token"] = "***"
        return data


class NodeOperation(BaseModel):
    op_id: str
    type: str
    status: str = "pending"
    phase: str = "pending"
    progress: float = 0.0
    model: Optional[str] = None
    message: str = ""
    error: Optional[str] = None
    created_at: float = Field(default_factory=time.time)
    updated_at: float = Field(default_factory=time.time)
    details: Dict[str, Any] = Field(default_factory=dict)


class NodeReport(BaseModel):
    node_id: str
    controller_id: Optional[str] = None
    op_id: Optional[str] = None
    op_type: Optional[str] = None
    phase: str = ""
    status: str = ""
    progress: float = 0.0
    message: str = ""
    resources: ResourceSnapshot = Field(default_factory=ResourceSnapshot)
    model: Optional[str] = None
    error: Optional[str] = None
    seq: int = 0
    ts: float = Field(default_factory=time.time)


class JoinRequest(BaseModel):
    controller_id: str
    report_url: Optional[str] = None
    name: str = ""
    # M2M token the agent echoes back on its node-reports so an auth-enabled hub
    # accepts them (the hub hands the agent its own service token on join).
    service_token: Optional[str] = None
    # Optional per-controller capacity contract. A native node keeps these
    # values in its persistent state and reports them to the hub planner.
    vram_budget_gib: Optional[float] = None
    ram_budget_gib: Optional[float] = None
    cores_budget: Optional[int] = None


class DownloadRequest(BaseModel):
    model: str
    source: ModelSource
    op_id: Optional[str] = None


class LoadMonitorRequest(BaseModel):
    controller_id: str
    op_id: str
    model: str
    report_node_id: Optional[str] = None
    layers: Optional[List[int]] = None
    planned_vram_gib: float = 0.0
    planned_ram_gib: float = 0.0
    report_url: Optional[str] = None
    interval_s: float = 2.0


class LoadRequest(BaseModel):
    model: str
    op_id: Optional[str] = None
    layers: Optional[List[int]] = None
    tensor_split: List[int] = Field(default_factory=list)
    offload: Optional[str] = None
    parallel: int = 1
    ctx: int = 4096
    kv_bits: int = 16
    cache_type_k: str = "f16"
    cache_type_v: str = "f16"


class UnloadRequest(BaseModel):
    op_id: Optional[str] = None
    reason: str = "requested"


class CancelLoadRequest(BaseModel):
    op_id: Optional[str] = None
    reason: str = "requested"


# Unit load sessions are the control-plane contract between controllers and
# remote units.  RPC ports remain data-plane-only: lifecycle, readiness,
# resources and diagnostics are reported by the owning unit over HTTP.
class UnitSessionNodeRequest(BaseModel):
    node_id: str
    layers: Optional[List[int]] = None
    planned_vram_gib: float = 0.0
    planned_ram_gib: float = 0.0


class UnitLoadSessionPrepareRequest(BaseModel):
    protocol_version: str = "unit-load-session/v1"
    session_id: str
    controller_id: str
    model: str
    nodes: List[UnitSessionNodeRequest] = Field(default_factory=list)
    diagnostics: bool = True


class UnitLoadSessionReleaseRequest(BaseModel):
    reason: str = "requested"


def model_to_dict(model: BaseModel) -> Dict[str, Any]:
    if hasattr(model, "model_dump"):
        return model.model_dump()
    return model.dict()
