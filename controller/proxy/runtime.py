"""linkcpp node-local adjacent-ring proxy runtime contract.

Everything specific to partial model loading, stage sockets, and the ring
request adapter belongs under this module/package boundary.
"""

from __future__ import annotations

import asyncio
import hashlib
import os
import time
import uuid
from pathlib import Path
from typing import Any, Iterable, Mapping

from controller.runtimes.base import RING_PROXY
from controller.proxy.protocol import StageInferRequest, StageStartRequest, UnloadRequest, model_to_dict
from controller.proxy.capability import compatibility, local_runtime_info
from controller.proxy import packs as runtime_packs
from controller.proxy.manifest import build_stage_manifests



def data_plane_contract(topology: Iterable[Mapping[str, Any]]) -> dict[str, Any]:
    stages = list(topology)
    info = local_runtime_info()
    contract = {
        "mode": RING_PROXY,
        "protocol": "linkcpp-stage-v1",
        "topology": "adjacent_ring",
        "weight_source": "node_local_gguf",
        "inference_payload": "versioned_graph_cut_set",
        "links": _adjacent_stage_links(stages),
        "available": bool(info.get("available")),
    }
    if not contract["available"]:
        detail = info.get("blocker") or "matching stage/server binaries are not installed"
        contract["blocker"] = f"proxy adapter is unavailable: {detail}"
    return contract


def _adjacent_stage_links(stages: list[Mapping[str, Any]]) -> list[dict[str, Any]]:
    if len(stages) < 2:
        return []
    pairs = list(zip(stages, stages[1:])) + [(stages[-1], stages[0])]
    return [
        {
            "upstream": left.get("node_id"),
            "downstream": right.get("node_id"),
            "upstream_endpoint": left.get("stage_endpoint"),
            "downstream_endpoint": right.get("stage_endpoint"),
        }
        for left, right in pairs
    ]


def enrich_topology_item(node: Mapping[str, Any], item: Mapping[str, Any]) -> dict[str, Any]:
    enriched = dict(item)
    endpoint = str(item.get("rpc_endpoint") or "")
    host, separator, _ = endpoint.rpartition(":")
    if not separator or not host:
        host = "127.0.0.1"
    rpc_port = int(node.get("rpc_port") or 50052)
    enriched["stage_endpoint"] = f"{host}:{int(node.get('stage_port') or rpc_port + 1000)}"
    return enriched


def catalog_entry() -> dict[str, Any]:
    info = local_runtime_info()
    entry = {
        "id": RING_PROXY,
        "label": "linkcpp Proxy",
        "stability": "preview",
        "available": bool(info.get("available")),
        "description": "Stock llama.cpp server orchestration over node-local adjacent graph cuts.",
        "protocol": info.get("protocol"),
        "adapter_abi": info.get("adapter_abi"),
        "build_id": info.get("build_id"),
        "state_snapshot": True,
        "chunked_state": True,
        "installed_packs": runtime_packs.list_packs(),
    }
    if not entry["available"]:
        entry["blocker"] = info.get("blocker") or "ring adapter is unavailable"
    return entry


def _role(index: int, count: int) -> str:
    if index == 0:
        return "first"
    if index == count - 1:
        return "last"
    return "middle"


async def _start_node(hub, node, request: StageStartRequest):
    if node.get("kind", "local") == "local":
        from controller.proxy.hub_api import start_local_stage
        return start_local_stage(node, request)
    if node.get("kind") == "agent":
        return await hub._agent_request(
            node, "POST", "/control/proxy/stage/start", model_to_dict(request), timeout=60,
        )
    if node.get("kind") == "remote_unit_node":
        source_id = node.get("remote_source_node_id") or ""
        return await hub._remote_unit_node_request(
            node, "POST", f"/api/proxy/nodes/{source_id}/stage/start",
            model_to_dict(request), timeout=60,
        )
    raise RuntimeError(f"node {node.get('id')} cannot host a proxy stage")


