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
import os, time, json, uuid, asyncio, subprocess, glob, collections, socket, re, logging, math
from typing import Optional
from urllib.parse import quote, urlparse
import httpx
from fastapi import FastAPI, HTTPException, Request, Query, WebSocket
from fastapi.responses import StreamingResponse, JSONResponse, FileResponse
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel

import hmac as _hmac
from controller import host_resources
from controller import siws
from controller import totp
from controller.model_catalog import is_embedding_model, model_label
from controller.planner import PLACEMENT_STRATEGIES, read_model, plan as run_plan
from controller.runtime_modes import (
    DEFAULT_RUNTIME_MODE,
    LLAMA_RPC,
    RING_PROXY,
    data_plane_contract,
    enrich_topology_item,
    normalize_runtime_mode,
    runtime_mode_catalog,
    serve_background as serve_selected_runtime,
    unload as unload_selected_runtime,
)
from controller.runtimes.gateway import chat as runtime_chat, stream_chat as runtime_stream_chat
from controller.protocol import (
    CancelLoadRequest,
    DownloadRequest,
    LoadMonitorRequest,
    LoadRequest,
    ModelSource,
    NodeReport,
    ResourceSnapshot,
    UnitLoadSessionPrepareRequest,
    UnitLoadSessionReleaseRequest,
    UnloadRequest,
    model_to_dict,
)
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
# Auth is ON when either an admin allowlist OR a balance gate is configured.
AUTH_ENABLED = bool(ADMIN_WALLETS) or MIN_OPERATOR_KVR > 0
SESSION_COOKIE = "linkcpp_session"
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
    return JSONResponse({"error": "authentication required"}, status_code=401)


@app.on_event("startup")
async def _startup_restore_state():
    _ensure_local_slots()
    await _discover_managed_agents()


# ------------------------------- helpers ----------------------------------
def list_gpus():
    return host_resources.local_gpus()


def _system_ram_gib():
    return round(host_resources.memory_info().get("total", 0.0), 1)


def _master_ram_budget_gib():
    memory = host_resources.memory_info()
    reserve = float(os.environ.get("LINKCPP_MASTER_RAM_RESERVE_GIB", "8"))
    return round(max(0.0, memory.get("total", 0.0) - memory.get("used", 0.0) - reserve), 2)


def _master_load_timeout_s(result):
    """Return the initial health-check window for a model load.

    With stock llama.cpp RPC, the master still opens every GGUF shard. A
    Windows Docker bind mount can sustain far less than local NVMe bandwidth,
    so this is only a bootstrap estimate.  Once RPC tensor traffic is observed,
    ``_wait_health`` replaces it with a progress-renewed deadline derived from
    the measured transfer rate.
    """
    weight_gib = max(0.0, float(result.get("total_weight_gib") or 0.0))
    read_mib_s = max(16.0, float(os.environ.get("LINKCPP_MASTER_LOAD_MIB_S", "64")))
    setup_s = max(0, int(os.environ.get("LINKCPP_MASTER_LOAD_SETUP_S", "300")))
    estimate_s = int(math.ceil(weight_gib * 1024.0 / read_mib_s)) + setup_s
    return max(900, estimate_s)


def _load_observed_transfer_bytes(c, parent_op_id):
    """Return cumulative RPC tensor bytes reported by this load's workers."""
    total = 0
    for op in _ctrl_ops(c).values():
        if op.get("type") != "node_load":
            continue
        details = op.get("details") or {}
        if details.get("parent_op_id") != parent_op_id:
            continue
        activity = details.get("rpc_activity") or {}
        total += max(0, int(activity.get("bytes_observed") or 0))
    return total


def _load_stall_timeout_s(expected_bytes, observed_bytes, bytes_per_s, bootstrap_s):
    """Allow twice the measured full-transfer duration before declaring a stall.

    This is intentionally a *stall* deadline, not a total wall-clock limit:
    any new tensor traffic renews it.  A large model on a slow LAN therefore
    continues loading, while a genuinely stuck load still terminates.
    """
    if bytes_per_s <= 0:
        return max(900, int(bootstrap_s))
    expected = max(int(expected_bytes or 0), int(observed_bytes or 0))
    full_transfer_s = expected / bytes_per_s
    return max(900, int(math.ceil(full_transfer_s * 2)))


def _master_startup_fatal(log_text):
    """Recognize errors that cannot become healthy by waiting longer."""
    text = (log_text or "").lower()
    return any(marker in text for marker in (
        "remote rpc server crashed",
        "remote rpc server returned malformed response",
        "failed to create graph node",
        "invalid data ptr",
        "failed to load model",
    ))


def local_resource_limits():
    return {
        "ram_total_gib": _system_ram_gib(),
        "cpu_cores": os.cpu_count() or 0,
    }


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


def _request_log_fields(body):
    messages = body.get("messages") or body.get("input") or []
    return {
        "prompt_chars": _payload_size_hint(body),
        "max_tokens": body.get("max_tokens") or body.get("max_output_tokens"),
        "stream": bool(body.get("stream")),
        "message_count": len(messages) if isinstance(messages, list) else 1,
    }


def _plan_log_summary(result):
    if not result:
        return {}
    placement = []
    for p in result.get("placement", []) or []:
        if not p.get("n_layers"):
            continue
        placement.append({
            "node": p.get("node"),
            "node_id": p.get("node_id"),
            "node_name": p.get("node_name"),
            "layers": p.get("layers"),
            "vram_used_gib": p.get("vram_used_gib"),
            "ram_used_gib": p.get("ram_used_gib"),
            "kv_vram_gib": p.get("kv_vram_gib"),
            "kv_ram_gib": p.get("kv_ram_gib"),
            "layer_body_vram_gib": p.get("layer_body_vram_gib"),
            "layer_body_ram_gib": p.get("layer_body_ram_gib"),
            "ffn_vram_gib": p.get("ffn_vram_gib"),
            "ffn_ram_gib": p.get("ffn_ram_gib"),
            "offload_policy": p.get("offload_policy"),
        })
    return {
        "feasible": result.get("feasible"),
        "reason": result.get("reason"),
        "model_ref": result.get("model_ref"),
        "model_label": result.get("model_label"),
        "resource_totals": result.get("resource_totals"),
        "need_vram_gib": result.get("need_vram_gib"),
        "sum_vram_budget_gib": result.get("sum_vram_budget_gib"),
        "sum_ram_budget_gib": result.get("sum_ram_budget_gib"),
        "kv_total_gib": result.get("kv_total_gib"),
        "kv_cache_location": result.get("kv_cache_location"),
        "kv_offload_enabled": result.get("kv_offload_enabled"),
        "cache_type_k": result.get("cache_type_k"),
        "cache_type_v": result.get("cache_type_v"),
        "flash_attention": result.get("flash_attention"),
        "nodes_used": result.get("nodes_used"),
        "tensor_split": result.get("tensor_split"),
        "master_load_timeout_s": result.get("master_load_timeout_s"),
        "placement": placement,
        "adaptive_load_available": result.get("adaptive_load_available"),
        "adaptive_load_blocker": result.get("adaptive_load_blocker"),
    }


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


def _load_diag_dir(c, op_id=None):
    existing = c.get("load_diagnostic_dir")
    if existing:
        return existing
    stamp = time.strftime("%Y%m%dT%H%M%SZ", time.gmtime())
    suffix = _safe_slug(op_id or uuid.uuid4().hex[:10])
    path = os.path.join(LOAD_DIAGNOSTICS_DIR, _safe_slug(c.get("id")), f"{stamp}_{suffix}")
    os.makedirs(path, exist_ok=True)
    c["load_diagnostic_dir"] = path
    return path


def _diag_write_json(path, data):
    with open(path, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=2, sort_keys=True, default=str)


def _diag_event(c, op_id, event, **fields):
    path = _load_diag_dir(c, op_id)
    payload = {"ts": time.time(), "event": event, **fields}
    with open(os.path.join(path, "events.jsonl"), "a", encoding="utf-8") as f:
        f.write(json.dumps(payload, sort_keys=True, default=str) + "\n")


def _master_process_snapshot(c):
    proc = c.get("master")
    log_path = _master_log_path(c)
    try:
        log_size = os.path.getsize(log_path)
    except OSError:
        log_size = 0
    return {
        "pid": getattr(proc, "pid", None),
        "returncode": proc.poll() if proc else None,
        "alive": _proc_alive(proc),
        "log_path": log_path,
        "log_size_bytes": log_size,
        "host_memory": host_resources.memory_info(),
    }


async def _capture_load_diagnostics(c, op_id, reason, *, include_node_logs=False):
    """Persist enough evidence to diagnose one distributed load after cleanup.

    Operations retain only compact UI data.  This artifact preserves the full
    master stderr and, on terminal paths, substantial worker-log tails before
    teardown removes the processes and their in-memory context.
    """
    path = _load_diag_dir(c, op_id)
    stamp = f"snapshot-{int(time.time() * 1000)}"
    snapshot = {
        "reason": reason,
        "controller": {k: c.get(k) for k in ("id", "name", "phase", "detail", "model", "master_port")},
        "master": _master_process_snapshot(c),
        "operations": list(_ctrl_ops(c).values()),
        "plan": c.get("plan"),
    }
    _diag_write_json(os.path.join(path, f"{stamp}.json"), snapshot)
    _diag_event(c, op_id, "snapshot", reason=reason, snapshot=f"{stamp}.json")
    try:
        with open(_master_log_path(c), "rb") as src, open(os.path.join(path, "master.log"), "wb") as dst:
            dst.write(src.read())
    except OSError as exc:
        _diag_event(c, op_id, "master_log_copy_failed", error=str(exc))
    if not include_node_logs:
        return path
    node_dir = os.path.join(path, "nodes")
    os.makedirs(node_dir, exist_ok=True)
    for nid in c.get("nodes", []):
        n = NODES.get(nid)
        if not n:
            continue
        try:
            payload = await _load_monitor_log(n)
        except Exception as exc:
            payload = {"error": str(exc), "log": ""}
        _diag_write_json(os.path.join(node_dir, f"{_safe_slug(nid)}.json"), {
            "node": node_view(n), "capture": {k: v for k, v in payload.items() if k != "log"},
        })
        with open(os.path.join(node_dir, f"{_safe_slug(nid)}.log"), "w", encoding="utf-8") as f:
            f.write(str(payload.get("log") or ""))
    return path


KV_CACHE_TYPES_FALLBACK = ["f32", "f16", "bf16", "q8_0", "q4_0", "q4_1", "iq4_nl", "q5_0", "q5_1"]
_KV_CACHE_TYPES_CACHE = None


def _supported_kv_cache_types():
    global _KV_CACHE_TYPES_CACHE
    if _KV_CACHE_TYPES_CACHE is not None:
        return _KV_CACHE_TYPES_CACHE
    try:
        out = subprocess.check_output([LLAMA_SERVER, "--help"], text=True, stderr=subprocess.STDOUT, timeout=10)
        match = re.search(r"--cache-type-k[\s\S]*?allowed values:\s*([^\r\n]+)", out)
        if match:
            values = [v.strip() for v in match.group(1).split(",") if v.strip()]
            if values:
                _KV_CACHE_TYPES_CACHE = values
                return values
    except Exception as exc:
        _log_event("kv_cache_type_probe_failed", error=str(exc), llama_server=LLAMA_SERVER)
    _KV_CACHE_TYPES_CACHE = list(KV_CACHE_TYPES_FALLBACK)
    return _KV_CACHE_TYPES_CACHE


def _validate_cache_type(value, field):
    value = str(value or "f16").lower()
    supported = _supported_kv_cache_types()
    if value not in supported:
        raise HTTPException(400, f"{field} {value!r} is not supported by this llama.cpp runtime")
    return value


def _local_slot_id(index):
    return f"node-slot-{index}"


def _default_slot_name(index):
    return f"Slot {index}"


def _slot_log_path(nid):
    return f"/tmp/node-{nid}.log"


def _touch_log(path):
    try:
        os.makedirs(os.path.dirname(path), exist_ok=True)
        open(path, "w").close()
    except Exception:
        pass


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


def _reset_local_slot(n):
    _kill(n.get("worker"))
    index = int(n.get("slot") or 1)
    fresh = _new_local_slot(index)
    fresh["log"] = n.get("log") or fresh["log"]
    NODES[n["id"]] = fresh
    try:
        _touch_log(fresh["log"])
    except Exception:
        pass
    return fresh


def _short_gpu_name(name):
    m = re.search(r"RTX\s+(\d+\w*)", name or "")
    return m.group(1) if m else (name or "node").split()[-1]


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


def _clear_ctrl_activity(c, reason, model=None):
    count = len(c.get("operations", {}) or {})
    c["operations"] = {}
    _log_event("controller_activity_cleared", controller_id=c.get("id"),
               reason=reason, model=model, cleared=count)


def _reset_inference_activity(c, reason):
    state = INFERENCE.pop(c.get("id"), None)
    count = len((state or {}).get("items", {}))
    if count:
        _log_event("inference_activity_cleared", controller_id=c.get("id"),
                   reason=reason, cleared=count)


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


def _load_monitor_key(c, node_id, op_id):
    return f"{c.get('id')}:{node_id}:{op_id}"


def _rpc_activity_metrics(log_text):
    log_text = log_text or ""
    sizes = [int(x) for x in re.findall(r"\[(?:set_tensor|get_tensor|alloc_buffer)\][^\n]*size:\s*(\d+)", log_text)]
    get_tensor_sizes = [int(x) for x in re.findall(r"\[get_tensor\][^\n]*size:\s*(\d+)", log_text)]
    return {
        "alloc_buffer_count": len(re.findall(r"\[alloc_buffer\]", log_text)),
        "set_tensor_count": len(re.findall(r"\[set_tensor\]", log_text)),
        "get_tensor_count": len(re.findall(r"\[get_tensor\]", log_text)),
        "get_alloc_size_count": len(re.findall(r"\[get_alloc_size\]", log_text)),
        "graph_compute_count": len(re.findall(r"\[graph_compute\]", log_text)),
        "accepted_connections": len(re.findall(r"Accepted client connection", log_text)),
        "get_tensor_bytes": sum(get_tensor_sizes),
        "bytes_observed": sum(sizes),
    }


def _node_resource_report(n, node_log=None):
    existing = n.get("resources") or {}
    if n.get("kind") == "local":
        vram_used = gpu_used_gib(n.get("gpu_uuid")) if n.get("gpu_uuid") else 0.0
        ram_used = n.get("ram_used", 0.0)
        ram_total = _system_ram_gib()
        cores_total = os.cpu_count() or 0
        cpu_used = host_resources.cpu_percent()
    else:
        vram_used = float((node_log or {}).get("vram_used_gib") or existing.get("vram_used_gib") or 0.0)
        ram_used = float((node_log or {}).get("ram_used_gib") or n.get("ram_used", 0.0) or 0.0)
        ram_total = float(existing.get("ram_total_gib") or n.get("ram") or 0.0)
        cores_total = int(existing.get("cores_total") or n.get("cores") or 0)
        cpu_used = float(existing.get("cpu_used_percent") or 0.0)
    return ResourceSnapshot(
        vram_total_gib=float(n.get("vram") or 0.0),
        vram_used_gib=round(vram_used, 3),
        vram_budget_gib=float(n.get("vram") or 0.0),
        ram_total_gib=ram_total,
        ram_used_gib=round(ram_used, 3),
        ram_budget_gib=float(n.get("ram") or 0.0),
        cores_total=cores_total,
        cores_budget=int(n.get("cores") or 0),
        cpu_used_percent=cpu_used,
        disk_free_gib=float(existing.get("disk_free_gib") or 0.0),
    )


async def _load_monitor_log(n):
    if n.get("kind") == "remote_unit_node":
        source_id = n.get("remote_source_node_id") or ""
        try:
            payload = await _remote_unit_node_request(
                n, "GET", f"/api/nodes/{quote(source_id, safe='')}/logs?tail=5000", timeout=10)
            return payload if isinstance(payload, dict) else {"log": str(payload)}
        except Exception as exc:
            return {"log": "", "error": str(exc)}
    if n.get("kind") == "agent":
        try:
            info = await _refresh_agent_node(n)
            reports = (info or {}).get("last_reports", [])
            log = "\n".join(
                f"{r.get('seq', '')} {r.get('op_type', '')} {r.get('phase', '')} "
                f"{r.get('status', '')} {r.get('progress', 0)}% {r.get('message', '')}"
                for r in reports[-20:])
            resources = n.get("resources") or {}
            return {"log": log, "vram_used_gib": resources.get("vram_used_gib", 0.0),
                    "ram_used_gib": resources.get("ram_used_gib", n.get("ram_used", 0.0))}
        except Exception as exc:
            return {"log": "", "error": str(exc)}
    return {"log": tail(n.get("log", ""), 320),
            "vram_used_gib": gpu_used_gib(n.get("gpu_uuid")) if n.get("gpu_uuid") else 0.0,
            "ram_used_gib": n.get("ram_used", 0.0)}


async def _unit_session_monitor_log(c, n):
    """Read measurement data from the unit session, with legacy log polling fallback."""
    session = (c.get("unit_load_sessions") or {}).get(n.get("remote_unit_id"))
    if not session or not session.get("response"):
        return await _load_monitor_log(n)
    try:
        data = await _remote_unit_node_request(
            n, "GET", f"/api/unit/load-sessions/{quote(session['session_id'], safe='')}", timeout=20)
        node = (data.get("nodes") or {}).get(n.get("remote_source_node_id")) or {}
        resources = node.get("resources") or {}
        return {
            "log": node.get("worker_log_tail", ""),
            "vram_used_gib": resources.get("vram_used_gib", 0.0),
            "ram_used_gib": resources.get("ram_used_gib", 0.0),
            "unit_session_id": session.get("session_id"),
            "unit_measurement": node,
        }
    except Exception as exc:
        fallback = await _load_monitor_log(n)
        fallback["unit_session_error"] = str(exc)
        return fallback


def _monitor_progress(sample, baseline, placement, c):
    planned_vram = float(placement.get("vram_used_gib") or 0.0)
    planned_ram = float(placement.get("ram_used_gib") or 0.0)
    layers = int(placement.get("n_layers") or 1)
    resources = sample["resources"]
    vram_delta = max(0.0, resources.vram_used_gib - baseline.get("vram_used_gib", resources.vram_used_gib))
    ram_delta = max(0.0, resources.ram_used_gib - baseline.get("ram_used_gib", resources.ram_used_gib))
    vram_ratio = min(1.0, vram_delta / planned_vram) if planned_vram > 0 else 0.0
    ram_ratio = min(1.0, ram_delta / planned_ram) if planned_ram > 0 else 0.0
    activity = sample["activity"]
    expected_events = max(20, layers * 8)
    activity_events = activity["alloc_buffer_count"] + activity["set_tensor_count"] + activity["get_alloc_size_count"] * 0.08
    activity_ratio = min(1.0, activity_events / expected_events)
    ratio = max(vram_ratio, ram_ratio * 0.6, activity_ratio)
    if _ctrl_phase(c) == "running":
        return 100.0
    if _ctrl_phase(c) == "error":
        return 0.0
    return round(10.0 + ratio * 80.0, 1)


def _node_load_report(c, n, op_id, phase, status, progress, message, resources,
                      model=None, error=None, details=None):
    seq = int((n.get("last_report") or {}).get("seq", 0)) + 1
    report = NodeReport(
        node_id=n["id"],
        controller_id=c.get("id"),
        op_id=op_id,
        op_type="node_load",
        phase=phase,
        status=status,
        progress=progress,
        message=message,
        resources=resources,
        model=model or c.get("model"),
        error=error,
        seq=seq,
    )
    data = model_to_dict(report)
    data.setdefault("details", {})
    if details:
        data["details"] = details
    n["last_report"] = data
    n["resources"] = data.get("resources", {})
    ops = n.setdefault("operations_map", {})
    ops[op_id] = data
    n["operations"] = list(ops.values())[-50:]
    _record_ctrl_op(c, "node_load", phase, status, progress, message,
                    op_id=op_id, node_id=n["id"], model=model or c.get("model"),
                    error=error, details=details or {})


async def _load_monitor_loop(c, req, nid, placement, op_id, parent_op_id):
    n = NODES.get(nid)
    if not n:
        return
    first_payload = (await _unit_session_monitor_log(c, n)
                     if n.get("kind") == "remote_unit_node" else await _load_monitor_log(n))
    baseline_resources = _node_resource_report(n, first_payload)
    baseline = {
        "vram_used_gib": baseline_resources.vram_used_gib,
        "ram_used_gib": baseline_resources.ram_used_gib,
    }
    _node_load_report(c, n, op_id, "worker_ready", "running", 10.0,
                      "node load monitor started", baseline_resources, model=req.model,
                      details={"parent_op_id": parent_op_id, "layers": placement.get("layers"),
                               "node_kind": n.get("kind", "local"), "baseline": baseline})
    try:
        while True:
            await asyncio.sleep(2.0)
            payload = (await _unit_session_monitor_log(c, n)
                       if n.get("kind") == "remote_unit_node" else await _load_monitor_log(n))
            resources = _node_resource_report(n, payload)
            activity = _rpc_activity_metrics(payload.get("log", ""))
            sample = {"resources": resources, "activity": activity}
            phase = "rpc_loading" if activity["alloc_buffer_count"] or activity["set_tensor_count"] or activity["get_alloc_size_count"] else "waiting_for_rpc"
            status = "running"
            progress = _monitor_progress(sample, baseline, placement, c)
            if _ctrl_phase(c) == "running":
                phase, status, progress = "loaded", "done", 100.0
            elif _ctrl_phase(c) == "error":
                phase, status, progress = "load_error", "error", 0.0
            message = (
                f"{phase}: vram {resources.vram_used_gib:.2f}/{placement.get('vram_used_gib', 0)} GiB, "
                f"rpc alloc {activity['alloc_buffer_count']}, set {activity['set_tensor_count']}"
            )
            if payload.get("error"):
                message += f", monitor warning: {payload['error']}"
            _node_load_report(c, n, op_id, phase, status, progress, message,
                              resources, model=req.model,
                              error=payload.get("error") if status == "error" else None,
                              details={"parent_op_id": parent_op_id,
                                       "layers": placement.get("layers"),
                                       "node_kind": n.get("kind", "local"),
                                       "planned_vram_gib": placement.get("vram_used_gib"),
                                       "planned_ram_gib": placement.get("ram_used_gib"),
                                       "rpc_activity": activity,
                                       "unit_session_id": payload.get("unit_session_id"),
                                       "unit_measurement": payload.get("unit_measurement"),
                                       "monitor_source": "unit_load_session" if payload.get("unit_session_id") else
                                       ("remote_unit_poll" if n.get("kind") == "remote_unit_node" else "local_scheduler")})
            if status in ("done", "error") or _ctrl_phase(c) not in ("loading",):
                return
    except asyncio.CancelledError:
        resources = _node_resource_report(n)
        _node_load_report(c, n, op_id, "canceled", "canceled", 0.0,
                          "node load monitor canceled", resources, model=req.model,
                          details={"parent_op_id": parent_op_id, "layers": placement.get("layers")})
        raise


