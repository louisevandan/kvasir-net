"""Acquire the pinned public checkpoint; imports no remote model code."""

from pathlib import Path

from p4hfadapter.models.qwen3_5_0_8b.identity import MODEL_ID, REVISION


def prepare(cache_dir: Path) -> Path:
    from huggingface_hub import snapshot_download
    return Path(snapshot_download(MODEL_ID, revision=REVISION, cache_dir=str(cache_dir),
                allow_patterns=["config.json", "*safetensors*", "tokenizer*", "chat_template.jinja",
                                "merges.txt", "vocab.json", "LICENSE"]))
