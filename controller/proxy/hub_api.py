"""FastAPI router for proxy-only hub control surfaces.

The stable hub imports only :func:`install`; all stage and runtime-pack logic
stays in the proxy package.
"""

from __future__ import annotations

import asyncio
import os
from pathlib import Path
from typing import Optional

from fastapi import APIRouter, HTTPException, Query, Request
from fastapi.responses import FileResponse, StreamingResponse

from controller.proxy import packs, stage_service
from controller.proxy.protocol import (
    RuntimePackInstallRequest,
    StageInferRequest,
    StageStartRequest,
    UnloadRequest,
    model_to_dict,
)


router = APIRouter(prefix="/api/proxy", tags=["proxy"])


def install(app) -> None:
    app.include_router(router)


def _hub():
    from controller import hub
    return hub


def _node(node_id: str):
    hub = _hub()
    hub._ensure_local_slots()
    node = hub.NODES.get(node_id)
    if not node:
        raise HTTPException(404, "unknown node")
    return hub, node


def _stage_log(node_id: str) -> str:
    hub = _hub()
    return os.path.join(hub.STAGE_DIR, "logs", f"{node_id}.log")


def _convert(exc: stage_service.StageError):
    raise HTTPException(exc.status_code, exc.detail) from exc


def start_local_stage(node: dict, request: StageStartRequest):
    hub = _hub()
    if not node.get("assigned", bool(node.get("gpu_uuid"))):
        raise HTTPException(409, "assign node resources before starting a stage")
    try:
        result = stage_service.start(
            node, request, model_dir=hub.MODEL_DIR, log_path=_stage_log(node["id"]),
            baked_stage=os.environ.get("LINKCPP_STAGE_BIN", "/app/bin/linkcpp-node"),
            baked_server=os.environ.get("LINKCPP_SERVER_BIN", "/app/bin/linkcpp-server"),
            gpu_uuid=str(node.get("gpu_uuid") or ""),
            stop_worker=lambda: hub._kill(node.get("worker")),
        )
    except stage_service.StageError as exc:
        _convert(exc)
    node["worker"] = None
    node["worker_running"] = False
    hub._persist_hub_state()
    return {**result, "node_id": node["id"]}


async def infer_local_stage(node: dict, request: StageInferRequest):
    try:
        return await stage_service.run_inference(node, request)
    except stage_service.StageError as exc:
        _convert(exc)


@router.get("/models/{name}/stage")
def stage_model_gguf(name: str, request: Request, layers: str = Query(...),
                     service_token: str = Query("")):
    """Serve a mini-GGUF with only the tensors a stage owning `layers=A:B` needs,
    so a node downloads its layer window instead of the whole model. Cached by
    (model, layers). Auth mirrors the hub's /api/models/{name}/file.

    Consuming this needs the rank-local loader to skip loading the stripped
    out-of-window layer tensors — see docs/request-partial-shard-loader.md."""
    from controller.proxy.manifest import write_stage_gguf
    hub = _hub()
    if hub.AUTH_ENABLED:
        st = request.headers.get("x-linkcpp-service-token", "") or service_token
        ok = bool(hub.SERVICE_TOKEN and st and hub._hmac.compare_digest(st, hub.SERVICE_TOKEN))
        # A node-scoped token (wallet signature, no 2FA) may pull its own shard
        # window — the gate already lists /api/proxy/models/ in _NODE_TOKEN_PREFIXES,
        # and a self-enrolled NAT node downloads over a plain bearer, not the M2M
        # service token. Mirror that here so the handler check matches the gate.
        if not ok and not hub._authed_wallet(request) and not hub._authed_node(request):
            raise HTTPException(401, "authentication required")
    try:
        start, end = (int(v) for v in layers.split(":"))
    except ValueError:
        raise HTTPException(400, "layers must be A:B")
    if not (0 <= start < end):
        raise HTTPException(400, "invalid layer range")
    src, _meta = hub._resolve_model(name)
    base = os.path.splitext(os.path.basename(src))[0]
    cache_dir = os.path.join(hub.DATA_DIR, "stage-gguf")
    os.makedirs(cache_dir, exist_ok=True)
    dst = os.path.join(cache_dir, f"{base}.s{start}-{end}.gguf")
    if not os.path.exists(dst):
        try:
            summary = write_stage_gguf(src, dst, start, end)
        except Exception as exc:
            if os.path.exists(dst):
                os.remove(dst)
            raise HTTPException(500, f"stage gguf build failed: {exc}") from exc
        hub._log_event("stage_gguf_built", model=name, **summary)
    return FileResponse(dst, filename=os.path.basename(dst),
                        media_type="application/octet-stream")


