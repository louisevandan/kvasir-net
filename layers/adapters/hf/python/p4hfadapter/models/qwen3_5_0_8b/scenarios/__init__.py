"""Admission for text workloads; requests remain individual, never padded batches."""

from dataclasses import dataclass, field
import json
from pathlib import Path
import uuid

from p4hfadapter.models.qwen3_5_0_8b.configuration import fields, positive


@dataclass
class Request:
    name: str
    tokens: list[int]
    limit: int
    chunk: int
    cancel_after: int | None
    session_id: str = field(default_factory=lambda: uuid.uuid4().hex)
    position: int = 0
    issue: int = 0
    generated: list[int] = field(default_factory=list)
    terminal: str | None = None


def read_scenario(path: Path, plan, tokenizer):
    raw = json.loads(path.read_text(encoding="utf-8"))
    fields(raw, ("name", "schedule", "requests"), "scenario")
    if type(raw["name"]) is not str or not raw["name"] or raw["schedule"] not in ("sequential", "round_robin"):
        raise ValueError("scenario needs a name and sequential or round_robin schedule")
    if type(raw["requests"]) is not list or not 0 < len(raw["requests"]) <= plan.max_requests:
        raise ValueError("scenario request count exceeds admission limit")
    requests, seen = [], set()
    for item in raw["requests"]:
        fields(item, ("id", "prompt", "max_new_tokens", "prefill_chunk", "cancel_after"), "request")
        name = item["id"]
        if type(name) is not str or not name or name in seen or type(item["prompt"]) is not str or not item["prompt"]:
            raise ValueError("request needs unique id and nonempty text prompt")
        limit = positive(item["max_new_tokens"], plan.max_new_tokens, "max_new_tokens")
        chunk = positive(item["prefill_chunk"], plan.context, "prefill_chunk")
        cancel = item["cancel_after"]
        if cancel is not None:
            positive(cancel, limit, "cancel_after")
        tokens = tokenizer.apply_chat_template([{"role": "user", "content": item["prompt"]}],
                                               tokenize=True, add_generation_prompt=True, enable_thinking=False, return_dict=False)
        if type(tokens) is not list or any(type(t) is not int for t in tokens):
            raise ValueError("tokenizer did not return a flat list of token IDs")
        if not tokens or len(tokens) + limit > plan.context or any(t in (248053, 248054, 248056, 248057) for t in tokens):
            raise ValueError("request exceeds context or contains unsupported multimodal tokens")
        requests.append(Request(name, tokens, limit, chunk, cancel))
        seen.add(name)
    return raw["name"], raw["schedule"], requests