def _start_load_monitor(c, req, nid, placement, parent_op_id):
    op_id = f"node-load-{parent_op_id}-{_safe_slug(nid)}"[:120]
    key = _load_monitor_key(c, nid, op_id)
    old = LOAD_MONITORS.pop(key, None)
    if old:
        old.cancel()
    task = asyncio.create_task(_load_monitor_loop(c, req, nid, placement, op_id, parent_op_id))
    LOAD_MONITORS[key] = task
    task.add_done_callback(lambda _t, k=key: LOAD_MONITORS.pop(k, None))
    n = NODES.get(nid)
    if n and n.get("kind") == "remote_unit_node" and PUBLIC_HUB_URL:
        asyncio.create_task(_start_remote_unit_standalone_monitor(c, req, n, placement, op_id))
    return op_id


def _cancel_load_monitors(c, *, final_status="canceled"):
    prefix = f"{c.get('id')}:"
    for key, task in list(LOAD_MONITORS.items()):
        if key.startswith(prefix):
            parts = key.split(":", 2)
            if len(parts) == 3:
                n = NODES.get(parts[1])
                if n and n.get("kind") == "remote_unit_node" and PUBLIC_HUB_URL:
                    asyncio.create_task(_stop_remote_unit_standalone_monitor(c, n, parts[2]))
                if n and n.get("kind") == "agent" and final_status == "done":
                    body = model_to_dict(CancelLoadRequest(op_id=parts[2], reason="load_done"))
                    asyncio.create_task(_agent_request(n, "POST", "/control/load-monitor/stop", body, timeout=10))
            if final_status == "done":
                continue
            task.cancel()


async def _start_remote_unit_standalone_monitor(c, req, n, placement, op_id):
    source_id = n.get("remote_source_node_id")
    if not source_id:
        return
    body = model_to_dict(LoadMonitorRequest(
        controller_id=c.get("id"),
        op_id=op_id,
        model=req.model,
        report_node_id=n.get("id"),
        layers=placement.get("layers"),
        planned_vram_gib=float(placement.get("vram_used_gib") or 0.0),
        planned_ram_gib=float(placement.get("ram_used_gib") or 0.0),
        report_url=PUBLIC_HUB_URL.rstrip("/") + "/api/node-reports",
        interval_s=2.0,
    ))
    try:
        await _remote_unit_node_request(
            n, "POST", f"/api/nodes/{quote(source_id, safe='')}/load-monitor/start", body=body, timeout=10)
        _log_event("remote_unit_load_monitor_started", controller_id=c.get("id"),
                   node_id=n.get("id"), source_node_id=source_id, op_id=op_id)
    except Exception as exc:
        _log_event("remote_unit_load_monitor_unavailable", controller_id=c.get("id"),
                   node_id=n.get("id"), source_node_id=source_id, op_id=op_id, error=str(exc))


async def _stop_remote_unit_standalone_monitor(c, n, op_id):
    source_id = n.get("remote_source_node_id")
    if not source_id:
        return
    body = model_to_dict(LoadMonitorRequest(
        controller_id=c.get("id"),
        op_id=op_id,
        model=c.get("model") or "",
        report_node_id=n.get("id"),
        report_url=PUBLIC_HUB_URL.rstrip("/") + "/api/node-reports",
    ))
    try:
        await _remote_unit_node_request(
            n, "POST", f"/api/nodes/{quote(source_id, safe='')}/load-monitor/stop", body=body, timeout=10)
    except Exception as exc:
        _log_event("remote_unit_load_monitor_stop_unavailable", controller_id=c.get("id"),
                   node_id=n.get("id"), source_node_id=source_id, op_id=op_id, error=str(exc))


async def _push_report(url, report):
    if not url:
        return False
    try:
        async with httpx.AsyncClient(timeout=5) as client:
            resp = await client.post(url, json=model_to_dict(report), headers=_service_headers())
            resp.raise_for_status()
        return True
    except Exception:
        return False


async def _standalone_node_monitor_loop(nid, req: LoadMonitorRequest):
    n = NODES.get(nid)
    if not n:
        return
    baseline_payload = await _load_monitor_log(n)
    baseline_resources = _node_resource_report(n, baseline_payload)
    baseline = {
        "vram_used_gib": baseline_resources.vram_used_gib,
        "ram_used_gib": baseline_resources.ram_used_gib,
    }
    seq = 0
    try:
        while True:
            payload = await _load_monitor_log(n)
            resources = _node_resource_report(n, payload)
            activity = _rpc_activity_metrics(payload.get("log", ""))
            vram_delta = max(0.0, resources.vram_used_gib - baseline["vram_used_gib"])
            ram_delta = max(0.0, resources.ram_used_gib - baseline["ram_used_gib"])
            vram_ratio = min(1.0, vram_delta / req.planned_vram_gib) if req.planned_vram_gib > 0 else 0.0
            ram_ratio = min(1.0, ram_delta / req.planned_ram_gib) if req.planned_ram_gib > 0 else 0.0
            activity_ratio = min(1.0, (activity["alloc_buffer_count"] + activity["set_tensor_count"] + activity["get_alloc_size_count"] * 0.08) / 40.0)
            progress = round(10.0 + max(vram_ratio, ram_ratio * 0.6, activity_ratio) * 80.0, 1)
            phase = "rpc_loading" if activity["alloc_buffer_count"] or activity["set_tensor_count"] or activity["get_alloc_size_count"] else "waiting_for_rpc"
            seq += 1
            report = NodeReport(
                node_id=req.report_node_id or nid,
                controller_id=req.controller_id,
                op_id=req.op_id,
                op_type="node_load",
                phase=phase,
                status="running",
                progress=progress,
                message=f"{phase}: vram {resources.vram_used_gib:.2f}/{req.planned_vram_gib:.2f} GiB, rpc alloc {activity['alloc_buffer_count']}, set {activity['set_tensor_count']}",
                resources=resources,
                model=req.model,
                error=payload.get("error"),
                seq=seq,
            )
            ops = n.setdefault("operations_map", {})
            ops[req.op_id] = model_to_dict(report)
            n["operations"] = list(ops.values())[-50:]
            n["last_report"] = model_to_dict(report)
            await _push_report(req.report_url, report)
            await asyncio.sleep(max(0.5, float(req.interval_s or 2.0)))
    except asyncio.CancelledError:
        seq += 1
        report = NodeReport(
            node_id=req.report_node_id or nid,
            controller_id=req.controller_id,
            op_id=req.op_id,
            op_type="node_load",
            phase="monitor_stopped",
            status="done",
            progress=100.0,
            message="node load monitor stopped",
            resources=_node_resource_report(n),
            model=req.model,
            seq=seq,
        )
        await _push_report(req.report_url, report)
        raise


@app.post("/api/nodes/{nid}/load-monitor/start")
async def api_node_load_monitor_start(nid: str, req: LoadMonitorRequest):
    _ensure_local_slots()
    n = NODES.get(nid)
    if not n:
        raise HTTPException(404, "unknown node")
    if n.get("kind", "local") != "local":
        raise HTTPException(409, "load monitor endpoint is only for local unit slots")
    key = f"standalone:{nid}:{req.op_id}"
    old = LOAD_MONITORS.pop(key, None)
    if old:
        old.cancel()
    task = asyncio.create_task(_standalone_node_monitor_loop(nid, req))
    LOAD_MONITORS[key] = task
    task.add_done_callback(lambda _t, k=key: LOAD_MONITORS.pop(k, None))
    return {"accepted": True, "op_id": req.op_id, "node_id": nid}


@app.post("/api/nodes/{nid}/load-monitor/stop")
async def api_node_load_monitor_stop(nid: str, req: LoadMonitorRequest):
    key = f"standalone:{nid}:{req.op_id}"
    task = LOAD_MONITORS.pop(key, None)
    if task:
        task.cancel()
    return {"stopped": bool(task), "op_id": req.op_id, "node_id": nid}


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


def _controller_teardown_lock(c):
    return CTRL_TEARDOWN_LOCKS.setdefault(c["id"], asyncio.Lock())


async def _wait_agent_worker_stopped(n, timeout=20.0, *, require_unbound=False):
    """Confirm native runtime processes exit before clearing node ownership."""
    deadline = time.time() + timeout
    last_error = ""
    while time.time() < deadline:
        try:
            info = await _agent_request(n, "GET", "/control/status", timeout=5)
            _apply_agent_info(n, info)
            released = (not n.get("worker_running") and not n.get("desired_load")
                        and not info.get("stage_running"))
            if released and (not require_unbound or not info.get("bound_to")):
                return info
        except Exception as exc:
            last_error = str(exc)
        await asyncio.sleep(0.5)
    detail = "agent still reports runtime resources as active"
    if require_unbound:
        detail += " or remains bound"
    if last_error:
        detail += f" ({last_error})"
    raise RuntimeError(detail)


async def _stop_agent_worker_confirmed(n, path, body, *, timeout=20.0):
    await _agent_request(n, "POST", path, body, timeout=timeout)
    return await _wait_agent_worker_stopped(n, timeout=timeout)


async def _unbind_agent_confirmed(n, *, timeout=20.0):
    await _agent_request(n, "POST", "/unbind", {}, timeout=timeout)
    return await _wait_agent_worker_stopped(n, timeout=timeout, require_unbound=True)


@app.get("/api/runtime")
def api_runtime():
    _ensure_local_slots()
    nodes = [node_view(n) for n in sorted(NODES.values(), key=_node_sort_key)
             if n.get("kind") != "remote_unit_node"] if NODES else []
    return {
        "runtime": runtime_identity(),
        "backend": backend_identity(),
        "label": runtime_label(),
        "backend_label": backend_label(),
        "kv_cache_types": _supported_kv_cache_types(),
        "flash_attention": {"forced": True, "value": "on"},
        "runtime_modes": runtime_mode_catalog(),
        "unit_control_protocols": ["unit-load-session/v1"],
        "nodes": nodes,
        "controllers": [ctrl_view(c) for c in CTRLS.values()],
        "remote_units": [_remote_unit_view(r) for r in REMOTE_UNITS.values()],
        "operator_wallet": OPERATOR_WALLET,
    }


class OperatorWallet(BaseModel):
    wallet: str = ""


_B58_CHARS = set("123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz")


def _valid_wallet(w):
    w = (w or "").strip()
    return w == "" or (32 <= len(w) <= 44 and all(c in _B58_CHARS for c in w))


@app.get("/api/operator")
def api_get_operator():
    return {"wallet": OPERATOR_WALLET}


@app.post("/api/operator")
def api_set_operator(body: OperatorWallet):
    global OPERATOR_WALLET
    w = (body.wallet or "").strip()
    if not _valid_wallet(w):
        raise HTTPException(400, "invalid Solana wallet address")
    OPERATOR_WALLET = w
    _persist_hub_state()
    return {"wallet": OPERATOR_WALLET}


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
        return {"enabled": False, "authenticated": True, "wallet": None}
    w = _authed_wallet(request)
    return {"enabled": True, "authenticated": bool(w), "wallet": w}


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


def _issue_session(wallet):
    resp = JSONResponse({"wallet": wallet})
    resp.set_cookie(SESSION_COOKIE, siws.make_session(wallet), httponly=True,
                    samesite="lax", max_age=86400, path="/")
    return resp


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


@app.get("/api/kv-cache-types")
def api_kv_cache_types():
    return {
        "types": _supported_kv_cache_types(),
        "default": "f16",
        "flash_attention": {"forced": True, "value": "on"},
    }


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


def _node_runtime_report(n):
    return compatibility_report(_node_runtime(n))


def _runtime_error_message(report):
    expected = runtime_label(report.get("expected"))
    actual = runtime_label(report.get("actual")) if report.get("actual") else "missing runtime identity"
    mismatched = ", ".join(m.get("field", "") for m in report.get("mismatches", [])) or "runtime"
    return f"protocol mismatch ({mismatched}); expected {expected}; got {actual}"


def _require_node_runtime(n):
    report = _node_runtime_report(n)
    if not report["compatible"]:
        raise HTTPException(409, _runtime_error_message(report))
    return report


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


def _require_controller_runtime(c):
    report = _controller_runtime_check(c)
    if not report["compatible"]:
        names = ", ".join(n.get("name") or n.get("id") for n in report["nodes"])
        raise HTTPException(409, f"controller has protocol-incompatible nodes: {names}")
    return report


def _start_node_worker(n):
    if n.get("kind") in ("remote", "remote_unit_node", "agent"):
        _log_event("node_worker_skip_start", node_id=n.get("id"), kind=n.get("kind"))
        return
    if not _slot_configured(n):
        raise HTTPException(409, "assign node resources before starting the worker")
    if _node_worker_running(n):
        _log_event("node_worker_already_running", node_id=n.get("id"), rpc_port=n.get("rpc_port"))
        return
    cache = f"/root/.cache/linkcpp/{n['id']}"
    os.makedirs(cache, exist_ok=True)
    env = dict(os.environ, CUDA_VISIBLE_DEVICES=n["gpu_uuid"],
               GGML_RPC_DEBUG="1", LLAMA_CACHE=cache)
    cmd = [RPC_BIN, "-H", "0.0.0.0", "-p", str(n["rpc_port"]), "-c"]
    _log_event("node_worker_starting", node_id=n.get("id"), rpc_port=n.get("rpc_port"),
               gpu_uuid=n.get("gpu_uuid"), log_path=n.get("log"), cmd=cmd)
    n["worker"] = subprocess.Popen(
        cmd,
        env=env, stdout=open(n["log"], "w"), stderr=subprocess.STDOUT)
    _log_event("node_worker_started", node_id=n.get("id"), rpc_port=n.get("rpc_port"),
               pid=n["worker"].pid, log_path=n.get("log"))


def _parse_endpoint(endpoint):
    host, sep, port = endpoint.strip().rpartition(":")
    if not sep or not host or not port.isdigit():
        raise HTTPException(400, "endpoint must be host:port")
    return host, int(port)


def _remote_unit_rpc_host(base_url):
    parsed = urlparse(base_url)
    if not parsed.hostname:
        raise HTTPException(400, "remote unit URL has no host")
    return parsed.hostname


def _remote_unit_source_node_configured(nd):
    if nd.get("assigned") is False and not nd.get("gpu_uuid"):
        return False
    return True


def _remote_unit_node_bound_in_unit(n):
    return n.get("kind") == "remote_unit_node" and bool(n.get("remote_controller_id"))


def _tcp_reachable(host, port, timeout=2.0):
    try:
        with socket.create_connection((host, port), timeout=timeout):
            return True
    except OSError:
        return False


def _rpc_log_ready(log_text):
    text = log_text or ""
    if "failed to initialize CUDA" in text or "Remote RPC server crashed" in text:
        return False
    return (
        "ggml_cuda_init: found" in text
        or "CUDA devices" in text
        or ("Starting RPC server" in text and "endpoint" in text)
        # The startup banner can age out of a bounded log tail while a healthy
        # worker continues handling RPC requests.
        or "Accepted client connection" in text
        or "graph_recompute" in text
        or "[graph_compute]" in text
    )


async def _wait_node_rpc_ready(n, timeout=20.0):
    deadline = time.time() + timeout
    last = {
        "worker_running": False,
        "tcp_reachable": False,
        "rpc_log_ready": False,
        "error": "",
        "log_tail": "",
    }
    while time.time() < deadline:
        host, port = n.get("rpc_host"), n.get("rpc_port")
        last["worker_running"] = bool(_node_worker_running(n))
        last["tcp_reachable"] = await asyncio.to_thread(_tcp_reachable, host, port, 1.0) if host and port else False
        if n.get("kind") == "remote_unit_node":
            sample = _read_remote_node_log_delta(n, {})
            log_text = sample.get("recent_log", "")
            last["error"] = sample.get("error", "")
        else:
            log_text = tail(n.get("log"), 80)
        last["log_tail"] = log_text[-1200:]
        last["rpc_log_ready"] = _rpc_log_ready(log_text)
        # Native managed agents own their worker log on the host, not in the
        # hub container.  Their successful control/load response plus a TCP
        # connection is the readiness signal; requiring the container-local
        # log would wait until timeout even after Metal is serving.
        ready_signal = last["tcp_reachable"] if n.get("kind") == "agent" else last["rpc_log_ready"]
        if last["worker_running"] and last["tcp_reachable"] and ready_signal and not last["error"]:
            return {**last, "ready": True}
        await asyncio.sleep(0.5)
    return {**last, "ready": False}


def _apply_remote_unit_node_status(n, info):
    resources = info.get("resources") or {}
    n["worker_running"] = bool(info.get("worker_running"))
    n["desired_load"] = info.get("desired_load")
    n["resources"] = resources
    n["ram_used"] = resources.get("ram_used_gib", info.get("ram_used_gib", n.get("ram_used", 0.0)))
    n["capabilities"] = info.get("capabilities", n.get("capabilities", {}))
    n["host_platform"] = info.get("host_platform", n.get("host_platform", {}))
    n["runtime"] = info.get("runtime", n.get("runtime"))
    n["backend"] = info.get("backend", n.get("backend")) or backend_from_runtime(n.get("runtime"))
    remote_cid = info.get("bound_to") or info.get("controller_id") or ""
    n["remote_controller_id"] = remote_cid
    n["remote_controller_name"] = info.get("bound_to_name") or remote_cid
    n["remote_status_error"] = ""
    n["remote_status_updated_at"] = time.time()
    return n


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


async def _refresh_remote_unit_node(n):
    source_id = n.get("remote_source_node_id") or ""
    if not source_id:
        n["worker_running"] = False
        n["remote_status_error"] = "remote source node id is missing"
        n["remote_status_updated_at"] = time.time()
        return None
    try:
        info = await _remote_unit_node_request(n, "GET", f"/api/nodes/{quote(source_id, safe='')}")
        _apply_remote_unit_node_status(n, info)
        _log_event("remote_unit_node_refreshed",
                   node_id=n.get("id"), remote_unit_url=n.get("remote_unit_url"),
                   remote_source_node_id=source_id, worker_running=n.get("worker_running"),
                   remote_bound_to=n.get("remote_controller_id"),
                   desired_load=bool(n.get("desired_load")))
        return info
    except Exception as exc:
        n["worker_running"] = False
        n["remote_status_error"] = str(exc)
        n["remote_status_updated_at"] = time.time()
        _log_event("remote_unit_node_refresh_failed",
                   node_id=n.get("id"), remote_unit_url=n.get("remote_unit_url"),
                   remote_source_node_id=source_id, error=str(exc))
        return None


async def _start_remote_unit_worker(n):
    source_id = n.get("remote_source_node_id") or ""
    if not source_id:
        raise RuntimeError("remote source node id is missing")
    result = await _remote_unit_node_request(
        n, "POST", f"/api/nodes/{quote(source_id, safe='')}/worker/start", timeout=30)
    info = result.get("node", result)
    if isinstance(info, dict):
        _apply_remote_unit_node_status(n, info)
    _log_event("remote_unit_worker_start_requested",
               node_id=n.get("id"), remote_unit_url=n.get("remote_unit_url"),
               remote_source_node_id=source_id, worker_running=n.get("worker_running"))
    return result


async def _stop_remote_unit_worker(n, reason="requested"):
    source_id = n.get("remote_source_node_id") or ""
    if not source_id:
        raise RuntimeError("remote source node id is missing")
    result = await _remote_unit_node_request(
        n, "POST", f"/api/nodes/{quote(source_id, safe='')}/worker/stop",
        body={"reason": reason}, timeout=15)
    info = result.get("node", result)
    if isinstance(info, dict):
        _apply_remote_unit_node_status(n, info)
    _log_event("remote_unit_worker_stop_requested",
               node_id=n.get("id"), remote_unit_url=n.get("remote_unit_url"),
               remote_source_node_id=source_id, reason=reason,
               worker_running=n.get("worker_running"))
    return result


