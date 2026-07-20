"""Helpers for presenting local GGUF model catalog entries."""

import os


def model_display_name(rel):
    normalized = rel.replace("\\", "/")
    folder = os.path.basename(os.path.dirname(normalized))
    fallback = os.path.splitext(os.path.basename(normalized))[0]
    name = folder or fallback
    gguf_at = name.lower().find("-gguf")
    if gguf_at >= 0:
        name = name[:gguf_at]
    return name or fallback or rel


def is_embedding_model(rel):
    return "embedding" in rel.replace("\\", "/").lower()


def model_label(rel, size_gib, shard_count=1):
    suffix = f"{size_gib:.2f} GiB"
    if shard_count > 1:
        suffix += f", {shard_count} shards"
    return f"{model_display_name(rel)} ({suffix})"
