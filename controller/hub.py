#!/usr/bin/env python3
"""linkcpp hub — the single all-in-one service (one image, one compose).

Manages, on this machine:
  * a pool of NODES   — each = one GPU + budget, exposed as a ggml-rpc-server worker;
    detail view shows its resources, bound controller, VRAM/RAM usage, and live logs
    (weight loading + inference activity via GGML_RPC_DEBUG).
  * a pool of CONTROLLERS — lightweight instances; each binds nodes, plans, serves a
    model on its own master llama-server, and exposes an OpenAI/Anthropic gateway.

Policy: a node bound to one controller cannot be bound to another.

  uvicorn controller.hub:app --host 0.0.0.0 --port 9000
"""
import os, time, json, uuid, asyncio, subprocess, glob, collections, socket, re, logging
from urllib.parse import quote, urlparse
import httpx
from fastapi import FastAPI, HTTPException, Request, Query, WebSocket
from fastapi.responses import JSONResponse, FileResponse
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel

import hmac as _hmac
from controller import host_resources
from controller import linker_client
from controller import siws
from controller import totp
from controller.model_catalog import is_embedding_model, model_label
from controller.planner import read_model
from controller.runtime_modes import LLAMA_RPC, data_plane_contract, enrich_topology_item
from controller.protocol import DownloadRequest, ModelSource, model_to_dict
from controller.versioning import (
    backend_from_runtime,
    backend_identity,
    backend_report,
    compatibility_report,
    incompatible_nodes,
    runtime_identity,
    runtime_label,
)

MODEL_DIR = os.environ.get("LINKCPP_MODEL_DIR", "/models")
DATA_DIR = os.environ.get("LINKCPP_DATA_DIR", os.path.join(MODEL_DIR, "linkcpp"))
STAGE_DIR = os.environ.get("LINKCPP_STAGE_DIR", os.path.join(DATA_DIR, "stage"))
HUB_STATE_FILE = os.environ.get("LINKCPP_HUB_STATE", os.path.join(DATA_DIR, "hub-state.json"))
INFERENCE_ARCHIVE_DIR = os.environ.get("LINKCPP_INFERENCE_DIR", os.path.join(DATA_DIR, "inference-runs"))
LOAD_DIAGNOSTICS_DIR = os.environ.get("LINKCPP_LOAD_DIAGNOSTICS_DIR", os.path.join(DATA_DIR, "load-diagnostics"))
RPC_BIN = os.environ.get("RPC_BIN", "/workspace/build/bin/ggml-rpc-server")
LLAMA_SERVER = os.environ.get("LLAMA_SERVER_BIN", "/workspace/build/bin/llama-server")
WEB_DIR = os.path.join(os.path.dirname(__file__), "web")
MAX_NODES = int(os.environ.get("LINKCPP_MAX_NODES", "5"))
RPC_BASE = int(os.environ.get("LINKCPP_RPC_PORT_BASE", "50052"))
MASTER_BASE = int(os.environ.get("LINKCPP_MASTER_PORT_BASE", "8080"))
PUBLIC_HUB_URL = os.environ.get("LINKCPP_PUBLIC_HUB_URL", "")
DEFAULT_COMPLETION_TOKENS = int(os.environ.get("LINKCPP_DEFAULT_COMPLETION_TOKENS", "512"))
MAX_COMPLETION_TOKENS = int(os.environ.get("LINKCPP_MAX_COMPLETION_TOKENS", "4096"))
MANAGED_AGENT_URLS = tuple(url.strip().rstrip("/") for url in
                           os.environ.get("LINKCPP_NODE_AGENT_URLS", "http://host.docker.internal:9101").split(",")
                           if url.strip())

app = FastAPI(title="linkcpp hub")
from controller.proxy import install_hub_api
install_hub_api(app)
LOG = logging.getLogger("linkcpp.hub")
LOG.setLevel(os.environ.get("LINKCPP_LOG_LEVEL", "INFO").upper())
if not LOG.handlers:
    _handler = logging.StreamHandler()
    _handler.setFormatter(logging.Formatter("%(levelname)s:%(name)s:%(message)s"))
    LOG.addHandler(_handler)
LOG.propagate = False
NODES = {}   # nid -> node dict; local node-slot-1..N are fixed port slots
CTRLS = {}   # cid -> controller dict
REMOTE_UNITS = {}  # uid -> imported remote unit dict
UNIT_LOAD_SESSIONS = {}  # session id -> unit-owned lifecycle and measurements
DL = {}      # filename -> {total,done,status}
INFERENCE = {}  # controller id -> live request tracker
LOAD_MONITORS = {}  # monitor id -> asyncio task
# Controller cancellation is reached both by the explicit cancel endpoint and
# by the loader task observing its cancellation token.  One lock per
# controller prevents those two paths from sending overlapping stop commands
# to a managed Metal node.
CTRL_TEARDOWN_LOCKS = {}
_LOCAL_SLOTS_LOADED = False
# Operator wallet: the headless hub has no wallet, so the operator sets the address
# that earns this hub's uptime rewards here (persisted). The settlement gateway reads
# it from /api/runtime and registers a hub node on this owner's behalf.
OPERATOR_WALLET = ""

# Per-wallet TOTP 2FA enrollment (persisted in hub-state.json). Shape:
#   { "<wallet>": {"secret": <b32>, "backup": [<sha256 hex>, ...], "enabled": bool} }
# enabled=False means enrollment started (secret issued) but not yet confirmed with a
# live code, so it is NOT enforced at login. Only enabled entries gate the login flow.
TWO_FACTOR = {}

# ---- auth (Sign-In With Solana) -------------------------------------------
# Admin wallet allowlist. EMPTY => auth DISABLED (trusted-LAN mode, backward
# compatible). Set to comma-separated Solana addresses to require SIWS login on
# every control API when the hub is exposed publicly.
ADMIN_WALLETS = set(w.strip() for w in os.environ.get("LINKCPP_ADMIN_WALLETS", "").split(",") if w.strip())
# Machine-to-machine secret for the settlement gateway: a request whose header
# X-Linkcpp-Service-Token matches this bypasses the session gate (read/inference).
SERVICE_TOKEN = os.environ.get("LINKCPP_HUB_SERVICE_TOKEN", "").strip()
# Token presented on hub-to-hub calls (remote units, unit load sessions, report
# push-back). The peer hub must accept it as its own LINKCPP_HUB_SERVICE_TOKEN,
# so a federated fleet shares one secret; defaults to our inbound token.
UNIT_SERVICE_TOKEN = os.environ.get("LINKCPP_UNIT_SERVICE_TOKEN", "").strip() or SERVICE_TOKEN


def _service_headers():
    return {"x-linkcpp-service-token": UNIT_SERVICE_TOKEN} if UNIT_SERVICE_TOKEN else {}
# Operator token-gate: a wallet may operate the hub if it is in ADMIN_WALLETS OR
# holds at least this many KVR on-chain (0 disables the balance gate). Checked at
# login only; the issued session is then trusted until it expires.
MIN_OPERATOR_KVR = float(os.environ.get("LINKCPP_MIN_OPERATOR_KVR", "0") or 0)
KVR_MINT = os.environ.get("LINKCPP_KVR_MINT", "").strip()
SOLANA_RPC = os.environ.get("LINKCPP_SOLANA_RPC", "https://api.devnet.solana.com").strip()
# The gateway is locked by default: without a wallet session nothing but the
# login flow is reachable. Set LINKCPP_REQUIRE_AUTH=0 only for a throwaway
# trusted-LAN instance — this process fronts an unauthenticated control plane,
# so an open gateway means an open control plane.
REQUIRE_AUTH = os.environ.get("LINKCPP_REQUIRE_AUTH", "1").strip().lower() not in ("0", "false", "no")
AUTH_ENABLED = REQUIRE_AUTH or bool(ADMIN_WALLETS) or MIN_OPERATOR_KVR > 0
# Locked with no way in is the safe failure, but it is never the intent, so say
# so loudly rather than letting an operator discover it at the sign-in prompt.
if AUTH_ENABLED and not (ADMIN_WALLETS or MIN_OPERATOR_KVR > 0):
    logging.getLogger("linkcpp").error(
        "auth is required but no wallet may pass it: set LINKCPP_ADMIN_WALLETS "
        "(comma-separated addresses) or LINKCPP_MIN_OPERATOR_KVR with LINKCPP_KVR_MINT")
SESSION_COOKIE = "linkcpp_session"
# Idle lifetime of a wallet session. The UI renews it on real user activity and
# locks itself when this elapses, so the two expire together; the server is what
# actually enforces it. Short by design — this session authorises the whole
# control plane.
SESSION_TTL = int(os.environ.get("LINKCPP_SESSION_TTL", "300"))
# Paths reachable without a session even when auth is on: the login flow + the UI
# shell/static (the UI itself renders a login gate) + health.
_AUTH_OPEN_PREFIXES = ("/api/auth/", "/web/", "/health")


def _kvr_balance(wallet):
    """On-chain KVR balance (float) for a wallet, or None if it can't be determined."""
    if not (KVR_MINT and SOLANA_RPC):
        return None
    try:
        r = httpx.post(SOLANA_RPC, json={
            "jsonrpc": "2.0", "id": 1, "method": "getTokenAccountsByOwner",
            "params": [wallet, {"mint": KVR_MINT}, {"encoding": "jsonParsed"}],
        }, timeout=8.0)
        r.raise_for_status()
        total = 0.0
        for v in ((r.json().get("result") or {}).get("value") or []):
            amt = v["account"]["data"]["parsed"]["info"]["tokenAmount"]
            total += float(amt.get("uiAmount") or 0)
        return total
    except Exception:
        logging.getLogger("linkcpp").warning("KVR balance check failed for %s", wallet)
        return None


def _operator_authorized(wallet):
    """A wallet may operate the hub if it is an explicit admin OR holds at least the
    configured minimum KVR balance (checked on-chain)."""
    if wallet in ADMIN_WALLETS:
        return True
    if MIN_OPERATOR_KVR > 0:
        bal = _kvr_balance(wallet)
        return bal is not None and bal >= MIN_OPERATOR_KVR
    return False


def _bearer_or_cookie(request):
    tok = request.cookies.get(SESSION_COOKIE) or ""
    if not tok:
        auth = request.headers.get("authorization", "")
        if auth.lower().startswith("bearer "):
            tok = auth[7:].strip()
    return tok


def _authed_wallet(request):
    tok = _bearer_or_cookie(request)
    # Sessions are only issued to authorized wallets (see /api/auth/verify), so a
    # valid, unexpired session is sufficient here.
    w = siws.verify_session(tok) if tok else None
    return w or None


# Endpoints a node-scoped token may reach: low-risk participation only (poll the
# demand market, volunteer/self-enroll, pull a shard, report status). Operator/
# asset actions are NOT here and still require a full 2FA session.
_NODE_TOKEN_PREFIXES = (
    "/api/shard-demand", "/api/shard-volunteer", "/api/shard-enroll",
    "/api/node-reports", "/api/proxy/models/", "/api/models/",
    "/api/auth/status",
    "/api/expert-demand", "/api/expert-volunteer", "/api/expert-coverage",
    "/api/expert-dispatch-map", "/api/moe/recruitment",
)


def _authed_node(request):
    """Wallet behind a valid node-scoped bearer token (minted by
    /api/auth/node-token from a wallet signature, no 2FA)."""
    tok = _bearer_or_cookie(request)
    return (siws.verify_token(tok, "node") if tok else None) or None


# --------------------------- linker delegation ----------------------------
# Nodes, controllers, planning, model loading and runtime state are owned by the
# linker service and reached only over its API. These prefixes are forwarded
# verbatim; everything else stays with the gateway.
#
# Deliberately NOT delegated:
#   /api/auth/*        session issuance, 2FA, node tokens. Linker never sees a
#                      browser session — see controller/linker_client.py.
#   /api/contributions the settlement surface the payout service polls.
#   the MoE expert market and the external-controller registration below.
#     Linker exposes same-named routes, but this hub carries its own
#     implementation (expert dispatch ports, relay registry, recruitment,
#     scarcity-weighted contribution flush). Delegating would strand that, so
#     the market stays here until the two are deliberately reconciled.
#   the stage/ring proxy routes, which are this gateway's own subsystem with
#     its own module-boundary tests.
_DELEGATED_PREFIXES = (
    "/api/nodes", "/api/controllers", "/api/models", "/api/gpus",
    "/api/runtime", "/api/kv-cache-types", "/api/operator",
    "/api/unit/", "/api/topology/", "/api/linker", "/api/version",
    "/api/shard-demand", "/api/shard-volunteer", "/api/shard-enroll",
    "/api/node-reports",
    "/c/",
)
# Paths inside a delegated prefix that this hub still answers itself.
_DELEGATION_EXCEPTIONS = (
    "/api/controllers/external",   # external-controller registration (local)
)


def _is_delegated(path):
    if path.startswith("/api/auth/") or path == "/api/contributions":
        return False
    if any(path.startswith(p) for p in _DELEGATION_EXCEPTIONS):
        return False
    return any(path.startswith(prefix) for prefix in _DELEGATED_PREFIXES)


# Registered before the auth gate so that, with Starlette building the stack
# outermost-last, requests are authenticated *before* anything is forwarded.
@app.middleware("http")
async def _linker_delegation(request: Request, call_next):
    if _is_delegated(request.url.path):
        return await linker_client.proxy(request)
    return await call_next(request)


@app.middleware("http")
async def _auth_gate(request: Request, call_next):
    if not AUTH_ENABLED:                       # auth disabled -> fully open (LAN mode)
        return await call_next(request)
    path = request.url.path
    if path == "/" or any(path.startswith(p) for p in _AUTH_OPEN_PREFIXES):
        return await call_next(request)
    st = request.headers.get("x-linkcpp-service-token", "")
    if SERVICE_TOKEN and st and _hmac.compare_digest(st, SERVICE_TOKEN):
        return await call_next(request)       # trusted machine-to-machine (gateway)
    if _authed_wallet(request):
        return await call_next(request)       # admin session
    # A node-scoped token (wallet signature, no 2FA) reaches participation
    # endpoints only — enough for an autonomous node, not for operator actions.
    if any(path.startswith(p) for p in _NODE_TOKEN_PREFIXES) and _authed_node(request):
        return await call_next(request)
    return _unauthenticated(request)


def _wants_document(request):
    """True when a browser is navigating, rather than a script calling an API.

    Sec-Fetch-Dest is the reliable signal and every current browser sends it;
    the Accept sniff only catches older ones. Either way a miss just falls back
    to the JSON body, which is the safe direction — a stray lock screen in an
    XHR would be far more confusing than a JSON 401 in a tab.
    """
    if request.headers.get("sec-fetch-dest") == "document":
        return True
    if request.headers.get("sec-fetch-mode") == "navigate":
        return True
    accept = request.headers.get("accept", "")
    return "text/html" in accept and "application/json" not in accept


