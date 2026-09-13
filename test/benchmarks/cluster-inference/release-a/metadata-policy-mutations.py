"""Verify that the metadata-only exception cannot widen to other engine code."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[4]
SCRIPTS = ROOT / "layers/adapters/llamacpp/staged/scripts/upstream"
CASES = {
    "baseline": None,
    "all_files_exempt": ('file.replaceAll("\\\\", "/") === "src/compat/model_support.cpp"', "true"),
    "engine_calls_allowed": ('if (call[1] !== "llama_model_meta_val_str")', "if (false)"),
}


def sha(data):
    return hashlib.sha256(data).hexdigest()


def main(output):
    output.mkdir(parents=True, exist_ok=False)
    records = []
    for name, change in CASES.items():
        case = output / name; case.mkdir()
        module = (SCRIPTS / "model-agnostic-boundary.mjs").read_text(encoding="utf-8")
        original = sha(module.encode())
        if change:
            assert module.count(change[0]) == 1
            module = module.replace(change[0], change[1])
        (case / "model-agnostic-boundary.mjs").write_text(module, encoding="utf-8", newline="\n")
        test = (SCRIPTS / "model-agnostic-boundary.test.mjs").read_bytes()
        (case / "model-agnostic-boundary.test.mjs").write_bytes(test)
        with (case / "test.log").open("wb") as log:
            result = subprocess.run(["node", "--test", "model-agnostic-boundary.test.mjs"], cwd=case,
                                    stdout=log, stderr=subprocess.STDOUT, timeout=60)
        log = (case / "test.log").read_text(encoding="utf-8", errors="replace")
        ok = result.returncode == (1 if change else 0) and (not change or "ERR_ASSERTION" in log)
        records.append({"case": name, "ok": ok, "exit": result.returncode, "before": original,
                        "after": sha(module.encode()), "test_sha256": sha(test),
                        "log_sha256": sha((case / "test.log").read_bytes())})
        (output / "summary.json").write_text(json.dumps(records, indent=2), encoding="utf-8")
        print(name, "PASS" if ok else "FAIL")
    return 0 if all(r["ok"] for r in records) else 1


if __name__ == "__main__":
    p = argparse.ArgumentParser(); p.add_argument("output", type=Path)
    raise SystemExit(main(p.parse_args().output.resolve()))
