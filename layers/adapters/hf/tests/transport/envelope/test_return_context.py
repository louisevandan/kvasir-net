"""Actual OUTER reader rejects incomplete or contradictory return envelopes."""
import io
import struct
import unittest
from p4hfadapter.integration.transport import Client, endpoint, text

class ReturnContextTests(unittest.TestCase):
    def test_response_context_is_checked_before_trace_acceptance(self):
        outer = (2, "tcp://127.0.0.1:41999", "owner", 7)
        other = (2, "tcp://127.0.0.2:41999", "other", 8)
        for route in (None, other, outer):
            with self.subTest(route=route):
                env = struct.pack("<H", 3) + text("reply") + text("request") + b"\0"
                env += endpoint((1, "tcp://127.0.0.3:41999", "node", 1)) + endpoint(outer)
                env += b"\0" if route is None else b"\1" + endpoint(route)[1:]
                env += b"\2" + struct.pack("<Q", 1) + b"\0\0" + text("application/test")
                frame = b"P4E3" + struct.pack("<II", len(env), 0) + env
                reader = io.BytesIO(struct.pack("<I", len(frame)) + frame)
                client = Client.__new__(Client)
                client.outer, client.trace, client.received_bytes = outer, [], 0
                client.exact = reader.read
                if route == outer:
                    meta, body = client.receive()
                    self.assertEqual(meta["return_route"], outer)
                    self.assertEqual(body, b"")
                    self.assertEqual(len(client.trace), 1)
                else:
                    with self.assertRaisesRegex(ValueError, "envelope mismatch"):
                        client.receive()
                    self.assertEqual(client.trace, [])
                    self.assertEqual(client.received_bytes, 0)
