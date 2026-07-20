#!/usr/bin/env python3
"""linkcpp controller — control plane.

Registry + model manager + planner + inference driver + multi-API gateway + web UI.
Runs GPU-less; spawns the master llama-server that drives the bound nodes as RPC
workers.  uvicorn controller.app:app --host 0.0.0.0 --port 9000
"""
import os, time, json, uuid, asyncio, subprocess, glob
import httpx
from fastapi import FastAPI, HTTPException, Request
from fastapi.responses import StreamingResponse, JSONResponse, FileResponse
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel

from controller.model_catalog import is_embedding_model, model_label
from controller.planner import read_model, plan as run_plan
from controller.versioning import (
    backend_from_runtime,
    backend_identity,
    backend_label,
    backend_report,
    compatibility_report,
    incompatible_nodes,
    runtime_identity,
    runtime_label,
)

MODEL_DIR = os.environ.get("LINKCPP_MODEL_DIR", "/models")
LLAMA_SERVER = os.environ.get("LLAMA_SERVER_BIN", "/workspace/build/bin/llama-server")
MASTER_PORT = int(os.environ.get("LINKCPP_MASTER_PORT", "8080"))
WEB_DIR = os.path.join(os.path.dirname(__file__), "web")
CID = os.environ.get("LINKCPP_CONTROLLER_ID", "linkcpp-controller-main")

app = FastAPI(title="linkcpp controller")
NODES = {}            # id -> {id, host, agent_port, rpc_port, info}
SERVING = {"model": None, "master": None, "plan": None, "np": None,
           "phase": "idle", "detail": ""}   # phase: idle|loading|running|error
DL = {}               # filename -> {"total","done","status"}
STAGE_DIR = os.environ.get("LINKCPP_STAGE_DIR", "/stage")   # fast local model staging


# ----------------------------- helpers ------------------------------------
REG_FILE = os.path.join(STAGE_DIR, "registry.json")


def _save_registry():
    try:
        os.makedirs(STAGE_DIR, exist_ok=True)
        with open(REG_FILE, "w") as f:
            json.dump([{"host": n["host"], "agent_port": n["agent_port"],
                        "rpc_port": n["rpc_port"]} for n in NODES.values()], f)
    except Exception:
        pass


@app.on_event("startup")
async def _restore_registry():
    """Re-bind previously registered nodes after a controller restart."""
    try:
        saved = json.load(open(REG_FILE))
    except Exception:
        return
    for s in saved:
        try:
            info = await agent(s["host"], s["agent_port"], "POST", "/bind",
                               {"controller_id": CID})
            nid = f"{s['host']}:{s['agent_port']}"
            NODES[nid] = {"id": nid, "host": s["host"], "agent_port": s["agent_port"],
                          "rpc_port": s["rpc_port"], "info": info}
        except Exception:
            pass


def node_list():
    def backend(info):
        return info.get("backend") or backend_from_runtime(info.get("runtime"))
    return [{"id": n["id"], "host": n["host"], "agent_port": n["agent_port"],
             "rpc_port": n["rpc_port"], **n["info"],
             "runtime_compatibility": compatibility_report(n["info"].get("runtime")),
             "backend": backend(n["info"]),
             "backend_compatibility": backend_report(backend(n["info"]))}
            for n in NODES.values()]


def _runtime_error(report):
    actual = runtime_label(report.get("actual")) if report.get("actual") else "missing runtime identity"
    return f"protocol mismatch; expected {runtime_label(report.get('expected'))}; got {actual}"


def _require_node_runtime(info):
    report = compatibility_report(info.get("runtime"))
    if not report["compatible"]:
        raise HTTPException(409, _runtime_error(report))
    return report


def _require_cluster_runtime():
    bad = incompatible_nodes({"id": n["id"], "name": n["id"], "runtime": n["info"].get("runtime")}
                             for n in NODES.values())
    if bad:
        names = ", ".join(n.get("id") or "" for n in bad)
        raise HTTPException(409, f"protocol-incompatible nodes: {names}")


def planner_nodes():
    """bound nodes -> planner node dicts, in registration order (pipeline order)."""
    out = []
    for n in NODES.values():
        i = n["info"]
        out.append({"vram": float(i.get("vram_budget_gib", 0)),
                    "ram": float(i.get("ram_budget_gib", 0)),
                    "cores": int(i.get("cores", 0))})
    return out


async def agent(host, port, method, path, json_body=None, timeout=15):
    url = f"http://{host}:{port}{path}"
    async with httpx.AsyncClient(timeout=timeout) as c:
        r = await c.request(method, url, json=json_body)
        r.raise_for_status()
        return r.json()


