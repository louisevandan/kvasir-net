#!/usr/bin/env python3
"""Run the complete local, non-model H0 v3 gate with fixed invocations."""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
from pathlib import Path


DIRECTORY = Path(__file__).resolve().parent
ROOT = DIRECTORY.parents[3]
SPEC = DIRECTORY / "benchmark-spec-qwen122b-h0-v3.json"


def run(argv: list[str]) -> subprocess.CompletedProcess[str]:
    completed = subprocess.run(
        argv, cwd=ROOT, text=True, encoding="utf-8",
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False,
    )
    if completed.returncode != 0:
        raise RuntimeError(json.dumps({
            "argv": argv, "exit_code": completed.returncode,
            "stdout": completed.stdout, "stderr": completed.stderr,
        }, ensure_ascii=False))
    return completed


def main() -> None:
    node = shutil.which("node")
    if node is None:
        raise RuntimeError("node executable is required")
    inspector = run([sys.executable, str(DIRECTORY / "test_inspect_h0_host.py")])
    if "Ran 4 tests" not in inspector.stderr or "OK" not in inspector.stderr:
        raise RuntimeError("host inspector test count differs")
    preflight = run([sys.executable, str(ROOT / "tools/tests/test_validate_event_runtime_preflight.py")])
    if "Ran 5 tests" not in preflight.stderr or "OK" not in preflight.stderr:
        raise RuntimeError("event preflight test count differs")
    prepare = run([sys.executable, str(DIRECTORY / "prepare-h1-quality.py"), "--self-test"])
    judge = run([sys.executable, str(DIRECTORY / "judge-h1-quality.py"), "--self-test"])
    if json.loads(prepare.stdout) != {"passed": True, "tests": 4}:
        raise RuntimeError("H1 materializer self-test summary differs")
    if json.loads(judge.stdout) != {"passed": True, "tests": 10}:
        raise RuntimeError("H1 judge self-test summary differs")
    run([node, "--test", str(DIRECTORY / "benchmark-spec.test.mjs")])
    verified = json.loads(run([
        node, str(DIRECTORY / "benchmark-spec.mjs"), str(SPEC),
    ]).stdout)
    expected = {
        "valid": True, "h0_status": "sealed", "load_authorized": True,
        "runtime_acceptance": False, "hosts": 3, "stages": 3, "corpus_requests": 64,
    }
    if verified != expected:
        raise RuntimeError("H0 v3 verifier summary differs")
    print(json.dumps({"passed": True, "checks": 6, "inspector_tests": 4,
                      "preflight_tests": 5,
                      "materializer_tests": 4, "judge_tests": 10,
                      "spec_tests": 4}, separators=(",", ":")))


if __name__ == "__main__":
    main()