async def _remote_unit_node_ready_check(n, start=False):
    endpoint = _node_rpc_endpoint(n)
    check = {
        "node_id": n.get("id"),
        "node_name": n.get("name"),
        "remote_unit_url": n.get("remote_unit_url"),
        "remote_source_node_id": n.get("remote_source_node_id"),
        "endpoint": endpoint,
        "started": False,
        "worker_running": False,
        "tcp_reachable": False,
        "ready": False,
        "error": "",
    }
    await _refresh_remote_unit_node(n)
    if start and not n.get("worker_running"):
        try:
            await _start_remote_unit_worker(n)
            check["started"] = True
        except Exception as exc:
            check["error"] = str(exc)
            n["worker_running"] = False
            n["remote_status_error"] = str(exc)
            n["remote_status_updated_at"] = time.time()
    host, port = n.get("rpc_host"), n.get("rpc_port")
    check["worker_running"] = bool(n.get("worker_running"))
    check["tcp_reachable"] = await asyncio.to_thread(_tcp_reachable, host, port) if host and port else False
    remote_managed_agent = bool((n.get("capabilities") or {}).get("native_agent")) or \
        _node_backend(n).get("backend_kind") == "metal"
    # A native agent can lose its in-memory Popen handle when its service is
    # restarted while its already-listening RPC child remains alive.  The
    # source unit then reports worker_running=false even though it accepted
    # this start request and the data-plane endpoint is live.  For that narrow
    # managed-agent case, successful lifecycle control plus a fresh TCP probe
    # is the authoritative observation; do not reject a usable worker solely
    # because the agent cannot reconstruct the old child handle.
    if remote_managed_agent and check["started"] and check["tcp_reachable"]:
        check["worker_running"] = True
        n["worker_running"] = True
    ready_probe = await _wait_node_rpc_ready(n, timeout=20.0) if check["worker_running"] else {"ready": False}
    # The first TCP probe is intentionally immediate, so it can race a native
    # worker that has accepted the start request but is still binding its RPC
    # socket.  _wait_node_rpc_ready owns the authoritative final observation.
    check["worker_running"] = bool(ready_probe.get("worker_running", check["worker_running"]))
    check["tcp_reachable"] = bool(ready_probe.get("tcp_reachable", check["tcp_reachable"]))
    if remote_managed_agent and check["started"] and check["tcp_reachable"]:
        check["worker_running"] = True
        n["worker_running"] = True
    # A managed Metal agent writes its RPC log on the source macOS host.  The
    # importing unit cannot read that file, so treating a reachable TCP worker
    # as not-ready rejects valid distributed plans.  Its source-unit status
    # already confirms the worker is running; TCP is the data-plane proof.
    # The importing hub cannot read the native macOS worker log.  Its local
    # log probe therefore times out even after the source unit has confirmed
    # the worker and its TCP endpoint are live; do not turn that expected
    # observability gap into a data-plane failure.
    check["rpc_log_ready"] = bool(ready_probe.get("rpc_log_ready")) or \
        bool(remote_managed_agent and check["worker_running"] and check["tcp_reachable"])
    check["rpc_ready_probe"] = ready_probe
    if n.get("remote_status_error") and not check["error"]:
        check["error"] = n.get("remote_status_error")
    if remote_managed_agent and check["worker_running"] and check["tcp_reachable"]:
        check["error"] = ""
        # Source-unit logs are not an RPC readiness contract for a managed
        # agent.  Once its controller confirms the worker and the master can
        # establish TCP, ignore stale/unavailable log-reader errors.
        n["remote_status_error"] = ""
    check["ready"] = bool(check["worker_running"] and check["tcp_reachable"] and check["rpc_log_ready"] and not check["error"])
    _log_event("remote_unit_node_ready_check", **check)
    return check


async def _prepare_remote_unit_sessions(c, req, active):
    """Ask each capable unit to prepare all of its selected nodes as one session."""
    groups = {}
    for nid, placement in active:
        n = NODES.get(nid)
        if not n or n.get("kind") != "remote_unit_node":
            continue
        uid = n.get("remote_unit_id")
        unit = REMOTE_UNITS.get(uid, {})
        if "unit-load-session/v1" not in unit.get("unit_control_protocols", []):
            continue
        groups.setdefault(uid, []).append((nid, placement))
    sessions = {}
    for uid, members in groups.items():
        first = NODES[members[0][0]]
        session_id = f"uls-{_safe_slug(c.get('id'))}-{uuid.uuid4().hex[:12]}"
        body = {
            "protocol_version": "unit-load-session/v1", "session_id": session_id,
            "controller_id": c.get("id"), "model": req.model, "diagnostics": True,
            "nodes": [{"node_id": NODES[nid].get("remote_source_node_id"),
                       "layers": placement.get("layers"),
                       "planned_vram_gib": placement.get("vram_used_gib", 0.0),
                       "planned_ram_gib": placement.get("ram_used_gib", 0.0)}
                      for nid, placement in members],
        }
        try:
            response = await _remote_unit_node_request(
                first, "POST", "/api/unit/load-sessions/prepare", body=body, timeout=90)
            sessions[uid] = {"session_id": session_id, "unit_url": first.get("remote_unit_url"),
                             "response": response, "members": [nid for nid, _ in members]}
        except Exception as exc:
            sessions[uid] = {"session_id": session_id, "unit_url": first.get("remote_unit_url"),
                             "error": str(exc), "members": [nid for nid, _ in members]}
    c["unit_load_sessions"] = sessions
    return sessions


async def _release_remote_unit_sessions(c, reason):
    releases = []
    for item in (c.get("unit_load_sessions") or {}).values():
        if not item.get("response"):
            continue
        members = item.get("members") or []
        n = NODES.get(members[0]) if members else None
        if not n:
            continue
        try:
            result = await _remote_unit_node_request(
                n, "POST", f"/api/unit/load-sessions/{quote(item['session_id'], safe='')}/release",
                body={"reason": reason}, timeout=30)
            releases.append({"session_id": item["session_id"], "released": True, "result": result})
        except Exception as exc:
            releases.append({"session_id": item["session_id"], "released": False, "error": str(exc)})
    c["unit_load_sessions"] = {}
    return releases


async def _check_remote_unit_nodes_ready(c, req, active):
    checks = []
    topology = _rpc_topology(c, active)
    topology_by_node = {item["node_id"]: item for item in topology}
    unit_sessions = await _prepare_remote_unit_sessions(c, req, active)
    for nid, placement in active:
        n = NODES.get(nid)
        if not n or n.get("kind") != "remote_unit_node":
            continue
        session = unit_sessions.get(n.get("remote_unit_id"))
        session_node = ((session or {}).get("response") or {}).get("nodes", {}).get(n.get("remote_source_node_id"))
        if session_node is not None:
            check = {
                "node_id": nid, "node_name": n.get("name"), "remote_unit_url": n.get("remote_unit_url"),
                "remote_source_node_id": n.get("remote_source_node_id"), "endpoint": _node_rpc_endpoint(n),
                "session_id": session.get("session_id"), "protocol": "unit-load-session/v1",
                "started": bool(session_node.get("started_by_session")),
                "worker_running": bool(session_node.get("readiness", {}).get("worker_running")),
                "tcp_reachable": bool(session_node.get("readiness", {}).get("tcp_reachable")),
                "rpc_log_ready": bool(session_node.get("readiness", {}).get("rpc_log_ready")),
                "ready": bool(session_node.get("ready")), "error": session_node.get("error", ""),
                "unit_measurement": session_node,
            }
            # Older source-unit agents can retain a live native RPC child but
            # lose its Popen handle after an agent-service restart.  Their
            # unit-load-session result is then false solely because of that
            # stale handle.  Re-run the managed-agent control/TCP proof rather
            # than rejecting a reachable data-plane worker.
            remote_managed_agent = bool((n.get("capabilities") or {}).get("native_agent")) or \
                _node_backend(n).get("backend_kind") == "metal"
            if remote_managed_agent and not check["ready"]:
                fallback = await _remote_unit_node_ready_check(n, start=True)
                fallback.update({
                    "session_id": session.get("session_id"),
                    "protocol": "unit-load-session/v1+managed-agent-tcp-fallback",
                    "unit_measurement": session_node,
                })
                check = fallback
        elif session and session.get("error"):
            check = {"node_id": nid, "node_name": n.get("name"), "remote_unit_url": n.get("remote_unit_url"),
                     "remote_source_node_id": n.get("remote_source_node_id"), "endpoint": _node_rpc_endpoint(n),
                     "session_id": session.get("session_id"), "protocol": "unit-load-session/v1",
                     "ready": False, "error": session["error"]}
        else:
            check = await _remote_unit_node_ready_check(n, start=True)
        topo = topology_by_node.get(nid, {})
        check["placement"] = {
            "layers": placement.get("layers"),
            "n_layers": placement.get("n_layers"),
            "vram_used_gib": placement.get("vram_used_gib"),
            "ram_used_gib": placement.get("ram_used_gib"),
            "position": topo.get("position"),
            "is_first": topo.get("is_first"),
            "is_last": topo.get("is_last"),
            "rpc_endpoint": topo.get("rpc_endpoint"),
            "remote_source_rpc_endpoint": topo.get("remote_source_rpc_endpoint"),
        }
        checks.append(check)
    if checks:
        _log_event("remote_unit_load_preflight",
                   controller_id=c.get("id"), model=req.model, checks=checks)
    return checks


def _remote_unit_block_message(checks):
    bad = [c for c in checks if not c.get("ready")]
    parts = []
    for item in bad:
        reason = item.get("error") or (
            "RPC TCP endpoint is not reachable" if not item.get("tcp_reachable")
            else "remote worker is not running")
        parts.append(f"{item.get('node_name') or item.get('node_id')} at {item.get('endpoint')}: {reason}")
    return "remote unit RPC worker not ready: " + "; ".join(parts)


class CreateNode(BaseModel):
    slot_id: str = ""
    gpu_uuid: str = ""
    name: str = ""
    vram: float = 0
    ram: float = 0
    cores: int = 0


class LinkAgentNode(BaseModel):
    agent_url: str
    name: str = ""
    vram_budget_gib: Optional[float] = None
    ram_budget_gib: Optional[float] = None
    cores_budget: Optional[int] = None


class MetalSlotResources(BaseModel):
    ram_budget_gib: float
    cores_budget: int
    # The UI includes this so a restarted hub can repair the one fixed Metal
    # slot when the native agent still remembers a controller ID from before
    # the hub state was recreated.
    node_id: Optional[str] = None


class RegisterRemoteUnit(BaseModel):
    unit_url: str = ""
    base_url: str = ""
    name: str = ""


class RemoteNodeClaim(BaseModel):
    controller_id: str
    controller_name: str = ""
    report_url: str = ""


class WorkerStopRequest(BaseModel):
    reason: str = "requested"


@app.get("/api/gpus")
def api_gpus():
    return {"gpus": list_gpus(), "system": local_resource_limits()}


@app.get("/api/nodes")
def api_nodes():
    _ensure_local_slots()
    # A remote unit is attached to one controller, not to this unit-wide node
    # pool.  Keep its projected nodes out of the global/sidebar surface.
    ordered = sorted((n for n in NODES.values() if n.get("kind") != "remote_unit_node"),
                     key=_node_sort_key)
    local_used = sum(1 for n in _local_slots() if _slot_configured(n))
    return {"nodes": [node_view(n) for n in ordered],
            "used": local_used, "total": len(ordered), "max": MAX_NODES}


@app.post("/api/nodes")
def api_create_node(c: CreateNode):
    _ensure_local_slots()
    gpus = {g["uuid"]: g for g in list_gpus()}
    if c.gpu_uuid not in gpus:
        raise HTTPException(400, f"unknown gpu {c.gpu_uuid}")
    if c.slot_id:
        n = NODES.get(c.slot_id)
        if not n or n.get("kind", "local") != "local":
            raise HTTPException(404, "unknown local node slot")
    else:
        n = next((slot for slot in _local_slots() if not _slot_configured(slot)), None)
        if not n:
            raise HTTPException(409, f"all {MAX_NODES} local node slots are assigned")
    if n.get("bound_to"):
        raise HTTPException(409, "unbind the node before changing resources")
    _kill(n.get("worker"))
    gpu_name = gpus[c.gpu_uuid]["name"]
    slot = int(n.get("slot") or 1)
    n.update({"name": c.name or f"{_short_gpu_name(gpu_name)}-{slot}",
              "gpu_uuid": c.gpu_uuid, "gpu_name": gpu_name, "assigned": True,
              "vram": c.vram or gpus[c.gpu_uuid]["vram_total_gib"], "ram": c.ram,
              "cores": c.cores, "worker": None, "ram_used": 0.0})
    _touch_log(n["log"])
    _persist_local_slots()
    return node_view(n)


def _parse_remote_unit_ref(req):
    raw = (req.unit_url or req.base_url or "").strip().rstrip("/")
    if not raw:
        raise HTTPException(400, "unit_url is required")
    raw = re.sub(r"^(https?);/+", r"\1://", raw, flags=re.IGNORECASE)
    if "://" not in raw:
        raw = "http://" + raw
    parsed = urlparse(raw)
    netloc = parsed.netloc
    if ";" in netloc and ":" not in netloc.rsplit("@", 1)[-1]:
        netloc = netloc.replace(";", ":", 1)
        parsed = urlparse(f"{parsed.scheme}://{netloc}")
    if parsed.scheme not in ("http", "https") or not parsed.netloc or not parsed.hostname:
        raise HTTPException(400, "unit_url must be an http(s) URL")
    try:
        port = parsed.port
    except ValueError:
        raise HTTPException(400, "unit_url port must be a number")
    host = parsed.hostname
    if ":" in host and not host.startswith("["):
        host = f"[{host}]"
    return f"{parsed.scheme}://{host}{f':{port}' if port else ''}"


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


def _remote_node_id(uid, source_node_id):
    safe = re.sub(r"[^A-Za-z0-9_.-]+", "-", source_node_id or uuid.uuid4().hex[:8]).strip("-")
    return f"runit-{uid}-{safe}"[:96]


def _remote_node_endpoint(nd):
    endpoint = nd.get("rpc_endpoint") or ""
    if endpoint:
        try:
            return _parse_endpoint(endpoint)
        except HTTPException:
            pass
    host = nd.get("rpc_host") or nd.get("host") or ""
    port = nd.get("rpc_port") or nd.get("port")
    if host and port:
        return str(host), int(port)
    raise HTTPException(400, f"remote unit node {nd.get('id', '')} has no rpc endpoint")


def _upsert_remote_unit_nodes(uid, remote_info, base_url, display_name="", owner_controller_id=""):
    name = display_name or remote_info.get("name") or urlparse(base_url).netloc
    existing = REMOTE_UNITS.get(uid, {})
    old_node_ids = set(existing.get("node_ids", []))
    incoming = remote_info.get("nodes") or []
    controllers = remote_info.get("controllers") or []
    unit_runtime = remote_info.get("runtime")
    unit_backend = remote_info.get("backend") or backend_from_runtime(unit_runtime)
    ctrl_names = {c.get("id"): c.get("name") or c.get("id") for c in controllers}
    node_ids = []

    for nd in incoming:
        if not _remote_unit_source_node_configured(nd):
            continue
        host, port = _remote_node_endpoint(nd)
        advertised_endpoint = f"{host}:{port}"
        host = _remote_unit_rpc_host(base_url)
        source_id = nd.get("id") or f"{host}:{port}"
        nid = _remote_node_id(uid, source_id)
        previous = NODES.get(nid, {})
        remote_cid = nd.get("bound_to") or nd.get("controller_id") or ""
        remote_cname = nd.get("bound_to_name") or ctrl_names.get(remote_cid, remote_cid)
        NODES[nid] = {
            "id": nid,
            "kind": "remote_unit_node",
            "name": nd.get("name") or source_id,
            "gpu_uuid": nd.get("gpu_uuid", ""),
            "gpu_name": nd.get("gpu_name") or nd.get("gpu") or "Remote GPU",
            "vram": nd.get("vram", nd.get("vram_budget_gib", 0.0)),
            "ram": nd.get("ram", nd.get("ram_budget_gib", 0.0)),
            "cores": nd.get("cores", nd.get("cores_budget", 0)),
            "rpc_host": host,
            "rpc_port": port,
            "bound_to": previous.get("bound_to"),
            "worker": None,
            "worker_running": bool(nd.get("worker_running")),
            "ram_used": nd.get("ram_used_gib", 0.0),
            "log": "",
            "resources": nd.get("resources", {}),
            "capabilities": nd.get("capabilities", {}),
            "host_platform": nd.get("host_platform", {}),
            "runtime": nd.get("runtime") or unit_runtime,
            "backend": nd.get("backend") or backend_from_runtime(nd.get("runtime")) or unit_backend,
            "remote_unit_id": uid,
            "remote_unit_name": name,
            "remote_unit_url": base_url,
            "owner_controller_id": owner_controller_id or existing.get("owner_controller_id"),
            "remote_controller_id": remote_cid,
            "remote_controller_name": remote_cname,
            "remote_status_error": nd.get("remote_status_error", ""),
            "remote_status_updated_at": time.time(),
            "remote_source_node_id": source_id,
            "remote_source_rpc_endpoint": advertised_endpoint,
        }
        node_ids.append(nid)

    for nid in old_node_ids - set(node_ids):
        removed = NODES.pop(nid, None)
        if removed and removed.get("bound_to") in CTRLS:
            CTRLS[removed["bound_to"]]["nodes"] = [x for x in CTRLS[removed["bound_to"]]["nodes"] if x != nid]

    REMOTE_UNITS[uid] = {
        "id": uid,
        "name": name,
        "base_url": base_url,
        "runtime": unit_runtime,
        "backend": unit_backend,
        "owner_controller_id": owner_controller_id or existing.get("owner_controller_id"),
        "controllers": controllers,
        "unit_control_protocols": remote_info.get("unit_control_protocols", []),
        "node_ids": node_ids,
        "updated_at": time.time(),
    }
    return REMOTE_UNITS[uid]


async def _fetch_remote_unit_nodes(base_url):
    async with httpx.AsyncClient(timeout=15, headers=_service_headers()) as client:
        runtime = None
        backend = None
        try:
            runtime_resp = await client.get(f"{base_url}/api/runtime")
            runtime_resp.raise_for_status()
            runtime_info = runtime_resp.json()
            runtime = runtime_info.get("runtime")
            backend = runtime_info.get("backend") or backend_from_runtime(runtime)
            unit_control_protocols = runtime_info.get("unit_control_protocols", [])
        except Exception:
            runtime = None
            backend = None
            unit_control_protocols = []
        ctrls = await client.get(f"{base_url}/api/controllers")
        ctrls.raise_for_status()
        controllers = ctrls.json().get("controllers", [])
        nodes = await client.get(f"{base_url}/api/nodes")
        nodes.raise_for_status()
        all_nodes = nodes.json().get("nodes", [])
        return {"controllers": controllers, "nodes": all_nodes, "runtime": runtime, "backend": backend,
                "unit_control_protocols": unit_control_protocols}


def _controller_remote_units(cid):
    return [r for r in REMOTE_UNITS.values() if r.get("owner_controller_id") == cid]


def _delete_remote_unit(uid):
    r = REMOTE_UNITS.pop(uid, None)
    if not r:
        raise HTTPException(404, "unknown remote unit")
    for nid in list(r.get("node_ids", [])):
        n = NODES.pop(nid, None)
        if n and n.get("bound_to") in CTRLS:
            CTRLS[n["bound_to"]]["nodes"] = [x for x in CTRLS[n["bound_to"]]["nodes"] if x != nid]
    return r


@app.get("/api/controllers/{cid}/remote-units")
def api_controller_remote_units(cid: str):
    if cid not in CTRLS:
        raise HTTPException(404, "unknown controller")
    return {"remote_units": [_remote_unit_view(r) for r in _controller_remote_units(cid)]}


@app.post("/api/controllers/{cid}/remote-units")
async def api_register_controller_remote_unit(cid: str, req: RegisterRemoteUnit):
    if cid not in CTRLS:
        raise HTTPException(404, "unknown controller")
    base_url = _parse_remote_unit_ref(req)
    remote_info = await _fetch_remote_unit_nodes(base_url)
    uid = "unit-" + uuid.uuid5(uuid.NAMESPACE_URL, f"{cid}:{base_url}").hex[:10]
    r = _upsert_remote_unit_nodes(uid, remote_info, base_url, req.name, cid)
    _persist_hub_state()
    return {"remote_unit": _remote_unit_view(r),
            "nodes": [node_view(NODES[nid]) for nid in r.get("node_ids", []) if nid in NODES]}


@app.post("/api/controllers/{cid}/remote-units/{uid}/refresh")
async def api_refresh_controller_remote_unit(cid: str, uid: str):
    r = REMOTE_UNITS.get(uid)
    if not r or r.get("owner_controller_id") != cid:
        raise HTTPException(404, "unknown remote unit")
    remote_info = await _fetch_remote_unit_nodes(r["base_url"])
    r = _upsert_remote_unit_nodes(uid, remote_info, r["base_url"], r["name"], cid)
    _persist_hub_state()
    return {"remote_unit": _remote_unit_view(r),
            "nodes": [node_view(NODES[nid]) for nid in r.get("node_ids", []) if nid in NODES]}


@app.delete("/api/controllers/{cid}/remote-units/{uid}")
def api_delete_controller_remote_unit(cid: str, uid: str):
    r = REMOTE_UNITS.get(uid)
    if not r or r.get("owner_controller_id") != cid:
        raise HTTPException(404, "unknown remote unit")
    _delete_remote_unit(uid)
    _persist_hub_state()
    return {"removed": uid}


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


async def _create_agent_node(cid, req, report_url):
    agent_url = req.agent_url.rstrip("/")
    parsed = urlparse(agent_url)
    if parsed.scheme not in ("http", "https") or not parsed.netloc:
        raise HTTPException(400, "agent_url must be an http(s) URL")
    async with httpx.AsyncClient(timeout=15) as client:
        try:
            r = await client.post(agent_url + "/control/join",
                                  json={"controller_id": cid, "report_url": report_url, "name": req.name,
                                        "vram_budget_gib": req.vram_budget_gib,
                                        "ram_budget_gib": req.ram_budget_gib,
                                        "cores_budget": req.cores_budget})
            r.raise_for_status()
            info = r.json()
        except httpx.HTTPStatusError as exc:
            if exc.response.status_code == 409:
                raise HTTPException(409, exc.response.json())
            raise
    runtime = info.get("runtime")
    runtime_check = compatibility_report(runtime)
    if not runtime_check["compatible"]:
        try:
            await _agent_request({"agent_url": agent_url}, "POST", "/unbind", timeout=10)
        except Exception:
            pass
        raise HTTPException(409, _runtime_error_message(runtime_check))
    resources = info.get("resources") or {}
    node_id = info.get("node_id") or "agent-" + uuid.uuid4().hex[:8]
    if node_id not in NODES and _owned_node_count() >= MAX_NODES:
        try:
            await _agent_request({"agent_url": agent_url}, "POST", "/unbind", timeout=10)
        except Exception:
            pass
        raise HTTPException(409, f"unit node limit reached ({MAX_NODES})")
    host = parsed.hostname or "127.0.0.1"
    backend = info.get("backend") or backend_from_runtime(runtime)
    existing = NODES.get(node_id) or {}
    n = {"id": node_id, "kind": "agent", "name": req.name or info.get("name") or node_id,
         # macOS exposes one Metal device, so it is always the unit's Slot 1.
         # Do not let historical/stale agent records shift it to Slot 2+.
         "logical_slot": 1 if (backend or {}).get("backend_kind") == "metal"
         else (existing.get("logical_slot") or _next_logical_slot()),
         "agent_url": agent_url, "report_url": report_url, "gpu_uuid": info.get("gpu_uuid", ""),
         "gpu_name": info.get("gpu", "Managed node"),
         "vram": resources.get("vram_budget_gib", info.get("vram_budget_gib", 0.0)),
         "ram": resources.get("ram_budget_gib", info.get("ram_budget_gib", 0.0)),
         "cores": resources.get("cores_budget", info.get("cores", 0)),
         "rpc_host": host, "rpc_port": int(info.get("rpc_port", 50052)),
         "bound_to": cid, "worker": None, "worker_running": bool(info.get("worker_running")),
         "ram_used": resources.get("ram_used_gib", 0.0), "log": "",
         "resources": resources, "operations": info.get("operations", []),
         "models": info.get("models", {}), "last_report": None,
         "desired_load": info.get("desired_load"),
         "capabilities": info.get("capabilities", {}),
         "host_platform": info.get("host_platform", {}),
         "owner": (info.get("owner") or "").strip(),
         "runtime": runtime,
         "backend": backend}
    NODES[node_id] = n
    return n


