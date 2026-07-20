#!/usr/bin/env python3
"""linkcpp node-host — a web service that runs immediately in a node container and lets
you create up to N **nodes** (each = one GPU + budget) in that one container.

Ports are pre-mapped: the container publishes a fixed pool of agent+rpc ports
(default 5 slots: agent 9101-9105, rpc 50052-50056). Creating a node claims the next
free slot and exposes it, so an external controller can reach it at PUBLIC_HOST:agent_port.
The UI shows that ready-to-link address for each node.

  uvicorn controller.nodehost:app --host 0.0.0.0 --port 9100
Env: LINKCPP_MAX_NODES (5), LINKCPP_AGENT_PORT_BASE (9101), LINKCPP_RPC_PORT_BASE (50052),
     LINKCPP_PUBLIC_HOST (else derived from the browser's request host).
"""
import os, subprocess, uuid, socket
from fastapi import FastAPI, HTTPException, Request
from fastapi.responses import FileResponse
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel
from controller import device_profiles, host_resources

WEB_DIR = os.path.join(os.path.dirname(__file__), "web")
MAX_NODES = int(os.environ.get("LINKCPP_MAX_NODES", "5"))
AGENT_BASE = int(os.environ.get("LINKCPP_AGENT_PORT_BASE", "9101"))
RPC_BASE = int(os.environ.get("LINKCPP_RPC_PORT_BASE", "50052"))
PUBLIC_HOST_ENV = os.environ.get("LINKCPP_PUBLIC_HOST", "")

app = FastAPI(title="linkcpp node-host")
# fixed slots: [(agent_port, rpc_port), …]
SLOTS = [(AGENT_BASE + i, RPC_BASE + i) for i in range(MAX_NODES)]
NODES = {}   # id -> {id, gpu_uuid, gpu_name, vram, ram, cores, agent_port, rpc_port, proc}


def public_host(req: Request):
    if PUBLIC_HOST_ENV:
        return PUBLIC_HOST_ENV
    # host the operator used to reach this page (strip port) — the LAN IP in multi-machine
    h = req.headers.get("host", socket.gethostname())
    return h.split(":")[0]


def list_gpus():
    try:
        out = subprocess.check_output(
            ["nvidia-smi", "--query-gpu=uuid,name,memory.total",
             "--format=csv,noheader,nounits"], text=True).strip().splitlines()
    except Exception:
        return []
    gpus = []
    for r in out:
        p = [x.strip() for x in r.split(",")]
        if len(p) >= 3:
            vram_total_gib = round(device_profiles.cuda_vram_gib(
                p[1], p[2], host_resources.memory_info()["total"],
                float(os.environ.get("LINKCPP_VRAM_TOTAL", 0) or 0),
            ), 1)
            gpus.append({"uuid": p[0], "name": p[1], "vram_total_gib": vram_total_gib})
    return gpus


def _reap():
    for n in NODES.values():
        if n["proc"] is not None and n["proc"].poll() is not None:
            n["proc"] = None


def _free_slot():
    used = {(n["agent_port"], n["rpc_port"]) for n in NODES.values()}
    for s in SLOTS:
        if s not in used:
            return s
    return None


def _view(n, host):
    return {"id": n["id"], "gpu_uuid": n["gpu_uuid"], "gpu_name": n["gpu_name"],
            "vram": n["vram"], "ram": n["ram"], "cores": n["cores"],
            "agent_port": n["agent_port"], "rpc_port": n["rpc_port"],
            "address": f"{host}:{n['agent_port']}",   # paste this into the controller
            "running": n["proc"] is not None and n["proc"].poll() is None}


class CreateNode(BaseModel):
    gpu_uuid: str
    vram: float = 0        # 0 = whole GPU
    ram: float = 0
    cores: int = 0


@app.get("/api/gpus")
def api_gpus(request: Request):
    return {"gpus": list_gpus(), "hostname": socket.gethostname(),
            "public_host": public_host(request)}


@app.get("/api/nodes")
def api_nodes(request: Request):
    _reap()
    host = public_host(request)
    return {"nodes": [_view(n, host) for n in NODES.values()],
            "used": len(NODES), "max": MAX_NODES, "public_host": host}


@app.post("/api/nodes")
def api_create(c: CreateNode, request: Request):
    _reap()
    if len(NODES) >= MAX_NODES:
        raise HTTPException(409, f"node limit reached ({MAX_NODES}); remove one first")
    gpus = {g["uuid"]: g for g in list_gpus()}
    if c.gpu_uuid not in gpus:
        raise HTTPException(400, f"unknown gpu {c.gpu_uuid}")
    slot = _free_slot()
    if not slot:
        raise HTTPException(409, "no free port slot")
    agent_port, rpc_port = slot
    nid = "node-" + uuid.uuid4().hex[:6]
    env = dict(os.environ,
               CUDA_VISIBLE_DEVICES=c.gpu_uuid,
               LINKCPP_RPC_PORT=str(rpc_port),
               LINKCPP_VRAM_BUDGET=str(c.vram or gpus[c.gpu_uuid]["vram_total_gib"]),
               LINKCPP_RAM_BUDGET=str(c.ram or 0),
               LINKCPP_CORES=str(c.cores or 0))
    proc = subprocess.Popen(
        ["python3", "-m", "uvicorn", "controller.nodeagent:app",
         "--host", "0.0.0.0", "--port", str(agent_port)],
        env=env, stdout=open(f"/tmp/agent-{nid}.log", "w"), stderr=subprocess.STDOUT)
    NODES[nid] = {"id": nid, "gpu_uuid": c.gpu_uuid, "gpu_name": gpus[c.gpu_uuid]["name"],
                  "vram": c.vram or gpus[c.gpu_uuid]["vram_total_gib"], "ram": c.ram,
                  "cores": c.cores, "agent_port": agent_port, "rpc_port": rpc_port,
                  "proc": proc}
    return _view(NODES[nid], public_host(request))


@app.delete("/api/nodes/{nid}")
def api_delete(nid: str):
    n = NODES.pop(nid, None)
    if not n:
        raise HTTPException(404, "unknown node")
    p = n["proc"]
    if p and p.poll() is None:
        p.terminate()
        try:
            p.wait(timeout=5)
        except Exception:
            p.kill()
    return {"removed": nid}


@app.get("/")
def index():
    return FileResponse(os.path.join(WEB_DIR, "nodehost.html"))


app.mount("/web", StaticFiles(directory=WEB_DIR), name="web")
