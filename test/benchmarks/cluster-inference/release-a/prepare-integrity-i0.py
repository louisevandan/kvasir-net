#!/usr/bin/env python3
"""Materialize the three Release A I0 requests under one LOAD/UNLOAD."""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile


DIRECTORY = Path(__file__).resolve().parent
H1_MATERIALIZER = DIRECTORY / "prepare-h1-quality.py"
H1_JUDGE = DIRECTORY / "judge-h1-quality.py"
I0_JUDGE = DIRECTORY / "judge-integrity-i0.py"
INTEGRITY_VALIDATOR = DIRECTORY / "validate-integrity-test-spec.py"


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def load_module(name: str, path: Path):
    module_spec = importlib.util.spec_from_file_location(name, path)
    if module_spec is None or module_spec.loader is None:
        raise RuntimeError(f"cannot load {path.name}")
    module = importlib.util.module_from_spec(module_spec)
    module_spec.loader.exec_module(module)
    return module


def reduce_to_i0(config: dict, seal: dict, group: dict, generation: int) -> tuple[dict, dict]:
    expected_group = {
        "arms": ["I0-S", "I0-M", "I0-L"], "same_load": True,
        "selection": ["case-00", "case-04", "case-06"],
        "classes": ["short", "medium", "long"], "requests": 3,
        "waves": [{"after_ms": 0, "count": 3}],
        "submission": "release_closed_loop", "max_in_flight": 1,
        "request_deadline_ms": [600000, 1200000, 1800000],
        "overall_grace_ms": 300000, "timeout_ms": 3900000,
        "pre_inference_hold_ms": 15000,
        "inference_start_hold_ms": 15000, "post_inference_hold_ms": 15000,
        "load_count": 1, "unload_count": 1,
    }
    if group != expected_group:
        raise ValueError("I0 execution group differs from the sealed contract")
    if group["timeout_ms"] != sum(group["request_deadline_ms"]) + group["overall_grace_ms"]:
        raise ValueError("I0 timeout arithmetic differs")

    base_cases = seal.get("cases") or []
    by_id: dict[str, tuple[int, dict]] = {}
    for index, case in enumerate(base_cases):
        case_id = case.get("id")
        if case_id in by_id:
            raise ValueError(f"duplicate base case identity: {case_id}")
        by_id[case_id] = (index, case)
    selected = []
    for position, (case_id, request_class, deadline) in enumerate(zip(
        group["selection"], group["classes"], group["request_deadline_ms"]
    )):
        if case_id not in by_id:
            raise ValueError(f"I0 case is absent: {case_id}")
        index, case = by_id[case_id]
        if case.get("class") != request_class or case.get("request_timeout_ms") != deadline:
            raise ValueError(f"I0 case contract differs: {case_id}")
        selected.append((position, index, case))

    prompts = config.get("prompts") or []
    responses = (config.get("acceptance") or {}).get("responses") or []
    processors = config.get("response_processors") or []
    if (len(prompts) != seal.get("requests") or len(responses) != len(prompts)
            or len(processors) != len(prompts)):
        raise ValueError("base H1 materialization shape differs")

    reduced = json.loads(json.dumps(config))
    reduced.update({
        "channel": f"release-a-integrity-i0-{generation}",
        "session_id": f"release-a-integrity-i0-{generation}",
        "request_id": "integrity-i0",
        "timeout_ms": group["timeout_ms"],
        "prompts": [prompts[index] for _, index, _ in selected],
        "waves": group["waves"],
        "max_in_flight": group["max_in_flight"],
        "request_timeout_ms": group["request_deadline_ms"],
        "pre_inference_hold_ms": group["pre_inference_hold_ms"],
        "inference_start_hold_ms": group["inference_start_hold_ms"],
        "post_inference_hold_ms": group["post_inference_hold_ms"],
    })
    reduced["acceptance"]["responses"] = [responses[index] for _, index, _ in selected]
    reduced["response_processors"] = [processors[index] for _, index, _ in selected]
    for node_index, node in enumerate(reduced["nodes"]):
        node["node"] = f"release-a-integrity-i0-qwen122-stage-{node_index}"

    reduced_seal = {
        "schema": 3, "mode": "release_a_integrity_i0", "generation": generation,
        "requests": group["requests"], "same_load": True,
        "load_count": group["load_count"], "unload_count": group["unload_count"],
        "arms": group["arms"], "max_in_flight": group["max_in_flight"],
        "request_timeout_ms": group["request_deadline_ms"],
        "slo": seal["slo"], "cases": [case for _, _, case in selected],
    }
    return reduced, reduced_seal


