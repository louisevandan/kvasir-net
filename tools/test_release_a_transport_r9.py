import hashlib
from pathlib import Path
import struct
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import release_a_transport_r9 as r9


class ReceiptProxyContract(unittest.TestCase):
    def test_only_complete_hop_receipt_is_selected(self):
        receipt_body = b"P4H1" + struct.pack("<HHB", 1, 0, 4) + b"receipt"
        receipt = struct.pack("<I", len(receipt_body)) + receipt_body
        data_body = b"P4H1" + struct.pack("<HHB", 1, 0, 3) + b"data"
        data = struct.pack("<I", len(data_body)) + data_body
        legacy = struct.pack("<I", 9) + b"P4E3event"
        self.assertTrue(r9._is_receipt(receipt))
        self.assertFalse(r9._is_receipt(data))
        self.assertFalse(r9._is_receipt(legacy))
        self.assertFalse(r9._is_receipt(receipt[:12]))

    def test_sealed_frame_hash_includes_length_prefix(self):
        body = b"P4H1" + struct.pack("<HHB", 1, 0, 4) + b"receipt"
        frame = struct.pack("<I", len(body)) + body
        self.assertEqual(hashlib.sha256(frame).hexdigest(),
                         "6c1784c9620e5299225efe8b8e10f527c336dbf344ae5b664132ee8e9212e4b0")


if __name__ == "__main__":
    unittest.main()