# ----------------------------- registry -----------------------------------
class AddNode(BaseModel):
    addr: str = ""          # "host:agent_port" as shown by a node-host (preferred)
    host: str = ""
    agent_port: int = 9101
    rpc_port: int = 0       # 0 = learn from the node's /info


@app.get("/api/nodes")
def api_nodes():
    return {"controller_id": CID, "nodes": node_list()}


@app.get("/api/runtime")
def api_runtime():
    return {"runtime": runtime_identity(), "backend": backend_identity(),
            "label": runtime_label(), "backend_label": backend_label(),
            "nodes": node_list()}


@app.post("/api/nodes")
async def api_add_node(n: AddNode):
    host, agent_port = n.host, n.agent_port
    if n.addr:
        h, _, p = n.addr.strip().rpartition(":")
        host = h or n.addr.strip()
        if p.isdigit():
            agent_port = int(p)
    if not host:
        raise HTTPException(400, "provide addr 'host:port' (or host + agent_port)")
    info = await agent(host, agent_port, "GET", "/info")
    if info.get("bound_to") and info["bound_to"] != CID:
        raise HTTPException(409, f"node already bound to {info['bound_to']}")
    _require_node_runtime(info)
    info = await agent(host, agent_port, "POST", "/bind", {"controller_id": CID})
    _require_node_runtime(info)
    rpc_port = n.rpc_port or int(info.get("rpc_port", 50052))   # learn from the node
    nid = f"{host}:{agent_port}"
    NODES[nid] = {"id": nid, "host": host, "agent_port": agent_port,
                  "rpc_port": rpc_port, "info": info}
    _save_registry()
    return {"id": nid, **info}


@app.delete("/api/nodes/{nid}")
async def api_del_node(nid: str):
    n = NODES.pop(nid, None)
    if not n:
        raise HTTPException(404, "unknown node")
    try:
        await agent(n["host"], n["agent_port"], "POST", "/unbind")
    except Exception:
        pass
    _save_registry()
    return {"removed": nid}


# ----------------------------- models -------------------------------------
@app.get("/api/models")
def api_models():
    files = sorted(glob.glob(os.path.join(MODEL_DIR, "*.gguf")))
    out = []
    for f in files:
        name = os.path.basename(f)
        if is_embedding_model(name):
            continue
        size_gib = round(os.path.getsize(f) / 1024**3, 2)
        out.append({"name": name,
                    "label": model_label(name, size_gib),
                    "size_gib": size_gib})
    return {"dir": MODEL_DIR, "models": out, "downloads": DL}


class Download(BaseModel):
    url: str
    name: str


async def _download(url, dest, name):
    DL[name] = {"total": 0, "done": 0, "status": "downloading"}
    try:
        async with httpx.AsyncClient(timeout=None, follow_redirects=True) as c:
            async with c.stream("GET", url) as r:
                r.raise_for_status()
                DL[name]["total"] = int(r.headers.get("content-length", 0))
                with open(dest, "wb") as fh:
                    async for chunk in r.aiter_bytes(1 << 20):
                        fh.write(chunk); DL[name]["done"] += len(chunk)
        DL[name]["status"] = "done"
    except Exception as e:
        DL[name]["status"] = f"error: {e}"


@app.post("/api/models/download")
async def api_download(d: Download):
    dest = os.path.join(MODEL_DIR, d.name)
    asyncio.create_task(_download(d.url, dest, d.name))
    return {"started": d.name}


# ----------------------------- plan / serve -------------------------------
class PlanReq(BaseModel):
    model: str
    ctx: int = 4096
    parallel: int = 1
    kv_bits: int = 16
    cache_type_k: str = "f16"
    cache_type_v: str = "f16"


def _do_plan(req: PlanReq):
    _require_cluster_runtime()
    nodes = planner_nodes()
    if not nodes:
        raise HTTPException(400, "no bound nodes")
    m = read_model(os.path.join(MODEL_DIR, req.model))
    return run_plan(m, nodes, req.ctx, req.parallel, req.kv_bits,
                    cache_type_k=req.cache_type_k, cache_type_v=req.cache_type_v)


@app.post("/api/plan")
def api_plan(req: PlanReq):
    return _do_plan(req)


def _stage_model(name):
    """Copy the model from the (slow) FUSE model dir to fast local storage once,
    reporting progress into SERVING['detail']. Returns the path to read from."""
    src = os.path.join(MODEL_DIR, name)
    try:
        os.makedirs(STAGE_DIR, exist_ok=True)
        dst = os.path.join(STAGE_DIR, name)
        total = os.path.getsize(src)
        if os.path.exists(dst) and os.path.getsize(dst) == total:
            SERVING["detail"] = "model already staged"
            return dst
        tmp = dst + ".part"
        done = 0
        gib = total / 1024**3
        with open(src, "rb") as fi, open(tmp, "wb") as fo:
            while True:
                b = fi.read(16 << 20)   # 16 MiB
                if not b:
                    break
                fo.write(b); done += len(b)
                SERVING["detail"] = f"staging model {done*100//total}% ({done/1024**3:.1f}/{gib:.1f} GiB)"
        os.replace(tmp, dst)
        return dst
    except Exception:
        return src   # fall back to reading from the mount


