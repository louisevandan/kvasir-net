#!/usr/bin/env python3
"""Managed linkcpp node-agent.

One node-agent owns one GPU budget, exposes a fixed llama.cpp RPC worker port,
accepts controller join/download/load/unload commands, and reports state with
short HTTP requests rather than a persistent stream.
"""
import asyncio
import collections
import hashlib
import json
import os
import re
import shutil
import subprocess
import time
import uuid
from typing import Any, Dict, Optional

import httpx
from fastapi import FastAPI, HTTPException, Query
from pydantic import BaseModel

from controller.protocol import (
    CancelLoadRequest,
    DownloadRequest,
    JoinRequest,
    LoadRequest,
    ModelSource,
    NodeOperation,
    NodeReport,
    ResourceSnapshot,
    UnloadRequest,
    model_to_dict,
)
from controller import host_resources
from controller.versioning import backend_identity, backend_report, compatibility_report, runtime_identity

RPC_BIN = os.environ.get("RPC_BIN", "/workspace/build/bin/ggml-rpc-server")
MODEL_DIR = os.environ.get("LINKCPP_MODEL_DIR", "/models")
CACHE = os.environ.get("LINKCPP_RPC_CACHE", "/root/.cache/llama.cpp/rpc")
RPC_PORT = int(os.environ.get("LINKCPP_RPC_PORT", "50052"))
# Wallet that owns this node — earns its inference contribution rewards. Empty falls
# back (at the hub) to the hub operator wallet. Lets a third party run a node and be
# paid for the work it does.
NODE_OWNER = os.environ.get("LINKCPP_NODE_OWNER", "").strip()
STATE_FILE = os.environ.get("LINKCPP_NODE_STATE", "/tmp/linkcpp-node-state.json")
WORKER_LOG = os.environ.get("LINKCPP_WORKER_LOG", "/tmp/worker.log")

app = FastAPI(title="linkcpp node-agent")
from controller.proxy import install_node_api
install_node_api(app)

state: Dict[str, Any] = {
    "node_id": "agent-" + uuid.uuid4().hex[:8],
    "controller_id": None,
    "report_url": None,
    "service_token": None,
    "name": "",
    "resource_limits": {},
    "worker": None,
    "worker_port": None,
    "desired_load": None,
    "operations": {},
    "models": {},
    "last_reports": [],
    "outbox": [],
    "seq": 0,
}
download_tasks: Dict[str, asyncio.Task] = {}
download_controls: Dict[str, Dict[str, bool]] = {}
load_monitor_tasks: Dict[str, asyncio.Task] = {}
load_monitor_stop_status: Dict[str, str] = {}
download_sources: Dict[str, ModelSource] = {}
# A Metal RPC process owns unified memory outside Python's heap.  Serialize all
# lifecycle changes so a late load request cannot race an unload/cancel and
# create a second worker before the first one has exited.
runtime_control_lock = asyncio.Lock()


def _now() -> float:
    return time.time()


def _save_state() -> None:
    serializable = {k: v for k, v in state.items() if k != "worker"}
    try:
        os.makedirs(os.path.dirname(STATE_FILE) or ".", exist_ok=True)
        with open(STATE_FILE, "w", encoding="utf-8") as f:
            json.dump(serializable, f)
    except Exception:
        pass


def _load_state() -> None:
    try:
        with open(STATE_FILE, encoding="utf-8") as f:
            saved = json.load(f)
    except Exception:
        return
    for key in ("node_id", "controller_id", "report_url", "service_token", "name", "resource_limits",
                "desired_load", "operations", "models", "last_reports", "outbox", "seq"):
        if key in saved:
            state[key] = saved[key]


_load_state()


