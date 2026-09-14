import unittest

from p4hfadapter.transport.framing.decoding import decode_header
from p4hfadapter.transport.framing.errors import InvalidFrame, PayloadTooLarge
from p4hfadapter.transport.framing.limits import FrameLimits


class DecodingTests(unittest.TestCase):
    def test_golden_header(self):
        self.assertEqual(decode_header(b"P4HF\x01\x00\x00\x00" + (256).to_bytes(8, "big"),
                                       FrameLimits(256)), 256)

    def test_bad_magic_version_reserved_and_length(self):
        header = b"P4HF\x01" + b"\x00" * 11
        cases = [b"bad!" + header[4:], header[:4] + b"\x02" + header[5:],
                 header[:5] + b"\x01" + header[6:], header[:-1], header + b"x"]
        for value in cases:
            with self.subTest(value=value), self.assertRaises(InvalidFrame):
                decode_header(value, FrameLimits(4))

    def test_oversize_and_u64_max_rejected(self):
        for length in (5, 2**64 - 1):
            with self.subTest(length=length), self.assertRaises(PayloadTooLarge):
                decode_header(b"P4HF\x01\x00\x00\x00" + length.to_bytes(8, "big"), FrameLimits(4))