async def _node_runtime_capability(hub, node) -> dict[str, Any] | None:
    kind = node.get("kind", "local")
    if kind == "local":
        return catalog_entry()
    if kind == "agent":
        info = await hub._agent_request(node, "GET", "/control/proxy/runtime", timeout=15)
        return {
            **(info.get("runtime_mode") or {}),
            "installed_packs": info.get("installed_packs") or [],
            "install_enabled": bool(info.get("install_enabled")),
        }
    if kind == "remote_unit_node":
        info = await hub._remote_unit_node_request(node, "GET", "/api/proxy/runtime", timeout=15)
        return {
            **(info.get("runtime_mode") or {}),
            "installed_packs": info.get("installed_packs") or [],
            "install_enabled": bool(info.get("install_enabled")),
        }
    return None


def _candidate_capabilities(capability: Mapping[str, Any] | None):
    if capability:
        yield capability
        for pack in capability.get("installed_packs") or []:
            manifest = pack.get("manifest") or {}
            yield {"available": True, **manifest}


def _capability_matches(capability: Mapping[str, Any] | None, build_id: str) -> bool:
    return any(
        compatibility(candidate, expected_build_id=build_id)["compatible"]
        for candidate in _candidate_capabilities(capability)
        if candidate.get("available")
    )


async def _upload_runtime_pack(node, capability, build_id: str) -> bool:
    import httpx

    auto = os.environ.get("LINKCPP_AUTO_DISTRIBUTE_RUNTIME_PACK", "1").strip().lower()
    if auto not in ("1", "true", "yes") or not capability or not capability.get("install_enabled"):
        return False
    artifact = next(
        (item for item in runtime_packs.bundled_artifacts()
         if (item.get("manifest") or {}).get("build_id") == build_id),
        None,
    )
    if artifact is None:
        return False
    kind = node.get("kind", "local")
    if kind == "agent":
        base_url = str(node.get("agent_url") or "").rstrip("/")
        route = "/control/proxy/runtime/packs/install/" + artifact["sha256"]
    elif kind == "remote_unit_node":
        base_url = str(node.get("remote_unit_url") or "").rstrip("/")
        route = "/api/proxy/runtime/packs/install/" + artifact["sha256"]
    else:
        return False
    if not base_url:
        return False

    async def content():
        with Path(artifact["path"]).open("rb") as stream:
            while True:
                chunk = await asyncio.to_thread(stream.read, 4 << 20)
                if not chunk:
                    break
                yield chunk

    async with httpx.AsyncClient(timeout=None) as client:
        response = await client.put(
            base_url + route, params={"expected_build_id": build_id}, content=content(),
        )
        response.raise_for_status()
    return True


async def _validate_node_runtimes(hub, nodes) -> dict[str, Any]:
    local = local_runtime_info()
    if not local.get("available") or not local.get("build_id"):
        raise RuntimeError(
            "ring adapter runtime mismatch: controller: "
            + str(local.get("blocker") or "ring adapter is unavailable")
        )
    expected_build_id = str(local["build_id"])
    capabilities = await asyncio.gather(
        *(_node_runtime_capability(hub, node) for node in nodes), return_exceptions=True,
    )
    failures = []
    for index, (node, capability) in enumerate(zip(nodes, capabilities)):
        if isinstance(capability, Exception):
            failures.append(f"{node.get('name') or node['id']}: {capability}")
            continue
        if not _capability_matches(capability, expected_build_id):
            try:
                if await _upload_runtime_pack(node, capability, expected_build_id):
                    capability = await _node_runtime_capability(hub, node)
                    capabilities[index] = capability
            except Exception as exc:
                failures.append(f"{node.get('name') or node['id']}: runtime pack upload failed: {exc}")
                continue
        reports = [
            compatibility(candidate, expected_build_id=expected_build_id)
            for candidate in _candidate_capabilities(capability)
            if candidate.get("available")
        ]
        if not reports or not any(report["compatible"] for report in reports):
            report = reports[0] if reports else compatibility(
                capability, expected_build_id=expected_build_id,
            )
            blocker = (capability or {}).get("blocker") or report["mismatches"]
            failures.append(f"{node.get('name') or node['id']}: {blocker}")
    if failures:
        raise RuntimeError("ring adapter runtime mismatch: " + "; ".join(failures))
    return local