def materialize(args: argparse.Namespace) -> dict:
    h0 = json.loads(args.h0_spec.read_text(encoding="utf-8"))
    integrity = json.loads(args.integrity_spec.read_text(encoding="utf-8"))
    load_module("release_a_integrity_validator", INTEGRITY_VALIDATOR).validate(integrity)
    if h0.get("h0_status") != "sealed" or h0.get("runtime_acceptance") is not False:
        raise ValueError("H0 spec is not sealed for I0 execution")
    if h0.get("source", {}).get("source_commit") != integrity["source"]["runtime_commit"]:
        raise ValueError("H0 runtime source differs from the integrity contract")
    artifacts = {row.get("id"): row for row in h0.get("artifacts") or []}
    if (artifacts.get("integrity_spec") or {}).get("sha256") != digest(args.integrity_spec.read_bytes()):
        raise ValueError("H0 does not bind the supplied integrity contract")
    if digest(args.corpus.read_bytes()) != integrity["source"]["corpus_sha256"]:
        raise ValueError("integrity contract is not bound to the supplied corpus")

    h1 = load_module("release_a_h1_materializer", H1_MATERIALIZER)
    with tempfile.TemporaryDirectory(prefix="p4-integrity-i0-") as temporary:
        base_output = Path(temporary) / "h1"
        h1.materialize(argparse.Namespace(
            spec=args.h0_spec, corpus=args.corpus, materialized=args.materialized,
            plan=args.plan, logical_agent=args.logical_agent,
            output_dir=base_output, generation=args.generation,
        ))
        base_config = json.loads((base_output / "h1-quality.json").read_text(encoding="utf-8"))
        base_seal = json.loads((base_output / "h1-judge-seal.json").read_text(encoding="utf-8"))

    config, seal = reduce_to_i0(
        base_config, base_seal, integrity["execution_groups"]["I0"], args.generation
    )
    args.output_dir.mkdir(parents=True, exist_ok=False)
    config_bytes = (json.dumps(config, ensure_ascii=False, separators=(",", ":")) + "\n").encode()
    (args.output_dir / "integrity-i0.json").write_bytes(config_bytes)
    seal.update({
        "h0_spec_sha256": digest(args.h0_spec.read_bytes()),
        "integrity_spec_sha256": digest(args.integrity_spec.read_bytes()),
        "corpus_sha256": digest(args.corpus.read_bytes()),
        "config_sha256": digest(config_bytes),
        "materializer_sha256": digest(Path(__file__).read_bytes()),
        "h1_materializer_sha256": digest(H1_MATERIALIZER.read_bytes()),
        "h1_judge_sha256": digest(H1_JUDGE.read_bytes()),
        "i0_judge_sha256": digest(I0_JUDGE.read_bytes()),
    })
    (args.output_dir / "integrity-i0-judge-seal.json").write_text(
        json.dumps(seal, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    return {
        "generation": args.generation, "config_sha256": seal["config_sha256"],
        "requests": seal["requests"], "arms": seal["arms"], "same_load": seal["same_load"],
    }


def self_test() -> None:
    config = {
        "prompts": [f"p{i}" for i in range(7)],
        "acceptance": {"responses": [{"minimum_generated_tokens": 1} for _ in range(7)]},
        "response_processors": [None] * 4 + ["engineering_power_v1"] * 3,
        "nodes": [{"node": "h1-0"}, {"node": "h1-1"}, {"node": "h1-2"}],
    }
    deadlines = {"short": 600000, "medium": 1200000, "long": 1800000}
    classes = ["short", "short", "short", "short", "medium", "medium", "long"]
    seal = {
        "requests": 7,
        "slo": {"percentile": "nearest_rank",
                "ttft_ms_by_class": {"short": 60000, "medium": 300000, "long": 900000},
                "itl_p95_ms": 250},
        "cases": [{"id": f"case-{i:02}", "class": name,
                   "request_timeout_ms": deadlines[name]} for i, name in enumerate(classes)],
    }
    group = {
        "arms": ["I0-S", "I0-M", "I0-L"], "same_load": True,
        "selection": ["case-00", "case-04", "case-06"],
        "classes": ["short", "medium", "long"], "requests": 3,
        "waves": [{"after_ms": 0, "count": 3}], "submission": "release_closed_loop",
        "max_in_flight": 1, "request_deadline_ms": [600000, 1200000, 1800000],
        "overall_grace_ms": 300000, "timeout_ms": 3900000,
        "pre_inference_hold_ms": 15000,
        "inference_start_hold_ms": 15000, "post_inference_hold_ms": 15000,
        "load_count": 1, "unload_count": 1,
    }
    reduced, result_seal = reduce_to_i0(config, seal, group, 7)
    assert reduced["prompts"] == ["p0", "p4", "p6"]
    assert reduced["response_processors"] == [None, "engineering_power_v1", "engineering_power_v1"]
    assert reduced["max_in_flight"] == 1 and reduced["timeout_ms"] == 3900000
    assert reduced["pre_inference_hold_ms"] == 15000
    assert reduced["inference_start_hold_ms"] == 15000
    assert reduced["post_inference_hold_ms"] == 15000
    assert [case["id"] for case in result_seal["cases"]] == group["selection"]
    mutations = []
    for key, value in (("load_count", 3), ("max_in_flight", 3), ("timeout_ms", 3899999),
                       ("pre_inference_hold_ms", 0),
                       ("inference_start_hold_ms", 0), ("post_inference_hold_ms", 0)):
        changed = json.loads(json.dumps(group)); changed[key] = value; mutations.append(changed)
    changed = json.loads(json.dumps(group)); changed["selection"][1] = "case-05"; mutations.append(changed)
    for changed in mutations:
        try:
            reduce_to_i0(config, seal, changed, 7)
        except ValueError:
            pass
        else:
            raise AssertionError("weakened I0 execution group was accepted")
    missing_processor = json.loads(json.dumps(config))
    missing_processor.pop("response_processors")
    try:
        reduce_to_i0(missing_processor, seal, group, 7)
    except ValueError:
        pass
    else:
        raise AssertionError("I0 accepted a missing product response path")
    print(json.dumps({"passed": True, "tests": 8}, separators=(",", ":")))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--h0-spec", type=Path)
    parser.add_argument("--integrity-spec", type=Path)
    parser.add_argument("--corpus", type=Path)
    parser.add_argument("--materialized", type=Path)
    parser.add_argument("--plan", type=Path, action="append", default=[])
    parser.add_argument("--logical-agent", action="append", default=[])
    parser.add_argument("--output-dir", type=Path)
    parser.add_argument("--generation", type=int)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return
    required = (args.h0_spec, args.integrity_spec, args.corpus, args.materialized,
                args.output_dir, args.generation)
    if any(value is None for value in required):
        parser.error("h0-spec, integrity-spec, corpus, materialized, output-dir and generation are required")
    print(json.dumps(materialize(args), separators=(",", ":")))


if __name__ == "__main__":
    main()