def _safe_model_name(name: str) -> str:
    """Accept a model path relative to the configured model directory.

    The hub catalog identifies models by their relative path so collections
    such as LM Studio's ``publisher/model/file.gguf`` layout must reach a
    native agent unchanged.  Normalize it and reject traversal/absolute
    paths so an agent can still only access its configured model directory.
    """
    raw = name.strip().replace("\\", "/")
    normalized = os.path.normpath(raw).replace("\\", "/")
    if (not raw or normalized in (".", "..") or normalized.startswith("../")
            or os.path.isabs(raw) or ":" in normalized):
        raise HTTPException(400, "model must be a relative path inside the model directory")
    return normalized


def _model_path(name: str) -> str:
    root = os.path.abspath(MODEL_DIR)
    path = os.path.abspath(os.path.join(root, _safe_model_name(name)))
    if os.path.commonpath([root, path]) != root:
        raise HTTPException(400, "model path must stay within the model directory")
    return path


def _gpu() -> Dict[str, Any]:
    return host_resources.accelerator()


def _meminfo() -> Dict[str, float]:
    return host_resources.memory_info()


def _cpu_percent() -> float:
    return host_resources.cpu_percent()


def resource_snapshot() -> ResourceSnapshot:
    data = host_resources.resource_snapshot(MODEL_DIR)
    limits = state.get("resource_limits") or {}
    for field, total_field in (("vram_budget_gib", "vram_total_gib"),
                               ("ram_budget_gib", "ram_total_gib")):
        if field in limits:
            data[field] = min(float(limits[field]), float(data[total_field]))
    if "cores_budget" in limits:
        data["cores_budget"] = min(int(limits["cores_budget"]), int(data["cores_total"]))
    # AMD VRAM: report the worker's OWN VRAM (attributed by PID). host_resources'
    # card-index heuristic can pick the wrong card when HIP and rocm-smi enumerate
    # devices in different orders, or the total of all workers on a multi-agent host.
    worker = state.get("worker")
    if worker is not None and worker.poll() is None:
        used = host_resources.rocm_process_vram_gib(worker.pid)
        if used is not None:
            data["vram_used_gib"] = used
    return ResourceSnapshot(**data)


def _backend() -> Dict[str, str]:
    gpu = _gpu()
    backend = backend_identity()
    if gpu.get("backend_kind") and not os.environ.get("LINKCPP_LLAMA_CPP_BACKEND"):
        backend["backend_kind"] = str(gpu["backend_kind"])
    if gpu.get("name") and not backend.get("backend_device"):
        backend["backend_device"] = str(gpu["name"])
    return backend


def _worker_running() -> bool:
    worker = state.get("worker")
    return worker is not None and worker.poll() is None


def _op(op_id: str) -> Dict[str, Any]:
    return state["operations"].setdefault(op_id, model_to_dict(NodeOperation(op_id=op_id, type="unknown")))


def _update_op(op_id: str, **changes: Any) -> Dict[str, Any]:
    op = _op(op_id)
    op.update(changes)
    op["updated_at"] = _now()
    _save_state()
    return op


def _new_op(kind: str, model: Optional[str] = None, op_id: Optional[str] = None,
            **details: Any) -> Dict[str, Any]:
    oid = op_id or f"{kind}-" + uuid.uuid4().hex[:10]
    op = NodeOperation(op_id=oid, type=kind, model=model, details=details)
    state["operations"][oid] = model_to_dict(op)
    _save_state()
    return state["operations"][oid]


def _report_headers() -> Dict[str, str]:
    """M2M auth for node-reports so an auth-enabled hub accepts them. The token is
    handed to the agent on /control/join; empty on a trusted-LAN (no-auth) hub."""
    token = state.get("service_token")
    return {"X-Linkcpp-Service-Token": token} if token else {}


async def _flush_outbox() -> None:
    url = state.get("report_url")
    if not url or not state["outbox"]:
        return
    pending = list(state["outbox"])
    state["outbox"] = []
    async with httpx.AsyncClient(timeout=5) as client:
        for item in pending:
            try:
                await client.post(url, json=item, headers=_report_headers())
            except Exception:
                state["outbox"].append(item)
    state["outbox"] = state["outbox"][-100:]
    _save_state()


