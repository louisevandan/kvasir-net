"""Each request owns only this stage's hybrid cache and its next position."""

from dataclasses import dataclass
import torch
from transformers.cache_utils import DynamicCache


@dataclass
class Session:
    cache: DynamicCache
    position: int = 0
    issue: int = 0


def cache_bytes(cache):
    tensors = []
    for layer in cache.layers:
        for name in ("keys", "values", "conv_states", "recurrent_states"):
            value = getattr(layer, name, None)
            if isinstance(value, torch.Tensor):
                tensors.append(value)
            elif isinstance(value, dict):
                tensors.extend(x for x in value.values() if isinstance(x, torch.Tensor))
    return sum(t.numel() * t.element_size() for t in tensors)


class StageSessions:
    def __init__(self, stage, plan):
        self.stage, self.plan = stage, plan
        self.active, self.retired = {}, set()

    @torch.inference_mode()
    def validate_step(self, session_id, issue, position, tensor):
        if type(session_id) is not str or not session_id or len(session_id) > 128 or session_id in self.retired:
            raise ValueError("invalid or retired session identity")
        if type(issue) is not int or type(position) is not int or issue < 0 or position < 0:
            raise ValueError("invalid issue/position")
        current = self.active.get(session_id)
        if current is None:
            if issue != 0 or position != 0 or len(self.active) + len(self.retired) >= self.plan.max_requests:
                raise ValueError("new session exceeds capacity or has stale issue/position")
        elif (issue, position) != (current.issue, current.position):
            raise ValueError("stale, duplicate or out-of-order step")
        first = self.stage.node.start == 0
        shape_ok = tensor.ndim == (2 if first else 3) and tensor.shape[0] == 1 and tensor.shape[1] > 0
        if not shape_ok or (not first and tensor.shape[2] != 1024):
            raise ValueError("invalid Qwen boundary shape")
        if tensor.shape[1] + position > self.plan.context:
            raise ValueError("context capacity exceeded")
        if first:
            if tensor.dtype != torch.int64 or tensor.min() < 0 or tensor.max() >= 248320:
                raise ValueError("invalid input token dtype/range")
            if any(bool((tensor == token).any()) for token in (248053, 248054, 248056, 248057)):
                raise ValueError("vision/video tokens are outside this text-only runner")
        elif tensor.dtype != getattr(torch, self.plan.dtype) or not torch.isfinite(tensor).all():
            raise ValueError("invalid hidden state dtype or values")
        return current

    @torch.inference_mode()
    def step(self, session_id, issue, position, tensor):
        current = self.validate_step(session_id, issue, position, tensor)
        if current is None:
            current = Session(DynamicCache(config=self.stage.config))
            self.active[session_id] = current
        result = self.stage(tensor.to(self.stage.node.device), current.cache, position)
        if not torch.isfinite(result).all():
            raise RuntimeError("nonfinite stage output")
        current.position += tensor.shape[1]
        current.issue += 1
        return result.cpu().contiguous(), {"position": current.position, "issue": current.issue,
                                           "cache_layers": len(current.cache.layers), "cache_bytes": cache_bytes(current.cache)}

    def release(self, session_id):
        if session_id not in self.active:
            raise ValueError("release of unknown or retired session")
        released_bytes = cache_bytes(self.active[session_id].cache)
        del self.active[session_id]
        self.retired.add(session_id)
        if self.stage.node.device.startswith("cuda"):
            torch.cuda.synchronize(self.stage.node.device)
        return {"active_sessions": len(self.active), "released": session_id, "released_cache_bytes": released_bytes,
                "remaining_cache_bytes": sum(cache_bytes(s.cache) for s in self.active.values()),
                "cuda_allocated_after_release": torch.cuda.memory_allocated(self.stage.node.device)
                if self.stage.node.device.startswith("cuda") else None}
