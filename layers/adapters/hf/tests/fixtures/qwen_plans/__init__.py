"""Concrete 24-layer plan fixtures without model-runtime imports."""

from p4hfadapter.models.qwen3_5_0_8b.identity import MODEL_ID, REVISION, SCHEMA


def plan():
    return {"schema": SCHEMA, "model_id": MODEL_ID, "revision": REVISION, "dtype": "bfloat16", "quantization": "none",
            "limits": {"context": 2048, "max_requests": 8, "max_new_tokens": 64},
            "nodes": [{"node_id": "first", "host": "local", "device": "cuda:0", "layers": [0, 3]},
                      {"node_id": "attention", "host": "local", "device": "cuda:1", "layers": [3, 4]},
                      {"node_id": "tail", "host": "local", "device": "cuda:0", "layers": [4, 24]}]}
