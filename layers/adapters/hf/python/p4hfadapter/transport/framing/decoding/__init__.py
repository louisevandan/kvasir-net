"""Validate a complete incoming header before any payload allocation/read."""

from p4hfadapter.transport.framing.errors import InvalidFrame, PayloadTooLarge
from p4hfadapter.transport.framing.layout import HEADER, MAGIC, RESERVED, VERSION
from p4hfadapter.transport.framing.limits import FrameLimits


def decode_header(header: bytes, limits: FrameLimits) -> int:
    if type(header) is not bytes or len(header) != HEADER.size:
        raise InvalidFrame(f"header must contain exactly {HEADER.size} bytes")
    magic, version, reserved, payload_size = HEADER.unpack(header)
    if magic != MAGIC:
        raise InvalidFrame("invalid frame magic")
    if version != VERSION:
        raise InvalidFrame(f"unsupported frame version {version}")
    if reserved != RESERVED:
        raise InvalidFrame("reserved header bytes must be zero")
    if payload_size > limits.max_payload_bytes:
        raise PayloadTooLarge(f"payload {payload_size} exceeds limit {limits.max_payload_bytes}")
    return payload_size
