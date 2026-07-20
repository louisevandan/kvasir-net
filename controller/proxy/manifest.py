"""Rank-local GGUF loading manifests for the linkcpp ring runtime.

The controller owns placement.  A worker never receives model weights from the
controller: it proves that its local GGUF has the same tensor index, then loads
only the tensor names assigned to its contiguous layer window.  The C++ stage
adapter consumes the resulting manifest in the next milestone.
"""

from __future__ import annotations

import hashlib
import os
from typing import Any, Iterable, Mapping

try:  # The controller image provides gguf; tests can inject a reader instead.
    from gguf import GGUFReader
except ImportError:  # pragma: no cover - exercised by unit-test doubles
    GGUFReader = None

try:
    from gguf import GGUFWriter, GGUFValueType
except ImportError:  # pragma: no cover
    GGUFWriter = None
    GGUFValueType = None


MANIFEST_VERSION = 1


def summarize_stage_manifest(manifest: Mapping[str, Any] | None) -> dict[str, Any] | None:
    """Return a bounded state/report form after the full manifest is verified."""
    if not manifest:
        return None
    identity = dict(manifest.get("identity") or {})
    return {
        "version": manifest.get("version"),
        "plan_id": manifest.get("plan_id"),
        "model_ref": manifest.get("model_ref"),
        "stage_index": manifest.get("stage_index"),
        "stage_count": manifest.get("stage_count"),
        "layers": list(manifest.get("layers") or []),
        "owns_input": bool(manifest.get("owns_input")),
        "owns_output": bool(manifest.get("owns_output")),
        "tensor_count": len(manifest.get("tensor_names") or []),
        "identity": {
            key: identity.get(key)
            for key in ("architecture", "n_layer", "n_embd", "tensor_index_sha256", "tensor_count")
        },
    }


def _scalar(field: Any) -> Any:
    """Small GGUF-field conversion kept independent from planner imports."""
    if field is None:
        return None
    try:
        return field.contents()
    except Exception:
        try:
            return field.parts[field.data[0]].tolist()
        except Exception:
            return field


def _reader(path: str):
    if GGUFReader is None:
        raise RuntimeError("GGUF support is not installed")
    return GGUFReader(path)


def _safe_relative(path: str) -> str:
    normalized = str(path or "").replace("\\", "/").strip("/")
    if not normalized or normalized.startswith("../") or "/../" in normalized:
        raise ValueError("model path must be a relative path within the model directory")
    return normalized


def _tensor_index(readers: Iterable[Any]) -> list[tuple[str, int]]:
    tensors: dict[str, int] = {}
    for reader in readers:
        for tensor in reader.tensors:
            name = str(tensor.name)
            size = int(tensor.n_bytes)
            existing = tensors.setdefault(name, size)
            if existing != size:
                raise ValueError(f"inconsistent tensor size for {name}")
    return sorted(tensors.items())


def _model_identity(paths: Iterable[str]) -> dict[str, Any]:
    paths = list(paths)
    if not paths:
        raise ValueError("a manifest needs at least one GGUF shard")
    readers = [_reader(path) for path in paths]
    primary = readers[0]
    fields = {name: _scalar(value) for name, value in primary.fields.items()}
    arch = str(fields.get("general.architecture", "llama"))
    n_layer_all = int(fields.get(f"{arch}.block_count", 0) or 0)
    n_layer_nextn = int(fields.get(f"{arch}.nextn_predict_layers", 0) or 0)
    n_layer = n_layer_all - n_layer_nextn
    if n_layer <= 0:
        raise ValueError("model has no executable transformer layers")
    n_embd = int(fields.get(f"{arch}.embedding_length", 0) or 0)
    index = _tensor_index(readers)
    digest = hashlib.sha256()
    digest.update(f"{arch}\0{n_layer}\0{n_embd}\0".encode("utf-8"))
    for name, size in index:
        digest.update(name.encode("utf-8"))
        digest.update(b"\0")
        digest.update(str(size).encode("ascii"))
        digest.update(b"\n")
    return {
        "architecture": arch,
        "n_layer": n_layer,
        "n_embd": n_embd,
        "tensor_index_sha256": digest.hexdigest(),
        "tensor_count": len(index),
        "tensor_sizes": dict(index),
    }


def build_stage_manifest(
    *,
    plan_id: str,
    model_ref: str,
    model_root: str,
    shard_refs: Iterable[str],
    placement: Mapping[str, Any],
    stage_index: int,
    stage_count: int,
) -> dict[str, Any]:
    """Build one immutable, rank-specific local loading contract."""
    refs = [_safe_relative(item) for item in shard_refs]
    paths = [os.path.join(model_root, item) for item in refs]
    identity = _model_identity(paths)
    return _build_manifest_from_identity(
        plan_id, model_ref, refs, placement, stage_index, stage_count, identity,
    )


