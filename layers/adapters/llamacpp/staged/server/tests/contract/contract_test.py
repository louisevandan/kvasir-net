#!/usr/bin/env python3
"""Executable, dependency-free contract tests for the staged local protocol."""

from __future__ import annotations

import io
import json
import os
import struct
import threading
import time
import unittest
from enum import Enum
from pathlib import Path


MAGIC = b"LCP4"
REVISION = 1
HEADER = struct.Struct("<4sHBBI")
PLAN_LENGTH = struct.Struct("<I")
MAX_PLAN_BYTES = 1024 * 1024


class Op(str, Enum):
    HELLO = ("HELLO", 1)
    HOP = ("HOP", 2)
    HOP_RESULT = ("HOP_RESULT", 3)
    CANCEL = ("CANCEL", 4)
    KV_SAVE = ("KV_SAVE", 5)
    KV_RESTORE = ("KV_RESTORE", 6)
    KV_DROP = ("KV_DROP", 7)
    KV_RESULT = ("KV_RESULT", 8)
    UNLOAD = ("UNLOAD", 9)
    ERROR = ("ERROR", 10)

    def __new__(cls, label: str, number: int):
        member = str.__new__(cls, label)
        member._value_ = label
        member.number = number
        return member


class ContractError(Exception):
    pass


def frame(operation: Op, body: bytes = b"") -> bytes:
    """Encode the LCP4 header used by the staged adapter."""
    return HEADER.pack(MAGIC, REVISION, operation.number, 0, len(body)) + body


def decode_frame(raw: bytes) -> tuple[Op, bytes]:
    if len(raw) < HEADER.size:
        raise ContractError("truncated frame")
    magic, revision, operation, flags, body_bytes = HEADER.unpack(raw[: HEADER.size])
    if magic != MAGIC or revision != REVISION or flags != 0:
        raise ContractError("invalid frame header")
    if len(raw) != HEADER.size + body_bytes:
        raise ContractError("frame length mismatch")
    try:
        op = next(item for item in Op if item.number == operation)
    except StopIteration:
        raise ContractError("unknown operation") from None
    return op, raw[HEADER.size :]


class Session:
    """Small reference state machine for local adapter/server sequencing."""

    def __init__(self) -> None:
        self.ready = False
        self.active_hop: int | None = None
        self.closed = False

    def accept(self, operation: Op, hop_id: int | None = None) -> str:
        if self.closed:
            raise ContractError("operation after unload")
        if operation is Op.HELLO:
            if self.ready or self.active_hop is not None:
                raise ContractError("HELLO is only the pre-ready exception")
            self.ready = True
            return "READY"
        if not self.ready:
            raise ContractError("HELLO required")
        if operation is Op.HOP:
            if self.active_hop is not None:
                raise ContractError("only one HOP may be in flight")
            if hop_id is None:
                raise ContractError("HOP requires hop_id")
            self.active_hop = hop_id
            return "HOP_ACCEPTED"
        if operation is Op.HOP_RESULT:
            self._require_current(hop_id)
            self.active_hop = None
            return "HOP_COMPLETE"
        if operation is Op.CANCEL:
            self._require_current(hop_id)
            self.active_hop = None
            return "HOP_CANCELLED"
        if operation in (Op.KV_SAVE, Op.KV_RESTORE, Op.KV_DROP):
            if self.active_hop is not None:
                raise ContractError("KV operation before HOP completion")
            return "KV_ACCEPTED"
        if operation is Op.UNLOAD:
            if self.active_hop is not None:
                raise ContractError("UNLOAD before HOP completion")
            self.closed = True
            self.ready = False
            return "UNLOADED"
        raise ContractError("unsupported operation")

    def _require_current(self, hop_id: int | None) -> None:
        if self.active_hop is None:
            raise ContractError("no current HOP")
        if hop_id != self.active_hop:
            raise ContractError("operation targets a non-current HOP")


