"""Independent-copy tests for Qwen admission and actual stage-consumer regressions."""

from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
from uuid import uuid4

root = Path(__file__).resolve().parents[3]
output = root.parents[2] / "target/hf/qwen-mutation" / (datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + "-" + uuid4().hex[:8])
output.mkdir(parents=True)
model_dir = (root.parents[2] / ".cache/hf/models/checkpoint-path.txt").read_text(encoding="utf-8")
prefix = "python/p4hfadapter/models/qwen3_5_0_8b/"
cases = {
    "baseline": None,
    "coverage_guard_removed": (prefix + "configuration/__init__.py", "start != cursor or ", ""),
    "tokenizer_contract_removed": (prefix + "scenarios/__init__.py", ", return_dict=False", ""),
    "duplicate_guard_removed": (prefix + "state/__init__.py", "elif (issue, position) != (current.issue, current.position):", "elif False:"),
    "local_cache_index_removed": (prefix + "forward/__init__.py", "Qwen3_5DecoderLayer(self.config, local_idx)", "Qwen3_5DecoderLayer(self.config, global_idx)"),
}
files = sorted(p for name in ("python", "tests", "scripts", "plans", "scenarios", "manifests")
               for p in (root / name).rglob("*") if p.is_file() and p.suffix in (".py", ".json"))
digests = {p.relative_to(root).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest() for p in files}
records = []
for case, mutation in cases.items():
    snapshot = output / case
    for source in files:
        target = snapshot / source.relative_to(root)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)
    if mutation:
        relative, old, new = mutation
        target = snapshot / relative
        text = target.read_text(encoding="utf-8")
        if text.count(old) != 1:
            raise RuntimeError(f"mutation target drift: {relative}")
        target.write_text(text.replace(old, new), encoding="utf-8", newline="\n")
    if case in ("baseline", "tokenizer_contract_removed", "local_cache_index_removed"):
        plan = "balanced_two_gpu_fp32" if case == "local_cache_index_removed" else "single_gpu"
        command = [sys.executable, "-B", str(snapshot / "scripts/models/qwen3_5_0_8b/cli/run.py"), "verify",
                   "--model-dir", model_dir, "--plan", str(snapshot / f"plans/qwen3_5_0_8b/{plan}/plan.json"),
                   "--scenario", str(snapshot / "scenarios/qwen3_5_0_8b/short/scenario.json"),
                   "--output", str(snapshot / "result")]
    else:
        script = "scripts/verification/qwen_state/run.py" if case == "duplicate_guard_removed" else "scripts/testing/run.py"
        command = [sys.executable, "-B", str(snapshot / script)]
    print(f"START {case}", flush=True)
    env = dict(os.environ, PYTHONPATH=str(snapshot / "python"), PYTHONDONTWRITEBYTECODE="1", PYTHONUTF8="1")
    completed = subprocess.run(command, cwd=snapshot, env=env, capture_output=True, timeout=300)
    (snapshot / "test.log").write_bytes(completed.stdout + completed.stderr)
    expected = completed.returncode == (0 if case == "baseline" else 1)
    if case in ("coverage_guard_removed", "duplicate_guard_removed"):
        expected = expected and b"FAILED (" in completed.stderr
    elif case != "baseline":
        report = json.loads((snapshot / "result/summary.json").read_text(encoding="utf-8"))
        error = report.get("first_error", "")
        expected = expected and (("flat list" in error and not list((snapshot / "result").glob("*.stderr.log")))
                                 if case == "tokenizer_contract_removed" else "IndexError" in error)
    record = {"case": case, "exit_code": completed.returncode, "expected_result": bool(expected), "command": command}
    if mutation:
        record["mutation"] = {"path": mutation[0], "before": digests[mutation[0]],
                              "after": hashlib.sha256((snapshot / mutation[0]).read_bytes()).hexdigest()}
    records.append(record)
    print(json.dumps(record), flush=True)
    (output / "summary.json").write_text(json.dumps({"source_sha256": digests, "cases": records}, indent=2) + "\n", encoding="utf-8")
preserved = all(hashlib.sha256((root / name).read_bytes()).hexdigest() == value for name, value in digests.items())
print(f"source_preserved={preserved} output={output}", flush=True)
raise SystemExit(0 if preserved and all(x["expected_result"] for x in records) else 1)