def _unauthenticated(request):
    """401 for API callers, a lock screen for a browser that navigated here.

    Linker's SPA opens in its own window at /linker. It has no idea this
    gateway's session expired, so without this it would render its own error
    against a wall of 401s. Serving the gateway's shell instead means that
    window shows the same lock screen as the main one, and signing in there
    brings it straight back.
    """
    if _wants_document(request):
        return FileResponse(os.path.join(WEB_DIR, "hub.html"), status_code=401)
    return JSONResponse({"error": "authentication required"}, status_code=401)


@app.on_event("startup")
async def _startup_restore_state():
    _ensure_local_slots()
    await _discover_managed_agents()


# ------------------------------- helpers ----------------------------------
def list_gpus():
    return host_resources.local_gpus()
















def gpu_used_gib(uuid_):
    try:
        out = subprocess.check_output(
            ["nvidia-smi", "--query-gpu=uuid,memory.used",
             "--format=csv,noheader,nounits"], text=True).strip().splitlines()
        for r in out:
            u, used = [x.strip() for x in r.split(",")]
            if u == uuid_:
                return round(int(used) / 1024, 2)
    except Exception:
        pass
    return 0.0


def tail(path, n=200):
    try:
        with open(path, "rb") as f:
            return b"".join(collections.deque(f, maxlen=n)).decode("utf-8", "replace")
    except Exception:
        return ""


def _parse_master_progress(log_text):
    progress = []
    for line in log_text.splitlines():
        task = re.search(r"task\s+(\d+)", line)
        decoded = re.search(r"n_decoded\s*=\s*(\d+)", line)
        prompt = re.search(r"prompt processing, n_tokens\s*=\s*(\d+), progress\s*=\s*([0-9.]+)", line)
        timing = re.search(r"(prompt eval time|eval time|total time)\s*=\s*([0-9.]+)\s*ms\s*/\s*(\d+)\s*tokens", line)
        if decoded:
            progress.append({
                "kind": "decode_progress",
                "task": int(task.group(1)) if task else None,
                "n_decoded": int(decoded.group(1)),
                "tokens_per_second": float(re.search(r"tg\s*=\s*([0-9.]+)", line).group(1)) if re.search(r"tg\s*=\s*([0-9.]+)", line) else None,
                "tokens_per_second_3s": float(re.search(r"tg_3s\s*=\s*([0-9.]+)", line).group(1)) if re.search(r"tg_3s\s*=\s*([0-9.]+)", line) else None,
                "line": line,
            })
        elif prompt:
            progress.append({
                "kind": "prompt_progress",
                "task": int(task.group(1)) if task else None,
                "n_tokens": int(prompt.group(1)),
                "progress": float(prompt.group(2)),
                "line": line,
            })
        elif timing:
            progress.append({
                "kind": timing.group(1).replace(" ", "_"),
                "task": int(task.group(1)) if task else None,
                "elapsed_ms": float(timing.group(2)),
                "tokens": int(timing.group(3)),
                "line": line,
            })
    return progress[-12:]


def _log_event(event, **fields):
    payload = {"event": event, **fields}
    try:
        LOG.info("linkcpp_event %s", json.dumps(payload, sort_keys=True, default=str))
    except Exception:
        LOG.info("linkcpp_event %s %s", event, payload)






def _rpc_topology(c, active, runtime_mode=LLAMA_RPC):
    topology = []
    total = len(active)
    for idx, (nid, placement) in enumerate(active):
        n = NODES.get(nid, {})
        item = {
            "position": idx,
            "is_first": idx == 0,
            "is_last": idx == total - 1,
            "node_id": nid,
            "node_name": n.get("name"),
            "node_kind": n.get("kind", "local"),
            "rpc_endpoint": _node_rpc_endpoint(n) if n else "",
            "remote_unit_url": n.get("remote_unit_url"),
            "remote_source_node_id": n.get("remote_source_node_id"),
            "remote_source_rpc_endpoint": n.get("remote_source_rpc_endpoint"),
            "layers": placement.get("layers"),
            "n_layers": placement.get("n_layers"),
        }
        topology.append(enrich_topology_item(runtime_mode, n, item))
    # Cross-host ring: a remote node (e.g. a phone at a LAN IP) cannot reach a
    # local agent advertised on 127.0.0.1, so rewrite every loopback stage/rpc
    # endpoint to the hub's LAN-reachable host once any stage is off-box.
    if any(not str(t.get("stage_endpoint", "")).startswith(("127.", "localhost"))
           and not str(t.get("rpc_endpoint", "")).startswith(("127.", "localhost"))
           for t in topology):
        lan_host = _hub_reachable_url().split("://", 1)[-1].split(":", 1)[0]
        for t in topology:
            for key in ("stage_endpoint", "rpc_endpoint"):
                val = str(t.get(key) or "")
                if val.startswith(("127.", "localhost")):
                    t[key] = lan_host + ":" + val.rsplit(":", 1)[-1]
    data_plane = data_plane_contract(runtime_mode, topology)
    data_plane["established"] = False
    if topology:
        _log_event("controller_rpc_topology", controller_id=c.get("id"),
                   model=c.get("model"), topology=topology, data_plane=data_plane)
    for item in topology:
        item["data_plane"] = data_plane
    return topology


def _master_log_path(c):
    return f"/tmp/master-{c['id']}.log"












KV_CACHE_TYPES_FALLBACK = ["f32", "f16", "bf16", "q8_0", "q4_0", "q4_1", "iq4_nl", "q5_0", "q5_1"]
_KV_CACHE_TYPES_CACHE = None






def _local_slot_id(index):
    return f"node-slot-{index}"


def _default_slot_name(index):
    return f"Slot {index}"


def _slot_log_path(nid):
    return f"/tmp/node-{nid}.log"




def _new_local_slot(index):
    nid = _local_slot_id(index)
    return {"id": nid, "name": _default_slot_name(index), "gpu_uuid": "",
            "kind": "local", "slot": index, "assigned": False,
            "rpc_host": "127.0.0.1", "gpu_name": "Unassigned",
            "vram": 0.0, "ram": 0.0, "cores": 0,
            "rpc_port": RPC_BASE + index - 1, "bound_to": None,
            "worker": None, "ram_used": 0.0, "log": _slot_log_path(nid)}


def _local_slot_snapshot(n):
    return {
        "id": n["id"],
        "slot": n.get("slot"),
        "assigned": bool(n.get("assigned")),
        "name": n.get("name", ""),
        "gpu_uuid": n.get("gpu_uuid", ""),
        "gpu_name": n.get("gpu_name", ""),
        "vram": n.get("vram", 0.0),
        "ram": n.get("ram", 0.0),
        "cores": n.get("cores", 0),
    }


def _controller_snapshot(c):
    return {
        "id": c["id"],
        "name": c.get("name") or c["id"],
        "nodes": list(c.get("nodes", [])),
        "ctx": int(c.get("ctx") or 4096),
        "parallel": int(c.get("parallel") or 1),
        "last_load": dict(c.get("last_load") or {}),
        "master_port": int(c.get("master_port") or 0),
    }


def _remote_unit_snapshot(r):
    return {
        "id": r["id"],
        "name": r.get("name", ""),
        "base_url": r.get("base_url", ""),
        "runtime": r.get("runtime"),
        "backend": r.get("backend"),
        "owner_controller_id": r.get("owner_controller_id"),
        "controllers": r.get("controllers", []),
        "unit_control_protocols": r.get("unit_control_protocols", []),
        "node_ids": r.get("node_ids", []),
        "updated_at": r.get("updated_at"),
    }


def _external_node_snapshot(n):
    keys = (
        "id", "kind", "name", "agent_url", "report_url", "gpu_uuid", "gpu_name", "vram", "ram", "cores", "logical_slot",
        "rpc_host", "rpc_port", "bound_to", "worker_running", "ram_used", "resources", "owner",
        "capabilities", "host_platform", "runtime", "backend", "remote_unit_id", "remote_unit_name",
        "remote_unit_url", "remote_controller_id", "remote_controller_name",
        "owner_controller_id",
        "remote_source_node_id", "remote_source_rpc_endpoint",
    )
    return {k: n.get(k) for k in keys if k in n}


def _persist_hub_state():
    data = {
        "version": 1,
        "local_slots": [
            _local_slot_snapshot(NODES[_local_slot_id(i)])
            for i in range(1, MAX_NODES + 1)
            if _local_slot_id(i) in NODES
        ],
        "controllers": [_controller_snapshot(c) for c in CTRLS.values() if not c.get("external")],
        "remote_units": [_remote_unit_snapshot(r) for r in REMOTE_UNITS.values()],
        "external_nodes": [
            _external_node_snapshot(n)
            for n in NODES.values()
            if n.get("kind") in ("remote_unit_node", "agent")
        ],
        "operator_wallet": OPERATOR_WALLET,
        "two_factor": TWO_FACTOR,
    }
    try:
        os.makedirs(os.path.dirname(HUB_STATE_FILE), exist_ok=True)
        tmp = HUB_STATE_FILE + ".tmp"
        with open(tmp, "w", encoding="utf-8") as f:
            json.dump(data, f, indent=2)
        os.replace(tmp, HUB_STATE_FILE)
    except Exception:
        pass


def _persist_local_slots():
    _persist_hub_state()


def _migrate_unified_memory_local_slots():
    """Move legacy GB10 RAM-only slot budgets into their CUDA VRAM budget.

    Older hub state predates GB10 unified-memory discovery, so an operator may
    have configured a logical slot as ``vram=0, ram=N``.  That makes the GGUF
    planner treat the slot as CPU-only even though CUDA can allocate from the
    same unified-memory pool.  Do not rewrite bound slots: their running agent
    still has the old environment and must be unbound before its budget can be
    changed.
    """
    try:
        gpus = {gpu["uuid"]: gpu for gpu in list_gpus()}
    except Exception:
        return
    changed = False
    for slot in _local_slots(load_state=False):
        gpu = gpus.get(slot.get("gpu_uuid"))
        if (not gpu or "gb10" not in str(gpu.get("name", "")).lower()
                or slot.get("bound_to") or float(slot.get("vram") or 0) > 0):
            continue
        legacy_ram = float(slot.get("ram") or 0)
        if legacy_ram <= 0:
            continue
        capacity = float(gpu.get("vram_total_gib") or 0)
        if capacity <= 0:
            continue
        slot["vram"] = min(legacy_ram, capacity)
        slot["ram"] = 0.0
        changed = True
    if changed:
        _persist_local_slots()


def _next_master_port(preferred=None):
    used = {int(c.get("master_port") or 0) for c in CTRLS.values()}
    if preferred and int(preferred) not in used:
        return int(preferred)
    port = MASTER_BASE
    while port in used:
        port += 1
    return port


def _load_hub_state():
    global _LOCAL_SLOTS_LOADED, OPERATOR_WALLET, TWO_FACTOR
    if _LOCAL_SLOTS_LOADED:
        return
    _LOCAL_SLOTS_LOADED = True
    try:
        with open(HUB_STATE_FILE, "r", encoding="utf-8") as f:
            data = json.load(f)
    except Exception:
        return
    OPERATOR_WALLET = (data.get("operator_wallet") or "").strip()
    tf = data.get("two_factor")
    if isinstance(tf, dict):
        TWO_FACTOR = tf
    for item in data.get("local_slots", []):
        try:
            index = int(item.get("slot") or 0)
        except Exception:
            continue
        if index < 1 or index > MAX_NODES:
            continue
        nid = _local_slot_id(index)
        n = NODES.get(nid)
        if not n:
            n = _new_local_slot(index)
            NODES[nid] = n
        n.update({
            "name": item.get("name") or _default_slot_name(index),
            "assigned": bool(item.get("assigned")),
            "gpu_uuid": item.get("gpu_uuid", ""),
            "gpu_name": item.get("gpu_name") or ("Unassigned" if not item.get("assigned") else "GPU"),
            "vram": float(item.get("vram") or 0),
            "ram": float(item.get("ram") or 0),
            "cores": int(item.get("cores") or 0),
            "bound_to": None,
            "worker": None,
            "ram_used": 0.0,
        })
    for unit in data.get("remote_units", []):
        uid = unit.get("id")
        base_url = unit.get("base_url")
        if not uid or not base_url:
            continue
        REMOTE_UNITS[uid] = {
            "id": uid,
            "name": unit.get("name") or urlparse(base_url).netloc,
            "base_url": base_url,
            "runtime": unit.get("runtime"),
            "backend": unit.get("backend") or backend_from_runtime(unit.get("runtime")),
            "owner_controller_id": unit.get("owner_controller_id"),
            "controllers": unit.get("controllers", []),
            "node_ids": unit.get("node_ids", []),
            "updated_at": unit.get("updated_at"),
        }
    for item in data.get("external_nodes", []):
        nid = item.get("id")
        kind = item.get("kind")
        if not nid or kind not in ("remote_unit_node", "agent"):
            continue
        NODES[nid] = {
            "id": nid,
            "kind": kind,
            "name": item.get("name") or nid,
            "agent_url": item.get("agent_url"),
            "report_url": item.get("report_url"),
            "gpu_uuid": item.get("gpu_uuid", ""),
            "gpu_name": item.get("gpu_name") or ("Managed node" if kind == "agent" else "Remote GPU"),
            "logical_slot": item.get("logical_slot"),
            "vram": item.get("vram", 0.0),
            "ram": item.get("ram", 0.0),
            "cores": item.get("cores", 0),
            "rpc_host": item.get("rpc_host", "127.0.0.1"),
            "rpc_port": int(item.get("rpc_port") or 50052),
            "bound_to": None,
            "worker": None,
            "worker_running": False if kind == "agent" else bool(item.get("worker_running", True)),
            "ram_used": 0.0,
            "log": "",
            "resources": item.get("resources", {}),
            "capabilities": item.get("capabilities", {}),
            "host_platform": item.get("host_platform", {}),
            "owner": (item.get("owner") or "").strip(),
            "runtime": item.get("runtime"),
            "backend": item.get("backend") or backend_from_runtime(item.get("runtime")),
            "remote_unit_id": item.get("remote_unit_id"),
            "remote_unit_name": item.get("remote_unit_name"),
            "remote_unit_url": item.get("remote_unit_url"),
            "owner_controller_id": item.get("owner_controller_id"),
            "remote_controller_id": item.get("remote_controller_id"),
            "remote_controller_name": item.get("remote_controller_name"),
            "remote_source_node_id": item.get("remote_source_node_id"),
            "remote_source_rpc_endpoint": item.get("remote_source_rpc_endpoint"),
        }
    for item in data.get("controllers", []):
        cid = item.get("id")
        if not cid:
            continue
        c = {
            "id": cid,
            "name": item.get("name") or cid,
            "nodes": [],
            "model": None,
            "ctx": int(item.get("ctx") or 4096),
            "parallel": int(item.get("parallel") or 1),
            "last_load": item.get("last_load") or {},
            "phase": "idle",
            "detail": "",
            "plan": None,
            "master": None,
            "master_port": _next_master_port(item.get("master_port")),
            "operations": {},
        }
        CTRLS[cid] = c
        for nid in item.get("nodes", []):
            n = NODES.get(nid)
            if not n or n.get("bound_to"):
                continue
            n["bound_to"] = cid
            c["nodes"].append(nid)

    # State written before remote units became controller-scoped has no owner
    # field.  Preserve the unambiguous case: if all still-bound projections of
    # that unit belong to one controller, migrate it to that controller.
    for unit in REMOTE_UNITS.values():
        if unit.get("owner_controller_id"):
            continue
        owners = {NODES[nid].get("bound_to") for nid in unit.get("node_ids", [])
                  if nid in NODES and NODES[nid].get("bound_to") in CTRLS}
        if len(owners) == 1:
            owner = owners.pop()
            unit["owner_controller_id"] = owner
            for nid in unit.get("node_ids", []):
                if nid in NODES:
                    NODES[nid]["owner_controller_id"] = owner

    _migrate_unified_memory_local_slots()


