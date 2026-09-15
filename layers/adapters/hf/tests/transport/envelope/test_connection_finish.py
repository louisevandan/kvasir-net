import struct
import unittest

from p4hfadapter.integration.transport import Client


class ScriptedSocket:
    def __init__(self, blocks):
        self.blocks = list(blocks)
        self.sent = []
        self.timeout = 120

    def gettimeout(self):
        return self.timeout

    def settimeout(self, value):
        self.timeout = value

    def sendall(self, value):
        self.sent.append(value)

    def recv(self, size):
        if not self.blocks:
            return b""
        block = self.blocks.pop(0)
        if isinstance(block, BaseException):
            raise block
        if len(block) > size:
            self.blocks.insert(0, block[size:])
            return block[:size]
        return block


def client(blocks):
    value = Client.__new__(Client)
    value.socket = ScriptedSocket(blocks)
    value.finishing = False
    value.sent_bytes = 0
    value.received_bytes = 0
    value.finish_unexpected_output = bytearray()
    return value


class ConnectionFinishTests(unittest.TestCase):
    def test_split_ack_finishes_once_and_restores_timeout(self):
        value = client([b"\0", b"\0\0\0"])
        value.finish(timeout=3)
        self.assertEqual(value.socket.sent, [bytes(4)])
        self.assertEqual(value.socket.gettimeout(), 120)
        self.assertEqual(value.received_bytes, 4)
        with self.assertRaisesRegex(RuntimeError, "already finishing"):
            value.finish()

    def test_unexpected_frame_is_fully_preserved(self):
        body = b"x" * 717
        frame = struct.pack("<I", len(body)) + body
        value = client([frame[:2], frame[2:64], frame[64:]])
        with self.assertRaisesRegex(ValueError, "frame_bytes=721 buffered_bytes=721"):
            value.finish()
        self.assertEqual(bytes(value.finish_unexpected_output), frame)
        self.assertEqual(value.received_bytes, len(frame))

    def test_partial_unexpected_frame_is_preserved_on_eof(self):
        body = b"partial"
        frame = struct.pack("<I", len(body)) + body[:3]
        value = client([frame])
        with self.assertRaisesRegex(EOFError, "buffered_bytes=7"):
            value.finish()
        self.assertEqual(bytes(value.finish_unexpected_output), frame)
        self.assertEqual(value.received_bytes, len(frame))

    def test_partial_finish_header_is_preserved_on_eof(self):
        value = client([b"\x07\0"])
        with self.assertRaisesRegex(EOFError, "buffered_bytes=2"):
            value.finish()
        self.assertEqual(bytes(value.finish_unexpected_output), b"\x07\0")
        self.assertEqual(value.received_bytes, 2)

    def test_partial_unexpected_frame_is_preserved_on_timeout(self):
        body = b"partial"
        frame = struct.pack("<I", len(body)) + body[:3]
        value = client([frame, TimeoutError("timed out")])
        with self.assertRaises(TimeoutError):
            value.finish()
        self.assertEqual(bytes(value.finish_unexpected_output), frame)
        self.assertEqual(value.received_bytes, len(frame))

    def test_oversize_is_rejected_before_body_read(self):
        prefix = struct.pack("<I", 40 * 1024 * 1024 + 1)
        value = client([prefix, b"must-not-be-read"])
        with self.assertRaisesRegex(ValueError, "exceeds client bound"):
            value.finish()
        self.assertEqual(bytes(value.finish_unexpected_output), prefix)
        self.assertEqual(len(value.socket.blocks), 1)


if __name__ == "__main__":
    unittest.main()