async def _report(op_id: Optional[str], op_type: str, phase: str, status: str,
                  progress: float = 0.0, message: str = "", model: Optional[str] = None,
                  error: Optional[str] = None) -> Dict[str, Any]:
    state["seq"] = int(state.get("seq", 0)) + 1
    report = NodeReport(
        node_id=state["node_id"],
        controller_id=state.get("controller_id"),
        op_id=op_id,
        op_type=op_type,
        phase=phase,
        status=status,
        progress=progress,
        message=message,
        resources=resource_snapshot(),
        model=model,
        error=error,
        seq=state["seq"],
    )
    data = model_to_dict(report)
    state["last_reports"] = (state["last_reports"] + [data])[-50:]
    url = state.get("report_url")
    if url:
        try:
            async with httpx.AsyncClient(timeout=5) as client:
                await client.post(url, json=data, headers=_report_headers())
        except Exception:
            state["outbox"] = (state["outbox"] + [data])[-100:]
    _save_state()
    return data


def _rpc_activity_metrics() -> Dict[str, Any]:
    try:
        with open(WORKER_LOG, "r", encoding="utf-8", errors="replace") as f:
            log = f.read()[-64000:]
    except Exception:
        log = ""
    sizes = [int(x) for x in re.findall(r"\[(?:set_tensor|get_tensor|alloc_buffer)\][^\n]*size:\s*(\d+)", log)]
    get_tensor_sizes = [int(x) for x in re.findall(r"\[get_tensor\][^\n]*size:\s*(\d+)", log)]
    return {
        "alloc_buffer_count": len(re.findall(r"\[alloc_buffer\]", log)),
        "set_tensor_count": len(re.findall(r"\[set_tensor\]", log)),
        "get_tensor_count": len(re.findall(r"\[get_tensor\]", log)),
        "get_alloc_size_count": len(re.findall(r"\[get_alloc_size\]", log)),
        "graph_compute_count": len(re.findall(r"\[graph_compute\]", log)),
        "accepted_connections": len(re.findall(r"Accepted client connection", log)),
        "get_tensor_bytes": sum(get_tensor_sizes),
        "bytes_observed": sum(sizes),
    }


def _worker_log_tail(lines: int = 200) -> str:
    """Return a bounded but substantial worker-log slice for cross-unit diagnosis."""
    try:
        with open(WORKER_LOG, "rb") as f:
            return b"".join(collections.deque(f, maxlen=max(20, min(lines, 10000)))).decode("utf-8", "replace")
    except Exception as exc:
        return f"worker log unavailable: {exc}\n"


async def _load_monitor_worker(op_id: str, model: str) -> None:
    baseline = resource_snapshot()
    try:
        while _worker_running() and state.get("desired_load"):
            await asyncio.sleep(2.0)
            resources = resource_snapshot()
            activity = _rpc_activity_metrics()
            vram_delta = max(0.0, resources.vram_used_gib - baseline.vram_used_gib)
            vram_ratio = min(1.0, vram_delta / max(resources.vram_budget_gib, 0.1))
            activity_ratio = min(1.0, (activity["alloc_buffer_count"] + activity["set_tensor_count"] + activity["get_alloc_size_count"] * 0.08) / 40.0)
            progress = round(10.0 + max(vram_ratio, activity_ratio) * 80.0, 1)
            phase = "rpc_loading" if activity["alloc_buffer_count"] or activity["set_tensor_count"] or activity["get_alloc_size_count"] else "waiting_for_rpc"
            _update_op(op_id, status="running", phase=phase, progress=progress,
                       message=f"{phase}: vram {resources.vram_used_gib:.2f} GiB, rpc alloc {activity['alloc_buffer_count']}, set {activity['set_tensor_count']}",
                       details={"rpc_activity": activity, "monitor_source": "node_agent_scheduler"})
            await _report(op_id, "node_load", phase, "running", progress,
                          _op(op_id).get("message", ""), model=model)
        _update_op(op_id, status="done", phase="monitor_stopped", progress=100.0,
                   message="node load monitor stopped")
        await _report(op_id, "node_load", "monitor_stopped", "done", 100.0,
                      "node load monitor stopped", model=model)
    except asyncio.CancelledError:
        if load_monitor_stop_status.pop(op_id, "canceled") == "done":
            _update_op(op_id, status="done", phase="monitor_stopped", progress=100.0,
                       message="node load monitor stopped")
            await _report(op_id, "node_load", "monitor_stopped", "done", 100.0,
                          "node load monitor stopped", model=model)
        else:
            _update_op(op_id, status="canceled", phase="canceled", progress=0.0,
                       message="node load monitor canceled")
            await _report(op_id, "node_load", "canceled", "canceled", 0.0,
                          "node load monitor canceled", model=model)
        raise


