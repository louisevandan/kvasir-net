"""Effect counter for admission tests, never used as inference evidence."""

from types import SimpleNamespace
import torch
from transformers.models.qwen3_5.configuration_qwen3_5 import Qwen3_5TextConfig


class RejectionStage:
    def __init__(self, start=0):
        self.node = SimpleNamespace(start=start, device="cpu")
        self.config = Qwen3_5TextConfig()
        self.calls = 0

    def __call__(self, tensor, cache, position):
        self.calls += 1
        return torch.zeros(1, 1, 10)
