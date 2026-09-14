"""Run bounded text scenarios over the explicit local node partition."""

from datetime import datetime, timezone
import json
from pathlib import Path
import platform
import time
import torch
from transformers import AutoTokenizer

from p4hfadapter.models.qwen3_5_0_8b.configuration import inspect_plan, read_plan
from p4hfadapter.models.qwen3_5_0_8b.evidence import execution_identity, verify_checkpoint
from p4hfadapter.models.qwen3_5_0_8b.loading import runtime_check
from p4hfadapter.models.qwen3_5_0_8b.routing import Pipeline
from p4hfadapter.models.qwen3_5_0_8b.scenarios import read_scenario


def execute(root, plan_path, plan, directory, scenario_path, output, verify=False, timeout=120):
    runtime_check()
    output.mkdir(parents=True, exist_ok=False)
    report = {"timestamp": datetime.now(timezone.utc).isoformat(), "host": platform.node(),
              "plan": inspect_plan(plan), "scenario": str(scenario_path), "ok": False,
              "scope": "text-only local processes; no P4 integration or physical-host acceptance",
              "parity_tolerance": {"atol": 0.125, "rtol": 0.01, "greedy_tokens_must_match": True},
              "requests": [], "first_error": None, "cleanup_error": None}
    pipeline = None
    reference = None
    try:
        frozen_plan, frozen_scenario = output / "plan.json", output / "scenario.json"
        frozen_plan.write_bytes(plan_path.read_bytes())
        frozen_scenario.write_bytes(scenario_path.read_bytes())
        if read_plan(frozen_plan) != plan:
            raise ValueError("plan changed during admission")
        report["evidence"] = execution_identity(root, frozen_plan, frozen_scenario)
        report["checkpoint"] = verify_checkpoint(root, directory)
        if any(n.host != "local" for n in plan.nodes):
            raise ValueError("run requires host=local; inspect supports remote placement descriptions")
        for node in plan.nodes:
            if node.device.startswith("cuda:") and (not torch.cuda.is_available() or int(node.device[5:]) >= torch.cuda.device_count()):
                raise ValueError(f"unavailable device {node.device}")
        tokenizer = AutoTokenizer.from_pretrained(directory, local_files_only=True)
        name, schedule, requests = read_scenario(frozen_scenario, plan, tokenizer)
        report["scenario_name"] = name
        pipeline = Pipeline(root, frozen_plan, plan, directory, output, timeout)
        report["nodes"] = pipeline.reports
        if verify:
            from p4hfadapter.models.qwen3_5_0_8b.reference import Reference
            reference = Reference(directory, plan.dtype, plan.nodes[0].device)
        start = time.monotonic()
        from p4hfadapter.models.qwen3_5_0_8b.scheduling import drive
        report["requests"] = drive(pipeline, requests, schedule, tokenizer, reference)
        pipeline.shutdown()
        report["scenario_wall_seconds"] = time.monotonic() - start
        report["ok"] = True
    except Exception as error:
        report["first_error"] = f"{type(error).__name__}: {error}"
        if hasattr(error, "cleanup_error"):
            report["cleanup_error"] = error.cleanup_error
        if hasattr(error, "partial_nodes"):
            report["nodes"] = error.partial_nodes
    finally:
        if reference:
            report["comparisons"] = reference.comparisons
        if pipeline:
            report["trace"] = pipeline.trace
            try:
                pipeline.close()
            except Exception as error:
                report["cleanup_error"] = f"{type(error).__name__}: {error}"
                report["ok"] = False
        (output / "summary.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return report