@router.get("/models/{name}/expert-shard")
def expert_shard_gguf(name: str, request: Request, layers: str = Query(...),
                      experts: str = Query(...), service_token: str = Query("")):
    """Serve a mini-GGUF with only the routed-expert slabs an expert worker owns:
    experts=A:B of layers=C:D (or a comma list). Cached by (model, layers, experts).
    Auth mirrors the stage window download — service token, full session, or a
    wallet node-token (the worker's own participation credential)."""
    from controller.proxy.manifest import write_expert_shard_gguf
    hub = _hub()
    if hub.AUTH_ENABLED:
        st = request.headers.get("x-linkcpp-service-token", "") or service_token
        ok = bool(hub.SERVICE_TOKEN and st and hub._hmac.compare_digest(st, hub.SERVICE_TOKEN))
        if not ok and not hub._authed_wallet(request) and not hub._authed_node(request):
            raise HTTPException(401, "authentication required")
    try:
        e0, e1 = (int(v) for v in experts.split(":"))
    except ValueError:
        raise HTTPException(400, "experts must be A:B")
    if not (0 <= e0 < e1):
        raise HTTPException(400, "invalid expert range")
    try:
        if ":" in layers:
            a, b = (int(v) for v in layers.split(":"))
            layer_list = list(range(a, b))
        else:
            layer_list = [int(v) for v in layers.split(",") if v != ""]
    except ValueError:
        raise HTTPException(400, "layers must be A:B or a comma list")
    if not layer_list:
        raise HTTPException(400, "no layers requested")
    src, _meta = hub._resolve_model(name)
    base = os.path.splitext(os.path.basename(src))[0]
    lspec = f"{layer_list[0]}-{layer_list[-1]}" if layer_list == list(range(layer_list[0], layer_list[-1] + 1)) \
        else "-".join(str(x) for x in layer_list)
    cache_dir = os.path.join(hub.DATA_DIR, "expert-shard")
    os.makedirs(cache_dir, exist_ok=True)
    dst = os.path.join(cache_dir, f"{base}.L{lspec}.e{e0}-{e1}.gguf")
    if not os.path.exists(dst):
        try:
            summary = write_expert_shard_gguf(src, dst, layer_list, e0, e1)
        except Exception as exc:
            if os.path.exists(dst):
                os.remove(dst)
            raise HTTPException(500, f"expert shard build failed: {exc}") from exc
        hub._log_event("expert_shard_built", model=name, **summary)
    return FileResponse(dst, filename=os.path.basename(dst),
                        media_type="application/octet-stream")


@router.get("/runtime")
def runtime_info():
    from controller.proxy.runtime import catalog_entry
    return {
        "runtime_mode": catalog_entry(),
        "installed_packs": packs.list_packs(),
        "install_enabled": packs.install_enabled(),
    }


@router.get("/runtime/packs")
def runtime_packs():
    return {
        "installed": packs.list_packs(),
        "bundled": packs.bundled_artifacts(),
        "install_enabled": packs.install_enabled(),
    }


@router.get("/runtime/packs/artifacts/{filename}")
def runtime_pack_artifact(filename: str):
    artifact = next((item for item in packs.bundled_artifacts() if item["filename"] == filename), None)
    if artifact is None:
        raise HTTPException(404, "bundled runtime pack not found")
    return FileResponse(artifact["path"], media_type="application/gzip", filename=artifact["filename"])