def _apply_agent_info(n, info):
    resources = info.get("resources") or {}
    n["resources"] = resources
    n["operations"] = info.get("operations", [])
    n["models"] = info.get("models", {})
    n["desired_load"] = info.get("desired_load")
    n["worker_running"] = bool(info.get("worker_running"))
    n["ram_used"] = resources.get("ram_used_gib", n.get("ram_used", 0.0))
    n["vram"] = resources.get("vram_budget_gib", n.get("vram", 0.0))
    n["ram"] = resources.get("ram_budget_gib", n.get("ram", 0.0))
    n["cores"] = resources.get("cores_budget", n.get("cores", 0))
    n["capabilities"] = info.get("capabilities", n.get("capabilities", {}))
    n["host_platform"] = info.get("host_platform", n.get("host_platform", {}))
    n["owner"] = (info.get("owner") or n.get("owner") or "").strip()
    n["runtime"] = info.get("runtime", n.get("runtime"))
    n["backend"] = info.get("backend", n.get("backend")) or backend_from_runtime(n.get("runtime"))
    return n


async def _refresh_agent_node(n):
    try:
        info = await _agent_request(n, "GET", "/control/status", timeout=10)
    except Exception:
        n["worker_running"] = False
        return None
    _apply_agent_info(n, info)
    return info


@app.get("/api/nodes/{nid}")
async def api_node_detail(nid: str):
    _ensure_local_slots()
    n = NODES.get(nid)
    if not n:
        raise HTTPException(404, "unknown node")
    if n.get("kind") == "agent":
        await _refresh_agent_node(n)
    elif n.get("kind") == "remote_unit_node":
        await _refresh_remote_unit_node(n)
    v = node_view(n)
    v["log"] = "" if n.get("kind") == "agent" else tail(n["log"])
    return v


@app.get("/api/nodes/{nid}/logs")
async def api_node_logs(nid: str, tail_lines: int = Query(500, alias="tail", ge=20, le=10000)):
    _ensure_local_slots()
    n = NODES.get(nid)
    if not n:
        raise HTTPException(404, "unknown node")
    if n.get("kind") == "agent":
        info = await _refresh_agent_node(n)
        worker_log = {}
        try:
            worker_log = await _agent_request(n, "GET", f"/control/logs?tail={tail_lines}", timeout=10)
        except Exception as exc:
            worker_log = {"error": str(exc)}
        reports = (info or {}).get("last_reports", []) if info else []
        report_log = "\n".join(
            f"{r.get('seq', '')} {r.get('op_type', '')} {r.get('phase', '')} "
            f"{r.get('status', '')} {r.get('progress', 0)}% {r.get('message', '')}"
            for r in reports[-100:])
        resources = n.get("resources") or {}
        return {"log": worker_log.get("log") or report_log or "managed node agent status unavailable\n",
                "agent_reports": report_log,
                "rpc_activity": worker_log.get("rpc_activity", {}),
                "worker_pid": worker_log.get("worker_pid"),
                "desired_load": worker_log.get("desired_load"),
                "diagnostic_error": worker_log.get("error", ""),
                "worker_running": _node_worker_running(n),
                "vram_used_gib": worker_log.get("vram_used_gib", resources.get("vram_used_gib", 0.0)),
                "vram": n["vram"],
                "ram_used_gib": worker_log.get("ram_used_gib", resources.get("ram_used_gib", n.get("ram_used", 0.0))),
                "ram": n["ram"],
                "operations": n.get("operations", []),
                "resources": resources}
    if n.get("kind") == "remote_unit_node":
        await _refresh_remote_unit_node(n)
        log = ""
        try:
            source_id = n.get("remote_source_node_id") or ""
            remote_logs = await _remote_unit_node_request(
                n, "GET", f"/api/nodes/{quote(source_id, safe='')}/logs?tail={tail_lines}", timeout=10)
            log = remote_logs.get("log", "") if isinstance(remote_logs, dict) else ""
        except Exception as exc:
            log = f"remote unit log unavailable: {exc}\n"
        return {"log": log,
                "worker_running": _node_worker_running(n),
                "vram_used_gib": 0.0,
                "vram": n["vram"],
                "ram_used_gib": n.get("ram_used", 0.0),
                "ram": n["ram"],
                "remote_status_error": n.get("remote_status_error", ""),
                "remote_status_updated_at": n.get("remote_status_updated_at")}
    if n.get("kind") == "remote":
        host, port = n["rpc_host"], n["rpc_port"]
        reachable = _tcp_reachable(host, port)
        return {"log": f"remote RPC endpoint: {host}:{port}\nreachable: {str(reachable).lower()}\n",
                "worker_running": reachable,
                "vram_used_gib": 0.0, "vram": n["vram"],
                "ram_used_gib": n.get("ram_used", 0.0), "ram": n["ram"]}
    return {"log": tail(n["log"], tail_lines),
            "worker_running": _node_worker_running(n),
            "vram_used_gib": gpu_used_gib(n["gpu_uuid"]) if n.get("gpu_uuid") else 0.0,
            "vram": n["vram"], "ram_used_gib": n.get("ram_used", 0.0), "ram": n["ram"]}


@app.get("/api/nodes/{nid}/status")
async def api_node_status(nid: str):
    _ensure_local_slots()
    n = NODES.get(nid)
    if not n:
        raise HTTPException(404, "unknown node")
    if n.get("kind") == "agent":
        info = await _refresh_agent_node(n)
        return info or {"error": "agent unavailable", "node": node_view(n)}
    if n.get("kind") == "remote_unit_node":
        await _refresh_remote_unit_node(n)
    return node_view(n)


async def _unit_session_prepare_node(n, request):
    """Have the owning unit prove worker readiness without using the RPC port as control API."""
    was_running = _node_worker_running(n)
    if n.get("kind") == "agent":
        if not was_running:
            info = await _agent_request(n, "POST", "/start_worker", {}, timeout=30)
            _apply_agent_info(n, info)
    elif n.get("kind") == "local":
        _start_node_worker(n)
        n["worker_running"] = _node_worker_running(n)
    else:
        raise HTTPException(409, "unit load sessions can only own local or managed-agent nodes")
    ready = await _wait_node_rpc_ready(n, timeout=60.0)
    payload = await _load_monitor_log(n)
    resources = _node_resource_report(n, payload)
    return {
        "node_id": n["id"], "rpc_endpoint": _node_rpc_endpoint(n),
        "started_by_session": not was_running and bool(ready.get("worker_running")),
        "ready": bool(ready.get("ready")), "readiness": ready,
        "resources": model_to_dict(resources),
        "rpc_activity": _rpc_activity_metrics(payload.get("log", "")),
        "worker_log_tail": str(payload.get("log") or "")[-16000:],
        "planned_layers": request.layers,
        "planned_vram_gib": request.planned_vram_gib,
        "planned_ram_gib": request.planned_ram_gib,
    }


@app.post("/api/unit/load-sessions/prepare")
async def api_unit_load_session_prepare(req: UnitLoadSessionPrepareRequest):
    """Prepare a unit-owned distributed-load session and return measured readiness.

    This is the only controller-to-unit readiness contract.  The controller
    later uses the returned RPC endpoint strictly for llama.cpp data-plane
    traffic, while lifecycle and diagnostics continue through this session.
    """
    _ensure_local_slots()
    existing = UNIT_LOAD_SESSIONS.get(req.session_id)
    if existing and existing.get("controller_id") != req.controller_id:
        raise HTTPException(409, "unit load session belongs to another controller")
    session = existing or {
        "session_id": req.session_id, "controller_id": req.controller_id,
        "model": req.model, "protocol_version": req.protocol_version,
        "created_at": time.time(), "nodes": {}, "phase": "preparing",
    }
    session.update({"updated_at": time.time(), "phase": "preparing", "diagnostics": req.diagnostics})
    UNIT_LOAD_SESSIONS[req.session_id] = session
    for item in req.nodes:
        n = NODES.get(item.node_id)
        if not n:
            session["nodes"][item.node_id] = {"node_id": item.node_id, "ready": False, "error": "unknown node"}
            continue
        try:
            session["nodes"][item.node_id] = await _unit_session_prepare_node(n, item)
        except Exception as exc:
            session["nodes"][item.node_id] = {"node_id": item.node_id, "ready": False, "error": str(exc)}
    session["updated_at"] = time.time()
    session["phase"] = "ready" if all(node.get("ready") for node in session["nodes"].values()) else "error"
    _log_event("unit_load_session_prepared", session_id=req.session_id, controller_id=req.controller_id,
               model=req.model, phase=session["phase"], nodes=session["nodes"])
    return session


@app.get("/api/unit/load-sessions/{session_id}")
async def api_unit_load_session_status(session_id: str):
    session = UNIT_LOAD_SESSIONS.get(session_id)
    if not session:
        raise HTTPException(404, "unknown unit load session")
    for node_id, status in session.get("nodes", {}).items():
        n = NODES.get(node_id)
        if not n:
            continue
        payload = await _load_monitor_log(n)
        status["resources"] = model_to_dict(_node_resource_report(n, payload))
        status["rpc_activity"] = _rpc_activity_metrics(payload.get("log", ""))
        status["worker_log_tail"] = str(payload.get("log") or "")[-16000:]
        status["worker_running"] = _node_worker_running(n)
        status["sampled_at"] = time.time()
    session["updated_at"] = time.time()
    return session


@app.post("/api/unit/load-sessions/{session_id}/release")
async def api_unit_load_session_release(session_id: str, req: UnitLoadSessionReleaseRequest):
    session = UNIT_LOAD_SESSIONS.get(session_id)
    if not session:
        raise HTTPException(404, "unknown unit load session")
    releases = []
    for node_id, status in session.get("nodes", {}).items():
        n = NODES.get(node_id)
        if not n or not status.get("started_by_session"):
            continue
        try:
            if n.get("kind") == "agent":
                await _stop_agent_worker_confirmed(n, "/control/unload", {"reason": req.reason})
            else:
                _kill(n.get("worker")); n["worker"] = None; n["worker_running"] = False
            releases.append({"node_id": node_id, "released": True})
        except Exception as exc:
            releases.append({"node_id": node_id, "released": False, "error": str(exc)})
    session.update({"phase": "released", "released_at": time.time(), "release_reason": req.reason,
                    "release": releases})
    _log_event("unit_load_session_released", session_id=session_id, reason=req.reason, release=releases)
    return session


@app.post("/api/nodes/{nid}/remote-claim")
async def api_node_remote_claim(nid: str, claim: RemoteNodeClaim):
    """Reserve a locally-owned node for a controller in another unit.

    A remote unit imports this node as a proxy, but the owning unit remains the
    source of truth for exclusivity and for starting the native worker.
    """
    _ensure_local_slots()
    n = NODES.get(nid)
    if not n or n.get("kind") == "remote_unit_node":
        raise HTTPException(404, "unknown locally-owned node")
    if not _slot_configured(n):
        raise HTTPException(409, "assign node resources before binding")
    if n.get("bound_to") and n["bound_to"] != claim.controller_id:
        raise HTTPException(409, f"node already bound to {n['bound_to']}")
    if n.get("kind") == "agent":
        info = await _agent_request(n, "POST", "/control/join", {
            "controller_id": claim.controller_id,
            "report_url": claim.report_url,
            "name": n.get("name") or "Managed node",
            "vram_budget_gib": n.get("vram"),
            "ram_budget_gib": n.get("ram"),
            "cores_budget": n.get("cores"),
        })
        _apply_agent_info(n, info)
    n["bound_to"] = claim.controller_id
    n["bound_to_name"] = claim.controller_name or claim.controller_id
    _persist_hub_state()
    return {"node": node_view(n)}


@app.post("/api/nodes/{nid}/remote-release")
async def api_node_remote_release(nid: str, claim: RemoteNodeClaim):
    _ensure_local_slots()
    n = NODES.get(nid)
    if not n or n.get("kind") == "remote_unit_node":
        raise HTTPException(404, "unknown locally-owned node")
    if n.get("bound_to") != claim.controller_id:
        raise HTTPException(409, "node is not bound to this remote controller")
    if n.get("kind") == "agent":
        try:
            await _unbind_agent_confirmed(n)
        except Exception as exc:
            raise HTTPException(409, f"node resources were not released: {exc}") from exc
    else:
        _kill(n.get("worker")); n["worker"] = None
        n["worker_running"] = False
        n["desired_load"] = None
    n["bound_to"] = None
    n["bound_to_name"] = None
    _persist_hub_state()
    return {"node": node_view(n)}


@app.post("/api/nodes/{nid}/worker/start")
async def api_node_worker_start(nid: str):
    _ensure_local_slots()
    n = NODES.get(nid)
    if not n:
        raise HTTPException(404, "unknown node")
    if n.get("kind") == "agent":
        if not n.get("bound_to"):
            raise HTTPException(409, "bind the managed node before starting a worker")
        info = await _agent_request(n, "POST", "/start_worker", {})
        _apply_agent_info(n, info)
        return {"node": node_view(n), "log": "managed worker started"}
    if n.get("kind", "local") != "local":
        raise HTTPException(409, "only locally-owned nodes can start a worker")
    if not _slot_configured(n):
        raise HTTPException(409, "assign node resources before starting the worker")
    if n.get("bound_to") in CTRLS:
        raise HTTPException(409, "unbind the node before remote worker start")
    _start_node_worker(n)
    n["worker_running"] = _node_worker_running(n)
    _log_event("node_worker_start_api", node_id=nid, worker_running=n["worker_running"],
               rpc_endpoint=_node_rpc_endpoint(n))
    return {"node": node_view(n), "log": tail(n["log"])}


@app.post("/api/nodes/{nid}/worker/stop")
async def api_node_worker_stop(nid: str, req: WorkerStopRequest = None):
    _ensure_local_slots()
    n = NODES.get(nid)
    if not n:
        raise HTTPException(404, "unknown node")
    if n.get("kind") == "agent":
        await _stop_agent_worker_confirmed(
            n, "/control/unload", model_to_dict(UnloadRequest(reason="remote_worker_stop")))
        return {"node": node_view(n), "log": "managed worker stopped"}
    if n.get("kind", "local") != "local":
        raise HTTPException(409, "only locally-owned nodes can stop a worker")
    if n.get("bound_to") in CTRLS:
        raise HTTPException(409, "unbind the node before remote worker stop")
    req = req or WorkerStopRequest()
    _kill(n.get("worker"))
    n["worker"] = None
    n["worker_running"] = False
    n["desired_load"] = None
    n["ram_used"] = 0.0
    _log_event("node_worker_stop_api", node_id=nid, reason=req.reason,
               rpc_endpoint=_node_rpc_endpoint(n))
    return {"node": node_view(n), "log": tail(n["log"])}


@app.delete("/api/nodes/{nid}")
async def api_del_node(nid: str):
    _ensure_local_slots()
    n = NODES.get(nid)
    if not n:
        raise HTTPException(404, "unknown node")
    if n.get("kind") == "remote_unit_node":
        raise HTTPException(409, "delete the registered remote unit instead")
    if n.get("kind", "local") == "local":
        if n.get("bound_to"):
            raise HTTPException(409, "unbind the node before clearing resources")
        _reset_local_slot(n)
        _persist_local_slots()
        return {"cleared": nid}
    n = NODES.pop(nid)
    if n["bound_to"] in CTRLS:
        CTRLS[n["bound_to"]]["nodes"] = [x for x in CTRLS[n["bound_to"]]["nodes"] if x != nid]
    if n.get("kind", "local") == "local":
        _kill(n.get("worker"))
    elif n.get("kind") == "agent":
        try:
            await _agent_request(n, "POST", "/control/unload", model_to_dict(UnloadRequest(reason="remove_node")))
        except Exception:
            pass
    _persist_hub_state()
    return {"removed": nid}


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


@app.get("/api/models")
def api_models():
    return {"dir": MODEL_DIR, "models": _scan_models(), "downloads": DL}


SHARD_TARGET_REPLICAS = int(os.environ.get("LINKCPP_SHARD_TARGET_REPLICAS", "2") or 2)

# Stage configs published for self-enrolled NAT nodes to pull and self-start,
# since the hub cannot push a stage-start to a node it can only reach outbound.
# node_id -> {controller_id, config, coordinator_endpoint}
SELF_START_CONFIGS: dict = {}
_RELAY_SEQ = 0


def _shard_coverage():
    """Roll up which layer segments of each model are currently covered by live
    ring stages, so nodes can see where coverage is thin and choose to fill it.
    Per model: a per-layer replica count folded into contiguous segments, each
    with a scarcity score (0 = fully covered, 1 = uncovered)."""
    per_model = {}   # model -> {"n_layer": int, "coverage": [int]*n_layer}
    for c in CTRLS.values():
        plan = c.get("plan") or {}
        if normalize_runtime_mode(plan.get("runtime_mode")) != RING_PROXY:
            continue
        model = plan.get("model_ref") or c.get("model") or c.get("serving")
        n_layer = int((plan.get("model") or {}).get("n_layer") or 0)
        if not model or n_layer <= 0:
            continue
        entry = per_model.setdefault(model, {"n_layer": n_layer, "coverage": [0] * n_layer})
        for p in plan.get("placement", []):
            window = p.get("layers") or []
            node = NODES.get(p.get("node_id")) or {}
            if len(window) != 2 or not node.get("worker_running"):
                continue
            start, end = int(window[0]), int(window[1])
            for i in range(max(0, start), min(entry["n_layer"], end)):
                entry["coverage"][i] += 1
    out = []
    for model, e in per_model.items():
        cov = e["coverage"]
        segments, i = [], 0
        while i < len(cov):
            j = i
            while j < len(cov) and cov[j] == cov[i]:
                j += 1
            replicas = cov[i]
            segments.append({
                "layers": [i, j], "replicas": replicas,
                "target": SHARD_TARGET_REPLICAS,
                "scarcity": round(max(0, SHARD_TARGET_REPLICAS - replicas) / SHARD_TARGET_REPLICAS, 3),
            })
            i = j
        out.append({"model": model, "n_layer": e["n_layer"],
                    "target_replicas": SHARD_TARGET_REPLICAS, "segments": segments})
    return out


@app.get("/api/shard-demand")
def api_shard_demand(model: str = Query("")):
    """Live shard coverage/demand map: for each model, which contiguous layer
    segments are under-replicated (high scarcity). A node polls this to choose a
    segment to serve — the scarce ones are where its contribution counts most."""
    demand = _shard_coverage()
    if model:
        demand = [d for d in demand if d["model"] == model or os.path.basename(d["model"]) == model]
    return {"target_replicas": SHARD_TARGET_REPLICAS, "models": demand}


def _recommend_segment(model="", max_layers=0):
    """Pick the segment a volunteering node should serve: the scarcest (most
    under-covered) window across the demand map, clipped to the node's layer
    budget. Returns None when nothing is under target. This is the bottom-up
    matchmaking a node uses instead of waiting to be force-placed."""
    best = None
    for entry in _shard_coverage():
        if model and os.path.basename(entry["model"]) != os.path.basename(model):
            continue
        for seg in entry["segments"]:
            if seg["scarcity"] <= 0:
                continue
            start, end = seg["layers"]
            if max_layers and (end - start) > max_layers:
                end = start + max_layers   # take a coverable sub-window of the gap
            cand = {"model": entry["model"], "n_layer": entry["n_layer"],
                    "layers": [start, end], "scarcity": seg["scarcity"],
                    "replicas": seg["replicas"], "target": seg["target"]}
            key = (seg["scarcity"], end - start)
            if best is None or key > best[0]:
                best = (key, cand)
    return best[1] if best else None


class ShardVolunteer(BaseModel):
    node_id: str = ""
    model: str = ""
    max_layers: int = 0


@app.post("/api/shard-volunteer")
def api_shard_volunteer(req: ShardVolunteer):
    """A node offers to serve a shard; the hub replies with the scarcest segment
    it should take (model + layer window), or none if coverage is already at
    target. The node then downloads just that window (partial shard) and serves
    it — bottom-up participation driven by where reward is highest (scarcity)."""
    seg = _recommend_segment(req.model, req.max_layers)
    if not seg:
        return {"assigned": False, "reason": "no under-covered segment"}
    _log_event("shard_volunteer_assigned", node_id=req.node_id, model=seg["model"],
               layers=seg["layers"], scarcity=seg["scarcity"])
    return {"assigned": True, **seg}


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


def _node_can_coordinate(nid) -> bool:
    n = NODES.get(nid) or {}
    hp = n.get("host_platform") or {}
    system = (hp.get("system") if isinstance(hp, dict) else "") or ""
    return system.lower() not in ("android", "ios")


class ShardEnroll(BaseModel):
    node_id: str
    name: str = ""
    controller_id: str = ""       # target ring controller (auto-found if empty)
    model: str = ""
    layers: list = []             # requested window (advisory; planner decides)
    host_platform: dict = {}
    backend: dict = {}
    vram_budget_gib: float = 4.0
    ram_budget_gib: float = 4.0
    cores: int = 4
    perf_tps: float = 0.0
    stage_port: int = 51072
    ctx: int = 512


