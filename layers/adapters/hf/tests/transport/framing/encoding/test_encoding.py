import unittest

from p4hfadapter.transport.framing.encoding import encode_header
from p4hfadapter.transport.framing.errors import InvalidFrame, PayloadTooLarge
from p4hfadapter.transport.framing.limits import FrameLimits


class EncodingTests(unittest.TestCase):
    def test_golden_header_uses_network_byte_order(self):
        self.assertEqual(encode_header(3, FrameLimits(3)),
                         b"P4HF\x01\x00\x00\x00" + (3).to_bytes(8, "big"))

    def test_zero_payload_is_a_frame(self):
        self.assertEqual(encode_header(0, FrameLimits(1)), b"P4HF\x01" + b"\x00" * 11)

    def test_size_rejected_before_serialization(self):
        with self.assertRaises(PayloadTooLarge):
            encode_header(5, FrameLimits(4))
        for value in (-1, True, 1.5, "1", 2**64):
            with self.subTest(value=value), self.assertRaises(InvalidFrame):
                encode_header(value, FrameLimits(4))
