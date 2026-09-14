import sys
import unittest

from p4hfadapter.transport.framing.limits import FrameLimits


class LimitsTests(unittest.TestCase):
    def test_limit_is_explicit_positive_platform_size(self):
        for value in (0, -1, True, 1.5, "10", sys.maxsize + 1):
            with self.subTest(value=value), self.assertRaises(ValueError):
                FrameLimits(value)
        self.assertEqual(FrameLimits(4).max_payload_bytes, 4)

    def test_limits_are_immutable(self):
        with self.assertRaises(AttributeError):
            FrameLimits(4).max_payload_bytes = 8
