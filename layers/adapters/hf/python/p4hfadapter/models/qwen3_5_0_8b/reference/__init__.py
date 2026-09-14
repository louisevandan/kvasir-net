"""Independent official full-model forward for teacher-forced parity."""

import torch
from transformers import Qwen3_5ForConditionalGeneration


class Reference:
    def __init__(self, directory, dtype, device):
        self.model = Qwen3_5ForConditionalGeneration.from_pretrained(
            directory, local_files_only=True, dtype=getattr(torch, dtype), attn_implementation="eager").to(device).eval()
        self.device, self.caches, self.comparisons = device, {}, []

    @torch.inference_mode()
    def compare(self, session_id, tokens, actual):
        result = self.model(input_ids=tokens.to(self.device), past_key_values=self.caches.get(session_id),
                            use_cache=True, logits_to_keep=1)
        self.caches[session_id] = result.past_key_values
        expected = result.logits.float().cpu()
        difference = (actual - expected).abs()
        matched = bool(torch.allclose(actual, expected, atol=0.125, rtol=0.01))
        token_match = int(actual.argmax(-1).item()) == int(expected.argmax(-1).item())
        record = {"session_id": session_id, "max_abs_logits": difference.max().item(),
                  "allclose": matched, "greedy_token_match": token_match}
        self.comparisons.append(record)
        if not matched or not token_match:
            raise AssertionError(f"reference parity failed: {record}")

    def release(self, session_id):
        self.caches.pop(session_id, None)
