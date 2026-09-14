"""Concrete Qwen CLI; inspect remains dependency-free."""

import argparse
import json
from pathlib import Path

from p4hfadapter.models.qwen3_5_0_8b.configuration import inspect_plan, read_plan


def main(root: Path):
    parser = argparse.ArgumentParser(description="Qwen3.5-0.8B text stage-plan runner")
    parser.add_argument("command", choices=("inspect", "run", "verify", "worker"))
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--model-dir", type=Path)
    parser.add_argument("--scenario", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--timeout", type=float, default=120)
    parser.add_argument("--node")
    parser.add_argument("--run-id")
    args = parser.parse_args()
    try:
        plan = read_plan(args.plan)
        if args.command == "inspect":
            print(json.dumps(inspect_plan(plan), indent=2))
            return 0
        directory = args.model_dir
        if directory is None:
            directory = Path((root.parents[2] / ".cache/hf/models/checkpoint-path.txt").read_text(encoding="utf-8").strip())
        directory = directory.resolve()
        if args.command == "worker":
            if not args.node or not args.run_id:
                raise ValueError("worker requires --node and --run-id")
            from p4hfadapter.models.qwen3_5_0_8b.evidence import verify_checkpoint
            verify_checkpoint(root, directory)
            from p4hfadapter.models.qwen3_5_0_8b.worker import serve
            return serve(plan, args.node, directory, args.run_id)
        if args.scenario is None or args.output is None or not 0 < args.timeout <= 600:
            raise ValueError("run/verify require --scenario, new --output directory, and timeout in (0,600]")
        from p4hfadapter.models.qwen3_5_0_8b.execution import execute
        report = execute(root, args.plan.resolve(), plan, directory, args.scenario.resolve(), args.output.resolve(),
                         verify=args.command == "verify", timeout=args.timeout)
        print(json.dumps({"ok": report["ok"], "summary": str(args.output / "summary.json"),
                          "first_error": report["first_error"], "cleanup_error": report["cleanup_error"]}, ensure_ascii=False))
        return 0 if report["ok"] else 1
    except Exception as error:
        import sys
        print(f"{type(error).__name__}: {error}", file=sys.stderr)
        return 2
