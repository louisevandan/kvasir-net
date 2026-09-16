#!/usr/bin/env python3
"""Materialize one sealed A-COST request through the H1 execution path."""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile


DIRECTORY = Path(__file__).resolve().parent
H1_MATERIALIZER = DIRECTORY / "prepare-h1-quality.py"
A_COST_JUDGE = DIRECTORY / "judge-a-cost.py"
H1_JUDGE = DIRECTORY / "judge-h1-quality.py"


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def load_h1_materializer():
    module_spec = importlib.util.spec_from_file_location(
        "release_a_h1_materializer", H1_MATERIALIZER
    )
    if module_spec is None or module_spec.loader is None:
        raise RuntimeError("cannot load the sealed H1 materializer")
    module = importlib.util.module_from_spec(module_spec)
    module_spec.loader.exec_module(module)
    return module


def reduce_to_cost(config: dict, seal: dict, gate: dict, generation: int) -> tuple[dict, dict]:
    required = {
        "case_id", "class", "selection", "requests", "waves", "submission",
        "max_in_flight", "request_deadline_ms", "overall_grace_ms", "timeout_ms",
        "normal", "scope", "h1_promotion_allowed",
    }
    if set(gate) != required:
        raise ValueError("A-COST gate fields differ from the contract")
    if gate != {
        "case_id": "case-00", "class": "short", "selection": "exact_corpus_case",
        "requests": 1, "waves": [{"after_ms": 0, "count": 1}],
        "submission": "release_closed_loop", "max_in_flight": 1,
        "request_deadline_ms": 600000, "overall_grace_ms": 300000,
        "timeout_ms": 900000, "normal": True,
        "scope": "short_feasibility_only", "h1_promotion_allowed": False,
    }:
        raise ValueError("A-COST gate is not the sealed short feasibility input")
    if gate["timeout_ms"] != gate["request_deadline_ms"] + gate["overall_grace_ms"]:
        raise ValueError("A-COST timeout arithmetic differs")

    matches = [
        (index, case) for index, case in enumerate(seal.get("cases") or [])
        if case.get("id") == gate["case_id"]
    ]
    if len(matches) != 1:
        raise ValueError("A-COST case identity is not unique")
    index, case = matches[0]
    if case.get("class") != gate["class"]:
        raise ValueError("A-COST class differs from the selected corpus case")
    if case.get("request_timeout_ms") != gate["request_deadline_ms"]:
        raise ValueError("A-COST request deadline differs from H1 authority")
    prompts = config.get("prompts") or []
    responses = (config.get("acceptance") or {}).get("responses") or []
    if len(prompts) != seal.get("requests") or len(responses) != len(prompts):
        raise ValueError("H1 materialization shape differs before A-COST reduction")

    result_config = json.loads(json.dumps(config))
    result_config.update({
        "channel": f"release-a-cost-short-{generation}",
        "session_id": f"release-a-cost-short-{generation}",
        "request_id": "a-cost-short",
        "timeout_ms": gate["timeout_ms"],
        "prompts": [prompts[index]],
        "waves": gate["waves"],
        "max_in_flight": gate["max_in_flight"],
        "request_timeout_ms": [gate["request_deadline_ms"]],
    })
    result_config["acceptance"]["responses"] = [responses[index]]
    for node_index, node in enumerate(result_config["nodes"]):
        node["node"] = f"release-a-cost-qwen122-stage-{node_index}"

    result_seal = {
        "schema": 1,
        "mode": "a_cost_short_feasibility",
        "generation": generation,
        "requests": 1,
        "scope": gate["scope"],
        "h1_promotion_allowed": gate["h1_promotion_allowed"],
        "case": case,
        "slo": {
            "percentile": "nearest_rank",
            "ttft_limit_ms": seal["slo"]["ttft_ms_by_class"][gate["class"]],
            "itl_p95_limit_ms": seal["slo"]["itl_p95_ms"],
            "e2e_limit_ms": gate["request_deadline_ms"],
        },
    }
    return result_config, result_seal


