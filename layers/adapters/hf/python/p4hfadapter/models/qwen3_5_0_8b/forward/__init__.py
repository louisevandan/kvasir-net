"""Qwen 3.5 text-only partial forward with stage-local cache indices."""

from copy import deepcopy
import torch
from torch import nn
from transformers.masking_utils import create_causal_mask
from transformers.models.qwen3_5.modeling_qwen3_5 import Qwen3_5DecoderLayer, Qwen3_5RMSNorm, Qwen3_5TextRotaryEmbedding


class QwenStage(nn.Module):
    def __init__(self, config, node):
        super().__init__()
        self.node = node
        self.config = deepcopy(config)
        self.config.layer_types = config.layer_types[node.start:node.end]
        self.config.num_hidden_layers = node.end - node.start
        self.config._attn_implementation = "eager"
        self.layers = nn.ModuleDict({str(global_idx): Qwen3_5DecoderLayer(self.config, local_idx)
                                    for local_idx, global_idx in enumerate(range(node.start, node.end))})
        if node.start == 0:
            self.embedding = nn.Embedding(config.vocab_size, config.hidden_size)
        if node.end == 24:
            self.norm = Qwen3_5RMSNorm(config.hidden_size, eps=config.rms_norm_eps)
            self.head = nn.Linear(config.hidden_size, config.vocab_size, bias=False)

    @torch.inference_mode()
    def forward(self, tensor, cache, position):
        hidden = self.embedding(tensor) if self.node.start == 0 else tensor
        positions = torch.arange(position, position + hidden.shape[1], device=hidden.device)[None, :]
        rotary = self.rotary(hidden, positions[None, ...].expand(3, -1, -1))
        mask = None
        if "full_attention" in self.config.layer_types:
            mask = create_causal_mask(config=self.config, inputs_embeds=hidden, attention_mask=None,
                                      past_key_values=cache, position_ids=positions,
                                      layer_idx=self.config.layer_types.index("full_attention"))
        for layer in self.layers.values():
            hidden = layer(hidden, position_embeddings=rotary, position_ids=positions,
                           attention_mask=mask if layer.block_type == "full_attention" else None,
                           past_key_values=cache, use_cache=True)
        if self.node.end == 24:
            return self.head(self.norm(hidden[:, -1:, :])).float()
        return hidden

    def initialize_rotary(self, device):
        self.rotary = Qwen3_5TextRotaryEmbedding(config=self.config).to(device)
