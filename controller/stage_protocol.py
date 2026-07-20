"""Minimal adjacent-stage binary protocol primitives.

The controller uses this module to derive and validate the data-plane contract
from a completed model placement. Model data never travels through the hub;
only neighboring stages exchange frames over persistent sockets.
"""

from __future__ import annotations

import struct
from dataclasses import dataclass
from typing import Iterable, Mapping


MAGIC = b"LKS1"
VERSION = 1
HEADER = struct.Struct("<4sBBH Q I I I I")
TENSOR_META = struct.Struct("<HHBBH Q Q Q Q")
MAX_PAYLOAD = 64 * 1024 * 1024

HELLO = 1
PREFILL = 2
DECODE = 3
TOKEN = 4
RESET = 5
ACK = 6


@dataclass(frozen=True)
class StageFrame:
    kind: int
    request_id: int
    sequence_id: int
    position: int
    payload: bytes = b""
    flags: int = 0


@dataclass(frozen=True)
class TensorDescriptor:
    role: str
    dtype: str
    shape: tuple[int, ...]
    strides: tuple[int, ...]
    sequence_id: int = 0
    position: int = 0
    view_offset: int = 0
    flags: int = 0


def encode_tensor_payload(descriptor: TensorDescriptor, raw: bytes) -> bytes:
    role = descriptor.role.encode("utf-8")
    dtype = descriptor.dtype.encode("ascii")
    shape = tuple(int(value) for value in descriptor.shape)
    strides = tuple(int(value) for value in descriptor.strides)
    if len(role) > 0xFFFF or len(dtype) > 0xFFFF:
        raise ValueError("tensor descriptor labels are too long")
    if len(shape) != len(strides) or len(shape) > 0xFF:
        raise ValueError("tensor shape and strides must have the same valid rank")
    if not 0 <= descriptor.flags <= 0xFFFF:
        raise ValueError("tensor descriptor flags must fit in two bytes")
    payload = TENSOR_META.pack(
        len(role), len(dtype), len(shape), 0, descriptor.flags,
        descriptor.sequence_id, descriptor.position, descriptor.view_offset, len(raw),
    )
    payload += struct.pack("<" + "q" * len(shape), *shape)
    payload += struct.pack("<" + "q" * len(strides), *strides)
    payload += role + dtype + bytes(raw)
    if len(payload) > MAX_PAYLOAD:
        raise ValueError("tensor payload is too large")
    return payload


def decode_tensor_payload(payload: bytes) -> tuple[TensorDescriptor, bytes]:
    if len(payload) < TENSOR_META.size:
        raise ValueError("tensor payload is truncated")
    role_len, dtype_len, ndim, reserved, flags, sequence_id, position, view_offset, raw_size = TENSOR_META.unpack_from(payload)
    if reserved != 0:
        raise ValueError("invalid tensor descriptor reserved field")
    offset = TENSOR_META.size
    vector_size = ndim * 8
    if offset + vector_size * 2 + role_len + dtype_len + raw_size != len(payload):
        raise ValueError("tensor payload length mismatch")
    shape = struct.unpack_from("<" + "q" * ndim, payload, offset) if ndim else ()
    offset += vector_size
    strides = struct.unpack_from("<" + "q" * ndim, payload, offset) if ndim else ()
    offset += vector_size
    role = payload[offset:offset + role_len].decode("utf-8")
    offset += role_len
    dtype = payload[offset:offset + dtype_len].decode("ascii")
    offset += dtype_len
    raw = payload[offset:]
    return TensorDescriptor(role, dtype, shape, strides, sequence_id, position, view_offset, flags), raw


def encode_frame(frame: StageFrame) -> bytes:
    payload = bytes(frame.payload)
    if not 0 <= frame.kind <= 255:
        raise ValueError("stage frame kind must fit in one byte")
    if not 0 <= frame.flags <= 0xFFFF:
        raise ValueError("stage frame flags must fit in two bytes")
    if len(payload) > MAX_PAYLOAD:
        raise ValueError("stage frame payload is too large")
    return HEADER.pack(
        MAGIC,
        VERSION,
        frame.kind,
        frame.flags,
        frame.request_id,
        frame.sequence_id,
        frame.position,
        len(payload),
        0,
    ) + payload


def decode_header(raw: bytes) -> tuple[int, int, int, int, int, int, int]:
    if len(raw) != HEADER.size:
        raise ValueError("invalid stage frame header size")
    magic, version, kind, flags, request_id, sequence_id, position, size, reserved = HEADER.unpack(raw)
    if magic != MAGIC or version != VERSION:
        raise ValueError("unsupported stage frame")
    if reserved != 0:
        raise ValueError("invalid stage frame reserved field")
    if size > MAX_PAYLOAD:
        raise ValueError("stage frame payload is too large")
    return kind, flags, request_id, sequence_id, position, size, reserved


def decode_frame(raw: bytes) -> StageFrame:
    if len(raw) < HEADER.size:
        raise ValueError("stage frame is truncated")
    kind, flags, request_id, sequence_id, position, size, _ = decode_header(raw[:HEADER.size])
    payload = raw[HEADER.size:]
    if len(payload) != size:
        raise ValueError("stage frame payload length mismatch")
    return StageFrame(kind, request_id, sequence_id, position, payload, flags)


def adjacent_links(topology: Iterable[Mapping[str, object]], cycle: bool = True) -> list[dict[str, object]]:
    """Return neighboring stage links, optionally closing the decode loop."""
    stages = list(topology)
    links = []
    pairs = list(zip(stages, stages[1:]))
    if cycle and len(stages) > 1:
        pairs.append((stages[-1], stages[0]))
    for left, right in pairs:
        links.append({
            "upstream": left.get("node_id"),
            "downstream": right.get("node_id"),
            "upstream_endpoint": left.get("stage_endpoint"),
            "downstream_endpoint": right.get("stage_endpoint"),
            "layers": [left.get("layers"), right.get("layers")],
        })
    return links


def stage_neighbors(topology: Iterable[Mapping[str, object]], node_id: str) -> dict[str, object]:
    stages = list(topology)
    index = next((i for i, item in enumerate(stages) if item.get("node_id") == node_id), -1)
    if index < 0:
        raise KeyError(node_id)
    if len(stages) <= 1:
        return {"upstream": None, "downstream": None}
    return {
        "upstream": stages[(index - 1) % len(stages)].get("node_id"),
        "downstream": stages[(index + 1) % len(stages)].get("node_id"),
    }