def encode_plan(plan: str) -> bytes:
    payload = plan.encode("utf-8")
    if len(payload) > MAX_PLAN_BYTES:
        raise ContractError("plan too large")
    return PLAN_LENGTH.pack(len(payload)) + payload


def read_plan(stream: io.BufferedIOBase) -> str:
    prefix = stream.read(PLAN_LENGTH.size)
    if len(prefix) != PLAN_LENGTH.size:
        raise ContractError("truncated plan length")
    size = PLAN_LENGTH.unpack(prefix)[0]
    if size > MAX_PLAN_BYTES:
        raise ContractError("plan too large")
    payload = stream.read(size)
    if len(payload) != size:
        raise ContractError("truncated plan")
    return payload.decode("utf-8")


class ContractTests(unittest.TestCase):
    def test_fixture_runs_as_sequential_mutating_session(self) -> None:
        fixture = json.loads((Path(__file__).parent / "fixtures" / "valid_contract.json").read_text())
        self.assertEqual(read_plan(io.BytesIO(encode_plan(fixture["plan"]))), fixture["plan"])
        session = Session()
        for item in fixture["operations"]:
            result = session.accept(Op(item["op"]), item.get("hop_id"))
            self.assertTrue(result)
        self.assertTrue(session.closed)

    def test_hello_is_the_only_pre_ready_exception(self) -> None:
        session = Session()
        self.assertEqual(session.accept(Op.HELLO), "READY")
        with self.assertRaises(ContractError):
            session.accept(Op.HELLO)

    def test_only_one_hop_and_cancel_current_hop_only(self) -> None:
        session = Session()
        session.accept(Op.HELLO)
        session.accept(Op.HOP, 42)
        with self.assertRaises(ContractError):
            session.accept(Op.HOP, 43)
        with self.assertRaises(ContractError):
            session.accept(Op.CANCEL, 43)
        self.assertEqual(session.active_hop, 42)
        self.assertEqual(session.accept(Op.CANCEL, 42), "HOP_CANCELLED")

    def test_kv_and_unload_wait_for_hop_completion(self) -> None:
        session = Session()
        session.accept(Op.HELLO)
        session.accept(Op.HOP, 1)
        for operation in (Op.KV_SAVE, Op.KV_RESTORE, Op.KV_DROP, Op.UNLOAD):
            with self.assertRaises(ContractError):
                session.accept(operation)
        session.accept(Op.HOP_RESULT, 1)
        session.accept(Op.KV_SAVE)
        self.assertEqual(session.accept(Op.UNLOAD), "UNLOADED")

    def test_wire_frame_is_length_consistent(self) -> None:
        raw = frame(Op.HOP, b"descriptor-payload")
        operation, body = decode_frame(raw)
        self.assertEqual(operation, Op.HOP)
        self.assertEqual(body, b"descriptor-payload")
        with self.assertRaises(ContractError):
            decode_frame(raw[:-1])

    def test_stdin_plan_stays_alive_until_parent_eof(self) -> None:
        read_fd, write_fd = os.pipe()
        received: list[str] = []
        finished = threading.Event()

        def server_reader() -> None:
            with os.fdopen(read_fd, "rb", closefd=True) as stream:
                received.append(read_plan(stream))
                # The same sole reader now waits for parent liveness EOF.
                self.assertEqual(stream.read(), b"")
            finished.set()

        thread = threading.Thread(target=server_reader, daemon=True)
        thread.start()
        os.write(write_fd, encode_plan("--ctx-size 4096 --model C:\\models\\m.gguf"))
        time.sleep(0.05)
        self.assertFalse(finished.is_set(), "plan completion must not close stdin")
        os.close(write_fd)
        self.assertTrue(finished.wait(1.0), "parent EOF must terminate the liveness reader")
        thread.join(1.0)
        self.assertEqual(received, ["--ctx-size 4096 --model C:\\models\\m.gguf"])


if __name__ == "__main__":
    unittest.main(verbosity=2)
