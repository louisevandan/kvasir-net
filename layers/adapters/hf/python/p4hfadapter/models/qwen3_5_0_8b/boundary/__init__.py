"""Qwen-only metadata plus safe tensor bytes inside the existing bounded frame."""

import json
import struct
from safetensors.torch import load, save


def pack(metadata, tensor=None):
    header = json.dumps(metadata, allow_nan=False, separators=(",", ":")).encode("utf-8")
    if len(header) > 65536:
        raise ValueError("metadata exceeds 64 KiB")
    body = b"" if tensor is None else save({"tensor": tensor.detach().cpu().contiguous()})
    return struct.pack("!I", len(header)) + header + body


def unpack(payload):
    if len(payload) < 4:
        raise ValueError("missing metadata length")
    length = struct.unpack("!I", payload[:4])[0]
    if not 0 < length <= 65536 or 4 + length > len(payload):
        raise ValueError("invalid metadata length")
    metadata = json.loads(payload[4:4 + length])
    if type(metadata) is not dict:
        raise ValueError("metadata must be an object")
    body = payload[4 + length:]
    tensor = None
    if body:
        tensors = load(body)
        if set(tensors) != {"tensor"}:
            raise ValueError("unexpected tensor bundle")
        tensor = tensors["tensor"]
    return metadata, tensor
