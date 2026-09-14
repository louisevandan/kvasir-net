"""The single supported checkpoint and audited upstream implementation."""

MODEL_ID = "Qwen/Qwen3.5-0.8B"
REVISION = "2fc06364715b967f1860aea9cf38778875588b17"
TRANSFORMERS_VERSION = "5.17.0"
TORCH_VERSION = "2.14.0+cu130"
SCHEMA = "qwen3.5-0.8b-plan-v1"
LAYERS = 24
LAYER_TYPES = ["linear_attention"] * 3 + ["full_attention"]
