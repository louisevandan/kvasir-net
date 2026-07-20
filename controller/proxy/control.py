"""Local binary control pipe for the first rank in a linkcpp ring."""

from __future__ import annotations

import struct
import time
from typing import Any, BinaryIO


VERSION = 1
FLAG_CHAT_MESSAGES = 1
FLAG_TOKEN_IDS = 4
REQUEST = struct.Struct("<4sBBH Q I I Q")
RESPONSE = struct.Struct("<4sBBH Q I I I I")
MAX_PROMPT_BYTES = 16 * 1024 * 1024
MAX_RESPONSE_BYTES = 64 * 1024 * 1024


def _encode_messages(messages: list[dict[str, str]]) -> bytes:
    if not messages or len(messages) > 1024:
        raise ValueError("ring chat must contain between 1 and 1024 messages")
    parts = [struct.pack("<I", len(messages))]
    for message in messages:
        role = str(message.get("role") or "user").encode("utf-8")
        content = str(message.get("content") or "").encode("utf-8")
        if not role or len(role) > 64 or len(content) > MAX_PROMPT_BYTES:
            raise ValueError("invalid ring chat message")
        parts.extend((struct.pack("<II", len(role), len(content)), role, content))
    return b"".join(parts)


def encode_request(prompt: str, max_tokens: int, request_id: int,
                   messages: list[dict[str, str]] | None = None) -> bytes:
    flags = FLAG_CHAT_MESSAGES if messages else 0
    raw = _encode_messages(messages) if messages else prompt.encode("utf-8")
    if not raw or len(raw) > MAX_PROMPT_BYTES:
        raise ValueError("ring prompt must be between 1 byte and 16 MiB")
    if not 1 <= int(max_tokens) <= 4096:
        raise ValueError("ring max_tokens must be between 1 and 4096")
    if not 0 < int(request_id) < 2**64:
        raise ValueError("ring request_id must fit in an unsigned 64-bit integer")
    return REQUEST.pack(b"LKC1", VERSION, flags, 0, int(request_id), len(raw), int(max_tokens), 0) + raw


def encode_token_request(tokens: list[int], request_id: int) -> bytes:
    if not tokens or len(tokens) > 4096 or any(not -(2**31) <= int(token) < 2**31 for token in tokens):
        raise ValueError("ring token input must contain 1 to 4096 signed 32-bit token IDs")
    if not 0 < int(request_id) < 2**64:
        raise ValueError("ring request_id must fit in an unsigned 64-bit integer")
    raw = struct.pack(f"<{len(tokens)}i", *map(int, tokens))
    return REQUEST.pack(b"LKC1", VERSION, FLAG_TOKEN_IDS, 0, int(request_id), len(raw), 1, 0) + raw


def _read_exact(stream: BinaryIO, size: int) -> bytes:
    chunks = []
    remaining = size
    while remaining:
        chunk = stream.read(remaining)
        if not chunk:
            raise RuntimeError("ring stage control pipe closed")
        chunks.append(chunk)
        remaining -= len(chunk)
    return b"".join(chunks)


def decode_response(stream: BinaryIO, expected_request_id: int) -> dict[str, Any]:
    header = _read_exact(stream, RESPONSE.size)
    magic, version, status, flags, request_id, text_bytes, token_count, elapsed_ms, reserved = RESPONSE.unpack(header)
    if magic != b"LKR1" or version != VERSION or flags != 0 or reserved != 0:
        raise RuntimeError("invalid ring stage control response")
    if request_id != expected_request_id:
        raise RuntimeError("ring stage response request_id mismatch")
    if text_bytes > MAX_RESPONSE_BYTES:
        raise RuntimeError("ring stage response is too large")
    text = _read_exact(stream, text_bytes).decode("utf-8", "replace") if text_bytes else ""
    result = {
        "request_id": request_id,
        "text": text,
        "tokens": token_count,
        "elapsed_ms": elapsed_ms,
        "status": "done" if status == 0 else "error",
    }
    if status != 0:
        raise RuntimeError(text or "ring stage inference failed")
    return result


def decode_token_response(stream: BinaryIO, expected_request_id: int) -> dict[str, Any]:
    header = _read_exact(stream, RESPONSE.size)
    magic, version, status, flags, request_id, body_bytes, token_count, elapsed_ms, reserved = RESPONSE.unpack(header)
    if magic != b"LKR1" or version != VERSION or reserved != 0 or request_id != expected_request_id:
        raise RuntimeError("invalid ring token-eval response")
    if body_bytes > MAX_RESPONSE_BYTES:
        raise RuntimeError("ring token-eval response is too large")
    body = _read_exact(stream, body_bytes) if body_bytes else b""
    if status != 0:
        raise RuntimeError(body.decode("utf-8", "replace") or "ring token evaluation failed")
    if flags != FLAG_TOKEN_IDS or token_count != 1 or len(body) != 4:
        raise RuntimeError("invalid ring token-eval payload")
    return {
        "request_id": request_id,
        "token": struct.unpack("<i", body)[0],
        "elapsed_ms": elapsed_ms,
        "status": "done",
    }


def infer_process(process: Any, prompt: str, max_tokens: int, request_id: int | None = None,
                  messages: list[dict[str, str]] | None = None) -> dict[str, Any]:
    if process is None or process.poll() is not None or process.stdin is None or process.stdout is None:
        raise RuntimeError("first ring stage is not running with control pipes")
    request_id = int(request_id or (time.time_ns() & ((1 << 63) - 1)) or 1)
    process.stdin.write(encode_request(prompt, max_tokens, request_id, messages))
    process.stdin.flush()
    return decode_response(process.stdout, request_id)


def infer_tokens_process(process: Any, tokens: list[int], request_id: int | None = None) -> dict[str, Any]:
    if process is None or process.poll() is not None or process.stdin is None or process.stdout is None:
        raise RuntimeError("first ring stage is not running with control pipes")
    request_id = int(request_id or (time.time_ns() & ((1 << 63) - 1)) or 1)
    process.stdin.write(encode_token_request(tokens, request_id))
    process.stdin.flush()
    return decode_token_response(process.stdout, request_id)