async def _serve_task(req, result, active, rpc_eps):
    try:
        SERVING.update(phase="loading", detail="staging model", model=req.model,
                       plan=result, np=req.parallel)
        model_path = await asyncio.to_thread(_stage_model, req.model)
        SERVING["detail"] = "starting workers"
        for n, _ in active:
            await agent(n["host"], n["agent_port"], "POST", "/start_worker",
                        {"port": n["rpc_port"]}, timeout=30)
        await asyncio.sleep(2)
        ts = ",".join(str(p["n_layers"]) for _, p in active)
        cmd = [LLAMA_SERVER, "-m", model_path, "-ngl", "99",
               "--rpc", ",".join(rpc_eps), "--tensor-split", ts,
               "-np", str(req.parallel), "-c", str(req.ctx * req.parallel),
               "--host", "127.0.0.1", "--port", str(MASTER_PORT)]
        ot = active[0][1].get("ot") if active else None
        if ot:
            cmd += ["-ot", ot]
        env = dict(os.environ, CUDA_VISIBLE_DEVICES="")   # GPU-less master
        _stop_master()
        SERVING["detail"] = "loading model into GPUs"
        SERVING["master"] = subprocess.Popen(cmd, env=env,
                                              stdout=open("/tmp/master.log", "w"),
                                              stderr=subprocess.STDOUT)
        ok = await _wait_master()
        if ok:
            SERVING.update(phase="running", detail="")
        else:
            SERVING.update(phase="error", detail="master did not become healthy")
    except Exception as e:
        SERVING.update(phase="error", detail=str(e))


@app.post("/api/serve")
async def api_serve(req: PlanReq):
    result = _do_plan(req)
    if not result["feasible"]:
        return JSONResponse(status_code=400, content={"error": "infeasible", "plan": result})
    nodes = list(NODES.values())
    active = [(nodes[p["node"]], p) for p in result["placement"] if p["n_layers"]]
    rpc_eps = [f"{n['host']}:{n['rpc_port']}" for n, _ in active]
    # kick off async; return immediately so the UI/API never block on the long load
    asyncio.create_task(_serve_task(req, result, active, rpc_eps))
    return {"accepted": req.model, "phase": "loading",
            "rpc_workers": rpc_eps, "plan": result}


def _stop_master():
    m = SERVING.get("master")
    if m and m.poll() is None:
        m.terminate()
        try:
            m.wait(timeout=8)
        except Exception:
            m.kill()
    SERVING["master"] = None


