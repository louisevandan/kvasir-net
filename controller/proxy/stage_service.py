"""Process lifecycle for one proxy stage, independent of hub/node-agent code."""

from __future__ import annotations

import asyncio
import os
import subprocess
from typing import Any, Callable, MutableMapping

import httpx

from controller.proxy import packs
from controller.proxy.manifest import summarize_stage_manifest, validate_stage_manifest
from controller.proxy.protocol import StageInferRequest, StageStartRequest
from controller.proxy.server import infer


class StageError(RuntimeError):
    def __init__(self, status_code: int, detail: str):
        super().__init__(detail)
        self.status_code = status_code
        self.detail = detail


def _kill(process) -> None:
    if not process or process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=8)
    except subprocess.TimeoutExpired:
        process.kill()


def _model_path(model_dir: str, model: str) -> str:
    normalized = str(model or "").replace("\\", "/").lstrip("/")
    path = os.path.abspath(os.path.join(model_dir, normalized))
    root = os.path.abspath(model_dir)
    if os.path.commonpath((root, path)) != root:
        raise StageError(400, "model path escapes model directory")
    return path


def start(
    owner: MutableMapping[str, Any],
    request: StageStartRequest,
    *,
    model_dir: str,
    log_path: str,
    baked_stage: str,
    baked_server: str,
    gpu_uuid: str = "",
    stop_worker: Callable[[], None] | None = None,
) -> dict[str, Any]:
    if request.role not in ("first", "middle", "last"):
        raise StageError(400, "stage role must be first, middle, or last")
    if len(request.layers) != 2 or request.layers[0] < 0 or request.layers[1] <= request.layers[0]:
        raise StageError(400, "stage layers must be [begin, end]")
    if not 1 <= request.listen_port <= 65535 or ":" not in request.next_endpoint:
        raise StageError(400, "stage listener and successor endpoint are required")
    if request.coordinator and (request.role != "first" or not 1 <= int(request.server_port or 0) <= 65535):
        raise StageError(400, "coordinator stage requires first role and server_port")
    try:
        stage_bin, stage_env, runtime_pack_id = packs.resolve_runtime(
            coordinator=request.coordinator,
            expected_protocol=request.ring_protocol,
            expected_adapter_abi=request.ring_adapter_abi,
            expected_build_id=request.ring_build_id,
            baked_stage=baked_stage,
            baked_server=baked_server,
        )
    except ValueError as exc:
        raise StageError(409, str(exc)) from exc
    model_path = _model_path(model_dir, request.model)
    if not os.path.isfile(model_path):
        raise StageError(404, "model file is not present on node")
    if request.stage_manifest:
        try:
            validate_stage_manifest(model_dir, request.stage_manifest)
        except ValueError as exc:
            raise StageError(409, str(exc)) from exc

    if stop_worker:
        stop_worker()
    _kill(owner.get("stage"))
    owner["stage"] = None
    if request.coordinator:
        server_port = int(request.server_port or request.listen_port + 1000)
        command = [
            stage_bin, "--model", model_path, "--host", "127.0.0.1", "--port", str(server_port),
            "--ctx-size", str(request.ctx), "--parallel", str(request.parallel),
            "--n-gpu-layers", str(request.gpu_layers),
            "--cache-type-k", request.cache_type_k, "--cache-type-v", request.cache_type_v,
        ]
        # Native tool calling + correct chat formatting on the coordinator's
        # OpenAI HTTP server (the coordinator is a stock llama-server). Inert for
        # models without a tool template.
        command += ["--jinja"]
        # One full-model node does not need a ring transport.  Omitting every
        # linkcpp flag leaves linkcpp-server in its stock llama-server mode,
        # with the model and all memory resident on the native node.
        if not request.single_node:
            command += ["--linkcpp-layers", f"{request.layers[0]}:{request.layers[1]}",
                        "--linkcpp-listen", str(request.listen_port),
                        "--linkcpp-next", request.next_endpoint]
            if request.dial_prev_endpoint:
                command += ["--linkcpp-dial-prev", request.dial_prev_endpoint]
            if request.accept_next:
                command += ["--linkcpp-accept-next"]
        # Backbone MoE expert RAM offload: linkcpp-server forwards unknown args to
        # the stock llama-server, which honours --override-tensor. This is what lets
        # a coordinator hold [0,k] of a large MoE with its experts streamed from RAM.
        if request.ot:
            command += ["--override-tensor", request.ot]
        if not request.kv_offload:
            command += ["--no-kv-offload"]
        if request.batch > 0:
            command += ["--batch-size", str(request.batch)]
        if request.ubatch > 0:
            command += ["--ubatch-size", str(request.ubatch)]
        if not request.cont_batching:
            command += ["--no-cont-batching"]
        # P2-3: prefix-cache-aware slot selection (docs/design/dispatch-comm-optimization.md).
        sps = float(getattr(request, "slot_prompt_similarity", 0.0) or 0.0)
        if sps > 0:
            command += ["--slot-prompt-similarity", str(sps)]
    else:
        command = [
            stage_bin, "--model", model_path,
            "--layers", f"{request.layers[0]}:{request.layers[1]}",
            "--gpu-layers", str(request.gpu_layers), "--role", request.role,
            "--listen", str(request.listen_port), "--next", request.next_endpoint,
            "--ctx", str(request.ctx), "--parallel", str(request.parallel),
            "--cache-type-k", request.cache_type_k, "--cache-type-v", request.cache_type_v,
        ]
        # Bound the stage's ubatch to the coordinator's so the compute-graph reserve
        # is capped at ubatch size, not full n_ctx — required for large contexts
        # (the coordinator drives the ring per-ubatch, so a stage never sees a
        # larger frame). Omitted => stage falls back to n_ctx (legacy).
        if request.batch > 0:
            command += ["--batch", str(request.batch)]
        if request.ubatch > 0:
            command += ["--ubatch", str(request.ubatch)]
        if not request.kv_offload:
            command += ["--no-kv-offload"]
        if request.dial_prev_endpoint:
            command += ["--dial-prev", request.dial_prev_endpoint]
        if request.accept_next:
            command += ["--accept-next"]
    os.makedirs(os.path.dirname(log_path) or ".", exist_ok=True)
    environment = dict(stage_env)
    if gpu_uuid:
        environment["CUDA_VISIBLE_DEVICES"] = gpu_uuid
    with open(log_path, "w", encoding="utf-8") as log:
        owner["stage"] = subprocess.Popen(
            command, env=environment, stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL, stderr=log,
        )
    owner["stage_infer_lock"] = asyncio.Lock() if request.coordinator else None
    owner["desired_load"] = {
        "model": request.model, "layers": request.layers, "stage": True,
        "role": request.role, "listen_port": request.listen_port,
        "next_endpoint": request.next_endpoint, "gpu_layers": request.gpu_layers,
        "parallel": request.parallel, "coordinator": request.coordinator,
        "single_node": request.single_node,
        "batch": request.batch, "ubatch": request.ubatch,
        "cache_type_k": request.cache_type_k, "cache_type_v": request.cache_type_v,
        "kv_offload": request.kv_offload,
        "server_port": request.server_port or request.listen_port + 1000,
        "runtime_pack_id": runtime_pack_id, "ring_build_id": request.ring_build_id,
        "stage_manifest": summarize_stage_manifest(request.stage_manifest),
    }
    return {"accepted": True, "pid": owner["stage"].pid, "log": log_path, "command": command}


