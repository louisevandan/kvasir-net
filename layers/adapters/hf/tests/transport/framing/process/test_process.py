import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import unittest


class ProcessTests(unittest.TestCase):
    def run_peer(self, raw):
        root = Path(__file__).resolve().parents[4]
        env = dict(os.environ, PYTHONPATH=str(root / "python"), PYTHONDONTWRITEBYTECODE="1", PYTHONUTF8="1")
        completed = subprocess.run([sys.executable, "-B", str(root / "tests/fixtures/framing_peer/main.py")],
                                   input=raw, capture_output=True, timeout=10, cwd=root, env=env)
        diagnostics = [json.loads(line) for line in completed.stderr.decode("utf-8").splitlines()]
        decoder = root / "python/p4hfadapter/transport/framing/decoding/__init__.py"
        self.assertEqual(Path(diagnostics[0]["decoder_source"]), decoder)
        self.assertEqual(diagnostics[0]["decoder_sha256"], hashlib.sha256(decoder.read_bytes()).hexdigest())
        return completed, diagnostics[-1]

    def test_opaque_binary_round_trip_and_clean_eof(self):
        payload = b"\x00\xff\nnot-json\x80"
        frame = b"P4HF\x01\x00\x00\x00" + len(payload).to_bytes(8, "big") + payload
        wire = frame + b"P4HF\x01" + b"\x00" * 11
        result, last = self.run_peer(wire)
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout, wire)
        self.assertEqual(last["consumed"], 2)

    def test_invalid_header_has_zero_consumption_and_output(self):
        valid = b"P4HF\x01" + b"\x00" * 11
        for raw in (b"BAD!" + valid[4:], valid[:4] + b"\x02" + valid[5:],
                    valid[:5] + b"\x01" + valid[6:],
                    valid[:8] + (1025).to_bytes(8, "big")):
            with self.subTest(raw=raw):
                result, last = self.run_peer(raw + valid)
                self.assertEqual(result.returncode, 2)
                self.assertEqual(result.stdout, b"")
                self.assertEqual(last["consumed"], 0)

    def test_truncated_frame_has_zero_consumption_and_output(self):
        for raw in (b"P4H", b"P4HF\x01\x00\x00\x00" + (3).to_bytes(8, "big") + b"ab"):
            with self.subTest(raw=raw):
                result, last = self.run_peer(raw)
                self.assertEqual(result.returncode, 2)
                self.assertEqual(result.stdout, b"")
                self.assertEqual(last, {"error": "TruncatedFrame", "consumed": 0})

    def test_limit_boundary_round_trip(self):
        wire = b"P4HF\x01\x00\x00\x00" + (1024).to_bytes(8, "big") + b"x" * 1024
        result, last = self.run_peer(wire)
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout, wire)
        self.assertEqual(last["consumed"], 1)