async def _node_status(hub, node):
    if node.get("kind", "local") == "local":
        from controller.proxy import stage_service
        from controller.proxy.hub_api import _stage_log
        return stage_service.status(node, _stage_log(node["id"]))
    if node.get("kind") == "agent":
        return await hub._agent_request(node, "GET", "/control/proxy/stage/status", timeout=15)
    if node.get("kind") == "remote_unit_node":
        source_id = node.get("remote_source_node_id") or ""
        return await hub._remote_unit_node_request(
            node, "GET", f"/api/proxy/nodes/{source_id}/stage/status", timeout=15,
        )
    return {"running": False, "log": "unsupported node kind"}


async def _wait_ready(hub, nodes, timeout: float):
    deadline = time.time() + timeout
    latest = {}
    while time.time() < deadline:
        statuses = await asyncio.gather(
            *(_node_status(hub, node) for node in nodes), return_exceptions=True,
        )
        ready = True
        for node, status in zip(nodes, statuses):
            if isinstance(status, Exception):
                latest[node["id"]] = {"running": False, "error": str(status)}
                ready = False
                continue
            latest[node["id"]] = status
            if not status.get("running"):
                raise RuntimeError(
                    f"ring stage exited on {node.get('name') or node['id']}: "
                    f"{status.get('log', '')[-800:]}"
                )
            log = status.get("log") or ""
            if "ring stage ready:" not in log and "listening on http://" not in log:
                ready = False
        if ready:
            return latest
        await asyncio.sleep(0.5)
    raise RuntimeError(f"ring stages did not become ready: {latest}")


def _partial_shard_enabled() -> bool:
    return os.environ.get("LINKCPP_RING_PARTIAL_SHARD", "0").strip().lower() in ("1", "true", "yes")


def _node_is_nat(node: Mapping[str, Any]) -> bool:
    """A node reachable only outbound: a mobile host (behind carrier/LAN NAT) or
    one explicitly flagged (e.g. self-enrolled from behind NAT). Such a node
    dials both ring neighbours instead of accepting an inbound connection."""
    if node.get("nat"):
        return True
    hp = node.get("host_platform") or {}
    system = (hp.get("system") if isinstance(hp, dict) else "") or ""
    return system.lower() in ("android", "ios")


def _partial_shard_urls(hub, model_ref, active) -> dict[str, str]:
    """Per-node download URL pointing at this node's layer window only (a
    mini-GGUF), so a node downloads its shard instead of the whole model."""
    from urllib.parse import quote
    name = os.path.basename(model_ref)
    base = hub._hub_reachable_url()
    token = hub._hub_model_service_token_qs()
    urls = {}
    for nid, placement in active:
        window = placement.get("layers") or []
        if len(window) != 2:
            continue
        qs = f"layers={window[0]}:{window[1]}" + (f"&{token}" if token else "")
        urls[nid] = f"{base}/api/proxy/models/{quote(name, safe='')}/stage?{qs}"
    return urls