def materialize(args: argparse.Namespace) -> dict:
    spec = json.loads(args.spec.read_text(encoding="utf-8"))
    if spec.get("h0_status") != "sealed" or spec.get("runtime_acceptance") is not False:
        raise ValueError("H0 spec is not sealed for A-COST")
    gate = (spec.get("feasibility") or {}).get("a_cost")
    if not isinstance(gate, dict):
        raise ValueError("H0 spec does not contain A-COST authority")

    h1 = load_h1_materializer()
    with tempfile.TemporaryDirectory(prefix="p4-a-cost-") as temporary:
        base_output = Path(temporary) / "h1"
        h1.materialize(argparse.Namespace(
            spec=args.spec,
            corpus=args.corpus,
            materialized=args.materialized,
            plan=args.plan,
            logical_agent=args.logical_agent,
            output_dir=base_output,
            generation=args.generation,
        ))
        base_config = json.loads((base_output / "h1-quality.json").read_text(encoding="utf-8"))
        base_seal = json.loads((base_output / "h1-judge-seal.json").read_text(encoding="utf-8"))

    config, seal = reduce_to_cost(base_config, base_seal, gate, args.generation)
    args.output_dir.mkdir(parents=True, exist_ok=False)
    config_bytes = (json.dumps(config, ensure_ascii=False, separators=(",", ":")) + "\n").encode()
    (args.output_dir / "a-cost.json").write_bytes(config_bytes)
    seal.update({
        "spec_sha256": digest(args.spec.read_bytes()),
        "corpus_sha256": digest(args.corpus.read_bytes()),
        "config_sha256": digest(config_bytes),
        "materializer_sha256": digest(Path(__file__).read_bytes()),
        "h1_materializer_sha256": digest(H1_MATERIALIZER.read_bytes()),
        "judge_sha256": digest(A_COST_JUDGE.read_bytes()),
        "h1_judge_sha256": digest(H1_JUDGE.read_bytes()),
    })
    (args.output_dir / "a-cost-judge-seal.json").write_text(
        json.dumps(seal, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    return {
        "generation": args.generation,
        "config_sha256": seal["config_sha256"],
        "case_id": seal["case"]["id"],
        "class": seal["case"]["class"],
        "scope": seal["scope"],
        "h1_promotion_allowed": seal["h1_promotion_allowed"],
    }


def self_test() -> None:
    config = {
        "prompts": ["p0", "p1"],
        "acceptance": {"responses": [{"x": 0}, {"x": 1}]},
        "nodes": [{"node": "h1-0"}, {"node": "h1-1"}],
    }
    seal = {
        "requests": 2,
        "slo": {"ttft_ms_by_class": {"short": 60}, "itl_p95_ms": 250},
        "cases": [
            {"id": "case-00", "class": "short", "request_timeout_ms": 600000},
            {"id": "case-01", "class": "short", "request_timeout_ms": 600000},
        ],
    }
    gate = {
        "case_id": "case-00", "class": "short", "selection": "exact_corpus_case",
        "requests": 1, "waves": [{"after_ms": 0, "count": 1}],
        "submission": "release_closed_loop", "max_in_flight": 1,
        "request_deadline_ms": 600000, "overall_grace_ms": 300000,
        "timeout_ms": 900000, "normal": True,
        "scope": "short_feasibility_only", "h1_promotion_allowed": False,
    }
    reduced, cost_seal = reduce_to_cost(config, seal, gate, 7)
    assert reduced["prompts"] == ["p0"] and reduced["timeout_ms"] == 900000
    assert cost_seal["case"]["id"] == "case-00" and cost_seal["h1_promotion_allowed"] is False
    changed = json.loads(json.dumps(gate)); changed["timeout_ms"] -= 1
    try:
        reduce_to_cost(config, seal, changed, 7)
    except ValueError:
        pass
    else:
        raise AssertionError("A-COST timeout weakening was accepted")
    duplicate = json.loads(json.dumps(seal)); duplicate["cases"].append(duplicate["cases"][0])
    try:
        reduce_to_cost(config, duplicate, gate, 7)
    except ValueError:
        pass
    else:
        raise AssertionError("duplicate A-COST case was accepted")
    print(json.dumps({"passed": True, "tests": 4}, separators=(",", ":")))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--spec", type=Path)
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
    required = (args.spec, args.corpus, args.materialized, args.output_dir, args.generation)
    if any(value is None for value in required):
        parser.error("spec, corpus, materialized, output-dir and generation are required")
    print(json.dumps(materialize(args), separators=(",", ":")))


if __name__ == "__main__":
    main()
