#!/usr/bin/env python3
"""Fail when the pinned llama.cpp architecture registry outruns the ring adapter.

This is a structural gate, not a substitute for golden inference tests.  The
adapter is intentionally required to remain free of architecture switches; a
new llama.cpp model therefore enters through the same graph cut/terminal ABI.
"""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path


MAPPING_RE = re.compile(
    r"case\s+(LLM_ARCH_[A-Z0-9_]+)\s*:\s*"
    r"return\s+new\s+(llama_model_[a-zA-Z0-9_]+)\(params\);"
)


def audit(repo: Path) -> dict:
    llama_root = repo / "external" / "llama.cpp"
    model_cpp = (llama_root / "src" / "llama-model.cpp").read_text(encoding="utf-8")
    graph_cpp = (llama_root / "src" / "llama-graph.cpp").read_text(encoding="utf-8")
    public_h = (llama_root / "include" / "llama.h").read_text(encoding="utf-8")
    server_main = (repo / "apps" / "linkcpp-server" / "main.cpp").read_text(encoding="utf-8")
    ring_executor = (repo / "apps" / "linkcpp-server" / "ring-executor.cpp").read_text(encoding="utf-8")

    mappings = MAPPING_RE.findall(model_cpp)
    architectures = [arch for arch, _ in mappings]
    duplicate_architectures = sorted({arch for arch in architectures if architectures.count(arch) > 1})

    start = graph_cpp.index("void llm_graph_result::apply_linkcpp_stage")
    end = graph_cpp.index("\nbool llm_graph_result::can_reuse", start)
    stage_body = graph_cpp[start:end]
    architecture_branches = sorted(set(re.findall(r"LLM_ARCH_[A-Z0-9_]+", stage_body)))

    required_descriptor_fields = {
        "type", "n_dims", "ne", "nb", "nbytes", "view_offset", "alias_of", "flags", "name",
    }
    descriptor_match = re.search(
        r"struct\s+llama_linkcpp_tensor_desc\s*\{(?P<body>.*?)\};", public_h, re.S,
    )
    descriptor_body = descriptor_match.group("body") if descriptor_match else ""
    missing_descriptor_fields = sorted(
        field for field in required_descriptor_fields
        if not re.search(rf"\b{re.escape(field)}\b", descriptor_body)
    )
    required_terminal_api = {
        "llama_linkcpp_terminal_count",
        "llama_linkcpp_terminal_desc",
        "llama_linkcpp_terminal_get",
        "llama_linkcpp_terminal_set",
        "llama_linkcpp_input_set_tensor",
        "llama_linkcpp_runtime_configure",
        "llama_linkcpp_stage_invocation",
    }
    missing_terminal_api = sorted(name for name in required_terminal_api if name not in public_h)
    required_state_api = {
        "llama_linkcpp_state_invocation",
        "LLAMA_LINKCPP_STATE_SEQ_GET_SIZE",
        "LLAMA_LINKCPP_STATE_SEQ_GET_DATA",
        "LLAMA_LINKCPP_STATE_SEQ_SET_DATA",
    }
    missing_state_api = sorted(name for name in required_state_api if name not in public_h)
    required_state_frames = {
        "FRAME_STATE_GET", "FRAME_STATE_CHUNK", "FRAME_STATE_DONE",
        "FRAME_STATE_SET_BEGIN", "FRAME_STATE_SET_CHUNK",
        "FRAME_STATE_SET_COMMIT", "FRAME_STATE_ACK",
    }
    missing_state_frames = sorted(name for name in required_state_frames if name not in ring_executor)
    forced_cache_disables = sorted(
        flag for flag in ("--no-cache-prompt", "--ctx-checkpoints", "--cache-ram")
        if flag in server_main
    )

    gaps = []
    if not mappings:
        gaps.append("llama_model_mapping registry was not found")
    if duplicate_architectures:
        gaps.append("duplicate architecture mappings")
    if architecture_branches:
        gaps.append("ring graph cutter contains architecture-specific branches")
    if missing_descriptor_fields:
        gaps.append("boundary descriptor cannot represent arbitrary tensor views")
    if missing_terminal_api:
        gaps.append("terminal tensor API is incomplete")
    if missing_state_api or missing_state_frames:
        gaps.append("distributed sequence state API is incomplete")
    if forced_cache_disables:
        gaps.append("ring mode still disables llama-server state features")

    return {
        "pinned_architecture_count": len(architectures),
        "architectures": architectures,
        "model_classes": [model for _, model in mappings],
        "architecture_branches_in_ring_cutter": architecture_branches,
        "missing_descriptor_fields": missing_descriptor_fields,
        "missing_terminal_api": missing_terminal_api,
        "missing_state_api": missing_state_api,
        "missing_state_frames": missing_state_frames,
        "forced_cache_disables": forced_cache_disables,
        "gaps": gaps,
        "ok": not gaps,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--compact", action="store_true")
    args = parser.parse_args()
    result = audit(args.repo.resolve())
    print(json.dumps(result, ensure_ascii=False, indent=None if args.compact else 2))
    return 0 if result["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
