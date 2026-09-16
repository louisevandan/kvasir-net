#!/usr/bin/env python3
"""Run the complete local, non-model H0 v4 gate with fixed invocations."""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
from pathlib import Path


DIRECTORY = Path(__file__).resolve().parent
ROOT = DIRECTORY.parents[3]
SPEC = DIRECTORY / "benchmark-spec-qwen122b-h0-v4.json"
INTEGRITY_SPEC = DIRECTORY / "integrity-test-spec-qwen122b-i0-v1.json"


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


def summary(argv: list[str], expected: dict, name: str) -> None:
    actual = json.loads(run(argv).stdout)
    if actual != expected:
        raise RuntimeError(f"{name} self-test summary differs: {actual!r}")


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
    summary([sys.executable, str(DIRECTORY / "prepare-h1-quality.py"), "--self-test"],
            {"passed": True, "tests": 4}, "H1 materializer")
    summary([sys.executable, str(DIRECTORY / "judge-h1-quality.py"), "--self-test"],
            {"passed": True, "tests": 10}, "H1 judge")
    summary([sys.executable, str(DIRECTORY / "validate-integrity-test-spec.py"), "--self-test"],
            {"passed": True, "tests": 23}, "integrity contract validator")
    summary([sys.executable, str(DIRECTORY / "validate-integrity-test-spec.py"),
             "--spec", str(INTEGRITY_SPEC)],
            {"passed": True, "arms": 14, "normal_arms": 8,
             "overload_arms": 1, "fault_arms": 5}, "integrity contract")
    summary([sys.executable, str(DIRECTORY / "prepare-integrity-i0.py"), "--self-test"],
            {"passed": True, "tests": 5}, "I0 materializer")
    summary([sys.executable, str(DIRECTORY / "inspect-i0-active-host.py"), "--self-test"],
            {"passed": True, "tests": 4}, "I0 active-host preflight")
    summary([sys.executable, str(DIRECTORY / "build-integrity-i0-evidence.py"),
             "--spec", str(INTEGRITY_SPEC), "--self-test"],
            {"passed": True, "tests": 10}, "I0 raw evidence builder")
    summary([sys.executable, str(DIRECTORY / "judge-integrity-i0.py"),
             "--spec", str(INTEGRITY_SPEC), "--self-test"],
            {"passed": True, "tests": 11}, "I0 raw judge")
    summary([sys.executable, str(DIRECTORY / "judge-integrity.py"),
             "--spec", str(INTEGRITY_SPEC), "--self-test"],
            {"passed": True, "tests": 27}, "integrity bundle judge")
    run([node, "--test", str(DIRECTORY / "benchmark-spec.test.mjs")])
    verified = json.loads(run([
        node, str(DIRECTORY / "benchmark-spec.mjs"), str(SPEC),
    ]).stdout)
    expected = {
        "valid": True, "h0_status": "sealed", "load_authorized": True,
        "runtime_acceptance": False, "hosts": 3, "stages": 3, "corpus_requests": 64,
    }
    if verified != expected:
        raise RuntimeError("H0 v4 verifier summary differs")
    print(json.dumps({"passed": True, "checks": 12, "inspector_tests": 4,
                      "preflight_tests": 5, "h1_materializer_tests": 4,
                      "h1_judge_tests": 10, "integrity_spec_tests": 23,
                      "integrity_arms": 14, "i0_materializer_tests": 5,
                      "i0_active_preflight_tests": 4, "i0_evidence_builder_tests": 10,
                      "i0_judge_tests": 11,
                      "integrity_judge_tests": 27,
                      "spec_tests": 4}, separators=(",", ":")))


if __name__ == "__main__":
    main()
