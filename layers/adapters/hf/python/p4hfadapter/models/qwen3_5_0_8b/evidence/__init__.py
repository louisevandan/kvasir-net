"""Bind each run to its checkpoint, source, plan and scenario bytes."""

import hashlib
import importlib.metadata
import importlib.util
import json
import sys
from pathlib import Path
from p4hfadapter.models.qwen3_5_0_8b.identity import MODEL_ID, REVISION


def digest(path):
    result = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for block in iter(lambda: stream.read(8 * 1024 * 1024), b""):
            result.update(block)
    return result.hexdigest()


def verify_checkpoint(root, directory):
    identity = json.loads((root / "manifests/qwen3_5_0_8b/artifact/identity.json").read_text(encoding="utf-8"))
    for name, expected in identity["files"].items():
        path = directory / name
        if path.stat().st_size != expected["bytes"] or digest(path) != expected["sha256"]:
            raise ValueError(f"checkpoint hash mismatch: {name}")
    return identity


def execution_identity(root, plan_path, scenario_path):
    return {"source_sha256": {p.relative_to(root).as_posix(): digest(p) for p in sorted((root / "python").rglob("*.py"))},
            "plan_sha256": digest(plan_path), "scenario_sha256": digest(scenario_path),
            "python": sys.version, "executable": sys.executable,
            "upstream_sha256": {name: digest(importlib.util.find_spec(name).origin) for name in (
                "transformers.models.qwen3_5.modeling_qwen3_5", "transformers.cache_utils", "transformers.masking_utils")},
            "packages": {name: importlib.metadata.version(name) for name in ("torch", "transformers", "safetensors", "huggingface-hub")}}


def checkpoint_identity(path: Path) -> dict:
    result = {}
    for file in sorted(path.iterdir()):
        if file.is_file():
            digest = hashlib.sha256()
            with file.open("rb") as stream:
                for block in iter(lambda: stream.read(8 * 1024 * 1024), b""):
                    digest.update(block)
            result[file.name] = {"sha256": digest.hexdigest(), "bytes": file.stat().st_size}
    return {"model_id": MODEL_ID, "revision": REVISION, "files": result}