def _start_load_monitor(op_id: str, model: str) -> None:
    old = load_monitor_tasks.pop(op_id, None)
    if old:
        old.cancel()
    load_monitor_stop_status.pop(op_id, None)
    load_monitor_tasks[op_id] = asyncio.create_task(_load_monitor_worker(op_id, model))


def _stop_load_monitors(final_status: str = "canceled") -> None:
    for op_id, task in list(load_monitor_tasks.items()):
        load_monitor_stop_status[op_id] = final_status
        task.cancel()
    load_monitor_tasks.clear()


def _info() -> Dict[str, Any]:
    gpu = _gpu()
    resources = resource_snapshot()
    runtime = runtime_identity()
    backend = _backend()
    host = host_resources.host_platform()
    return {
        "node_id": state["node_id"],
        "owner": NODE_OWNER,
        "hostname": host["hostname"],
        "host_platform": host,
        "name": state.get("name") or state["node_id"],
        "gpu": gpu["name"],
        "gpu_uuid": gpu["uuid"],
        "vram_budget_gib": resources.vram_budget_gib,
        "ram_budget_gib": resources.ram_budget_gib,
        "cores": resources.cores_budget,
        "resources": model_to_dict(resources),
        "bound_to": state.get("controller_id"),
        "rpc_port": RPC_PORT,
        "worker_running": _worker_running(),
        "worker_port": state.get("worker_port"),
        "operations": list(state["operations"].values()),
        "models": state["models"],
        "desired_load": state.get("desired_load"),
        "last_reports": state["last_reports"][-10:],
        "runtime": runtime,
        "runtime_compatibility": compatibility_report(runtime),
        "backend": backend,
        "backend_compatibility": backend_report(backend),
        "capabilities": {
            "managed": True,
            "download": True,
            "pause_resume_cancel": True,
            "load": True,
            "unload": True,
            "reports": True,
            "native_agent": True,
            "host_system": host["system"],
            "backend_kind": backend.get("backend_kind", "unknown"),
        },
    }


def _stop_worker() -> None:
    worker = state.get("worker")
    if worker and worker.poll() is None:
        worker.terminate()
        try:
            worker.wait(timeout=5)
        except subprocess.TimeoutExpired:
            worker.kill()
            try:
                worker.wait(timeout=5)
            except subprocess.TimeoutExpired as exc:
                # Do not claim that Metal memory has been released while the
                # owning process is still alive.  The hub will keep this node
                # in an unload-error state and can retry safely.
                raise RuntimeError("RPC worker did not exit after SIGKILL") from exc
    state["worker"] = None
    state["worker_port"] = None