@app.post("/api/shard-enroll")
async def api_shard_enroll(req: ShardEnroll):
    """A NAT'd node self-enrolls to serve a shard. The hub registers it, binds it
    to a ring controller that has a coordinator on a public node, and serves.
    The hub cannot push a stage-start to a node reachable only outbound, so
    serve() publishes this node's stage config for it to pull
    (GET /api/shard-enroll/config) and self-start — inverting the node lifecycle
    for NAT participants."""
    c = CTRLS.get(req.controller_id) if req.controller_id else None
    if not c:
        # Auto-find a controller with a coordinator-capable node. Only an IDLE one:
        # a self-enroll triggers a re-serve, so falling back to a serving/loading
        # controller would preempt it (drop its model, splice this NAT node into the
        # ring) — exactly what let an offline phone repeatedly break a 122B ring.
        cands = [cc for cc in CTRLS.values()
                 if any(_node_can_coordinate(nid) for nid in cc.get("nodes", []))]
        c = next((cc for cc in cands if _ctrl_phase(cc) not in ("running", "loading")), None)
    if not c:
        raise HTTPException(409, "no idle ring controller with a coordinator is available")
    # Never preempt a controller that is serving or mid-load, even if named explicitly.
    if _ctrl_phase(c) in ("running", "loading"):
        raise HTTPException(409, "target ring controller is serving; self-enroll will not preempt it")
    n = NODES.get(req.node_id) or {}
    n.update({
        "id": req.node_id, "name": req.name or req.node_id,
        "kind": "self_enrolled", "nat": True, "self_enrolled": True,
        "host_platform": req.host_platform or {"system": "android", "machine": "arm64"},
        "backend": req.backend or {}, "vram": float(req.vram_budget_gib),
        "ram": float(req.ram_budget_gib), "cores": int(req.cores),
        "perf_tps": float(req.perf_tps), "vram_budget": float(req.vram_budget_gib),
        "ram_budget": float(req.ram_budget_gib), "vram_used": 0.0, "ram_used": 0.0,
        "rpc_port": int(req.stage_port) - 1000, "stage_port": int(req.stage_port),
        "worker_running": True, "bound_to": c["id"], "bound_to_name": c.get("name"),
        "gpu_uuid": "self-" + req.node_id, "gpu_name": (req.backend or {}).get("backend_device")
            or ((req.host_platform or {}).get("hostname")) or req.node_id,
        "resources": {"vram_budget_gib": float(req.vram_budget_gib),
                      "ram_budget_gib": float(req.ram_budget_gib),
                      "cores_budget": int(req.cores)},
    })
    NODES[req.node_id] = n
    if req.node_id not in c.get("nodes", []):
        c.setdefault("nodes", []).append(req.node_id)
    SELF_START_CONFIGS.pop(req.node_id, None)
    _log_event("shard_enroll", node_id=req.node_id, controller_id=c["id"],
               model=req.model, layers=req.layers)
    sreq = ServeReq(model=req.model, ctx=int(req.ctx), parallel=1, batch=128, ubatch=128,
                    runtime_mode=RING_PROXY)
    asyncio.create_task(api_ctrl_serve(c["id"], sreq))
    return {"enrolled": True, "controller_id": c["id"],
            "config_url": "/api/shard-enroll/config?node_id=" + req.node_id}


@app.get("/api/shard-enroll/config")
def api_shard_enroll_config(node_id: str = Query("")):
    """A self-enrolled node polls this for its stage config once serve() has
    planned the ring, then downloads its window and self-starts its stage."""
    cfg = SELF_START_CONFIGS.get(node_id)
    return {"ready": True, **cfg} if cfg else {"ready": False}


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


@app.get("/api/models/{name}/file")
def api_model_file(name: str, request: Request, service_token: str = Query("")):
    """Serve a staged GGUF so managed nodes (e.g. phones, which download over a
    plain URL) can pull it straight from the hub. Auth-gated: an auth-enabled hub
    requires a valid session, the M2M service token header, or ?service_token=
    (URLSession on the phone cannot set headers on a background download)."""
    if AUTH_ENABLED:
        st = request.headers.get("x-linkcpp-service-token", "") or service_token
        ok = bool(SERVICE_TOKEN and st and _hmac.compare_digest(st, SERVICE_TOKEN))
        if not ok and not _authed_wallet(request):
            raise HTTPException(401, "authentication required")
    path, _meta = _resolve_model(name)
    return FileResponse(path, filename=os.path.basename(path),
                        media_type="application/octet-stream")


class Download(BaseModel):
    url: str
    name: str


class ControllerDownload(BaseModel):
    model: str
    source: ModelSource
    op_id: str = ""


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
    asyncio.create_task(_download(d.url, os.path.join(MODEL_DIR, d.name), d.name))
    return {"started": d.name}


def _model_source_url(source):
    headers = {}
    if source.kind == "direct_url":
        if not source.url:
            raise HTTPException(400, "source.url is required")
        return source.url, headers
    if source.kind == "huggingface":
        if not source.repo_id or not source.filename:
            raise HTTPException(400, "repo_id and filename are required")
        url = f"https://huggingface.co/{source.repo_id}/resolve/{source.revision or 'main'}/{source.filename}"
        if source.hf_token:
            headers["Authorization"] = f"Bearer {source.hf_token}"
        return url, headers
    raise HTTPException(400, "source.kind must be direct_url or huggingface")


async def _download_from_source(source, dest, name):
    url, headers = _model_source_url(source)
    DL[name] = {"total": 0, "done": 0, "status": "downloading"}
    try:
        os.makedirs(os.path.dirname(dest), exist_ok=True)
        async with httpx.AsyncClient(timeout=None, follow_redirects=True) as c:
            async with c.stream("GET", url, headers=headers) as r:
                r.raise_for_status()
                DL[name]["total"] = int(r.headers.get("content-length", 0))
                tmp = dest + ".part"
                with open(tmp, "wb") as fh:
                    async for chunk in r.aiter_bytes(1 << 20):
                        fh.write(chunk)
                        DL[name]["done"] += len(chunk)
                os.replace(tmp, dest)
        DL[name]["status"] = "done"
    except Exception as e:
        DL[name]["status"] = f"error: {e}"


@app.post("/api/node-reports")
def api_node_reports(report: NodeReport):
    data = model_to_dict(report)
    n = NODES.get(report.node_id)
    if n:
        last_seq = int((n.get("last_report") or {}).get("seq", -1))
        if report.seq >= last_seq:
            n["last_report"] = data
            n["resources"] = data.get("resources", {})
            if report.op_type == "load":
                n["worker_running"] = report.status in ("running", "done") and report.phase not in ("model_missing", "error")
            elif report.op_type == "unload":
                n["worker_running"] = False
            if report.op_id:
                ops = n.setdefault("operations_map", {})
                ops[report.op_id] = data
                n["operations"] = list(ops.values())[-50:]
            if data.get("resources"):
                n["ram_used"] = data["resources"].get("ram_used_gib", n.get("ram_used", 0.0))
    c = CTRLS.get(report.controller_id or "")
    if c and report.op_id:
        _record_ctrl_op(c, report.op_type or "node", report.phase, report.status,
                        report.progress, report.message, op_id=report.op_id,
                        node_id=report.node_id, model=report.model, error=report.error,
                        details={"report_seq": report.seq})
    return {"accepted": True}


class LoadCancelled(Exception):
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


def _append_llama_perf_args(cmd, req):
    spec_type = _validate_spec_types(getattr(req, "spec_type", "none"))
    if spec_type != "none":
        cmd += ["--spec-type", spec_type]
    draft_model = str(getattr(req, "spec_draft_model", "") or "").strip()
    if draft_model:
        cmd += ["--spec-draft-model", draft_model]
    for attr, flag in (
        ("batch", "-b"),
        ("ubatch", "-ub"),
        ("poll", "--poll"),
        ("spec_draft_n_max", "--spec-draft-n-max"),
        ("spec_draft_n_min", "--spec-draft-n-min"),
        ("spec_ngram_mod_n_min", "--spec-ngram-mod-n-min"),
        ("spec_ngram_mod_n_max", "--spec-ngram-mod-n-max"),
        ("spec_ngram_mod_n_match", "--spec-ngram-mod-n-match"),
        ("spec_ngram_simple_size_n", "--spec-ngram-simple-size-n"),
        ("spec_ngram_simple_size_m", "--spec-ngram-simple-size-m"),
        ("spec_ngram_simple_min_hits", "--spec-ngram-simple-min-hits"),
        ("cache_reuse", "--cache-reuse"),
    ):
        value = _positive_int(getattr(req, attr, 0))
        if value:
            cmd += [flag, str(value)]
    for attr, flag in (
        ("spec_draft_p_min", "--spec-draft-p-min"),
        ("spec_draft_p_split", "--spec-draft-p-split"),
    ):
        value = _non_negative_float(getattr(req, attr, None))
        if value is not None:
            cmd += [flag, str(value)]
    for attr, flag in (
        ("lookup_cache_static", "--lookup-cache-static"),
        ("lookup_cache_dynamic", "--lookup-cache-dynamic"),
    ):
        value = str(getattr(req, attr, "") or "").strip()
        if value:
            cmd += [flag, value]
    if getattr(req, "cont_batching", True) is False:
        cmd += ["--no-cont-batching"]
    return cmd


def _load_cancel_requested(c):
    return bool((c.get("load_cancel") or {}).get("requested"))


def _raise_if_load_cancelled(c):
    if _load_cancel_requested(c):
        raise LoadCancelled((c.get("load_cancel") or {}).get("reason") or "load canceled")


def _copy_model_file(src, dst, sink, label, cancel=None):
    if cancel:
        cancel()
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    total = os.path.getsize(src)
    if os.path.exists(dst) and os.path.getsize(dst) == total:
        return dst
    # Models and the staging area normally share the same mounted volume.  A
    # hard link avoids duplicating multi-shard GGUF files before an RPC master
    # reads them, while unsupported mounts retain the existing copy behavior.
    try:
        os.link(src, dst)
        sink["detail"] = f"staged {label} as hard link"
        return dst
    except FileExistsError:
        if os.path.getsize(dst) == total:
            return dst
    except OSError:
        pass
    tmp = dst + ".part"; done = 0; gib = total / 1024**3
    try:
        with open(src, "rb") as fi, open(tmp, "wb") as fo:
            while True:
                b = fi.read(16 << 20)
                if not b:
                    break
                if cancel:
                    cancel()
                fo.write(b); done += len(b)
                sink["detail"] = f"staging {label} {done*100//total}% ({done/1024**3:.1f}/{gib:.1f} GiB)"
    except LoadCancelled:
        try:
            os.remove(tmp)
        except OSError:
            pass
        raise
    if cancel:
        cancel()
    os.replace(tmp, dst)
    return dst


def _stage_model(model_ref, sink, cancel=None):
    try:
        os.makedirs(STAGE_DIR, exist_ok=True)
        primary_src, meta = _resolve_model(model_ref)
        for rel in meta.get("shards", []) + meta.get("aux_files", []):
            src = os.path.join(MODEL_DIR, rel)
            if os.path.exists(src):
                _copy_model_file(src, os.path.join(STAGE_DIR, rel), sink, rel, cancel=cancel)
        if cancel:
            cancel()
        sink["detail"] = "model already staged"
        return os.path.join(STAGE_DIR, meta["primary_file"])
    except LoadCancelled:
        raise
    except Exception:
        path, _ = _resolve_model(model_ref)
        if cancel:
            cancel()
        return path


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


class CreateCtrl(BaseModel):
    name: str = ""


@app.get("/api/controllers")
def api_ctrls():
    _ensure_local_slots()
    return {"controllers": [ctrl_view(c) for c in CTRLS.values()]}


@app.post("/api/controllers")
def api_create_ctrl(c: CreateCtrl):
    cid = "ctrl-" + uuid.uuid4().hex[:6]
    CTRLS[cid] = {"id": cid, "name": c.name or cid, "nodes": [], "model": None,
                  "ctx": 4096, "parallel": 1, "phase": "idle", "detail": "",
                  "plan": None, "master": None, "last_load": {},
                  "master_port": _next_master_port(), "operations": {}}
    _persist_hub_state()
    return ctrl_view(CTRLS[cid], full=True)


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


@app.get("/api/controllers/{cid}")
def api_ctrl_detail(cid: str):
    _ensure_local_slots()
    c = CTRLS.get(cid)
    if not c:
        raise HTTPException(404, "unknown controller")
    return ctrl_view(c, full=True)


@app.get("/api/controllers/{cid}/runtime-check")
def api_ctrl_runtime_check(cid: str):
    c = CTRLS.get(cid)
    if not c:
        raise HTTPException(404, "unknown controller")
    return _controller_runtime_check(c)


@app.delete("/api/controllers/{cid}")
async def api_del_ctrl(cid: str):
    _ensure_local_slots()
    c = CTRLS.get(cid)
    if not c:
        raise HTTPException(404, "unknown controller")
    _kill(c.get("master"))
    for nid in list(c["nodes"]):
        n = NODES.get(nid)
        if not n:
            continue
        try:
            if n.get("kind", "local") == "local":
                _kill(n.get("worker")); n["worker"] = None
                n["worker_running"] = False
                n["desired_load"] = None
            elif n.get("kind") == "agent":
                await _unbind_agent_confirmed(n)
            elif n.get("kind") == "remote_unit_node":
                await _remote_unit_node_request(
                    n, "POST",
                    f"/api/nodes/{quote(n.get('remote_source_node_id') or '', safe='')}/remote-release",
                    body={"controller_id": cid})
            n["bound_to"] = None
            n["bound_to_name"] = None
        except Exception as exc:
            # Do not discard the controller record while it still owns a
            # worker.  Keeping it visible makes the release safely retryable.
            raise HTTPException(409, f"controller deletion blocked; {nid} resources were not released: {exc}") from exc
    for unit in list(_controller_remote_units(cid)):
        _delete_remote_unit(unit["id"])
    CTRLS.pop(cid, None)
    _persist_hub_state()
    return {"removed": cid}


class BindNode(BaseModel):
    node_id: str


@app.post("/api/controllers/{cid}/bind")
async def api_bind(cid: str, b: BindNode, request: Request = None):
    _ensure_local_slots()
    c = CTRLS.get(cid)
    n = NODES.get(b.node_id)
    if not c or not n:
        raise HTTPException(404, "unknown controller or node")
    if not _slot_configured(n):
        raise HTTPException(409, "assign node resources before binding")
    if (n.get("kind") == "remote_unit_node" and n.get("owner_controller_id")
            and n.get("owner_controller_id") != cid):
        raise HTTPException(404, "remote unit node is not registered by this controller")
    if n["bound_to"] and n["bound_to"] != cid:
        raise HTTPException(409, f"node already bound to {n['bound_to']}")
    if _remote_unit_node_bound_in_unit(n) and n.get("bound_to") != cid:
        owner = n.get("remote_controller_name") or n.get("remote_controller_id")
        raise HTTPException(409, f"remote unit node already bound in its unit: {owner}")
    _require_node_runtime(n)
    if n.get("kind") == "remote_unit_node" and request is not None:
        report_base = PUBLIC_HUB_URL or str(request.base_url)
        try:
            await _remote_unit_node_request(n, "POST",
                                            f"/api/nodes/{quote(n.get('remote_source_node_id') or '', safe='')}/remote-claim",
                                            body={"controller_id": cid, "controller_name": c.get("name", cid),
                                                  "report_url": report_base.rstrip("/") + "/api/node-reports"})
        except Exception as exc:
            raise HTTPException(409, f"remote node cannot be bound: {exc}") from exc
    n["bound_to"] = cid
    if n.get("kind") == "agent" and n.get("agent_url"):
        # Hand the managed agent its report URL + M2M token so its node telemetry
        # reaches an auth-enabled hub. Startup discovery (via /control/status) does
        # not establish these, so without this a bound agent never reports.
        report_base = PUBLIC_HUB_URL or (str(request.base_url) if request is not None else "")
        if report_base:
            n["report_url"] = report_base.rstrip("/") + "/api/node-reports"
            try:
                await _agent_request(n, "POST", "/control/join", body={
                    "controller_id": cid, "report_url": n["report_url"],
                    "service_token": SERVICE_TOKEN or None, "name": n.get("name"),
                }, timeout=10)
            except Exception:
                pass  # best-effort; the bind itself still succeeds
    if b.node_id not in c["nodes"]:
        c["nodes"].append(b.node_id)
    _persist_hub_state()
    return ctrl_view(c, full=True)


@app.post("/api/controllers/{cid}/unbind")
async def api_unbind(cid: str, b: BindNode):
    _ensure_local_slots()
    c = CTRLS.get(cid)
    n = NODES.get(b.node_id)
    if not c or not n:
        raise HTTPException(404, "unknown controller or node")
    if n.get("kind") == "remote_unit_node":
        try:
            await _remote_unit_node_request(n, "POST",
                                            f"/api/nodes/{quote(n.get('remote_source_node_id') or '', safe='')}/remote-release",
                                            body={"controller_id": cid})
        except Exception as exc:
            raise HTTPException(409, f"remote node cannot be released: {exc}") from exc
    if n.get("kind", "local") == "local":
        _kill(n.get("worker")); n["worker"] = None
        n["worker_running"] = False
        n["desired_load"] = None
    elif n.get("kind") == "agent":
        try:
            await _unbind_agent_confirmed(n)
        except Exception as exc:
            # Keep ownership intact: a second controller must never acquire a
            # node whose previous Metal worker has not released its memory.
            raise HTTPException(409, f"node resources were not released: {exc}") from exc
    n["bound_to"] = None
    n["bound_to_name"] = None
    c["nodes"] = [x for x in c["nodes"] if x != b.node_id]
    _persist_hub_state()
    return ctrl_view(c, full=True)


async def api_ctrl_link_agent_node(cid: str, req: LinkAgentNode, request: Request):
    c = CTRLS.get(cid)
    if not c:
        raise HTTPException(404, "unknown controller")
    report_base = PUBLIC_HUB_URL or str(request.base_url)
    report_url = report_base.rstrip("/") + "/api/node-reports"
    n = await _create_agent_node(cid, req, report_url)
    if n["id"] not in c["nodes"]:
        c["nodes"].append(n["id"])
    _record_ctrl_op(c, "join", "joined", "done", 100.0, "agent node linked", node_id=n["id"])
    _persist_hub_state()
    return ctrl_view(c, full=True)


@app.post("/api/controllers/{cid}/metal-slot/resources")
async def api_update_metal_slot_resources(cid: str, req: MetalSlotResources):
    c = CTRLS.get(cid)
    recovered_stale_binding = False
    node = None
    if not c and req.node_id:
        candidate = NODES.get(req.node_id)
        # A Metal host has exactly one logical slot.  If its native agent is
        # bound to a controller that this hub no longer knows about, the only
        # idle controller is the safe replacement.  This happens after the
        # hub and native-agent state files are restored independently.
        candidates = [item for item in CTRLS.values()
                      if item.get("phase") in ("idle", "error")
                      and not _proc_alive(item.get("master"))]
        if (candidate and candidate.get("kind") == "agent"
                and _node_backend(candidate).get("backend_kind") == "metal"
                and candidate.get("bound_to") == cid and len(candidates) == 1):
            c = candidates[0]
            node = candidate
            recovered_stale_binding = True
    if not c:
        raise HTTPException(404, "unknown controller")
    if c.get("phase") not in ("idle", "error") or _proc_alive(c.get("master")):
        raise HTTPException(409, "unload the controller before changing Metal slot resources")
    node = node or next((NODES.get(nid) for nid in c.get("nodes", [])
                         if NODES.get(nid, {}).get("kind") == "agent"
                         and _node_backend(NODES[nid]).get("backend_kind") == "metal"), None)
    if not node:
        raise HTTPException(404, "no Metal slot is bound to this controller")
    if req.ram_budget_gib <= 0 or req.cores_budget <= 0:
        raise HTTPException(400, "RAM and CPU core budgets must be greater than zero")
    try:
        if recovered_stale_binding:
            # The old controller does not exist in this hub.  Release that
            # orphaned ownership before assigning the fixed Metal slot to the
            # recovered controller.
            await _agent_request(node, "POST", "/unbind")
        info = await _agent_request(node, "POST", "/control/join", {
            "controller_id": c["id"],
            "report_url": node.get("report_url"),
            "name": node.get("name") or "Slot 1",
            # Apple Silicon uses unified memory. The planner uses the same selected
            # budget for Metal weights and RAM rather than pretending there are two
            # independently partitionable pools.
            "vram_budget_gib": req.ram_budget_gib,
            "ram_budget_gib": req.ram_budget_gib,
            "cores_budget": req.cores_budget,
        })
    except httpx.HTTPError as exc:
        raise HTTPException(503, "Mac Metal Slot 1 agent is unavailable; start the native node agent and retry") from exc
    resources = info.get("resources") or {}
    if recovered_stale_binding:
        node["bound_to"] = c["id"]
        node["bound_to_name"] = c["name"]
        if node["id"] not in c["nodes"]:
            c["nodes"].append(node["id"])
        _record_ctrl_op(c, "join", "recovered", "done", 100.0,
                        "recovered stale Mac Metal Slot 1 binding", node_id=node["id"])
    node.update({"vram": resources.get("vram_budget_gib", node.get("vram", 0.0)),
                 "ram": resources.get("ram_budget_gib", node.get("ram", 0.0)),
                 "cores": resources.get("cores_budget", node.get("cores", 0)),
                 "resources": resources, "backend": info.get("backend", node.get("backend"))})
    _persist_hub_state()
    return ctrl_view(c, full=True)


class ServeReq(BaseModel):
    model: str
    ctx: int = 4096
    parallel: int = 1
    kv_bits: int = 16
    cache_type_k: str = "f16"
    cache_type_v: str = "f16"
    no_cpu_offload: bool = False
    reserve_mib: int = 1024
    placement_strategy: str = "balanced"
    require_all_nodes: bool = False
    runtime_mode: str = DEFAULT_RUNTIME_MODE
    batch: int = 0
    ubatch: int = 0
    poll: int = 0
    cont_batching: bool = True
    cache_reuse: int = 0
    spec_type: str = "none"
    spec_draft_model: str = ""
    spec_draft_n_max: int = 0
    spec_draft_n_min: int = 0
    spec_draft_p_min: Optional[float] = None
    spec_draft_p_split: Optional[float] = None
    spec_ngram_mod_n_min: int = 0
    spec_ngram_mod_n_max: int = 0
    spec_ngram_mod_n_match: int = 0
    spec_ngram_simple_size_n: int = 0
    spec_ngram_simple_size_m: int = 0
    spec_ngram_simple_min_hits: int = 0
    lookup_cache_static: str = ""
    lookup_cache_dynamic: str = ""