def build_stage_manifests(
    *,
    plan_id: str,
    model_ref: str,
    model_root: str,
    shard_refs: Iterable[str],
    placements: Iterable[Mapping[str, Any]],
) -> list[dict[str, Any]]:
    """Build all rank manifests after reading a split GGUF index only once."""
    refs = [_safe_relative(item) for item in shard_refs]
    paths = [os.path.join(model_root, item) for item in refs]
    identity = _model_identity(paths)
    placements = list(placements)
    return [
        _build_manifest_from_identity(plan_id, model_ref, refs, placement, index, len(placements), identity)
        for index, placement in enumerate(placements)
    ]


def _build_manifest_from_identity(plan_id, model_ref, refs, placement, stage_index, stage_count, identity):
    identity = dict(identity)
    start, end = (int(value) for value in placement.get("layers", (0, 0)))
    if not (0 <= start <= end <= identity["n_layer"]):
        raise ValueError("stage layer range is outside the model")
    names = identity.pop("tensor_sizes")
    owned = sorted(name for name in names if name.startswith("blk.") and _owns_layer(name, start, end))
    # Embedding is only needed by the first stage; output/norm only by the last.
    if stage_index == 0:
        owned.extend(name for name in names if not name.startswith("blk.") and "output" not in name)
    if stage_index == stage_count - 1:
        owned.extend(name for name in names if not name.startswith("blk.") and "output" in name)
    return {
        "version": MANIFEST_VERSION,
        "plan_id": plan_id,
        "model_ref": _safe_relative(model_ref),
        "shards": refs,
        "stage_index": int(stage_index),
        "stage_count": int(stage_count),
        "layers": [start, end],
        "owns_input": stage_index == 0,
        "owns_output": stage_index == stage_count - 1,
        "tensor_names": sorted(set(owned)),
        "identity": identity,
    }


def _owns_layer(name: str, start: int, end: int) -> bool:
    parts = name.split(".", 2)
    if len(parts) < 3:
        return False
    try:
        return start <= int(parts[1]) < end
    except ValueError:
        return False


def stage_gguf_tensor_names(reader, start: int, end: int) -> list[str]:
    """Tensors a stage's mini-GGUF keeps: its own layer window plus the
    architecture's non-block tensors (token_embd / output / norms), which
    llama.cpp's loader structurally requires on every stage. The stripped
    out-of-window layer weights are the file's bulk on large models."""
    names = [str(t.name) for t in reader.tensors]
    owned = [n for n in names if n.startswith("blk.") and _owns_layer(n, start, end)]
    owned += [n for n in names if not n.startswith("blk.")]
    return sorted(set(owned))


def write_stage_gguf(src_path: str, dst_path: str, start: int, end: int) -> dict[str, Any]:
    """Write a mini-GGUF holding only the tensors a stage owning layers
    [start, end) needs, with block_count and all metadata preserved so the
    rank-local loader still sees the full model shape. Returns a small summary."""
    if GGUFReader is None or GGUFWriter is None:
        raise RuntimeError("gguf read/write support is not installed")
    reader = GGUFReader(src_path)
    arch = reader.fields["general.architecture"].contents()
    writer = GGUFWriter(dst_path, arch)
    for key, field in reader.fields.items():
        if key.startswith("GGUF.") or key == "general.architecture":
            continue
        vtype = field.types[0]
        if vtype == GGUFValueType.ARRAY:
            writer.add_key_value(key, field.contents(), GGUFValueType.ARRAY, sub_type=field.types[1])
        else:
            writer.add_key_value(key, field.contents(), vtype)
    keep = set(stage_gguf_tensor_names(reader, start, end))
    kept = 0
    for tensor in reader.tensors:
        if str(tensor.name) not in keep:
            continue
        writer.add_tensor(str(tensor.name), tensor.data, raw_dtype=tensor.tensor_type)
        kept += 1
    writer.write_header_to_file()
    writer.write_kv_data_to_file()
    writer.write_tensors_to_file()
    writer.close()
    return {"tensors_kept": kept, "tensors_total": len(reader.tensors),
            "layers": [start, end], "bytes": os.path.getsize(dst_path)}


def _expert_dim_size(reader) -> int:
    """The model's routed-expert count (n_expert), read from arch metadata."""
    arch = reader.fields["general.architecture"].contents()
    field = reader.fields.get(f"{arch}.expert_count")
    return int(field.contents()) if field is not None else 0


