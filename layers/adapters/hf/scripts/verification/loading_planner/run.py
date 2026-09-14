"""Real CPU profile -> generated plan -> existing Qwen verify consumer."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

root = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(root / "python"))
from p4hfadapter.models.qwen3_5_0_8b.identity import MODEL_ID, REVISION
from p4hfadapter.models.qwen3_5_0_8b.planning import source_identity


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--model-dir", type=Path)
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    directory = args.model_dir or Path((root.parents[2] / ".cache/hf/models/checkpoint-path.txt").read_text(encoding="utf-8").strip())
    request = {"schema": "qwen3.5-0.8b-loading-request-v1", "model_id": MODEL_ID, "revision": REVISION,
               "dtype": "float32", "quantization": "none", "limits": {"context": 64, "max_requests": 2, "max_new_tokens": 8},
               "prefill_chunk": 16, "cuts": [0, 12, 24], "minimum_hosts": 1,
               "hosts": [{"id": "local", "available_bytes": 16 * 1024**3, "reserve_bytes": 1024**3}],
               "devices": [{"id": "cpu", "host": "local", "device": "cpu", "available_bytes": 16 * 1024**3,
                            "reserve_bytes": 1024**3, "enabled": True}],
               "slots": [{"node_id": "first", "device_id": "cpu"}, {"node_id": "second", "device_id": "cpu"}]}
    (output / "request.json").write_text(json.dumps(request, indent=2) + "\n", encoding="utf-8")
    source = source_identity()
    cli = root / "scripts/models/qwen3_5_0_8b/cli/run.py"
    commands = []
    def run(name, arguments):
        command = [sys.executable, "-B", str(cli), *map(str, arguments)]
        print("START " + name, flush=True)
        result = subprocess.run(command, capture_output=True, timeout=600)
        log = output / (name + ".log")
        log.write_bytes(result.stdout + result.stderr)
        commands.append({"command": command, "exit_code": result.returncode, "log_sha256": hashlib.sha256(log.read_bytes()).hexdigest()})
        if result.returncode:
            raise AssertionError(f"{name} failed: {log}")
    report = {"ok": False, "source_sha256": source, "commands": commands, "scope": "CPU model conformance; not remote/SLO acceptance"}
    try:
        run("profile", ["profile", "--request", output / "request.json", "--model-dir", directory, "--output", output / "profile", "--timeout", 600])
        data = json.loads((output / "profile/profiles.json").read_text(encoding="utf-8"))[0]
        samples = {tuple(s["layers"]): s for s in data["samples"]}
        # First/last stages copy the tied embedding; a full stage aliases it once.
        duplication = samples[0, 12]["weight_bytes"] + samples[12, 24]["weight_bytes"] - samples[0, 24]["weight_bytes"]
        assert duplication == 248320 * 1024 * 4, duplication
        assert samples[0, 12]["cache_bytes"] + samples[12, 24]["cache_bytes"] == samples[0, 24]["cache_bytes"]
        assert all(s["cache_bytes"] > 0 and s["active_after_release"] == 0 for s in samples.values())
        run("plan", ["plan", "--request", output / "request.json", "--profiles", output / "profile/profiles.json", "--output", output / "planned"])
        scenario = {"name": "loading-planner-consumer", "schedule": "round_robin", "requests": [
            {"id": "arithmetic", "prompt": "What is 2 + 2? Reply with only the digit.", "max_new_tokens": 8, "prefill_chunk": 16, "cancel_after": None},
            {"id": "korean", "prompt": "대한민국의 수도 이름만 답하세요.", "max_new_tokens": 8, "prefill_chunk": 16, "cancel_after": None}]}
        (output / "scenario.json").write_text(json.dumps(scenario, ensure_ascii=False, indent=2), encoding="utf-8")
        run("verify", ["verify", "--plan", output / "planned/plan.json", "--model-dir", directory,
                       "--scenario", output / "scenario.json", "--output", output / "consumer", "--timeout", 600])
        consumer = json.loads((output / "consumer/summary.json").read_text(encoding="utf-8"))
        assert consumer["ok"] and consumer["first_error"] is None and consumer["cleanup_error"] is None
        assert consumer["comparisons"]
        # The actual StageSessions guard rejects an unprofiled shape before creating cache.
        import torch
        from p4hfadapter.models.qwen3_5_0_8b.configuration import read_plan
        from p4hfadapter.models.qwen3_5_0_8b.loading import load_stage
        from p4hfadapter.models.qwen3_5_0_8b.state import StageSessions
        plan = read_plan(output / "planned/plan.json")
        stage, _ = load_stage(directory, plan.nodes[0], plan.dtype)
        sessions = StageSessions(stage, plan)
        try:
            sessions.step("oversize", 0, 0, torch.ones((1, 17), dtype=torch.int64))
            raise AssertionError("unprofiled prefill was executed")
        except ValueError as error:
            assert "profiled prefill_chunk" in str(error)
        assert sessions.active == {} and sessions.retired == set()
        assert source_identity() == source
        report.update(ok=True, tied_copy_bytes=duplication, cache_bytes=samples[0, 24]["cache_bytes"],
                      comparisons=len(consumer["comparisons"]), oversized_step_no_effect=True,
                      requests=consumer["requests"], assessment=json.loads((output / "planned/assessment.json").read_text(encoding="utf-8")))
    finally:
        (output / "summary.json").write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(json.dumps({"ok": report["ok"], "output": str(output)}), flush=True)


if __name__ == "__main__":
    main()
