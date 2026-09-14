"""Exercise the real StageSessions admission guard without allocating weights."""
from pathlib import Path
import sys
from types import SimpleNamespace
import unittest

root = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(root / "python"))
import torch
from transformers.models.qwen3_5.configuration_qwen3_5 import Qwen3_5TextConfig
from p4hfadapter.models.qwen3_5_0_8b.configuration import Plan
from p4hfadapter.models.qwen3_5_0_8b.state import StageSessions


class Stage:
    node = SimpleNamespace(start=0, device="cpu")
    config = Qwen3_5TextConfig(num_hidden_layers=1, layer_types=["full_attention"])
    def __init__(self): self.calls = 0
    def __call__(self, tensor, cache, position):
        self.calls += 1
        return tensor.float()


class ShapeTests(unittest.TestCase):
    def test_oversize_has_no_forward_state_or_retirement_effect(self):
        stage = Stage()
        sessions = StageSessions(stage, Plan((), "float32", 64, 2, 8, 16))
        with self.assertRaisesRegex(ValueError, "profiled prefill_chunk"):
            sessions.step("request", 0, 0, torch.ones((1, 17), dtype=torch.int64))
        self.assertEqual((stage.calls, sessions.active, sessions.retired), (0, {}, set()))
        sessions.step("request", 0, 0, torch.ones((1, 16), dtype=torch.int64))
        self.assertEqual(stage.calls, 1)
        sessions.release("request")
        self.assertEqual(sessions.active, {})


if __name__ == "__main__":
    unittest.main()