def write_expert_shard_gguf(src_path: str, dst_path: str, layers, expert_begin: int,
                            expert_end: int) -> dict[str, Any]:
    """Write a mini-GGUF holding only the routed-expert slabs an *expert worker*
    owns: experts [expert_begin, expert_end) of the given layers.

    A MoE expert tensor stacks all experts on its outermost ggml dimension
    (``blk.N.ffn_{gate,up,down}_exps`` = ne ``[.., .., n_expert]``), so the reader
    exposes it as raw quantized bytes shaped ``(n_expert, rows, row_bytes)`` and
    expert ``e`` is a contiguous, quant-block-aligned slab — the slice is a plain
    ``data[a:b]`` with no dequant/re-pack. The router (``ffn_gate_inp``) and the
    always-on shared expert stay on the backbone and are intentionally omitted.
    Global→local expert mapping is recorded in ``linkcpp.expert_shard.*`` so the
    worker maps a dispatched global id to its local slab. All model KV metadata is
    preserved so the loader still sees the full model shape."""
    if GGUFReader is None or GGUFWriter is None:
        raise RuntimeError("gguf read/write support is not installed")
    a, b = int(expert_begin), int(expert_end)
    owned_layers = sorted({int(x) for x in layers})
    reader = GGUFReader(src_path)
    n_expert = _expert_dim_size(reader)
    if not (0 <= a < b <= (n_expert or b)):
        raise ValueError(f"expert range [{a},{b}) outside model n_expert={n_expert}")
    arch = reader.fields["general.architecture"].contents()
    writer = GGUFWriter(dst_path, arch)
    for key, field in reader.fields.items():
        if key.startswith("GGUF.") or key == "general.architecture":
            continue
        vtype = field.types[0]
        if vtype == GGUFValueType.ARRAY:
            writer.add_key_value(key, field.contents(), GGUFValueType.ARRAY, sub_type=field.types[1])
        else:
            writer.add_key_value(key, field.contents(), vtype)
    writer.add_key_value("linkcpp.expert_shard.expert_begin", a, GGUFValueType.INT32)
    writer.add_key_value("linkcpp.expert_shard.expert_end", b, GGUFValueType.INT32)
    writer.add_key_value("linkcpp.expert_shard.n_expert_global", int(n_expert), GGUFValueType.INT32)
    # Model dims carried on the slice so a worker (phone) is self-describing: it
    # reads n_embd/n_layer via meta_i32 like it already reads the expert range,
    # and needs no out-of-band --n-embd. (n_expert_global above is the top-k pool.)
    def _mi(k):
        try:
            return int(reader.fields[f"{arch}.{k}"].contents())
        except Exception:
            return 0
    writer.add_key_value("linkcpp.expert_shard.n_embd", _mi("embedding_length"), GGUFValueType.INT32)
    writer.add_key_value("linkcpp.expert_shard.n_layer", _mi("block_count"), GGUFValueType.INT32)
    writer.add_key_value("linkcpp.expert_shard.layers", owned_layers,
                         GGUFValueType.ARRAY, sub_type=GGUFValueType.INT32)
    layer_set = set(owned_layers)
    kept = 0
    body_bytes = 0
    for tensor in reader.tensors:
        name = str(tensor.name)
        if not name.startswith("blk."):
            continue
        try:
            layer = int(name.split(".")[1])
        except (IndexError, ValueError):
            continue
        # Routed experts only: the stacked "*_exps" tensors. "*_shexp" (shared) and
        # "ffn_gate_inp" (router) contain "exp"/"inp", not "exps", so they are skipped.
        if layer not in layer_set or "exps" not in name:
            continue
        sliced = tensor.data[a:b]                      # outermost axis = expert
        writer.add_tensor(name, sliced, raw_dtype=tensor.tensor_type)
        kept += 1
        body_bytes += int(sliced.nbytes)
    writer.write_header_to_file()
    writer.write_kv_data_to_file()
    writer.write_tensors_to_file()
    writer.close()
    return {"tensors_kept": kept, "expert_range": [a, b], "n_expert_global": int(n_expert),
            "layers": owned_layers, "body_bytes": body_bytes, "bytes": os.path.getsize(dst_path)}


def validate_stage_manifest(model_root: str, manifest: Mapping[str, Any]) -> dict[str, Any]:
    """Verify local files and rank ownership before allocating a stage model."""
    if int(manifest.get("version", 0)) != MANIFEST_VERSION:
        raise ValueError("unsupported stage manifest version")
    refs = [_safe_relative(item) for item in manifest.get("shards", [])]
    root = os.path.abspath(model_root)
    paths = [os.path.abspath(os.path.join(root, item)) for item in refs]
    if not paths or any(os.path.commonpath([root, path]) != root or not os.path.isfile(path) for path in paths):
        raise ValueError("one or more manifest GGUF shards are missing locally")
    identity = _model_identity(paths)
    expected = dict(manifest.get("identity") or {})
    for key in ("architecture", "n_layer", "n_embd", "tensor_index_sha256", "tensor_count"):
        if identity.get(key) != expected.get(key):
            raise ValueError(f"local model identity mismatch: {key}")
    names = identity["tensor_sizes"]
    requested = list(manifest.get("tensor_names") or [])
    missing = [name for name in requested if name not in names]
    if missing:
        raise ValueError(f"local model is missing {len(missing)} stage tensors")
    start, end = (int(value) for value in manifest.get("layers", (0, 0)))
    if not (0 <= start <= end <= identity["n_layer"]):
        raise ValueError("manifest layer range is outside the local model")
    return {
        "verified": True,
        "architecture": identity["architecture"],
        "layers": [start, end],
        "local_tensor_count": len(requested),
        "local_tensor_bytes": sum(names[name] for name in requested),
        "tensor_index_sha256": identity["tensor_index_sha256"],
    }