def _ctrl_planner_nodes(c):
    # host_platform + backend let the ring planner keep the coordinator off phones
    # and weight layer splits by compute tier (see planner._can_coordinate /
    # _node_compute_factor).
    return [{"vram": NODES[nid]["vram"], "ram": NODES[nid]["ram"], "cores": NODES[nid]["cores"],
             "host_platform": NODES[nid].get("host_platform") or {},
             "backend": NODES[nid].get("backend") or {},
             "perf_tps": NODES[nid].get("perf_tps")}
            for nid in c["nodes"] if nid in NODES]


def _node_monitored(n):
    return n.get("kind") != "remote"


def _annotate_plan(c, result, model_meta):
    result["model_ref"] = model_meta["id"]
    result["model_label"] = model_meta["label"]
    result["model_shards"] = list(model_meta.get("shards") or [model_meta["primary_file"]])
    result["adaptive_load_available"] = True
    result["monitoring_required"] = True
    nids = [nid for nid in c["nodes"] if nid in NODES]
    for p in result.get("placement", []):
        idx = p.get("node")
        if idx is None or idx >= len(nids):
            continue
        n = NODES[nids[idx]]
        p["node_id"] = n["id"]
        p["node_name"] = n["name"]
        p["node_kind"] = n.get("kind", "local")
        p["gpu_name"] = n.get("gpu_name", "")
        p["monitoring"] = _node_monitored(n)
        p["kv_vram_gib"] = p.get("kv_vram_gib", result.get("kv_per_layer_gib", 0) * p.get("n_layers", 0))
        if p.get("n_layers") and not p["monitoring"]:
            result["adaptive_load_available"] = False
    if not result["adaptive_load_available"]:
        result["adaptive_load_blocker"] = "some nodes cannot report VRAM/RAM usage"
    return result


def _do_plan(c, req):
    _require_controller_runtime(c)
    nl = _ctrl_planner_nodes(c)
    if not nl:
        raise HTTPException(400, "controller has no bound nodes")
    model_path, meta = _resolve_model(req.model)
    req.cache_type_k = _validate_cache_type(req.cache_type_k, "cache_type_k")
    req.cache_type_v = _validate_cache_type(req.cache_type_v, "cache_type_v")
    _validate_spec_types(req.spec_type)
    if req.reserve_mib < 0:
        raise HTTPException(400, "reserve_mib must be non-negative")
    try:
        req.runtime_mode = normalize_runtime_mode(req.runtime_mode)
    except ValueError as exc:
        raise HTTPException(400, str(exc))
    if req.placement_strategy not in PLACEMENT_STRATEGIES:
        raise HTTPException(400, f"unsupported placement strategy: {req.placement_strategy}")
    # A ring keeps every bound rank (weights + KV live on the rank that owns the
    # layer), so ring loads always plan with the all-rank ring-stage strategy
    # regardless of the caller's default RPC-oriented choice.
    if req.runtime_mode == RING_PROXY and req.placement_strategy != "ring-stage-vram-weighted":
        req.placement_strategy = "ring-stage-vram-weighted"
    shard_paths = [os.path.join(MODEL_DIR, rel) for rel in meta.get("shards", [])]
    m = read_model(model_path, shard_paths=shard_paths)
    result = _annotate_plan(
        c,
        run_plan(m, nl, req.ctx, req.parallel, req.kv_bits,
                 cache_type_k=req.cache_type_k, cache_type_v=req.cache_type_v,
                 reserve_mib=req.reserve_mib, no_cpu_offload=req.no_cpu_offload,
                 placement_strategy=req.placement_strategy,
                 master_ram_gib=_master_ram_budget_gib(),
                 _allow_subset_search=not req.require_all_nodes),
        meta,
    )
    result["master_load_timeout_s"] = _master_load_timeout_s(result)
    result["runtime_mode"] = req.runtime_mode
    nids = [nid for nid in c["nodes"] if nid in NODES]
    active = [(nids[p["node"]], p) for p in result.get("placement", []) if p.get("n_layers")]
    topology = _rpc_topology(c, active, req.runtime_mode)
    result["data_plane"] = topology[0]["data_plane"] if topology else data_plane_contract(req.runtime_mode, [])
    return result


@app.post("/api/controllers/{cid}/plan")
def api_ctrl_plan(cid: str, req: ServeReq):
    c = CTRLS.get(cid)
    if not c:
        raise HTTPException(404, "unknown controller")
    if _ctrl_phase(c) not in ("loading", "unloading"):
        _clear_ctrl_activity(c, "new_plan", req.model)
    _log_event("plan_request_received", controller_id=cid, model=req.model,
               ctx=req.ctx, parallel=req.parallel, kv_bits=req.kv_bits,
               cache_type_k=req.cache_type_k, cache_type_v=req.cache_type_v,
               nodes=list(c.get("nodes", [])))
    result = _do_plan(c, req)
    c["plan"] = result
    _log_event("load_plan_result", controller_id=cid, model=req.model,
               plan=_plan_log_summary(result))
    _record_ctrl_op(c, "plan", "planned", "done" if result.get("feasible") else "error",
                    100.0 if result.get("feasible") else 0.0,
                    "load plan generated", model=result.get("model_ref", req.model),
                    details={"adaptive_load_available": result.get("adaptive_load_available")})
    return result


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


@app.post("/api/controllers/{cid}/models/download")
async def api_ctrl_download_model(cid: str, req: ControllerDownload):
    c = CTRLS.get(cid)
    if not c:
        raise HTTPException(404, "unknown controller")
    model = os.path.basename(req.model)
    op_id = req.op_id or "download-" + uuid.uuid4().hex[:10]
    _record_ctrl_op(c, "download", "dispatch", "running", 0.0,
                    "dispatching model download", op_id=op_id, model=model,
                    details={"source": req.source.redacted()})
    if not os.path.exists(os.path.join(MODEL_DIR, model)):
        asyncio.create_task(_download_from_source(req.source, os.path.join(MODEL_DIR, model), model))
    else:
        DL[model] = {"total": os.path.getsize(os.path.join(MODEL_DIR, model)),
                     "done": os.path.getsize(os.path.join(MODEL_DIR, model)),
                     "status": "done"}
    agent_results = []
    for nid in c["nodes"]:
        n = NODES.get(nid)
        if not n or n.get("kind") != "agent":
            continue
        body = model_to_dict(DownloadRequest(model=model, source=req.source, op_id=f"{op_id}-{nid}"))
        try:
            agent_results.append({"node_id": nid, "result": await _agent_request(n, "POST", "/control/download", body)})
        except Exception as exc:
            agent_results.append({"node_id": nid, "error": str(exc)})
    _record_ctrl_op(c, "download", "dispatched", "running", 5.0,
                    "download dispatched", op_id=op_id, model=model,
                    details={"agents": agent_results})
    return {"accepted": True, "op_id": op_id, "model": model, "agents": agent_results,
            "controller_download": DL.get(model)}


async def _load_rpc_node(c, req, result, nid, placement):
    _raise_if_load_cancelled(c)
    n = NODES[nid]
    n["ram_used"] = round(placement.get("ram_used_gib", 0.0), 2)
    desired_load = {
        "model": req.model,
        "layers": placement.get("layers"),
        "tensor_split": result.get("tensor_split", []),
        "offload": placement.get("ot"),
        "parallel": req.parallel,
        "ctx": req.ctx,
        "kv_bits": req.kv_bits,
        "cache_type_k": result.get("cache_type_k", req.cache_type_k),
        "cache_type_v": result.get("cache_type_v", req.cache_type_v),
    }
    if n.get("kind") == "agent":
        op_id = f"load-{c['id']}-{nid}-{uuid.uuid4().hex[:6]}"
        body = model_to_dict(LoadRequest(
            model=req.model,
            op_id=op_id,
            layers=placement.get("layers"),
            tensor_split=result.get("tensor_split", []),
            offload=placement.get("ot"),
            parallel=req.parallel,
            ctx=req.ctx,
            kv_bits=req.kv_bits,
            cache_type_k=result.get("cache_type_k", req.cache_type_k),
            cache_type_v=result.get("cache_type_v", req.cache_type_v),
        ))
        _record_ctrl_op(c, "load", "load_requested", "running", 5.0,
                        "requesting node load", op_id=op_id, node_id=nid, model=req.model)
        _log_event("agent_node_load_request", controller_id=c.get("id"), node_id=nid,
                   op_id=op_id, model=req.model, placement=placement)
        info = await _agent_request(n, "POST", "/control/load", body, timeout=60)
        _raise_if_load_cancelled(c)
        n["worker_running"] = True
        n["desired_load"] = body
        n["operations"] = info.get("status", {}).get("operations", n.get("operations", []))
        # A native Metal RPC worker initializes the Metal runtime before it
        # binds its TCP socket.  Do not start the Linux master after a fixed
        # delay: on Apple Silicon that races the bind and produces a misleading
        # malformed-RPC response.
        ready_probe = await _wait_node_rpc_ready(n, timeout=60.0)
        if not ready_probe.get("ready"):
            raise HTTPException(503, f"node RPC worker is not ready at {_node_rpc_endpoint(n)}")
        return _node_rpc_endpoint(n)
    if n.get("kind") == "remote_unit_node":
        check = await _remote_unit_node_ready_check(n, start=True)
        if not check["ready"]:
            raise HTTPException(409, _remote_unit_block_message([check]))
        n["desired_load"] = desired_load
        _log_event("remote_unit_node_load_ready", controller_id=c.get("id"), node_id=nid,
                   model=req.model, check=check, placement=placement)
        return _node_rpc_endpoint(n)
    _log_event("local_node_load_request", controller_id=c.get("id"), node_id=nid,
               model=req.model, placement=placement)
    _start_node_worker(n)
    _raise_if_load_cancelled(c)
    n["worker_running"] = True
    n["desired_load"] = desired_load
    ready_probe = await _wait_node_rpc_ready(n, timeout=20.0)
    _log_event("local_node_rpc_ready_check", controller_id=c.get("id"), node_id=nid,
               model=req.model, check=ready_probe)
    if not ready_probe.get("ready"):
        raise HTTPException(409, f"local RPC worker not ready: {nid} at {_node_rpc_endpoint(n)}")
    return _node_rpc_endpoint(n)


def _combined_ot(active):
    rules = []
    seen = set()
    for _, placement in active:
        for rule in (placement.get("ot") or "").split(","):
            rule = rule.strip()
            if rule and rule not in seen:
                seen.add(rule)
                rules.append(rule)
    return ",".join(rules)


async def _cancel_load_runtime(c, reason="requested"):
    async with _controller_teardown_lock(c):
        _kill(c.get("master")); c["master"] = None
        node_results = [{"kind": "unit_load_session", **item}
                        for item in await _release_remote_unit_sessions(c, reason)]
        for nid in list(c.get("nodes", [])):
            n = NODES.get(nid)
            if not n:
                continue
            if n.get("kind") == "local":
                _kill(n.get("worker")); n["worker"] = None
                n["worker_running"] = False
                n["desired_load"] = None
                n["ram_used"] = 0.0
                node_results.append({"node_id": nid, "status": "canceled", "kind": "local"})
            elif n.get("kind") == "agent":
                if not n.get("worker_running") and not n.get("desired_load"):
                    node_results.append({"node_id": nid, "status": "already_stopped", "kind": "agent"})
                    continue
                try:
                    body = model_to_dict(CancelLoadRequest(
                        op_id=f"cancel-{c['id']}-{nid}-{uuid.uuid4().hex[:6]}",
                        reason=reason,
                    ))
                    await _stop_agent_worker_confirmed(n, "/control/load/cancel", body)
                    n["ram_used"] = 0.0
                    node_results.append({"node_id": nid, "status": "canceled", "kind": "agent"})
                except Exception as exc:
                    node_results.append({"node_id": nid, "status": "error", "kind": "agent", "error": str(exc)})
            elif n.get("kind") == "remote_unit_node":
                if not n.get("worker_running") and not n.get("desired_load"):
                    node_results.append({"node_id": nid, "status": "already_stopped", "kind": "remote_unit_node"})
                    continue
                try:
                    await _stop_remote_unit_worker(n, reason)
                    await _refresh_remote_unit_node(n)
                    if n.get("worker_running") or n.get("desired_load"):
                        raise RuntimeError("remote unit still reports its worker as running")
                    n["ram_used"] = 0.0
                    node_results.append({"node_id": nid, "status": "canceled", "kind": "remote_unit_node"})
                except Exception as exc:
                    node_results.append({"node_id": nid, "status": "error",
                                         "kind": "remote_unit_node", "error": str(exc)})
        return node_results


async def _serve_llama_rpc(c, req, result, active):
    serve_op = None
    try:
        if normalize_runtime_mode(req.runtime_mode) != LLAMA_RPC:
            raise RuntimeError("llama RPC loader received a non-RPC runtime mode")
        _require_controller_runtime(c)
        _raise_if_load_cancelled(c)
        _log_event("load_task_started", controller_id=c.get("id"), model=req.model,
                   ctx=req.ctx, parallel=req.parallel, plan=_plan_log_summary(result))
        c.update(phase="loading", detail="staging model", model=req.model,
                 parallel=req.parallel, plan=result)
        serve_op = _record_ctrl_op(c, "load", "staging_model", "running", 1.0,
                                   "staging model", model=req.model)
        _record_ctrl_op(c, "calibration", "first_layer_probe", "done", 100.0,
                        "first-layer calibration uses monitored node reports when available",
                        model=req.model, details={"confidence": min((p.get("confidence", 0.75) for _, p in active), default=0.75)})
        model_path = await asyncio.to_thread(_stage_model, req.model, c, lambda: _raise_if_load_cancelled(c))
        _raise_if_load_cancelled(c)
        _log_event("model_staged", controller_id=c.get("id"), model=req.model,
                   model_path=model_path)
        c["detail"] = "starting workers"
        topology = _rpc_topology(c, active, req.runtime_mode)
        rpc_eps = []
        for nid, p in active:
            _raise_if_load_cancelled(c)
            rpc_eps.append(await _load_rpc_node(c, req, result, nid, p))
            _start_load_monitor(c, req, nid, p, serve_op["op_id"])
        await asyncio.sleep(2)
        _raise_if_load_cancelled(c)
        ts = ",".join(str(p["n_layers"]) for _, p in active)
        gpu_layers = int(result.get("gpu_layers") or result.get("model", {}).get("n_layer") or 0)
        # A Metal-enabled RPC server advertises its Metal device followed by a
        # CPU fallback device.  The fallback has no allocatable device memory,
        # but llama.cpp otherwise auto-selects it along with the intended RPC
        # endpoints.  Keep the tensor-split ranks aligned with the planned
        # accelerators by selecting only the first device for each endpoint.
        rpc_devices = []
        rpc_device_index = 0
        for nid, _ in active:
            rpc_devices.append(f"RPC{rpc_device_index}")
            rpc_device_index += 1
            if _node_backend(NODES[nid]).get("backend_kind") == "metal":
                rpc_device_index += 1
        cmd = [LLAMA_SERVER, "-m", model_path, "-ngl", str(gpu_layers), "--rpc", ",".join(rpc_eps),
               "--device", ",".join(rpc_devices), "--tensor-split", ts, "-np", str(req.parallel),
               "-c", str(req.ctx * req.parallel), "--host", "127.0.0.1",
               "--port", str(c["master_port"]), "--flash-attn", "on",
               "--cache-type-k", result.get("cache_type_k", req.cache_type_k),
               "--cache-type-v", result.get("cache_type_v", req.cache_type_v)]
        # The RPC master must own a bounded, resident model copy before it
        # distributes tensors.  mmap faults through a host bind mount bypass
        # that budget and can stall a large distributed load indefinitely.
        cmd.append("--no-mmap")
        # Use the model's Jinja chat template so native tool calling (tools /
        # tool_choice -> tool_calls) and correct role formatting work through the
        # OpenAI-compatible gateway. Inert for models without a tool template.
        cmd.append("--jinja")
        # The linkcpp planner has already reserved per-worker VRAM/RAM.  Do
        # not let llama.cpp's auto-fit issue an unbounded device-memory probe
        # to every RPC backend; some managed Metal workers cannot serve that
        # optional query even though tensor RPC is available.
        cmd += ["--fit", "off"]
        # Keep a detailed master-side trace in the load artifact.  Worker-side
        # RPC tracing is enabled separately through GGML_RPC_DEBUG.
        cmd += ["-lv", "4"]
        _append_llama_perf_args(cmd, req)
        if result.get("kv_cache_location") == "ram":
            cmd += ["--no-kv-offload"]
        ot = _combined_ot(active)
        if ot:
            cmd += ["-ot", ot]
        env = dict(os.environ, CUDA_VISIBLE_DEVICES="", GGML_RPC_DEBUG="1")
        _kill(c.get("master"))
        _raise_if_load_cancelled(c)
        c["detail"] = "loading model into GPUs"
        _record_ctrl_op(c, "load", "master_starting", "running", 40.0,
                        "starting llama-server master", op_id=serve_op["op_id"], model=req.model,
                        details={"rpc_workers": rpc_eps, "tensor_split": ts, "ot": ot,
                                 "flash_attention": True,
                                 "cache_type_k": result.get("cache_type_k"),
                                 "cache_type_v": result.get("cache_type_v"),
                                 "performance": _perf_req_snapshot(req),
                                 "rpc_topology": topology})
        master_log = _master_log_path(c)
        _log_event("master_starting", controller_id=c.get("id"), model=req.model,
                   cmd=cmd, rpc_workers=rpc_eps, tensor_split=ts, ot=ot,
                   rpc_topology=topology, log_path=master_log)
        c["master"] = subprocess.Popen(cmd, env=env,
                                       stdout=open(master_log, "w"),
                                       stderr=subprocess.STDOUT)
        diag_dir = _load_diag_dir(c, serve_op["op_id"])
        _diag_write_json(os.path.join(diag_dir, "manifest.json"), {
            "created_at": time.time(), "controller_id": c.get("id"), "model": req.model,
            "command": cmd, "environment": {k: env.get(k) for k in ("CUDA_VISIBLE_DEVICES", "GGML_RPC_DEBUG", "LLAMA_CACHE")},
            "plan": result, "rpc_topology": topology, "rpc_workers": rpc_eps,
            "master": _master_process_snapshot(c),
        })
        _diag_event(c, serve_op["op_id"], "master_started", pid=c["master"].pid,
                    diagnostic_dir=diag_dir, rpc_workers=rpc_eps)
        _log_event("master_started", controller_id=c.get("id"), pid=c["master"].pid,
                   port=c["master_port"], log_path=master_log)
        load_timeout_s = int(result.get("master_load_timeout_s") or _master_load_timeout_s(result))
        expected_load_bytes = int(max(0.0, float(result.get("total_weight_gib") or 0.0)) * 1024 ** 3)
        ok = await _wait_health(c["master_port"], c, op_id=serve_op["op_id"], secs=load_timeout_s,
                                expected_load_bytes=expected_load_bytes)
        _raise_if_load_cancelled(c)
        if ok:
            c["detail"] = "running smoke inference"
            _log_event("master_healthy", controller_id=c.get("id"), port=c["master_port"])
            try:
                _raise_if_load_cancelled(c)
                await _master_chat(c["master_port"], {
                    "messages": [{"role": "user", "content": "Reply with exactly: ok"}],
                    "max_tokens": 8,
                    "temperature": 0,
                })
                _raise_if_load_cancelled(c)
                c.update(phase="running", detail="")
                c["last_load"] = _serve_req_snapshot(req)
                c["ctx"] = req.ctx
                c["parallel"] = req.parallel
                _persist_hub_state()
                _record_ctrl_op(c, "load", "running", "done", 100.0,
                                "model loaded and smoke test passed",
                                op_id=serve_op["op_id"], model=req.model)
                await _capture_load_diagnostics(c, serve_op["op_id"], "load_healthy", include_node_logs=True)
                _cancel_load_monitors(c, final_status="done")
                _log_event("load_task_done", controller_id=c.get("id"), model=req.model)
            except Exception as exc:
                c.update(phase="error", detail=f"smoke inference failed: {exc}")
                _record_ctrl_op(c, "load", "smoke_error", "error", 90.0,
                                "smoke inference failed", op_id=serve_op["op_id"],
                                model=req.model, error=str(exc))
                _log_event("smoke_inference_failed", controller_id=c.get("id"),
                           model=req.model, error=str(exc), exc_info=True)
                _cancel_load_monitors(c)
        else:
            c.update(phase="error", detail="master not healthy")
            master_tail = tail(_master_log_path(c), 120)
            diagnostic_dir = await _capture_load_diagnostics(
                c, serve_op["op_id"], "master_not_healthy_pre_cleanup", include_node_logs=True)
            master_snapshot = _master_process_snapshot(c)
            # A health timeout is a terminal failure for this load attempt.
            # Leaving llama-server alive keeps its partial RPC allocations and
            # makes the next attempt contend with a ghost master process.
            _kill(c.get("master"))
            c["master"] = None
            node_results = await _cancel_load_runtime(c, "master not healthy")
            _record_ctrl_op(c, "load", "error", "error", 0.0,
                            "master not healthy", op_id=serve_op["op_id"], model=req.model,
                            error="master not healthy", details={"master_log_tail": master_tail,
                                                                   "nodes": node_results,
                                                                   "diagnostic_dir": diagnostic_dir,
                                                                   "master_snapshot": master_snapshot})
            _log_event("master_not_healthy", controller_id=c.get("id"), model=req.model,
                       port=c["master_port"], master_returncode=master_snapshot.get("returncode"),
                       master_log_tail=master_tail, diagnostic_dir=diagnostic_dir)
            _cancel_load_monitors(c)
    except LoadCancelled as e:
        reason = str(e) or "load canceled"
        node_results = await _cancel_load_runtime(c, reason)
        c.update(phase="idle", detail="", model=None)
        _record_ctrl_op(c, "load", "canceled", "canceled", 0.0,
                        reason, op_id=serve_op["op_id"] if serve_op else None,
                        model=req.model, details={"nodes": node_results})
        _log_event("load_task_canceled", controller_id=c.get("id"), model=req.model,
                   reason=reason, nodes=node_results)
        _cancel_load_monitors(c)
        _persist_hub_state()
    except Exception as e:
        if serve_op:
            await _capture_load_diagnostics(c, serve_op["op_id"], "load_task_exception", include_node_logs=True)
        _kill(c.get("master"))
        c["master"] = None
        c.update(phase="error", detail=str(e))
        _record_ctrl_op(c, "load", "error", "error", 0.0,
                        "serve failed", model=req.model, error=str(e))
        LOG.exception("linkcpp_event %s", json.dumps({
            "event": "load_task_exception",
            "controller_id": c.get("id"),
            "model": getattr(req, "model", None),
            "error": str(e),
        }, sort_keys=True, default=str))
        _cancel_load_monitors(c)
    finally:
        c.pop("load_cancel", None)
        c.pop("load_task", None)
        c.pop("pending_load", None)


