"""Conformance-only snapshot of every declared hybrid-cache tensor, in global layer order."""
import torch
from safetensors.torch import save, load


def tensors(cache, start=0):
    result = {}
    for index, layer in enumerate(cache.layers, start):
        for name in ("keys", "values", "conv_states", "recurrent_states"):
            value = getattr(layer, name, None)
            if isinstance(value, torch.Tensor):
                result[f"{index}.{name}"] = value.detach().cpu().contiguous()
            elif isinstance(value, dict):
                for key, tensor in value.items():
                    if isinstance(tensor, torch.Tensor):
                        result[f"{index}.{name}.{key}"] = tensor.detach().cpu().contiguous()
    return result


def export(cache, start):
    return save(tensors(cache, start))


def compare(actual_bytes, reference, begin, end):
    actual = load(actual_bytes)
    expected = {k: v for k, v in tensors(reference).items() if begin <= int(k.split('.')[0]) < end}
    if actual.keys() != expected.keys():
        raise AssertionError(f"cache members differ: {actual.keys()} vs {expected.keys()}")
    maximum = 0.0
    for key, tensor in actual.items():
        target = expected[key]
        if tensor.shape != target.shape or tensor.dtype != target.dtype:
            raise AssertionError(f"cache shape/dtype mismatch: {key}")
        if tensor.numel():
            maximum = max(maximum, (tensor-target).abs().max().item())
        if not torch.allclose(tensor, target, atol=0.125, rtol=0.01):
            raise AssertionError(f"cache content mismatch: {key}, max_abs={maximum}")
    return {"tensors": len(actual), "elements": sum(t.numel() for t in actual.values()), "max_abs": maximum}