@router.post("/runtime/packs/install")
async def install_runtime_pack(request: RuntimePackInstallRequest):
    if not packs.install_enabled():
        raise HTTPException(403, "runtime-pack installation is disabled")
    try:
        return await asyncio.to_thread(
            packs.download_and_install, request.url, request.sha256,
            expected_build_id=request.expected_build_id,
        )
    except (OSError, ValueError) as exc:
        raise HTTPException(409, str(exc)) from exc


@router.put("/runtime/packs/install/{archive_sha256}")
async def upload_runtime_pack(
    archive_sha256: str, request: Request, expected_build_id: Optional[str] = Query(None),
):
    if not packs.install_enabled():
        raise HTTPException(403, "runtime-pack installation is disabled")
    try:
        return await packs.stream_and_install(
            request.stream(), archive_sha256, expected_build_id=expected_build_id,
        )
    except (OSError, ValueError) as exc:
        raise HTTPException(409, str(exc)) from exc


@router.post("/nodes/{node_id}/stage/start")
async def start_stage(node_id: str, request: StageStartRequest):
    hub, node = _node(node_id)
    if node.get("kind", "local") == "local":
        return start_local_stage(node, request)
    if node.get("kind") == "agent":
        return await hub._agent_request(
            node, "POST", "/control/proxy/stage/start", model_to_dict(request), timeout=60,
        )
    raise HTTPException(409, "node does not expose a proxy stage runner")


@router.get("/nodes/{node_id}/stage/status")
async def stage_status(node_id: str):
    hub, node = _node(node_id)
    if node.get("kind", "local") == "local":
        return {"node_id": node_id, **stage_service.status(node, _stage_log(node_id))}
    if node.get("kind") == "agent":
        return await hub._agent_request(node, "GET", "/control/proxy/stage/status", timeout=15)
    raise HTTPException(409, "node does not expose a proxy stage runner")


@router.post("/nodes/{node_id}/stage/infer")
async def stage_infer(node_id: str, request: StageInferRequest):
    hub, node = _node(node_id)
    if node.get("kind", "local") == "local":
        return await infer_local_stage(node, request)
    if node.get("kind") == "agent":
        return await hub._agent_request(
            node, "POST", "/control/proxy/stage/infer", model_to_dict(request), timeout=None,
        )
    raise HTTPException(409, "node does not expose a proxy stage runner")


@router.post("/nodes/{node_id}/stage/chat")
async def stage_chat(node_id: str, request: Request):
    hub, node = _node(node_id)
    body = await request.json()
    if node.get("kind", "local") == "local":
        return await stage_service.run_chat(node, body)
    if node.get("kind") == "agent":
        return await hub._agent_request(node, "POST", "/control/proxy/stage/chat", body, timeout=None)
    raise HTTPException(409, "node does not expose a proxy stage runner")


@router.post("/nodes/{node_id}/stage/chat/stream")
async def stage_chat_stream(node_id: str, request: Request):
    hub, node = _node(node_id)
    body = await request.json()
    kind = node.get("kind", "local")

    async def gen():
        if kind == "local":
            async for chunk in stage_service.run_chat_stream(node, body):
                yield chunk
        elif kind == "agent":
            async for chunk in hub._agent_request_stream(node, "POST", "/control/proxy/stage/chat/stream", body):
                yield chunk

    if kind in ("local", "agent"):
        return StreamingResponse(gen(), media_type="text/event-stream")
    raise HTTPException(409, "node does not expose a proxy stage runner")


@router.post("/nodes/{node_id}/stage/stop")
async def stop_stage(node_id: str, request: Optional[UnloadRequest] = None):
    hub, node = _node(node_id)
    request = request or UnloadRequest()
    if node.get("kind", "local") == "local":
        result = stage_service.stop(node, request.reason)
        hub._persist_hub_state()
        return {**result, "node_id": node_id}
    if node.get("kind") == "agent":
        return await hub._agent_request(
            node, "POST", "/control/proxy/stage/stop", model_to_dict(request), timeout=15,
        )
    raise HTTPException(409, "node does not expose a proxy stage runner")