def _start_worker() -> None:
    _stop_worker()
    os.makedirs(CACHE, exist_ok=True)
    if not shutil.which(RPC_BIN) and not os.path.exists(RPC_BIN):
        raise HTTPException(500, f"rpc binary missing: {RPC_BIN}")
    env = dict(os.environ, GGML_RPC_DEBUG="1", LLAMA_CACHE=CACHE)
    cmd = [RPC_BIN, "-H", "0.0.0.0", "-p", str(RPC_PORT)]
    # A Metal RPC build can also advertise its CPU fallback backend.  That is
    # useful locally, but a distributed llama-server then sees two RPC devices
    # for one Mac and may select the 0 MiB CPU device during warmup.  Export
    # only the real Metal accelerator so one linkcpp node always maps to one
    # RPC device.  The override keeps custom/native builds configurable.
    if _backend().get("backend_kind") == "metal":
        # llama.cpp names the Apple Metal backend MTL0 (not Metal0).
        cmd += ["--device", os.environ.get("LINKCPP_RPC_DEVICE", "MTL0")]
    state["worker"] = subprocess.Popen(
        # The RPC file-cache mode can delay the socket bind substantially on
        # macOS while Metal is initialized.  Models are already shared through
        # the configured model directory, so start the serving endpoint
        # directly and let the controller wait for its readiness probe.
        cmd,
        env=env, stdout=open(WORKER_LOG, "w", encoding="utf-8"), stderr=subprocess.STDOUT,
        start_new_session=True)
    state["worker_port"] = RPC_PORT


def _download_url(source: ModelSource) -> tuple[str, Dict[str, str]]:
    headers: Dict[str, str] = {}
    if source.kind == "direct_url":
        if not source.url:
            raise HTTPException(400, "source.url is required")
        url = source.url
    elif source.kind == "huggingface":
        if not source.repo_id or not source.filename:
            raise HTTPException(400, "repo_id and filename are required for huggingface")
        rev = source.revision or "main"
        url = f"https://huggingface.co/{source.repo_id}/resolve/{rev}/{source.filename}"
        if source.hf_token:
            headers["Authorization"] = f"Bearer {source.hf_token}"
    else:
        raise HTTPException(400, "source.kind must be direct_url or huggingface")
    return url, headers


def _parse_content_range(value: str) -> Optional[int]:
    if not value or "/" not in value:
        return None
    try:
        return int(value.rsplit("/", 1)[1])
    except ValueError:
        return None


async def _download_worker(op_id: str, model: str, source: ModelSource) -> None:
    dest = _model_path(model)
    part = dest + ".part"
    ctrl = download_controls.setdefault(op_id, {"pause": False, "cancel": False})
    os.makedirs(MODEL_DIR, exist_ok=True)
    url, headers = _download_url(source)
    done = os.path.getsize(part) if os.path.exists(part) else 0
    total = source.size_bytes or (os.path.getsize(dest) if os.path.exists(dest) else 0)
    digest = hashlib.sha256() if source.sha256 else None
    if digest and done:
        try:
            with open(part, "rb") as existing:
                for block in iter(lambda: existing.read(1 << 20), b""):
                    digest.update(block)
        except OSError:
            pass
    try:
        if os.path.exists(dest):
            size = os.path.getsize(dest)
            state["models"][model] = {"path": dest, "size_bytes": size, "present": True}
            _update_op(op_id, status="done", phase="already_present", progress=100.0,
                       message="model already present")
            await _report(op_id, "download", "already_present", "done", 100.0,
                          "model already present", model=model)
            return
        req_headers = dict(headers)
        if done:
            req_headers["Range"] = f"bytes={done}-"
        _update_op(op_id, status="running", phase="downloading", progress=0.0,
                   details={"source": source.redacted(), "done": done, "total": total})
        await _report(op_id, "download", "downloading", "running", 0.0,
                      "download started", model=model)
        async with httpx.AsyncClient(timeout=None, follow_redirects=True) as client:
            async with client.stream("GET", url, headers=req_headers) as response:
                if response.status_code == 416 and os.path.exists(part):
                    os.replace(part, dest)
                    response_total = os.path.getsize(dest)
                    state["models"][model] = {"path": dest, "size_bytes": response_total, "present": True}
                    _update_op(op_id, status="done", phase="complete", progress=100.0)
                    await _report(op_id, "download", "complete", "done", 100.0, "download complete", model=model)
                    return
                response.raise_for_status()
                content_total = _parse_content_range(response.headers.get("content-range", ""))
                total = content_total or (done + int(response.headers.get("content-length", "0") or 0))
                mode = "ab" if done else "wb"
                with open(part, mode) as f:
                    async for chunk in response.aiter_bytes(1 << 20):
                        if ctrl.get("cancel"):
                            raise asyncio.CancelledError()
                        if ctrl.get("pause"):
                            _update_op(op_id, status="paused", phase="paused",
                                       progress=(done / total * 100.0) if total else 0.0,
                                       message="download paused",
                                       details={"source": source.redacted(), "done": done, "total": total})
                            await _report(op_id, "download", "paused", "paused",
                                          (done / total * 100.0) if total else 0.0,
                                          "download paused", model=model)
                            return
                        if not chunk:
                            continue
                        f.write(chunk)
                        if digest:
                            digest.update(chunk)
                        done += len(chunk)
                        progress = (done / total * 100.0) if total else 0.0
                        _update_op(op_id, status="running", phase="downloading",
                                   progress=round(progress, 2),
                                   details={"source": source.redacted(), "done": done, "total": total})
                        if done == len(chunk) or done % (32 << 20) < len(chunk):
                            await _report(op_id, "download", "downloading", "running",
                                          round(progress, 2), "download progress", model=model)
        if source.sha256 and digest and digest.hexdigest().lower() != source.sha256.lower():
            raise RuntimeError("sha256 mismatch")
        os.replace(part, dest)
        size = os.path.getsize(dest)
        state["models"][model] = {"path": dest, "size_bytes": size, "present": True}
        _update_op(op_id, status="done", phase="complete", progress=100.0,
                   message="download complete", details={"source": source.redacted(), "done": size, "total": size})
        await _report(op_id, "download", "complete", "done", 100.0, "download complete", model=model)
    except asyncio.CancelledError:
        if os.path.exists(part):
            try:
                os.remove(part)
            except OSError:
                pass
        _update_op(op_id, status="canceled", phase="canceled", progress=0.0, message="download canceled")
        await _report(op_id, "download", "canceled", "canceled", 0.0, "download canceled", model=model)
    except Exception as exc:
        _update_op(op_id, status="error", phase="error", error=str(exc), message="download failed")
        await _report(op_id, "download", "error", "error", 0.0, "download failed", model=model, error=str(exc))


