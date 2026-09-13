"""Exercise the actual native inspector on a host with distinct CUDA device orders."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys


def main(args):
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    source = Path(__file__).with_name("probe-memory-plan.py").read_text(encoding="utf-8")
    # Never mutate the working source or a native binary used by a measured arm.
    variants = {
        "baseline": source,
        "omit_environment": source.replace('    environment.update(config.get("environment", []))\n', '', 1),
        "omit_device_check": source.replace('if any(device not in observed for device in args.expect_device):', 'if False:', 1),
    }
    assert all(variants[k] != source for k in ("omit_environment", "omit_device_check"))
    for name, content in variants.items():
        (out / (name + ".py")).write_text(content, encoding="utf-8")
    environment = os.environ.copy()
    environment["CUDA_DEVICE_ORDER"] = "FASTEST_FIRST"
    cases = [
        ("baseline", "baseline", args.expected_device, 0),
        ("wrong_device", "baseline", args.alternate_device, 1),
        ("omit_environment", "omit_environment", args.expected_device, 1),
        ("omit_device_check", "omit_device_check", args.alternate_device, 0),
    ]
    results = []
    for name, variant, expected, exit_code in cases:
        script = out / (variant + ".py")
        p = subprocess.run([sys.executable, str(script), "--config", str(args.config.resolve()),
                            "--output", str(out / name), "--expect-device", expected],
                           env=environment, capture_output=True, timeout=330)
        (out / (name + ".stdout.log")).write_bytes(p.stdout)
        (out / (name + ".stderr.log")).write_bytes(p.stderr)
        record = json.loads((out / name / "result.json").read_text())
        observed = [e["description"] for e in record["native_plan"]["entries"] if e["scope"] == "device"]
        expected_actual = args.alternate_device if variant == "omit_environment" else args.expected_device
        ok = (p.returncode == exit_code and record["exit"] == 0 and expected_actual in observed
              and record["inspector_sha256"] == hashlib.sha256(script.read_bytes()).hexdigest())
        results.append(dict(case=name, exit=p.returncode, expected_exit=exit_code, observed=observed,
                            inspector_sha256=record["inspector_sha256"], binary_sha256=record["binary_sha256"], ok=ok))
    ok = all(r["ok"] for r in results) and len({r["binary_sha256"] for r in results}) == 1
    result = dict(ok=ok, mutations_detected=2 if ok else None, cases=results, release_acceptance=False)
    (out / "summary.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(json.dumps(result))
    return 0 if ok else 1


if __name__ == "__main__":
    p = argparse.ArgumentParser()
    p.add_argument("--config", type=Path, required=True)
    p.add_argument("--output", type=Path, required=True)
    p.add_argument("--expected-device", required=True)
    p.add_argument("--alternate-device", required=True)
    raise SystemExit(main(p.parse_args()))