async def _wait_health(port, c, op_id=None, secs=900, expected_load_bytes=0):
    started = time.time()
    last_report = 0.0
    last_diagnostic = 0.0
    last_transfer_bytes = _load_observed_transfer_bytes(c, op_id) if op_id else 0
    last_transfer_at = started
    last_progress_at = started
    observed_rate = 0.0
    async with httpx.AsyncClient(timeout=3) as cl:
        while True:
            if _load_cancel_requested(c):
                _log_event("master_health_wait_canceled", controller_id=c.get("id"),
                           port=port, elapsed_s=round(time.time() - started, 3))
                raise LoadCancelled((c.get("load_cancel") or {}).get("reason") or "load canceled")
            if c.get("master") is not None and c["master"].poll() is not None:
                _log_event("master_health_wait_exit", controller_id=c.get("id"),
                           port=port, elapsed_s=round(time.time() - started, 3),
                           returncode=c["master"].poll(), log_tail=tail(_master_log_path(c), 80))
                return False
            log_tail = tail(_master_log_path(c), 80)
            if _master_startup_fatal(log_tail):
                _log_event("master_health_wait_fatal", controller_id=c.get("id"),
                           port=port, elapsed_s=round(time.time() - started, 3),
                           log_tail=log_tail)
                return False
            try:
                if (await cl.get(f"http://127.0.0.1:{port}/health")).status_code == 200:
                    _log_event("master_health_wait_ok", controller_id=c.get("id"),
                               port=port, elapsed_s=round(time.time() - started, 3))
                    return True
            except Exception:
                pass
            now = time.time()
            if op_id and now - last_diagnostic >= 15.0:
                last_diagnostic = now
                try:
                    await _capture_load_diagnostics(c, op_id, "health_wait", include_node_logs=False)
                except Exception as exc:
                    _log_event("load_diagnostic_snapshot_failed", controller_id=c.get("id"),
                               op_id=op_id, error=str(exc))
            observed_bytes = _load_observed_transfer_bytes(c, op_id) if op_id else 0
            if observed_bytes > last_transfer_bytes:
                elapsed = max(0.001, now - last_transfer_at)
                instant_rate = (observed_bytes - last_transfer_bytes) / elapsed
                observed_rate = instant_rate if observed_rate <= 0 else (observed_rate * 0.7 + instant_rate * 0.3)
                last_transfer_bytes = observed_bytes
                last_transfer_at = now
                last_progress_at = now
            stall_timeout_s = _load_stall_timeout_s(
                expected_load_bytes, observed_bytes, observed_rate, secs)
            if now - last_progress_at >= stall_timeout_s:
                _log_event("master_health_wait_stalled", controller_id=c.get("id"), port=port,
                           elapsed_s=round(now - started, 3), idle_s=round(now - last_progress_at, 3),
                           observed_bytes=observed_bytes, bytes_per_s=round(observed_rate, 2),
                           stall_timeout_s=stall_timeout_s, log_tail=log_tail)
                return False
            if op_id and now - last_report >= 30.0:
                last_report = now
                _record_ctrl_op(
                    c, "load", "master_loading", "running", 45.0,
                    "waiting for llama-server health",
                    op_id=op_id, model=c.get("model"),
                    details={"master_log_tail": log_tail,
                             "elapsed_s": round(now - started, 3),
                             "timeout_s": stall_timeout_s,
                             "timeout_mode": "progress_renewed",
                             "observed_transfer_bytes": observed_bytes,
                             "observed_transfer_mib_s": round(observed_rate / (1024 ** 2), 3),
                             "last_progress_age_s": round(now - last_progress_at, 3)})
            await asyncio.sleep(2)


@app.post("/api/controllers/{cid}/serve")
async def api_ctrl_serve(cid: str, req: ServeReq):
    existing = CTRLS.get(cid)
    if existing and _ctrl_phase(existing) == "loading":
        raise HTTPException(409, "load already in progress; cancel it before starting another load")
    _ensure_local_slots()
    c = CTRLS.get(cid)
    if not c:
        raise HTTPException(404, "unknown controller")
    if _ctrl_phase(c) == "loading":
        raise HTTPException(409, "load already in progress; cancel it before starting another load")
    c.pop("load_diagnostic_dir", None)
    _clear_ctrl_activity(c, "new_load", req.model)
    _reset_inference_activity(c, "new_load")
    c["ctx"] = req.ctx
    c["parallel"] = req.parallel
    _persist_hub_state()
    _log_event("load_request_received", controller_id=cid, model=req.model,
               ctx=req.ctx, parallel=req.parallel, kv_bits=req.kv_bits,
               cache_type_k=req.cache_type_k, cache_type_v=req.cache_type_v, runtime_mode=req.runtime_mode,
               nodes=list(c.get("nodes", [])))
    result = _do_plan(c, req)
    c["plan"] = result
    _log_event("load_plan_result", controller_id=cid, model=req.model,
               plan=_plan_log_summary(result))
    if not result["feasible"]:
        _record_ctrl_op(c, "plan", "infeasible", "error", 0.0,
                        result.get("reason", "cannot place model"),
                        model=result.get("model_ref", req.model),
                        details={"plan": _plan_log_summary(result)})
        _record_ctrl_op(c, "load", "blocked", "error", 0.0,
                        "infeasible: " + result.get("reason", "cannot place model"),
                        model=result.get("model_ref", req.model),
                        details={"plan": result})
        _log_event("load_rejected", controller_id=cid, model=req.model,
                   error="infeasible", plan=_plan_log_summary(result))
        return JSONResponse(status_code=400, content={"error": "infeasible", "plan": result})
    if not result.get("adaptive_load_available", True):
        _record_ctrl_op(c, "load", "blocked", "error", 0.0,
                        result.get("adaptive_load_blocker", "adaptive load unavailable"),
                        model=result.get("model_ref", req.model),
                        details={"plan": result})
        _log_event("load_rejected", controller_id=cid, model=req.model,
                   error="adaptive_load_unavailable", plan=_plan_log_summary(result))
        return JSONResponse(status_code=400, content={
            "error": "adaptive_load_unavailable",
            "detail": result.get("adaptive_load_blocker", "adaptive load unavailable"),
            "plan": result,
        })
    if req.runtime_mode != LLAMA_RPC:
        data_plane = result.get("data_plane") or data_plane_contract(RING_PROXY, [])
        if not data_plane.get("available"):
            detail = data_plane.get("blocker") or "selected runtime is not available"
            _record_ctrl_op(c, "load", "runtime_mode_blocked", "error", 0.0, detail,
                            model=result.get("model_ref", req.model),
                            details={"runtime_mode": req.runtime_mode, "data_plane": data_plane})
            return JSONResponse(status_code=501, content={"error": "runtime_mode_unavailable", "detail": detail,
                "runtime_mode": req.runtime_mode, "data_plane": data_plane, "plan": result})
    nids = c["nodes"]
    active = [(nids[p["node"]], p) for p in result["placement"] if p["n_layers"]]
    remote_checks = await _check_remote_unit_nodes_ready(c, req, active)
    if any(not item.get("ready") for item in remote_checks):
        detail = _remote_unit_block_message(remote_checks)
        _record_ctrl_op(c, "load", "remote_unit_not_ready", "error", 0.0,
                        detail, model=result.get("model_ref", req.model),
                        details={"remote_unit_checks": remote_checks})
        _log_event("load_rejected", controller_id=cid, model=req.model,
                   error="remote_unit_not_ready", remote_unit_checks=remote_checks)
        return JSONResponse(status_code=409, content={
            "error": "remote_unit_not_ready",
            "detail": detail,
            "remote_unit_checks": remote_checks,
            "plan": result,
        })
    c["pending_load"] = _serve_req_snapshot(req)
    c["load_cancel"] = {"requested": False, "reason": "", "ts": None}
    c["phase"] = "loading"
    c["detail"] = "load queued"
    c["model"] = req.model
    loader = (_serve_llama_rpc(c, req, result, active) if req.runtime_mode == LLAMA_RPC
              else serve_selected_runtime(req.runtime_mode, c, req, result, active))
    c["load_task"] = asyncio.create_task(loader)
    return {"accepted": req.model, "phase": "loading", "plan": result}


@app.post("/api/controllers/{cid}/load")
async def api_ctrl_load(cid: str, req: ServeReq):
    return await api_ctrl_serve(cid, req)


@app.post("/api/controllers/{cid}/load/cancel")
async def api_ctrl_cancel_load(cid: str, req: CancelLoadRequest = None):
    c = CTRLS.get(cid)
    if not c:
        raise HTTPException(404, "unknown controller")
    if _ctrl_phase(c) != "loading":
        raise HTTPException(409, "no load is in progress")
    req = req or CancelLoadRequest()
    token = c.setdefault("load_cancel", {})
    token.update({"requested": True, "reason": req.reason or "requested", "ts": time.time()})
    c["detail"] = "canceling load"
    op = _record_ctrl_op(c, "load", "cancel_requested", "running", 0.0,
                         "cancel requested", op_id=req.op_id, model=c.get("model"),
                         details={"reason": req.reason})
    _log_event("load_cancel_requested", controller_id=cid, model=c.get("model"),
               reason=req.reason, op_id=op["op_id"])
    nodes = await _cancel_load_runtime(c, req.reason)
    task = c.get("load_task")
    if not task or task.done():
        c.update(model=None, phase="idle", detail="")
        c.pop("load_cancel", None)
        c.pop("load_task", None)
        c.pop("pending_load", None)
        _record_ctrl_op(c, "load", "canceled", "canceled", 0.0,
                        "load canceled", op_id=op["op_id"], model=c.get("model"),
                        details={"nodes": nodes})
        _persist_hub_state()
    return {"canceling": True, "op_id": op["op_id"], "nodes": nodes}


@app.get("/api/controllers/{cid}/status")
def api_ctrl_status(cid: str):
    _ensure_local_slots()
    c = CTRLS.get(cid)
    if not c:
        raise HTTPException(404, "unknown controller")
    ph = _ctrl_phase(c)
    # A ring controller has no hub-local master (its coordinator runs on the first
    # ring node), so "loaded" is gated on the ring being up (proxy_first_node set),
    # not on a local master process — otherwise running ring models look unloaded
    # and never surface to the pay gateway / model list.
    runtime_loaded = ph in ("running", "error") and (
        c.get("proxy_first_node") is not None or _proc_alive(c.get("master")))
    can_unload = runtime_loaded or ph in ("running", "unloading", "error")
    return {"phase": ph, "detail": c["detail"], "serving": c["model"] if ph in ("loading", "running") else None,
            "active_model": c["model"], "can_unload": can_unload,
            "can_cancel_load": ph == "loading", "runtime_loaded": runtime_loaded,
            "last_load": c.get("last_load") or {},
            "running": ph == "running", "parallel": c["parallel"], "plan": c["plan"],
            "nodes": len(c["nodes"]), "operations": list(_ctrl_ops(c).values())[-50:]}


@app.get("/api/controllers/{cid}/operations")
def api_ctrl_operations(cid: str):
    _ensure_local_slots()
    c = CTRLS.get(cid)
    if not c:
        raise HTTPException(404, "unknown controller")
    return {"operations": list(_ctrl_ops(c).values())[-100:]}


@app.get("/api/controllers/{cid}/inference-activity")
def api_ctrl_inference_activity(cid: str):
    _ensure_local_slots()
    c = CTRLS.get(cid)
    if not c:
        raise HTTPException(404, "unknown controller")
    return _inference_snapshot(c)


@app.get("/api/controllers/{cid}/inference-history")
def api_ctrl_inference_history(cid: str, limit: int = Query(20, ge=1, le=100),
                               detail: bool = Query(False)):
    _ensure_local_slots()
    c = CTRLS.get(cid)
    if not c:
        raise HTTPException(404, "unknown controller")
    return {"history": _load_inference_history(c, limit=limit, include_detail=detail),
            "archive_dir": os.path.join(INFERENCE_ARCHIVE_DIR, _safe_slug(c.get("id")))}


@app.get("/api/controllers/{cid}/logs")
def api_ctrl_logs(cid: str, tail_lines: int = Query(200, alias="tail")):
    _ensure_local_slots()
    c = CTRLS.get(cid)
    if not c:
        raise HTTPException(404, "unknown controller")
    lines = max(20, min(1000, int(tail_lines or 200)))
    log = tail(_master_log_path(c), lines)
    return {
        "controller_id": cid,
        "phase": _ctrl_phase(c),
        "master_port": c.get("master_port"),
        "master_running": _proc_alive(c.get("master")),
        "log_path": _master_log_path(c),
        "log": log,
        "master_progress": _parse_master_progress(log),
        "inference_activity": _inference_snapshot(c),
    }


@app.get("/api/controllers/{cid}/load-diagnostics")
def api_ctrl_load_diagnostics(cid: str):
    """List persisted diagnostic bundles for a controller's load attempts."""
    c = CTRLS.get(cid)
    if not c:
        raise HTTPException(404, "unknown controller")
    root = os.path.join(LOAD_DIAGNOSTICS_DIR, _safe_slug(cid))
    bundles = []
    for path in sorted(glob.glob(os.path.join(root, "*")), reverse=True):
        if not os.path.isdir(path):
            continue
        bundles.append({
            "id": os.path.basename(path),
            "path": path,
            "files": sorted(os.path.relpath(p, path) for p in glob.glob(os.path.join(path, "**", "*"), recursive=True) if os.path.isfile(p)),
        })
    return {"controller_id": cid, "active": c.get("load_diagnostic_dir"), "bundles": bundles[:20]}


@app.get("/api/controllers/{cid}/load-diagnostics/{bundle}/{artifact:path}")
def api_ctrl_load_diagnostic_file(cid: str, bundle: str, artifact: str):
    root = os.path.abspath(os.path.join(LOAD_DIAGNOSTICS_DIR, _safe_slug(cid), _safe_slug(bundle)))
    target = os.path.abspath(os.path.join(root, artifact))
    if os.path.commonpath((root, target)) != root or not os.path.isfile(target):
        raise HTTPException(404, "diagnostic artifact not found")
    return FileResponse(target, filename=os.path.basename(target))


async def _unload_ctrl(c, reason="requested"):
    _reset_inference_activity(c, "unload")
    runtime_mode = normalize_runtime_mode((c.get("plan") or {}).get("runtime_mode"))
    if runtime_mode != LLAMA_RPC:
        c.update(phase="unloading", detail="stopping selected runtime")
        return await unload_selected_runtime(runtime_mode, c, reason)
    c.update(phase="unloading", detail="stopping master")
    op = _record_ctrl_op(c, "unload", "unloading", "running", 10.0, "stopping master",
                         model=c.get("model"), details={"reason": reason})
    _kill(c.get("master")); c["master"] = None
    unload_results = []
    node_ids = list(c["nodes"])
    total = max(1, len(node_ids))
    for idx, nid in enumerate(node_ids, start=1):
        n = NODES.get(nid)
        if not n:
            unload_results.append({"node_id": nid, "status": "missing", "message": "node not found"})
            continue
        node_name = n.get("name") or nid
        node_op_id = f"{op['op_id']}-{nid}"
        progress = 10.0 + (idx - 1) / total * 80.0
        _record_ctrl_op(c, "unload", "node_unloading", "running", progress,
                        f"unloading {node_name}", op_id=node_op_id, node_id=nid,
                        model=c.get("model"), details={"node_kind": n.get("kind", "local")})
        if n.get("kind") == "local":
            _kill(n.get("worker")); n["worker"] = None
            n["worker_running"] = False
            n["desired_load"] = None
            n["ram_used"] = 0.0
            result = {"node_id": nid, "name": node_name, "kind": "local", "status": "done",
                      "message": "local worker stopped"}
            _record_ctrl_op(c, "unload", "node_unloaded", "done", progress + 80.0 / total,
                            result["message"], op_id=node_op_id, node_id=nid, model=c.get("model"))
        elif n.get("kind") == "agent":
            try:
                await _stop_agent_worker_confirmed(
                    n, "/control/unload",
                    model_to_dict(UnloadRequest(op_id=f"{op['op_id']}-{nid}", reason=reason)))
                n["ram_used"] = 0.0
                result = {"node_id": nid, "name": node_name, "kind": "agent", "status": "done",
                          "message": "agent RPC worker exit confirmed"}
                _record_ctrl_op(c, "unload", "node_unloaded", "done", progress + 80.0 / total,
                                result["message"], op_id=node_op_id, node_id=nid, model=c.get("model"))
            except Exception as exc:
                result = {"node_id": nid, "name": node_name, "kind": "agent", "status": "error",
                          "message": str(exc)}
                _record_ctrl_op(c, "unload", "node_unload_error", "error", 50.0,
                                "node unload failed", node_id=nid, error=str(exc))
        elif n.get("kind") == "remote_unit_node":
            try:
                await _stop_remote_unit_worker(n, reason)
                n["worker_running"] = False
                n["desired_load"] = None
                n["ram_used"] = 0.0
                result = {"node_id": nid, "name": node_name, "kind": "remote_unit_node", "status": "done",
                          "message": "remote unit worker stopped"}
                _record_ctrl_op(c, "unload", "node_unloaded", "done", progress + 80.0 / total,
                                result["message"], op_id=node_op_id, node_id=nid, model=c.get("model"))
            except Exception as exc:
                result = {"node_id": nid, "name": node_name, "kind": "remote_unit_node", "status": "error",
                          "message": str(exc)}
                _record_ctrl_op(c, "unload", "node_unload_error", "error", 50.0,
                                "remote unit worker stop failed", node_id=nid, error=str(exc))
        else:
            result = {"node_id": nid, "name": node_name, "kind": n.get("kind", "remote"),
                      "status": "skipped", "message": "node is not managed by this controller"}
            _record_ctrl_op(c, "unload", "node_skipped", "done", progress + 80.0 / total,
                            result["message"], op_id=node_op_id, node_id=nid, model=c.get("model"))
        unload_results.append(result)
    failed = [item for item in unload_results if item.get("status") == "error"]
    if failed:
        _record_ctrl_op(c, "unload", "unload_incomplete", "error", 100.0,
                        "unload incomplete; one or more workers remain active",
                        op_id=op["op_id"], model=c.get("model"), details={"nodes": unload_results})
        c.update(phase="error", detail="unload incomplete; retry after checking failed nodes")
        _persist_hub_state()
        return {"stopped": False, "nodes": unload_results,
                "operations": list(_ctrl_ops(c).values())[-50:],
                "cleared": {"plan": False, "operations": False}}
    _record_ctrl_op(c, "unload", "unloaded", "done", 100.0, "unload complete",
                    op_id=op["op_id"], model=c.get("model"))
    final_operations = list(_ctrl_ops(c).values())[-50:]
    c.update(model=None, phase="idle", detail="", plan=None)
    c["operations"] = {}
    _persist_hub_state()
    return {"stopped": True, "nodes": unload_results, "operations": final_operations,
            "cleared": {"plan": True, "operations": True}}


@app.post("/api/controllers/{cid}/unload")
async def api_ctrl_unload(cid: str, req: UnloadRequest = None):
    c = CTRLS.get(cid)
    if not c:
        raise HTTPException(404, "unknown controller")
    req = req or UnloadRequest()
    return await _unload_ctrl(c, req.reason)


@app.post("/api/controllers/{cid}/stop")
async def api_ctrl_stop(cid: str):
    c = CTRLS.get(cid)
    if not c:
        raise HTTPException(404, "unknown controller")
    return await _unload_ctrl(c, "stop")


# ------------------------------- gateway (per controller) -----------------
async def _master_chat(port, payload):
    async with httpx.AsyncClient(timeout=None) as c:
        r = await c.post(f"http://127.0.0.1:{port}/v1/chat/completions", json=payload)
        r.raise_for_status()
        return r.json()


async def _read_json_body(req):
    try:
        return await req.json()
    except json.JSONDecodeError as exc:
        raise HTTPException(400, f"invalid JSON body: {exc.msg}") from exc


def _normalize_generation_limits(body):
    normalized = dict(body)
    requested = normalized.get("max_tokens")
    key = "max_tokens"
    if requested is None and "max_output_tokens" in normalized:
        requested = normalized.get("max_output_tokens")
        key = "max_output_tokens"
    try:
        requested_int = int(requested) if requested is not None else DEFAULT_COMPLETION_TOKENS
    except Exception:
        requested_int = DEFAULT_COMPLETION_TOKENS
    max_allowed = max(1, MAX_COMPLETION_TOKENS)
    effective = max(1, min(requested_int, max_allowed))
    normalized[key] = effective
    limit_info = {
        "requested_max_tokens": requested,
        "effective_max_tokens": effective,
        "max_completion_tokens": max_allowed,
        "capped": requested_int != effective,
    }
    return normalized, limit_info


def _need_running(cid):
    c = CTRLS.get(cid)
    if not c:
        raise HTTPException(404, "unknown controller")
    _require_controller_runtime(c)
    if _ctrl_phase(c) != "running":
        raise HTTPException(503, f"model not ready (phase: {c['phase']} {c['detail']})")
    return c


