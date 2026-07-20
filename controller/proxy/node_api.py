"""Proxy-only routes installed into the native managed node agent."""

from __future__ import annotations

import json
import os
from typing import Optional

from fastapi import APIRouter, HTTPException, Query, Request
from fastapi.responses import StreamingResponse

from controller.proxy import packs, stage_service
from controller.proxy.protocol import StageInferRequest, StageStartRequest, UnloadRequest


router = APIRouter(prefix="/control/proxy", tags=["proxy"])
_stage_owner = {}


def _stage_log() -> str:
    return os.environ.get("LINKCPP_STAGE_LOG", "/tmp/linkcpp-stage.log")


def install(app) -> None:
    app.include_router(router)


def _host():
    from controller import nodeagent
    return nodeagent


def _convert(exc: stage_service.StageError):
    raise HTTPException(exc.status_code, exc.detail) from exc


@router.get("/runtime")
async def runtime_info():
    # A managed node can receive a new native build in place.  Do not report a
    # failed probe cached before that replacement; the hub uses this endpoint
    # as the compatibility gate before it asks the node to start a stage.
    from controller.proxy.capability import local_runtime_info
    from controller.proxy.runtime import catalog_entry
    local_runtime_info.cache_clear()
    return {
        "runtime_mode": catalog_entry(),
        "installed_packs": packs.list_packs(),
        "install_enabled": packs.install_enabled(),
    }


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


@router.post("/stage/start")
async def start_stage(request: StageStartRequest):
    host = _host()
    try:
        result = stage_service.start(
            _stage_owner, request, model_dir=host.MODEL_DIR, log_path=_stage_log(),
            baked_stage=os.environ.get("LINKCPP_STAGE_BIN", "/app/bin/linkcpp-node"),
            baked_server=os.environ.get("LINKCPP_SERVER_BIN", "/app/bin/linkcpp-server"),
            stop_worker=host._stop_worker,
        )
    except stage_service.StageError as exc:
        _convert(exc)
    return {**result, "status": stage_service.status(_stage_owner, _stage_log())}


@router.get("/stage/status")
def status():
    return stage_service.status(_stage_owner, _stage_log())


@router.post("/stage/infer")
async def infer(request: StageInferRequest):
    try:
        return await stage_service.run_inference(_stage_owner, request)
    except stage_service.StageError as exc:
        _convert(exc)


@router.post("/stage/chat")
async def stage_chat(request: Request):
    """Native OpenAI chat passthrough to the local coordinator (tools support)."""
    body = await request.json()
    try:
        return await stage_service.run_chat(_stage_owner, body)
    except stage_service.StageError as exc:
        _convert(exc)


@router.post("/stage/chat/stream")
async def stage_chat_stream(request: Request):
    """Streaming (SSE) OpenAI chat passthrough relayed from the local coordinator."""
    body = await request.json()

    async def gen():
        try:
            async for chunk in stage_service.run_chat_stream(_stage_owner, body):
                yield chunk
        except stage_service.StageError as exc:
            yield ("data: " + json.dumps({"error": {"message": str(exc)}}) + "\n\n").encode()

    return StreamingResponse(gen(), media_type="text/event-stream")


@router.post("/stage/stop")
def stop(request: Optional[UnloadRequest] = None):
    request = request or UnloadRequest()
    result = stage_service.stop(_stage_owner, request.reason)
    return {**result, "status": stage_service.status(_stage_owner, _stage_log())}
