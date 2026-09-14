"""Read only this Qwen stage's tensors, including the explicit tied head copy."""

import importlib.metadata
import json
from pathlib import Path
import torch
from safetensors import safe_open
from transformers.models.qwen3_5.configuration_qwen3_5 import Qwen3_5TextConfig

from p4hfadapter.models.qwen3_5_0_8b.forward import QwenStage
from p4hfadapter.models.qwen3_5_0_8b.identity import LAYER_TYPES, TORCH_VERSION, TRANSFORMERS_VERSION


def runtime_check():
    if importlib.metadata.version("transformers") != TRANSFORMERS_VERSION or torch.__version__ != TORCH_VERSION:
        raise ValueError(f"audited runtime requires transformers {TRANSFORMERS_VERSION}, torch {TORCH_VERSION}")
    torch.set_num_threads(4)
    torch.backends.cuda.matmul.allow_tf32 = False
    torch.backends.cudnn.allow_tf32 = False


def text_config(directory: Path):
    raw = json.loads((directory / "config.json").read_text(encoding="utf-8"))
    text = raw.get("text_config", {})
    if (raw.get("model_type"), text.get("hidden_size"), text.get("num_hidden_layers"), text.get("vocab_size"),
        text.get("layer_types"), text.get("tie_word_embeddings")) != (
            "qwen3_5", 1024, 24, 248320, LAYER_TYPES * 6, True):
        raise ValueError("checkpoint is not the audited Qwen3.5-0.8B text configuration")
    config = Qwen3_5TextConfig(**text)
    config._attn_implementation = "eager"
    return config


def load_stage(directory: Path, node, dtype: str):
    runtime_check()
    config = text_config(directory)
    with torch.device("meta"):
        stage = QwenStage(config, node)
    index = json.loads((directory / "model.safetensors.index.json").read_text(encoding="utf-8"))["weight_map"]
    mapping = {}
    for name in stage.state_dict():
        if name in ("embedding.weight", "head.weight"):
            key = "model.language_model.embed_tokens.weight"
        elif name == "norm.weight":
            key = "model.language_model.norm.weight"
        else:
            key = "model.language_model." + name
        if key not in index:
            raise ValueError(f"missing required tensor {key}")
        mapping[name] = key
    weights, loaded = {}, {}
    for name, key in mapping.items():
        if key not in loaded:
            file = (directory / index[key]).resolve()
            if not file.is_relative_to(directory.resolve()):
                raise ValueError("checkpoint shard escapes its directory")
            with safe_open(file, framework="pt", device="cpu") as reader:
                loaded[key] = reader.get_tensor(key).to(device=node.device, dtype=getattr(torch, dtype))
        weights[name] = loaded[key]
    stage.load_state_dict(weights, strict=True, assign=True)
    if node.start == 0 and node.end == 24:
        stage.head.weight = stage.embedding.weight
    stage.initialize_rotary(node.device)
    stage.eval()
    stage.requires_grad_(False)
    if any(parameter.is_meta for parameter in stage.parameters()):
        raise RuntimeError("unloaded meta weight remains")
    report = {"node_id": node.node_id, "layers": [node.start, node.end], "device": node.device,
              "device_name": torch.cuda.get_device_name(node.device) if node.device.startswith("cuda") else "CPU",
              "tensor_names": sorted(loaded), "weight_bytes": sum(x.numel() * x.element_size() for x in loaded.values()),
              "dtype": dtype, "quantization": "none", "attention": "eager", "linear_attention": "torch fallback",
              "cuda_allocated": torch.cuda.memory_allocated(node.device) if node.device.startswith("cuda") else None}
    return stage, report