def _need_controller(cid):
    c = CTRLS.get(cid)
    if not c:
        raise HTTPException(404, "unknown controller")
    return c


def _controller_served_model(c):
    return c.get("model") if _ctrl_phase(c) == "running" else None


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


def _json_safe(value):
    try:
        json.dumps(value)
        return value
    except Exception:
        return str(value)


def _node_archive_snapshot(c):
    rows = []
    for nid in c.get("nodes", []):
        n = NODES.get(nid)
        if not n:
            rows.append({"id": nid, "missing": True})
            continue
        rows.append({
            "id": n.get("id"),
            "name": n.get("name"),
            "kind": n.get("kind", "local"),
            "gpu_name": n.get("gpu_name"),
            "vram_budget_gib": n.get("vram"),
            "ram_budget_gib": n.get("ram"),
            "rpc_endpoint": _node_rpc_endpoint(n),
            "remote_unit_id": n.get("remote_unit_id"),
            "remote_unit_name": n.get("remote_unit_name"),
            "remote_unit_url": n.get("remote_unit_url"),
            "remote_controller_id": n.get("remote_controller_id"),
            "remote_controller_name": n.get("remote_controller_name"),
            "remote_source_node_id": n.get("remote_source_node_id"),
            "remote_source_rpc_endpoint": n.get("remote_source_rpc_endpoint"),
            "runtime": _node_runtime(n),
            "backend": _node_backend(n),
        })
    return rows


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


def _archive_path_for_inference(c, item):
    stamp = time.strftime("%Y%m%dT%H%M%SZ", time.gmtime(item.get("created_at", time.time())))
    day = time.strftime("%Y%m%d", time.gmtime(item.get("created_at", time.time())))
    name = f"{stamp}_seq-{item.get('seq', 0):04d}_{_safe_slug(item.get('kind'))}_{_safe_slug(item.get('id'))}.json"
    return os.path.join(INFERENCE_ARCHIVE_DIR, _safe_slug(c.get("id")), day, name)


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


def _record_stream_chunk(c, item, raw):
    text = raw.decode("utf-8", "replace") if isinstance(raw, (bytes, bytearray)) else str(raw)
    buf = item.get("sse_buffer", "") + text
    lines = buf.splitlines(keepends=True)
    item["sse_buffer"] = ""
    for line in lines:
        if not line.endswith("\n") and not line.endswith("\r"):
            item["sse_buffer"] += line
            continue
        line = line.strip()
        if not line.startswith("data:"):
            continue
        data = line[5:].strip()
        if not data or data == "[DONE]":
            continue
        try:
            chunk = json.loads(data)
        except Exception:
            continue
        choice = (chunk.get("choices") or [{}])[0]
        delta = ((choice.get("delta") or {}).get("content")
                 or (choice.get("message") or {}).get("content") or "")
        if delta:
            item["output_events"] = int(item.get("output_events") or 0) + 1
            item["output_chars"] = int(item.get("output_chars") or 0) + len(delta)
        if choice.get("finish_reason"):
            item["finish_reason"] = choice.get("finish_reason")
        if chunk.get("usage"):
            item["usage"] = chunk.get("usage") or {}
        if chunk.get("timings"):
            item["timings"] = chunk.get("timings") or {}
        _sample_inference(c, item, usage=item.get("usage"), timings=item.get("timings"),
                          finish_reason=item.get("finish_reason"))


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


def _persist_inference_record(c, item, status, message, error=None, usage=None, timings=None,
                              finish_reason=None, limits=None):
    metrics = _sample_inference(c, item, usage=usage, timings=timings, finish_reason=finish_reason)
    finished = time.time()
    path = _archive_path_for_inference(c, item)
    active = []
    plan = c.get("plan") or {}
    for p in _plan_placements(c):
        if p.get("node_id"):
            active.append((p.get("node_id"), p))
    record = {
        "schema_version": 1,
        "request_id": item.get("id"),
        "seq": item.get("seq"),
        "kind": item.get("kind"),
        "status": status,
        "message": message,
        "error": error,
        "created_at": item.get("created_at"),
        "started_at": item.get("started_at"),
        "finished_at": finished,
        "duration_s": round(finished - item.get("created_at", finished), 3),
        "controller": {
            "id": c.get("id"),
            "name": c.get("name"),
            "parallel": c.get("parallel"),
            "master_port": c.get("master_port"),
        },
        "model": c.get("model"),
        "request": {
            "stream": bool(item.get("stream")),
            "prompt_chars": item.get("prompt_chars"),
            "max_tokens": item.get("max_tokens"),
            "limits": limits or item.get("limits") or {},
        },
        "plan": plan,
        "nodes": _node_archive_snapshot(c),
        "rpc_topology": _rpc_topology(c, active) if active else [],
        "usage": usage or item.get("usage") or {},
        "timings": timings or item.get("timings") or {},
        "finish_reason": finish_reason or item.get("finish_reason"),
        "metrics": metrics,
        "samples": item.get("samples") or [],
        "archive_path": path,
    }
    try:
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w", encoding="utf-8") as f:
            json.dump(_json_safe(record), f, ensure_ascii=False, indent=2, sort_keys=True)
        item["record_path"] = path
        state = _inference_tracker(c)
        history = state.setdefault("history", [])
        history.insert(0, _inference_record_summary(record, include_detail=True))
        del history[50:]
        _log_event("inference_record_persisted", controller_id=c.get("id"),
                   request_id=item.get("id"), path=path, status=status,
                   output_tokens=metrics.get("output_tokens"), tps=metrics.get("tps"))
    except Exception as exc:
        _log_event("inference_record_persist_failed", controller_id=c.get("id"),
                   request_id=item.get("id"), path=path, error=str(exc))
    return path


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


def _new_inference_item(c, kind, body, stream=False):
    state = _inference_tracker(c)
    state["seq"] += 1
    item = {
        "id": "inference-" + uuid.uuid4().hex[:10],
        "seq": state["seq"],
        "kind": kind,
        "status": "queued",
        "phase": "queued",
        "progress": 0.0,
        "message": f"{kind} request queued",
        "model": c.get("model"),
        "stream": bool(stream),
        "created_at": time.time(),
        "updated_at": time.time(),
        "slot_acquired": False,
        "prompt_chars": _payload_size_hint(body),
        "max_tokens": body.get("max_tokens") or body.get("max_output_tokens"),
        "output_events": 0,
        "output_chars": 0,
        "samples": [],
        "metrics": {},
        "plan_snapshot": c.get("plan") or {},
        "node_snapshot": _node_archive_snapshot(c),
    }
    state["items"][item["id"]] = item
    _log_event("inference_request_queued", controller_id=c.get("id"), request_id=item["id"],
               kind=kind, parallel_limit=state["limit"], stream=bool(stream))
    return item


def _payload_size_hint(body):
    try:
        return len(json.dumps(body.get("messages") or body.get("input") or "", ensure_ascii=False))
    except Exception:
        return 0


async def _acquire_inference_slot(c, item):
    state = _inference_tracker(c)
    # Creating an asyncio primitive while rendering a synchronous API response
    # fails on Python 3.9 when no event loop is installed. Create it only in
    # the request coroutine that will own and use it.
    if state.get("semaphore") is None:
        state["semaphore"] = asyncio.Semaphore(state["limit"])
    await state["semaphore"].acquire()
    item["slot_acquired"] = True
    item.update(status="running", phase="master_request", progress=20.0,
                message="forwarded to llama-server master", started_at=time.time(),
                updated_at=time.time())
    _log_event("inference_request_started", controller_id=c.get("id"), request_id=item["id"],
               kind=item.get("kind"), queue_wait_s=round(item["started_at"] - item["created_at"], 3))


def _mark_inference_streaming(c, item):
    item.update(status="streaming", phase="streaming", progress=50.0,
                message="streaming response from master", updated_at=time.time())
    _log_event("inference_request_streaming", controller_id=c.get("id"), request_id=item["id"])


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


def _window_scarcity_multiplier(model, layers):
    """Reward for covering a thin segment: 1.0 when a node's window is at/above
    the target replica count, rising toward SHARD_SCARCITY_MAX as coverage of
    that window gets scarcer. Uses the live coverage map, so the sole coverer of
    an otherwise-uncovered segment earns the most."""
    if not SHARD_SCARCITY_REWARD or not layers or len(layers) != 2:
        return 1.0
    name = os.path.basename(str(model or ""))
    mid = (int(layers[0]) + int(layers[1])) // 2
    for entry in _shard_coverage():
        if os.path.basename(entry["model"]) != name:
            continue
        for seg in entry["segments"]:
            if seg["layers"][0] <= mid < seg["layers"][1]:
                mult = 1.0 + SHARD_SCARCITY_ALPHA * seg["scarcity"]
                return round(min(SHARD_SCARCITY_MAX, mult), 3)
    return 1.0


def _credit_node_contribution(c, item):
    """On a completed inference, split the output tokens across the participating
    nodes by their layer share and accumulate per-node contribution units. Each node
    is credited to its own owner wallet if it reported one, else the hub operator.
    With LINKCPP_SHARD_SCARCITY_REWARD, units are scaled by a scarcity multiplier
    so nodes that fill thin shard segments earn more."""
    tokens = _usage_output_tokens(item.get("usage"))
    if tokens is None:
        tokens = (item.get("metrics") or {}).get("output_tokens")
    tokens = int(tokens or 0)
    if tokens <= 0:
        return
    placements = _plan_placements(c)
    total = sum(int(p.get("n_layers") or 0) for p in placements)
    if total <= 0:
        return
    tps = (item.get("metrics") or {}).get("tps")
    model = c.get("model") or c.get("serving")
    for p in placements:
        nid = p.get("node_id")
        nlay = int(p.get("n_layers") or 0)
        if not nid or nlay <= 0:
            continue
        node = _placement_node(c, p) or {}
        owner = (node.get("owner") or "").strip() or OPERATOR_WALLET
        if not owner:
            continue  # node has no owner and no hub operator wallet => can't credit
        backend = node.get("backend")
        backend_kind = backend.get("backend_kind") if isinstance(backend, dict) else backend
        scarcity_mult = _window_scarcity_multiplier(model, p.get("layers"))
        rec = CONTRIBUTIONS.setdefault(nid, {"node_id": nid, "units": 0.0})
        # layer-weighted (1 unit == 1k tokens), optionally scaled by shard scarcity
        rec["units"] += (tokens / 1000.0) * (nlay / total) * scarcity_mult
        rec["scarcity_multiplier"] = scarcity_mult
        rec["owner"] = owner
        rec["model"] = model
        rec["node_name"] = p.get("node_name") or node.get("name") or nid
        rec["backend"] = backend_kind
        rec["os"] = (node.get("host_platform") or {}).get("system") or node.get("os")
        rec["accelerator"] = "gpu"
        rec["device_kind"] = node.get("kind") or "node"
        if tps is not None:
            rec["perf_tps"] = tps


@app.get("/api/contributions")
def api_contributions():
    """Cumulative per-node inference contribution for the settlement gateway to poll
    and credit. Behind the auth gate (session or M2M service token)."""
    return {"contributions": list(CONTRIBUTIONS.values())}


def _finish_inference(c, item, status="done", message="complete", error=None, usage=None,
                      timings=None, finish_reason=None, limits=None):
    if item.get("slot_acquired"):
        state = _inference_tracker(c)
        state["semaphore"].release()
        item["slot_acquired"] = False
    item.update(status=status, phase="complete" if status == "done" else "error",
                progress=100.0 if status == "done" else 0.0,
                message=message, error=error, usage=usage or item.get("usage") or {},
                timings=timings or item.get("timings") or {},
                finish_reason=finish_reason or item.get("finish_reason"),
                updated_at=time.time())
    _persist_inference_record(c, item, status, message, error=error, usage=item.get("usage"),
                              timings=item.get("timings"), finish_reason=item.get("finish_reason"),
                              limits=limits)
    if status == "done":
        try:
            _credit_node_contribution(c, item)
        except Exception:
            logging.getLogger("linkcpp").warning("node contribution credit failed", exc_info=True)
    duration = round(item["updated_at"] - item["created_at"], 3)
    _log_event("inference_request_finished", controller_id=c.get("id"), request_id=item["id"],
               status=status, duration_s=duration, error=error,
               tps=(item.get("metrics") or {}).get("tps"),
               output_tokens=(item.get("metrics") or {}).get("output_tokens"),
               record_path=item.get("record_path"))
    state = _inference_tracker(c)
    state["items"].pop(item["id"], None)


@app.get("/c/{cid}/v1/models")
async def gw_openai_models(cid: str):
    c = _need_controller(cid)
    return _openai_model_list(_controller_served_model(c))


@app.post("/c/{cid}/v1/chat/completions")
async def gw_chat(cid: str, req: Request):
    c = _need_running(cid)
    raw_body = await _read_json_body(req)
    body, limits = _normalize_generation_limits(raw_body)
    item = _new_inference_item(c, "chat", body, stream=bool(body.get("stream")))
    item["limits"] = limits
    inf_id = item["id"]
    _record_ctrl_op(c, "inference", "queued", "running", 0.0, "chat request queued",
                    op_id=inf_id, model=c.get("model"),
                    details={"nodes": c["nodes"], "stream": bool(body.get("stream")), **limits})
    _log_event("inference_gateway_request", controller_id=c.get("id"), request_id=inf_id,
               kind="chat", **_request_log_fields(body), **limits)
    if body.get("stream"):
        async def gen():
            started = time.time()
            try:
                await _acquire_inference_slot(c, item)
                _mark_inference_streaming(c, item)
                _record_ctrl_op(c, "inference", "streaming", "running", 20.0,
                                "streaming chat request", op_id=inf_id, model=c.get("model"))
                async for line in runtime_stream_chat(c, body, request_id=inf_id):
                    _record_stream_chunk(c, item, line)
                    yield line
                _record_ctrl_op(c, "inference", "complete", "done", 100.0,
                                "stream complete", op_id=inf_id, model=c.get("model"),
                                details={"duration_s": round(time.time() - started, 3),
                                         "usage": item.get("usage", {}),
                                         "timings": item.get("timings", {}),
                                         "finish_reason": item.get("finish_reason"),
                                         "metrics": item.get("metrics", {}), **limits})
                _finish_inference(c, item, "done", "stream complete",
                                  usage=item.get("usage"), timings=item.get("timings"),
                                  finish_reason=item.get("finish_reason"), limits=limits)
            except asyncio.CancelledError:
                _record_ctrl_op(c, "inference", "canceled", "canceled", 0.0,
                                "stream client disconnected", op_id=inf_id, model=c.get("model"),
                                details={"metrics": item.get("metrics", {}), **limits})
                _finish_inference(c, item, "canceled", "stream client disconnected",
                                  error="client disconnected", usage=item.get("usage"),
                                  timings=item.get("timings"), finish_reason=item.get("finish_reason"),
                                  limits=limits)
                raise
            except Exception as exc:
                _record_ctrl_op(c, "inference", "error", "error", 0.0,
                                "stream failed", op_id=inf_id, model=c.get("model"), error=str(exc),
                                details={"metrics": item.get("metrics", {}), **limits})
                _finish_inference(c, item, "error", "stream failed", error=str(exc),
                                  usage=item.get("usage"), timings=item.get("timings"),
                                  finish_reason=item.get("finish_reason"), limits=limits)
                raise
        return StreamingResponse(gen(), media_type="text/event-stream")
    started = time.time()
    try:
        await _acquire_inference_slot(c, item)
        out = await runtime_chat(c, body, request_id=inf_id)
        choice = (out.get("choices") or [{}])[0]
        finish_reason = choice.get("finish_reason")
        timings = out.get("timings") or {}
        usage = out.get("usage", {})
        metrics = _sample_inference(c, item, usage=usage, timings=timings, finish_reason=finish_reason)
        _record_ctrl_op(c, "inference", "complete", "done", 100.0,
                        "chat complete", op_id=inf_id, model=c.get("model"),
                        details={"duration_s": round(time.time() - started, 3), "usage": usage,
                                 "timings": timings, "finish_reason": finish_reason,
                                 "metrics": metrics, **limits})
        _finish_inference(c, item, "done", "chat complete", usage=usage,
                          timings=timings, finish_reason=finish_reason, limits=limits)
        return JSONResponse(out)
    except Exception as exc:
        _record_ctrl_op(c, "inference", "error", "error", 0.0,
                        "chat failed", op_id=inf_id, model=c.get("model"), error=str(exc),
                        details={"metrics": item.get("metrics", {}), **limits})
        _finish_inference(c, item, "error", "chat failed", error=str(exc),
                          usage=item.get("usage"), timings=item.get("timings"),
                          finish_reason=item.get("finish_reason"), limits=limits)
        raise


@app.post("/c/{cid}/v1/responses")
async def gw_responses(cid: str, req: Request):
    c = _need_running(cid)
    body = await _read_json_body(req)
    item = _new_inference_item(c, "responses", body)
    inf_id = item["id"]
    _record_ctrl_op(c, "inference", "running", "running", 20.0,
                    "responses request running", op_id=inf_id, model=c.get("model"),
                    details={"nodes": c["nodes"]})
    msgs = []
    if body.get("instructions"):
        msgs.append({"role": "system", "content": body["instructions"]})
    inp = body.get("input", "")
    if isinstance(inp, str):
        msgs.append({"role": "user", "content": inp})
    else:
        for it in inp:
            ct = it.get("content", "")
            if isinstance(ct, list):
                ct = "".join(p.get("text", "") for p in ct)
            msgs.append({"role": it.get("role", "user"), "content": ct})
    started = time.time()
    try:
        await _acquire_inference_slot(c, item)
        chat = await runtime_chat(c, {"messages": msgs,
                "temperature": body.get("temperature", 1.0), "max_tokens": body.get("max_output_tokens", 256)},
                request_id=inf_id)
        choice = (chat.get("choices") or [{}])[0]
        text = choice["message"]["content"]
        usage = chat.get("usage", {})
        timings = chat.get("timings") or {}
        finish_reason = choice.get("finish_reason")
        metrics = _sample_inference(c, item, usage=usage, timings=timings, finish_reason=finish_reason)
        _record_ctrl_op(c, "inference", "complete", "done", 100.0,
                        "responses request complete", op_id=inf_id, model=c.get("model"),
                        details={"duration_s": round(time.time() - started, 3), "usage": usage,
                                 "timings": timings, "finish_reason": finish_reason, "metrics": metrics})
        _finish_inference(c, item, "done", "responses request complete", usage=usage,
                          timings=timings, finish_reason=finish_reason)
        return {"id": "resp_" + uuid.uuid4().hex, "object": "response", "created_at": int(time.time()),
                "model": c["model"], "status": "completed",
                "output": [{"id": "msg_" + uuid.uuid4().hex, "type": "message", "role": "assistant",
                            "content": [{"type": "output_text", "text": text}]}],
                "output_text": text, "usage": chat.get("usage", {})}
    except Exception as exc:
        _record_ctrl_op(c, "inference", "error", "error", 0.0,
                        "responses request failed", op_id=inf_id, model=c.get("model"), error=str(exc))
        _finish_inference(c, item, "error", "responses request failed", error=str(exc),
                          usage=item.get("usage"), timings=item.get("timings"),
                          finish_reason=item.get("finish_reason"))
        raise


@app.get("/c/{cid}/anthropic/v1/models")
async def gw_anthropic_models(cid: str):
    c = _need_controller(cid)
    return _anthropic_model_list(_controller_served_model(c))


@app.post("/c/{cid}/anthropic/v1/messages")
async def gw_anthropic(cid: str, req: Request):
    c = _need_running(cid)
    body = await _read_json_body(req)
    item = _new_inference_item(c, "anthropic", body)
    inf_id = item["id"]
    _record_ctrl_op(c, "inference", "running", "running", 20.0,
                    "anthropic request running", op_id=inf_id, model=c.get("model"),
                    details={"nodes": c["nodes"]})
    msgs = []
    if body.get("system"):
        msgs.append({"role": "system", "content": body["system"]})
    for m in body.get("messages", []):
        ct = m["content"]
        if isinstance(ct, list):
            ct = "".join(b.get("text", "") for b in ct if b.get("type") == "text")
        msgs.append({"role": m["role"], "content": ct})
    started = time.time()
    try:
        await _acquire_inference_slot(c, item)
        chat = await runtime_chat(c, {"messages": msgs,
                "max_tokens": body.get("max_tokens", 256), "temperature": body.get("temperature", 1.0)},
                request_id=inf_id)
        choice = (chat.get("choices") or [{}])[0]
        text = choice["message"]["content"]; u = chat.get("usage", {})
        timings = chat.get("timings") or {}
        finish_reason = choice.get("finish_reason")
        metrics = _sample_inference(c, item, usage=u, timings=timings, finish_reason=finish_reason)
        _record_ctrl_op(c, "inference", "complete", "done", 100.0,
                        "anthropic request complete", op_id=inf_id, model=c.get("model"),
                        details={"duration_s": round(time.time() - started, 3), "usage": u,
                                 "timings": timings, "finish_reason": finish_reason, "metrics": metrics})
        _finish_inference(c, item, "done", "anthropic request complete", usage=u,
                          timings=timings, finish_reason=finish_reason)
        return {"id": "msg_" + uuid.uuid4().hex, "type": "message", "role": "assistant", "model": c["model"],
                "content": [{"type": "text", "text": text}], "stop_reason": "end_turn",
                "usage": {"input_tokens": u.get("prompt_tokens", 0), "output_tokens": u.get("completion_tokens", 0)}}
    except Exception as exc:
        _record_ctrl_op(c, "inference", "error", "error", 0.0,
                        "anthropic request failed", op_id=inf_id, model=c.get("model"), error=str(exc))
        _finish_inference(c, item, "error", "anthropic request failed", error=str(exc),
                          usage=item.get("usage"), timings=item.get("timings"),
                          finish_reason=item.get("finish_reason"))
        raise


# ------------------------------- web UI -----------------------------------
@app.get("/")
def index():
    return FileResponse(os.path.join(WEB_DIR, "hub.html"))


app.mount("/web", StaticFiles(directory=WEB_DIR), name="web")