async def _wait_master(secs=900):
    async with httpx.AsyncClient(timeout=3) as c:
        for _ in range(secs // 2):
            m = SERVING.get("master")
            if m is not None and m.poll() is not None:
                return False   # master exited/crashed
            try:
                r = await c.get(f"http://127.0.0.1:{MASTER_PORT}/health")
                if r.status_code == 200:
                    return True
            except Exception:
                pass
            await asyncio.sleep(2)
    return False


def _phase():
    """Reconcile stored phase with the actual master process."""
    m = SERVING.get("master")
    alive = m is not None and m.poll() is None
    if SERVING["phase"] == "running" and not alive:
        SERVING.update(phase="error", detail="master exited")
    return SERVING["phase"]


@app.get("/api/status")
def api_status():
    phase = _phase()
    return {"controller_id": CID,
            "serving": SERVING["model"] if phase in ("loading", "running") else None,
            "phase": phase, "detail": SERVING["detail"],
            "running": phase == "running", "parallel": SERVING["np"],
            "plan": SERVING["plan"], "nodes": len(NODES)}


@app.post("/api/stop")
def api_stop():
    _stop_master()
    SERVING.update(model=None, phase="idle", detail="")
    return {"stopped": True}


# ----------------------------- gateway ------------------------------------
MASTER = f"http://127.0.0.1:{MASTER_PORT}"


async def _master_chat(payload):
    async with httpx.AsyncClient(timeout=None) as c:
        r = await c.post(f"{MASTER}/v1/chat/completions", json=payload)
        r.raise_for_status()
        return r.json()


def _require_serving():
    _require_cluster_runtime()
    if _phase() != "running":
        raise HTTPException(503, f"model not ready (phase: {SERVING['phase']} {SERVING['detail']})")


def _model_created(model_id):
    try:
        path = model_id if os.path.isabs(model_id) else os.path.join(MODEL_DIR, model_id)
        path = os.path.abspath(path)
        root = os.path.abspath(MODEL_DIR)
        if (path == root or path.startswith(root + os.sep)) and os.path.exists(path):
            return int(os.path.getmtime(path))
    except Exception:
        pass
    return 0


def _openai_model_list(model_id):
    data = []
    if model_id:
        data.append({
            "id": model_id,
            "object": "model",
            "created": _model_created(model_id),
            "owned_by": "linkcpp",
        })
    return {"object": "list", "data": data}


def _served_model():
    return SERVING.get("model") if _phase() == "running" else None


def _anthropic_model_list(model_id):
    data = []
    if model_id:
        created = _model_created(model_id)
        data.append({
            "type": "model",
            "id": model_id,
            "display_name": os.path.splitext(os.path.basename(model_id))[0] or model_id,
            "created_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(created)),
        })
    return {
        "data": data,
        "has_more": False,
        "first_id": data[0]["id"] if data else None,
        "last_id": data[-1]["id"] if data else None,
    }


@app.api_route("/v1/chat/completions", methods=["POST"])
async def v1_chat(req: Request):
    _require_serving()
    body = await req.json()
    if body.get("stream"):
        async def gen():
            async with httpx.AsyncClient(timeout=None) as c:
                async with c.stream("POST", f"{MASTER}/v1/chat/completions", json=body) as r:
                    async for line in r.aiter_raw():
                        yield line
        return StreamingResponse(gen(), media_type="text/event-stream")
    return JSONResponse(await _master_chat(body))


@app.api_route("/v1/completions", methods=["POST"])
async def v1_comp(req: Request):
    _require_serving()
    body = await req.json()
    async with httpx.AsyncClient(timeout=None) as c:
        r = await c.post(f"{MASTER}/v1/completions", json=body)
        return JSONResponse(r.json(), status_code=r.status_code)


@app.get("/v1/models")
async def v1_models():
    return _openai_model_list(_served_model())


# --- OpenAI Responses API (stateless: no previous_response_id) ---
@app.post("/v1/responses")
async def v1_responses(req: Request):
    _require_serving()
    body = await req.json()
    # translate Responses.input -> chat messages
    msgs = []
    if body.get("instructions"):
        msgs.append({"role": "system", "content": body["instructions"]})
    inp = body.get("input", "")
    if isinstance(inp, str):
        msgs.append({"role": "user", "content": inp})
    else:
        for item in inp:
            role = item.get("role", "user")
            content = item.get("content", "")
            if isinstance(content, list):
                content = "".join(p.get("text", "") for p in content)
            msgs.append({"role": role, "content": content})
    chat = await _master_chat({"messages": msgs, "temperature": body.get("temperature", 1.0),
                               "max_tokens": body.get("max_output_tokens", 256)})
    text = chat["choices"][0]["message"]["content"]
    rid = "resp_" + uuid.uuid4().hex
    return {
        "id": rid, "object": "response", "created_at": int(time.time()),
        "model": SERVING.get("model"), "status": "completed",
        "output": [{"id": "msg_" + uuid.uuid4().hex, "type": "message", "role": "assistant",
                    "content": [{"type": "output_text", "text": text}]}],
        "output_text": text,
        "usage": chat.get("usage", {}),
    }


# --- Anthropic Messages API ---
@app.get("/anthropic/v1/models")
async def anthropic_models():
    return _anthropic_model_list(_served_model())


@app.post("/anthropic/v1/messages")
async def anthropic_messages(req: Request):
    _require_serving()
    body = await req.json()
    msgs = []
    if body.get("system"):
        msgs.append({"role": "system", "content": body["system"]})
    for m in body.get("messages", []):
        content = m["content"]
        if isinstance(content, list):
            content = "".join(b.get("text", "") for b in content if b.get("type") == "text")
        msgs.append({"role": m["role"], "content": content})
    chat = await _master_chat({"messages": msgs,
                               "max_tokens": body.get("max_tokens", 256),
                               "temperature": body.get("temperature", 1.0)})
    text = chat["choices"][0]["message"]["content"]
    u = chat.get("usage", {})
    return {
        "id": "msg_" + uuid.uuid4().hex, "type": "message", "role": "assistant",
        "model": SERVING.get("model"),
        "content": [{"type": "text", "text": text}],
        "stop_reason": "end_turn",
        "usage": {"input_tokens": u.get("prompt_tokens", 0),
                  "output_tokens": u.get("completion_tokens", 0)},
    }


# ----------------------------- web UI -------------------------------------
@app.get("/")
def index():
    return FileResponse(os.path.join(WEB_DIR, "index.html"))


app.mount("/web", StaticFiles(directory=WEB_DIR), name="web")