async def run_inference(owner: MutableMapping[str, Any], request: StageInferRequest) -> dict[str, Any]:
    desired = owner.get("desired_load") or {}
    if desired.get("role") != "first" or not desired.get("coordinator"):
        raise StageError(409, "stage is not the ring coordinator")
    if not request.prompt.strip() and not request.messages:
        raise StageError(400, "ring prompt or chat messages are required")
    lock = owner.setdefault("stage_infer_lock", asyncio.Lock())
    async with lock:
        try:
            return await infer(f"http://127.0.0.1:{desired['server_port']}", request)
        except (RuntimeError, ValueError, httpx.HTTPError) as exc:
            raise StageError(502, str(exc)) from exc


def _coordinator_port(owner: MutableMapping[str, Any]) -> int:
    desired = owner.get("desired_load") or {}
    if desired.get("role") != "first" or not desired.get("coordinator"):
        raise StageError(409, "stage is not the ring coordinator")
    return int(desired["server_port"])


async def run_chat(owner: MutableMapping[str, Any], body: dict) -> dict[str, Any]:
    """Forward a raw OpenAI chat body to the coordinator's native llama-server
    HTTP endpoint (so tools / tool_choice / structured content pass through).
    Non-streaming."""
    server_port = _coordinator_port(owner)
    lock = owner.setdefault("stage_infer_lock", asyncio.Lock())
    async with lock:
        try:
            async with httpx.AsyncClient(timeout=None) as client:
                response = await client.post(
                    f"http://127.0.0.1:{server_port}/v1/chat/completions",
                    json={**body, "stream": False},
                )
                response.raise_for_status()
                return response.json()
        except (RuntimeError, ValueError, httpx.HTTPError) as exc:
            raise StageError(502, str(exc)) from exc


async def run_chat_stream(owner: MutableMapping[str, Any], body: dict):
    """Stream (SSE) a raw OpenAI chat body from the coordinator's native
    llama-server HTTP endpoint. Yields raw event-stream bytes for relay."""
    server_port = _coordinator_port(owner)
    lock = owner.setdefault("stage_infer_lock", asyncio.Lock())
    async with lock:
        try:
            async with httpx.AsyncClient(timeout=None) as client:
                async with client.stream(
                    "POST",
                    f"http://127.0.0.1:{server_port}/v1/chat/completions",
                    json={**body, "stream": True},
                ) as response:
                    response.raise_for_status()
                    async for chunk in response.aiter_raw():
                        yield chunk
        except (RuntimeError, ValueError, httpx.HTTPError) as exc:
            raise StageError(502, str(exc)) from exc


def status(owner: MutableMapping[str, Any], log_path: str) -> dict[str, Any]:
    process = owner.get("stage")
    try:
        with open(log_path, encoding="utf-8", errors="replace") as stream:
            log = "".join(stream.readlines()[-160:])
    except OSError:
        log = ""
    return {
        "running": bool(process and process.poll() is None),
        "exit_code": process.poll() if process else None,
        "desired_load": owner.get("desired_load"),
        "log": log,
    }


def stop(owner: MutableMapping[str, Any], reason: str = "requested") -> dict[str, Any]:
    _kill(owner.get("stage"))
    owner["stage"] = None
    owner["stage_infer_lock"] = None
    owner["desired_load"] = None
    return {"stopped": True, "reason": reason}