async def serve(controller, request, result, active):
    """Load local layer windows and establish one predecessor/successor ring."""
    from controller import hub

    if not active:
        raise RuntimeError("ring proxy requires at least one active node")
    nodes = [hub.NODES[nid] for nid, _ in active]
    # A self-enrolled NAT node can't be reached inbound, so the hub can neither
    # probe its runtime nor its readiness — it self-attests and self-starts.
    reachable = [n for n in nodes if not n.get("self_enrolled")]
    ring_runtime = await _validate_node_runtimes(hub, reachable or nodes)
    # Auto-stage the model to any agent (e.g. a phone) that lacks it, so a ring
    # load is one call instead of a manual download step per node. With partial
    # sharding on, each node fetches only its layer window (a mini-GGUF from the
    # proxy's /api/proxy/models/{name}/stage route).
    controller.update(phase="loading", detail="staging model to ring nodes")
    model_ref = result.get("model_ref", request.model)
    node_source_urls = _partial_shard_urls(hub, model_ref, active) if _partial_shard_enabled() else {}
    await hub._ensure_agent_models_for_ring(nodes, model_ref, node_source_urls=node_source_urls)
    topology = hub._rpc_topology(controller, active, RING_PROXY)
    shard_refs = result.get("model_shards") or [result.get("model_ref", request.model)]
    plan_id = f"{controller['id']}-{uuid.uuid4().hex[:12]}"
    manifests = build_stage_manifests(
        plan_id=plan_id,
        model_ref=result.get("model_ref", request.model),
        model_root=hub.MODEL_DIR,
        shard_refs=shard_refs,
        placements=[placement for _, placement in active],
    )
    architectures = {
        str((manifest.get("identity") or {}).get("architecture") or "").lower()
        for manifest in manifests
    }
    if len(architectures) != 1 or "" in architectures:
        raise RuntimeError("ring proxy stage manifests disagree on model architecture")
    result["stage_plan_id"] = plan_id
    result["stage_manifests"] = [
        {
            "stage_index": manifest["stage_index"],
            "layers": manifest["layers"],
            "tensor_count": len(manifest["tensor_names"]),
            "identity": manifest["identity"],
        }
        for manifest in manifests
    ]
    # NAT traversal: a node that can only be reached outbound (a mobile host, or
    # one that self-enrolled from behind NAT) dials both its neighbours; its
    # predecessor accepts that edge instead of dialing it. A public neighbour is
    # always dial-reachable, so only the NAT node and its predecessor change.
    n = len(active)
    nat = [_node_is_nat(node) for node in nodes]
    requests = []
    for index, ((_, placement), node, manifest) in enumerate(zip(active, nodes, manifests)):
        requests.append(StageStartRequest(
            model=result.get("model_ref", request.model),
            layers=list(placement["layers"]),
            role=_role(index, len(active)),
            listen_port=int(node.get("stage_port") or (int(node.get("rpc_port", 50052)) + 1000)),
            next_endpoint=topology[(index + 1) % len(topology)]["stage_endpoint"],
            dial_prev_endpoint=topology[(index - 1) % n]["stage_endpoint"] if nat[index] else "",
            accept_next=nat[(index + 1) % n],
            op_id=f"stage-{controller['id']}-{node['id']}-{uuid.uuid4().hex[:6]}",
            gpu_layers=-1,
            ctx=request.ctx * request.parallel,
            parallel=request.parallel,
            batch=int(getattr(request, "batch", 0) or 0),
            ubatch=int(getattr(request, "ubatch", 0) or 0),
            cache_type_k=str(getattr(request, "cache_type_k", "f16") or "f16"),
            cache_type_v=str(getattr(request, "cache_type_v", "f16") or "f16"),
            kv_offload=str(result.get("kv_cache_location") or "vram") != "ram",
            cont_batching=bool(getattr(request, "cont_batching", True)),
            coordinator=index == 0,
            single_node=len(active) == 1,
            server_port=(int(node.get("stage_port") or (int(node.get("rpc_port", 50052)) + 1000)) + 1000)
                if index == 0 else None,
            stage_manifest=manifest,
            ot=str(placement.get("ot") or ""),
            ring_protocol=str(ring_runtime["protocol"]),
            ring_adapter_abi=int(ring_runtime["adapter_abi"]),
            ring_build_id=str(ring_runtime["build_id"]),
        ))
    # The coordinator's ring listener, reachable from the hub (co-located on the
    # GPU box). A NAT node that can't open the coordinator's port directly relays
    # its ring bytes to it through the hub over 443 (see /api/ring-relay).
    controller["coordinator_ring"] = {"host": "127.0.0.1", "port": int(requests[0].listen_port)}
    controller.update(phase="loading", detail="starting ring stages", model=request.model, plan=result)
    hub._record_ctrl_op(
        controller, "proxy_load", "stage_starting", "running", 30.0,
        "starting node-local ring stages", model=request.model,
        details={"plan_id": plan_id, "topology": topology},
    )
    # A self-enrolled NAT node can only be reached outbound, so the hub cannot
    # push a stage-start to it. Publish its stage config for the node to pull
    # (GET /api/shard-enroll/config) and self-start; push to every other node.
    push = []
    for node, req in zip(nodes, requests):
        if node.get("self_enrolled"):
            hub.SELF_START_CONFIGS[node["id"]] = {
                "controller_id": controller["id"], "config": model_to_dict(req),
                "coordinator_endpoint": req.dial_prev_endpoint or req.next_endpoint,
                # A NAT node reaches the coordinator by relaying ring bytes through
                # the hub over 443, so it needs no public coordinator port.
                "relay": {"path": "/api/ring-relay", "controller_id": controller["id"]},
            }
            hub._log_event("stage_awaiting_self_start", controller_id=controller["id"],
                           node_id=node["id"], layers=list(req.layers))
        else:
            push.append(_start_node(hub, node, req))
    await asyncio.gather(*push)
    controller["detail"] = "waiting for adjacent ring"
    timeout = min(3600.0, max(300.0, float(result.get("total_weight_gib") or 0) * 20.0))
    statuses = await _wait_ready(hub, reachable, timeout)
    if isinstance(result.get("data_plane"), dict):
        result["data_plane"]["established"] = True
    for stage in topology:
        if isinstance(stage.get("data_plane"), dict):
            stage["data_plane"]["established"] = True
    controller["last_load"] = hub._serve_req_snapshot(request)
    controller["proxy_first_node"] = nodes[0]["id"]
    controller["ctx"] = request.ctx
    controller["parallel"] = request.parallel
    controller.update(phase="running", detail="")
    hub._persist_hub_state()
    hub._record_ctrl_op(
        controller, "proxy_load", "running", "done", 100.0,
        "node-local ring is ready", model=request.model,
        details={"plan_id": plan_id, "statuses": statuses},
    )


