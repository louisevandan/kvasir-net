"""Model-opaque v2 metadata/body packet inside the established P4HF frame."""
import json
import struct


def pack(meta, body=b""):
    header = json.dumps(meta, ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()
    if not 0 < len(header) <= 65536:
        raise ValueError("metadata exceeds 64 KiB")
    return struct.pack("!I", len(header)) + header + body


def unpack(data):
    if len(data) < 4:
        raise ValueError("missing metadata length")
    n, = struct.unpack("!I", data[:4])
    if not 0 < n <= 65536 or n + 4 > len(data):
        raise ValueError("invalid metadata length")
    meta = json.loads(data[4:4+n])
    if not isinstance(meta, dict):
        raise ValueError("metadata must be an object")
    return meta, data[4+n:]

