"""Run the native inspector without starting an inference service."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import struct
import subprocess


def main(args):
    out = args.output.resolve(); out.mkdir(parents=True, exist_ok=False)
    config = json.loads(args.config.read_text(encoding="utf-8-sig"))
    environment = os.environ.copy()
    environment.update(config.get("environment", []))
    plan = (config["plan"] + " --inspect-memory-plan").encode("utf-8")
    record = {"binary_sha256": hashlib.sha256(Path(config["binary"]).read_bytes()).hexdigest(),
              "inspector_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              "config_sha256": hashlib.sha256(args.config.read_bytes()).hexdigest(),
              "environment": config.get("environment", []), "expected_devices": args.expect_device,
              "native_plan": None, "error": None, "exit": None,
              "public_plan_acceptance": False, "release_acceptance": False}
    (out / "config.json").write_text(json.dumps(config, indent=2), encoding="utf-8")
    try:
        with (out / "stdout.log").open("wb") as stdout, (out / "stderr.log").open("wb") as stderr:
            result = subprocess.run([config["binary"], *config.get("args", []), "--port", "43290", "--bind", "127.0.0.1"],
                                    input=struct.pack("<I", len(plan)) + plan, stdout=stdout, stderr=stderr,
                                    timeout=args.timeout, env=environment)
        record["exit"] = result.returncode
        lines = [line[len(b"MEMORY_PLAN "):] for line in (out / "stderr.log").read_bytes().splitlines()
                 if line.startswith(b"MEMORY_PLAN ")]
        if len(lines) == 1:
            record["native_plan"] = json.loads(lines[0])
            observed = [e.get("description") for e in record["native_plan"].get("entries", [])
                        if e.get("scope") == "device"]
            if any(device not in observed for device in args.expect_device):
                record["error"] = "expected device not observed: " + repr(observed)
        else:
            record["error"] = "missing or duplicate native plan"
    except Exception as error:
        record["error"] = str(error)
    p = record["native_plan"]
    record["ok"] = record["exit"] == 0 and record["error"] is None and p is not None and p.get("complete") is True and p.get("fits_current_free") is True
    (out / "result.json").write_text(json.dumps(record, indent=2), encoding="utf-8")
    print(json.dumps({k: v for k, v in record.items() if k != "native_plan"}))
    return 0 if record["ok"] else 1


if __name__ == "__main__":
    p = argparse.ArgumentParser()
    p.add_argument("--config", type=Path, required=True)
    p.add_argument("--output", type=Path, required=True)
    p.add_argument("--timeout", type=int, default=300)
    p.add_argument("--expect-device", action="append", default=[], help="Exact native device description; repeat for multiple devices")
    raise SystemExit(main(p.parse_args()))