def _wire_request_id(request_id) -> int | None:
    """The LKC1 ring frame carries a u64 request id; the hub tracks inferences by
    string id. Map any non-integer id deterministically into (0, 2^64)."""
    if request_id is None:
        return None
    try:
        value = int(request_id)
    except (TypeError, ValueError):
        digest = hashlib.sha256(str(request_id).encode("utf-8")).digest()
        value = int.from_bytes(digest[:8], "big")
    value %= 2**64
    return value or 1


async def infer(controller, prompt: str, max_tokens: int, request_id=None, messages=None):
    from controller import hub

    node_id = controller.get("proxy_first_node")
    node = hub.NODES.get(node_id or "")
    if not node:
        raise RuntimeError("first ring stage is not registered")
    request = StageInferRequest(prompt=prompt, messages=messages, max_tokens=max_tokens,
                                request_id=_wire_request_id(request_id))
    if node.get("kind", "local") == "local":
        from controller.proxy.hub_api import infer_local_stage
        return await infer_local_stage(node, request)
    if node.get("kind") == "agent":
        return await hub._agent_request(
            node, "POST", "/control/proxy/stage/infer", model_to_dict(request), timeout=None,
        )
    if node.get("kind") == "remote_unit_node":
        source_id = node.get("remote_source_node_id") or ""
        return await hub._remote_unit_node_request(
            node, "POST", f"/api/proxy/nodes/{source_id}/stage/infer",
            model_to_dict(request), timeout=None,
        )
    raise RuntimeError("first ring stage cannot accept inference")


def _chat_prompt(messages) -> str:
    parts = []
    for message in messages or []:
        role = str(message.get("role") or "user")
        content = message.get("content", "")
        if isinstance(content, list):
            content = "".join(
                str(item.get("text") or "") for item in content if isinstance(item, dict)
            )
        parts.append(f"<|im_start|>{role}\n{content}<|im_end|>\n")
    parts.append("<|im_start|>assistant\n")
    return "".join(parts)


def _first_stage_node(controller):
    from controller import hub

    node_id = controller.get("proxy_first_node")
    node = hub.NODES.get(node_id or "")
    if not node:
        raise RuntimeError("first ring stage is not registered")
    return node


async def chat_completion(controller, body, request_id=None):
    """Forward the raw OpenAI chat body to the ring coordinator's native
    llama-server endpoint so tools / tool_choice / structured content pass
    through, routed to wherever the first stage runs (local / agent / remote)."""
    from controller import hub

    node = _first_stage_node(controller)
    payload = {**body, "stream": False}
    kind = node.get("kind", "local")
    try:
        if kind == "local":
            from controller.proxy.stage_service import run_chat
            return await run_chat(node, payload)
        if kind == "agent":
            return await hub._agent_request(
                node, "POST", "/control/proxy/stage/chat", payload, timeout=None,
            )
        if kind == "remote_unit_node":
            source_id = node.get("remote_source_node_id") or ""
            return await hub._remote_unit_node_request(
                node, "POST", f"/api/proxy/nodes/{source_id}/stage/chat", payload, timeout=None,
            )
        raise RuntimeError("first ring stage cannot accept inference")
    except _httpx_status_error() as exc:
        # A stage still on the previous runtime has no /stage/chat endpoint (404).
        # Fall back to the legacy text protocol so serving keeps working during a
        # rolling upgrade; native tools/streaming come online once every stage is
        # updated. Only 404 falls back — real coordinator errors propagate.
        if getattr(getattr(exc, "response", None), "status_code", None) != 404:
            raise
        return await _chat_completion_text(controller, body, request_id=request_id)


