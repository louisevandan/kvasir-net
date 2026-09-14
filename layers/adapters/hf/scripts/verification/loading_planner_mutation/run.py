"""Run planner/consumer removals in fresh source copies and bind failures to hashes."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

root = Path(__file__).resolve().parents[3]
prefix = "python/p4hfadapter/models/qwen3_5_0_8b/"
planner = prefix + "planning/__init__.py"
cases = [
    ("reserve", planner, ' - integer(device["reserve_bytes"], "device reserve")', ''),
    ("disabled", planner, 'if not device["enabled"]:\n            continue', 'if False:\n            continue'),
    ("shared-capacity", planner, 'value + demand.get(name, 0)', 'demand.get(name, 0)'),
    ("serial-objective", planner, '(state[4], state[3], len(state[5]), state[5])', '(state[3], state[4], len(state[5]), state[5])'),
    ("source-binding", planner, 'or profile.get("source_sha256") != sources', 'or False'),
    ("prefill-forwarding", planner, 'result["limits"]["prefill_chunk"] = request["prefill_chunk"]', 'pass'),
    ("native-admission", prefix + "state/__init__.py", 'if self.plan.prefill_chunk is not None and tensor.shape[1] > self.plan.prefill_chunk:', 'if False:'),
]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    sources = [p for folder in ("python", "tests", "scripts", "manifests") for p in (root / folder).rglob("*")
               if p.is_file() and p.suffix in (".py", ".json")]
    sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
    original = {p.relative_to(root).as_posix(): sha(p) for p in sources}
    records = []
    for name, file, before, after in [("baseline", None, None, None), *cases]:
        copy = args.output / name
        for source in sources:
            target = copy / source.relative_to(root)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, target)
        if file:
            target = copy / file
            text = target.read_text(encoding="utf-8")
            if text.count(before) != 1:
                raise AssertionError(f"mutation drift: {name}")
            target.write_text(text.replace(before, after), encoding="utf-8", newline="\n")
        env = dict(os.environ, PYTHONPATH=str(copy / "python"), PYTHONDONTWRITEBYTECODE="1", PYTHONUTF8="1")
        if name == "native-admission":
            command = [sys.executable, "-B", str(copy / "scripts/verification/loading_planner_shape/run.py")]
        else:
            command = [sys.executable, "-B", "-m", "unittest", "discover", "-s", str(copy / "tests/models/qwen3_5_0_8b/planning"), "-p", "test_*.py"]
        run = subprocess.run(command, cwd=copy, env=env, capture_output=True, timeout=180)
        log = copy / "test.log"
        log.write_bytes(run.stdout + run.stderr)
        expected = run.returncode == (0 if name == "baseline" else 1)
        if name != "baseline":
            expected = expected and b"AssertionError" in run.stderr and b"FAILED (" in run.stderr
        records.append({"name": name, "exit_code": run.returncode, "expected": expected, "command": command,
                        "log_sha256": sha(log), "mutated_sha256": sha(copy / file) if file else None})
        (args.output / "summary.json").write_text(json.dumps({"source_sha256": original, "cases": records}, indent=2) + "\n", encoding="utf-8")
        print(json.dumps(records[-1]), flush=True)
        if not expected:
            raise AssertionError(f"unexpected mutation result: {name}; {log}")
    assert all(sha(root / file) == value for file, value in original.items())


if __name__ == "__main__":
    main()