class Bind(BaseModel):
    controller_id: str


class StartWorker(BaseModel):
    port: int = 0


@app.get("/info")
def get_info():
    return _info()


@app.get("/status")
def status():
    return _info()


@app.post("/bind")
async def bind(b: Bind):
    return await control_join(JoinRequest(controller_id=b.controller_id))


@app.post("/unbind")
async def unbind():
    await control_unload(UnloadRequest(reason="unbind"))
    state["controller_id"] = None
    state["report_url"] = None
    _save_state()
    return _info()


@app.post("/start_worker")
async def start_worker(s: StartWorker):
    if state["controller_id"] is None:
        raise HTTPException(400, "node is free; bind first")
    _start_worker()
    await _report(None, "load", "worker_started", "running", 20.0, "worker started")
    return _info()


@app.post("/stop")
async def stop():
    _stop_worker()
    _save_state()
    return {"stopped": True}


@app.post("/control/join")
async def control_join(req: JoinRequest):
    if state.get("controller_id") and state["controller_id"] != req.controller_id:
        raise HTTPException(status_code=409, detail={"bound_to": state["controller_id"]})
    state["controller_id"] = req.controller_id
    state["report_url"] = req.report_url
    if req.service_token:
        state["service_token"] = req.service_token
    if req.name:
        state["name"] = req.name
    requested = {
        "vram_budget_gib": req.vram_budget_gib,
        "ram_budget_gib": req.ram_budget_gib,
        "cores_budget": req.cores_budget,
    }
    if any(value is not None for value in requested.values()):
        available = host_resources.resource_snapshot(MODEL_DIR)
        limits = dict(state.get("resource_limits") or {})
        for field, value in requested.items():
            if value is None:
                continue
            total_field = field.replace("budget", "total")
            if value <= 0 or value > available[total_field]:
                raise HTTPException(400, f"{field} must be greater than 0 and at most {available[total_field]}")
            limits[field] = int(value) if field == "cores_budget" else float(value)
        state["resource_limits"] = limits
    _new_op("join", op_id="join-" + uuid.uuid4().hex[:10],
            controller_id=req.controller_id, report_url=bool(req.report_url),
            resource_limits=state.get("resource_limits") or {})
    _save_state()
    await _flush_outbox()
    await _report(None, "join", "joined", "done", 100.0, "node joined controller")
    return _info()