def _httpx_status_error():
    import httpx
    return httpx.HTTPStatusError


async def stream_chat_completion(controller, body, request_id=None):
    """Stream (SSE) the raw OpenAI chat body from the ring coordinator's native
    endpoint, relayed through the first stage's channel (real token streaming)."""
    from controller import hub

    node = _first_stage_node(controller)
    payload = {**body, "stream": True}
    kind = node.get("kind", "local")
    if kind == "local":
        from controller.proxy.stage_service import run_chat_stream
        async for chunk in run_chat_stream(node, payload):
            yield chunk
        return
    if kind == "agent":
        async for chunk in hub._agent_request_stream(
            node, "POST", "/control/proxy/stage/chat/stream", payload,
        ):
            yield chunk
        return
    if kind == "remote_unit_node":
        source_id = node.get("remote_source_node_id") or ""
        async for chunk in hub._remote_unit_node_request_stream(
            node, "POST", f"/api/proxy/nodes/{source_id}/stage/chat/stream", payload,
        ):
            yield chunk
        return
    raise RuntimeError("first ring stage cannot stream inference")


async def _chat_completion_text(controller, body, request_id=None):
    """Legacy text-only ring chat (no tools). Retained as a fallback reference."""
    messages = body.get("messages") or None
    prompt = str(body.get("prompt") or "")
    if messages:
        normalized = []
        for message in messages:
            content = message.get("content", "")
            if isinstance(content, list):
                content = "".join(
                    str(item.get("text") or "") for item in content if isinstance(item, dict)
                )
            normalized.append({"role": str(message.get("role") or "user"), "content": str(content)})
        messages = normalized
    elif not prompt:
        prompt = _chat_prompt([])
    max_tokens = int(body.get("max_tokens") or 512)
    result = await infer(controller, prompt, max_tokens, request_id=request_id, messages=messages)
    elapsed_ms = int(result.get("elapsed_ms") or 0)
    completion_tokens = int(result.get("tokens") or 0)
    return {
        "id": "chatcmpl-proxy-" + uuid.uuid4().hex,
        "object": "chat.completion",
        "created": int(time.time()),
        "model": controller.get("model"),
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": result.get("text", "")},
            "finish_reason": "stop",
        }],
        "usage": {
            "prompt_tokens": 0,
            "completion_tokens": completion_tokens,
            "total_tokens": completion_tokens,
        },
        "timings": {
            "predicted_n": completion_tokens,
            "predicted_ms": elapsed_ms,
            "predicted_per_second": round(completion_tokens * 1000 / elapsed_ms, 3) if elapsed_ms else None,
        },
        "proxy": {"request_id": result.get("request_id"), "elapsed_ms": elapsed_ms},
    }


async def unload(controller, reason: str):
    from controller import hub

    nodes = [hub.NODES[nid] for nid in controller.get("nodes", []) if nid in hub.NODES]
    request = UnloadRequest(reason=reason)

    async def stop(node):
        if node.get("kind", "local") == "local":
            from controller.proxy import stage_service
            stage_service.stop(node, reason)
            return
        if node.get("kind") == "agent":
            await hub._agent_request(
                node, "POST", "/control/proxy/stage/stop", model_to_dict(request), timeout=15,
            )
            return
        if node.get("kind") == "remote_unit_node":
            source_id = node.get("remote_source_node_id") or ""
            await hub._remote_unit_node_request(
                node, "POST", f"/api/proxy/nodes/{source_id}/stage/stop",
                model_to_dict(request), timeout=15,
            )

    await asyncio.gather(*(stop(node) for node in nodes), return_exceptions=False)
    controller.update(model=None, phase="idle", detail="", plan=None)
    controller.pop("proxy_first_node", None)
    hub._persist_hub_state()
    return {"stopped": True, "nodes": [node["id"] for node in nodes]}
