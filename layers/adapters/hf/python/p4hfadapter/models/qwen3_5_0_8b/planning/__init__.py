"""Choose a Qwen plan from measured stage envelopes and an ordered fleet."""

from copy import deepcopy
import hashlib
import json
import math
from pathlib import Path
import re

from p4hfadapter.models.qwen3_5_0_8b.configuration import fields, parse_plan
from p4hfadapter.models.qwen3_5_0_8b.identity import MODEL_ID, REVISION, SCHEMA, TORCH_VERSION, TRANSFORMERS_VERSION


class LoadingInfeasible(ValueError):
    """No plan fits within the explicitly profiled search space."""


def fingerprint(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def source_identity():
    root = Path(__file__).resolve().parents[1]
    return {p.relative_to(root).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in sorted(root.rglob("*.py"))}


def integer(value, where, minimum=0):
    if type(value) is not int or not minimum <= value <= 2**53 - 1:
        raise ValueError(f"{where}: invalid integer")
    return value


def identifier(value, where):
    if type(value) is not str or not re.fullmatch(r"[a-zA-Z0-9_-]{1,64}", value):
        raise ValueError(f"{where}: invalid identifier")
    return value


def base_plan(request, nodes):
    result = {key: deepcopy(request[key]) for key in ("model_id", "revision", "dtype", "quantization", "limits")} | {
        "schema": SCHEMA, "nodes": nodes}
    result["limits"]["prefill_chunk"] = request["prefill_chunk"]
    return result


def profile_contract(request):
    return {key: deepcopy(request[key]) for key in ("model_id", "revision", "dtype", "quantization", "limits", "prefill_chunk")}


def validate_request(request):
    fields(request, ("schema", "model_id", "revision", "dtype", "quantization", "limits", "prefill_chunk",
                     "cuts", "hosts", "devices", "slots", "minimum_hosts"), "loading request")
    if request["schema"] != "qwen3.5-0.8b-loading-request-v1":
        raise ValueError("unsupported loading request")
    fields(request["limits"], ("context", "max_requests", "max_new_tokens"), "request limits")
    parse_plan(base_plan(request, [{"node_id": "check", "host": "local", "device": "cpu", "layers": [0, 24]}]))
    if request["dtype"] != "float32":
        raise ValueError("automatic planning requires float32; heterogeneous BF16 is not approved")
    integer(request["prefill_chunk"], "prefill_chunk", 1)
    if request["prefill_chunk"] > request["limits"]["context"]:
        raise ValueError("prefill_chunk exceeds context")
    cuts = request["cuts"]
    if (type(cuts) is not list or len(cuts) < 2 or any(type(x) is not int for x in cuts)
            or cuts != sorted(set(cuts)) or cuts[0] != 0 or cuts[-1] != 24):
        raise ValueError("cuts must be sorted unique boundaries from 0 to 24")
    hosts, devices, capacities, physical = {}, {}, {}, set()
    for host in request["hosts"]:
        fields(host, ("id", "available_bytes", "reserve_bytes"), "host")
        key = identifier(host["id"], "host id")
        if key in hosts:
            raise ValueError("duplicate host")
        hosts[key] = host
        capacities["host:" + key] = max(0, integer(host["available_bytes"], "host available") - integer(host["reserve_bytes"], "host reserve"))
    for device in request["devices"]:
        fields(device, ("id", "host", "device", "available_bytes", "reserve_bytes", "enabled"), "device")
        key = identifier(device["id"], "device id")
        location = (device["host"], device["device"])
        if key in devices or device["host"] not in hosts or location in physical:
            raise ValueError("duplicate physical device or unknown host")
        if type(device["device"]) is not str or not re.fullmatch(r"cpu|cuda:[0-9]+", device["device"]):
            raise ValueError("HF loader supports cpu or cuda:N, not Metal/GGUF offload tiers")
        if type(device["enabled"]) is not bool:
            raise ValueError("enabled must be boolean")
        physical.add(location)
        devices[key] = device
        capacities["device:" + key] = max(0, integer(device["available_bytes"], "device available") - integer(device["reserve_bytes"], "device reserve"))
    if type(request["slots"]) is not list or not 1 <= len(request["slots"]) <= 32:
        raise ValueError("provide 1..32 ordered slots")
    seen = set()
    for slot in request["slots"]:
        fields(slot, ("node_id", "device_id"), "slot")
        key = identifier(slot["node_id"], "node id")
        if key in seen or slot["device_id"] not in devices:
            raise ValueError("duplicate node or unknown slot device")
        seen.add(key)
    integer(request["minimum_hosts"], "minimum_hosts", 1)
    return hosts, devices, capacities


def validate_profiles(request, profiles, devices):
    table = {}
    contract = profile_contract(request)
    sources = source_identity()
    for profile in profiles:
        if (profile.get("schema") != "qwen3.5-0.8b-stage-profile-v1" or profile.get("contract") != contract
                or profile.get("source_sha256") != sources
                or profile.get("runtime") != {"torch": TORCH_VERSION, "transformers": TRANSFORMERS_VERSION}
                or profile.get("model_revision") != REVISION or profile.get("model_id") != MODEL_ID):
            raise ValueError("profile identity/workload/runtime/source mismatch")
        binding = profile.get("binding", {})
        device = devices.get(binding.get("id"))
        if device is None or binding != {key: device[key] for key in ("id", "host", "device")}:
            raise ValueError("profile device binding mismatch")
        if not profile.get("checkpoint_sha256") or not profile.get("measurement_host"):
            raise ValueError("profile lacks checkpoint or measurement identity")
        if profile["checkpoint_sha256"] != profile_checkpoint_sha():
            raise ValueError("profile checkpoint mismatch")
        for sample in profile["samples"]:
            start, end = sample["layers"]
            if type(start) is not int or type(end) is not int or not 0 <= start < end <= 24:
                raise ValueError("invalid profile range")
            key = (device["id"], start, end)
            if key in table:
                raise ValueError("duplicate stage profile")
            for name in ("host_peak_bytes", "device_peak_bytes", "weight_bytes", "cache_bytes"):
                integer(sample[name], name)
            if sample["host_peak_bytes"] == 0 or sample["device_peak_bytes"] < sample["weight_bytes"] + sample["cache_bytes"]:
                raise ValueError("profile memory envelope is smaller than weights/cache")
            service = sample["service_ms"]
            if type(service) not in (float, int) or not math.isfinite(service) or service <= 0:
                raise ValueError("invalid service time")
            if type(sample.get("active_after_release")) is not int or sample["active_after_release"] != 0:
                raise ValueError("profile did not release all state")
            table[key] = sample
    active = {slot["device_id"] for slot in request["slots"] if devices[slot["device_id"]]["enabled"]}
    for device_id in active:
        for i, start in enumerate(request["cuts"][:-1]):
            for end in request["cuts"][i + 1:]:
                if (device_id, start, end) not in table:
                    raise ValueError(f"missing calibrated stage: {device_id} [{start},{end})")
    return table


def profile_checkpoint_sha():
    root = Path(__file__).resolve().parents[5]
    manifest = json.loads((root / "manifests/qwen3_5_0_8b/artifact/identity.json").read_text(encoding="utf-8"))
    return fingerprint(manifest)


def plan_loading(request, profiles, *, state_limit=100000):
    """Return a runtime-consumable plan; never load tensors or alter input/evidence."""
    hosts, devices, capacities = validate_request(request)
    table = validate_profiles(request, profiles, devices)
    names = sorted(capacities)
    # States retain every memory/service tradeoff; later bottlenecks can change total-time ties.
    states = [(0, frozenset(), (0,) * len(names), 0.0, 0.0, ())]
    for slot in request["slots"]:
        device = devices[slot["device_id"]]
        if not device["enabled"]:
            continue
        options = list(states)
        for start, used, usage, peak, total, path in states:
            for end in request["cuts"]:
                if end <= start:
                    continue
                sample = table[(device["id"], start, end)]
                demand = {"host:" + device["host"]: sample["host_peak_bytes"], "device:" + device["id"]: sample["device_peak_bytes"]}
                next_usage = tuple(value + demand.get(name, 0) for name, value in zip(names, usage))
                if any(value > capacities[name] for name, value in zip(names, next_usage)):
                    continue
                options.append((end, used | {device["host"]}, next_usage, max(peak, sample["service_ms"]),
                                total + sample["service_ms"], path + ((slot["node_id"], device["id"], start, end),)))
        groups = {}
        for candidate in options:
            end, used, usage, peak, total, path = candidate
            frontier = groups.setdefault((end, used), [])
            def dominates(a, b):
                return (a[3] <= b[3] and a[4] <= b[4] and len(a[5]) <= len(b[5])
                        and all(x <= y for x, y in zip(a[2], b[2])))
            if any(dominates(old, candidate) for old in frontier):
                continue
            frontier[:] = [old for old in frontier if not dominates(candidate, old)]
            frontier.append(candidate)
        states = [state for frontier in groups.values() for state in frontier]
        if len(states) > state_limit:
            raise ValueError("exact search state limit exceeded; narrow slots/cuts, no infeasibility verdict")
    complete = [state for state in states if state[0] == 24 and len(state[1]) >= request["minimum_hosts"]]
    if not complete:
        raise LoadingInfeasible("no feasible plan within profiled cuts, ordered slots and current capacity")
    # Both current HF controllers synchronously wait for each stage in order.
    best = min(complete, key=lambda state: (state[4], state[3], len(state[5]), state[5]))
    nodes, allocations = [], []
    for node_id, device_id, start, end in best[5]:
        device, sample = devices[device_id], table[(device_id, start, end)]
        nodes.append({"node_id": node_id, "host": device["host"], "device": device["device"], "layers": [start, end]})
        allocations.append({"node_id": node_id, "device_id": device_id, **deepcopy(sample)})
    plan = base_plan(request, nodes)
    parse_plan(plan)
    return {"schema": "qwen3.5-0.8b-loading-result-v1", "plan": plan, "allocations": allocations,
            "resources": {name: {"required_bytes": value, "usable_bytes": capacities[name]} for name, value in zip(names, best[2])},
            "objective": {"total_stage_service_ms": best[4], "maximum_stage_service_ms": best[3], "stages": len(nodes)},
            "request_sha256": fingerprint(request), "profile_sha256": [fingerprint(p) for p in profiles],
            "scope": "minimum serial compute service within supplied cuts/slots; synthetic workload; IPC/network and contention unmeasured, not TPS/remote LOAD approval",
            "execution_constraints": {"maximum_prefill_chunk": request["prefill_chunk"], "capacity_recheck_required": True,
                                      "driver_allocator_and_unmeasured_peak_headroom": "caller-provided reserves"}}
