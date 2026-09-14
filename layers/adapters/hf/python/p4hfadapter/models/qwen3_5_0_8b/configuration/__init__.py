"""Validate a concrete 24-layer Qwen partition before loading any tensors."""

from dataclasses import dataclass
import json
from pathlib import Path
import re

from p4hfadapter.models.qwen3_5_0_8b.identity import LAYERS, MODEL_ID, REVISION, SCHEMA


@dataclass(frozen=True)
class Node:
    node_id: str
    host: str
    device: str
    start: int
    end: int


@dataclass(frozen=True)
class Plan:
    nodes: tuple[Node, ...]
    dtype: str
    context: int
    max_requests: int
    max_new_tokens: int
    prefill_chunk: int | None = None


def fields(value, expected, where):
    if type(value) is not dict or set(value) != set(expected):
        raise ValueError(f"{where}: expected exactly {sorted(expected)}")


def positive(value, ceiling, where):
    if type(value) is not int or not 0 < value <= ceiling:
        raise ValueError(f"{where}: expected integer in 1..{ceiling}")
    return value


def parse_plan(value) -> Plan:
    fields(value, ("schema", "model_id", "revision", "dtype", "quantization", "limits", "nodes"), "plan")
    if (value["schema"], value["model_id"], value["revision"]) != (SCHEMA, MODEL_ID, REVISION):
        raise ValueError("plan: unsupported model, revision or schema")
    if value["dtype"] not in ("float32", "bfloat16") or value["quantization"] != "none":
        raise ValueError("this runner supports dense float32/bfloat16 only")
    limits = value["limits"]
    expected = ("context", "max_requests", "max_new_tokens")
    fields(limits, expected + (("prefill_chunk",) if type(limits) is dict and "prefill_chunk" in limits else ()), "limits")
    context = positive(limits["context"], 4096, "context")
    requests = positive(limits["max_requests"], 16, "max_requests")
    output = positive(limits["max_new_tokens"], context, "max_new_tokens")
    chunk = positive(limits["prefill_chunk"], context, "prefill_chunk") if "prefill_chunk" in limits else None
    if type(value["nodes"]) is not list or not value["nodes"]:
        raise ValueError("nodes must be a nonempty ordered list")
    nodes, seen, cursor = [], set(), 0
    for item in value["nodes"]:
        fields(item, ("node_id", "host", "device", "layers"), "node")
        name, host, device, cut = (item[key] for key in ("node_id", "host", "device", "layers"))
        if type(name) is not str or not re.fullmatch(r"[a-zA-Z0-9_-]{1,64}", name) or name in seen:
            raise ValueError("node_id must be unique and contain only letters, digits, dash or underscore")
        if type(host) is not str or not host or type(device) is not str or not re.fullmatch(r"cpu|cuda:[0-9]+", device):
            raise ValueError("node requires a host and cpu or cuda:N device")
        if type(cut) is not list or len(cut) != 2 or any(type(x) is not int for x in cut):
            raise ValueError("layers must be [start, exclusive_end]")
        start, end = cut
        if start != cursor or not start < end <= LAYERS:
            raise ValueError("nodes must cover layers 0..24 exactly once in pipeline order")
        nodes.append(Node(name, host, device, start, end))
        seen.add(name)
        cursor = end
    if cursor != LAYERS:
        raise ValueError("partition does not reach layer 24")
    return Plan(tuple(nodes), value["dtype"], context, requests, output, chunk)


def read_plan(path: str | Path) -> Plan:
    return parse_plan(json.loads(Path(path).read_text(encoding="utf-8")))


def inspect_plan(plan: Plan) -> dict:
    return {"model_id": MODEL_ID, "revision": REVISION, "dtype": plan.dtype,
            "limits": {"context": plan.context, "max_requests": plan.max_requests,
                       "max_new_tokens": plan.max_new_tokens, "prefill_chunk": plan.prefill_chunk},
            "execution": "local-process", "physical_hosts_verified": False,
            "nodes": [{"node_id": n.node_id, "host": n.host, "device": n.device,
                       "layers": [n.start, n.end], "embedding": n.start == 0,
                       "output_head": n.end == LAYERS,
                       "full_attention_layers": [i for i in range(n.start, n.end) if i % 4 == 3],
                       "linear_attention_layers": [i for i in range(n.start, n.end) if i % 4 != 3]}
                      for n in plan.nodes]}
