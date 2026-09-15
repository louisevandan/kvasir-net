"""Backend-neutral P4 node lifecycle framing for Python OUTER clients."""

import json
import struct


NODE_LOAD = "application/vnd.p4.node.load-v1"
NODE_UNLOAD = "application/vnd.p4.node.unload-v1"
NODE_LIFECYCLE_RESULT = "application/vnd.p4.node.lifecycle-result-v1"
SCHEMA = 1
MAX_METADATA_BYTES = 64 * 1024
ADAPTER_KIND = "hf-transformers"

_CAPACITIES = {
    "queue_capacity",
    "completion_capacity",
    "retained_capacity",
    "retained_bytes",
}
_REQUEST_REQUIRED = {
    "schema",
    "node_id",
    "node_generation",
    "adapter_kind",
    "adapter_content_type",
}
_RESULT_REQUIRED = _REQUEST_REQUIRED | {"operation", "status", "resource_state"}
_RESULT_ALLOWED = _RESULT_REQUIRED | {"first_error", "cleanup_error"}


def _canonical(value):
    return json.dumps(
        value,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
        allow_nan=False,
    ).encode("utf-8")


def _identity(metadata):
    if metadata.get("schema") != SCHEMA:
        raise ValueError("unsupported node lifecycle schema")
    if not isinstance(metadata.get("node_id"), str) or not metadata["node_id"]:
        raise ValueError("node lifecycle requires node_id")
    generation = metadata.get("node_generation")
    if isinstance(generation, bool) or not isinstance(generation, int) or generation <= 0:
        raise ValueError("node lifecycle requires positive node_generation")
    for field in ("adapter_kind", "adapter_content_type"):
        if not isinstance(metadata.get(field), str) or not metadata[field]:
            raise ValueError(f"node lifecycle requires {field}")


def _encode(metadata, opaque):
    if not isinstance(opaque, bytes):
        raise TypeError("node lifecycle opaque payload must be bytes")
    header = _canonical(metadata)
    if len(header) > MAX_METADATA_BYTES:
        raise ValueError("node lifecycle metadata is too large")
    return struct.pack("<I", len(header)) + header + opaque


def _decode(payload):
    if not isinstance(payload, bytes) or len(payload) < 4:
        raise ValueError("node lifecycle metadata length is incomplete")
    size, = struct.unpack("<I", payload[:4])
    if size > MAX_METADATA_BYTES:
        raise ValueError("node lifecycle metadata is too large")
    end = 4 + size
    if end > len(payload):
        raise ValueError("node lifecycle metadata is truncated")
    try:
        metadata = json.loads(payload[4:end])
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValueError(f"invalid node lifecycle metadata: {error}") from error
    if not isinstance(metadata, dict):
        raise ValueError("node lifecycle metadata must be an object")
    return metadata, payload[end:]


def encode_request(operation, node, adapter_content_type, opaque, capacities=None):
    if operation not in ("load", "unload"):
        raise ValueError("unsupported node lifecycle operation")
    metadata = {
        "schema": SCHEMA,
        "node_id": node["node"],
        "node_generation": node["generation"],
        "adapter_kind": ADAPTER_KIND,
        "adapter_content_type": adapter_content_type,
    }
    if operation == "load":
        if not isinstance(capacities, dict) or set(capacities) != _CAPACITIES:
            raise ValueError("LOAD requires exact lifecycle capacities")
        if any(
            isinstance(value, bool) or not isinstance(value, int) or value <= 0
            for value in capacities.values()
        ):
            raise ValueError("LOAD lifecycle capacities must be positive integers")
        metadata.update(capacities)
    elif capacities is not None:
        raise ValueError("UNLOAD must not carry lifecycle capacities")
    _identity(metadata)
    return _encode(metadata, opaque)


def decode_request(payload, operation):
    metadata, opaque = _decode(payload)
    expected = _REQUEST_REQUIRED | (_CAPACITIES if operation == "load" else set())
    if set(metadata) != expected:
        raise ValueError("node lifecycle request fields differ from the contract")
    _identity(metadata)
    if operation == "load" and any(
        isinstance(metadata[field], bool)
        or not isinstance(metadata[field], int)
        or metadata[field] <= 0
        for field in _CAPACITIES
    ):
        raise ValueError("LOAD lifecycle capacities must be positive integers")
    return metadata, opaque


def decode_result(payload, operation, node, adapter_content_type):
    metadata, opaque = _decode(payload)
    if not _RESULT_REQUIRED <= set(metadata) <= _RESULT_ALLOWED:
        raise ValueError("node lifecycle result fields differ from the contract")
    _identity(metadata)
    if (
        metadata["node_id"] != node["node"]
        or metadata["node_generation"] != node["generation"]
        or metadata["adapter_kind"] != ADAPTER_KIND
        or metadata["adapter_content_type"] != adapter_content_type
        or metadata["operation"] != operation
    ):
        raise ValueError("node lifecycle result identity mismatch")
    if metadata["status"] not in ("succeeded", "rejected", "failed"):
        raise ValueError("invalid node lifecycle result status")
    if metadata["resource_state"] not in ("absent", "present", "unknown"):
        raise ValueError("invalid node lifecycle resource state")
    first_error = metadata.get("first_error")
    cleanup_error = metadata.get("cleanup_error")
    if metadata["status"] == "succeeded":
        if first_error is not None or cleanup_error is not None:
            raise ValueError("successful lifecycle result cannot contain an error")
    elif not isinstance(first_error, str) or not first_error:
        raise ValueError("rejected or failed lifecycle result requires first_error")
    if cleanup_error is not None and (
        not isinstance(cleanup_error, str) or not cleanup_error
    ):
        raise ValueError("cleanup_error must be a non-empty string")
    return metadata, opaque


def encode_result(metadata, opaque=b""):
    """Encode fixtures against the same strict result contract."""
    if (
        not isinstance(metadata, dict)
        or not _RESULT_REQUIRED <= set(metadata) <= _RESULT_ALLOWED
    ):
        raise ValueError("node lifecycle result fields differ from the contract")
    _identity(metadata)
    return _encode(metadata, opaque)