@app.get("/control/logs")
async def control_logs(tail_lines: int = Query(500, alias="tail", ge=20, le=10000)):
    """Expose native worker stderr, state and parsed RPC counters to a hub.

    Managed Metal workers run outside the importing hub, so controller reports
    alone cannot identify a stuck RPC request.  This endpoint is deliberately
    read-only and returns the exact worker tail used by diagnostics.
    """
    resources = resource_snapshot()
    log = _worker_log_tail(tail_lines)
    return {
        "node_id": state.get("node_id"),
        "worker_running": _worker_running(),
        "worker_pid": getattr(state.get("worker"), "pid", None),
        "worker_port": state.get("worker_port"),
        "desired_load": state.get("desired_load"),
        "resources": model_to_dict(resources),
        "vram_used_gib": resources.vram_used_gib,
        "ram_used_gib": resources.ram_used_gib,
        "rpc_activity": _rpc_activity_metrics(),
        "log": log,
    }


@app.get("/control/status")
async def control_status():
    await _flush_outbox()
    return _info()


@app.post("/control/download")
async def control_download(req: DownloadRequest):
    model = _safe_model_name(req.model)
    dest = _model_path(model)
    op = _new_op("download", model=model, op_id=req.op_id, source=req.source.redacted())
    if os.path.exists(dest):
        size = os.path.getsize(dest)
        state["models"][model] = {"path": dest, "size_bytes": size, "present": True}
        _update_op(op["op_id"], status="done", phase="already_present", progress=100.0,
                   message="model already present")
        await _report(op["op_id"], "download", "already_present", "done", 100.0,
                      "model already present", model=model)
        return {"accepted": False, "op_id": op["op_id"], "already_present": True}
    download_controls[op["op_id"]] = {"pause": False, "cancel": False}
    download_sources[op["op_id"]] = req.source
    task = asyncio.create_task(_download_worker(op["op_id"], model, req.source))
    download_tasks[op["op_id"]] = task
    return {"accepted": True, "op_id": op["op_id"], "already_present": False}


@app.post("/control/download/{op_id}/pause")
async def control_download_pause(op_id: str):
    if op_id not in state["operations"]:
        raise HTTPException(404, "unknown operation")
    download_controls.setdefault(op_id, {"pause": False, "cancel": False})["pause"] = True
    _update_op(op_id, status="paused", phase="paused", message="pause requested")
    await _report(op_id, "download", "paused", "paused", _op(op_id).get("progress", 0.0), "pause requested")
    return {"paused": op_id}


@app.post("/control/download/{op_id}/resume")
async def control_download_resume(op_id: str):
    op = state["operations"].get(op_id)
    if not op:
        raise HTTPException(404, "unknown operation")
    if op.get("type") != "download":
        raise HTTPException(400, "operation is not a download")
    model = op.get("model")
    source = download_sources.get(op_id)
    source_data = op.get("details", {}).get("source")
    if not source and source_data:
        source = ModelSource(**source_data)
    if not model or not source:
        raise HTTPException(400, "download source is unavailable")
    download_controls[op_id] = {"pause": False, "cancel": False}
    download_tasks[op_id] = asyncio.create_task(_download_worker(op_id, model, source))
    return {"resumed": op_id}