def _ensure_local_slots():
    for index in range(1, MAX_NODES + 1):
        nid = _local_slot_id(index)
        if nid not in NODES:
            NODES[nid] = _new_local_slot(index)
        else:
            n = NODES[nid]
            n.setdefault("kind", "local")
            n.setdefault("slot", index)
            n.setdefault("assigned", bool(n.get("gpu_uuid")))
            n.setdefault("rpc_host", "127.0.0.1")
            n["rpc_port"] = RPC_BASE + index - 1
            n.setdefault("log", _slot_log_path(nid))
    _load_hub_state()


def _node_sort_key(n):
    if n.get("kind", "local") == "local":
        return (0, int(n.get("slot") or 9999), n["id"])
    return (1, n.get("remote_unit_name") or "", n["name"], n["id"])


def _local_slots(load_state=True):
    if load_state:
        _ensure_local_slots()
    return [NODES[_local_slot_id(i)] for i in range(1, MAX_NODES + 1)]


def _owned_node_count():
    """Configured local slots and managed agents are this unit's node quota."""
    return sum(1 for n in NODES.values()
               if (n.get("kind", "local") == "local" and _slot_configured(n))
               or n.get("kind") == "agent")


def _next_logical_slot():
    used = {int(n["logical_slot"]) for n in NODES.values()
            if n.get("kind") == "agent" and n.get("logical_slot")}
    for number in range(1, MAX_NODES + 1):
        if number not in used:
            return number
    return None


def _slot_configured(n):
    return n.get("kind", "local") != "local" or bool(n.get("assigned"))






def _proc_alive(p):
    return p is not None and p.poll() is None


def _kill(p):
    if _proc_alive(p):
        p.terminate()
        try:
            p.wait(timeout=6)
        except Exception:
            p.kill()


def _ctrl_ops(c):
    return c.setdefault("operations", {})






def _record_ctrl_op(c, kind, phase, status, progress=0.0, message="", op_id=None,
                    node_id=None, model=None, error=None, details=None):
    oid = op_id or f"{kind}-" + uuid.uuid4().hex[:10]
    ops = _ctrl_ops(c)
    op = ops.get(oid, {"op_id": oid, "type": kind, "created_at": time.time(), "details": {}})
    op.update({"phase": phase, "status": status, "progress": progress,
               "message": message, "updated_at": time.time()})
    if node_id:
        op["node_id"] = node_id
    if model:
        op["model"] = model
    if error:
        op["error"] = error
    if details:
        op.setdefault("details", {}).update(details)
    ops[oid] = op
    _log_event("controller_operation", controller_id=c.get("id"), op_id=oid,
               type=kind, phase=phase, status=status, progress=progress,
               node_id=node_id, model=model, message=message, error=error)
    return op


































async def _agent_request(n, method, path, body=None, timeout=30):
    async with httpx.AsyncClient(timeout=timeout) as client:
        r = await client.request(method, n["agent_url"].rstrip("/") + path, json=body)
        r.raise_for_status()
        return r.json()


async def _agent_request_stream(n, method, path, body=None):
    """Stream a managed-agent response back (raw bytes), e.g. SSE relay from the
    agent's local ring coordinator."""
    async with httpx.AsyncClient(timeout=None) as client:
        async with client.stream(method, n["agent_url"].rstrip("/") + path, json=body) as r:
            r.raise_for_status()
            async for chunk in r.aiter_raw():
                yield chunk














_B58_CHARS = set("123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz")








class AuthChallenge(BaseModel):
    wallet: str = ""


class AuthVerify(BaseModel):
    wallet: str = ""
    nonce: str = ""
    signature: str = ""


@app.get("/api/auth/status")
def api_auth_status(request: Request):
    # enabled=False means the hub runs open (trusted LAN); UI shows no login gate.
    if not AUTH_ENABLED:
        return {"enabled": False, "authenticated": True, "wallet": None, "idle_ttl": 0}
    w = _authed_wallet(request)
    # idle_ttl drives the UI's own lock timer, so both sides expire together
    # instead of the screen staying up against a session the server has dropped.
    return {"enabled": True, "authenticated": bool(w), "wallet": w, "idle_ttl": SESSION_TTL}


@app.get("/api/auth/kvr-balance")
def api_auth_kvr_balance(wallet: str = ""):
    # Open (pre-login) lookup so the sign-in screen can show the operator whether
    # their wallet qualifies before they sign.
    w = (wallet or "").strip()
    bal = _kvr_balance(w) if w else None
    ok = bool(w) and ((w in ADMIN_WALLETS) or (MIN_OPERATOR_KVR > 0 and bal is not None and bal >= MIN_OPERATOR_KVR))
    return {"wallet": w, "balance": bal, "min": MIN_OPERATOR_KVR,
            "admin": w in ADMIN_WALLETS, "gated": MIN_OPERATOR_KVR > 0, "ok": ok}


@app.post("/api/auth/challenge")
def api_auth_challenge(body: AuthChallenge):
    if not AUTH_ENABLED:
        raise HTTPException(400, "auth is disabled")
    w = (body.wallet or "").strip()
    if not _operator_authorized(w):
        raise HTTPException(403, "wallet is not authorized to operate this hub")
    nonce, message = siws.new_challenge(w)
    return {"nonce": nonce, "message": message}


def _twofa_enabled(wallet):
    return bool((TWO_FACTOR.get(wallet) or {}).get("enabled"))


def _issue_session(wallet, body=None):
    resp = JSONResponse(body if body is not None else {"wallet": wallet})
    resp.set_cookie(SESSION_COOKIE, siws.make_session(wallet, ttl=SESSION_TTL),
                    httponly=True, samesite="lax", max_age=SESSION_TTL, path="/")
    return resp


@app.post("/api/auth/touch")
def api_auth_touch(request: Request):
    """Slide the session forward. Called by the UI on real user activity only.

    The idle window has to be driven by the client because the UI polls on a
    timer: renewing on any authenticated request would mean an unattended
    browser holds a session open forever, which is the opposite of what an idle
    timeout is for.
    """
    wallet = _authed_wallet(request)
    if not wallet:
        raise HTTPException(401, "no session to renew")
    return _issue_session(wallet, {"wallet": wallet, "ttl": SESSION_TTL})


@app.post("/api/auth/verify")
def api_auth_verify(body: AuthVerify):
    w = (body.wallet or "").strip()
    if not siws.verify_login(w, (body.nonce or "").strip(), body.signature or ""):
        raise HTTPException(401, "signature verification failed")
    if not _operator_authorized(w):
        raise HTTPException(403, "wallet is not authorized to operate this hub")
    # First factor (wallet signature) verified. If this wallet has confirmed 2FA,
    # withhold the session and issue a short-lived pre-auth token; the client must
    # complete /api/auth/2fa/login with a TOTP or backup code to get the session.
    if _twofa_enabled(w):
        return JSONResponse({"wallet": w, "twofa_required": True,
                             "pre_auth": siws.make_token(w, "pre2fa", 300)})
    return _issue_session(w)


class TwoFAConfirm(BaseModel):
    code: str = ""


class TwoFALogin(BaseModel):
    pre_auth: str = ""
    code: str = ""


@app.get("/api/auth/2fa/status")
def api_2fa_status(request: Request):
    w = _authed_wallet(request)
    if not w:
        raise HTTPException(401, "sign in first")
    return {"wallet": w, "enabled": _twofa_enabled(w)}


@app.post("/api/auth/2fa/enroll")
def api_2fa_enroll(request: Request):
    # Start (or restart) enrollment for the signed-in wallet: mint a fresh secret +
    # backup codes, store them as not-yet-enabled, and return the provisioning data.
    # Backup codes are returned in plaintext ONCE here and only stored hashed.
    w = _authed_wallet(request)
    if not w:
        raise HTTPException(401, "sign in first")
    secret = totp.new_secret()
    codes = totp.new_backup_codes()
    TWO_FACTOR[w] = {"secret": secret, "backup": [totp.hash_backup(c) for c in codes],
                     "enabled": False}
    _persist_hub_state()
    return {"secret": secret, "otpauth_uri": totp.otpauth_uri(secret, w),
            "backup_codes": codes}


@app.post("/api/auth/2fa/confirm")
def api_2fa_confirm(request: Request, body: TwoFAConfirm):
    # Confirm enrollment by proving a live code against the pending secret.
    w = _authed_wallet(request)
    if not w:
        raise HTTPException(401, "sign in first")
    ent = TWO_FACTOR.get(w) or {}
    if not ent.get("secret"):
        raise HTTPException(400, "no enrollment in progress")
    if not totp.verify_code(ent["secret"], body.code or ""):
        raise HTTPException(400, "invalid authenticator code")
    ent["enabled"] = True
    TWO_FACTOR[w] = ent
    _persist_hub_state()
    return {"enabled": True}


@app.post("/api/auth/2fa/disable")
def api_2fa_disable(request: Request, body: TwoFAConfirm):
    # Turn 2FA off — requires a current code (or a backup code) so a hijacked
    # session alone cannot strip the second factor.
    w = _authed_wallet(request)
    if not w:
        raise HTTPException(401, "sign in first")
    ent = TWO_FACTOR.get(w) or {}
    if not ent.get("enabled"):
        raise HTTPException(400, "2FA is not enabled")
    ok = totp.verify_code(ent.get("secret", ""), body.code or "")
    if not ok:
        remaining = totp.verify_and_consume_backup(ent.get("backup", []), body.code or "")
        ok = remaining is not None
    if not ok:
        raise HTTPException(400, "invalid authenticator or backup code")
    TWO_FACTOR.pop(w, None)
    _persist_hub_state()
    return {"enabled": False}


@app.post("/api/auth/2fa/login")
def api_2fa_login(body: TwoFALogin):
    # Second factor: exchange the pre-auth token + a TOTP/backup code for a session.
    w = siws.verify_token(body.pre_auth or "", "pre2fa")
    if not w:
        raise HTTPException(401, "2FA challenge expired — sign in again")
    ent = TWO_FACTOR.get(w) or {}
    if not ent.get("enabled"):
        # 2FA was disabled meanwhile — the wallet sig already passed, issue session.
        return _issue_session(w)
    if totp.verify_code(ent.get("secret", ""), body.code or ""):
        return _issue_session(w)
    remaining = totp.verify_and_consume_backup(ent.get("backup", []), body.code or "")
    if remaining is not None:
        ent["backup"] = remaining
        TWO_FACTOR[w] = ent
        _persist_hub_state()
        return _issue_session(w)
    raise HTTPException(401, "invalid authenticator or backup code")


class NodeTokenReq(BaseModel):
    wallet: str = ""
    nonce: str = ""
    signature: str = ""


NODE_TOKEN_TTL = int(os.environ.get("LINKCPP_NODE_TOKEN_TTL_DAYS", "30") or 30) * 86400


