"""Sequential real-weight verification matrix for the checked-in local plans."""

from datetime import datetime, timezone
import json
import hashlib
from pathlib import Path
import subprocess
import sys
from uuid import uuid4

root = Path(__file__).resolve().parents[3]
output = root.parents[2] / "target/hf/qwen" / (datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + "-" + uuid4().hex[:8])
output.mkdir(parents=True)
source_paths = sorted((root / "python").rglob("*.py"))
source_hashes = {str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest() for p in source_paths}
(output / "source-manifest.json").write_text(json.dumps(source_hashes, indent=2) + "\n", encoding="utf-8")
cases = [(name, "short") for name in ("single_gpu", "balanced_two_gpu_fp32", "uneven_three_stage_fp32", "attention_boundaries_fp32", "cpu_gpu", "single_cpu")]
cases += [("balanced_two_gpu_fp32", name) for name in ("chunked_prefill", "interleaved_cancel")]
results = []
for plan, scenario in cases:
    name = f"{plan}--{scenario}"
    command = [sys.executable, "-B", str(root / "scripts/models/qwen3_5_0_8b/cli/run.py"), "verify",
               "--plan", str(root / f"plans/qwen3_5_0_8b/{plan}/plan.json"),
               "--scenario", str(root / f"scenarios/qwen3_5_0_8b/{scenario}/scenario.json"),
               "--output", str(output / name)]
    print(f"START {name}", flush=True)
    with (output / f"{name}.log").open("wb") as log:
        completed = subprocess.run(command, cwd=root, stdout=log, stderr=subprocess.STDOUT, timeout=600)
    report_path = output / name / "summary.json"
    report = json.loads(report_path.read_text(encoding="utf-8")) if report_path.exists() else {}
    unchanged = all(hashlib.sha256(p.read_bytes()).hexdigest() == source_hashes[str(p.relative_to(root))] for p in source_paths)
    record = {"case": name, "exit_code": completed.returncode, "ok": report.get("ok", False),
              "first_error": report.get("first_error"), "command": command, "source_unchanged": unchanged,
              "comparisons": len(report.get("comparisons", [])),
              "max_abs_logits": max((x["max_abs_logits"] for x in report.get("comparisons", [])), default=None)}
    results.append(record)
    print(json.dumps(record, ensure_ascii=False), flush=True)
    (output / "matrix.json").write_text(json.dumps(results, indent=2) + "\n", encoding="utf-8", newline="\n")
    if not record["ok"] or not unchanged:
        break
print(output, flush=True)
raise SystemExit(0 if len(results) == len(cases) and all(x["ok"] and x["exit_code"] == 0 and x["source_unchanged"] for x in results) else 1)