@app.post("/control/download/{op_id}/cancel")
async def control_download_cancel(op_id: str):
    if op_id not in state["operations"]:
        raise HTTPException(404, "unknown operation")
    download_controls.setdefault(op_id, {"pause": False, "cancel": False})["cancel"] = True
    task = download_tasks.get(op_id)
    if task:
        task.cancel()
    _update_op(op_id, status="canceled", phase="canceled", message="cancel requested")
    await _report(op_id, "download", "canceled", "canceled", 0.0, "cancel requested")
    return {"canceled": op_id}


@app.post("/control/load")
async def control_load(req: LoadRequest):
    model = _safe_model_name(req.model)
    op = _new_op("load", model=model, op_id=req.op_id, layers=req.layers,
                 tensor_split=req.tensor_split, offload=req.offload,
                 parallel=req.parallel, ctx=req.ctx, kv_bits=req.kv_bits,
                 cache_type_k=req.cache_type_k, cache_type_v=req.cache_type_v)
    if not os.path.exists(_model_path(model)):
        _update_op(op["op_id"], status="error", phase="model_missing", progress=0.0,
                   error="model_missing", message="model file is not present")
        await _report(op["op_id"], "load", "model_missing", "error", 0.0,
                      "model file is not present", model=model, error="model_missing")
        raise HTTPException(404, "model file is not present on node")
    _update_op(op["op_id"], status="running", phase="load_requested", progress=5.0,
               message="load requested")
    await _report(op["op_id"], "load", "load_requested", "running", 5.0,
                  "load requested", model=model)
    async with runtime_control_lock:
        _start_worker()
        state["desired_load"] = {
            "model": model,
            "layers": req.layers,
            "tensor_split": req.tensor_split,
            "offload": req.offload,
            "parallel": req.parallel,
            "ctx": req.ctx,
            "kv_bits": req.kv_bits,
            "cache_type_k": req.cache_type_k,
            "cache_type_v": req.cache_type_v,
        }
    _update_op(op["op_id"], status="done", phase="worker_started", progress=100.0,
               message="worker started; waiting for controller master RPC load")
    await _report(op["op_id"], "load", "worker_started", "done", 100.0,
                  "worker started; waiting for controller master RPC load", model=model)
    _start_load_monitor(f"{op['op_id']}-monitor", model)
    return {"accepted": True, "op_id": op["op_id"], "rpc_port": RPC_PORT, "status": _info()}


@app.post("/control/load-monitor/stop")
async def control_load_monitor_stop(req: CancelLoadRequest):
    _stop_load_monitors("done")
    return {"stopped": True, "op_id": req.op_id, "status": _info()}


@app.post("/control/unload")
async def control_unload(req: UnloadRequest):
    op = _new_op("unload", op_id=req.op_id, reason=req.reason)
    _update_op(op["op_id"], status="running", phase="unloading", progress=10.0,
               message="stopping worker")
    await _report(op["op_id"], "unload", "unloading", "running", 10.0, "stopping worker")
    async with runtime_control_lock:
        _stop_load_monitors()
        _stop_worker()
        state["desired_load"] = None
    _update_op(op["op_id"], status="done", phase="unloaded", progress=100.0,
               message="worker stopped")
    await _report(op["op_id"], "unload", "unloaded", "done", 100.0, "worker stopped")
    _save_state()
    return {"unloaded": True, "op_id": op["op_id"], "status": _info()}


@app.post("/control/load/cancel")
async def control_load_cancel(req: CancelLoadRequest):
    op = _new_op("load_cancel", op_id=req.op_id, reason=req.reason)
    _update_op(op["op_id"], status="running", phase="canceling", progress=10.0,
               message="canceling load")
    await _report(op["op_id"], "load_cancel", "canceling", "running", 10.0,
                  "canceling load")
    async with runtime_control_lock:
        _stop_load_monitors()
        _stop_worker()
        state["desired_load"] = None
    _update_op(op["op_id"], status="canceled", phase="canceled", progress=100.0,
               message="load canceled")
    await _report(op["op_id"], "load_cancel", "canceled", "canceled", 100.0,
                  "load canceled")
    _save_state()
    return {"canceled": True, "op_id": op["op_id"], "status": _info()}
