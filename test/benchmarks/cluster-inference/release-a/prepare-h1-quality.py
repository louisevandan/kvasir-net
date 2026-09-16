#!/usr/bin/env python3
"""Materialize the sealed Release A H1 closed-loop quality run."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def read_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def quality_deadlines(spec: dict, corpus: dict) -> list[int]:
    mode = spec["workload"]["modes"]["quality"]
    if mode.get("submission") != "release_closed_loop" or mode.get("max_in_flight") != 1:
        raise ValueError("H1 quality must submit one request after terminal RELEASE")
    if mode.get("open_loop") is not False:
        raise ValueError("H1 quality cannot be open-loop")
    if mode.get("waves") != [{"after_ms": 0, "count": mode["requests"]}]:
        raise ValueError("H1 quality must preserve corpus order in one gated wave")
    limits = mode.get("request_deadline_ms_by_class")
    if not isinstance(limits, dict) or set(limits) != {"short", "medium", "long"}:
        raise ValueError("H1 quality deadline classes differ")
    if any(not isinstance(value, int) or value <= 0 for value in limits.values()):
        raise ValueError("H1 quality deadlines must be positive integers")
    requests = corpus.get("requests") or []
    if len(requests) != mode["requests"]:
        raise ValueError("H1 quality request count differs from corpus")
    deadlines = []
    for request in requests:
        request_class = request.get("class")
        if request_class not in limits:
            raise ValueError(f"unknown corpus class: {request_class!r}")
        deadlines.append(limits[request_class])
    grace = mode.get("overall_grace_ms")
    if not isinstance(grace, int) or grace < 0:
        raise ValueError("H1 quality overall grace is invalid")
    if mode.get("timeout_ms") != sum(deadlines) + grace:
        raise ValueError("H1 quality overall timeout arithmetic differs")
    return deadlines


def quality_slo(spec: dict) -> dict:
    slo = spec.get("slo") or {}
    ttft = slo.get("ttft_ms")
    itl = slo.get("itl_ms")
    if not isinstance(ttft, dict) or set(ttft) != {"short", "medium", "long"}:
        raise ValueError("H1 TTFT classes differ")
    if any(not isinstance(value, int) or isinstance(value, bool) or value <= 0
           for value in ttft.values()):
        raise ValueError("H1 TTFT limits must be positive integers")
    if not isinstance(itl, int) or isinstance(itl, bool) or itl <= 0:
        raise ValueError("H1 ITL limit must be a positive integer")
    return {"percentile": "nearest_rank", "ttft_ms_by_class": ttft, "itl_p95_ms": itl}


def resource_profile(capacity: dict, stage: dict) -> dict:
    return {
        "version": 1,
        "max_requests": capacity["admission"]["max_requests"],
        "max_request_retained_bytes": capacity["admission"]["retained_request_bytes"],
        "max_input_tokens": capacity["admission"]["input_tokens"],
        "max_request_bytes": capacity["request"]["max_bytes"],
        "max_output_tokens_per_request": capacity["request"]["max_output_tokens"],
        "max_output_tokens": capacity["admission"]["output_tokens"],
        "max_physical_result_bytes": stage["result_bounds"]["max_physical_result_bytes"],
        "max_completion_payload_bytes": stage["result_bounds"]["max_completion_payload_bytes"],
        "max_completion_retained_bytes": capacity["stores"]["completion"]["bytes"],
        "max_edge_retained_bytes": capacity["stores"]["edge"]["bytes"],
        "max_receipt_retained_bytes": capacity["stores"]["receipt"]["bytes"],
    }


def response_expectations(cases: list[dict]) -> list[dict]:
    """Bind OUTER acceptance to the independent, source-derived corpus oracle."""
    expectations = []
    for case in cases:
        expected = case.get("expected")
        if not isinstance(expected, dict) or not expected:
            raise ValueError(f"missing JSON object oracle: {case.get('id')}")
        expectations.append({
            "minimum_generated_tokens": 1, "minimum_response_chars": 2,
            "expected_json": expected,
        })
    return expectations


def materialize(args: argparse.Namespace) -> dict:
    spec = read_json(args.spec)
    corpus = read_json(args.corpus)
    deadlines = quality_deadlines(spec, corpus)
    slo = quality_slo(spec)
    if spec["h0_status"] != "sealed" or spec["runtime_acceptance"] is not False:
        raise ValueError("H0 spec is not sealed for execution")
    if corpus.get("runtime_acceptance") is not False:
        raise ValueError("corpus improperly claims runtime acceptance")

    prompts, cases = [], []
    total_bytes = total_tokens = 0
    for index, row in enumerate(corpus["requests"]):
        prompt_bytes = (args.materialized / f"{row['id']}.prompt.txt").read_bytes()
        oracle_bytes = (args.materialized / f"{row['id']}.oracle.json").read_bytes()
        if len(prompt_bytes) != row["prompt_bytes"] or digest(prompt_bytes) != row["prompt_sha256"]:
            raise ValueError(f"prompt binding differs: {row['id']}")
        if digest(oracle_bytes) != row["oracle_sha256"]:
            raise ValueError(f"oracle binding differs: {row['id']}")
        prompts.append(prompt_bytes.decode("utf-8"))
        cases.append({"id": row["id"], "class": row["class"],
                      "prompt_sha256": row["prompt_sha256"], "expected": json.loads(oracle_bytes),
                      "input_tokens": row["input_tokens"],
                      "request_timeout_ms": deadlines[index]})
        total_bytes += len(prompt_bytes)
        total_tokens += row["input_tokens"]

    capacity = spec["capacity"]
    if total_bytes > capacity["admission"]["retained_request_bytes"]:
        raise ValueError("corpus retained bytes exceed admission")
    if total_tokens > capacity["admission"]["input_tokens"]:
        raise ValueError("corpus input tokens exceed admission")
    max_tokens = capacity["request"]["max_output_tokens"]
    if len(prompts) * max_tokens > capacity["admission"]["output_tokens"]:
        raise ValueError("corpus output reservation exceeds admission")

    plans = [read_json(path) for path in args.plan]
    layout = spec["execution_layout"]
    if len(plans) != len(layout["stages"]) or len(args.logical_agent) != len(plans):
        raise ValueError("plan/agent count differs from the sealed layout")
    binaries = [host["binaries"]["native"]["path"] for host in spec["hosts"]]
    nodes = []
    for index, (base, stage, agent, binary) in enumerate(zip(plans, layout["stages"], args.logical_agent, binaries)):
        required = [
            f"--layer-begin {stage['layer_begin']} --layer-end {stage['layer_end']}",
            f"--kv-layer-begin {stage['kv_layer_begin']} --kv-layer-end {stage['kv_layer_end']}",
            f"--n-seq-max {layout['shape']['sequence_capacity']}",
            f"--ctx-size {layout['shape']['total_context']}",
        ]
        if any(fragment not in base["plan"] for fragment in required):
            raise ValueError(f"stage plan differs: {stage['id']}")
        nodes.append({
            "agent": agent, "node": f"release-a-h1-qwen122-stage-{index}",
            "generation": args.generation, "binary": binary,
            "endpoint": f"127.0.0.1:{stage['native_port']}", "plan": base["plan"],
            "args": [], "environment": base["environment"],
            "n_batch": layout["shape"]["n_batch"], "n_ubatch": layout["shape"]["n_ubatch"],
            "context_size": layout["shape"]["context_per_sequence"],
            "total_context_size": layout["shape"]["total_context"],
            "sequence_capacity": layout["shape"]["sequence_capacity"],
            "resource_profile": resource_profile(capacity, stage),
        })

    quality = spec["workload"]["modes"]["quality"]
    config = {
        "pipeline_compatibility": "physical-wire-v4", "ingress_agent": args.logical_agent[0],
        "channel": f"release-a-h1-quality-{args.generation}",
        "connection_generation": args.generation, "load_generation": args.generation,
        "session_id": f"release-a-h1-quality-{args.generation}", "request_id": "h1-quality",
        "max_tokens": max_tokens, "timeout_ms": quality["timeout_ms"], "prompts": prompts,
        "waves": quality["waves"], "max_in_flight": quality["max_in_flight"],
        "request_timeout_ms": deadlines,
        "options": json.dumps({"temperature": 0, "seed": spec["model"]["sampling"]["seed"]}, separators=(",", ":")),
        "acceptance": {"minimum_generated_tokens": 1, "allowed_stop_reasons": ["eos"],
                       "responses": response_expectations(cases)},
        "nodes": nodes,
    }
    args.output_dir.mkdir(parents=True, exist_ok=False)
    config_bytes = (json.dumps(config, ensure_ascii=False, separators=(",", ":")) + "\n").encode()
    (args.output_dir / "h1-quality.json").write_bytes(config_bytes)
    seal = {
        "schema": 3, "generation": args.generation, "spec_sha256": digest(args.spec.read_bytes()),
        "corpus_sha256": digest(args.corpus.read_bytes()), "config_sha256": digest(config_bytes),
        "materializer_sha256": digest(Path(__file__).read_bytes()), "requests": len(prompts),
        "total_prompt_bytes": total_bytes, "total_input_tokens": total_tokens,
        "max_output_tokens": max_tokens, "max_in_flight": 1,
        "request_timeout_ms_sha256": digest(json.dumps(deadlines, separators=(",", ":")).encode()),
        "request_timeout_ms": deadlines, "slo": slo, "cases": cases,
    }
    (args.output_dir / "h1-judge-seal.json").write_text(
        json.dumps(seal, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return {key: seal[key] for key in ("generation", "config_sha256", "requests", "total_prompt_bytes",
                                        "total_input_tokens", "max_output_tokens", "max_in_flight")}


def self_test() -> None:
    corpus = {"requests": [{"class": name} for name in ("short", "medium", "long")]}
    quality = {"requests": 3, "submission": "release_closed_loop", "max_in_flight": 1,
               "open_loop": False, "waves": [{"after_ms": 0, "count": 3}],
               "request_deadline_ms_by_class": {"short": 10, "medium": 20, "long": 30},
               "overall_grace_ms": 5, "timeout_ms": 65}
    spec = {"workload": {"modes": {"quality": quality}},
            "slo": {"ttft_ms": {"short": 4, "medium": 5, "long": 6}, "itl_ms": 3}}
    assert quality_deadlines(spec, corpus) == [10, 20, 30]
    assert quality_slo(spec) == {"percentile": "nearest_rank",
                                 "ttft_ms_by_class": {"short": 4, "medium": 5, "long": 6},
                                 "itl_p95_ms": 3}
    assert response_expectations([{"id": "case-00", "expected": {"answer": 7}}]) == [
        {"minimum_generated_tokens": 1, "minimum_response_chars": 2,
         "expected_json": {"answer": 7}}]
    try:
        response_expectations([{"id": "case-04"}])
    except ValueError:
        pass
    else:
        raise AssertionError("missing source-derived oracle was accepted")
    quality["max_in_flight"] = None
    try:
        quality_deadlines(spec, corpus)
    except ValueError:
        pass
    else:
        raise AssertionError("open-loop H1 quality was accepted")
    spec["slo"]["itl_ms"] = 0
    try:
        quality_slo(spec)
    except ValueError:
        pass
    else:
        raise AssertionError("zero H1 ITL SLO was accepted")
    print(json.dumps({"passed": True, "tests": 6}, separators=(",", ":")))


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
