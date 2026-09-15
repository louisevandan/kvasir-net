#!/usr/bin/env python3
"""Read only GGUF headers and derive exact total/active parameter counts."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import struct
from pathlib import Path


SCALAR = {
    0: ("B", 1), 1: ("b", 1), 2: ("H", 2), 3: ("h", 2),
    4: ("I", 4), 5: ("i", 4), 6: ("f", 4), 7: ("?", 1),
    10: ("Q", 8), 11: ("q", 8), 12: ("d", 8),
}
SELECTED_KEYS = {
    "general.architecture", "general.size_label", "general.quantization_version",
    "general.file_type", "qwen35moe.block_count", "qwen35moe.context_length",
    "qwen35moe.expert_count", "qwen35moe.expert_used_count",
    "qwen35moe.nextn_predict_layers", "tokenizer.chat_template",
    "split.no", "split.count", "split.tensors.count",
}


def read_exact(stream, count: int) -> bytes:
    data = stream.read(count)
    if len(data) != count:
        raise EOFError(f"wanted {count} bytes, got {len(data)}")
    return data


def unsigned(stream, bits: int) -> int:
    return struct.unpack("<I" if bits == 32 else "<Q", read_exact(stream, bits // 8))[0]


def string(stream) -> str:
    return read_exact(stream, unsigned(stream, 64)).decode("utf-8")


def value(stream, kind: int, keep: bool):
    if kind == 8:
        result = string(stream)
        return result if keep else None
    if kind in SCALAR:
        fmt, size = SCALAR[kind]
        raw = read_exact(stream, size)
        return struct.unpack("<" + fmt, raw)[0] if keep else None
    if kind == 9:
        element = unsigned(stream, 32)
        count = unsigned(stream, 64)
        if element in SCALAR:
            stream.seek(SCALAR[element][1] * count, 1)
        elif element == 8:
            for _ in range(count):
                stream.seek(unsigned(stream, 64), 1)
        else:
            raise ValueError(f"unsupported nested array element type {element}")
        return {"element_type": element, "count": count} if keep else None
    raise ValueError(f"unsupported GGUF metadata type {kind}")


def inspect_file(path: Path) -> dict[str, object]:
    with path.open("rb") as stream:
        if read_exact(stream, 4) != b"GGUF":
            raise ValueError(f"not GGUF: {path}")
        version = unsigned(stream, 32)
        tensor_count = unsigned(stream, 64)
        metadata_count = unsigned(stream, 64)
        metadata = {}
        for _ in range(metadata_count):
            key = string(stream)
            kind = unsigned(stream, 32)
            result = value(stream, kind, key in SELECTED_KEYS)
            if key in SELECTED_KEYS:
                metadata[key] = result
        total_parameters = 0
        expert_parameters = 0
        for _ in range(tensor_count):
            name = string(stream)
            dimensions = [unsigned(stream, 64) for _ in range(unsigned(stream, 32))]
            unsigned(stream, 32)  # ggml type
            unsigned(stream, 64)  # data offset
            parameters = math.prod(dimensions)
            total_parameters += parameters
            if "_exps." in name:
                expert_parameters += parameters
    return {
        "path": path.as_posix(), "bytes": path.stat().st_size, "version": version,
        "tensor_count": tensor_count, "metadata_count": metadata_count,
        "metadata": metadata, "total_parameters": total_parameters,
        "expert_parameters": expert_parameters,
    }


def inspect(paths: list[Path]) -> dict[str, object]:
    if not paths:
        raise ValueError("at least one GGUF shard is required")
    shards = [inspect_file(path) for path in paths]
    metadata = {}
    for shard in shards:
        for key, item in shard["metadata"].items():
            if key in metadata and metadata[key] != item and key not in {"split.no"}:
                raise ValueError(f"metadata differs between shards: {key}")
            metadata[key] = item
    split_count = int(metadata.get("split.count", 1))
    split_numbers = sorted(int(shard["metadata"].get("split.no", 0)) for shard in shards)
    if split_count != len(shards) or split_numbers != list(range(split_count)):
        raise ValueError("GGUF split membership is incomplete or duplicated")
    tensor_count = sum(int(shard["tensor_count"]) for shard in shards)
    if int(metadata.get("split.tensors.count", tensor_count)) != tensor_count:
        raise ValueError("GGUF split tensor count differs")
    expert_count = int(metadata["qwen35moe.expert_count"])
    expert_used = int(metadata["qwen35moe.expert_used_count"])
    if not 0 < expert_used <= expert_count:
        raise ValueError("invalid expert activation ratio")
    total = sum(int(shard["total_parameters"]) for shard in shards)
    expert = sum(int(shard["expert_parameters"]) for shard in shards)
    active_numerator = expert * expert_used
    if active_numerator % expert_count:
        raise ValueError("active expert parameter count is fractional")
    active = total - expert + active_numerator // expert_count
    template = str(metadata["tokenizer.chat_template"])
    return {
        "schema": "p4.release-a.gguf-metadata.v1",
        "shards": shards,
        "architecture": metadata["general.architecture"],
        "size_label": metadata["general.size_label"],
        "quantization_version": metadata["general.quantization_version"],
        "file_type": metadata["general.file_type"],
        "block_count": metadata["qwen35moe.block_count"],
        "context_length": metadata["qwen35moe.context_length"],
        "nextn_predict_layers": metadata["qwen35moe.nextn_predict_layers"],
        "expert_count": expert_count,
        "expert_used_count": expert_used,
        "tensor_count": tensor_count,
        "total_parameters": total,
        "expert_parameters": expert,
        "nonexpert_parameters": total - expert,
        "active_parameters_per_token": active,
        "active_parameter_method": "nonexpert + expert_parameters * expert_used_count / expert_count",
        "chat_template_sha256": hashlib.sha256(template.encode()).hexdigest(),
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("shards", nargs="+", type=Path)
    args = parser.parse_args()
    print(json.dumps(inspect(args.shards), ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