@app.post("/api/auth/node-token")
def api_auth_node_token(body: NodeTokenReq):
    """Mint a long-lived NODE token from a wallet signature alone — no 2FA.

    A node token is scoped (see _NODE_TOKEN_PREFIXES) to low-risk participation:
    poll the demand market, volunteer/self-enroll, pull a shard, report status.
    The wallet is already KVR-gated by _operator_authorized, so the second factor
    is reserved for high-risk operator/asset actions. This lets a headless node
    (e.g. a mobile app with no OTP entry) authenticate to a public hub once and
    keep participating without an interactive 2FA prompt each session."""
    w = (body.wallet or "").strip()
    if not siws.verify_login(w, (body.nonce or "").strip(), body.signature or ""):
        raise HTTPException(401, "signature verification failed")
    if not _operator_authorized(w):
        raise HTTPException(403, "wallet is not authorized to participate")
    _log_event("node_token_issued", wallet=w, ttl_days=NODE_TOKEN_TTL // 86400)
    return {"node_token": siws.make_token(w, "node", NODE_TOKEN_TTL),
            "wallet": w, "expires_in": NODE_TOKEN_TTL}


@app.post("/api/auth/logout")
def api_auth_logout():
    resp = JSONResponse({"ok": True})
    resp.delete_cookie(SESSION_COOKIE, path="/")
    return resp




# ------------------------------- nodes ------------------------------------
def _node_worker_running(n):
    if n.get("kind") == "remote":
        return True
    if n.get("kind") == "remote_unit_node":
        return bool(n.get("worker_running"))
    if n.get("kind") == "agent":
        return bool(n.get("worker_running"))
    return _proc_alive(n.get("worker"))


def node_view(n):
    resources = n.get("resources") or {}
    if n.get("kind") == "agent":
        vram_used = resources.get("vram_used_gib", 0.0)
    elif n.get("kind") in ("remote", "remote_unit_node") or not n.get("gpu_uuid"):
        vram_used = 0.0
    else:
        vram_used = gpu_used_gib(n["gpu_uuid"])
    runtime = _node_runtime(n)
    backend = _node_backend(n)
    protocol_report = (
        compatibility_report(runtime)
        if _slot_configured(n)
        else {"compatible": True, "not_applicable": True, "scope": "protocol", "mismatches": []}
    )
    backend_status = (
        backend_report(backend)
        if _slot_configured(n)
        else {"compatible": True, "not_applicable": True, "scope": "backend", "warnings": []}
    )
    return {"id": n["id"], "kind": n.get("kind", "local"), "name": n["name"],
            "slot": n.get("slot"), "logical_slot": n.get("logical_slot"),
            "assigned": _slot_configured(n),
            "gpu_uuid": n["gpu_uuid"],
            "gpu_name": n["gpu_name"], "vram": n["vram"], "ram": n["ram"],
            "cores": n["cores"], "rpc_host": n.get("rpc_host", "127.0.0.1"),
            "rpc_port": n["rpc_port"], "rpc_endpoint": _node_rpc_endpoint(n),
            "agent_url": n.get("agent_url"),
            "remote_unit_id": n.get("remote_unit_id"),
            "remote_unit_name": n.get("remote_unit_name"),
            "remote_unit_url": n.get("remote_unit_url"),
            "remote_controller_id": n.get("remote_controller_id"),
            "remote_controller_name": n.get("remote_controller_name"),
            "remote_status_error": n.get("remote_status_error", ""),
            "remote_status_updated_at": n.get("remote_status_updated_at"),
            "remote_source_node_id": n.get("remote_source_node_id"),
            "remote_source_rpc_endpoint": n.get("remote_source_rpc_endpoint"),
            "resources": resources,
            "capabilities": n.get("capabilities", {}),
            "host_platform": n.get("host_platform", {}),
            "runtime": runtime,
            "runtime_compatibility": protocol_report,
            "backend": backend,
            "backend_compatibility": backend_status,
            "bound_to": n["bound_to"],
            "bound_to_name": CTRLS[n["bound_to"]]["name"] if n["bound_to"] in CTRLS else None,
            "worker_running": _node_worker_running(n),
            "desired_load": n.get("desired_load"),
            "vram_used_gib": vram_used,
            "ram_used_gib": n.get("ram_used", 0.0)}


def _node_rpc_endpoint(n):
    return f"{n.get('rpc_host', '127.0.0.1')}:{n['rpc_port']}"


def _node_runtime(n):
    if n.get("runtime"):
        return n["runtime"]
    if n.get("kind", "local") == "local" and _slot_configured(n):
        return runtime_identity()
    return None


def _node_backend(n):
    if n.get("backend"):
        return n["backend"]
    if n.get("kind", "local") == "local" and _slot_configured(n):
        return backend_identity()
    return backend_from_runtime(_node_runtime(n))








def _controller_nodes(c):
    return [NODES[nid] for nid in c.get("nodes", []) if nid in NODES]


def _controller_runtime_check(c):
    # Self-enrolled NAT nodes can't be probed by the hub — they self-attest and
    # their ring ABI is checked at the hello handshake, so exclude them here.
    nodes = [n for n in _controller_nodes(c) if not n.get("self_enrolled")]
    bad = incompatible_nodes({**n, "runtime": _node_runtime(n)} for n in nodes)
    return {
        "compatible": not bad,
        "expected": runtime_identity(),
        "label": runtime_label(),
        "nodes": bad,
    }






















async def _remote_unit_node_request(n, method, path, body=None, timeout=15):
    base_url = (n.get("remote_unit_url") or "").rstrip("/")
    if not base_url:
        raise RuntimeError("remote unit URL is missing")
    async with httpx.AsyncClient(timeout=timeout) as client:
        resp = await client.request(method, base_url + path, json=body, headers=_service_headers())
        resp.raise_for_status()
        return resp.json()


async def _remote_unit_node_request_stream(n, method, path, body=None):
    """Stream a remote-unit response back (raw bytes) — SSE relay from the unit's
    ring coordinator."""
    base_url = (n.get("remote_unit_url") or "").rstrip("/")
    if not base_url:
        raise RuntimeError("remote unit URL is missing")
    async with httpx.AsyncClient(timeout=None) as client:
        async with client.stream(method, base_url + path, json=body, headers=_service_headers()) as r:
            r.raise_for_status()
            async for chunk in r.aiter_raw():
                yield chunk






































def _remote_unit_view(r):
    runtime = r.get("runtime")
    backend = r.get("backend") or backend_from_runtime(runtime)
    return {
        "id": r["id"],
        "name": r["name"],
        "base_url": r["base_url"],
        "unit_url": r["base_url"],
        "runtime": runtime,
        "runtime_compatibility": compatibility_report(runtime),
        "backend": backend,
        "backend_compatibility": backend_report(backend),
        "owner_controller_id": r.get("owner_controller_id"),
        "controllers": r.get("controllers", []),
        "node_ids": r.get("node_ids", []),
        "node_count": len(r.get("node_ids", [])),
        "updated_at": r.get("updated_at"),
    }










def _controller_remote_units(cid):
    return [r for r in REMOTE_UNITS.values() if r.get("owner_controller_id") == cid]












async def _discover_managed_agents():
    """Expose configured native agents as unbound unit nodes.

    Discovery is deliberately independent of controllers: an agent is a node
    first, and a controller claims it only through the ordinary bind action.
    Unreachable optional URLs are ignored so a portable Linux hub does not
    fail to start.
    """
    for agent_url in MANAGED_AGENT_URLS:
        parsed = urlparse(agent_url)
        if parsed.scheme not in ("http", "https") or not parsed.netloc:
            continue
        try:
            async with httpx.AsyncClient(timeout=3) as client:
                response = await client.get(agent_url + "/control/status")
                response.raise_for_status()
                info = response.json()
        except Exception:
            continue
        runtime = info.get("runtime")
        if not compatibility_report(runtime).get("compatible"):
            continue
        node_id = info.get("node_id") or "agent-" + uuid.uuid4().hex[:8]
        existing = NODES.get(node_id) or {}
        if node_id not in NODES and _owned_node_count() >= MAX_NODES:
            continue
        resources = info.get("resources") or {}
        backend = info.get("backend") or backend_from_runtime(runtime)
        n = {
            "id": node_id, "kind": "agent", "name": info.get("name") or existing.get("name") or node_id,
            "logical_slot": 1 if (backend or {}).get("backend_kind") == "metal"
            else (existing.get("logical_slot") or _next_logical_slot()),
            "agent_url": agent_url, "report_url": existing.get("report_url"),
            "gpu_uuid": info.get("gpu_uuid", ""), "gpu_name": info.get("gpu", "Managed node"),
            "vram": resources.get("vram_budget_gib", info.get("vram_budget_gib", 0.0)),
            "ram": resources.get("ram_budget_gib", info.get("ram_budget_gib", 0.0)),
            "cores": resources.get("cores_budget", info.get("cores", 0)),
            "rpc_host": parsed.hostname or "127.0.0.1", "rpc_port": int(info.get("rpc_port", 50052)),
            "bound_to": existing.get("bound_to") or info.get("bound_to"),
            "worker": None, "worker_running": bool(info.get("worker_running")),
            "ram_used": resources.get("ram_used_gib", 0.0), "log": "", "resources": resources,
            "operations": info.get("operations", []), "models": info.get("models", {}),
            "last_report": None, "desired_load": info.get("desired_load"),
            "capabilities": info.get("capabilities", {}), "host_platform": info.get("host_platform", {}),
            "owner": (info.get("owner") or existing.get("owner") or "").strip(),
            "runtime": runtime, "backend": backend,
        }
        NODES[node_id] = n
        owner = n.get("bound_to")
        if owner in CTRLS and node_id not in CTRLS[owner].get("nodes", []):
            CTRLS[owner]["nodes"].append(node_id)
    _persist_hub_state()
































# ------------------------------- models -----------------------------------
_SPLIT_GGUF_RE = re.compile(r"^(?P<base>.+)-(?P<idx>\d{5})-of-(?P<count>\d{5})\.gguf$", re.IGNORECASE)


def _safe_rel(path):
    rel = os.path.relpath(path, MODEL_DIR)
    if rel.startswith("..") or os.path.isabs(rel):
        raise HTTPException(400, "model path escapes model directory")
    return rel.replace("\\", "/")


def _is_aux_model_file(name):
    return "mmproj" in name.lower()


def _is_internal_model_rel(rel):
    return rel == "linkcpp" or rel.startswith("linkcpp/")


def _is_excluded_model_rel(rel):
    return _is_internal_model_rel(rel) or is_embedding_model(rel)


def _model_label(rel, size_gib, shard_count=1):
    return model_label(rel, size_gib, shard_count)


def _scan_models():
    entries = []
    if not os.path.isdir(MODEL_DIR):
        return entries

    files = sorted(glob.glob(os.path.join(MODEL_DIR, "**", "*.gguf"), recursive=True))
    aux_by_dir = collections.defaultdict(list)
    split_groups = {}
    standalone = []

    for f in files:
        name = os.path.basename(f)
        rel = _safe_rel(f)
        if _is_excluded_model_rel(rel):
            continue
        if _is_aux_model_file(name):
            aux_by_dir[os.path.dirname(f)].append(rel)
            continue
        m = _SPLIT_GGUF_RE.match(name)
        if m:
            key = (os.path.dirname(f), m.group("base"), m.group("count"))
            split_groups.setdefault(key, []).append((int(m.group("idx")), f))
        else:
            standalone.append(f)

    for f in standalone:
        rel = _safe_rel(f)
        size = os.path.getsize(f)
        entry = {
            "id": rel,
            "name": os.path.basename(f),
            "label": _model_label(rel, round(size / 1024**3, 2)),
            "relative_path": rel,
            "primary_file": rel,
            "size_gib": round(size / 1024**3, 2),
            "shards": [rel],
            "aux_files": aux_by_dir.get(os.path.dirname(f), []),
        }
        entries.append(entry)

    for (dir_, base, count), shards in split_groups.items():
        shards = sorted(shards)
        primary = next((f for idx, f in shards if idx == 1), shards[0][1])
        rels = [_safe_rel(f) for _, f in shards]
        size = sum(os.path.getsize(f) for _, f in shards)
        primary_rel = _safe_rel(primary)
        entry = {
            "id": primary_rel,
            "name": os.path.basename(primary),
            "label": _model_label(primary_rel, round(size / 1024**3, 2), len(shards)),
            "relative_path": primary_rel,
            "primary_file": primary_rel,
            "size_gib": round(size / 1024**3, 2),
            "shards": rels,
            "aux_files": aux_by_dir.get(dir_, []),
        }
        entries.append(entry)

    return sorted(entries, key=lambda x: x["relative_path"].lower())


def _resolve_model(ref):
    if not ref:
        raise HTTPException(400, "model is required")
    models = _scan_models()
    for m in models:
        candidates = {m["id"], m["relative_path"], m["primary_file"], m["name"]}
        if ref in candidates:
            path = os.path.abspath(os.path.join(MODEL_DIR, m["primary_file"]))
            root = os.path.abspath(MODEL_DIR)
            if os.path.commonpath([root, path]) != root:
                raise HTTPException(400, "model path escapes model directory")
            return path, m
    legacy = os.path.abspath(os.path.join(MODEL_DIR, ref))
    root = os.path.abspath(MODEL_DIR)
    if os.path.commonpath([root, legacy]) == root and os.path.exists(legacy):
        rel = _safe_rel(legacy)
        return legacy, {
            "id": rel, "name": os.path.basename(legacy), "label": rel,
            "relative_path": rel, "primary_file": rel,
            "size_gib": round(os.path.getsize(legacy) / 1024**3, 2),
            "shards": [rel], "aux_files": [],
        }
    raise HTTPException(404, f"unknown model {ref}")




SHARD_TARGET_REPLICAS = int(os.environ.get("LINKCPP_SHARD_TARGET_REPLICAS", "2") or 2)

# Stage configs published for self-enrolled NAT nodes to pull and self-start,
# since the hub cannot push a stage-start to a node it can only reach outbound.
# node_id -> {controller_id, config, coordinator_endpoint}
SELF_START_CONFIGS: dict = {}
_RELAY_SEQ = 0












# ---- expert coverage market (M3): the layer market, at (layer, expert-range) grain -------
# The same scarcity/reward mechanism as _shard_coverage, but the coverage unit is a
# routed-expert range within a layer, so a weak node can volunteer for a handful of
# experts. Expert workers heartbeat their coverage; nodes poll demand and take the
# scarcest range they can afford.
EXPERT_WORKERS: dict = {}   # worker_id -> {model, n_layer, n_expert, segments, url, ts}
EXPERT_TARGET_REPLICAS = int(os.environ.get("LINKCPP_EXPERT_TARGET_REPLICAS", "2"))

# ---- load-adaptive recruitment (Phase 2): a saturated coordinator raises demand ----
# A background loop polls each external MoE coordinator's /slots. When a model's
# coordinator is saturated (busy inference slots >= threshold), the hub boosts that
# model's effective expert-replica target so the coverage market surfaces demand and
# idle nodes polling /api/expert-volunteer are pulled in to add aggregate throughput.
# When load subsides the target falls back and the extra workers naturally age out.
# Pull-based by design: it fits the same NAT-friendly market phones already poll,
# and needs no push channel to reach a worker behind a home router.
COORD_LOAD: dict = {}   # model basename -> {busy, slots, saturated, ts, master_port, name}
RECRUIT_SATURATE_SLOTS = int(os.environ.get("LINKCPP_RECRUIT_SATURATE_SLOTS", "2"))
EXPERT_RECRUIT_BOOST = int(os.environ.get("LINKCPP_EXPERT_RECRUIT_BOOST", "1"))
_RECRUIT_TTL = float(os.environ.get("LINKCPP_RECRUIT_TTL_S", "45"))   # a stale reading stops recruiting


def _recruiting(model: str) -> bool:
    """True while this model's coordinator was recently observed saturated."""
    ld = COORD_LOAD.get(os.path.basename(model or ""))
    return bool(ld and ld.get("saturated") and (time.time() - float(ld.get("ts", 0)) < _RECRUIT_TTL))


def _effective_target(model: str) -> int:
    """Expert-replica target driving the coverage market: the base target, raised
    by the recruit boost while the model's coordinator is saturated."""
    return EXPERT_TARGET_REPLICAS + EXPERT_RECRUIT_BOOST if _recruiting(model) else EXPERT_TARGET_REPLICAS


class ExpertCoverage(BaseModel):
    worker_id: str
    model: str
    n_layer: int = 0
    n_expert: int = 0
    segments: list = []          # [[layer, expert_begin, expert_end], ...]
    url: str = ""                # "relay:<session>" (dials in) | "tcp:<port>" | ""
    owner: str = ""              # worker owner wallet, for contribution accounting


# ---- auto-wiring: coverage -> relay session + coordinator dispatch slot ------
# A relay-dialable worker (a phone dials in over 443) is auto-wired the moment it
# registers coverage: the hub allocates it a stable coordinator listen port,
# registers the relay session that bridges its dial to that port, and publishes
# the (range -> session/port) row on /api/expert-dispatch-map. A dispatch
# coordinator polls that map and attaches the worker with no operator action.
EXPERT_DISPATCH = {}             # worker_id -> {model,layer,experts,session,listen_port,owner,node_id}
_EXPERT_PORT_BASE = int(os.environ.get("LINKCPP_EXPERT_PORT_BASE", "52970"))
_EXPERT_PORT_SPAN = 60


def _alloc_dispatch_port(worker_id: str) -> int:
    used = {d["listen_port"] for d in EXPERT_DISPATCH.values()}
    ex = EXPERT_DISPATCH.get(worker_id)
    if ex:
        return ex["listen_port"]
    for i in range(_EXPERT_PORT_SPAN):
        p = _EXPERT_PORT_BASE + i
        if p not in used:
            return p
    return _EXPERT_PORT_BASE  # pool exhausted (shouldn't happen at this scale)


def _autowire_expert_worker(c: ExpertCoverage):
    """If the worker dials in via relay, wire it end-to-end: allocate a
    coordinator listen port, register the bridging relay session, and record the
    dispatch slot so the map endpoint surfaces it to the coordinator."""
    url = (c.url or "").strip()
    if not url.startswith("relay:"):
        return
    # Convention (matches the mobile app): the app dials session "expert-<nodeId>"
    # and the worker_id IS the node id, so rewards land on that node's row. Honor
    # an explicit session in the url only if given; otherwise derive it.
    node_id = c.worker_id
    session = url[len("relay:"):] or ("expert-" + c.worker_id)
    # the worker covers one contiguous (layer, expert-range) in the common case
    seg = (c.segments or [[0, 0, 0]])[0]
    layer, a, b = (list(seg) + [0, 0, 0])[:3]
    port = _alloc_dispatch_port(c.worker_id)
    # A coverage heartbeat re-runs this on every POST. If the app omits its owner
    # here but proved a wallet when it dialed the relay (api_expert_relay adopts
    # the dialing wallet), preserve that owner instead of clobbering it to "" —
    # otherwise a heartbeat landing after the dial would drop reward attribution.
    prev_owner = (EXPERT_RELAY_TARGETS.get(session) or {}).get("owner", "")
    owner = c.owner or prev_owner
    EXPERT_DISPATCH[c.worker_id] = {
        "model": os.path.basename(c.model), "layer": int(layer),
        "experts": [int(a), int(b)], "session": session, "listen_port": port,
        "owner": owner, "node_id": node_id, "ts": time.time(),
    }
    # bridge the worker's dial (over 443) to the coordinator's listen port
    EXPERT_RELAY_TARGETS[session] = {
        "host": "127.0.0.1", "port": port, "model": os.path.basename(c.model),
        "layer": int(layer), "experts": [int(a), int(b)], "owner": owner,
        "node_id": node_id, "ts": time.time(),
    }


@app.post("/api/expert-coverage")
def api_expert_coverage(c: ExpertCoverage):
    """An expert worker registers/heartbeats which (layer, expert-range) slabs it
    holds. A relay-dialable worker is auto-wired to the dispatch coordinator here."""
    EXPERT_WORKERS[c.worker_id] = {
        "model": os.path.basename(c.model), "n_layer": int(c.n_layer),
        "n_expert": int(c.n_expert), "segments": [[int(x) for x in s] for s in c.segments],
        "url": c.url, "owner": c.owner, "ts": time.time(),
    }
    _autowire_expert_worker(c)
    d = EXPERT_DISPATCH.get(c.worker_id)
    return {"ok": True, "workers": len(EXPERT_WORKERS),
            "wired": bool(d), "listen_port": d["listen_port"] if d else None,
            "session": d["session"] if d else None}


@app.get("/api/expert-dispatch-map")
def api_expert_dispatch_map(model: str = Query(""), layer: int = Query(-1), stale: float = 120.0):
    """Live (expert-range -> coordinator listen port) rows a dispatch coordinator
    polls to attach/detach workers with no operator action. One row per live
    relay-wired worker; the coordinator opens a listener per row and dispatches
    that range to it. Filtered by model (and layer when >=0)."""
    now = time.time()
    mb = os.path.basename(model) if model else ""
    rows = []
    for wid, d in list(EXPERT_DISPATCH.items()):
        if now - float(d.get("ts", 0)) > stale:
            continue
        if mb and d["model"] != mb:
            continue
        if layer >= 0 and int(d["layer"]) != layer:
            continue
        rows.append({"worker_id": wid, "layer": d["layer"], "experts": d["experts"],
                     "listen_port": d["listen_port"], "session": d["session"],
                     "node_id": d["node_id"]})
    return {"model": mb, "rows": rows}


def _expert_coverage(model: str = "", stale: float = 120.0):
    """Per model: replica count per expert per layer, folded into contiguous
    (expert-range) segments with a scarcity score (0 = at target, 1 = uncovered).
    Under recruitment (a saturated coordinator) the effective target rises, so
    already-covered experts read as scarce again and a model with no live workers
    is seeded from its GGUF dims so the first recruits have somewhere to land."""
    now = time.time()
    per = {}
    for w in EXPERT_WORKERS.values():
        if now - float(w.get("ts", 0)) > stale or not w.get("n_expert"):
            continue
        m = w["model"]
        if model and os.path.basename(model) != m:
            continue
        e = per.setdefault(m, {"n_layer": w["n_layer"], "n_expert": w["n_expert"], "cov": {}})
        for seg in w["segments"]:
            if len(seg) != 3:
                continue
            layer, a, b = seg
            arr = e["cov"].setdefault(int(layer), [0] * e["n_expert"])
            for i in range(max(0, int(a)), min(e["n_expert"], int(b))):
                arr[i] += 1
    # Recruitment seeding: a saturated model with no folded coverage still needs
    # demand surfaced so the first volunteers know where to land. Seed the dispatched
    # layer (0 by convention) fully uncovered from the model's GGUF dims. Read only
    # the warm cache here — the load poll pre-warms dims off the request path, since
    # reading a large GGUF's metadata is seconds-slow and must never block a poll.
    for m in list(COORD_LOAD.keys()):
        if (model and os.path.basename(model) != m) or m in per or not _recruiting(m):
            continue
        dims = _MODEL_DIMS_CACHE.get(m, {})
        n_layer, n_expert = int(dims.get("n_layer") or 0), int(dims.get("n_expert") or 0)
        if n_layer and n_expert:
            per.setdefault(m, {"n_layer": n_layer, "n_expert": n_expert,
                               "cov": {0: [0] * n_expert}})
    out = []
    for m, e in per.items():
        tgt = _effective_target(m)
        layers = []
        for layer, arr in sorted(e["cov"].items()):
            segs, i = [], 0
            while i < len(arr):
                j = i
                while j < len(arr) and arr[j] == arr[i]:
                    j += 1
                segs.append({"experts": [i, j], "replicas": arr[i], "target": tgt,
                             "scarcity": round(max(0, tgt - arr[i]) / tgt, 3)})
                i = j
            layers.append({"layer": layer, "segments": segs})
        out.append({"model": m, "n_layer": e["n_layer"], "n_expert": e["n_expert"],
                    "target_replicas": tgt, "recruiting": _recruiting(m), "layers": layers})
    return out


@app.get("/api/expert-demand")
def api_expert_demand(model: str = Query("")):
    """Live expert coverage/demand map: which (layer, expert-range) segments are
    scarce. ``recruiting`` is set while a coordinator's load has boosted demand."""
    cov = _expert_coverage(model)
    return {"target_replicas": EXPERT_TARGET_REPLICAS,
            "recruiting": any(c.get("recruiting") for c in cov),
            "models": cov}


@app.get("/api/moe/recruitment")
def api_moe_recruitment():
    """Load-adaptive recruitment state (Phase 2): per-model coordinator saturation
    observed by the /slots poll and the resulting effective expert-replica target
    that drives the demand market. Read-only observability for operators."""
    now = time.time()
    out = []
    for m, ld in sorted(COORD_LOAD.items()):
        workers = sum(1 for w in EXPERT_WORKERS.values()
                      if w.get("model") == m and now - float(w.get("ts", 0)) < 120)
        out.append({
            "model": m, "busy": ld.get("busy"), "slots": ld.get("slots"),
            "saturated": bool(ld.get("saturated")), "recruiting": _recruiting(m),
            "base_target": EXPERT_TARGET_REPLICAS, "effective_target": _effective_target(m),
            "expert_workers": workers, "age_s": round(now - float(ld.get("ts", 0)), 1),
        })
    return {"saturate_at": RECRUIT_SATURATE_SLOTS, "boost": EXPERT_RECRUIT_BOOST,
            "target_base": EXPERT_TARGET_REPLICAS, "models": out}


def _recommend_expert_segment(model: str = "", max_experts: int = 0):
    """The scarcest (layer, expert-range) a volunteering node should take, clipped to budget."""
    best = None
    for entry in _expert_coverage(model):
        for lyr in entry["layers"]:
            for seg in lyr["segments"]:
                if seg["scarcity"] <= 0:
                    continue
                a, b = seg["experts"]
                if max_experts and (b - a) > max_experts:
                    b = a + max_experts
                cand = {"model": entry["model"], "layer": lyr["layer"], "experts": [a, b],
                        "scarcity": seg["scarcity"], "replicas": seg["replicas"], "target": seg["target"]}
                key = (seg["scarcity"], b - a)
                if best is None or key > best[0]:
                    best = (key, cand)
    return best[1] if best else None


class ExpertVolunteer(BaseModel):
    model: str = ""
    max_experts: int = 0


_MODEL_DIMS_CACHE: dict = {}   # basename -> {"n_embd","n_layer","n_expert"} (read once)


def _model_dims(model: str) -> dict:
    """n_embd/n_layer/n_expert for a served model, read from its GGUF metadata
    once and cached. A volunteering worker (esp. a phone that can't read the
    full model) needs n_embd to size dispatch buffers before it has a slice."""
    key = os.path.basename(model or "")
    if key in _MODEL_DIMS_CACHE:
        return _MODEL_DIMS_CACHE[key]
    dims = {}
    try:
        path, _meta = _resolve_model(model)
        if path:
            m = read_model(path)
            dims = {"n_embd": int(m.get("n_embd") or 0),
                    "n_layer": int(m.get("n_layer") or 0),
                    "n_expert": int(m.get("n_expert") or 0)}
    except Exception:
        dims = {}
    if dims.get("n_embd"):
        _MODEL_DIMS_CACHE[key] = dims
    return dims


@app.post("/api/expert-volunteer")
def api_expert_volunteer(req: ExpertVolunteer):
    """A node offers to serve some experts; the hub replies with the scarcest
    (layer, expert-range) it should take, or none if coverage is at target.
    The reply carries the model's n_embd/n_layer/n_expert so a phone worker can
    size its dispatch buffers without reading the full model (the slice GGUF
    also carries linkcpp.expert_shard.n_embd for the worker itself)."""
    rec = _recommend_expert_segment(req.model, req.max_experts)
    if rec:
        rec = {**rec, **_model_dims(rec.get("model") or req.model)}
    return {"assigned": bool(rec), **(rec or {"reason": "expert coverage at target"})}


# ---- M2: expert-dispatch relay ----------------------------------------------
# The backbone dispatches sharded-MoE expert work over one long-lived raw TCP
# stream per worker (linkcpp-moe-verify/linkcpp-server --dispatch-listen). A
# worker that can't be dialed directly (NAT phone, remote site) dials OUT to
# this hub over WS and the hub bridges the byte stream to the backbone's
# registered listener — the ring-relay pattern at expert-dispatch granularity.
EXPERT_RELAY_TARGETS = {}   # session -> {"host","port","model","layer","owner","node_id","ts"}
EXPERT_RELAY_STATS = {}     # session -> {"opened","closed","ws2tcp","tcp2ws","credited_bytes"}

# Bridged expert work settles through the same pull pipeline as ring inference:
# it accrues into CONTRIBUTIONS (see the settlement section) and the gateway
# polls /api/contributions and credits deltas. The operator tunes the bytes ->
# units rate to their reward policy (ring convention: 1 unit == 1k tokens of a
# node's layer share; one dispatched token moves n_embd*n_used*4 output bytes).
EXPERT_UNITS_PER_MB = float(os.environ.get("LINKCPP_EXPERT_UNITS_PER_MB", "1.0") or 1.0)


class ExpertRelayTarget(BaseModel):
    session: str
    host: str
    port: int
    model: str = ""
    layer: int = -1
    experts: list = []       # [a, b) range the worker covers (scarcity weighting)
    owner: str = ""          # worker owner wallet, for contribution accounting
    node_id: str = ""        # settlement node id (defaults to expert-{session})


@app.post("/api/expert-relay/register")
def api_expert_relay_register(t: ExpertRelayTarget):
    """The backbone side registers its dispatch TCP listener for one relay
    session; the worker side then dials /api/expert-relay?session=... and the
    hub bridges the two. Not node-token-reachable when auth is enabled: the
    target is a host:port this hub will dial, so only the service token or an
    admin session may set it."""
    EXPERT_RELAY_TARGETS[t.session] = {
        "host": t.host, "port": int(t.port), "model": os.path.basename(t.model),
        "layer": int(t.layer), "experts": [int(x) for x in t.experts[:2]],
        "owner": t.owner,
        "node_id": t.node_id or ("expert-" + t.session), "ts": time.time(),
    }
    return {"ok": True, "session": t.session}


@app.get("/api/expert-relay/sessions")
def api_expert_relay_sessions():
    """Relay sessions + byte counters — the ledger source for dispatched expert
    work (bytes bridged per session/owner feed contribution reporting)."""
    out = []
    for s, t in EXPERT_RELAY_TARGETS.items():
        out.append({"session": s, **t, **(EXPERT_RELAY_STATS.get(s) or {})})
    return {"sessions": out}


def _expert_segment_scarcity_multiplier(model, layer, experts):
    """Reward covering a thin expert range: 1.0 at/above target, rising toward
    SHARD_SCARCITY_MAX as live coverage of the session's range gets scarcer —
    the layer-shard scarcity policy (same env knobs) at expert grain."""
    if not SHARD_SCARCITY_REWARD or layer is None or int(layer) < 0:
        return 1.0
    a, b = (experts + [0, 0])[:2] if experts else (0, 0)
    for entry in _expert_coverage(model):
        for lyr in entry["layers"]:
            if lyr["layer"] != int(layer):
                continue
            tot = n = 0.0
            for seg in lyr["segments"]:
                s0, s1 = seg["experts"]
                lo, hi = (max(a, s0), min(b, s1)) if b > a else (s0, s1)
                if hi > lo:
                    tot += seg["scarcity"] * (hi - lo)
                    n += hi - lo
            if n:
                return round(min(SHARD_SCARCITY_MAX, 1.0 + SHARD_SCARCITY_ALPHA * tot / n), 4)
    return 1.0


def _expert_flush_contribution(session, tgt, st):
    """Fold this relay session's un-credited bridged work into CONTRIBUTIONS —
    the same cumulative ledger the settlement gateway polls and delta-credits
    for ring inference. Work = the larger direction (expert outputs dominate
    whichever side the worker sits on)."""
    work = max(int(st.get("ws2tcp", 0)), int(st.get("tcp2ws", 0)))
    delta = work - int(st.get("credited_bytes", 0))
    if delta <= 0 or not tgt.get("owner"):
        return
    st["credited_bytes"] = work
    mult = _expert_segment_scarcity_multiplier(tgt.get("model", ""), tgt.get("layer"), tgt.get("experts") or [])
    node_id = tgt.get("node_id") or ("expert-" + session)
    c = CONTRIBUTIONS.setdefault(node_id, {
        "node_id": node_id, "owner": tgt["owner"], "units": 0.0,
        "model": tgt.get("model", ""), "node_name": "expert:" + session,
        "backend": "", "os": "", "accelerator": "gpu",
        "device_kind": "expert-worker", "perf_tps": None,
    })
    c["scarcity_multiplier"] = mult
    c["units"] = round(float(c.get("units", 0.0)) + delta / 1e6 * EXPERT_UNITS_PER_MB * mult, 6)


async def _expert_contrib_flush_loop():
    """Long-lived dispatch streams flush their work periodically, not only at
    bridge teardown, so the gateway's poll sees contributions as they happen."""
    while True:
        await asyncio.sleep(60)
        try:
            for session, tgt in list(EXPERT_RELAY_TARGETS.items()):
                st = EXPERT_RELAY_STATS.get(session)
                if st:
                    _expert_flush_contribution(session, tgt, st)
        except Exception as exc:
            LOG.info("expert contribution flush failed: %r", exc)


@app.on_event("startup")
async def _startup_expert_contrib_flush():
    asyncio.create_task(_expert_contrib_flush_loop())


async def _coordinator_load_poll_loop():
    """Poll each external MoE coordinator's /slots to detect saturation (Phase 2).
    A saturated coordinator boosts its model's effective replica target so the
    coverage market recruits idle nodes under load. A controller without /slots
    (not a dispatch server) or one that is unreachable simply never registers as
    saturated, so the poll is safe to run against every external controller."""
    interval = int(os.environ.get("LINKCPP_RECRUIT_POLL_S", "10"))
    while True:
        await asyncio.sleep(interval)
        try:
            targets = [(c["model"], int(c["master_port"]), c.get("name", ""))
                       for c in list(CTRLS.values())
                       if c.get("external") and c.get("master_port")]
        except Exception:
            targets = []
        for model, port, name in targets:
            busy, slots = -1, 0
            try:
                async with httpx.AsyncClient(timeout=4) as client:
                    r = await client.get(f"http://127.0.0.1:{port}/slots")
                arr = r.json() if r.status_code == 200 else None
                if isinstance(arr, list):
                    slots = len(arr)
                    busy = sum(1 for s in arr if isinstance(s, dict) and s.get("is_processing"))
            except Exception:
                busy = -1
            if busy < 0:
                continue   # no /slots (not a dispatch coordinator) or unreachable
            mb = os.path.basename(model)
            COORD_LOAD[mb] = {
                "busy": busy, "slots": slots,
                "saturated": busy >= RECRUIT_SATURATE_SLOTS,
                "ts": time.time(), "master_port": port, "name": name,
            }
            # Pre-warm the model's expert dims so recruitment seeding stays a cache
            # read. Reading a large GGUF's metadata is seconds-slow, so do it once,
            # off the event loop, the first time we see this coordinator.
            if mb not in _MODEL_DIMS_CACHE:
                try:
                    await asyncio.to_thread(_model_dims, model)
                except Exception:
                    pass


@app.on_event("startup")
async def _startup_coordinator_load_poll():
    asyncio.create_task(_coordinator_load_poll_loop())


@app.websocket("/api/expert-relay")
async def api_expert_relay(ws: WebSocket, session: str = Query(""), token: str = Query("")):
    """Bridge one expert-dispatch TCP stream (backbone <-> worker) over WS."""
    wallet = (siws.verify_token(token, "node") if token else None) or None
    ok = bool(wallet)
    if AUTH_ENABLED and not ok and not (SERVICE_TOKEN and token and _hmac.compare_digest(token, SERVICE_TOKEN)):
        await ws.close(code=4401)
        return
    tgt = EXPERT_RELAY_TARGETS.get(session)
    if not tgt:
        await ws.close(code=4404)
        return
    # Reward attribution: the wallet that authenticated this dial owns the
    # session's bridged work. An app need not echo its owner in the coverage
    # POST — the node token it dials with already proves the wallet. Honor an
    # explicit coverage-supplied owner; otherwise adopt the dialing wallet.
    if wallet and not tgt.get("owner"):
        tgt["owner"] = wallet
    await ws.accept()
    global _RELAY_SEQ
    _RELAY_SEQ += 1
    cid = _RELAY_SEQ
    LOG.info("expert-relay[%d] %s open -> %s:%s", cid, session, tgt["host"], tgt["port"])
    try:
        reader, writer = await asyncio.open_connection(tgt["host"], int(tgt["port"]))
        sock = writer.get_extra_info("socket")
        if sock is not None:
            sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
    except Exception as exc:
        LOG.info("expert-relay[%d] backbone connect FAILED: %r", cid, exc)
        await ws.close(code=1011)
        return

    stats = EXPERT_RELAY_STATS.setdefault(session, {"ws2tcp": 0, "tcp2ws": 0})
    stats["opened"] = time.time()
    stats.pop("closed", None)

    async def ws_to_tcp():
        try:
            while True:
                data = await ws.receive_bytes()
                stats["ws2tcp"] += len(data)
                writer.write(data)
                await writer.drain()
        except Exception as exc:
            LOG.info("expert-relay[%d] ws->tcp END after %dB: %r", cid, stats["ws2tcp"], exc)

    async def tcp_to_ws():
        try:
            while True:
                data = await reader.read(65536)
                if not data:
                    break
                stats["tcp2ws"] += len(data)
                await ws.send_bytes(data)
        except Exception as exc:
            LOG.info("expert-relay[%d] tcp->ws END after %dB: %r", cid, stats["tcp2ws"], exc)

    # As in the ring relay: a half-open dispatch stream is dead, so the bridge
    # tears down as soon as EITHER direction ends.
    try:
        done, pending = await asyncio.wait(
            {asyncio.ensure_future(ws_to_tcp()), asyncio.ensure_future(tcp_to_ws())},
            return_when=asyncio.FIRST_COMPLETED,
        )
        for task in pending:
            task.cancel()
    finally:
        stats["closed"] = time.time()
        _expert_flush_contribution(session, tgt, stats)
        LOG.info("expert-relay[%d] teardown ws->tcp=%dB tcp->ws=%dB", cid, stats["ws2tcp"], stats["tcp2ws"])
        try:
            writer.close()
        except Exception:
            pass
        try:
            await ws.close()
        except Exception:
            pass










@app.websocket("/api/ring-relay")
async def api_ring_relay(ws: WebSocket, controller_id: str = Query(""), token: str = Query("")):
    """Relay one ring TCP stream between a NAT node and the coordinator over 443.

    A node behind NAT (or a hub behind Cloudflare, which proxies only 80/443)
    can't open the coordinator's raw ring port. Instead the node opens a
    WebSocket here — one per ring edge — and the hub bridges it to the
    coordinator's ring listener, co-located on the GPU box. Ring bytes (role
    preamble, hello, boundary frames) pass through untouched, so no ring port
    needs to be publicly reachable."""
    ok = bool(siws.verify_token(token, "node")) if token else False
    if AUTH_ENABLED and not ok and not (SERVICE_TOKEN and token and _hmac.compare_digest(token, SERVICE_TOKEN)):
        await ws.close(code=4401)
        return
    ring = (CTRLS.get(controller_id) or {}).get("coordinator_ring")
    if not ring:
        await ws.close(code=4404)
        return
    await ws.accept()
    global _RELAY_SEQ
    _RELAY_SEQ += 1
    cid = _RELAY_SEQ
    _rlog = LOG
    _rlog.info("relay[%d] open -> %s:%s", cid, ring["host"], ring["port"])
    try:
        reader, writer = await asyncio.open_connection(ring["host"], int(ring["port"]))
        sock = writer.get_extra_info("socket")
        if sock is not None:
            sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
    except Exception as exc:
        _rlog.info("relay[%d] coordinator connect FAILED: %r", cid, exc)
        await ws.close(code=1011)
        return

    stats = {"w2t": 0, "t2w": 0}

    async def ws_to_tcp():
        try:
            while True:
                data = await ws.receive_bytes()
                stats["w2t"] += len(data)
                writer.write(data)
                await writer.drain()
        except Exception as exc:
            _rlog.info("relay[%d] ws->tcp END after %dB: %r", cid, stats["w2t"], exc)
        else:
            _rlog.info("relay[%d] ws->tcp END clean after %dB", cid, stats["w2t"])

    async def tcp_to_ws():
        try:
            while True:
                data = await reader.read(65536)
                if not data:
                    _rlog.info("relay[%d] tcp->ws EOF after %dB", cid, stats["t2w"])
                    break
                stats["t2w"] += len(data)
                await ws.send_bytes(data)
        except Exception as exc:
            _rlog.info("relay[%d] tcp->ws END after %dB: %r", cid, stats["t2w"], exc)

    # Bridge is done as soon as EITHER direction ends: a half-open ring edge is
    # dead, so tear both sides down instead of leaking the still-blocked
    # direction (which would keep the coordinator connection ESTAB forever).
    try:
        done, pending = await asyncio.wait(
            {asyncio.ensure_future(ws_to_tcp()), asyncio.ensure_future(tcp_to_ws())},
            return_when=asyncio.FIRST_COMPLETED,
        )
        for task in pending:
            task.cancel()
    finally:
        _rlog.info("relay[%d] teardown ws->tcp=%dB tcp->ws=%dB", cid, stats["w2t"], stats["t2w"])
        try:
            writer.close()
        except Exception:
            pass
        try:
            await ws.close()
        except Exception:
            pass




















def _serve_req_snapshot(req):
    data = {
        "model": req.model,
        "ctx": int(req.ctx),
        "parallel": int(req.parallel),
        "kv_bits": int(req.kv_bits),
        "cache_type_k": req.cache_type_k,
        "cache_type_v": req.cache_type_v,
        "no_cpu_offload": bool(req.no_cpu_offload),
        "reserve_mib": int(req.reserve_mib),
        "placement_strategy": req.placement_strategy,
    }
    perf = _perf_req_snapshot(req)
    if perf:
        data["performance"] = perf
    return data


_SPEC_TYPES = {
    "none",
    "draft-simple",
    "draft-eagle3",
    "draft-mtp",
    "draft-dflash",
    "ngram-simple",
    "ngram-map-k",
    "ngram-map-k4v",
    "ngram-mod",
    "ngram-cache",
}


def _split_spec_types(value):
    raw = str(value or "none").strip()
    if not raw:
        return ["none"]
    out = [x.strip() for x in raw.split(",") if x.strip()]
    return out or ["none"]


def _validate_spec_types(value):
    types = _split_spec_types(value)
    bad = [x for x in types if x not in _SPEC_TYPES]
    if bad:
        raise HTTPException(400, f"unsupported speculative decoding type: {', '.join(bad)}")
    if "none" in types and len(types) > 1:
        raise HTTPException(400, "speculative decoding type 'none' cannot be combined with other types")
    return ",".join(types)


def _positive_int(value):
    try:
        n = int(value or 0)
    except (TypeError, ValueError):
        return 0
    return max(0, n)


def _non_negative_float(value):
    if value is None or value == "":
        return None
    try:
        n = float(value)
    except (TypeError, ValueError):
        return None
    return max(0.0, n)


def _perf_req_snapshot(req):
    data = {}
    spec_type = _validate_spec_types(getattr(req, "spec_type", "none"))
    if spec_type != "none":
        data["spec_type"] = spec_type
    string_fields = (
        "spec_draft_model",
        "lookup_cache_static",
        "lookup_cache_dynamic",
    )
    for field in string_fields:
        value = str(getattr(req, field, "") or "").strip()
        if value:
            data[field] = value
    int_fields = (
        "batch",
        "ubatch",
        "poll",
        "spec_draft_n_max",
        "spec_draft_n_min",
        "spec_ngram_mod_n_min",
        "spec_ngram_mod_n_max",
        "spec_ngram_mod_n_match",
        "spec_ngram_simple_size_n",
        "spec_ngram_simple_size_m",
        "spec_ngram_simple_min_hits",
        "cache_reuse",
    )
    for field in int_fields:
        value = _positive_int(getattr(req, field, 0))
        if value:
            data[field] = value
    float_fields = ("spec_draft_p_min", "spec_draft_p_split")
    for field in float_fields:
        value = _non_negative_float(getattr(req, field, None))
        if value is not None:
            data[field] = value
    if getattr(req, "cont_batching", True) is False:
        data["cont_batching"] = False
    return data












# ------------------------------- controllers ------------------------------
def _controller_node_items(c):
    """Nodes visible in one controller's Node tab.

    Local/managed nodes are unit-wide resources. Projected remote-unit nodes
    belong only to the controller that registered their unit.
    """
    return [node_view(n) for n in sorted(NODES.values(), key=_node_sort_key)
            if n.get("kind") != "remote_unit_node"
            or n.get("owner_controller_id") == c["id"]]


def ctrl_view(c, full=False):
    runtime_check = _controller_runtime_check(c)
    ph = _ctrl_phase(c)
    can_cancel_load = ph == "loading"
    # A ring controller has no hub-local master (its coordinator runs on the first
    # ring node), so "loaded" is gated on the ring being up (proxy_first_node set),
    # not on a local master process — otherwise running ring models look unloaded
    # and never surface to the pay gateway / model list.
    runtime_loaded = ph in ("running", "error") and (
        c.get("proxy_first_node") is not None or _proc_alive(c.get("master")))
    can_unload = runtime_loaded or ph in ("running", "unloading", "error")
    v = {"id": c["id"], "name": c["name"], "phase": ph, "detail": c["detail"],
         "serving": c["model"] if ph in ("loading", "running") else None,
         "active_model": c["model"],
         "can_unload": can_unload,
         "can_cancel_load": can_cancel_load,
         "runtime_loaded": runtime_loaded,
         "last_load": c.get("last_load") or {},
         "parallel": c["parallel"], "master_port": c["master_port"],
         "nodes": c["nodes"], "runtime": runtime_identity(),
         "backend": backend_identity(),
         "runtime_compatibility": runtime_check,
         "operations": list(_ctrl_ops(c).values())[-50:],
         "inference_activity": _inference_snapshot(c)}
    if full:
        v["node_items"] = [node_view(NODES[nid]) for nid in c["nodes"] if nid in NODES]
        v["available_node_items"] = _controller_node_items(c)
        v["remote_units"] = [_remote_unit_view(r) for r in _controller_remote_units(c["id"])]
        v["plan"] = c["plan"]
    return v


def _ctrl_phase(c):
    # A ring controller has no hub-local master: its coordinator (linkcpp-server)
    # runs on the first ring node, so the local-process liveness check does not apply.
    if (c["phase"] == "running" and not c.get("proxy_first_node")
            and not _proc_alive(c.get("master"))):
        c["phase"] = "error"; c["detail"] = "master exited"
    return c["phase"]








class ExternalCtrl(BaseModel):
    model: str                 # served model name (what surfaces in the model list)
    master_port: int           # 127.0.0.1:<port> where an OpenAI-compatible endpoint already serves /v1
    name: str = ""             # display name (defaults to model)


@app.post("/api/controllers/external")
def api_register_external_ctrl(req: ExternalCtrl):
    """Surface an already-running local OpenAI-compatible endpoint as a controller.

    An expert-swarm model — a backbone coordinator that dispatches routed experts
    to relay/phone workers, or a standalone server reached through a local tunnel —
    serves on ``127.0.0.1:master_port`` but is NOT one of the hub's ring/RPC
    controllers, so it never appears on ``/api/controllers`` and the pay gateway
    never advertises or routes to it. This registers a proxy-only controller (like
    a ring controller, ``proxy_first_node`` marks it loaded) so the model shows up
    in the model list and ``/c/{cid}/v1/...`` forwards inference to that port. The
    hub does not launch or supervise the process; it is runtime-only and not
    persisted (re-register after a hub restart, alongside the coordinator)."""
    cid = "ctrl-ext-" + uuid.uuid4().hex[:6]
    CTRLS[cid] = {"id": cid, "name": req.name or os.path.basename(req.model),
                  "nodes": [], "model": os.path.basename(req.model),
                  "ctx": 4096, "parallel": 1, "phase": "running",
                  "detail": "external coordinator", "plan": None, "master": None,
                  "last_load": {}, "master_port": int(req.master_port),
                  "operations": {}, "proxy_first_node": "external", "external": True}
    return ctrl_view(CTRLS[cid])






























def _hub_reachable_url():
    """A URL a managed node (e.g. a phone on the LAN) can fetch from. Prefers the
    operator-set public URL; otherwise derives the primary non-loopback IPv4."""
    if PUBLIC_HUB_URL:
        return PUBLIC_HUB_URL.rstrip("/")
    ip = "127.0.0.1"
    try:
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        s.connect(("8.8.8.8", 80))     # no packets sent; just selects the egress iface
        ip = s.getsockname()[0]
        s.close()
    except Exception:
        pass
    port = os.environ.get("LINKCPP_UI_PORT", "19000")
    return f"http://{ip}:{port}"


RING_PARTIAL_SHARD = os.environ.get("LINKCPP_RING_PARTIAL_SHARD", "0").strip().lower() \
    in ("1", "true", "yes")


def _hub_model_service_token_qs():
    """Query-string fragment carrying the M2M token for a phone's plain-URL
    download (URLSession can't set headers on a background download)."""
    return "service_token=" + quote(SERVICE_TOKEN, safe="") if SERVICE_TOKEN else ""


async def _ensure_agent_models_for_ring(nodes, model_name, node_source_urls=None):
    """Before a ring load, push the model to any agent node that lacks it and wait
    until it reports the file present. The download URL for each node is provided
    by the caller (the ring runtime), which decides full-file vs per-stage
    partial download; node_source_urls maps node_id -> download URL, and any node
    without an entry falls back to the full-model file route."""
    # Keep the catalog-relative model reference intact.  A native agent may use
    # an LM Studio-style tree (``publisher/model/file.gguf``), and reducing it
    # to a basename makes an already-present file look absent.  Besides causing
    # an unnecessary multi-GB download, that would make the stage request point
    # at a different path than the manifest and the node-local server.
    model_ref = str(model_name).strip().replace("\\", "/")
    name = os.path.basename(model_ref)
    base = _hub_reachable_url()
    token = _hub_model_service_token_qs()
    full_url = f"{base}/api/models/{quote(name, safe='')}/file" + (f"?{token}" if token else "")
    node_source_urls = node_source_urls or {}
    pending = []
    for n in nodes:
        if n.get("kind") != "agent":
            continue
        try:
            status = await _agent_request(n, "GET", "/control/status", timeout=10)
        except Exception:
            status = {}
        if model_ref in (status.get("models") or {}):
            continue
        source = ModelSource(kind="direct_url", url=node_source_urls.get(n.get("id"), full_url))
        # The agent treats an existing destination as an immediate, no-copy
        # registration.  This is how a shared local model library is adopted.
        body = model_to_dict(DownloadRequest(model=model_ref, source=source,
                                             op_id=f"ringstage-{n['id']}-{uuid.uuid4().hex[:6]}"))
        await _agent_request(n, "POST", "/control/download", body, timeout=30)
        pending.append(n)
    deadline = time.time() + 600
    for n in pending:
        while time.time() < deadline:
            try:
                status = await _agent_request(n, "GET", "/control/status", timeout=10)
            except Exception:
                status = {}
            if model_ref in (status.get("models") or {}):
                break
            await asyncio.sleep(3)
        else:
            raise RuntimeError(f"model staging to {n.get('name') or n['id']} timed out")








































# ------------------------------- gateway (per controller) -----------------


















def _inference_tracker(c):
    cid = c["id"]
    limit = max(1, int(c.get("parallel") or 1))
    state = INFERENCE.get(cid)
    if not state or state.get("limit") != limit:
        state = {
            "limit": limit,
            "seq": (state or {}).get("seq", 0),
            "items": (state or {}).get("items", {}),
            "history": (state or {}).get("history", []),
            "node_runtime": (state or {}).get("node_runtime", {}),
        }
        INFERENCE[cid] = state
    return state


def _safe_slug(value):
    slug = re.sub(r"[^A-Za-z0-9_.-]+", "-", str(value or "").strip()).strip("-")
    return slug[:80] or "unknown"






def _parse_rpc_activity_text(log_text):
    counts = {"graph_compute": 0, "set_tensor": 0, "get_tensor": 0, "alloc_buffer": 0, "get_alloc_size": 0}
    bytes_seen = {"set_tensor": 0, "get_tensor": 0, "alloc_buffer": 0}
    timings_us = {"graph_compute": 0, "graph_recompute": 0, "get_tensor": 0}
    latest = {}
    lines = [line.strip() for line in (log_text or "").splitlines() if line.strip()]
    for line in lines:
        timing_match = re.search(r"\[rpc_timing\]\s+op=(graph_compute|graph_recompute|get_tensor).*?elapsed_us=(\d+)", line)
        if timing_match:
            timings_us[timing_match.group(1)] += int(timing_match.group(2))
            continue
        op = None
        for candidate in counts:
            if f"[{candidate}]" in line:
                op = candidate
                break
        if not op:
            continue
        counts[op] += 1
        size_match = re.search(r"\bsize:\s*(\d+)", line)
        size = int(size_match.group(1)) if size_match else None
        if size is not None and op in bytes_seen:
            bytes_seen[op] += size
        node_match = re.search(r"\bn_nodes:\s*(\d+)", line)
        tensor_match = re.search(r"\bn_tensors:\s*(\d+)", line)
        latest = {
            "op": op,
            "line": line[-220:],
            "size_bytes": size,
            "n_nodes": int(node_match.group(1)) if node_match else None,
            "n_tensors": int(tensor_match.group(1)) if tensor_match else None,
        }
    return {"counts": counts, "bytes": bytes_seen, "timings_us": timings_us,
            "latest": latest, "line_count": len(lines)}


def _runtime_phase_from_op(op):
    if op == "graph_compute":
        return "computing"
    if op == "set_tensor":
        return "transferring_in"
    if op == "get_tensor":
        return "transferring_out"
    if op == "alloc_buffer":
        return "allocating"
    if op == "get_alloc_size":
        return "probing_capacity"
    return "active"


def _read_local_node_log_delta(n, prev):
    path = n.get("log") or ""
    now = time.time()
    try:
        st = os.stat(path)
        size = int(st.st_size)
        prev_size = prev.get("log_size")
        if prev_size is not None and size >= prev_size:
            start = int(prev_size)
            if size - start > 262144:
                start = size - 262144
            with open(path, "rb") as f:
                f.seek(start)
                delta = f.read().decode("utf-8", "replace")
        else:
            delta = tail(path, 240)
        recent = tail(path, 160)
        return {
            "ok": True,
            "source": "local_rpc_log",
            "delta_log": delta,
            "recent_log": recent,
            "log_size": size,
            "log_mtime": st.st_mtime,
            "sampled_at": now,
        }
    except Exception as exc:
        return {"ok": False, "source": "local_rpc_log", "error": str(exc),
                "delta_log": "", "recent_log": "", "sampled_at": now}


def _read_remote_node_log_delta(n, prev):
    now = time.time()
    cache = n.get("_runtime_log_cache") or {}
    if cache and now - cache.get("sampled_at", 0) < 0.75:
        return cache
    base_url = (n.get("remote_unit_url") or "").rstrip("/")
    source_id = n.get("remote_source_node_id") or ""
    if not base_url or not source_id:
        return {"ok": False, "source": "remote_rpc_log", "error": "remote source missing",
                "delta_log": "", "recent_log": "", "sampled_at": now}
    try:
        resp = httpx.get(f"{base_url}/api/nodes/{quote(source_id, safe='')}/logs", timeout=2.0,
                         headers=_service_headers())
        resp.raise_for_status()
        data = resp.json()
        log_text = data.get("log", "") if isinstance(data, dict) else ""
        n["worker_running"] = bool(data.get("worker_running", n.get("worker_running"))) if isinstance(data, dict) else n.get("worker_running")
        if isinstance(data, dict):
            n["ram_used"] = data.get("ram_used_gib", n.get("ram_used", 0.0))
            n.setdefault("resources", {})["vram_used_gib"] = data.get("vram_used_gib", 0.0)
            n.setdefault("resources", {})["ram_used_gib"] = data.get("ram_used_gib", n.get("ram_used", 0.0))
            n["remote_status_error"] = data.get("remote_status_error", "")
            n["remote_status_updated_at"] = time.time()
        digest = str(hash(log_text))
        delta_log = log_text if digest != prev.get("remote_log_digest") else ""
        result = {
            "ok": True,
            "source": "remote_rpc_log",
            "delta_log": delta_log,
            "recent_log": log_text,
            "remote_log_digest": digest,
            "sampled_at": now,
        }
        n["_runtime_log_cache"] = result
        return result
    except Exception as exc:
        result = {"ok": False, "source": "remote_rpc_log", "error": str(exc),
                  "delta_log": "", "recent_log": "", "sampled_at": now}
        n["_runtime_log_cache"] = result
        n["remote_status_error"] = str(exc)
        n["remote_status_updated_at"] = time.time()
        return result


def _node_runtime_status(c, item, placement, master_task=None):
    state = _inference_tracker(c)
    runtime_state = state.setdefault("node_runtime", {})
    nid = placement.get("node_id")
    n = NODES.get(nid) or {}
    prev = runtime_state.get(nid, {})
    sample = (_read_remote_node_log_delta(n, prev)
              if n.get("kind") == "remote_unit_node"
              else _read_local_node_log_delta(n, prev))
    delta_activity = _parse_rpc_activity_text(sample.get("delta_log", ""))
    recent_activity = _parse_rpc_activity_text(sample.get("recent_log", ""))
    latest = (delta_activity.get("latest") or recent_activity.get("latest") or {})
    op_count = sum(delta_activity.get("counts", {}).values())
    transfer_delta = sum(delta_activity.get("bytes", {}).get(k, 0) for k in ("set_tensor", "get_tensor"))
    now = time.time()
    active_now = op_count > 0
    last_active_at = now if active_now else prev.get("last_active_at")
    if active_now:
        runtime_phase = _runtime_phase_from_op(latest.get("op"))
    elif last_active_at and now - last_active_at <= 2.5:
        runtime_phase = "recently_active"
    elif _node_worker_running(n):
        runtime_phase = "idle"
    else:
        runtime_phase = "offline"
    active_items = [x for x in state.get("items", {}).values() if x.get("status") in ("running", "streaming")]
    association = "single-active-request" if len(active_items) == 1 else ("ambiguous" if active_items else "none")
    status = {
        "source": sample.get("source"),
        "ok": bool(sample.get("ok")),
        "error": sample.get("error"),
        "state": runtime_phase,
        "worker_running": _node_worker_running(n),
        "active_now": active_now,
        "last_active_age_s": round(now - last_active_at, 3) if last_active_at else None,
        "last_operation": latest,
        "delta": {
            "operation_count": op_count,
            "graph_compute_count": delta_activity.get("counts", {}).get("graph_compute", 0),
            "set_tensor_count": delta_activity.get("counts", {}).get("set_tensor", 0),
            "get_tensor_count": delta_activity.get("counts", {}).get("get_tensor", 0),
            "transfer_bytes": transfer_delta,
            "set_tensor_bytes": delta_activity.get("bytes", {}).get("set_tensor", 0),
            "get_tensor_bytes": delta_activity.get("bytes", {}).get("get_tensor", 0),
            "graph_compute_us": delta_activity.get("timings_us", {}).get("graph_compute", 0),
            "graph_recompute_us": delta_activity.get("timings_us", {}).get("graph_recompute", 0),
            "get_tensor_us": delta_activity.get("timings_us", {}).get("get_tensor", 0),
        },
        "recent_counts": recent_activity.get("counts", {}),
        "queue": {
            "request_id": item.get("id"),
            "request_seq": item.get("seq"),
            "request_status": item.get("status"),
            "master_task": master_task,
            "association": association,
            "parallel_limit": state.get("limit"),
            "active_requests": len(active_items),
            "queued_requests": sum(1 for x in state.get("items", {}).values() if x.get("status") == "queued"),
        },
        "desired_load": n.get("desired_load") or {
            "model": c.get("model"),
            "layers": placement.get("layers"),
            "parallel": c.get("parallel"),
            "ctx": c.get("ctx"),
        },
    }
    prev_next = {
        "last_active_at": last_active_at,
        "last_status": status,
        "remote_log_digest": sample.get("remote_log_digest", prev.get("remote_log_digest")),
        "log_size": sample.get("log_size", prev.get("log_size")),
        "log_mtime": sample.get("log_mtime", prev.get("log_mtime")),
        "sampled_at": now,
    }
    runtime_state[nid] = prev_next
    return status




def _edge_cross_host(source_endpoint, target_endpoint):
    def host(endpoint):
        return (str(endpoint or "").split(":", 1)[0] or "").lower()
    a, b = host(source_endpoint), host(target_endpoint)
    local = {"127.0.0.1", "localhost", "::1", ""}
    return a != b and (a not in local or b not in local)


def _plan_placements(c):
    plan = c.get("plan") or {}
    return [p for p in plan.get("placement", []) if p.get("n_layers")]


def _placement_node(c, placement):
    nid = placement.get("node_id")
    return NODES.get(nid) or {}


def _activation_bytes_per_token(c):
    model = (c.get("plan") or {}).get("model") or {}
    try:
        n_embd = int(model.get("n_embd") or 0)
    except Exception:
        n_embd = 0
    if n_embd <= 0:
        return 0
    return n_embd * 2


def _transfer_edges(c, output_tokens=0):
    placements = _plan_placements(c)
    bytes_per_token = _activation_bytes_per_token(c)
    edges = []
    for idx, p in enumerate(placements):
        source = _placement_node(c, p)
        if idx + 1 < len(placements):
            target_p = placements[idx + 1]
            target = _placement_node(c, target_p)
            edge_type = "node_to_node"
            target_name = target_p.get("node_name") or target.get("name") or f"Node {target_p.get('node')}"
            target_id = target_p.get("node_id")
            target_endpoint = _node_rpc_endpoint(target) if target else ""
        else:
            edge_type = "last_node_to_master"
            target_name = "llama-server master"
            target_id = "master"
            target_endpoint = f"127.0.0.1:{c.get('master_port')}"
        source_endpoint = _node_rpc_endpoint(source) if source else ""
        edge_bytes = int(bytes_per_token * max(0, int(output_tokens or 0)))
        edges.append({
            "edge_type": edge_type,
            "source_node_id": p.get("node_id"),
            "source_node_name": p.get("node_name") or source.get("name") or f"Node {p.get('node')}",
            "source_node_kind": source.get("kind", "local"),
            "source_endpoint": source_endpoint,
            "source_remote_unit_url": source.get("remote_unit_url"),
            "target_node_id": target_id,
            "target_node_name": target_name,
            "target_endpoint": target_endpoint,
            "cross_host": _edge_cross_host(source_endpoint, target_endpoint),
            "bytes_per_token": bytes_per_token,
            "cumulative_bytes": edge_bytes,
            "cumulative_mib": round(edge_bytes / (1024 * 1024), 3),
            "estimated": True,
            "estimate_reason": "activation boundary estimate from n_embd * f16 bytes; replace with RPC byte counters when available",
        })
    return edges


def _node_inference_metrics(c, item, include_runtime=False, master_task=None):
    rows = []
    status = item.get("status")
    plan = c.get("plan") or {}
    for idx, p in enumerate(_plan_placements(c)):
        n = _placement_node(c, p)
        kv_vram = float(p.get("kv_vram_gib") or 0.0)
        kv_ram = float(p.get("kv_ram_gib") or 0.0)
        if kv_vram > 0:
            kv_location = "vram"
            kv_gib = kv_vram
        elif kv_ram > 0:
            kv_location = "ram"
            kv_gib = kv_ram
        else:
            kv_location = "none"
            kv_gib = 0.0
        if status == "queued":
            phase = "waiting"
        elif status in ("running", "streaming"):
            phase = "active"
        else:
            phase = status or "unknown"
        row = {
            "position": idx,
            "node_id": p.get("node_id"),
            "node_name": p.get("node_name") or n.get("name") or f"Node {p.get('node')}",
            "node_kind": n.get("kind", "local"),
            "gpu_name": p.get("gpu_name") or n.get("gpu_name", ""),
            "layers": p.get("layers"),
            "n_layers": p.get("n_layers", 0),
            "phase": phase,
            "phase_source": "pipeline-estimate",
            "kv_cache": {
                "used": kv_gib > 0,
                "location": kv_location,
                "used_gib": round(kv_gib, 3),
                "cache_type_k": plan.get("cache_type_k"),
                "cache_type_v": plan.get("cache_type_v"),
            },
            "vram_used_gib": p.get("vram_used_gib"),
            "ram_used_gib": p.get("ram_used_gib"),
            "offload_policy": p.get("offload_policy"),
            "experts_on_cpu": bool(p.get("experts_on_cpu")),
            "body_on_cpu": bool(p.get("body_on_cpu")),
            "rpc_endpoint": _node_rpc_endpoint(n) if n else "",
            "remote_unit_url": n.get("remote_unit_url"),
            "remote_source_rpc_endpoint": n.get("remote_source_rpc_endpoint"),
        }
        if include_runtime:
            row["runtime"] = _node_runtime_status(c, item, p, master_task=master_task)
        elif item.get("node_runtime"):
            row["runtime"] = (item.get("node_runtime") or {}).get(row["node_id"])
        rows.append(row)
    return rows


def _pipeline_for_request(c, item):
    rows = []
    placements = _plan_placements(c)
    status = item.get("status")
    for idx, p in enumerate(placements):
        if status == "queued":
            phase = "waiting"
            detail = "waiting for controller parallel slot"
        elif status in ("running", "streaming"):
            phase = "active"
            if idx == 0:
                detail = "receives prompt and starts pipeline"
            else:
                prev = placements[idx - 1].get("node_name") or f"node {idx - 1}"
                detail = f"receives hidden state from {prev}"
        else:
            phase = status or "unknown"
            detail = item.get("message", "")
        rows.append({
            "node_id": p.get("node_id"),
            "node_name": p.get("node_name") or f"Node {p.get('node')}",
            "gpu_name": p.get("gpu_name", ""),
            "layers": p.get("layers"),
            "phase": phase,
            "detail": detail,
            "handoff_to": placements[idx + 1].get("node_name") if idx + 1 < len(placements) else "master response",
        })
    return rows


def _usage_output_tokens(usage):
    usage = usage or {}
    for key in ("completion_tokens", "output_tokens", "predicted_n"):
        try:
            if usage.get(key) is not None:
                return int(usage.get(key))
        except Exception:
            pass
    return None


def _latest_decode_progress(progress):
    for row in reversed(progress or []):
        if row.get("kind") == "decode_progress":
            return row
    return None


def _sample_inference(c, item, usage=None, timings=None, finish_reason=None, master_progress=None,
                      include_node_runtime=False):
    now = time.time()
    usage = usage if usage is not None else item.get("usage") or {}
    timings = timings if timings is not None else item.get("timings") or {}
    if usage:
        item["usage"] = usage
    if timings:
        item["timings"] = timings
    if finish_reason:
        item["finish_reason"] = finish_reason
    decode = _latest_decode_progress(master_progress)
    tokens = _usage_output_tokens(usage)
    token_source = "usage"
    if tokens is None and decode and decode.get("n_decoded") is not None:
        tokens = int(decode.get("n_decoded") or 0)
        token_source = "master_log"
    if tokens is None and timings.get("predicted_n") is not None:
        try:
            tokens = int(timings.get("predicted_n") or 0)
            token_source = "timings"
        except Exception:
            tokens = None
    if tokens is None:
        tokens = int(item.get("output_events") or 0)
        token_source = "stream_events"
    started = item.get("started_at") or item.get("created_at") or now
    elapsed = max(0.001, now - started)
    tps = None
    for key in ("predicted_per_second", "tokens_per_second", "tps"):
        try:
            if timings.get(key) is not None:
                tps = float(timings.get(key))
                break
        except Exception:
            pass
    if tps is None and decode and decode.get("tokens_per_second") is not None:
        tps = float(decode.get("tokens_per_second"))
    if tps is None and tokens:
        tps = float(tokens) / elapsed
    master_task = decode.get("task") if decode else None
    node_metrics = _node_inference_metrics(
        c, item, include_runtime=include_node_runtime, master_task=master_task)
    if include_node_runtime:
        item["node_runtime"] = {n.get("node_id"): n.get("runtime") for n in node_metrics if n.get("node_id")}
    edges = _transfer_edges(c, tokens)
    metrics = {
        "elapsed_s": round(elapsed, 3),
        "output_tokens": int(tokens or 0),
        "output_token_source": token_source,
        "output_events": int(item.get("output_events") or 0),
        "output_chars": int(item.get("output_chars") or 0),
        "tps": round(tps, 3) if tps is not None else None,
        "finish_reason": finish_reason or item.get("finish_reason"),
        "transfer_edges": edges,
        "transfer_total_mib": round(sum(e.get("cumulative_bytes", 0) for e in edges) / (1024 * 1024), 3),
        "node_metrics": node_metrics,
        "master_task": master_task,
        "sample_source": token_source,
    }
    sample = {
        "ts": now,
        "elapsed_s": metrics["elapsed_s"],
        "output_tokens": metrics["output_tokens"],
        "tps": metrics["tps"],
        "transfer_total_mib": metrics["transfer_total_mib"],
        "transfer_edges": edges,
    }
    samples = item.setdefault("samples", [])
    if not samples or sample["output_tokens"] != samples[-1].get("output_tokens") or now - samples[-1].get("ts", 0) > 2:
        samples.append(sample)
        del samples[:-300]
    item["metrics"] = metrics
    item["updated_at"] = now
    return metrics




def _inference_record_detail(record):
    metrics = record.get("metrics") or {}
    return {
        "message": record.get("message"),
        "error": record.get("error"),
        "started_at": record.get("started_at"),
        "finished_at": record.get("finished_at"),
        "controller": record.get("controller") or {},
        "request": record.get("request") or {},
        "usage": record.get("usage") or {},
        "timings": record.get("timings") or {},
        "finish_reason": record.get("finish_reason"),
        "metrics": metrics,
        "samples": record.get("samples") or [],
        "plan": record.get("plan") or {},
        "nodes": record.get("nodes") or [],
        "rpc_topology": record.get("rpc_topology") or [],
    }


def _inference_record_summary(record, include_detail=False):
    metrics = record.get("metrics") or {}
    path = record.get("archive_path")
    summary = {
        "request_id": record.get("request_id"),
        "seq": record.get("seq"),
        "kind": record.get("kind"),
        "status": record.get("status"),
        "created_at": record.get("created_at"),
        "finished_at": record.get("finished_at"),
        "duration_s": record.get("duration_s"),
        "model": record.get("model"),
        "output_tokens": metrics.get("output_tokens"),
        "tps": metrics.get("tps"),
        "transfer_total_mib": metrics.get("transfer_total_mib"),
        "node_count": len(record.get("nodes") or []),
        "archive_path": path,
        "archive_name": os.path.basename(path) if path else "",
    }
    if include_detail:
        summary["detail"] = _inference_record_detail(record)
    return summary


def _load_inference_history(c, limit=20, include_detail=False):
    root = os.path.join(INFERENCE_ARCHIVE_DIR, _safe_slug(c.get("id")))
    paths = sorted(glob.glob(os.path.join(root, "*", "*.json")), key=os.path.getmtime, reverse=True)[:max(1, limit)]
    out = []
    for path in paths:
        try:
            with open(path, "r", encoding="utf-8") as f:
                record = json.load(f)
            record["archive_path"] = record.get("archive_path") or path
            out.append(_inference_record_summary(record, include_detail=include_detail))
        except Exception as exc:
            out.append({"archive_path": path, "status": "unreadable", "error": str(exc)})
    return out




def _inference_snapshot(c):
    state = _inference_tracker(c)
    items = []
    now = time.time()
    master_log = tail(_master_log_path(c), 160)
    master_progress = _parse_master_progress(master_log)
    for item in sorted(state["items"].values(), key=lambda x: x.get("created_at", 0)):
        if item.get("status") in ("running", "streaming"):
            _sample_inference(c, item, master_progress=master_progress, include_node_runtime=True)
        view = {k: v for k, v in item.items() if k not in ("slot_acquired", "sse_buffer")}
        view["elapsed_s"] = round(now - item.get("created_at", now), 3)
        view["queue_wait_s"] = round((item.get("started_at") or now) - item.get("created_at", now), 3)
        view["pipeline"] = _pipeline_for_request(c, item)
        if item.get("status") in ("running", "streaming") and master_progress:
            view["master_progress"] = master_progress
        items.append(view)
    history = list(state.get("history") or [])
    if len(history) < 20:
        seen = {h.get("archive_path") for h in history}
        for rec in _load_inference_history(c, limit=20, include_detail=True):
            if rec.get("archive_path") not in seen:
                history.append(rec)
            if len(history) >= 20:
                break
    return {"limit": state["limit"], "active": sum(1 for x in items if x["status"] in ("running", "streaming")),
            "queued": sum(1 for x in items if x["status"] == "queued"), "items": items, "history": history[:20],
            "archive_dir": os.path.join(INFERENCE_ARCHIVE_DIR, _safe_slug(c.get("id")))}










# Per-node inference contribution (runtime-only). The settlement gateway polls
# /api/contributions and credits DELTAS, remembering what it already credited, so a
# hub restart resetting these counters to 0 is safe (the gateway resets its baseline
# when it sees the cumulative decrease). v1: all of this hub's inference nodes are
# owned by the hub OPERATOR_WALLET; units are each node's LAYER SHARE of the tokens
# produced (1 unit == 1k tokens, matching the gateway's REWARD_PER_UNIT convention).
CONTRIBUTIONS = {}  # node_id -> {node_id, owner, units, model, node_name, backend, os, accelerator, device_kind, perf_tps}


SHARD_SCARCITY_REWARD = os.environ.get("LINKCPP_SHARD_SCARCITY_REWARD", "0").strip().lower() \
    in ("1", "true", "yes")
SHARD_SCARCITY_ALPHA = float(os.environ.get("LINKCPP_SHARD_SCARCITY_ALPHA", "1.0") or 1.0)
SHARD_SCARCITY_MAX = float(os.environ.get("LINKCPP_SHARD_SCARCITY_MAX", "2.0") or 2.0)






@app.get("/api/contributions")
def api_contributions():
    """Cumulative per-node inference contribution for the settlement gateway to poll
    and credit. Behind the auth gate (session or M2M service token)."""
    return {"contributions": list(CONTRIBUTIONS.values())}














# ------------------------------- web UI -----------------------------------
@app.get("/health")
def health():
    """Liveness only — deliberately does not probe linker, so a control-plane
    outage does not make the container look dead and get restarted."""
    return {"ok": True}


@app.get("/")
def index():
    return FileResponse(os.path.join(WEB_DIR, "hub.html"))


# ----------------------------- linker UI ----------------------------------
# The gateway menu opens this in a new window, full screen. Linker's SPA is
# served as-is: its assets are absolute (/assets/...) and the gateway's own UI
# lives under /web/, so the two never collide and no HTML rewriting is needed.
# The app carries no client-side router, so a single entry point is enough.
@app.get("/linker")
async def linker_ui(request: Request):
    return await linker_client.proxy(request, "/")


@app.get("/assets/{path:path}")
async def linker_assets(request: Request, path: str):
    return await linker_client.proxy(request, "/assets/" + path)


# Linker's SPA opens this against window.location.host, i.e. the gateway.
@app.websocket("/api/events")
async def linker_events(ws: WebSocket):
    await linker_client.relay_websocket(ws, "/api/events")


@app.on_event("shutdown")
async def _close_linker_client():
    await linker_client.aclose()


app.mount("/web", StaticFiles(directory=WEB_DIR), name="web")
