"""Validate an outgoing byte count and encode its header."""

from p4hfadapter.transport.framing.errors import InvalidFrame, PayloadTooLarge
from p4hfadapter.transport.framing.layout import HEADER, MAGIC, RESERVED, VERSION
from p4hfadapter.transport.framing.limits import FrameLimits


def encode_header(payload_size: int, limits: FrameLimits) -> bytes:
    if type(payload_size) is not int or not 0 <= payload_size < 2**64:
        raise InvalidFrame("payload_size must be an unsigned 64-bit integer")
    if payload_size > limits.max_payload_bytes:
        raise PayloadTooLarge(f"payload {payload_size} exceeds limit {limits.max_payload_bytes}")
    return HEADER.pack(MAGIC, VERSION, RESERVED, payload_size)
